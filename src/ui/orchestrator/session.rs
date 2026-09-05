//! One attach, and what it does with an event.
//!
//! Split out of `orchestrator.rs`'s `attach_workspace`, which had grown to
//! ~1.5k lines carrying five jobs at once: the server handshake, the frame
//! loop, the cadence of every background read, slot lifecycle, and a
//! 46-arm `match` over every key and click the workspace understands.
//!
//! Two of those live here — [`Attach::pump`], which absorbs what the
//! server and the background reads have said and asks for whatever has
//! gone stale, and [`Attach::handle`], which answers one event. [`Attach`]
//! itself is the state both needed: the model, the connection it drives
//! the server through, and the channels its reads answer on, so a handler
//! reaches for a field instead of closing over a local and the loop that
//! calls them fits on a screen.
//!
//! # Why one match became several
//!
//! The arms were never mixed: a `Event::Key(_) if …` guard can only ever
//! match a key, and every mouse arm tested exactly one `MouseEventKind`.
//! Splitting by event kind is therefore exact rather than a judgement
//! call, and it leaves the thing the guards *do* encode legible — modal
//! precedence. In [`Attach::key`] and [`Attach::press`] the order of the
//! guards is the order overlays stack in, and each overlay's own keys
//! live in a method named after it, so those two lists are the precedence
//! and nothing else. `WorkspaceModel::no_modal_open` names the same set
//! from one place.

use super::*;

/// Where an event leaves the loop.
pub(super) enum Flow {
    /// Keep going — almost everything.
    Continue,
    /// Ctrl+O to management, or Ctrl+Q out.
    Exit(WorkspaceExit),
}

/// The channels one attach's background reads answer on.
///
/// Cloned senders, not the receivers: the receivers stay with
/// [`WorkspaceMemory`] so an answer still in flight when the user leaves
/// for management lands when they come back.
pub(super) struct AttachAnswers {
    pub(super) support: mpsc::Sender<SupportResolution>,
    pub(super) tasks: mpsc::Sender<TaskResolution>,
    pub(super) deliveries: mpsc::Sender<DeliveryResolution>,
    pub(super) git: mpsc::Sender<GitResolution>,
    pub(super) commit_details: mpsc::Sender<CommitDetailResolution>,
    pub(super) git_views: mpsc::Sender<GitViewResolution>,
    pub(super) occupancy: mpsc::Sender<OccupancyResolution>,
    pub(super) placements: mpsc::Sender<PlacementResolution>,
}

/// The frame an event is handled against.
///
/// Recomputed once per iteration, before any event is read, so a resize
/// that arrived in the same tick is already accounted for — several arms
/// size a PTY from it, and sizing one from a stale layout is how a pane
/// ends up drawn at one size and running at another.
pub(super) struct Viewport {
    pub(super) size: ratatui::layout::Size,
    pub(super) layout: WorkspaceLayout,
    pub(super) columns: u16,
    pub(super) rows: u16,
}

/// Everything one attach holds while its loop runs.
pub(super) struct Attach<'a> {
    pub(super) model: WorkspaceModel,
    pub(super) stream: std::os::unix::net::UnixStream,
    pub(super) home: &'a UzeHome,
    /// The registered harness set, resolved once per attach — it cannot
    /// change mid-session.
    pub(super) identities: Vec<AgentIdentity>,
    pub(super) answers: AttachAnswers,
    /// Drives the agent-activity animation. Ratatui owns the alternate
    /// screen, so this one is hidden and only its position is read — see
    /// [`AGENT_ACTIVITY_FRAMES`].
    pub(super) spinner: ProgressBar,
    pub(super) next_tick: Instant,
}

impl Attach<'_> {
    /// Routes one event to the half of the client that owns it.
    pub(super) fn handle(&mut self, event: Event, viewport: &Viewport) -> Flow {
        match event {
            Event::Key(key) => self.key(key, viewport),
            Event::Paste(text) => self.paste(text),
            Event::Mouse(mouse) => self.mouse(mouse, viewport),
            _ => Flow::Continue,
        }
    }

    /// Keyboard input, guards in modal-precedence order: the innermost
    /// open overlay answers first, and the pane itself answers last.
    /// Keyboard input, guards in modal-precedence order: the innermost
    /// open overlay answers first, and the pane itself answers last.
    ///
    /// Each overlay's own keys live in a method named after it, so this
    /// list is the precedence and nothing else — the one thing about it
    /// that is easy to get wrong by inserting an arm in the wrong place.
    fn key(&mut self, key: KeyEvent, viewport: &Viewport) -> Flow {
        let Viewport { columns, rows, .. } = *viewport;
        match key {
            _ if self.model.root_picker.is_some() => {
                self.root_picker_key(key, viewport);
            }
            _ if self.model.renaming.is_some() => {
                self.rename_key(key);
            }
            _ if self.model.agent_picker.is_some() => {
                self.agent_picker_key(key, viewport);
            }
            _ if self.model.preserved.is_some() => {
                self.preserved_key(key);
            }
            _ if self.model.support_dropdown.is_some() => {
                self.model.support_dropdown = None;
                self.model.dirty = true;
            }
            _ if self.model.commit_detail_open() => {
                self.model.dismiss_commit_detail();
            }
            _ if self.model.status_catalog.is_some() => {
                self.model.status_catalog = None;
                self.model.dirty = true;
            }
            _ if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('i') => {
                deliver_selected_tab(&mut self.model, self.home, &self.answers.deliveries);
            }
            _ if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('I') => {
                if let Some(cwd) = selected_pane_cwd(&self.model) {
                    spawn_delivery(self.home, cwd, None, self.answers.deliveries.clone());
                    self.model.set_busy_notice("delivering all".to_owned());
                }
            }
            _ if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('p') => {
                self.model.preserved = match self.model.preserved {
                    Some(_) => None,
                    None => Some(PreservedOverlay {
                        selected: 0,
                        confirm_discard: false,
                    }),
                };
                self.model.dirty = true;
            }
            _ if self.model.context_menu.is_some() => {
                self.context_menu_key(key);
            }
            _ if self.model.git_view.is_some() => {
                self.git_view_key(key);
            }
            _ if key.modifiers.contains(KeyModifiers::CONTROL)
                && key.code == KeyCode::Char('o') =>
            {
                let _ = send_request(&mut self.stream, &ClientRequest::Detach);
                return Flow::Exit(WorkspaceExit::Management);
            }
            _ if key.modifiers.contains(KeyModifiers::CONTROL)
                && key.code == KeyCode::Char('q') =>
            {
                let _ = send_request(&mut self.stream, &ClientRequest::Detach);
                return Flow::Exit(WorkspaceExit::Quit);
            }
            _ if key.modifiers.contains(KeyModifiers::CONTROL)
                && key.code == KeyCode::Char('t') =>
            {
                let _ = send_request(
                    &mut self.stream,
                    &ClientRequest::CreateTab {
                        label: next_shell_label(&self.model, &self.identities),
                        agent: context_agent(&self.model, &self.identities),
                        columns,
                        rows,
                        cwd: new_shell_cwd(&self.model, &self.identities),
                        command: None,
                    },
                );
            }
            _ if key.modifiers.contains(KeyModifiers::CONTROL)
                && key.code == KeyCode::Char('w') =>
            {
                if let Some(tab) = self.model.selected_tab() {
                    let _ = send_request(&mut self.stream, &ClientRequest::CloseTab { tab });
                }
            }
            _ if key.modifiers.contains(KeyModifiers::CONTROL)
                && key.code == KeyCode::Char('g') =>
            {
                open_git_view(&mut self.model);
            }
            _ if key.modifiers.contains(KeyModifiers::ALT)
                && matches!(key.code, KeyCode::Char('1'..='9')) =>
            {
                let index = match key.code {
                    KeyCode::Char(value) => value as usize - '1' as usize,
                    _ => 0,
                };
                if let Some(tab) = self
                    .model
                    .session
                    .as_ref()
                    .and_then(|session| session.selected_space().tabs.get(index))
                    .map(|tab| tab.id)
                {
                    self.model.acknowledge_completed_agent_tab(tab);
                    let _ = send_request(&mut self.stream, &ClientRequest::SelectTab { tab });
                    if let Some(pane) = self.model.pane_for_tab(tab) {
                        resize_pane(&mut self.stream, &mut self.model, pane, columns, rows);
                    }
                }
            }
            _ => {
                self.pane_key(key);
            }
        }
        Flow::Continue
    }

    /// The "+ new space" prompt: type to narrow, Tab to walk into the
    /// highlighted directory, Enter to open a space at it.
    fn root_picker_key(&mut self, key: KeyEvent, viewport: &Viewport) {
        let Viewport { columns, rows, .. } = *viewport;
        match key.code {
            KeyCode::Up => {
                if let Some(picker) = self.model.root_picker.as_mut() {
                    picker.move_selection(-1);
                }
            }
            KeyCode::Down => {
                if let Some(picker) = self.model.root_picker.as_mut() {
                    picker.move_selection(1);
                }
            }
            // Tab walks into the highlighted directory, so a
            // root several levels down is reached by narrowing
            // one level at a time instead of typing the path.
            KeyCode::Tab => {
                if let Some(picker) = self.model.root_picker.as_mut() {
                    picker.descend();
                }
            }
            KeyCode::Enter => {
                if let Some(root) = self.model.root_picker.as_ref().and_then(RootPicker::chosen) {
                    self.model.root_picker = None;
                    self.open_space_at(root, columns, rows);
                }
            }
            KeyCode::Esc => self.model.root_picker = None,
            KeyCode::Backspace => {
                if let Some(picker) = self.model.root_picker.as_mut() {
                    picker.backspace();
                }
            }
            KeyCode::Char(character) => {
                if let Some(picker) = self.model.root_picker.as_mut() {
                    picker.typed(character);
                }
            }
            _ => {}
        }
        self.model.dirty = true;
    }

    /// The inline rename buffer over a tab or a space label.
    fn rename_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                if let Some((target, buffer)) = self.model.renaming.take() {
                    let trimmed = buffer.trim().to_owned();
                    if !trimmed.is_empty() {
                        let _ = send_request(
                            &mut self.stream,
                            &match target {
                                RenameTarget::Tab(tab) => ClientRequest::RenameTab {
                                    tab,
                                    label: trimmed,
                                },
                                RenameTarget::Space(space) => ClientRequest::RenameSpace {
                                    space,
                                    label: trimmed,
                                },
                            },
                        );
                    }
                }
            }
            KeyCode::Esc => self.model.renaming = None,
            KeyCode::Backspace => {
                if let Some((_, buffer)) = self.model.renaming.as_mut() {
                    buffer.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some((_, buffer)) = self.model.renaming.as_mut() {
                    buffer.push(c);
                }
            }
            _ => {}
        }
        self.model.dirty = true;
    }

    /// The "+ new agent" popup — pick a harness, or Esc.
    fn agent_picker_key(&mut self, key: KeyEvent, viewport: &Viewport) {
        let Viewport { columns, rows, .. } = *viewport;
        match key.code {
            KeyCode::Up => {
                if let Some(picker) = self.model.agent_picker.as_mut() {
                    picker.selected = picker.selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(picker) = self.model.agent_picker.as_mut() {
                    picker.selected =
                        (picker.selected + 1).min(picker.options.len().saturating_sub(1));
                }
            }
            KeyCode::Enter => {
                if let Some(picker) = self.model.agent_picker.take()
                    && let Some(option) = picker.options.get(picker.selected)
                {
                    let label = next_agent_label(&self.model);
                    let command = option.command.clone();
                    self.launch_agent(
                        label,
                        command,
                        picker.cwd.clone(),
                        picker.resume.clone(),
                        (columns, rows),
                    );
                }
            }
            // Esc, or anything else — the picker only reacts to
            // Up/Down/Enter, so any other key just dismisses it
            // rather than leaking through to the pane.
            _ => self.model.agent_picker = None,
        }
        self.model.dirty = true;
    }

    /// The preserved-work list: tasks holding work no live tab is in
    /// front of, with resume and a confirmed discard.
    fn preserved_key(&mut self, key: KeyEvent) {
        let preserved = self.model.preserved_tasks();
        let overlay = self.model.preserved.as_mut().expect("guarded");
        match key.code {
            KeyCode::Esc => self.model.preserved = None,
            KeyCode::Up => {
                overlay.selected = overlay.selected.saturating_sub(1);
                overlay.confirm_discard = false;
            }
            KeyCode::Down => {
                overlay.selected = (overlay.selected + 1).min(preserved.len().saturating_sub(1));
                overlay.confirm_discard = false;
            }
            KeyCode::Char('i') => {
                if let Some((cwd, task)) = preserved.get(overlay.selected) {
                    self.model.delivery_pending.insert(task.id.clone());
                    spawn_delivery(
                        self.home,
                        cwd.clone(),
                        Some(task.id.clone()),
                        self.answers.deliveries.clone(),
                    );
                }
            }
            KeyCode::Char('f') => {
                if let Some((cwd, task)) = preserved.get(overlay.selected)
                    && let Ok(app) = tui_application(self.home.clone())
                {
                    let _ = app.workspace().finish_task(cwd, &task.id);
                    self.model
                        .schedule_evaluation(self.home, cwd.clone(), &self.answers.tasks);
                }
            }
            // Into the task's own slot when it still has one; otherwise
            // placement gives it a slot again on its own branch — a
            // checkout removed by hand took only the uncommitted work.
            KeyCode::Char('r') => {
                if let Some((primary, task)) = preserved.get(overlay.selected) {
                    let (cwd, resume) = match task.checkout.clone() {
                        Some(checkout) => (Some(checkout), None),
                        None => (
                            None,
                            Some(ResumeTarget {
                                primary: primary.clone(),
                                task: task.id.clone(),
                                // Asked for from the list, not from a
                                // row: there is no dead tab behind it.
                                replacing: None,
                            }),
                        ),
                    };
                    self.model.preserved = None;
                    self.model.agent_picker = Some(AgentPicker {
                        options: agent_options(self.home),
                        selected: 0,
                        anchor: Rect::default(),
                        cwd,
                        resume,
                    });
                }
            }
            // Discard is the one action that deletes work, so
            // it is the one that asks twice.
            KeyCode::Char('d') => overlay.confirm_discard = true,
            KeyCode::Char('y') if overlay.confirm_discard => {
                overlay.confirm_discard = false;
                if let Some((cwd, task)) = preserved.get(overlay.selected)
                    && let Ok(app) = tui_application(self.home.clone())
                {
                    match app.workspace().discard_task(cwd, &task.id) {
                        Ok(()) => {
                            self.model.set_notice(format!("{}: discarded", task.label));
                        }
                        Err(error) => self.model.set_notice(error.to_string()),
                    }
                    self.model
                        .schedule_evaluation(self.home, cwd.clone(), &self.answers.tasks);
                }
            }
            _ => overlay.confirm_discard = false,
        }
        self.model.dirty = true;
    }

    /// The tab/space context menu.
    fn context_menu_key(&mut self, key: KeyEvent) {
        // Up/Down move the selection; Enter confirms whichever
        // row is selected; anything else (Esc included)
        // dismisses without acting — same "only reacts to its
        // own actions" rule the agent picker above uses.
        match key.code {
            KeyCode::Up => {
                if let Some(menu) = self.model.context_menu.as_mut() {
                    menu.selected = menu.selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(menu) = self.model.context_menu.as_mut() {
                    menu.selected = (menu.selected + 1).min(menu.items.len().saturating_sub(1));
                }
            }
            KeyCode::Enter => {
                if let Some(menu) = self.model.context_menu.take()
                    && let Some(action) = menu.items.get(menu.selected).copied()
                {
                    dispatch_menu_action(
                        &mut self.stream,
                        &mut self.model,
                        &self.identities,
                        menu.target,
                        action,
                    );
                }
            }
            _ => self.model.context_menu = None,
        }
        self.model.dirty = true;
    }

    /// The Git changes overlay, which answers for itself — this only
    /// learns whether it wants to stay open.
    fn git_view_key(&mut self, key: KeyEvent) {
        if let Some(view) = self.model.git_view.as_mut()
            && matches!(git::handle_key(view, key), git::GitViewOutcome::Close)
        {
            self.model.git_view = None;
        }
        self.model.dirty = true;
    }

    /// Nothing of uze's is open, so the key belongs to the pane: encode
    /// it for the PTY and record what it does to the prompt buffer.
    fn pane_key(&mut self, key: KeyEvent) {
        if let Some(bytes) = encode_key(key) {
            let pane = self.model.focused_pane();
            // `encode_key` emits a bare CR for Enter and 0x03
            // for Ctrl+C, so these are exact byte comparisons
            // rather than a substring scan that a pasted or
            // multi-byte sequence could trip.
            let submitted = bytes.as_slice() == *b"\r";
            let cancelled = bytes.as_slice() == [3u8];
            let prompt = if submitted {
                self.model.prompt_buffers.entry(pane).or_default().submit()
            } else {
                if cancelled {
                    self.model.prompt_buffers.remove(&pane);
                } else {
                    self.model
                        .prompt_buffers
                        .entry(pane)
                        .or_default()
                        .apply(key);
                }
                None
            };
            // Forwarded before anything is recorded: the pane's
            // own responsiveness must never wait on history.
            let _ = send_request(&mut self.stream, &ClientRequest::Input { pane, bytes });
            self.model.note_pane_input(pane);
            if submitted {
                self.model
                    .note_agent_prompt_submission(pane, &self.identities, prompt.as_deref());
            }
        }
    }

    /// Bracketed paste. Only three surfaces take one: the root picker, a
    /// rename buffer, and — with nothing open — the focused pane.
    fn paste(&mut self, text: String) -> Flow {
        match text {
            _ if self.model.root_picker.is_some() => {
                if let Some(picker) = self.model.root_picker.as_mut() {
                    picker.pasted(text.trim_end_matches(['\r', '\n']));
                }
                self.model.dirty = true;
            }
            _ if self.model.renaming.is_some() => {
                if let Some((_, buffer)) = self.model.renaming.as_mut() {
                    buffer.push_str(text.trim_end_matches(['\r', '\n']));
                }
                self.model.dirty = true;
            }
            _ if self.model.no_modal_open() => {
                let pane = self.model.focused_pane();
                self.model
                    .prompt_buffers
                    .entry(pane)
                    .or_default()
                    .paste(&text);
                forward_paste(&mut self.stream, &self.model, &text);
                self.model.note_pane_paste(pane);
            }
            _ => {}
        }
        Flow::Continue
    }

    /// Clicks, drags and wheels. The same precedence the keyboard has,
    /// plus the hit list the last frame left behind
    /// (`WorkspaceModel::hits`) for everything that resolves to chrome.
    /// Clicks, drags and wheels, routed by button and kind.
    ///
    /// The kinds partition the arms exactly — no guard ever tested two —
    /// so what used to be one 26-arm match is six matches whose *name*
    /// says which gesture they answer, and whose guards say only what is
    /// open.
    fn mouse(&mut self, mouse: MouseEvent, viewport: &Viewport) -> Flow {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => self.press(mouse, viewport),
            MouseEventKind::Drag(MouseButton::Left) => self.drag(mouse, viewport),
            MouseEventKind::Up(MouseButton::Left) => self.release(mouse, viewport),
            MouseEventKind::Down(MouseButton::Right) => self.open_context_menu(mouse),
            MouseEventKind::Moved => self.hover(mouse),
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => self.wheel(mouse, viewport),
            _ => Flow::Continue,
        }
    }

    /// Opens a space at `root` — the one thing both ways of picking a
    /// directory (Enter on the prompt, a click on one of its rows) do.
    ///
    /// A directory another space already holds is opened all the same:
    /// the server numbers the repeated name rather than refusing (see
    /// `Session::create_space`), because one repository is routinely
    /// worth two spaces and the prompt is an explicit request for one.
    fn open_space_at(&mut self, root: PathBuf, columns: u16, rows: u16) {
        let _ = send_request(
            &mut self.stream,
            &ClientRequest::CreateSpace {
                label: None,
                root,
                columns,
                rows,
            },
        );
    }

    /// A left button going down.
    ///
    /// Guards first, in the same modal-precedence order the keyboard has:
    /// a click outside an open overlay discards it rather than reaching
    /// the chrome underneath. What is left resolves against the hit list
    /// the last frame drew (`WorkspaceModel::hits`).
    fn press(&mut self, mouse: MouseEvent, viewport: &Viewport) -> Flow {
        let Viewport {
            ref layout,
            columns,
            rows,
            ..
        } = *viewport;
        match mouse {
            _ if self.model.renaming.is_some() => {
                // Same rule the management TUI's overlays use: a click
                // outside the thing being edited discards it rather
                // than silently confirming or acting on the click.
                self.model.renaming = None;
                self.model.dirty = true;
            }
            _ if self.model.root_picker.is_some() => {
                match hit_at(&self.model, mouse.column, mouse.row) {
                    Some(WorkspaceHit::PickSpaceRoot(index)) => {
                        if let Some(root) = self.model.root_picker.as_mut().and_then(|picker| {
                            picker.select(index);
                            picker.chosen()
                        }) {
                            self.model.root_picker = None;
                            self.open_space_at(root, columns, rows);
                        }
                    }
                    // Click outside the picker's own rows discards it —
                    // same rule `renaming` uses.
                    _ => self.model.root_picker = None,
                }
                self.model.dirty = true;
            }
            _ if self.model.agent_picker.is_some() => {
                // `hit_at`, not the tree's own first-rect search: the
                // picker is drawn last and hangs over whatever asked for
                // it — the sidebar row a "resume" was clicked on, with
                // rows of its own under half of every option. Its hover
                // half already reads the last rect; a click that read the
                // first one landed on the tree beneath instead, and the
                // picker closed having launched nothing.
                match hit_at(&self.model, mouse.column, mouse.row) {
                    Some(WorkspaceHit::PickAgent(index)) => {
                        if let Some(picker) = self.model.agent_picker.take()
                            && let Some(option) = picker.options.get(index)
                        {
                            let label = next_agent_label(&self.model);
                            let command = option.command.clone();
                            self.launch_agent(
                                label,
                                command,
                                picker.cwd.clone(),
                                picker.resume.clone(),
                                (columns, rows),
                            );
                        }
                    }
                    // Click outside the picker's own rows discards it —
                    // same rule `renaming` uses.
                    _ => self.model.agent_picker = None,
                }
                self.model.dirty = true;
            }
            _ if self.model.support_dropdown.is_some() => {
                // Informational dropdown: every click simply dismisses
                // it, preventing the click from leaking into the pane.
                self.model.support_dropdown = None;
                self.model.dirty = true;
            }
            _ if self.model.commit_detail_open() => {
                // Informational, like the support dropdown: any click
                // dismisses it rather than leaking into the pane.
                self.model.dismiss_commit_detail();
            }
            _ if self.model.status_catalog.is_some() => {
                // Informational, like the support dropdown: any click
                // dismisses it rather than leaking into the pane.
                self.model.status_catalog = None;
                self.model.dirty = true;
            }
            _ if self.model.context_menu.is_some() => {
                // `.rev()`: the popup renders last, so its own rows sit
                // at the tail of `hits` — searching forward could match
                // an older, now visually-covered sidebar row underneath
                // it instead (the tight, gapless sidebar packing meant
                // this landed on a covered row far more often than not,
                // which is what made the popup's own click feel
                // intermittent — it depended on which row was
                // right-clicked, not on timing).
                let hit = self
                    .model
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
                    Some(WorkspaceHit::ContextMenuAction(index)) => self
                        .model
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
                let target = self.model.context_menu.take().map(|menu| menu.target);
                if let (Some(target), Some(action)) = (target, action) {
                    dispatch_menu_action(
                        &mut self.stream,
                        &mut self.model,
                        &self.identities,
                        target,
                        action,
                    );
                }
                self.model.dirty = true;
            }
            _ if self.model.git_view.is_some() => {
                let hit = self
                    .model
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
                // dragging instead of reaching `git::handle_mouse`,
                // which only knows about `ExtensionHit`s that are its
                // own — the resize handle's drag lifecycle belongs to
                // this workspace client, not the extension.
                let view_hit = match hit {
                    Some(WorkspaceHit::Extension(ExtensionHit::Git(view_hit))) => Some(view_hit),
                    _ => None,
                };
                if view_hit == Some(ViewHit::ResizeNavigator) {
                    self.model.dragging_git_tree = true;
                } else if let Some(view) = self.model.git_view.as_mut()
                    && matches!(
                        git::handle_mouse(view, view_hit),
                        git::GitViewOutcome::Close
                    )
                {
                    self.model.git_view = None;
                }
                self.model.dirty = true;
            }
            _ => {
                let Some((hit_rect, hit)) = self.model.hit_rect_at(mouse.column, mouse.row) else {
                    self.model.last_click = None;
                    forward_mouse(&mut self.stream, &self.model, layout.pane, mouse);
                    return Flow::Continue;
                };
                let now = std::time::Instant::now();
                let is_double_click = self.model.last_click.is_some_and(|(at, previous)| {
                    previous == hit && now.duration_since(at) < DOUBLE_CLICK_WINDOW
                });
                self.model.last_click = Some((now, hit));
                if is_double_click {
                    self.model.last_click = None;
                    self.double_click(hit);
                    return Flow::Continue;
                }
                return self.click(hit, hit_rect, mouse, viewport);
            }
        }
        Flow::Continue
    }

    /// A left button held and moved: the sidebar's edge, the timeline's
    /// divider, the git overlay's navigator split, and a tab being carried
    /// to a new position.
    fn drag(&mut self, mouse: MouseEvent, viewport: &Viewport) -> Flow {
        let Viewport {
            size, ref layout, ..
        } = *viewport;
        match mouse {
            _ if self.model.dragging_git_tree => {
                let frame_area = Rect::new(0, 0, size.width, size.height);
                let (tree_column, diff_column, _footer) =
                    crate::ui::extension_view::content_columns(
                        frame_area,
                        self.model.git_tree_width,
                    );
                let new_width = crate::ui::extension_view::clamp_navigator_width(
                    mouse.column.saturating_sub(tree_column.x),
                    tree_column.width + diff_column.width,
                );
                if self.model.git_tree_width != Some(new_width) {
                    self.model.git_tree_width = Some(new_width);
                    self.model.dirty = true;
                }
            }
            _ if self.model.dragging_timeline => {
                // The divider follows the pointer; what is remembered
                // is the commit rows that leaves under it, never fewer
                // than one — folding is the header's own click, not a
                // drag to nothing.
                let wanted = layout
                    .sidebar
                    .bottom()
                    .saturating_sub(mouse.row)
                    .saturating_sub(render::TIMELINE_CHROME - 1)
                    .clamp(1, TIMELINE_COMMITS as u16);
                if self.model.timeline_rows != Some(wanted) {
                    self.model.timeline_rows = Some(wanted);
                    self.model.dirty = true;
                }
            }
            _ if self.model.dragging_sidebar => {
                let new_width = crate::ui::clamp_sidebar_width(
                    mouse.column.saturating_sub(layout.sidebar.x),
                    size.width,
                );
                if self.model.sidebar_width != Some(new_width) {
                    self.model.sidebar_width = Some(new_width);
                    self.model.dirty = true;
                }
            }
            _ if self.model.dragging_tab.is_some() => {
                let Some(mut dragging) = self.model.dragging_tab else {
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
                            &self.model,
                            &self.identities,
                            layout,
                            dragging.group,
                        )
                        .into_iter()
                        .filter(|(_, tab)| *tab != dragging.tab)
                        .collect::<Vec<_>>();
                        pending_tab_drop(&members, dragging.group, pointer, dragging.origin)
                    })
                    .flatten();
                self.model.dragging_tab = Some(dragging);
                self.model.dirty = true;
            }
            _ if !self.model.dragging_sidebar
                && !self.model.dragging_git_tree
                && !self.model.dragging_timeline
                && self.model.dragging_tab.is_none()
                && self.model.no_modal_open() =>
            {
                forward_mouse(&mut self.stream, &self.model, layout.pane, mouse);
            }
            _ => {}
        }
        Flow::Continue
    }

    /// A left button coming up: every drag this client understands ends
    /// here, and a tab carried far enough lands where the indicator said
    /// it would.
    fn release(&mut self, mouse: MouseEvent, viewport: &Viewport) -> Flow {
        let Viewport { ref layout, .. } = *viewport;
        // A drag this client never owned (no flag was set, no
        // tab drag was in progress, and nothing modal was open
        // to have owned it either) is one it was forwarding
        // into the pane above — the matching release belongs
        // there too, not just silently dropped the way it was
        // before pane forwarding existed.
        if !self.model.dragging_sidebar
            && !self.model.dragging_git_tree
            && !self.model.dragging_timeline
            && self.model.dragging_tab.is_none()
            && self.model.no_modal_open()
        {
            forward_mouse(&mut self.stream, &self.model, layout.pane, mouse);
        }
        if let Some(dragging) = self.model.dragging_tab.take()
            && let Some(pending) = dragging.pending
        {
            let _ = send_request(
                &mut self.stream,
                &ClientRequest::ReorderTab {
                    tab: dragging.tab,
                    before: pending.as_before(),
                },
            );
        }
        if self.model.dragging_timeline || self.model.dragging_sidebar {
            // Where the drag settled is the user's answer, kept for the
            // next run; the widths and heights it passed through on the
            // way there are not, which is why this is the release rather
            // than the motion above.
            self.model.remember_sidebar();
        }
        self.model.dragging_sidebar = false;
        self.model.dragging_git_tree = false;
        self.model.dragging_timeline = false;
        self.model.dirty = true;
        Flow::Continue
    }

    /// The right button: the tab/space context menu, anchored where it
    /// was asked for.
    fn open_context_menu(&mut self, mouse: MouseEvent) -> Flow {
        match mouse {
            _ if self.model.renaming.is_none()
                && self.model.root_picker.is_none()
                && self.model.agent_picker.is_none()
                && self.model.context_menu.is_none() =>
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
                let hit = self
                    .model
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
                    if self
                        .model
                        .session
                        .as_ref()
                        .is_some_and(|session| session.workspace.spaces.len() > 1)
                    {
                        items.push(MenuAction::Close);
                    }
                    self.model.context_menu = Some(ContextMenu {
                        target: MenuTarget::Space(space),
                        items,
                        selected: 0,
                        anchor,
                    });
                    self.model.dirty = true;
                } else if let Some(WorkspaceHit::SelectTab(tab)) = hit {
                    let mut items = vec![MenuAction::Rename];
                    if can_close_tab_from_menu(&self.model, &self.identities, tab) {
                        items.push(MenuAction::Close);
                    }
                    self.model.context_menu = Some(ContextMenu {
                        target: MenuTarget::Tab(tab),
                        items,
                        selected: 0,
                        anchor,
                    });
                    self.model.dirty = true;
                }
            }
            _ => {}
        }
        Flow::Continue
    }

    /// Pointer motion. Only the three list-shaped overlays follow it —
    /// moving the highlight under the pointer is what makes them read as
    /// menus rather than as keyboard-only lists.
    fn hover(&mut self, mouse: MouseEvent) -> Flow {
        match mouse {
            _ if self.model.root_picker.is_some() => {
                // The highlight follows the pointer, the same way the
                // agent picker and the sidebar context menu already do.
                if let Some(WorkspaceHit::PickSpaceRoot(index)) =
                    hit_at(&self.model, mouse.column, mouse.row)
                    && let Some(picker) = self.model.root_picker.as_mut()
                    && picker.selected() != index
                {
                    picker.select(index);
                    self.model.dirty = true;
                }
            }
            _ if self.model.agent_picker.is_some() => {
                // Keep this dropdown's pointer behavior aligned with the
                // sidebar context menu: the highlighted option follows
                // the cursor, while keyboard navigation remains intact.
                let hit = self
                    .model
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
                    && let Some(picker) = self.model.agent_picker.as_mut()
                    && picker.selected != index
                {
                    picker.selected = index;
                    self.model.dirty = true;
                }
            }
            _ if self.model.context_menu.is_some() => {
                // Hovering a row selects it, same as Up/Down — so the
                // popup reads as a real menu (highlight follows the
                // cursor) instead of only reacting to a click. Only
                // marks the frame dirty when the hover actually moved
                // onto a different row, so waving the mouse across the
                // rest of the screen doesn't force a redraw every tick.
                let hit = self
                    .model
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
                    && let Some(menu) = self.model.context_menu.as_mut()
                    && menu.selected != index
                {
                    menu.selected = index;
                    self.model.dirty = true;
                }
            }
            _ => {}
        }
        Flow::Continue
    }

    /// The wheel, routed by *where the pointer is* rather than by what
    /// holds keyboard focus — the same rule everything else with two
    /// scrollable halves uses.
    fn wheel(&mut self, mouse: MouseEvent, viewport: &Viewport) -> Flow {
        let Viewport {
            size, ref layout, ..
        } = *viewport;
        match mouse {
            _ if self.model.git_view.is_some() => {
                if let Some(view) = self.model.git_view.as_mut()
                    && let Some(target) = crate::ui::extension_view::scroll_target(
                        Rect::new(0, 0, size.width, size.height),
                        self.model.git_tree_width,
                        mouse.column,
                        mouse.row,
                    )
                {
                    git::handle_scroll(
                        view,
                        target,
                        if mouse.kind == MouseEventKind::ScrollUp {
                            ScrollDirection::Up
                        } else {
                            ScrollDirection::Down
                        },
                    );
                }
                self.model.dirty = true;
            }
            _ if self.model.commit_detail.is_some() => {
                if let Some(popup) = self.model.commit_detail.as_mut() {
                    let limit = render::commit_detail_layout(
                        Rect::new(0, 0, size.width, size.height),
                        popup,
                    )
                    .scroll_limit();
                    popup.scroll = if mouse.kind == MouseEventKind::ScrollUp {
                        popup.scroll.saturating_sub(1)
                    } else {
                        popup.scroll.saturating_add(1).min(limit)
                    };
                    self.model.dirty = true;
                }
            }
            _ if self.model.no_modal_open()
                && self.model.over_timeline(mouse.column, mouse.row) =>
            {
                scroll_timeline(
                    &mut self.model,
                    if mouse.kind == MouseEventKind::ScrollUp {
                        ScrollDirection::Up
                    } else {
                        ScrollDirection::Down
                    },
                );
            }
            // Anywhere else in the sidebar scrolls the space tree — the
            // timeline section took the wheel over itself in the branch
            // above, and the tree is the rest of that column.
            _ if self.model.no_modal_open() && mouse.column < layout.sidebar.right() => {
                scroll_tree(
                    &mut self.model,
                    if mouse.kind == MouseEventKind::ScrollUp {
                        ScrollDirection::Up
                    } else {
                        ScrollDirection::Down
                    },
                );
            }
            _ if self.model.no_modal_open() => {
                forward_scroll(&mut self.stream, &self.model, layout.pane, mouse);
            }
            _ => {}
        }
        Flow::Continue
    }

    /// What a second click inside [`DOUBLE_CLICK_WINDOW`] means. Only a
    /// few hits have an answer; the rest fall through to nothing rather
    /// than repeating the single-click one.
    fn double_click(&mut self, hit: WorkspaceHit) {
        match hit {
            WorkspaceHit::SelectTab(tab) => {
                begin_rename(&mut self.model, MenuTarget::Tab(tab));
                self.model.dirty = true;
            }
            WorkspaceHit::SelectSpace(space) => {
                begin_rename(&mut self.model, MenuTarget::Space(space));
                self.model.dirty = true;
            }
            // Two quick clicks on the toggle are two
            // toggles, not a gesture of their own.
            WorkspaceHit::ToggleSpaceRoot(space) => {
                toggle_space_root(&mut self.model, space);
            }
            WorkspaceHit::Extension(ExtensionHit::GitTimeline(ViewHit::ToggleSection)) => {
                toggle_timeline(&mut self.model);
            }
            _ => {}
        }
    }

    /// What one click on a piece of workspace chrome does.
    ///
    /// `hit_rect` is the rectangle the last frame drew that hit into —
    /// several answers anchor a popup to it, which is why the geometry
    /// travels with the hit instead of being re-derived here.
    fn click(
        &mut self,
        hit: WorkspaceHit,
        hit_rect: Rect,
        mouse: MouseEvent,
        viewport: &Viewport,
    ) -> Flow {
        let Viewport {
            ref layout,
            columns,
            rows,
            ..
        } = *viewport;
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
                let already_selected = self.model.session.as_ref().is_some_and(|session| {
                    session
                        .workspace
                        .spaces
                        .iter()
                        .any(|space| space.selected_tab == tab)
                });
                // Walking into an agent from the sidebar resumes it where
                // it was left: the shell opened beside it, if that is
                // where the user was working, rather than the agent's own
                // tab every time. Only when travelling — clicking the
                // agent already in context is a deliberate move back to
                // the agent itself, and the strip is right there for
                // anything else.
                let selected = if hit_rect.x < layout.sidebar.right()
                    && !self.model.is_context_agent(tab, &self.identities)
                {
                    self.model.strip_tab_for(tab)
                } else {
                    tab
                };
                self.model.acknowledge_completed_agent_tab(tab);
                let _ = send_request(
                    &mut self.stream,
                    &ClientRequest::SelectTab { tab: selected },
                );
                if let Some(pane) = self.model.pane_for_tab(selected) {
                    resize_pane(&mut self.stream, &mut self.model, pane, columns, rows);
                }
                if already_selected
                    && let Some(group) =
                        tab_drag_group(&self.model, &self.identities, layout, hit_rect, tab)
                {
                    let origin = match group {
                        TabDragGroup::Agents(_) => mouse.row,
                        TabDragGroup::Strip(..) => mouse.column,
                    };
                    self.model.dragging_tab = Some(DraggingTab {
                        tab,
                        group,
                        origin,
                        armed: false,
                        pending: None,
                    });
                }
            }
            WorkspaceHit::CloseTab(tab) => {
                let _ = send_request(&mut self.stream, &ClientRequest::CloseTab { tab });
            }
            WorkspaceHit::NewTab => {
                let _ = send_request(
                    &mut self.stream,
                    &ClientRequest::CreateTab {
                        label: next_shell_label(&self.model, &self.identities),
                        agent: context_agent(&self.model, &self.identities),
                        columns,
                        rows,
                        cwd: new_shell_cwd(&self.model, &self.identities),
                        command: None,
                    },
                );
            }
            WorkspaceHit::NewAgentMenu => {
                self.model.agent_picker = Some(AgentPicker {
                    options: agent_options(self.home),
                    selected: 0,
                    anchor: hit_rect,
                    cwd: None,
                    resume: None,
                });
                // Unlike every other arm here, this is a purely
                // local state change with no server round trip
                // to eventually mark the model dirty via
                // `apply()` — without this the popup exists in
                // `model` but the screen never redraws to show
                // it.
                self.model.dirty = true;
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
                let landing = self.model.session.as_ref().and_then(|session| {
                    let space = session
                        .workspace
                        .spaces
                        .iter()
                        .find(|candidate| candidate.id == space)?;
                    Some((space.selected_tab, space_own_tab(space, &self.identities)))
                });
                if let Some((selected, own)) = landing {
                    self.model
                        .acknowledge_completed_agent_tab(own.unwrap_or(selected));
                }
                let _ = match landing.and_then(|(_, own)| own) {
                    Some(tab) => send_request(&mut self.stream, &ClientRequest::SelectTab { tab }),
                    None => send_request(&mut self.stream, &ClientRequest::SelectSpace { space }),
                };
                // Resize the pane the same way `SelectTab` does
                // — switching spaces switches which tab (and so
                // which pane) is focused, same as switching
                // tabs within one space already does.
                if let Some(pane) = landing
                    .map(|(selected, own)| own.unwrap_or(selected))
                    .and_then(|tab| self.model.pane_for_tab(tab))
                {
                    resize_pane(&mut self.stream, &mut self.model, pane, columns, rows);
                }
            }
            WorkspaceHit::ContextMenuAction(_) => {
                // Only reachable while the context menu is
                // open, which the guarded arm above already
                // handles — same as `PickAgent` above for the
                // agent picker.
            }
            WorkspaceHit::NewSpace => {
                let prefill = self
                    .model
                    .session
                    .as_ref()
                    .map(|session| crate::ui::display_project_path(&session.selected_space().root))
                    .unwrap_or_else(|| "~".to_owned());
                self.model.root_picker = Some(RootPicker::opened_in(&prefill));
                self.model.dirty = true;
            }
            WorkspaceHit::PickSpaceRoot(_) => {
                // Only reachable while the root picker is open,
                // which the guarded arm above already handles —
                // same as `PickAgent` for the agent picker.
            }
            WorkspaceHit::OpenGitView => {
                open_git_view(&mut self.model);
            }
            WorkspaceHit::Deliver(_) => {
                deliver_selected_tab(&mut self.model, self.home, &self.answers.deliveries);
            }
            WorkspaceHit::ToggleSpaceRoot(space) => {
                toggle_space_root(&mut self.model, space);
            }
            WorkspaceHit::ResumeLostCheckout(tab) => {
                let resume = self
                    .model
                    .tab_focus_pane(tab)
                    .and_then(|pane| self.model.lost_task(pane))
                    .map(|(primary, task)| ResumeTarget {
                        primary: primary.clone(),
                        task: task.id.clone(),
                        replacing: Some(tab),
                    });
                if let Some(resume) = resume {
                    // The revived agent is created in the *selected*
                    // space, and the row it takes over from is closed
                    // once it is open — a tab cannot be the last one
                    // left in its space and still close. Selecting the
                    // row first makes both land in the same space,
                    // whichever space the operator was looking at.
                    let _ = send_request(&mut self.stream, &ClientRequest::SelectTab { tab });
                    self.model.agent_picker = Some(AgentPicker {
                        options: agent_options(self.home),
                        selected: 0,
                        anchor: hit_rect,
                        cwd: None,
                        resume: Some(resume),
                    });
                    self.model.dirty = true;
                }
            }
            WorkspaceHit::OpenStatusCatalog(anchor) => {
                self.model.status_catalog = Some(anchor);
                // Purely local state, no server round trip to
                // eventually mark the model dirty — same as
                // `NewAgentMenu` above.
                self.model.dirty = true;
            }
            WorkspaceHit::OpenAgentSupport(anchor) => {
                self.model.support_dropdown = selected_agent_context(&self.model, &self.identities)
                    .map(|key| AgentSupportDropdown { key, anchor });
                // Opening always re-reads, even when an answer
                // for this key is already held: `AGENTS.md` and
                // `.agents/` can change under an open workspace,
                // and this is the one moment the user is
                // actually looking at the answer.
                if let Some(dropdown) = &self.model.support_dropdown {
                    self.model.agent_support_pending = Some(dropdown.key.clone());
                    spawn_support_refresh(
                        self.home,
                        dropdown.key.clone(),
                        self.answers.support.clone(),
                    );
                }
                self.model.dirty = true;
            }
            // The extension's own surfaces, in its own vocabulary. The
            // overlay's hits are answered by the guarded arm in `press`
            // — same as `PickAgent`/`ContextMenuAction` above for the
            // other two overlays; the sidebar section's are answered
            // here, since it is drawn as part of the sidebar rather than
            // over it.
            WorkspaceHit::Extension(ExtensionHit::Git(_)) => {}
            WorkspaceHit::Extension(ExtensionHit::GitTimeline(hit)) => match hit {
                ViewHit::ToggleSection => toggle_timeline(&mut self.model),
                ViewHit::ResizeSection => self.model.dragging_timeline = true,
                ViewHit::SelectItem(index) => open_commit_detail(
                    &mut self.model,
                    index,
                    hit_rect,
                    &self.answers.commit_details,
                ),
                ViewHit::ResizeNavigator | ViewHit::Close => {}
            },
            WorkspaceHit::SwitchToManagement => {
                let _ = send_request(&mut self.stream, &ClientRequest::Detach);
                return Flow::Exit(WorkspaceExit::Management);
            }
            WorkspaceHit::ResizeSidebar => {
                self.model.dragging_sidebar = true;
            }
        }
        Flow::Continue
    }
}

/// Every receiver one attach drains, borrowed from [`WorkspaceMemory`].
///
/// The receivers stay in the memory rather than in [`Attach`] on purpose:
/// an answer still in flight when the user leaves for management has to
/// land when they come back, and a receiver dropped at the end of an
/// attach would leave its key reserved forever.
pub(super) struct AttachInbox<'a> {
    pub(super) events: &'a mpsc::Receiver<ClientEvent>,
    pub(super) support: &'a mpsc::Receiver<SupportResolution>,
    pub(super) tasks: &'a mpsc::Receiver<TaskResolution>,
    pub(super) deliveries: &'a mpsc::Receiver<DeliveryResolution>,
    pub(super) git: &'a mpsc::Receiver<GitResolution>,
    pub(super) commit_details: &'a mpsc::Receiver<CommitDetailResolution>,
    pub(super) git_views: &'a mpsc::Receiver<GitViewResolution>,
    pub(super) occupancy: &'a mpsc::Receiver<OccupancyResolution>,
    pub(super) placements: &'a mpsc::Receiver<PlacementResolution>,
}

impl Attach<'_> {
    /// Opens a tab for a new agent.
    ///
    /// A picker carrying a `cwd` is resuming a preserved task, whose slot
    /// already exists — that tab opens at once. Anything else needs a slot
    /// acquired for it, which is `git worktree add` plus the project's own
    /// link materialization and `setup` command: far too much to run where
    /// a keystroke is being handled, so it is asked for here and the tab
    /// opens in [`Attach::absorb_placement`] when the answer lands.
    fn launch_agent(
        &mut self,
        label: String,
        command: Vec<String>,
        cwd: Option<PathBuf>,
        resume: Option<ResumeTarget>,
        size: (u16, u16),
    ) {
        let Some(cwd) = cwd else {
            let replacing = resume.as_ref().and_then(|target| target.replacing);
            let request = match resume {
                Some(target) => PlacementRequest::Resume {
                    primary: target.primary,
                    task: target.task,
                },
                None => match selected_pane_cwd(&self.model) {
                    Some(from) => PlacementRequest::New { from },
                    // Nothing selected to place a slot relative to — the
                    // server's own default directory it is, same as
                    // before slots existed.
                    None => {
                        self.open_agent_tab_at(label, command, None, size);
                        return;
                    }
                },
            };
            if self.model.placement_pending {
                return;
            }
            self.model.placement_pending = true;
            self.model.set_busy_notice(format!("{label}: preparing"));
            let occupied: Vec<PathBuf> = self.model.occupied_checkouts.iter().cloned().collect();
            spawn_agent_placement(
                self.home,
                request,
                occupied,
                label,
                command,
                replacing,
                self.answers.placements.clone(),
            );
            return;
        };
        self.model
            .schedule_evaluation(self.home, cwd.clone(), &self.answers.tasks);
        self.open_agent_tab(label, command, cwd, size);
    }

    /// The one place a `CreateTab` for an agent is sent, so the two ways
    /// of asking for one cannot disagree about what a tab is.
    fn open_agent_tab(
        &mut self,
        label: String,
        command: Vec<String>,
        cwd: PathBuf,
        size: (u16, u16),
    ) {
        self.open_agent_tab_at(label, command, Some(cwd), size);
    }

    /// `None` leaves the directory to the server — the one case where
    /// there is no pane to place the agent relative to.
    fn open_agent_tab_at(
        &mut self,
        label: String,
        command: Vec<String>,
        cwd: Option<PathBuf>,
        size: (u16, u16),
    ) {
        let _ = send_request(
            &mut self.stream,
            &ClientRequest::CreateTab {
                cwd,
                label,
                agent: None,
                columns: size.0,
                rows: size.1,
                command: Some(command),
            },
        );
    }

    /// Opens the tab a placement was acquired for.
    fn absorb_placement(&mut self, resolution: PlacementResolution) {
        self.model.placement_pending = false;
        self.model.occupancy_stale = true;
        let PlacementResolution {
            label,
            command,
            placement,
            replacing,
        } = resolution;
        // A resume with nowhere to go opens nothing: the task keeps its
        // branch and stays in the preserved list, and the reason is said.
        let placement = match placement {
            Ok(placement) => placement,
            Err(reason) => {
                self.model.set_notice(format!("{label}: {reason}"));
                return;
            }
        };
        // What the launch could not do, said once — the tab opens either
        // way. An agent that could not be isolated is the louder of the
        // two: it is about to write in the operator's own checkout, and
        // the reason for that was computed and then dropped on the floor
        // until this read it.
        let said = match &placement.isolation {
            uze_application::Isolation::Unisolated { reason } => {
                Some(format!("no checkout — {reason}"))
            }
            uze_application::Isolation::Slot { .. } => placement.warnings.first().cloned(),
        };
        match said {
            Some(text) => self.model.set_notice(format!("{label}: {text}")),
            None => {
                self.model.notice = None;
                self.model.dirty = true;
            }
        }
        self.model
            .schedule_evaluation(self.home, placement.cwd.clone(), &self.answers.tasks);
        // The size the last frame actually drew — the same value the
        // resize path keeps in step with the layout.
        let size = self.model.last_size;
        self.open_agent_tab(label, command, placement.cwd, size);
        // The agent this one took over from stood in a directory that no
        // longer exists: nothing it is told can reach the task any more,
        // and the operator asked for that task to continue here. Sent
        // after the new tab, so the space is never left without one.
        if let Some(tab) = replacing {
            let _ = send_request(&mut self.stream, &ClientRequest::CloseTab { tab });
        }
    }

    /// Turns a finished reconciliation into what the operator sees: a
    /// notice for work that was parked rather than dropped, and a re-read
    /// of every repository whose tasks actually moved.
    fn absorb_occupancy(&mut self, resolution: OccupancyResolution) {
        self.model.occupancy_pending = false;
        let OccupancyResolution { reconciliation } = resolution;
        if let Some(parked) = reconciliation.released.iter().find(|task| task.parked) {
            self.model
                .set_notice(format!("{}: parked (alt+p)", parked.label));
        }
        for cwd in reconciliation.changed {
            self.model
                .schedule_evaluation(self.home, cwd, &self.answers.tasks);
        }
    }

    /// One turn of everything that is not an event: absorb what the
    /// server and the background reads have said, then ask for whatever
    /// has gone stale.
    ///
    /// Nothing here blocks. Every read this schedules runs on a thread of
    /// its own and answers through [`AttachInbox`], which is what lets
    /// this be called every tick without the frame waiting on any of it.
    pub(super) fn pump(&mut self, inbox: &AttachInbox<'_>) {
        while let Ok(event) = inbox.events.try_recv() {
            self.model.apply(event, &self.identities);
        }
        for request in adopt_agent_labels(&mut self.model, &self.identities) {
            let _ = send_request(&mut self.stream, &request);
        }
        // Absorb before scheduling, throughout: an answer sitting in the
        // channel still holds its reservation, so draining first is what
        // lets the very same tick ask the next question.
        while let Ok(resolution) = inbox.occupancy.try_recv() {
            self.absorb_occupancy(resolution);
        }
        sync_slot_occupancy(
            &mut self.model,
            self.home,
            &self.answers.occupancy,
            &self.answers.tasks,
        );
        while let Ok(resolution) = inbox.placements.try_recv() {
            self.absorb_placement(resolution);
        }
        while let Ok(resolution) = inbox.support.try_recv() {
            if self.model.agent_support_pending.as_ref() == Some(&resolution.key) {
                self.model.agent_support_pending = None;
            }
            self.model.agent_support = Some(resolution);
            self.model.dirty = true;
        }
        while let Ok(resolution) = inbox.tasks.try_recv() {
            self.model.task_eval_pending.remove(&resolution.key);
            let Some(EvaluationAnswer {
                primary,
                branch,
                target,
                sync,
                evaluation,
            }) = resolution.answered
            else {
                continue;
            };
            match branch {
                Some(branch) => self.model.branches.insert(resolution.key.clone(), branch),
                None => self.model.branches.remove(&resolution.key),
            };
            match target {
                Some(target) => self.model.targets.insert(resolution.key.clone(), target),
                None => self.model.targets.remove(&resolution.key),
            };
            match sync {
                Some(sync) => self.model.upstream_syncs.insert(resolution.key, sync),
                None => self.model.upstream_syncs.remove(&resolution.key),
            };
            self.model.tasks.insert(primary, evaluation.tasks);
            self.model.bind_pane_tasks();
            // A conflict found while a clean task followed the target is
            // the agent's to resolve: the message goes into its pane, as
            // one submission.
            for notice in evaluation.notices {
                if let Some(pane) = self.model.pane_for_checkout(&notice.checkout) {
                    let mut bytes = notice.message.into_bytes();
                    bytes.push(b'\r');
                    let _ = send_request(&mut self.stream, &ClientRequest::Input { pane, bytes });
                }
            }
            self.model.dirty = true;
        }
        while let Ok(resolution) = inbox.deliveries.try_recv() {
            for report in &resolution.reports {
                self.model.delivery_pending.remove(&report.task.id);
                self.model.set_task_notice(
                    &report.task.id,
                    &report.task.label,
                    describe_delivery(report),
                );
                // Two endings are the owning agent's to act on — a
                // delivery that came back to it, and a published branch
                // whose request is still unopened — and both reach it the
                // same way: one submission into its pane.
                if let DeliveryOutcome::ReturnedToAgent(notice)
                | DeliveryOutcome::AwaitingRequest(notice) = &report.outcome
                    && let Some(pane) = self.model.pane_for_checkout(&notice.checkout)
                {
                    let mut bytes = notice.message.clone().into_bytes();
                    bytes.push(b'\r');
                    let _ = send_request(&mut self.stream, &ClientRequest::Input { pane, bytes });
                }
            }
            if resolution.reports.is_empty() {
                self.model.set_notice("nothing ready".to_owned());
            }
            self.model
                .schedule_evaluation(self.home, resolution.cwd, &self.answers.tasks);
            self.model.dirty = true;
        }
        // Readiness is a Git fact, read when a pane goes quiet and, less
        // often, on a clock — never told by the agent.
        let quiet_panes = std::mem::take(&mut self.model.recently_quiet);
        let quiet: Vec<PathBuf> = quiet_panes
            .into_iter()
            .filter_map(|pane| self.model.pane_cwd(pane))
            .collect();
        for cwd in quiet {
            self.model
                .schedule_evaluation(self.home, cwd, &self.answers.tasks);
        }
        if self
            .model
            .last_task_refresh
            .is_none_or(|last| last.elapsed() >= TASK_REFRESH)
        {
            self.model.last_task_refresh = Some(Instant::now());
            if let Some(cwd) = selected_pane_cwd(&self.model) {
                self.model
                    .schedule_evaluation(self.home, cwd, &self.answers.tasks);
            }
        }
        // Work still in flight has no deadline: it is retired by the
        // outcome that replaces it, never by a clock that would leave the
        // operator watching nothing while it ran.
        if self
            .model
            .notice
            .as_ref()
            .is_some_and(|notice| !notice.busy && notice.since.elapsed() >= NOTICE_TTL)
        {
            self.model.notice = None;
            self.model.dirty = true;
        }
        // Contextual resolution: whatever the selection currently is, that
        // is what must be resolved. Keyed on `(harness, cwd)`, so this
        // fires exactly when the answer could have changed — a different
        // agent tab selected, or the server's live probe reporting the
        // pane moved — and never repeats for an answer already held.
        if let Some(key) = selected_agent_context(&self.model, &self.identities)
            && self.model.agent_support_pending.as_ref() != Some(&key)
            && self
                .model
                .agent_support
                .as_ref()
                .is_none_or(|resolution| resolution.key != key)
        {
            self.model.agent_support_pending = Some(key.clone());
            spawn_support_refresh(self.home, key, self.answers.support.clone());
        }
        while let Ok(resolution) = inbox.git.try_recv() {
            self.model.dirty |= self.model.absorb_git_read(resolution);
        }
        while let Ok(resolution) = inbox.commit_details.try_recv() {
            self.model.dirty |= self.model.absorb_commit_detail(resolution);
        }
        while let Ok(resolution) = inbox.git_views.try_recv() {
            self.model.dirty |= self.model.absorb_git_view_reload(resolution);
        }
        self.model.schedule_git_read(&self.answers.git);
        self.model.schedule_git_view_reload(&self.answers.git_views);
        if self.model.expire_agent_activity(Instant::now()) {
            self.model.dirty = true;
        }
        // The same clock drives the notice chip's spinner, so it has to
        // turn for a busy notice even with every agent idle.
        if workspace_has_active_agent_operation(&self.model, &self.identities)
            || self.model.notice_is_busy()
        {
            let now = Instant::now();
            if now >= self.next_tick {
                self.spinner.inc(1);
                self.model.tick = self.spinner.position() as usize;
                self.next_tick = now + AGENT_ACTIVITY_TICK;
                self.model.dirty = true;
            }
        }
    }
}
