//! Workspace client for the persistent local terminal runtime (ADR-038).
//!
//! Presentation deliberately reuses the management TUI's palette and layout
//! conventions (`super::BASE`/`ACCENT`/`BORDER`/…, hairline dividers, no
//! filled panels) so switching between the workspace and management
//! contexts with Ctrl+O reads as one product, not two.

use super::tui_application;
use crate::ui::extension_host::WorkspaceHost;
use crate::ui::root_picker::RootPicker;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::{self, BufReader},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use uze_application::AgentIdentity;
use uze_application::{
    DeliveryOutcome, DeliveryReport, Evaluation, TaskStateView, TaskView, UpstreamSync,
};
use uze_application::{Result, UzeError, UzeHome};
use uze_extensions::{
    ExtensionHit, git_diff,
    view::{ScrollDirection, ViewHit},
};
use uze_terminal::{
    CellAttributes, ClientEvent, ClientRequest, Cursor, PROTOCOL_VERSION, PaneDamage, PaneId,
    PaneSnapshot, RenderCell, Session, Space, SpaceId, Tab, TabId, TerminalColor, attach,
    read_event, send_request,
};

/// Input/redraw cadence. Unlike the pane content itself — which the server
/// now pushes on PTY output instead of the client polling for it (see
/// ADR-038 follow-up: the previous per-frame `Refresh` request serialized
/// every cell of every pane up to 60x/sec regardless of activity, which is
/// what made typing feel like it hung under any real system load) — this
/// timeout only bounds keyboard/mouse latency.
const POLL: Duration = Duration::from_millis(16);

/// Git inspection runs locally but still launches a process. Refresh often
/// enough to follow commands typed in the active pane without attaching that
/// cost to every PTY damage redraw.
const GIT_BADGE_REFRESH: Duration = Duration::from_millis(750);

/// The same frames configure the hidden `indicatif` spinner that schedules
/// this animation. Ratatui owns the alternate screen, so it paints the frame
/// instead of letting indicatif write to stderr.
const AGENT_ACTIVITY_FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
const AGENT_ACTIVITY_TICK: Duration = Duration::from_millis(120);
/// How long an agent pane must stay quiet before its work reads as
/// finished. Agent harnesses animate while they work — a spinner, an
/// elapsed-token counter — so a pane that is genuinely busy keeps emitting
/// damage well inside this window even across a slow tool call, and a pane
/// that stops emitting has stopped working. Long enough to ride out a
/// harness that redraws less eagerly, short enough that "done" arrives
/// while the user still cares; unlike the previous 10s window, guessing
/// low is no longer terminal because renewed output re-enters `Working`
/// on its own (see [`WorkspaceModel::note_agent_output`]).
const AGENT_QUIET_AFTER: Duration = Duration::from_secs(3);

/// How much repainting a pane must do before it reads as an agent at
/// work. A harness running a turn *animates* — a spinner, an elapsed
/// counter — so it repaints many times a second; an idle one still
/// repaints, just sporadically: a status line, a rotating hint, a pasted
/// image being laid out, the full redraw a reattach provokes. Counting
/// frames inside one short window is what separates the two without
/// reading vendor-specific pixels. Treating any single repaint as work
/// is what left merely-open agents spinning forever.
///
/// The frames must also be *spread* across [`AGENT_BUSY_SPAN`], not just
/// numerous: one repaint whose bytes reach the client in several chunks
/// arrives as several damage events a few milliseconds apart, and counting
/// that as animation would put every reattach, resize and pasted image
/// straight back on the spinner.
const AGENT_BUSY_WINDOW: Duration = Duration::from_millis(1000);
const AGENT_BUSY_SPAN: Duration = Duration::from_millis(300);
const AGENT_BUSY_REPAINTS: usize = 5;

/// A pane echoes what the user types or pastes, and that echo is damage
/// like any other. Damage inside the window that input opens is treated
/// as that echo, not as the agent working — otherwise composing a prompt,
/// or dropping an image into one, would light the sidebar up as busy.
/// Enter is exempt: it marks the pane busy explicitly. A paste gets the
/// longer window because the harness reflows its whole prompt box around
/// the pasted content, well after the bytes themselves landed.
const AGENT_ECHO_GRACE: Duration = Duration::from_millis(150);
const AGENT_PASTE_GRACE: Duration = Duration::from_millis(750);

/// A pane the client just resized — on attach, on a tab switch, on a
/// terminal resize — redraws because we asked it to, and that redraw is
/// not the agent working either. Long enough for a harness to repaint a
/// full screen, short enough that a turn genuinely running through the
/// resize is only briefly understated.
const AGENT_REDRAW_GRACE: Duration = Duration::from_millis(1000);

mod input;
mod render;
use input::*;
use render::*;

pub(crate) enum WorkspaceExit {
    Management,
    Quit,
}

/// Which harness, resolved against which directory. Both halves are the
/// identity of one support resolution: the same agent open in two panes
/// sitting in two different projects has two different answers, and a
/// resolution computed for one must never be shown for the other. This is
/// the whole reason the old session-wide "resolve once at the attach root"
/// read was wrong.
type SupportKey = (String, PathBuf);

/// A finished resolution, tagged with the key it answers. `support` is
/// `None` when the read failed outright — kept as a resolved-but-empty
/// answer rather than dropped, so a failing read cannot spin the refresh
/// loop by looking forever unresolved.
struct SupportResolution {
    key: SupportKey,
    support: Option<super::agent_support::AgentSupport>,
}

/// Computes one agent's support read model in a background thread and
/// delivers it through `sender`.
///
/// Everything about the answer comes from `key`: the harness the pane is
/// actually running, and that pane's own working directory. The
/// application resolves the project from there
/// (`UzeApplication::agent_context_for`), the same way the runtime shim
/// resolves it when it execs the harness from that directory — so the
/// popup reports the delivery a launch here would really perform, not the
/// one that would have happened wherever `uze` itself was started.
fn spawn_support_refresh(home: &UzeHome, key: SupportKey, sender: mpsc::Sender<SupportResolution>) {
    let support_home = home.clone();
    thread::spawn(move || {
        let support = super::tui_application(support_home).ok().and_then(|app| {
            let context = app.workspace().agent_context_for(&key.0, &key.1).ok()?;
            let health = app.health().harness(&key.0).ok()?;
            let profiles = app.profiles().list().unwrap_or_default();
            let active_profile = profiles.iter().find(|profile| profile.active);
            Some(super::agent_support::AgentSupport::resolve(
                health,
                &context,
                active_profile,
            ))
        });
        let _ = sender.send(SupportResolution { key, support });
    });
}

/// How often every visible repository's tasks are re-read even when no
/// pane went quiet — a task delivered from another client, a branch
/// integrated by hand, a checkout removed.
const TASK_REFRESH: Duration = Duration::from_secs(20);
/// How long a one-line notice stays on screen.
const NOTICE_TTL: Duration = Duration::from_secs(6);

/// What a background evaluation answered.
struct TaskResolution {
    /// The key [`WorkspaceModel::schedule_evaluation`] reserved, released
    /// on arrival whatever the answer was. It travels with the request
    /// because the two ends resolve a repository differently — the
    /// scheduler lexically, off the path it already holds, the evaluation
    /// by asking Git — and a key removed under the second spelling never
    /// matches the one inserted under the first, which leaves that
    /// directory reserved for the life of the session and its status
    /// frozen at whatever it last read.
    key: PathBuf,
    /// What the directory's repository holds, or `None` when the
    /// directory turned out not to be a Git working tree.
    answered: Option<EvaluationAnswer>,
}

/// One evaluated directory: the repository its tasks hang off, the branch
/// checked out at the directory the evaluation is *keyed* under (and,
/// when that branch is the delivery target, how it stands against its
/// upstream), and what the repository now holds. The key, not the `cwd`
/// that asked: a slot is keyed by its primary, and a slot's pane going
/// quiet must not write the slot's own branch where an agent outside any
/// slot — the one case the sidebar has no task to read a branch from —
/// then reads the primary's.
struct EvaluationAnswer {
    primary: PathBuf,
    branch: Option<String>,
    sync: Option<UpstreamSync>,
    evaluation: Evaluation,
}

/// What a background delivery answered.
struct DeliveryResolution {
    cwd: PathBuf,
    reports: Vec<DeliveryReport>,
}

/// Open state of the preserved-work list: tasks holding work that no live
/// tab is in front of.
struct PreservedOverlay {
    selected: usize,
    /// A discard was asked for and waits for its confirmation.
    confirm_discard: bool,
}

/// What an evaluation of `cwd` is reserved under, so two panes of one
/// repository do not both pay for the same answer.
///
/// Lexical on purpose: this runs on the UI thread, and the repository a
/// path belongs to is only knowable for certain by asking Git — which is
/// the work being deferred. Every slot of a repository resolves to that
/// repository, which is the case the sidebar is full of; a directory that
/// is not a slot answers itself, so two subdirectories of one primary pay
/// twice. Coarse, never wrong.
fn evaluation_key(cwd: &Path) -> PathBuf {
    uze_application::isolated_checkout(cwd)
        .map(|checkout| checkout.primary.to_path_buf())
        .unwrap_or_else(|| cwd.to_path_buf())
}

/// Re-reads the tasks of the repository `cwd` belongs to, off the UI
/// thread: every evaluation asks Git, and a delivery may run a gate.
fn spawn_task_evaluation(
    home: &UzeHome,
    key: PathBuf,
    cwd: PathBuf,
    sender: mpsc::Sender<TaskResolution>,
) {
    let home = home.clone();
    thread::spawn(move || {
        // Every path out of here answers, including the ones that found
        // nothing: a request that returns in silence never releases its
        // key, and the directory is then never evaluated again.
        let answered = tui_application(home).ok().and_then(|app| {
            let workspace = app.workspace();
            Some(EvaluationAnswer {
                primary: workspace.primary_of(&cwd)?,
                branch: workspace.current_branch(&key),
                sync: workspace.target_upstream_sync(&key),
                evaluation: workspace.evaluate_tasks(&cwd),
            })
        });
        let _ = sender.send(TaskResolution { key, answered });
    });
}

/// Delivers one task, or every ready one when `task` is `None`.
fn spawn_delivery(
    home: &UzeHome,
    cwd: PathBuf,
    task: Option<String>,
    sender: mpsc::Sender<DeliveryResolution>,
) {
    let home = home.clone();
    thread::spawn(move || {
        let Ok(app) = tui_application(home) else {
            return;
        };
        let reports = match task {
            Some(task) => app
                .workspace()
                .deliver_task(&cwd, &task)
                .into_iter()
                .collect(),
            None => app.workspace().deliver_ready(&cwd),
        };
        let _ = sender.send(DeliveryResolution { cwd, reports });
    });
}

/// The one line a delivery leaves on screen — no label of its own: whoever
/// shows it decides whether the task it is about still needs naming (see
/// `WorkspaceModel::notice_for_tab`/`notice_for_footer`).
fn describe_delivery(report: &DeliveryReport) -> String {
    match &report.outcome {
        DeliveryOutcome::Handoff => format!("ready on {}", report.task.branch),
        DeliveryOutcome::Merged => format!("merged into {}", report.task.target),
        DeliveryOutcome::Published { branch, request } => match request {
            Some(url) => url.clone(),
            None => format!("pushed as {branch}"),
        },
        DeliveryOutcome::Refused(reason) => format!("not delivered — {reason}"),
        DeliveryOutcome::ReturnedToAgent(_) => "returned to its agent to resolve".to_owned(),
    }
}

pub(crate) fn attach_workspace(
    terminal: &mut super::TerminalSession,
    root: &Path,
    sidebar_width: &mut Option<u16>,
    memory: &mut WorkspaceMemory,
    home: &UzeHome,
    pending_tab: Option<TabId>,
) -> Result<WorkspaceExit> {
    // The handshake below must ship the real terminal size: it sizes the PTY
    // used for the session's *already-selected* pane (e.g. a tab restored
    // from a prior attach), and the per-frame resize further down only
    // corrects the size actually visible in that loop's compute_layout call.
    // A placeholder here previously left a stale-selected pane pinned to a
    // wrong fixed size until something happened to trigger a fresh resize.
    // `*sidebar_width` carries over whatever the user last dragged it to —
    // in this mode or the management one, they share the one value (see
    // `super::run`) — so the pane starts at its real width immediately
    // instead of assuming the sidebar's responsive default.
    let size = terminal.size()?;
    let layout = compute_layout(Rect::new(0, 0, size.width, size.height), *sidebar_width);
    let (columns, rows) = (layout.pane.width, layout.pane.height);

    // One server per user; what the launch directory decides is which
    // space this client lands in. Resolving the workspace root *before*
    // attaching is what makes a repository and a subdirectory of it the
    // same space rather than two.
    let workspace_root = uze_application::space_root(root);
    let mut stream = attach(&workspace_root, columns, rows).map_err(runtime_error)?;
    let read_stream = stream.try_clone().map_err(io_error)?;
    send_request(
        &mut stream,
        &ClientRequest::Attach {
            version: PROTOCOL_VERSION,
            workspace: uze_terminal::WorkspaceId("client".into()),
            columns,
            rows,
            root: Some(workspace_root.clone()),
        },
    )
    .map_err(runtime_error)?;
    let (events, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(read_stream);
        while let Ok(Some(event)) = read_event(&mut reader) {
            if events.send(event).is_err() {
                break;
            }
        }
    });
    // Keyed on the root of the space the prompt was typed in — the same
    // answer the management Overview reads against. Writes run on their
    // own thread so a keystroke never waits on the filesystem, and the
    // thread ends when `model` drops its sender at the end of this attach.
    let (prompt_recorder, recorded_prompts) =
        mpsc::channel::<(PathBuf, uze_application::PromptOrigin, String)>();
    thread::spawn({
        let home = home.clone();
        move || {
            while let Ok((root, origin, prompt)) = recorded_prompts.recv() {
                let _ = tui_application(home.clone())
                    .and_then(|app| app.workspace().record_prompt(&root, &origin, &prompt));
            }
        }
    });
    let mut model = WorkspaceModel {
        dirty: true,
        last_size: (columns, rows),
        sidebar_width: *sidebar_width,
        prompt_recorder: Some(prompt_recorder),
        ..WorkspaceModel::recall(std::mem::take(&mut memory.remembered))
    };
    // A registered harness set doesn't change mid-session, so this is built
    // once per attach.
    let identities = agent_identities(home);
    // This is the exact `HarnessHealth` read model used by the Integrations
    // screen. It loads asynchronously so inspecting support never delays a
    // live terminal attach or a pane redraw. Unlike `identities` above, this
    // one *does* go stale — `AGENTS.md`/the runtime projection can change
    // underneath an open workspace (another session writing it, a race in
    // `claude_runtime_projection` resolving) — so `OpenAgentSupport` below
    // fires a fresh one on every open rather than trusting this attach-time
    // snapshot for the rest of the session.
    // No attach-time prefetch: there is nothing to resolve until a tab is
    // recognized as running an agent, and what to resolve then depends on
    // that pane's own directory. The loop below kicks a refresh the moment
    // the selection names an agent whose answer is not already in hand —
    // the same moment the "✦" badge appears.
    // The answer channels outlive this attach with the rest of the memory:
    // a read still running when the user leaves for management lands after
    // they come back, instead of vanishing with a receiver that was dropped
    // and leaving its key reserved forever.
    let support_sender = memory.support.sender.clone();
    let support_receiver = &memory.support.receiver;
    let task_sender = memory.tasks.sender.clone();
    let task_receiver = &memory.tasks.receiver;
    let delivery_sender = memory.deliveries.sender.clone();
    let delivery_receiver = &memory.deliveries.receiver;
    let activity_spinner = ProgressBar::new_spinner();
    activity_spinner.set_draw_target(ProgressDrawTarget::hidden());
    activity_spinner
        .set_style(ProgressStyle::default_spinner().tick_strings(&AGENT_ACTIVITY_FRAMES));
    let mut next_activity_tick = Instant::now();
    // The server's session/pane state is persistent — reattaching after a
    // Ctrl+O round trip to management finds the same shells exactly as they
    // were left. But the client's view of that session and its panes always
    // starts empty (only what it resolved on its own carries over, see
    // `WorkspaceMemory`), so without this wait the very first frame renders
    // before the server's initial `Attached`/`Snapshot` reply lands,
    // flashing the "starting shell…" placeholder and repainting the whole
    // pane a moment later — reading as a lost/reset session even though
    // nothing server-side ever was. A
    // generous timeout is still a safety net, not the expected path: this
    // is a local Unix socket round trip, normally sub-millisecond, and the
    // shared `TerminalSession` (see `super::TerminalSession`) is already
    // showing the same open alternate screen management just used, so this
    // blocks inside a continuously open uze, not a flash back to the shell.
    while model.session.is_none() || model.panes.is_empty() {
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(event) => model.apply(event, &identities),
            Err(_) => break,
        }
    }
    // Attaching makes the whole workspace repaint: the client has no
    // baseline to diff against, and the panes it inherits are mid-screen.
    // None of that is an agent starting a turn, which is what made every
    // open agent spin for a few seconds each time uze was reopened.
    model.note_attach_redraw();
    // `select_tab` moves the selected space too when the tab lives in
    // another one, so the space needs no separate request. A tab closed
    // since its prompt was logged simply selects nothing.
    if let Some(tab) = pending_tab {
        let _ = send_request(&mut stream, &ClientRequest::SelectTab { tab });
        // Resize the newly selected pane to the current layout size, same as
        // the manual SelectTab handler does, so the PTY size matches the
        // visible rect immediately.
        if let Some(pane) = model.pane_for_tab(tab) {
            resize_pane(&mut stream, &mut model, pane, columns, rows);
        }
    }
    // Every way out of the loop — Ctrl+O, Ctrl+Q, an error — must hand the
    // model's memory back, so the loop runs inside one call whose result is
    // read only after that handover.
    let outcome: Result<WorkspaceExit> = (|| loop {
        while let Ok(event) = receiver.try_recv() {
            model.apply(event, &identities);
        }
        for request in adopt_agent_labels(&mut model, &identities) {
            let _ = send_request(&mut stream, &request);
        }
        sync_slot_occupancy(&mut model, home, &task_sender);
        while let Ok(resolution) = support_receiver.try_recv() {
            if model.agent_support_pending.as_ref() == Some(&resolution.key) {
                model.agent_support_pending = None;
            }
            model.agent_support = Some(resolution);
            model.dirty = true;
        }
        while let Ok(resolution) = task_receiver.try_recv() {
            model.task_eval_pending.remove(&resolution.key);
            let Some(EvaluationAnswer {
                primary,
                branch,
                sync,
                evaluation,
            }) = resolution.answered
            else {
                continue;
            };
            match branch {
                Some(branch) => model.branches.insert(resolution.key.clone(), branch),
                None => model.branches.remove(&resolution.key),
            };
            match sync {
                Some(sync) => model.upstream_syncs.insert(resolution.key, sync),
                None => model.upstream_syncs.remove(&resolution.key),
            };
            model.tasks.insert(primary, evaluation.tasks);
            // A conflict found while a clean task followed the target is
            // the agent's to resolve: the message goes into its pane, as
            // one submission.
            for notice in evaluation.notices {
                if let Some(pane) = model.pane_for_checkout(&notice.checkout) {
                    let mut bytes = notice.message.into_bytes();
                    bytes.push(b'\r');
                    let _ = send_request(&mut stream, &ClientRequest::Input { pane, bytes });
                }
            }
            model.dirty = true;
        }
        while let Ok(resolution) = delivery_receiver.try_recv() {
            for report in &resolution.reports {
                model.delivery_pending.remove(&report.task.id);
                model.set_task_notice(
                    &report.task.id,
                    &report.task.label,
                    describe_delivery(report),
                );
                if let DeliveryOutcome::ReturnedToAgent(notice) = &report.outcome
                    && let Some(pane) = model.pane_for_checkout(&notice.checkout)
                {
                    let mut bytes = notice.message.clone().into_bytes();
                    bytes.push(b'\r');
                    let _ = send_request(&mut stream, &ClientRequest::Input { pane, bytes });
                }
            }
            if resolution.reports.is_empty() {
                model.set_notice("nothing ready to deliver".to_owned());
            }
            model.schedule_evaluation(home, resolution.cwd, &task_sender);
            model.dirty = true;
        }
        // Readiness is a Git fact, read when a pane goes quiet and, less
        // often, on a clock — never told by the agent.
        let quiet_panes = std::mem::take(&mut model.recently_quiet);
        let quiet: Vec<PathBuf> = quiet_panes
            .into_iter()
            .filter_map(|pane| model.pane_cwd(pane))
            .collect();
        for cwd in quiet {
            model.schedule_evaluation(home, cwd, &task_sender);
        }
        if model
            .last_task_refresh
            .is_none_or(|last| last.elapsed() >= TASK_REFRESH)
        {
            model.last_task_refresh = Some(Instant::now());
            if let Some(cwd) = selected_pane_cwd(&model) {
                model.schedule_evaluation(home, cwd, &task_sender);
            }
        }
        if model
            .notice
            .as_ref()
            .is_some_and(|notice| notice.since.elapsed() >= NOTICE_TTL)
        {
            model.notice = None;
            model.dirty = true;
        }
        // Contextual resolution: whatever the selection currently is, that
        // is what must be resolved. Keyed on `(harness, cwd)`, so this
        // fires exactly when the answer could have changed — a different
        // agent tab selected, or the server's live probe reporting the
        // pane moved — and never repeats for an answer already held.
        if let Some(key) = selected_agent_context(&model, &identities)
            && model.agent_support_pending.as_ref() != Some(&key)
            && model
                .agent_support
                .as_ref()
                .is_none_or(|resolution| resolution.key != key)
        {
            model.agent_support_pending = Some(key.clone());
            spawn_support_refresh(home, key, support_sender.clone());
        }
        if let Some(view) = model.git_view.as_mut()
            && view.refresh_due()
        {
            view.refresh(&WorkspaceHost);
            model.dirty = true;
        }
        if model.expire_agent_activity(Instant::now()) {
            model.dirty = true;
        }
        if workspace_has_active_agent_operation(&model, &identities) {
            let now = Instant::now();
            if now >= next_activity_tick {
                activity_spinner.inc(1);
                model.tick = activity_spinner.position() as usize;
                next_activity_tick = now + AGENT_ACTIVITY_TICK;
                model.dirty = true;
            }
        }
        let size = terminal.size()?;
        let layout = compute_layout(
            Rect::new(0, 0, size.width, size.height),
            model.sidebar_width,
        );
        let (columns, rows) = (layout.pane.width, layout.pane.height);
        if (columns, rows) != model.last_size {
            model.last_size = (columns, rows);
            model.dirty = true;
            let focused = model.focused_pane();
            resize_pane(&mut stream, &mut model, focused, columns, rows);
        }
        if model.dirty {
            model.refresh_git_badge();
            let mut hits = Vec::new();
            terminal.draw(|frame| render(frame, &model, &identities, &mut hits))?;
            model.hits = hits;
            model.dirty = false;
        }
        if event::poll(POLL).map_err(io_error)? {
            match event::read().map_err(io_error)? {
                Event::Key(key) if model.root_picker.is_some() => {
                    match key.code {
                        KeyCode::Up => {
                            if let Some(picker) = model.root_picker.as_mut() {
                                picker.move_selection(-1);
                            }
                        }
                        KeyCode::Down => {
                            if let Some(picker) = model.root_picker.as_mut() {
                                picker.move_selection(1);
                            }
                        }
                        // Tab walks into the highlighted directory, so a
                        // root several levels down is reached by narrowing
                        // one level at a time instead of typing the path.
                        KeyCode::Tab => {
                            if let Some(picker) = model.root_picker.as_mut() {
                                picker.descend();
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(root) =
                                model.root_picker.as_ref().and_then(RootPicker::chosen)
                            {
                                model.root_picker = None;
                                let _ = send_request(
                                    &mut stream,
                                    &ClientRequest::CreateSpace {
                                        label: None,
                                        root,
                                        columns,
                                        rows,
                                    },
                                );
                            }
                        }
                        KeyCode::Esc => model.root_picker = None,
                        KeyCode::Backspace => {
                            if let Some(picker) = model.root_picker.as_mut() {
                                picker.backspace();
                            }
                        }
                        KeyCode::Char(character) => {
                            if let Some(picker) = model.root_picker.as_mut() {
                                picker.typed(character);
                            }
                        }
                        _ => {}
                    }
                    model.dirty = true;
                }
                Event::Paste(text) if model.root_picker.is_some() => {
                    if let Some(picker) = model.root_picker.as_mut() {
                        picker.pasted(text.trim_end_matches(['\r', '\n']));
                    }
                    model.dirty = true;
                }
                Event::Key(key) if model.renaming.is_some() => {
                    match key.code {
                        KeyCode::Enter => {
                            if let Some((target, buffer)) = model.renaming.take() {
                                let trimmed = buffer.trim().to_owned();
                                if !trimmed.is_empty() {
                                    let _ = send_request(
                                        &mut stream,
                                        &match target {
                                            RenameTarget::Tab(tab) => ClientRequest::RenameTab {
                                                tab,
                                                label: trimmed,
                                            },
                                            RenameTarget::Space(space) => {
                                                ClientRequest::RenameSpace {
                                                    space,
                                                    label: trimmed,
                                                }
                                            }
                                        },
                                    );
                                }
                            }
                        }
                        KeyCode::Esc => model.renaming = None,
                        KeyCode::Backspace => {
                            if let Some((_, buffer)) = model.renaming.as_mut() {
                                buffer.pop();
                            }
                        }
                        KeyCode::Char(c) => {
                            if let Some((_, buffer)) = model.renaming.as_mut() {
                                buffer.push(c);
                            }
                        }
                        _ => {}
                    }
                    model.dirty = true;
                }
                Event::Paste(text) if model.renaming.is_some() => {
                    if let Some((_, buffer)) = model.renaming.as_mut() {
                        buffer.push_str(text.trim_end_matches(['\r', '\n']));
                    }
                    model.dirty = true;
                }
                Event::Key(key) if model.agent_picker.is_some() => {
                    match key.code {
                        KeyCode::Up => {
                            if let Some(picker) = model.agent_picker.as_mut() {
                                picker.selected = picker.selected.saturating_sub(1);
                            }
                        }
                        KeyCode::Down => {
                            if let Some(picker) = model.agent_picker.as_mut() {
                                picker.selected = (picker.selected + 1)
                                    .min(picker.options.len().saturating_sub(1));
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(picker) = model.agent_picker.take()
                                && let Some(option) = picker.options.get(picker.selected)
                            {
                                let label = next_agent_label(&model);
                                let cwd = picker
                                    .cwd
                                    .clone()
                                    .or_else(|| agent_launch_cwd(&model, home));
                                if let Some(cwd) = cwd.clone() {
                                    model.schedule_evaluation(home, cwd, &task_sender);
                                }
                                let _ = send_request(
                                    &mut stream,
                                    &ClientRequest::CreateTab {
                                        cwd,
                                        label,
                                        agent: None,
                                        columns,
                                        rows,
                                        command: Some(option.command.clone()),
                                    },
                                );
                            }
                        }
                        // Esc, or anything else — the picker only reacts to
                        // Up/Down/Enter, so any other key just dismisses it
                        // rather than leaking through to the pane.
                        _ => model.agent_picker = None,
                    }
                    model.dirty = true;
                }
                Event::Key(key) if model.preserved.is_some() => {
                    let preserved = model.preserved_tasks();
                    let overlay = model.preserved.as_mut().expect("guarded");
                    match key.code {
                        KeyCode::Esc => model.preserved = None,
                        KeyCode::Up => {
                            overlay.selected = overlay.selected.saturating_sub(1);
                            overlay.confirm_discard = false;
                        }
                        KeyCode::Down => {
                            overlay.selected =
                                (overlay.selected + 1).min(preserved.len().saturating_sub(1));
                            overlay.confirm_discard = false;
                        }
                        KeyCode::Char('i') => {
                            if let Some((cwd, task)) = preserved.get(overlay.selected) {
                                model.delivery_pending.insert(task.id.clone());
                                spawn_delivery(
                                    home,
                                    cwd.clone(),
                                    Some(task.id.clone()),
                                    delivery_sender.clone(),
                                );
                            }
                        }
                        KeyCode::Char('f') => {
                            if let Some((cwd, task)) = preserved.get(overlay.selected)
                                && let Ok(app) = tui_application(home.clone())
                            {
                                let _ = app.workspace().finish_task(cwd, &task.id);
                                model.schedule_evaluation(home, cwd.clone(), &task_sender);
                            }
                        }
                        KeyCode::Char('r') => {
                            if let Some((_, task)) = preserved.get(overlay.selected)
                                && let Some(checkout) = task.checkout.clone()
                            {
                                model.preserved = None;
                                model.agent_picker = Some(AgentPicker {
                                    options: agent_options(home),
                                    selected: 0,
                                    anchor: Rect::default(),
                                    cwd: Some(checkout),
                                });
                            }
                        }
                        // Discard is the one action that deletes work, so
                        // it is the one that asks twice.
                        KeyCode::Char('d') => overlay.confirm_discard = true,
                        KeyCode::Char('y') if overlay.confirm_discard => {
                            overlay.confirm_discard = false;
                            if let Some((cwd, task)) = preserved.get(overlay.selected)
                                && let Ok(app) = tui_application(home.clone())
                            {
                                match app.workspace().discard_task(cwd, &task.id) {
                                    Ok(()) => {
                                        model.set_notice(format!("{}: discarded", task.label));
                                    }
                                    Err(error) => model.set_notice(error.to_string()),
                                }
                                model.schedule_evaluation(home, cwd.clone(), &task_sender);
                            }
                        }
                        _ => overlay.confirm_discard = false,
                    }
                    model.dirty = true;
                }
                Event::Key(_) if model.support_dropdown.is_some() => {
                    model.support_dropdown = None;
                    model.dirty = true;
                }
                Event::Key(_) if model.status_catalog.is_some() => {
                    model.status_catalog = None;
                    model.dirty = true;
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && key.code == KeyCode::Char('i') =>
                {
                    deliver_selected_tab(&mut model, home, &delivery_sender);
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && key.code == KeyCode::Char('I') =>
                {
                    if let Some(cwd) = selected_pane_cwd(&model) {
                        spawn_delivery(home, cwd, None, delivery_sender.clone());
                        model.set_notice("delivering every ready task…".to_owned());
                    }
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && key.code == KeyCode::Char('p') =>
                {
                    model.preserved = match model.preserved {
                        Some(_) => None,
                        None => Some(PreservedOverlay {
                            selected: 0,
                            confirm_discard: false,
                        }),
                    };
                    model.dirty = true;
                }
                Event::Key(key) if model.context_menu.is_some() => {
                    // Up/Down move the selection; Enter confirms whichever
                    // row is selected; anything else (Esc included)
                    // dismisses without acting — same "only reacts to its
                    // own actions" rule the agent picker above uses.
                    match key.code {
                        KeyCode::Up => {
                            if let Some(menu) = model.context_menu.as_mut() {
                                menu.selected = menu.selected.saturating_sub(1);
                            }
                        }
                        KeyCode::Down => {
                            if let Some(menu) = model.context_menu.as_mut() {
                                menu.selected =
                                    (menu.selected + 1).min(menu.items.len().saturating_sub(1));
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(menu) = model.context_menu.take()
                                && let Some(action) = menu.items.get(menu.selected).copied()
                            {
                                dispatch_menu_action(
                                    &mut stream,
                                    &mut model,
                                    &identities,
                                    menu.target,
                                    action,
                                );
                            }
                        }
                        _ => model.context_menu = None,
                    }
                    model.dirty = true;
                }
                Event::Key(key) if model.git_view.is_some() => {
                    if let Some(view) = model.git_view.as_mut()
                        && matches!(
                            git_diff::handle_key(&WorkspaceHost, view, key),
                            git_diff::GitViewOutcome::Close
                        )
                    {
                        model.git_view = None;
                    }
                    model.dirty = true;
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('o') =>
                {
                    let _ = send_request(&mut stream, &ClientRequest::Detach);
                    return Ok(WorkspaceExit::Management);
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('q') =>
                {
                    let _ = send_request(&mut stream, &ClientRequest::Detach);
                    return Ok(WorkspaceExit::Quit);
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('t') =>
                {
                    let _ = send_request(
                        &mut stream,
                        &ClientRequest::CreateTab {
                            label: next_shell_label(&model, &identities),
                            agent: context_agent(&model, &identities),
                            columns,
                            rows,
                            cwd: new_shell_cwd(&model, &identities),
                            command: None,
                        },
                    );
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('w') =>
                {
                    if let Some(tab) = model.selected_tab() {
                        let _ = send_request(&mut stream, &ClientRequest::CloseTab { tab });
                    }
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('g') =>
                {
                    open_git_view(&mut model);
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && matches!(key.code, KeyCode::Char('1'..='9')) =>
                {
                    let index = match key.code {
                        KeyCode::Char(value) => value as usize - '1' as usize,
                        _ => 0,
                    };
                    if let Some(tab) = model
                        .session
                        .as_ref()
                        .and_then(|session| session.selected_space().tabs.get(index))
                        .map(|tab| tab.id)
                    {
                        model.acknowledge_completed_agent_tab(tab);
                        let _ = send_request(&mut stream, &ClientRequest::SelectTab { tab });
                        if let Some(pane) = model.pane_for_tab(tab) {
                            resize_pane(&mut stream, &mut model, pane, columns, rows);
                        }
                    }
                }
                Event::Key(key) => {
                    if let Some(bytes) = encode_key(key) {
                        let pane = model.focused_pane();
                        // `encode_key` emits a bare CR for Enter and 0x03
                        // for Ctrl+C, so these are exact byte comparisons
                        // rather than a substring scan that a pasted or
                        // multi-byte sequence could trip.
                        let submitted = bytes.as_slice() == *b"\r";
                        let cancelled = bytes.as_slice() == [3u8];
                        let prompt = if submitted {
                            model.prompt_buffers.entry(pane).or_default().submit()
                        } else {
                            if cancelled {
                                model.prompt_buffers.remove(&pane);
                            } else {
                                model.prompt_buffers.entry(pane).or_default().apply(key);
                            }
                            None
                        };
                        // Forwarded before anything is recorded: the pane's
                        // own responsiveness must never wait on history.
                        let _ = send_request(&mut stream, &ClientRequest::Input { pane, bytes });
                        model.note_pane_input(pane);
                        if submitted {
                            model.note_agent_prompt_submission(
                                pane,
                                &identities,
                                prompt.as_deref(),
                            );
                        }
                    }
                }
                Event::Paste(text) if model.no_modal_open() => {
                    let pane = model.focused_pane();
                    model.prompt_buffers.entry(pane).or_default().paste(&text);
                    forward_paste(&mut stream, &model, &text);
                    model.note_pane_paste(pane);
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && model.renaming.is_some() =>
                {
                    // Same rule the management TUI's overlays use: a click
                    // outside the thing being edited discards it rather
                    // than silently confirming or acting on the click.
                    model.renaming = None;
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && model.root_picker.is_some() =>
                {
                    match hit_at(&model, mouse.column, mouse.row) {
                        Some(WorkspaceHit::PickSpaceRoot(index)) => {
                            if let Some(root) = model.root_picker.as_mut().and_then(|picker| {
                                picker.select(index);
                                picker.chosen()
                            }) {
                                model.root_picker = None;
                                let _ = send_request(
                                    &mut stream,
                                    &ClientRequest::CreateSpace {
                                        label: None,
                                        root,
                                        columns,
                                        rows,
                                    },
                                );
                            }
                        }
                        // Click outside the picker's own rows discards it —
                        // same rule `renaming` uses.
                        _ => model.root_picker = None,
                    }
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Moved && model.root_picker.is_some() =>
                {
                    // The highlight follows the pointer, the same way the
                    // agent picker and the sidebar context menu already do.
                    if let Some(WorkspaceHit::PickSpaceRoot(index)) =
                        hit_at(&model, mouse.column, mouse.row)
                        && let Some(picker) = model.root_picker.as_mut()
                        && picker.selected() != index
                    {
                        picker.select(index);
                        model.dirty = true;
                    }
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && model.agent_picker.is_some() =>
                {
                    let hit = model
                        .hits
                        .iter()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(_, hit)| *hit);
                    match hit {
                        Some(WorkspaceHit::PickAgent(index)) => {
                            if let Some(picker) = model.agent_picker.take()
                                && let Some(option) = picker.options.get(index)
                            {
                                let label = next_agent_label(&model);
                                let cwd = picker
                                    .cwd
                                    .clone()
                                    .or_else(|| agent_launch_cwd(&model, home));
                                if let Some(cwd) = cwd.clone() {
                                    model.schedule_evaluation(home, cwd, &task_sender);
                                }
                                let _ = send_request(
                                    &mut stream,
                                    &ClientRequest::CreateTab {
                                        cwd,
                                        label,
                                        agent: None,
                                        columns,
                                        rows,
                                        command: Some(option.command.clone()),
                                    },
                                );
                            }
                        }
                        // Click outside the picker's own rows discards it —
                        // same rule `renaming` uses.
                        _ => model.agent_picker = None,
                    }
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Moved && model.agent_picker.is_some() =>
                {
                    // Keep this dropdown's pointer behavior aligned with the
                    // sidebar context menu: the highlighted option follows
                    // the cursor, while keyboard navigation remains intact.
                    let hit = model
                        .hits
                        .iter()
                        .rev()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(_, hit)| *hit);
                    if let Some(WorkspaceHit::PickAgent(index)) = hit
                        && let Some(picker) = model.agent_picker.as_mut()
                        && picker.selected != index
                    {
                        picker.selected = index;
                        model.dirty = true;
                    }
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && model.support_dropdown.is_some() =>
                {
                    // Informational dropdown: every click simply dismisses
                    // it, preventing the click from leaking into the pane.
                    model.support_dropdown = None;
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && model.status_catalog.is_some() =>
                {
                    // Informational, like the support dropdown: any click
                    // dismisses it rather than leaking into the pane.
                    model.status_catalog = None;
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && model.context_menu.is_some() =>
                {
                    // `.rev()`: the popup renders last, so its own rows sit
                    // at the tail of `hits` — searching forward could match
                    // an older, now visually-covered sidebar row underneath
                    // it instead (the tight, gapless sidebar packing meant
                    // this landed on a covered row far more often than not,
                    // which is what made the popup's own click feel
                    // intermittent — it depended on which row was
                    // right-clicked, not on timing).
                    let hit = model
                        .hits
                        .iter()
                        .rev()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(_, hit)| *hit);
                    let action = match hit {
                        Some(WorkspaceHit::ContextMenuAction(index)) => model
                            .context_menu
                            .as_ref()
                            .and_then(|menu| menu.items.get(index).copied()),
                        _ => None,
                    };
                    // Dismiss unconditionally (any click, on the popup or
                    // outside it, closes the menu) but only dispatch when
                    // the click actually resolved to one of its own rows —
                    // the two used to be one `if let` that discarded the
                    // menu before checking the hit, so a miss silently
                    // dismissed without acting instead of visibly no-oping.
                    let target = model.context_menu.take().map(|menu| menu.target);
                    if let (Some(target), Some(action)) = (target, action) {
                        dispatch_menu_action(&mut stream, &mut model, &identities, target, action);
                    }
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Moved && model.context_menu.is_some() =>
                {
                    // Hovering a row selects it, same as Up/Down — so the
                    // popup reads as a real menu (highlight follows the
                    // cursor) instead of only reacting to a click. Only
                    // marks the frame dirty when the hover actually moved
                    // onto a different row, so waving the mouse across the
                    // rest of the screen doesn't force a redraw every tick.
                    let hit = model
                        .hits
                        .iter()
                        .rev()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(_, hit)| *hit);
                    if let Some(WorkspaceHit::ContextMenuAction(index)) = hit
                        && let Some(menu) = model.context_menu.as_mut()
                        && menu.selected != index
                    {
                        menu.selected = index;
                        model.dirty = true;
                    }
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && model.git_view.is_some() =>
                {
                    let hit = model
                        .hits
                        .iter()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(_, hit)| *hit);
                    // Mirrors `WorkspaceHit::ResizeSidebar` below: arms
                    // dragging instead of reaching `git_diff::handle_mouse`,
                    // which only knows about `ExtensionHit`s that are its
                    // own — the resize handle's drag lifecycle belongs to
                    // this workspace client, not the extension.
                    let view_hit = match hit {
                        Some(WorkspaceHit::Extension(ExtensionHit::GitChanges(view_hit))) => {
                            Some(view_hit)
                        }
                        _ => None,
                    };
                    if view_hit == Some(ViewHit::ResizeNavigator) {
                        model.dragging_git_tree = true;
                    } else if let Some(view) = model.git_view.as_mut()
                        && matches!(
                            git_diff::handle_mouse(&WorkspaceHost, view, view_hit),
                            git_diff::GitViewOutcome::Close
                        )
                    {
                        model.git_view = None;
                    }
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Drag(MouseButton::Left)
                        && model.dragging_git_tree =>
                {
                    let frame_area = Rect::new(0, 0, size.width, size.height);
                    let (tree_column, diff_column, _footer) =
                        crate::ui::extension_view::content_columns(
                            frame_area,
                            model.git_tree_width,
                        );
                    let new_width = crate::ui::extension_view::clamp_navigator_width(
                        mouse.column.saturating_sub(tree_column.x),
                        tree_column.width + diff_column.width,
                    );
                    if model.git_tree_width != Some(new_width) {
                        model.git_tree_width = Some(new_width);
                        model.dirty = true;
                    }
                }
                Event::Mouse(mouse)
                    if matches!(
                        mouse.kind,
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    ) && model.git_view.is_some() =>
                {
                    if let Some(view) = model.git_view.as_mut()
                        && let Some(target) = crate::ui::extension_view::scroll_target(
                            Rect::new(0, 0, size.width, size.height),
                            model.git_tree_width,
                            mouse.column,
                            mouse.row,
                        )
                    {
                        git_diff::handle_scroll(
                            &WorkspaceHost,
                            view,
                            target,
                            if mouse.kind == MouseEventKind::ScrollUp {
                                ScrollDirection::Up
                            } else {
                                ScrollDirection::Down
                            },
                        );
                    }
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if matches!(
                        mouse.kind,
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    ) && model.no_modal_open() =>
                {
                    forward_scroll(&mut stream, &model, layout.pane, mouse);
                }
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    let Some((hit_rect, hit)) = model
                        .hits
                        .iter()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(rect, hit)| (*rect, *hit))
                    else {
                        model.last_click = None;
                        forward_mouse(&mut stream, &model, layout.pane, mouse);
                        continue;
                    };
                    let now = std::time::Instant::now();
                    let is_double_click = model.last_click.is_some_and(|(at, previous)| {
                        previous == hit && now.duration_since(at) < DOUBLE_CLICK_WINDOW
                    });
                    model.last_click = Some((now, hit));
                    if is_double_click {
                        model.last_click = None;
                        match hit {
                            WorkspaceHit::SelectTab(tab) => {
                                begin_rename(&mut model, MenuTarget::Tab(tab));
                                model.dirty = true;
                            }
                            WorkspaceHit::SelectSpace(space) => {
                                begin_rename(&mut model, MenuTarget::Space(space));
                                model.dirty = true;
                            }
                            // Two quick clicks on the toggle are two
                            // toggles, not a gesture of their own.
                            WorkspaceHit::ToggleSpaceRoot(space) => {
                                toggle_space_root(&mut model, space);
                            }
                            _ => {}
                        }
                        continue;
                    }
                    match hit {
                        WorkspaceHit::SelectTab(tab) => {
                            // Whether this click landed on the tab already
                            // holding its space's selection — read before
                            // `SelectTab` is sent below, since the model
                            // only updates once the server's broadcast
                            // confirms it, not optimistically here. A drag
                            // candidate arms only on this "second click":
                            // the first click on a different tab just
                            // selects it, exactly like before dragging
                            // existed. Without this, every plain selection
                            // click also armed a drag from that row, and
                            // any incidental pointer motion afterward
                            // (moving toward the next click, mouse jitter)
                            // could cross the threshold and show the drop
                            // indicator on a row the user never meant to
                            // touch.
                            let already_selected = model.session.as_ref().is_some_and(|session| {
                                session
                                    .workspace
                                    .spaces
                                    .iter()
                                    .any(|space| space.selected_tab == tab)
                            });
                            model.acknowledge_completed_agent_tab(tab);
                            let _ = send_request(&mut stream, &ClientRequest::SelectTab { tab });
                            if let Some(pane) = model.pane_for_tab(tab) {
                                resize_pane(&mut stream, &mut model, pane, columns, rows);
                            }
                            if already_selected
                                && let Some(group) =
                                    tab_drag_group(&model, &identities, &layout, hit_rect, tab)
                            {
                                let origin = match group {
                                    TabDragGroup::Agents(_) => mouse.row,
                                    TabDragGroup::Strip(..) => mouse.column,
                                };
                                model.dragging_tab = Some(DraggingTab {
                                    tab,
                                    group,
                                    origin,
                                    armed: false,
                                    pending: None,
                                });
                            }
                        }
                        WorkspaceHit::CloseTab(tab) => {
                            let _ = send_request(&mut stream, &ClientRequest::CloseTab { tab });
                        }
                        WorkspaceHit::NewTab => {
                            let _ = send_request(
                                &mut stream,
                                &ClientRequest::CreateTab {
                                    label: next_shell_label(&model, &identities),
                                    agent: context_agent(&model, &identities),
                                    columns,
                                    rows,
                                    cwd: new_shell_cwd(&model, &identities),
                                    command: None,
                                },
                            );
                        }
                        WorkspaceHit::NewAgentMenu => {
                            model.agent_picker = Some(AgentPicker {
                                options: agent_options(home),
                                selected: 0,
                                anchor: hit_rect,
                                cwd: None,
                            });
                            // Unlike every other arm here, this is a purely
                            // local state change with no server round trip
                            // to eventually mark the model dirty via
                            // `apply()` — without this the popup exists in
                            // `model` but the screen never redraws to show
                            // it.
                            model.dirty = true;
                        }
                        WorkspaceHit::PickAgent(_) => {
                            // Only reachable while the picker is open, which
                            // the guarded arm above already handles; a
                            // stale hit here (picker just closed) is a
                            // no-op.
                        }
                        WorkspaceHit::SelectSpace(space) => {
                            // A space's own row is its own context: it
                            // lands on a shell belonging to no agent, the
                            // way each agent row lands on that agent. That
                            // is the whole way back to the space's shells
                            // once an agent is what the strip is showing.
                            // A space of nothing but agents has no such
                            // tab, and the click stays a plain switch.
                            let landing = model.session.as_ref().and_then(|session| {
                                let space = session
                                    .workspace
                                    .spaces
                                    .iter()
                                    .find(|candidate| candidate.id == space)?;
                                Some((space.selected_tab, space_own_tab(space, &identities)))
                            });
                            if let Some((selected, own)) = landing {
                                model.acknowledge_completed_agent_tab(own.unwrap_or(selected));
                            }
                            let _ = match landing.and_then(|(_, own)| own) {
                                Some(tab) => {
                                    send_request(&mut stream, &ClientRequest::SelectTab { tab })
                                }
                                None => {
                                    send_request(&mut stream, &ClientRequest::SelectSpace { space })
                                }
                            };
                            // Resize the pane the same way `SelectTab` does
                            // — switching spaces switches which tab (and so
                            // which pane) is focused, same as switching
                            // tabs within one space already does.
                            if let Some(pane) = landing
                                .map(|(selected, own)| own.unwrap_or(selected))
                                .and_then(|tab| model.pane_for_tab(tab))
                            {
                                resize_pane(&mut stream, &mut model, pane, columns, rows);
                            }
                        }
                        WorkspaceHit::ContextMenuAction(_) => {
                            // Only reachable while the context menu is
                            // open, which the guarded arm above already
                            // handles — same as `PickAgent` above for the
                            // agent picker.
                        }
                        WorkspaceHit::NewSpace => {
                            let prefill = model
                                .session
                                .as_ref()
                                .map(|session| {
                                    crate::ui::display_project_path(&session.selected_space().root)
                                })
                                .unwrap_or_else(|| "~".to_owned());
                            model.root_picker = Some(RootPicker::opened_in(&prefill));
                            model.dirty = true;
                        }
                        WorkspaceHit::PickSpaceRoot(_) => {
                            // Only reachable while the root picker is open,
                            // which the guarded arm above already handles —
                            // same as `PickAgent` for the agent picker.
                        }
                        WorkspaceHit::OpenGitView => {
                            open_git_view(&mut model);
                        }
                        WorkspaceHit::Deliver(_) => {
                            deliver_selected_tab(&mut model, home, &delivery_sender);
                        }
                        WorkspaceHit::ToggleSpaceRoot(space) => {
                            toggle_space_root(&mut model, space);
                        }
                        WorkspaceHit::OpenStatusCatalog(anchor) => {
                            model.status_catalog = Some(anchor);
                            // Purely local state, no server round trip to
                            // eventually mark the model dirty — same as
                            // `NewAgentMenu` above.
                            model.dirty = true;
                        }
                        WorkspaceHit::OpenAgentSupport(anchor) => {
                            model.support_dropdown = selected_agent_context(&model, &identities)
                                .map(|key| AgentSupportDropdown { key, anchor });
                            // Opening always re-reads, even when an answer
                            // for this key is already held: `AGENTS.md` and
                            // `.agents/` can change under an open workspace,
                            // and this is the one moment the user is
                            // actually looking at the answer.
                            if let Some(dropdown) = &model.support_dropdown {
                                model.agent_support_pending = Some(dropdown.key.clone());
                                spawn_support_refresh(
                                    home,
                                    dropdown.key.clone(),
                                    support_sender.clone(),
                                );
                            }
                            model.dirty = true;
                        }
                        WorkspaceHit::Extension(_) => {
                            // Only reachable while the git view is open,
                            // which its own guarded arm below already
                            // handles — same as `PickAgent`/`CloseSpace`
                            // above for the other two overlays.
                        }
                        WorkspaceHit::SwitchToManagement => {
                            let _ = send_request(&mut stream, &ClientRequest::Detach);
                            return Ok(WorkspaceExit::Management);
                        }
                        WorkspaceHit::ResizeSidebar => {
                            model.dragging_sidebar = true;
                        }
                    }
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Drag(MouseButton::Left)
                        && model.dragging_sidebar =>
                {
                    let new_width = super::clamp_sidebar_width(
                        mouse.column.saturating_sub(layout.sidebar.x),
                        size.width,
                    );
                    if model.sidebar_width != Some(new_width) {
                        model.sidebar_width = Some(new_width);
                        // Written straight through to the shared value (not
                        // just kept on `model`) so a Ctrl+O switch to
                        // management picks up this width immediately,
                        // instead of only on the next drag.
                        *sidebar_width = model.sidebar_width;
                        model.dirty = true;
                    }
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Drag(MouseButton::Left)
                        && model.dragging_tab.is_some() =>
                {
                    let Some(mut dragging) = model.dragging_tab else {
                        unreachable!("guarded by the match arm above");
                    };
                    let pointer = match dragging.group {
                        TabDragGroup::Agents(_) => mouse.row,
                        TabDragGroup::Strip(..) => mouse.column,
                    };
                    if !dragging.armed {
                        dragging.armed = pointer.abs_diff(dragging.origin) >= TAB_DRAG_THRESHOLD;
                    }
                    dragging.pending = dragging
                        .armed
                        .then(|| {
                            // The dragged tab's own rect stays in `hits`
                            // (nothing about the underlying order changes
                            // during the drag — see the design's "indicator,
                            // not a live reorder" decision), so it has to be
                            // excluded here or its own midpoint would offer
                            // itself as a drop target.
                            let members = tab_drag_group_members(
                                &model,
                                &identities,
                                &layout,
                                dragging.group,
                            )
                            .into_iter()
                            .filter(|(_, tab)| *tab != dragging.tab)
                            .collect::<Vec<_>>();
                            pending_tab_drop(&members, dragging.group, pointer, dragging.origin)
                        })
                        .flatten();
                    model.dragging_tab = Some(dragging);
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Drag(MouseButton::Left)
                        && !model.dragging_sidebar
                        && !model.dragging_git_tree
                        && model.dragging_tab.is_none()
                        && model.no_modal_open() =>
                {
                    forward_mouse(&mut stream, &model, layout.pane, mouse);
                }
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Up(MouseButton::Left) => {
                    // A drag this client never owned (no flag was set, no
                    // tab drag was in progress, and nothing modal was open
                    // to have owned it either) is one it was forwarding
                    // into the pane above — the matching release belongs
                    // there too, not just silently dropped the way it was
                    // before pane forwarding existed.
                    if !model.dragging_sidebar
                        && !model.dragging_git_tree
                        && model.dragging_tab.is_none()
                        && model.no_modal_open()
                    {
                        forward_mouse(&mut stream, &model, layout.pane, mouse);
                    }
                    if let Some(dragging) = model.dragging_tab.take()
                        && let Some(pending) = dragging.pending
                    {
                        let _ = send_request(
                            &mut stream,
                            &ClientRequest::ReorderTab {
                                tab: dragging.tab,
                                before: pending.as_before(),
                            },
                        );
                    }
                    model.dragging_sidebar = false;
                    model.dragging_git_tree = false;
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Right)
                        && model.renaming.is_none()
                        && model.root_picker.is_none()
                        && model.agent_picker.is_none()
                        && model.context_menu.is_none() =>
                {
                    // The only way to close a space or an agent tab: right-
                    // click it, then confirm in the popup this opens (see
                    // `ContextMenu`) — never a direct click, so a stray
                    // click can't kill a running agent or a whole space's
                    // worth of them by accident. Guarded on `context_menu`
                    // being closed too (like the left-click/Enter handlers
                    // already are) so right-clicking a different row while
                    // a menu is open can't silently swap its target instead
                    // of requiring the open menu be dismissed first.
                    let hit = model
                        .hits
                        .iter()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(_, hit)| *hit);
                    // Anchored to the cursor itself, not the clicked row's
                    // rect — a row spans the sidebar's full width, so
                    // anchoring to `rect.x` always opened the menu at the
                    // row's left edge regardless of where along it you
                    // right-clicked, which read as the popup ignoring the
                    // mouse entirely.
                    let anchor = Rect::new(mouse.column, mouse.row, 1, 1);
                    // `rename` is always offered. A tab can close with a
                    // sibling as usual, and a lone agent can close because
                    // the action replaces it with a plain shell. Renaming a
                    // lone space or shell remains the only available action.
                    if let Some(WorkspaceHit::SelectSpace(space)) = hit {
                        let mut items = vec![MenuAction::Rename];
                        if model
                            .session
                            .as_ref()
                            .is_some_and(|session| session.workspace.spaces.len() > 1)
                        {
                            items.push(MenuAction::Close);
                        }
                        model.context_menu = Some(ContextMenu {
                            target: MenuTarget::Space(space),
                            items,
                            selected: 0,
                            anchor,
                        });
                        model.dirty = true;
                    } else if let Some(WorkspaceHit::SelectTab(tab)) = hit {
                        let mut items = vec![MenuAction::Rename];
                        if can_close_tab_from_menu(&model, &identities, tab) {
                            items.push(MenuAction::Close);
                        }
                        model.context_menu = Some(ContextMenu {
                            target: MenuTarget::Tab(tab),
                            items,
                            selected: 0,
                            anchor,
                        });
                        model.dirty = true;
                    }
                }
                Event::Resize(_, _) | Event::Mouse(_) => {}
                _ => {}
            }
        }
    })();
    memory.remembered = model.remember();
    outcome
}

/// `pub(super)` (not private) so `uze_extensions::git_diff` — a crate
/// this one depends on, not a child module of `orchestrator` — can
/// construct `ExtensionHit`s from its own render function; `Extension`
/// below wraps them into the same `hits` vec every other overlay already
/// shares, rather than this workspace client threading a second, parallel
/// hit-testing vec just for one extension.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkspaceHit {
    SelectTab(TabId),
    CloseTab(TabId),
    NewTab,
    /// Opens the agent picker (`WorkspaceModel::agent_picker`) — the tab
    /// strip's "✦" button, creating a new agent tab inside the selected
    /// space.
    NewAgentMenu,
    /// One row of the open agent picker, by index into its `options`.
    PickAgent(usize),
    /// A space's header row in the sidebar — click selects it (switching
    /// which space's tabs the tab strip and pane show).
    SelectSpace(SpaceId),
    /// The `⇄` behind a space's name — flips that header between its label
    /// and its root (see `WorkspaceModel::roots_shown`).
    ToggleSpaceRoot(SpaceId),
    /// One row of the open [`ContextMenu`], by index into its `items` —
    /// generic over whatever action that row is, same pattern
    /// [`WorkspaceHit::PickAgent`] uses for the agent picker.
    ContextMenuAction(usize),
    /// The sidebar's "+ new" row — opens the root picker
    /// ([`WorkspaceModel::root_picker`]), since a space is born from a
    /// directory and that directory is chosen, not typed blind.
    NewSpace,
    /// One row of the open root picker, by index into its current matches
    /// — same pattern [`WorkspaceHit::PickAgent`] uses for the agent
    /// picker.
    PickSpaceRoot(usize),
    /// The tab strip's right-corner button — opens the Git changes
    /// extension (`WorkspaceModel::git_view`), scoped to the active tab's
    /// live `cwd`.
    OpenGitView,
    /// Opens contextual support details for the selected agent tab.
    OpenAgentSupport(Rect),
    /// The task mark on a sidebar agent row — opens the catalog of what
    /// every glyph in both columns means, anchored to the mark. A status
    /// column is a wordless vocabulary; this is where it is written down.
    OpenStatusCatalog(Rect),
    /// The tab strip's delivery button — delivers the selected tab's task
    /// the way the project's completion says. Present only when the task
    /// is deliverable.
    Deliver(TabId),
    /// A hit the open extension's own render pass produced (a file row, a
    /// worktree header, its tree/diff resize handle, its close button —
    /// see `uze_extensions::ExtensionHit`), wrapped instead of given its
    /// own `WorkspaceHit` variant per extension. The one Git extension
    /// today owns every `ExtensionHit` variant that exists; a second
    /// extension adds to that enum, not to this one.
    Extension(ExtensionHit),
    SwitchToManagement,
    ResizeSidebar,
}

/// What [`WorkspaceModel::renaming`] is currently editing — a tab or a
/// space header both use the exact same inline-edit interaction (double-
/// click to enter, Enter/Esc/Backspace/typing to edit, click-away to
/// discard), so one buffer serves both; this just says which request to
/// send on commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenameTarget {
    Tab(TabId),
    Space(SpaceId),
}

/// Which set of tabs a drag-to-reorder gesture is confined to — the exact
/// grouping `render.rs` already filters `Space.tabs` by to build the
/// sidebar's agent list (`agent_tabs`) and the tab strip (`strip`). A drag
/// never offers, nor accepts, a drop target from outside the group it
/// began in — see `tab_drag_group`/`tab_drag_group_members`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TabDragGroup {
    /// The agent rows of one space's sidebar list, by space id.
    Agents(SpaceId),
    /// The tabs of one strip: shells opened alongside one agent tab, or —
    /// when `None` — a space's own shells with no agent selected. Matches
    /// `Tab::agent`'s own vocabulary.
    Strip(SpaceId, Option<TabId>),
}

/// A pending tab-reorder drop position, in exactly the shape
/// [`ClientRequest::ReorderTab`] expects it: before a specific tab, or at
/// the end of the group.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingDrop {
    Before(TabId),
    End,
}

impl PendingDrop {
    fn as_before(self) -> Option<TabId> {
        match self {
            PendingDrop::Before(tab) => Some(tab),
            PendingDrop::End => None,
        }
    }
}

/// A tab being dragged for reordering. Armed once the pointer moves past
/// [`TAB_DRAG_THRESHOLD`] from where the press started, so a plain click —
/// still handled immediately and unchanged on `MouseEventKind::Down` — is
/// never mistaken for a drag. Cleared on release either way, and pruned
/// early if the dragged tab disappears from a `Session` update received
/// mid-drag (see `WorkspaceModel::prune_dragging_tab`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DraggingTab {
    tab: TabId,
    group: TabDragGroup,
    /// Row (`Agents`) or column (`Strip`) the press started at.
    origin: u16,
    armed: bool,
    /// Where `tab` would land if released right now. `None` while unarmed,
    /// or whenever the pointer isn't currently over a valid drop position
    /// for `group` — a release in either state is a no-op.
    pending: Option<PendingDrop>,
}

impl DraggingTab {
    /// Whether `tab` — the last member of `group` when `is_last` — is
    /// where this drag's current pending drop would land. Used by the
    /// sidebar and tab-strip renderers to place the one insertion
    /// indicator each draws; `false` for a drag that isn't `armed`, is
    /// over a different group than the one being rendered, or has no
    /// pending drop right now (the pointer has left the group's area).
    fn is_pending_drop_row(self, group: TabDragGroup, tab: TabId, is_last: bool) -> bool {
        if !self.armed || self.group != group {
            return false;
        }
        match self.pending {
            Some(PendingDrop::Before(before)) => before == tab,
            Some(PendingDrop::End) => is_last,
            None => false,
        }
    }
}

/// How far (rows for `Agents`, columns for `Strip`) the pointer must move
/// from a press before it's treated as a reorder drag rather than a plain
/// click — small enough that dragging still feels immediate, large enough
/// to rule out an ordinary click's own jitter.
const TAB_DRAG_THRESHOLD: u16 = 2;

/// A second `Down(Left)` on the same hit within this window counts as a
/// double-click (enters tab rename); slower than this, it's just another
/// single click.
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(400);

/// One selectable row of the agent picker: what to show, and the `argv` to
/// launch in the new pane if chosen.
struct AgentOption {
    display_name: String,
    command: Vec<String>,
}

/// Open state of the "+ new agent" popup (`WorkspaceHit::NewAgentMenu`) —
/// built fresh each time it opens from `agent_options`, never persisted.
struct AgentPicker {
    options: Vec<AgentOption>,
    selected: usize,
    /// The tab strip's "✦" button's own rect — the popup anchors just
    /// under it.
    anchor: Rect,
    /// A directory the new agent must start in — a preserved task's own
    /// slot, when resuming it. `None` lets placement acquire a slot.
    cwd: Option<PathBuf>,
}

/// Open state for the informational support dropdown in an active agent
/// session. The `(harness, cwd)` key keeps it tied to the exact live agent
/// it was opened over, rather than to a mutable display label or process
/// name — and makes a resolution for some other pane unrenderable here.
struct AgentSupportDropdown {
    key: SupportKey,
    anchor: Rect,
}

/// What a right-click-opened [`ContextMenu`] targets — the space or tab its
/// [`MenuAction`]s act on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuTarget {
    Space(SpaceId),
    Tab(TabId),
}

/// One row a [`ContextMenu`] can offer — the menu itself (items, selection,
/// rendering) is generic over this enum, so adding a third action is adding
/// a variant plus a match arm here and in [`dispatch_menu_action`], not
/// restructuring the popup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuAction {
    Rename,
    Close,
}

impl MenuAction {
    fn label(self) -> &'static str {
        match self {
            MenuAction::Rename => "rename",
            MenuAction::Close => "close",
        }
    }
}

/// Open state of the right-click action menu a space header or agent tab
/// raises. Closing a space/tab is never one click any more — right-click,
/// then confirm the menu's own "close" row — deliberately two steps, so an
/// accidental click can't kill a running agent or an entire space's worth
/// of them; other, non-destructive actions this menu grows (like `rename`)
/// don't need that same two-step guard, but share the same
/// open/navigate/confirm mechanics.
/// Built fresh on each right-click, never persisted.
struct ContextMenu {
    target: MenuTarget,
    items: Vec<MenuAction>,
    /// Index into `items` the keyboard's Up/Down currently highlights —
    /// same role [`AgentPicker::selected`] plays.
    selected: usize,
    /// The right-clicked row's own rect — the popup anchors just under it,
    /// same placement rule [`AgentPicker::anchor`] uses.
    anchor: Rect,
}

/// Resolved entirely through the generic `IntegrationPort` contract
/// (`.id()`/`.display_name()`/`.aliases()`) — never a hardcoded vendor list,
/// which `src/` is not allowed to hold (see
/// `tests/integrations/identity.rs::cli_and_tui_never_names_a_vendor_harness`).
/// A registry that fails to construct (rare — see `src/shim.rs`'s identical
/// `.ok()` fallback) just yields no identities rather than failing the
/// whole workspace session.
fn agent_identities(home: &UzeHome) -> Vec<AgentIdentity> {
    tui_application(home.clone())
        .map(|app| app.workspace().agent_identities())
        .unwrap_or_default()
}

/// The harnesses the agent picker offers — one row per
/// [`AgentIdentity`], `command` set to launch that identity's `binary`.
fn agent_options(home: &UzeHome) -> Vec<AgentOption> {
    agent_identities(home)
        .into_iter()
        .map(|identity| AgentOption {
            display_name: identity.display_name.to_owned(),
            command: vec![identity.binary.to_owned()],
        })
        .collect()
}

/// The recognized agent, if any, running in `tab`'s focused pane — matched
/// against `identities` primarily by live foreground process name (a
/// shim-launched process reports its invoked alias there via
/// `UZE_SHIM_NAME`, not its raw `comm` — see
/// `uze_terminal::PaneRuntime::foreground_status`), falling back to the
/// tab's own label only for legacy tabs created before generic agent labels
/// were introduced. Returns the harness's short binary/alias
/// name (`claude`, `codex`, …) — what the sidebar and tab strip show in
/// place of the raw process string, and what decides whether a tab lists
/// under "agents" or "shell" at all.
fn agent_identity_for_tab<'a>(identities: &'a [AgentIdentity], tab: &Tab) -> Option<&'a str> {
    let process = pane_in_layout(&tab.layout, tab.focus.pane).map(|pane| pane.process.as_str());
    identities
        .iter()
        .find(|identity| {
            process.is_some_and(|process| process.eq_ignore_ascii_case(identity.binary))
                || tab.label.eq_ignore_ascii_case(identity.display_name)
        })
        .map(|identity| identity.binary)
}

/// What the sidebar shows beside one agent tab. These four states are the
/// whole vocabulary, and they are mutually exclusive by the precedence
/// encoded in [`WorkspaceModel::agent_tab_status`] — the single place that
/// decides, so the glyph can never disagree with the model that produced
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AgentTabStatus {
    /// The agent is producing output right now.
    Working,
    /// The agent finished while the user was looking somewhere else, and
    /// the user has not opened the tab since.
    Completed,
    /// The tab the user is on, with nothing in flight.
    Selected,
    /// A quiet agent tab the user is not on.
    Idle,
}

impl AgentTabStatus {
    /// The indicator column, including its trailing space. `tick` only
    /// matters for [`AgentTabStatus::Working`], whose glyph animates.
    pub(super) fn glyph(self, tick: usize) -> String {
        match self {
            AgentTabStatus::Working => format!("{} ", agent_activity_frame(tick)),
            AgentTabStatus::Completed => "\u{2713} ".to_owned(),
            AgentTabStatus::Selected => "\u{25cf} ".to_owned(),
            AgentTabStatus::Idle => "\u{25cb} ".to_owned(),
        }
    }

    /// Idle is the only state drawn faint: the other three all report
    /// something the user asked for or needs to notice.
    pub(super) fn color(self) -> Color {
        match self {
            AgentTabStatus::Idle => crate::ui::TEXT_FAINT,
            _ => crate::ui::ACCENT,
        }
    }
}

/// One agent pane's recent repaints and the deadline its busy state runs
/// to. Both halves are load-bearing: the repaints are the evidence that
/// the pane is animating rather than blinking once, and the deadline is
/// what carries "working" across the gaps in that animation.
#[derive(Debug, Default)]
struct AgentActivity {
    repaints: VecDeque<Instant>,
    working_until: Option<Instant>,
}

impl AgentActivity {
    fn is_working(&self) -> bool {
        self.working_until.is_some()
    }

    /// Records one repaint and reports whether the pane has now painted
    /// often enough, recently enough, to read as an animating agent.
    fn note_repaint(&mut self, now: Instant) -> bool {
        self.forget_repaints_before(now);
        self.repaints.push_back(now);
        let spread = match self.repaints.front() {
            Some(oldest) => now.duration_since(*oldest),
            None => Duration::ZERO,
        };
        self.repaints.len() >= AGENT_BUSY_REPAINTS && spread >= AGENT_BUSY_SPAN
    }

    /// Drops the deadline once it has passed, reporting whether this call
    /// is the one that ended the pane's turn, and forgets repaints too old
    /// to still be evidence of animation.
    fn expire(&mut self, now: Instant) -> bool {
        let ended = self.working_until.is_some_and(|deadline| now >= deadline);
        if ended {
            self.working_until = None;
        }
        self.forget_repaints_before(now);
        ended
    }

    fn forget_repaints_before(&mut self, now: Instant) {
        while self
            .repaints
            .front()
            .is_some_and(|at| now.duration_since(*at) >= AGENT_BUSY_WINDOW)
        {
            self.repaints.pop_front();
        }
    }
}

/// Resizes one pane and records that whatever it repaints next is the
/// harness reacting to us. Every resize path goes through here: a pane
/// redrawing on our command is the client's own doing, and counting it as
/// activity is what made every open agent spin for a moment whenever the
/// workspace was opened, a tab selected, or the terminal resized.
fn resize_pane<W: io::Write>(
    stream: &mut W,
    model: &mut WorkspaceModel,
    pane: PaneId,
    columns: u16,
    rows: u16,
) {
    let _ = send_request(
        stream,
        &ClientRequest::Resize {
            pane,
            columns,
            rows,
        },
    );
    model.note_pane_redraw(pane);
}

/// Whether this damage describes an agent painting a frame at all. Damage
/// that redescribes the entire grid is the server having no comparable
/// baseline to diff against — the first push after an attach, or a resize —
/// and counting those is what lit every open agent up as busy for a few
/// seconds whenever the workspace was reopened.
fn is_incremental_repaint(damage: &PaneDamage) -> bool {
    let grid = usize::from(damage.columns) * usize::from(damage.rows);
    !damage.changed.is_empty() && damage.changed.len() < grid
}

fn workspace_has_active_agent_operation(
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
) -> bool {
    model.session.as_ref().is_some_and(|session| {
        session
            .workspace
            .spaces
            .iter()
            .flat_map(|space| &space.tabs)
            .any(|tab| {
                agent_identity_for_tab(identities, tab).is_some()
                    && model.agent_is_working(tab.focus.pane)
            })
    })
}

/// The selected tab's agent, paired with that tab's focused pane's own
/// working directory — `None` for a tab that is not running a recognized
/// agent, which is also what hides the "✦" badge.
fn selected_agent_context(
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
) -> Option<SupportKey> {
    let tab = model.session.as_ref()?.selected_tab();
    let binary = agent_identity_for_tab(identities, tab)?;
    let integration = identities
        .iter()
        .find(|identity| identity.binary == binary)
        .map(|identity| identity.integration)?;
    let cwd = pane_in_layout(&tab.layout, tab.focus.pane)?.cwd.clone();
    Some((integration.to_owned(), cwd))
}

/// A one-line message on screen, and — for one about a single task —
/// enough to tell whether that task is the one currently in front of the
/// operator.
struct Notice {
    text: String,
    since: Instant,
    owner: Option<NoticeOwner>,
}

/// The task a [`Notice`] is about: its id, for matching the selected tab's
/// own task (`WorkspaceModel::notice_for_tab`), and its label, for the
/// footer's fallback when that task is not what is on screen
/// (`WorkspaceModel::notice_for_footer`) — the header never needs it, since
/// the tab it lands next to already says whose agent this is.
struct NoticeOwner {
    task: String,
    label: String,
}

/// One background read's answer channel, kept as a pair so the two ends
/// live and die together.
struct Answers<T> {
    sender: mpsc::Sender<T>,
    receiver: mpsc::Receiver<T>,
}

impl<T> Default for Answers<T> {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }
}

/// What the workspace client keeps between attaches.
///
/// Owned by `super::run` for the life of the process, not by one call to
/// [`attach_workspace`]: a Ctrl+O round trip to management and back is a
/// detach and a fresh attach, and everything this client had resolved on
/// its own — every task, branch, badge and agent status in the sidebar —
/// used to leave with the model that held it. The trip back then redrew
/// every agent row from its bare working directory and filled the captions
/// in again one answer at a time, reading as the whole workspace being
/// resolved from scratch. What comes from the server (the session, the
/// pane grids) is deliberately *not* here: the attach re-reads it, and a
/// stale copy would be worse than a short wait for the real one.
#[derive(Default)]
pub(crate) struct WorkspaceMemory {
    /// The model's own remembered half, taken by the attach and handed
    /// back when it ends (see [`WorkspaceModel::recall`]/[`WorkspaceModel::remember`]).
    remembered: Remembered,
    /// The channels background reads answer on. Kept with the answers they
    /// carry, so a read still running when the user leaves lands after
    /// they come back instead of vanishing with a dropped receiver — which
    /// would also have left its key reserved in the pending sets forever.
    support: Answers<SupportResolution>,
    tasks: Answers<TaskResolution>,
    deliveries: Answers<DeliveryResolution>,
}

/// The fields of [`WorkspaceModel`] that outlive one attach — each is
/// documented on the model, where it is read. Everything not listed here
/// belongs to one attach: the server's view of the session, presentation
/// state such as open overlays and drags, and per-attach transients like
/// echo windows and hit rects.
#[derive(Default)]
struct Remembered {
    agent_activity: BTreeMap<PaneId, AgentActivity>,
    completed_agent_panes: BTreeSet<PaneId>,
    agent_support: Option<SupportResolution>,
    agent_support_pending: Option<SupportKey>,
    git_badge: Option<GitBadge>,
    prompt_buffers: BTreeMap<PaneId, PromptBuffer>,
    tasks: BTreeMap<PathBuf, Vec<TaskView>>,
    branches: BTreeMap<PathBuf, String>,
    upstream_syncs: BTreeMap<PathBuf, UpstreamSync>,
    task_eval_pending: BTreeSet<PathBuf>,
    label_adoptions: BTreeMap<TabId, String>,
    last_task_refresh: Option<Instant>,
    delivery_pending: BTreeSet<String>,
    notice: Option<Notice>,
    pane_checkouts: BTreeMap<PaneId, PathBuf>,
    occupied_checkouts: BTreeSet<PathBuf>,
    slots_swept: bool,
    roots_shown: BTreeSet<SpaceId>,
}

impl WorkspaceModel {
    /// A model starting an attach with what the previous one remembered.
    fn recall(remembered: Remembered) -> Self {
        let Remembered {
            agent_activity,
            completed_agent_panes,
            agent_support,
            agent_support_pending,
            git_badge,
            prompt_buffers,
            tasks,
            branches,
            upstream_syncs,
            task_eval_pending,
            label_adoptions,
            last_task_refresh,
            delivery_pending,
            notice,
            pane_checkouts,
            occupied_checkouts,
            slots_swept,
            roots_shown,
        } = remembered;
        Self {
            agent_activity,
            completed_agent_panes,
            agent_support,
            agent_support_pending,
            git_badge,
            prompt_buffers,
            tasks,
            branches,
            upstream_syncs,
            task_eval_pending,
            label_adoptions,
            last_task_refresh,
            delivery_pending,
            notice,
            pane_checkouts,
            occupied_checkouts,
            slots_swept,
            roots_shown,
            ..Self::default()
        }
    }

    /// What this attach leaves for the next one.
    fn remember(self) -> Remembered {
        Remembered {
            agent_activity: self.agent_activity,
            completed_agent_panes: self.completed_agent_panes,
            agent_support: self.agent_support,
            agent_support_pending: self.agent_support_pending,
            git_badge: self.git_badge,
            prompt_buffers: self.prompt_buffers,
            tasks: self.tasks,
            branches: self.branches,
            upstream_syncs: self.upstream_syncs,
            task_eval_pending: self.task_eval_pending,
            label_adoptions: self.label_adoptions,
            last_task_refresh: self.last_task_refresh,
            delivery_pending: self.delivery_pending,
            notice: self.notice,
            pane_checkouts: self.pane_checkouts,
            occupied_checkouts: self.occupied_checkouts,
            slots_swept: self.slots_swept,
            roots_shown: self.roots_shown,
        }
    }
}

#[derive(Default)]
struct WorkspaceModel {
    session: Option<Session>,
    panes: BTreeMap<PaneId, PaneSnapshot>,
    last_size: (u16, u16),
    error: Option<String>,
    tick: usize,
    /// Per-agent-pane repaint evidence and busy deadline. A pane starts
    /// working either because the user submitted a line into it or because
    /// it started animating on its own — an agent that resumes work with
    /// nothing typed (a hook, a queued turn, a subagent reporting back) is
    /// working just as much as one answering a prompt. Sustained repainting
    /// extends the deadline; a pane that only blinks does not, which is why
    /// merely having an agent process open — or reattaching to one — is
    /// deliberately not activity.
    agent_activity: BTreeMap<PaneId, AgentActivity>,
    /// Agent panes that stopped working while the user was looking
    /// somewhere else. The sidebar keeps their check visible until the tab
    /// is actually on screen, making completion discoverable without
    /// leaving a stale busy spinner.
    completed_agent_panes: BTreeSet<PaneId>,
    /// Until when each pane's own repaints are the echo of input we
    /// forwarded to it, rather than the agent working (see
    /// [`AGENT_ECHO_GRACE`] and [`AGENT_PASTE_GRACE`]).
    input_echo_until: BTreeMap<PaneId, Instant>,
    hits: Vec<(Rect, WorkspaceHit)>,
    /// Set whenever applying an event (or a resize) changes what should be
    /// on screen; the input loop only redraws when this is true. Redrawing
    /// unconditionally at the input-poll rate was the other half of the
    /// workspace client's earlier CPU/latency problem (see [`POLL`]):
    /// ratatui re-copying and diffing a full grid ~60x/sec regardless of
    /// whether anything changed.
    dirty: bool,
    /// User-dragged sidebar width; `None` falls back to `sidebar_width_for`.
    /// Client-local presentation state — never sent to the server.
    sidebar_width: Option<u16>,
    dragging_sidebar: bool,
    /// What's being renamed (a tab or a space) and its live edit buffer.
    /// While set, all keyboard input edits this instead of reaching the
    /// pane, and any click elsewhere cancels it (same "click outside
    /// discards" rule the management TUI's overlays use).
    renaming: Option<(RenameTarget, String)>,
    /// Open state of the sidebar's "+ new" prompt — the directory the next
    /// space is born from, chosen from a live listing that narrows as it is
    /// typed. Same "click outside discards" rule as `renaming`.
    root_picker: Option<RootPicker>,
    last_click: Option<(std::time::Instant, WorkspaceHit)>,
    /// Open state of the "+ new agent" popup; `None` when closed. Same
    /// "click outside discards" rule as `renaming`.
    agent_picker: Option<AgentPicker>,
    /// Contextual support information for the active harness tab.
    support_dropdown: Option<AgentSupportDropdown>,
    /// The open status catalog and the glyph it hangs off — the legend for
    /// the two status columns a sidebar agent row carries. Informational
    /// and anchored, like `support_dropdown`: any click or key dismisses
    /// it. Just the anchor, since the catalog itself is generated from the
    /// same tables the sidebar draws with and holds no state of its own.
    status_catalog: Option<Rect>,
    /// The most recently resolved agent support answer, tagged with the
    /// `(harness, cwd)` it answers — never assumed to apply to a different
    /// selection.
    agent_support: Option<SupportResolution>,
    /// The key a background resolution is currently in flight for, so the
    /// per-frame check cannot queue the same read repeatedly.
    agent_support_pending: Option<SupportKey>,
    /// Open state of the right-click close-confirmation popup; `None` when
    /// closed. Same "click outside discards" rule as `renaming`.
    context_menu: Option<ContextMenu>,
    /// Open state of the git changes overlay; `None` when closed. Unlike
    /// `renaming`/`agent_picker`/`context_menu` there is no "click outside
    /// discards" rule — it covers the full frame, so there is no outside;
    /// `Esc` (or the same shortcut that opened it) is the only dismissal.
    git_view: Option<git_diff::GitView>,
    /// User-dragged Git changes tree width; `None` falls back to its own
    /// responsive default. Mirrors `sidebar_width`/`dragging_sidebar`
    /// above, kept on the model rather than on `GitView` itself so it
    /// survives closing and reopening the overlay within the same
    /// session, the same way the sidebar's width survives switching tabs.
    git_tree_width: Option<u16>,
    dragging_git_tree: bool,
    /// An in-progress tab-reorder drag; `None` when no tab is being
    /// dragged. Client-local presentation state — nothing is sent to the
    /// server until release (see `TabDragGroup`/`DraggingTab`).
    dragging_tab: Option<DraggingTab>,
    /// Cached Git summary for the selected agent/shell tab's live cwd.
    /// Stored client-side because it is display chrome, not terminal session
    /// state that belongs in `uze-terminal`.
    git_badge: Option<GitBadge>,
    /// Per-pane reconstruction of the line being typed, flushed on Enter.
    prompt_buffers: BTreeMap<PaneId, PromptBuffer>,
    /// Sink for recorded prompts. `None` leaves the history untouched —
    /// the default, so tests exercise the submission path without writing
    /// to a real UZE home.
    prompt_recorder: Option<mpsc::Sender<(PathBuf, uze_application::PromptOrigin, String)>>,
    /// Every repository's tasks as last evaluated, keyed by its primary
    /// checkout. Display state: the truth is Git and the task store.
    tasks: BTreeMap<PathBuf, Vec<TaskView>>,
    /// The branch checked out at each evaluation key (see
    /// [`evaluation_key`]) — the primary's for every slot of a repository,
    /// a directory's own outside any slot. Read for an agent outside any
    /// slot, whose caption has no task to take a branch from.
    branches: BTreeMap<PathBuf, String>,
    /// How the branch in [`Self::branches`] stands against its upstream,
    /// under the same key, for the keys where that branch is the delivery
    /// target and tracks something. Read for an agent outside any slot:
    /// the operator's own tree is the one a pull or a push is due on.
    upstream_syncs: BTreeMap<PathBuf, UpstreamSync>,
    /// Repositories an evaluation is in flight for, so a quiet pane and
    /// the clock cannot queue the same read twice.
    task_eval_pending: BTreeSet<PathBuf>,
    /// Shell tabs told to take an agent label, and the label each was
    /// told, until the session confirms it — so two updates arriving
    /// before the rename lands do not ask twice.
    label_adoptions: BTreeMap<TabId, String>,
    last_task_refresh: Option<Instant>,
    /// Agent panes that went quiet since the last tick — the moment
    /// readiness is re-read.
    recently_quiet: Vec<PaneId>,
    /// Tasks a delivery is in flight for.
    delivery_pending: BTreeSet<String>,
    /// A one-line message and when it appeared.
    notice: Option<Notice>,
    /// Open state of the preserved-work list; `None` when closed.
    preserved: Option<PreservedOverlay>,
    /// The checkout each open pane was first seen in — a pane's slot does
    /// not change when it `cd`s.
    pane_checkouts: BTreeMap<PaneId, PathBuf>,
    /// The slot directories a pane still holds. A checkout that leaves this
    /// set lost its last pane, which is what ends the task running there.
    occupied_checkouts: BTreeSet<PathBuf>,
    /// Whether the sweep for tasks nobody's session restored has run.
    slots_swept: bool,
    /// The spaces whose header row shows its root instead of its label —
    /// flipped by the `⇄` behind the name (see
    /// [`WorkspaceHit::ToggleSpaceRoot`]). Never both at once: the row is
    /// one line wide and a path is the one thing on it that can be any
    /// length. Remembered across attaches like any other sidebar
    /// resolution, so a Ctrl+O round trip does not flip it back.
    roots_shown: BTreeSet<SpaceId>,
}

/// Client-side reconstruction of what the user typed into a pane before
/// Enter.
///
/// UZE forwards keystrokes to a PTY whose line editor it cannot observe, so
/// this models only an ordinary single-line edit: printable characters,
/// backspace/delete, and horizontal cursor movement. Anything that could
/// rewrite the line invisibly — history recall, completion, a kill ring, a
/// control chord this does not encode — marks the buffer untrusted, and an
/// untrusted buffer is discarded at Enter. Recording nothing is always
/// preferable to recording a prompt the user never typed.
struct PromptBuffer {
    characters: Vec<char>,
    cursor: usize,
    trusted: bool,
}

impl Default for PromptBuffer {
    fn default() -> Self {
        Self {
            characters: Vec::new(),
            cursor: 0,
            trusted: true,
        }
    }
}

impl PromptBuffer {
    fn apply(&mut self, key: KeyEvent) {
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            self.trusted = false;
            return;
        }
        match key.code {
            KeyCode::Char(character) => {
                self.characters.insert(self.cursor, character);
                self.cursor += 1;
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.characters.remove(self.cursor);
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.characters.len() {
                    self.characters.remove(self.cursor);
                }
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.characters.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.characters.len(),
            _ => self.trusted = false,
        }
    }

    fn paste(&mut self, text: &str) {
        for character in text.chars().map(|c| if c == '\r' { '\n' } else { c }) {
            self.characters.insert(self.cursor, character);
            self.cursor += 1;
        }
    }

    /// The text to record, or `None` when there is nothing trustworthy to
    /// record — including the line-continuation case, where the Enter
    /// inserts a newline instead of submitting and the buffer must survive.
    fn submit(&mut self) -> Option<String> {
        if self.trusted && self.cursor > 0 && self.characters[self.cursor - 1] == '\\' {
            self.characters[self.cursor - 1] = '\n';
            return None;
        }
        let flushed = std::mem::take(self);
        flushed
            .trusted
            .then(|| flushed.characters.into_iter().collect())
    }
}

struct GitBadge {
    cwd: PathBuf,
    summary: Option<git_diff::GitChangeSummary>,
    checked_at: Instant,
}
impl WorkspaceModel {
    fn apply(&mut self, event: ClientEvent, identities: &[AgentIdentity]) {
        if !matches!(event, ClientEvent::Error { .. }) {
            self.error = None;
        }
        self.dirty = true;
        match event {
            ClientEvent::Attached { session } => self.session = Some(session),
            ClientEvent::Snapshot { session, panes } => {
                self.session = Some(session);
                self.panes = panes.into_iter().map(|pane| (pane.pane, pane)).collect();
            }
            ClientEvent::SessionUpdated { session } => {
                self.session = Some(session);
                self.prune_dragging_tab();
            }
            ClientEvent::Damage(damage) => {
                if is_incremental_repaint(&damage) {
                    self.note_agent_output(damage.pane, identities, Instant::now());
                }
                let entry = self
                    .panes
                    .entry(damage.pane)
                    .or_insert_with(|| blank_pane(damage.pane, damage.columns, damage.rows));
                if entry.columns != damage.columns || entry.rows != damage.rows {
                    *entry = blank_pane(damage.pane, damage.columns, damage.rows);
                }
                entry.cursor = damage.cursor;
                entry.alternate_screen = damage.alternate_screen;
                entry.mouse = damage.mouse;
                entry.bracketed_paste = damage.bracketed_paste;
                for (row, column, cell) in damage.changed {
                    let index =
                        usize::from(row) * usize::from(damage.columns) + usize::from(column);
                    if let Some(slot) = entry.cells.get_mut(index) {
                        *slot = cell;
                    }
                }
            }
            ClientEvent::Error { message } => self.error = Some(message),
            ClientEvent::Detached | ClientEvent::Stopped => {}
        }
    }
    /// Clears an in-progress tab drag if the tab it names no longer exists
    /// — closed by another client, or by a concurrent `CloseTab`, while
    /// this one was mid-drag. Called on every `SessionUpdated`; leaves an
    /// unrelated drag (or none at all) alone.
    fn prune_dragging_tab(&mut self) {
        let Some(dragging) = self.dragging_tab else {
            return;
        };
        let still_exists = self.session.as_ref().is_some_and(|session| {
            session
                .workspace
                .spaces
                .iter()
                .any(|space| space.tabs.iter().any(|tab| tab.id == dragging.tab))
        });
        if !still_exists {
            self.dragging_tab = None;
        }
    }
    /// None of the modal overlays that own mouse input while they're open
    /// (rename buffer, new-space root picker, agent picker, support
    /// dropdown, status catalog, isolation tip,
    /// context menu, Git changes view) are
    /// currently up — the precondition for forwarding a drag/release/scroll
    /// that isn't already claimed by one of them straight into the focused
    /// pane's PTY instead of dropping it.
    fn no_modal_open(&self) -> bool {
        self.renaming.is_none()
            && self.root_picker.is_none()
            && self.agent_picker.is_none()
            && self.support_dropdown.is_none()
            && self.status_catalog.is_none()
            && self.preserved.is_none()
            && self.context_menu.is_none()
            && self.git_view.is_none()
    }
    fn focused_pane(&self) -> PaneId {
        self.session
            .as_ref()
            .map(|session| session.selected_tab().focus.pane)
            .unwrap_or(PaneId(1))
    }
    fn selected_tab(&self) -> Option<TabId> {
        self.session
            .as_ref()
            .map(|session| session.selected_tab().id)
    }
    fn pane_for_tab(&self, tab: TabId) -> Option<PaneId> {
        self.session.as_ref().and_then(|session| {
            session
                .workspace
                .spaces
                .iter()
                .flat_map(|space| &space.tabs)
                .find(|candidate| candidate.id == tab)
                .map(|candidate| candidate.focus.pane)
        })
    }
    /// Marks the pane as busy and, when the submission was reconstructed
    /// with confidence, records it. Activity is noted for every Enter in an
    /// agent pane — that signal predates the history and does not depend on
    /// knowing what was typed.
    fn note_agent_prompt_submission(
        &mut self,
        pane: PaneId,
        identities: &[AgentIdentity],
        prompt: Option<&str>,
    ) {
        let origin = self.session.as_ref().and_then(|session| {
            session.workspace.spaces.iter().find_map(|space| {
                space.tabs.iter().find_map(|tab| {
                    if tab.focus.pane != pane {
                        return None;
                    }
                    agent_identity_for_tab(identities, tab).map(|binary| {
                        uze_application::PromptOrigin {
                            space_label: space.label.clone(),
                            tab_id: tab.id.0,
                            tab_label: tab.label.clone(),
                            agent_binary: binary.to_owned(),
                        }
                    })
                })
            })
        });
        let Some(origin) = origin else {
            return;
        };
        self.agent_activity.entry(pane).or_default().working_until =
            Some(Instant::now() + AGENT_QUIET_AFTER);
        self.completed_agent_panes.remove(&pane);
        self.dirty = true;

        if let (Some(prompt), Some(recorder)) = (prompt, self.prompt_recorder.as_ref())
            && let Some(root) = self.space_root_of_pane(pane)
        {
            let _ = recorder.send((root, origin, prompt.to_owned()));
        }
    }

    /// Notes that `pane` painted something. Repainting both *starts* and
    /// extends an agent's busy state — the asymmetry where only Enter could
    /// start it left every self-driven turn, and every turn that resumed
    /// after a quiet stretch, showing as idle — but only once there is
    /// enough of it to be animation rather than a blink (see
    /// [`AGENT_BUSY_REPAINTS`]). Damage that is really the echo of the
    /// user's own typing or pasting is ignored, as is damage in a shell
    /// pane.
    fn note_agent_output(&mut self, pane: PaneId, identities: &[AgentIdentity], now: Instant) {
        if !self.is_agent_pane(pane, identities) || self.is_echoing_input(pane, now) {
            return;
        }
        let activity = self.agent_activity.entry(pane).or_default();
        if activity.note_repaint(now) {
            activity.working_until = Some(now + AGENT_QUIET_AFTER);
            self.completed_agent_panes.remove(&pane);
        }
    }

    fn agent_is_working(&self, pane: PaneId) -> bool {
        self.agent_activity
            .get(&pane)
            .is_some_and(AgentActivity::is_working)
    }

    fn is_echoing_input(&self, pane: PaneId, now: Instant) -> bool {
        self.input_echo_until
            .get(&pane)
            .is_some_and(|until| now < *until)
    }

    /// Opens the window in which `pane`'s own repaints read as the echo of
    /// a keystroke we just forwarded there.
    fn note_pane_input(&mut self, pane: PaneId) {
        self.open_echo_window(pane, Instant::now(), AGENT_ECHO_GRACE);
    }

    /// Same, for a paste: the harness re-lays out its prompt box around the
    /// pasted content — an image especially — long after the bytes landed.
    fn note_pane_paste(&mut self, pane: PaneId) {
        self.open_echo_window(pane, Instant::now(), AGENT_PASTE_GRACE);
    }

    /// Same, for a redraw this client provoked by resizing the pane.
    fn note_pane_redraw(&mut self, pane: PaneId) {
        self.open_echo_window(pane, Instant::now(), AGENT_REDRAW_GRACE);
    }

    /// Same, for the repaint an attach provokes across every open pane at
    /// once rather than in the one pane being resized.
    fn note_attach_redraw(&mut self) {
        let now = Instant::now();
        let panes: Vec<PaneId> = self.panes.keys().copied().collect();
        for pane in panes {
            self.open_echo_window(pane, now, AGENT_REDRAW_GRACE);
        }
    }

    fn open_echo_window(&mut self, pane: PaneId, now: Instant, grace: Duration) {
        self.input_echo_until.insert(pane, now + grace);
    }

    fn is_agent_pane(&self, pane: PaneId, identities: &[AgentIdentity]) -> bool {
        self.session.as_ref().is_some_and(|session| {
            session
                .workspace
                .spaces
                .iter()
                .flat_map(|space| &space.tabs)
                .any(|tab| {
                    tab.focus.pane == pane && agent_identity_for_tab(identities, tab).is_some()
                })
        })
    }

    /// Advances every agent pane's phase for the current instant: a pane
    /// quiet for [`AGENT_QUIET_AFTER`] stops working, and one that stopped
    /// out of sight keeps a check until its tab is actually on screen.
    /// Clearing that check here — rather than only at the handful of call
    /// sites that switch tabs — is what makes "done" disappear exactly when
    /// the user looks at it, whichever way they got there.
    fn expire_agent_activity(&mut self, now: Instant) -> bool {
        let focused = self.focused_pane();
        let mut expired = Vec::new();
        for (pane, activity) in &mut self.agent_activity {
            if activity.expire(now) {
                expired.push(*pane);
            }
        }
        for pane in &expired {
            if *pane != focused {
                self.completed_agent_panes.insert(*pane);
            }
        }
        self.recently_quiet.extend(expired.iter().copied());
        // A closed echo window, and a pane holding neither a deadline nor
        // recent repaints, have nothing left to say about themselves —
        // dropping them keeps this tick's early exit reachable.
        self.input_echo_until.retain(|_, until| now < *until);
        self.agent_activity
            .retain(|_, activity| activity.is_working() || !activity.repaints.is_empty());
        let acknowledged = self.completed_agent_panes.remove(&focused);
        // A pane whose tab is gone can never be looked at again, and its id
        // is free to be handed to a future pane — leaving its check behind
        // would eventually surface on something unrelated.
        self.forget_closed_agent_panes();
        !expired.is_empty() || acknowledged
    }

    fn forget_closed_agent_panes(&mut self) {
        // Runs on every input tick, so it earns the early exit: with no
        // per-pane state held there is nothing to reconcile, and walking
        // the tab tree to build a live-pane set would be pure overhead.
        if self.agent_activity.is_empty()
            && self.completed_agent_panes.is_empty()
            && self.input_echo_until.is_empty()
        {
            return;
        }
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let live: BTreeSet<PaneId> = session
            .workspace
            .spaces
            .iter()
            .flat_map(|space| &space.tabs)
            .map(|tab| tab.focus.pane)
            .collect();
        self.agent_activity.retain(|pane, _| live.contains(pane));
        self.completed_agent_panes
            .retain(|pane| live.contains(pane));
        self.input_echo_until.retain(|pane, _| live.contains(pane));
    }

    /// The one place the four sidebar states are decided. Working outranks
    /// Completed (fresh output means the run the check would announce is
    /// not over), and both outrank Selected — a spinner or a check on the
    /// tab you are already on still carries information the plain dot does
    /// not.
    /// The root of the space `pane`'s tab belongs to.
    fn space_root_of_pane(&self, pane: PaneId) -> Option<PathBuf> {
        let session = self.session.as_ref()?;
        session
            .workspace
            .spaces
            .iter()
            .find(|space| {
                space
                    .tabs
                    .iter()
                    .any(|tab| pane_in_layout(&tab.layout, pane).is_some())
            })
            .map(|space| space.root.clone())
    }

    /// The task whose slot `cwd` sits in, as last evaluated. Lexical: the
    /// slot's name is its identifier, and the primary it hangs off keys
    /// the repository's tasks.
    ///
    /// A slot outlives the tasks that run in it, and a task that ended
    /// keeps naming the slot it ran in, so more than one task can point at
    /// the same directory. The newest is the one running there — the same
    /// rule `checkout::slot_state` reads occupancy by. Taking the first
    /// match instead handed a new agent the previous occupant's branch and
    /// its status mark.
    fn task_for_cwd(&self, cwd: &Path) -> Option<&TaskView> {
        let checkout = uze_application::isolated_checkout(cwd)?;
        self.tasks
            .get(checkout.primary)?
            .iter()
            .filter(|task| {
                task.checkout
                    .as_deref()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == checkout.name)
            })
            .max_by_key(|task| task.created_at_unix)
    }

    pub(super) fn tab_task(&self, tab: TabId) -> Option<&TaskView> {
        let cwd = tab_cwd(self, tab)?;
        self.task_for_cwd(&cwd)
    }

    fn pane_cwd(&self, pane: PaneId) -> Option<PathBuf> {
        let session = self.session.as_ref()?;
        session
            .workspace
            .spaces
            .iter()
            .flat_map(|space| &space.tabs)
            .find_map(|tab| pane_in_layout(&tab.layout, pane).map(|pane| pane.cwd.clone()))
    }

    /// The pane of the tab running in `checkout` — where a message for
    /// that task's agent goes, and what makes the task "in front of
    /// someone". Any tab counts: a shell the operator opened in a slot is
    /// as much in front of it as the agent was.
    fn pane_for_checkout(&self, checkout: &Path) -> Option<PaneId> {
        let session = self.session.as_ref()?;
        session
            .workspace
            .spaces
            .iter()
            .flat_map(|space| &space.tabs)
            .find_map(|tab| {
                let pane = pane_in_layout(&tab.layout, tab.focus.pane)?;
                pane.cwd.starts_with(checkout).then_some(pane.id)
            })
    }

    /// Tasks holding work that no live agent tab is in front of, with the
    /// repository each belongs to — what "preserved from yesterday" lists.
    pub(super) fn preserved_tasks(&self) -> Vec<(PathBuf, TaskView)> {
        let mut preserved: Vec<(PathBuf, TaskView)> = self
            .tasks
            .iter()
            .flat_map(|(primary, tasks)| tasks.iter().map(move |task| (primary, task)))
            .filter(|(_, task)| {
                !matches!(
                    task.state,
                    TaskStateView::Integrated | TaskStateView::Closed
                )
            })
            .filter(|(_, task)| {
                task.checkout
                    .as_deref()
                    .is_none_or(|checkout| self.pane_for_checkout(checkout).is_none())
            })
            .map(|(primary, task)| (primary.clone(), task.clone()))
            .collect();
        preserved.sort_by_key(|(_, task)| task.created_at_unix);
        preserved
    }

    fn schedule_evaluation(
        &mut self,
        home: &UzeHome,
        cwd: PathBuf,
        sender: &mpsc::Sender<TaskResolution>,
    ) {
        let key = evaluation_key(&cwd);
        if !self.task_eval_pending.insert(key.clone()) {
            return;
        }
        spawn_task_evaluation(home, key, cwd, sender.clone());
    }

    fn set_notice(&mut self, text: String) {
        self.notice = Some(Notice {
            text,
            since: Instant::now(),
            owner: None,
        });
        self.dirty = true;
    }

    /// Same as `set_notice`, but attributed to one task: shown next to
    /// that task's own tab in the header, label-free, when that tab is the
    /// one currently in front of the operator — the footer only takes it,
    /// labeled, when the task the message is about is not what is on
    /// screen.
    fn set_task_notice(&mut self, task: &str, label: &str, text: String) {
        self.notice = Some(Notice {
            text,
            since: Instant::now(),
            owner: Some(NoticeOwner {
                task: task.to_owned(),
                label: label.to_owned(),
            }),
        });
        self.dirty = true;
    }

    /// The active notice's text, when it is about `tab`'s own task — what
    /// the header shows next to that task's deliver button, since the tab
    /// is already what says whose agent this is.
    pub(super) fn notice_for_tab(&self, tab: TabId) -> Option<&str> {
        let notice = self.notice.as_ref()?;
        let owner = notice.owner.as_ref()?;
        let selected = self.tab_task(tab)?;
        (selected.id == owner.task).then_some(notice.text.as_str())
    }

    /// The active notice as the footer shows it: nothing, when it already
    /// surfaced in the header next to the selected tab's own task; the
    /// bare text for a workspace-wide message; `"label: text"` for one
    /// about a task that is not what is currently on screen.
    pub(super) fn notice_for_footer(&self) -> Option<String> {
        let notice = self.notice.as_ref()?;
        let Some(owner) = &notice.owner else {
            return Some(notice.text.clone());
        };
        let shown_in_header = self
            .selected_tab()
            .and_then(|tab| self.tab_task(tab))
            .is_some_and(|selected| selected.id == owner.task);
        (!shown_in_header).then(|| format!("{}: {}", owner.label, notice.text))
    }

    fn agent_tab_status(&self, pane: PaneId, selected: bool) -> AgentTabStatus {
        if self.agent_is_working(pane) {
            AgentTabStatus::Working
        } else if self.completed_agent_panes.contains(&pane) {
            AgentTabStatus::Completed
        } else if selected {
            AgentTabStatus::Selected
        } else {
            AgentTabStatus::Idle
        }
    }

    fn acknowledge_completed_agent_tab(&mut self, tab: TabId) {
        if let Some(pane) = self.pane_for_tab(tab)
            && self.completed_agent_panes.remove(&pane)
        {
            self.dirty = true;
        }
    }
    fn refresh_git_badge(&mut self) {
        let cwd = self.session.as_ref().and_then(|session| {
            let tab = session.selected_tab();
            pane_in_layout(&tab.layout, tab.focus.pane).map(|pane| pane.cwd.clone())
        });
        let now = Instant::now();
        if self.git_badge.as_ref().is_some_and(|badge| {
            cwd.as_ref().is_some_and(|cwd| cwd == &badge.cwd)
                && now.duration_since(badge.checked_at) < GIT_BADGE_REFRESH
        }) {
            return;
        }
        self.git_badge = cwd.map(|cwd| GitBadge {
            summary: git_diff::change_summary(&WorkspaceHost, &cwd),
            cwd,
            checked_at: now,
        });
    }
}

// --- Layout --------------------------------------------------------------

// --- Rendering -------------------------------------------------------------

/// The label a plain "$ shell" tab opens with — numbered off the selected
/// space's current tab count (shells are per-space, same as everything
/// else in the tab strip) so opening several in a row reads as "shell 2",
/// "shell 3", … instead of every one showing the identical generic "shell".
/// Numbered within the context it opens in — the shells shown beside one
/// agent count from one, rather than inheriting a number from every other
/// tab of the space, which is what made a fresh agent's first shell read
/// as "shell 4".
fn next_shell_label(model: &WorkspaceModel, identities: &[AgentIdentity]) -> String {
    let count = model.session.as_ref().map_or(0, |session| {
        let context = context_agent(model, identities);
        session
            .selected_space()
            .tabs
            .iter()
            .filter(|tab| tab.agent == context && agent_identity_for_tab(identities, tab).is_none())
            .count()
    });
    format!("shell {}", count + 1)
}

/// The agent the workspace is currently *about*: the selected tab when it
/// is an agent, otherwise the agent that tab was opened alongside. `None`
/// is the space's own context — its bootstrap shell, and anything opened
/// with no agent in front of the person.
///
/// This is what makes the tab strip contextual: one agent and the shells
/// that belong with it at a time, never another agent's.
fn context_agent(model: &WorkspaceModel, identities: &[AgentIdentity]) -> Option<TabId> {
    space_context_agent(model.session.as_ref()?.selected_space(), identities)
}

/// [`context_agent`] for any one space, whether or not it is selected:
/// the agent `space` is currently about. The sidebar marks this agent as
/// the selected one, so switching to one of its shells never unselects
/// it — the shells are part of the agent's own context, not a way out
/// of it.
fn space_context_agent(space: &Space, identities: &[AgentIdentity]) -> Option<TabId> {
    let selected = space.tabs.iter().find(|tab| tab.id == space.selected_tab)?;
    let agent = match agent_identity_for_tab(identities, selected) {
        Some(_) => selected.id,
        None => selected.agent?,
    };
    // The tab a shell points at can have stopped being an agent under it
    // (the harness exited, leaving a plain shell behind); the space is the
    // honest context then, not a tab that no longer runs anything.
    space
        .tabs
        .iter()
        .find(|tab| tab.id == agent && agent_identity_for_tab(identities, tab).is_some())
        .map(|tab| tab.id)
}

/// Which drag-reorder group `hit_rect` (a `WorkspaceHit::SelectTab(tab)`
/// rect) belongs to, if any — `Agents` for a sidebar row, keyed by `tab`'s
/// own space (found by searching, same as every other tab lookup in this
/// module); `Strip` for a tab-strip chip, keyed by the selected space and
/// its current context agent. Neither region test needs `tab` to already
/// be known to be an agent or a shell — the region the rect landed in
/// already says which grouping applies.
fn tab_drag_group(
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    layout: &WorkspaceLayout,
    hit_rect: Rect,
    tab: TabId,
) -> Option<TabDragGroup> {
    let session = model.session.as_ref()?;
    if hit_rect.x < layout.sidebar.right() {
        let space = session
            .workspace
            .spaces
            .iter()
            .find(|space| space.tabs.iter().any(|t| t.id == tab))?;
        return Some(TabDragGroup::Agents(space.id));
    }
    if hit_rect.y >= layout.tab_strip.y && hit_rect.y < layout.tab_strip.bottom() {
        let space = session.selected_space();
        return Some(TabDragGroup::Strip(
            space.id,
            context_agent(model, identities),
        ));
    }
    None
}

/// Every `SelectTab` rect currently on screen that belongs to `group`,
/// sorted along the axis a drag within that group moves on — top-to-bottom
/// for `Agents`, left-to-right for `Strip`. Read straight off the render
/// pass's own `hits` (via [`tab_drag_group`], the same classifier a
/// mousedown used to arm the drag in the first place) rather than
/// recomputed from `Space.tabs` and the render filters a second time, so
/// this can never disagree with what's actually drawn. A sidebar tab pushes
/// two hits (its label and detail rows) — merged into the one rect
/// spanning both, not collapsed to just the first. Keeping only the label
/// row here used to leave each member exactly 1 row tall, which made its
/// own midpoint equal its own top edge — hovering anywhere on a tab's own
/// two rows could never register as "before this tab", only ever "before
/// the next one" (see [`pending_tab_drop`]'s halves).
fn tab_drag_group_members(
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    layout: &WorkspaceLayout,
    group: TabDragGroup,
) -> Vec<(Rect, TabId)> {
    let mut union: std::collections::BTreeMap<TabId, Rect> = std::collections::BTreeMap::new();
    for (rect, hit) in &model.hits {
        let WorkspaceHit::SelectTab(tab) = hit else {
            continue;
        };
        if tab_drag_group(model, identities, layout, *rect, *tab) != Some(group) {
            continue;
        }
        union
            .entry(*tab)
            .and_modify(|merged| *merged = merged.union(*rect))
            .or_insert(*rect);
    }
    let mut members: Vec<(Rect, TabId)> =
        union.into_iter().map(|(tab, rect)| (rect, tab)).collect();
    match group {
        TabDragGroup::Agents(_) => members.sort_by_key(|(rect, _)| rect.y),
        TabDragGroup::Strip(..) => members.sort_by_key(|(rect, _)| rect.x),
    }
    members
}

/// Where a tab being dragged within `group` would land if released with
/// the pointer at `pointer` (a row for `Agents`, a column for `Strip`),
/// given `members` as [`tab_drag_group_members`] already sorted them (the
/// dragged tab itself excluded) and `origin` as that dragged tab's own
/// position before it was excluded — `None` when `pointer` isn't over the
/// group's own area at all (a different space's rows, a different agent's
/// strip, blank space), which is what clears the insertion indicator and
/// makes an eventual release a no-op. Walking each member in order and
/// stopping at the first whose own midpoint the pointer hasn't reached yet
/// is what turns "the top half of a row" into "drop before it" and "the
/// bottom half" into "keep looking at the next one" — falling off the end
/// means "drop after the last one".
///
/// One member is special: whichever sat immediately after the dragged tab
/// originally. Landing "before" it reconstructs the exact slot the tab
/// just left — `Session::reorder_tab`'s own `landing == from` check
/// already treats that as a no-op — so unlike every other member, its own
/// near half is never offered as a distinct target; touching any part of
/// it resolves straight through to "after it" instead. Without this, that
/// member's top half — its own label row, the one a click naturally aims
/// for — silently did nothing, and only entering the *next* member's own
/// zone (one full row further down than expected) produced a visible
/// move.
fn pending_tab_drop(
    members: &[(Rect, TabId)],
    group: TabDragGroup,
    pointer: u16,
    origin: u16,
) -> Option<PendingDrop> {
    if members.is_empty() {
        return None;
    }
    let position = |rect: Rect| match group {
        TabDragGroup::Agents(_) => rect.y,
        TabDragGroup::Strip(..) => rect.x,
    };
    let extent = |rect: Rect| match group {
        TabDragGroup::Agents(_) => rect.y + rect.height,
        TabDragGroup::Strip(..) => rect.x + rect.width,
    };
    // A little slack past either end: dragging just above the first row,
    // or just past the last tab, still means "put it there" rather than
    // needing to land exactly on a row/chip.
    let slack: u16 = match group {
        TabDragGroup::Agents(_) => 2,
        TabDragGroup::Strip(..) => 4,
    };
    let first = position(members[0].0);
    let last = extent(members[members.len() - 1].0);
    if pointer + slack < first || pointer > last + slack {
        return None;
    }
    let mut passed_origin = false;
    for &(rect, tab) in members {
        let is_moot_successor = !passed_origin && position(rect) > origin;
        passed_origin = passed_origin || position(rect) > origin;
        if is_moot_successor {
            if pointer < position(rect) {
                return Some(PendingDrop::Before(tab));
            }
            continue;
        }
        let midpoint = position(rect) + (extent(rect) - position(rect)) / 2;
        if pointer < midpoint {
            return Some(PendingDrop::Before(tab));
        }
    }
    Some(PendingDrop::End)
}

/// Where a shell opened by hand starts: the context agent's own directory,
/// so every shell in an agent's group opens on the work that agent is
/// doing — its slot, not wherever the previous shell was left.
fn new_shell_cwd(model: &WorkspaceModel, identities: &[AgentIdentity]) -> Option<PathBuf> {
    context_agent(model, identities)
        .and_then(|agent| tab_cwd(model, agent))
        .or_else(|| selected_pane_cwd(model))
}

/// The tab a click on a space's own row lands on: the space's own context
/// (a shell belonging to no agent), keeping the current selection when it
/// already is one. `None` means the space has nothing but agents, and the
/// click stays a plain space switch.
fn space_own_tab(space: &Space, identities: &[AgentIdentity]) -> Option<TabId> {
    let own = |tab: &&Tab| tab.agent.is_none() && agent_identity_for_tab(identities, tab).is_none();
    space
        .tabs
        .iter()
        .find(|tab| tab.id == space.selected_tab)
        .filter(own)
        .or_else(|| space.tabs.iter().find(own))
        .map(|tab| tab.id)
}

/// The label a new agent tab opens with. Agent labels are deliberately
/// independent of the chosen harness: the picker selects what runs, while
/// the tab is numbered by the user's workspace organization.
fn next_agent_label(model: &WorkspaceModel) -> String {
    let count = model.session.as_ref().map_or(0, |session| {
        session
            .selected_space()
            .tabs
            .iter()
            .filter(|tab| is_generated_agent_label(&tab.label))
            .count()
    });
    format!("agent {}", count + 1)
}

fn is_generated_agent_label(label: &str) -> bool {
    is_generated_label(label, "agent")
}

/// A label the runtime or [`next_shell_label`] gave a plain shell — the
/// bootstrap `shell`, or `shell N` — as opposed to one the user typed.
fn is_generated_shell_label(label: &str) -> bool {
    label == "shell" || is_generated_label(label, "shell")
}

fn is_generated_label(label: &str, prefix: &str) -> bool {
    label
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix(' '))
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|number| number > 0)
}

/// The renames owed to shells that started running an agent — one typed
/// straight into a shell tab, which then keeps the tab's own label. A
/// generated `shell N` label says nothing the user meant, so the tab takes
/// the `agent N` label it would have opened with; a label the user chose
/// is theirs and stays. Numbered per space, the same way
/// [`next_agent_label`] numbers, and in tab order when several adopt at
/// once. Each tab is asked once: `label_adoptions` remembers the request
/// until the session shows the label changed, or the tab is gone.
fn adopt_agent_labels(
    model: &mut WorkspaceModel,
    identities: &[AgentIdentity],
) -> Vec<ClientRequest> {
    let Some(session) = model.session.as_ref() else {
        return Vec::new();
    };
    let tabs: Vec<&Tab> = session
        .workspace
        .spaces
        .iter()
        .flat_map(|space| &space.tabs)
        .collect();
    model.label_adoptions.retain(|tab, _| {
        tabs.iter()
            .any(|candidate| candidate.id == *tab && is_generated_shell_label(&candidate.label))
    });
    let mut requests = Vec::new();
    for space in &session.workspace.spaces {
        let mut agents = space
            .tabs
            .iter()
            .filter(|tab| is_generated_agent_label(&tab.label))
            .count();
        for tab in &space.tabs {
            if !is_generated_shell_label(&tab.label)
                || agent_identity_for_tab(identities, tab).is_none()
                || model.label_adoptions.contains_key(&tab.id)
            {
                continue;
            }
            agents += 1;
            let label = format!("agent {agents}");
            requests.push(ClientRequest::RenameTab {
                tab: tab.id,
                label: label.clone(),
            });
            model.label_adoptions.insert(tab.id, label);
        }
    }
    requests
}

/// A sidebar action may target an agent in a background space, so its
/// replacement shell must be numbered from that space rather than the local
/// selection.
fn next_shell_label_for_tab(model: &WorkspaceModel, tab: TabId) -> String {
    let count = model
        .session
        .as_ref()
        .and_then(|session| {
            session
                .workspace
                .spaces
                .iter()
                .find(|space| space.tabs.iter().any(|candidate| candidate.id == tab))
        })
        .map_or(0, |space| space.tabs.len());
    format!("shell {}", count + 1)
}

/// The hit zone under a pointer position, latest-drawn first — an overlay
/// row drawn over the sidebar owns the click rather than the row beneath
/// it.
fn hit_at(model: &WorkspaceModel, column: u16, row: u16) -> Option<WorkspaceHit> {
    model
        .hits
        .iter()
        .rev()
        .find(|(rect, _)| {
            rect.x <= column
                && column < rect.x + rect.width
                && rect.y <= row
                && row < rect.y + rect.height
        })
        .map(|(_, hit)| *hit)
}

/// Confirms one [`ContextMenu`] row against its `target` — sent from both
/// the popup's own click zone and its keyboard Enter shortcut, so each
/// [`MenuAction`] only needs writing once here as the menu grows.
fn dispatch_menu_action<W: io::Write>(
    stream: &mut W,
    model: &mut WorkspaceModel,
    identities: &[AgentIdentity],
    target: MenuTarget,
    action: MenuAction,
) {
    match action {
        MenuAction::Rename => begin_rename(model, target),
        MenuAction::Close => match target {
            MenuTarget::Space(space) => {
                let _ = send_request(stream, &ClientRequest::CloseSpace { space });
            }
            MenuTarget::Tab(tab) => {
                if tab_needs_replacement_shell(model, identities, tab) {
                    // The runtime refuses to leave a space without a focused tab.
                    // Select the target first: right-clicking a background agent must
                    // replace it in its own space, not in the currently selected one.
                    let _ = send_request(stream, &ClientRequest::SelectTab { tab });
                    let (columns, rows) = model.last_size;
                    let _ = send_request(
                        stream,
                        &ClientRequest::CreateTab {
                            label: next_shell_label_for_tab(model, tab),
                            // It stands in for the agent rather than
                            // beside it: the agent is on its way out.
                            agent: None,
                            columns,
                            rows,
                            cwd: tab_cwd(model, tab),
                            command: None,
                        },
                    );
                }
                let _ = send_request(stream, &ClientRequest::CloseTab { tab });
            }
        },
    }
}

/// A normal tab can close when it has a sibling. A lone recognized agent is
/// also closable from the sidebar menu: it is replaced by a plain shell so
/// the space remains usable after its process is stopped.
fn can_close_tab_from_menu(
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    tab: TabId,
) -> bool {
    model.session.as_ref().is_some_and(|session| {
        session.workspace.spaces.iter().any(|space| {
            space.tabs.iter().any(|candidate| candidate.id == tab)
                && (space.tabs.len() > 1 || tab_needs_replacement_shell(model, identities, tab))
        })
    })
}

fn tab_needs_replacement_shell(
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    tab: TabId,
) -> bool {
    model.session.as_ref().is_some_and(|session| {
        session.workspace.spaces.iter().any(|space| {
            space.tabs.len() == 1
                && space.tabs.first().is_some_and(|candidate| {
                    candidate.id == tab && agent_identity_for_tab(identities, candidate).is_some()
                })
        })
    })
}

/// Opens the inline rename editor (`WorkspaceModel::renaming`) for `target`,
/// seeded with its current label — shared by the tab-strip/sidebar
/// double-click gesture and the context menu's `rename` row so the lookup
/// only lives once.
fn begin_rename(model: &mut WorkspaceModel, target: MenuTarget) {
    let (rename_target, label) = match target {
        MenuTarget::Tab(tab) => {
            let label = model
                .session
                .as_ref()
                .and_then(|session| {
                    session
                        .workspace
                        .spaces
                        .iter()
                        .flat_map(|space| &space.tabs)
                        .find(|t| t.id == tab)
                })
                .map(|t| t.label.clone())
                .unwrap_or_default();
            (RenameTarget::Tab(tab), label)
        }
        MenuTarget::Space(space) => {
            let label = model
                .session
                .as_ref()
                .and_then(|session| session.workspace.spaces.iter().find(|s| s.id == space))
                .map(|s| s.label.clone())
                .unwrap_or_default();
            (RenameTarget::Space(space), label)
        }
    };
    model.renaming = Some((rename_target, label));
}

fn pane_in_layout(layout: &uze_terminal::Layout, wanted: PaneId) -> Option<&uze_terminal::Pane> {
    match layout {
        uze_terminal::Layout::Pane(pane) if pane.id == wanted => Some(pane),
        uze_terminal::Layout::Pane(_) => None,
        uze_terminal::Layout::Split { first, second, .. } => {
            pane_in_layout(first, wanted).or_else(|| pane_in_layout(second, wanted))
        }
    }
}

/// Delivers the selected tab's task, the way the project's completion says.
/// Nothing to deliver is said, never silently ignored.
fn deliver_selected_tab(
    model: &mut WorkspaceModel,
    home: &UzeHome,
    sender: &mpsc::Sender<DeliveryResolution>,
) {
    let Some(tab) = model.selected_tab() else {
        return;
    };
    let Some(task) = model.tab_task(tab).cloned() else {
        model.set_notice("this tab has no task to deliver".to_owned());
        return;
    };
    if !task.state.is_deliverable() {
        model.set_notice(format!(
            "{}: {}",
            task.label,
            match &task.state {
                TaskStateView::Running => "nothing committed yet",
                TaskStateView::Uncommitted => "uncommitted changes in its checkout",
                TaskStateView::Conflicted { .. } => "a rebase is paused; the agent is on it",
                TaskStateView::Integrating => "already being delivered",
                TaskStateView::Integrated => "already delivered",
                TaskStateView::Closed => "its branch holds nothing",
                TaskStateView::Parked | TaskStateView::Ready | TaskStateView::GateFailed =>
                    "not deliverable",
            }
        ));
        return;
    }
    if !model.delivery_pending.insert(task.id.clone()) {
        return;
    }
    let Some(cwd) = tab_cwd(model, tab) else {
        return;
    };
    model.set_notice(format!("{}: delivering…", task.label));
    spawn_delivery(home, cwd, Some(task.id), sender.clone());
}

/// Ends the tasks whose agent is gone, so their slots can be reused.
///
/// A slot is occupied by the pane sitting in it and by nothing else: the
/// task record alone cannot say whether an agent is still there, and a task
/// that stays live keeps its checkout out of the pool for good. Two things
/// end one — a tab that closed, seen here as a checkout whose last pane is
/// gone, and a session nobody restored, swept once before this client can
/// place its first agent.
///
/// A pane is bound to the checkout it was *first seen* in rather than to
/// wherever it currently sits: an agent that `cd`s out of its own slot has
/// not left it, and must never have it handed to somebody else.
fn sync_slot_occupancy(
    model: &mut WorkspaceModel,
    home: &UzeHome,
    sender: &mpsc::Sender<TaskResolution>,
) {
    let Some(session) = model.session.as_ref() else {
        return;
    };
    let live: Vec<(PaneId, PathBuf)> = session
        .workspace
        .spaces
        .iter()
        .flat_map(|space| &space.tabs)
        .flat_map(|tab| panes_in_layout(&tab.layout))
        .map(|pane| (pane.id, pane.cwd.clone()))
        .collect();
    for (pane, cwd) in &live {
        if let Some(checkout) = uze_application::isolated_checkout(cwd) {
            model
                .pane_checkouts
                .entry(*pane)
                .or_insert_with(|| checkout.directory());
        }
    }
    model
        .pane_checkouts
        .retain(|pane, _| live.iter().any(|(live, _)| live == pane));
    let occupied: BTreeSet<PathBuf> = model.pane_checkouts.values().cloned().collect();
    let vanished: Vec<PathBuf> = model
        .occupied_checkouts
        .difference(&occupied)
        .cloned()
        .collect();
    let sweeping = !model.slots_swept;
    model.slots_swept = true;
    model.occupied_checkouts = occupied.clone();
    if !sweeping && vanished.is_empty() {
        return;
    }
    // A repository is named by any path inside it: the checkout a pane just
    // left, or — for the sweep — every space's own root.
    let mut repositories: Vec<PathBuf> = vanished;
    if sweeping {
        repositories.extend(
            session
                .workspace
                .spaces
                .iter()
                .map(|space| space.root.clone()),
        );
    }
    let Ok(app) = tui_application(home.clone()) else {
        return;
    };
    let held: Vec<PathBuf> = occupied.into_iter().collect();
    let mut seen = BTreeSet::new();
    for cwd in repositories {
        let Some(primary) = app.workspace().primary_of(&cwd) else {
            continue;
        };
        if !seen.insert(primary) {
            continue;
        }
        let released = app.workspace().release_abandoned_tasks(&cwd, &held);
        if let Some(parked) = released.iter().find(|task| task.parked) {
            model.set_notice(format!(
                "{}: parked, its work is preserved (alt+p)",
                parked.label
            ));
        }
        if !released.is_empty() {
            model.schedule_evaluation(home, cwd.clone(), sender);
        }
        // Only ever the removals that cannot lose work, and only from the
        // path that just changed what "in use" means.
        app.workspace().collect_slot_garbage(&cwd);
    }
}

/// Every pane of a layout, in the order they are laid out.
fn panes_in_layout(layout: &uze_terminal::Layout) -> Vec<&uze_terminal::Pane> {
    match layout {
        uze_terminal::Layout::Pane(pane) => vec![pane],
        uze_terminal::Layout::Split { first, second, .. } => {
            let mut panes = panes_in_layout(first);
            panes.extend(panes_in_layout(second));
            panes
        }
    }
}

/// Where a newly created agent starts: the slot the application acquired
/// for it, or — when isolation is impossible — the directory it was created
/// from. The application decides; this only asks from the selected pane.
fn agent_launch_cwd(model: &WorkspaceModel, home: &UzeHome) -> Option<PathBuf> {
    let pane_cwd = selected_pane_cwd(model)?;
    let Ok(app) = tui_application(home.clone()) else {
        return Some(pane_cwd);
    };
    Some(app.workspace().place_new_agent(&pane_cwd).cwd)
}

/// The new-agent picker inherits the selected pane's live directory. The
/// runtime's workspace root is only a fallback for callers that omit it.
fn selected_pane_cwd(model: &WorkspaceModel) -> Option<PathBuf> {
    let session = model.session.as_ref()?;
    let tab = session.selected_tab();
    pane_in_layout(&tab.layout, tab.focus.pane).map(|pane| pane.cwd.clone())
}

fn tab_cwd(model: &WorkspaceModel, tab: TabId) -> Option<PathBuf> {
    let tab = model
        .session
        .as_ref()?
        .workspace
        .spaces
        .iter()
        .find_map(|space| space.tabs.iter().find(|candidate| candidate.id == tab))?;
    pane_in_layout(&tab.layout, tab.focus.pane).map(|pane| pane.cwd.clone())
}

/// Flips one space's header between its label and its root. Purely local
/// state, no server round trip to eventually mark the model dirty — same
/// as `OpenStatusCatalog`.
fn toggle_space_root(model: &mut WorkspaceModel, space: SpaceId) {
    if !model.roots_shown.remove(&space) {
        model.roots_shown.insert(space);
    }
    model.dirty = true;
}

/// Opens the git changes overlay scoped to the *currently selected tab's*
/// live `cwd` — the hierarchy the user gave for this feature is
/// `Workspace > Space > Agent/Shell > Git`, one level further down than
/// the space itself. Snapshotted once here; the view doesn't track further
/// `cd`s in that tab while it's open (see `git_diff`'s own module doc).
fn open_git_view(model: &mut WorkspaceModel) {
    let Some(session) = model.session.as_ref() else {
        return;
    };
    let tab = session.selected_tab();
    let Some(pane) = pane_in_layout(&tab.layout, tab.focus.pane) else {
        return;
    };
    model.git_view = Some(git_diff::GitView::open(&WorkspaceHost, pane.cwd.clone()));
    model.dirty = true;
}

fn io_error(source: io::Error) -> UzeError {
    UzeError::Write {
        path: "terminal".into(),
        source,
    }
}
fn runtime_error(error: uze_terminal::RuntimeError) -> UzeError {
    UzeError::AcquisitionFailed(error.to_string())
}

#[cfg(test)]
mod tests;
