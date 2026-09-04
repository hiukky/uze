//! The slot-and-delivery engine end to end (L3): the real `uze` binary's
//! terminal server, real Git, an isolated `$UZE_HOME`, and a scripted agent
//! in place of a model — driven through the client protocol the way the
//! TUI drives it, with the application asked what the TUI would ask it.
//!
//! No container and no PTY driver: everything the engine needs is local,
//! and what a container adds — a vendor's binary, a synthetic provider —
//! is not part of slots, rebases, gates or fast-forwards.

use std::{
    fs,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Child, Stdio},
    time::{Duration, Instant},
};

use uze_application::{DeliveryOutcome, Isolation, TaskStateView, UzeApplication, UzeHome};
use uze_terminal::{
    ClientEvent, ClientRequest, PROTOCOL_VERSION, PaneId, Session, WorkspaceId, attach, open_space,
    read_event, send_request, socket_path,
};
use uze_testkit::{env::ProcessEnvGuard, fake_harness::FakeHarness, temp::TestEnvironment};

use super::util::uze_bin;

const WAIT: Duration = Duration::from_secs(30);

/// One repository, one server, one attached client, and the scripts its
/// agents will play.
struct Engine {
    env: TestEnvironment,
    scripts: PathBuf,
    server: Child,
    stream: UnixStream,
    reader: UnixStream,
    session: Option<Session>,
    _guard: ProcessEnvGuard<'static>,
}

impl Engine {
    fn start(lock: &str) -> Self {
        let env = TestEnvironment::isolated();
        // The application runs in this process and spawns Git and the forge
        // CLI, so this process must see the isolated HOME, UZE_HOME and the
        // fake bin on PATH for as long as the engine lives.
        let guard: ProcessEnvGuard<'static> = unsafe { std::mem::transmute(env.apply()) };
        let project = env.project.clone();
        let git = |args: &[&str]| {
            let output = env.command("git").args(args).output().unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        git(&["init", "--quiet", "-b", "main", "."]);
        git(&["config", "user.name", "Operator"]);
        git(&["config", "user.email", "operator@uze.invalid"]);
        fs::write(project.join("README.md"), "# engine\n").unwrap();
        fs::write(project.join(".gitignore"), "target/\n").unwrap();
        fs::write(
            project.join("agents.lock"),
            format!("version: 1\nworktrees:\n{lock}"),
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "init"]);

        let scripts = env.root().join("agent-scripts");
        fs::create_dir_all(&scripts).unwrap();
        FakeHarness::scripted_agent(&env.fake_bin, "agent");

        let server = env
            .command(uze_bin())
            .args(["terminal", "serve", "--root"])
            .arg(&project)
            .env("AGENT_SCRIPTS", &scripts)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the real uze binary serves a terminal");
        let socket = socket_path(&project).unwrap();
        wait_until("the server's socket appears", || socket.exists());
        let (stream, reader) = connect(&project);
        let mut engine = Self {
            env,
            scripts,
            server,
            stream,
            reader,
            session: None,
            _guard: guard,
        };
        engine.wait_for_session();
        engine
    }

    fn project(&self) -> &Path {
        &self.env.project
    }

    fn app(&self) -> UzeApplication {
        UzeApplication::new(UzeHome::at(&self.env.uze_home), Vec::new())
    }

    fn git(&self, cwd: &Path, args: &[&str]) -> String {
        let output = self
            .env
            .command("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn wait_for_session(&mut self) {
        let started = Instant::now();
        loop {
            match read_event(&mut self.reader).expect("the server keeps talking") {
                Some(ClientEvent::Attached { session })
                | Some(ClientEvent::Snapshot { session, .. })
                | Some(ClientEvent::SessionUpdated { session }) => {
                    self.session = Some(session);
                    return;
                }
                Some(ClientEvent::Error { message }) => panic!("server: {message}"),
                Some(_) => {}
                None => panic!("the server hung up"),
            }
            assert!(started.elapsed() < WAIT, "no session update in time");
        }
    }

    /// Launches an agent the way the TUI does: placement first, then a tab
    /// whose first process is the scripted agent, started in the slot.
    /// `start` is what the agent does before it goes quiet.
    fn launch(&mut self, start: &str) -> (String, PathBuf) {
        let placement = self.app().workspace().place_new_agent(self.project(), &[]);
        let Isolation::Slot { task, .. } = &placement.isolation else {
            panic!("{placement:?}");
        };
        let slot = placement.cwd.clone();
        let name = slot.file_name().unwrap().to_string_lossy().into_owned();
        fs::write(self.scripts.join(format!("{name}.start.sh")), start).unwrap();
        // A reused slot keeps its name, and with it the marker the previous
        // agent left: clearing it is what makes the wait below prove that
        // *this* agent ran.
        let started = self.scripts.join(format!("{name}.started"));
        let _ = fs::remove_file(&started);
        let label = format!(
            "agent {}",
            self.session
                .as_ref()
                .map_or(1, |s| s.selected_space().tabs.len())
        );
        send_request(
            &mut self.stream,
            &ClientRequest::CreateTab {
                label,
                agent: None,
                columns: 80,
                rows: 24,
                cwd: Some(slot.clone()),
                command: Some(vec!["agent".into()]),
            },
        )
        .unwrap();
        self.wait_for_tab_in(&slot);
        wait_until("the agent played its script", || started.exists());
        (task.as_str().to_owned(), slot)
    }

    /// The pane of the tab running in `slot`, from the latest session.
    fn pane_in(&self, slot: &Path) -> PaneId {
        self.find_pane_in(slot).expect("a tab runs in the slot")
    }

    fn find_pane_in(&self, slot: &Path) -> Option<PaneId> {
        let slot = slot.canonicalize().unwrap_or_else(|_| slot.to_path_buf());
        self.session
            .as_ref()?
            .workspace
            .spaces
            .iter()
            .flat_map(|space| &space.tabs)
            .find_map(|tab| match &tab.layout {
                uze_terminal::Layout::Pane(pane)
                    if pane.cwd.canonicalize().unwrap_or_else(|_| pane.cwd.clone()) == slot =>
                {
                    Some(pane.id)
                }
                _ => None,
            })
    }

    /// Reads session updates until `accept` holds: the server also pushes
    /// updates on its own clock (a pane's process name resolving), so the
    /// next event is not necessarily the one a request produced.
    fn wait_for_session_where(&mut self, what: &str, accept: impl Fn(&Session) -> bool) {
        let started = Instant::now();
        while !self.session.as_ref().is_some_and(&accept) {
            assert!(started.elapsed() < WAIT, "no session update where {what}");
            self.wait_for_session();
        }
    }

    /// Reads session updates until a tab runs in `slot`: the server answers
    /// an attach with more than one session event, and the one carrying the
    /// new tab is not necessarily the first to arrive.
    fn wait_for_tab_in(&mut self, slot: &Path) {
        let started = Instant::now();
        while self.find_pane_in(slot).is_none() {
            assert!(
                started.elapsed() < WAIT,
                "no tab appeared in {}",
                slot.display()
            );
            self.wait_for_session();
        }
    }

    /// The checkout directories a pane still sits in — what the TUI hands
    /// the application to say which slots are still somebody's.
    fn occupied(&self) -> Vec<PathBuf> {
        self.session
            .as_ref()
            .into_iter()
            .flat_map(|session| &session.workspace.spaces)
            .flat_map(|space| &space.tabs)
            .filter_map(|tab| match &tab.layout {
                uze_terminal::Layout::Pane(pane) => Some(pane.cwd.clone()),
                _ => None,
            })
            .collect()
    }

    /// Closes the tab running in `slot`, the way the operator does.
    fn close_tab_in(&mut self, slot: &Path) {
        let pane = self.pane_in(slot);
        let tab = self
            .session
            .as_ref()
            .unwrap()
            .workspace
            .spaces
            .iter()
            .flat_map(|space| &space.tabs)
            .find(|tab| tab.focus.pane == pane)
            .expect("the pane belongs to a tab")
            .id;
        send_request(&mut self.stream, &ClientRequest::CloseTab { tab }).unwrap();
        let slot = slot.to_path_buf();
        self.wait_for_session_where("the closed tab is gone", |_| true);
        while self.find_pane_in(&slot).is_some() {
            self.wait_for_session();
        }
    }

    /// Types one line into the agent's pane, as the TUI types a notice.
    fn tell(&mut self, pane: PaneId, message: &str) {
        let mut bytes = message.as_bytes().to_vec();
        bytes.push(b'\r');
        send_request(&mut self.stream, &ClientRequest::Input { pane, bytes }).unwrap();
    }

    fn state_of(&self, id: &str) -> TaskStateView {
        self.app()
            .workspace()
            .tasks(self.project())
            .into_iter()
            .find(|task| task.id == id)
            .map(|task| task.state)
            .unwrap_or_else(|| panic!("task {id} is recorded"))
    }

    fn evaluate(&self) -> uze_application::Evaluation {
        self.app().workspace().evaluate_tasks(self.project())
    }

    fn restart_server(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
        let project = self.project().to_path_buf();
        let socket = socket_path(&project).unwrap();
        wait_until("the dead server's socket is gone or stale", || {
            UnixStream::connect(&socket).is_err()
        });
        let _ = fs::remove_file(&socket);
        self.server = self
            .env
            .command(uze_bin())
            .args(["terminal", "serve", "--root"])
            .arg(&project)
            .env("AGENT_SCRIPTS", &self.scripts)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        wait_until("the new server's socket appears", || socket.exists());
        let (stream, reader) = connect(&project);
        self.stream = stream;
        self.reader = reader;
        self.wait_for_session();
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = send_request(&mut self.stream, &ClientRequest::Detach);
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

fn connect(project: &Path) -> (UnixStream, UnixStream) {
    let mut stream = attach(project, 80, 24).expect("connects to the server started above");
    let reader = stream.try_clone().unwrap();
    send_request(
        &mut stream,
        &ClientRequest::Attach {
            version: PROTOCOL_VERSION,
            workspace: WorkspaceId("engine-test".into()),
            columns: 80,
            rows: 24,
            root: Some(project.to_path_buf()),
        },
    )
    .unwrap();
    (stream, reader)
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(started.elapsed() < WAIT, "timed out waiting until {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Evaluates until every listed task reads as `wanted`, or fails.
fn wait_for_states(engine: &Engine, ids: &[&str], wanted: &TaskStateView) {
    wait_until(&format!("tasks {ids:?} read as {wanted:?}"), || {
        engine.evaluate();
        ids.iter().all(|id| &engine.state_of(id) == wanted)
    });
}

fn commit_script(file: &str, contents: &str) -> String {
    format!("printf '{contents}' > {file}\ngit add {file}\ngit commit --quiet -m '{file}'\n")
}

#[test]
fn three_agents_deliver_into_a_linear_target_around_the_operators_edits() {
    let mut engine = Engine::start("  completion: merge\n  gate: test -f README.md\n");
    let project = engine.project().to_path_buf();
    // The operator is mid-edit in the primary the whole time.
    fs::write(project.join("README.md"), "# engine, edited\n").unwrap();
    fs::write(project.join("scratch.txt"), "untracked\n").unwrap();

    let a = engine.launch(&commit_script("a.rs", "fn a() {}\\n"));
    let b = engine.launch(&commit_script("b.rs", "fn b() {}\\n"));
    let c = engine.launch(&commit_script("c.rs", "fn c() {}\\n"));
    let slots = [&a.1, &b.1, &c.1];
    assert!(
        slots
            .iter()
            .all(|slot| slot.starts_with(project.join(".worktrees")))
    );
    assert_eq!(
        slots
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );

    wait_for_states(&engine, &[&a.0, &b.0, &c.0], &TaskStateView::Ready);
    let reports = engine.app().workspace().deliver_ready(&project);
    assert_eq!(reports.len(), 3, "{reports:?}");
    assert!(
        reports
            .iter()
            .all(|report| report.outcome == DeliveryOutcome::Merged),
        "{reports:?}"
    );

    for file in ["a.rs", "b.rs", "c.rs"] {
        assert!(project.join(file).is_file(), "{file} landed in the primary");
    }
    let parents = engine.git(&project, &["log", "--format=%p", "-n", "3"]);
    assert!(
        parents
            .lines()
            .all(|line| line.split_whitespace().count() == 1),
        "linear: {parents}"
    );
    assert_eq!(
        fs::read_to_string(project.join("README.md")).unwrap(),
        "# engine, edited\n",
        "the operator's uncommitted edit survived three deliveries"
    );
    let status = engine.git(&project, &["status", "--porcelain"]);
    assert_eq!(
        status.lines().count(),
        2,
        "only the operator's own changes: {status}"
    );
    for id in [&a.0, &b.0, &c.0] {
        assert_eq!(engine.state_of(id), TaskStateView::Integrated);
    }
}

/// The whole point of a slot: an agent closed without delivering anything
/// gives its checkout back, and one that left work behind does not.
#[test]
fn a_closed_agent_gives_its_slot_back_and_one_holding_work_keeps_it() {
    let mut engine = Engine::start("  completion: merge\n");
    let project = engine.project().to_path_buf();

    let (empty, slot) = engine.launch("true\n");
    engine.close_tab_in(&slot);
    let occupied = engine.occupied();
    let released = engine
        .app()
        .workspace()
        .release_abandoned_tasks(&project, &occupied);
    assert_eq!(released.len(), 1, "{released:?}");
    assert!(!released[0].parked, "the checkout held nothing");
    assert_eq!(
        engine.state_of(&empty),
        TaskStateView::Closed,
        "it ended holding nothing, which is not the same as delivered"
    );

    let (_, reused) = engine.launch("true\n");
    assert_eq!(
        reused, slot,
        "the freed slot is taken instead of a new working tree"
    );
    let occupied = engine.occupied();
    assert!(
        engine
            .app()
            .workspace()
            .release_abandoned_tasks(&project, &occupied)
            .is_empty(),
        "an agent sitting in its slot is not abandoned"
    );

    let (unsaved, kept) = engine.launch("printf 'draft\\n' > draft.rs\n");
    engine.close_tab_in(&kept);
    let occupied = engine.occupied();
    let released = engine
        .app()
        .workspace()
        .release_abandoned_tasks(&project, &occupied);
    assert_eq!(released.len(), 1, "{released:?}");
    assert!(released[0].parked, "it holds uncommitted work");
    assert_eq!(engine.state_of(&unsaved), TaskStateView::Parked);
    assert!(kept.join("draft.rs").is_file(), "every file is preserved");

    let (_, fresh) = engine.launch("true\n");
    assert_ne!(fresh, kept, "a parked slot is never handed to a new agent");
}

/// The status column is read off the checkout, every time, for as long as
/// somebody is in it: an agent almost never stops at its first delivery,
/// and a row that froze on `delivered` was reporting the past over a slot
/// full of work.
#[test]
fn a_slots_status_follows_the_agent_through_a_delivery_and_past_it() {
    let mut engine = Engine::start("  completion: merge\n");
    let project = engine.project().to_path_buf();

    let (id, slot) = engine.launch("printf 'draft\\n' > draft.rs\n");
    wait_for_states(&engine, &[&id], &TaskStateView::Uncommitted);

    engine.git(&slot, &["add", "draft.rs"]);
    engine.git(&slot, &["commit", "--quiet", "-m", "draft"]);
    wait_for_states(&engine, &[&id], &TaskStateView::Ready);

    let report = engine
        .app()
        .workspace()
        .deliver_task(&project, &id)
        .unwrap();
    assert_eq!(report.outcome, DeliveryOutcome::Merged, "{report:?}");
    assert_eq!(engine.state_of(&id), TaskStateView::Integrated);

    engine.evaluate();
    assert_eq!(
        engine.state_of(&id),
        TaskStateView::Integrated,
        "nothing new leaves the delivery standing"
    );

    // The same agent carries on in the same slot.
    fs::write(slot.join("after.rs"), "fn after() {}\n").unwrap();
    wait_for_states(&engine, &[&id], &TaskStateView::Uncommitted);

    engine.git(&slot, &["add", "after.rs"]);
    engine.git(&slot, &["commit", "--quiet", "-m", "after"]);
    wait_for_states(&engine, &[&id], &TaskStateView::Ready);

    let report = engine
        .app()
        .workspace()
        .deliver_task(&project, &id)
        .unwrap();
    assert_eq!(report.outcome, DeliveryOutcome::Merged, "{report:?}");
    assert!(project.join("after.rs").is_file(), "and the second lands");
}

#[test]
fn a_conflict_goes_to_the_agents_pane_and_comes_back_resolved() {
    let mut engine = Engine::start("  completion: merge\n");
    let project = engine.project().to_path_buf();
    let (id, slot) = engine.launch(&commit_script("shared.rs", "agent\\n"));
    wait_for_states(&engine, &[&id], &TaskStateView::Ready);
    // The target moves under the task, on the same file.
    fs::write(project.join("shared.rs"), "operator\n").unwrap();
    engine.git(&project, &["add", "shared.rs"]);
    engine.git(&project, &["commit", "--quiet", "-m", "operator's shared"]);
    let target_before = engine.git(&project, &["rev-parse", "main"]);

    let evaluation = engine.evaluate();
    assert_eq!(evaluation.notices.len(), 1, "{evaluation:?}");
    let notice = evaluation.notices[0].clone();
    assert_eq!(notice.checkout, slot);
    assert!(matches!(
        engine.state_of(&id),
        TaskStateView::Conflicted { .. }
    ));
    assert_eq!(engine.git(&project, &["rev-parse", "main"]), target_before);

    // What the agent does with the message: resolve, continue, end the turn.
    let name = slot.file_name().unwrap().to_string_lossy().into_owned();
    fs::write(
        engine.scripts.join(format!("{name}.conflict.sh")),
        "printf 'both\\n' > shared.rs\ngit add shared.rs\nGIT_EDITOR=true git rebase --continue\n",
    )
    .unwrap();
    let pane = engine.pane_in(&slot);
    engine.tell(pane, &notice.message);
    let resolved = engine.scripts.join(format!("{name}.resolved"));
    wait_until("the agent resolved the rebase", || resolved.exists());

    wait_for_states(&engine, &[&id], &TaskStateView::Ready);
    let report = engine
        .app()
        .workspace()
        .deliver_task(&project, &id)
        .unwrap();
    assert_eq!(report.outcome, DeliveryOutcome::Merged, "{report:?}");
    assert_eq!(
        fs::read_to_string(project.join("shared.rs")).unwrap(),
        "both\n"
    );
}

#[test]
fn a_server_restart_loses_no_task_and_a_dirty_orphan_is_parked() {
    let mut engine = Engine::start("  completion: handoff\n");
    let project = engine.project().to_path_buf();
    let (id, slot) = engine.launch(&commit_script("kept.rs", "kept\\n"));
    wait_for_states(&engine, &[&id], &TaskStateView::Ready);
    // A checkout from before task state existed, with work in it.
    engine.git(
        &project,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "agent/agent-2",
            ".worktrees/agent-2",
            "HEAD",
        ],
    );
    fs::write(
        project.join(".worktrees/agent-2/half-done.rs"),
        "unfinished\n",
    )
    .unwrap();

    engine.restart_server();

    let evaluation = engine.evaluate();
    let kept = evaluation
        .tasks
        .iter()
        .find(|task| task.id == id)
        .expect("the task outlived the server");
    assert_eq!(kept.state, TaskStateView::Ready);
    assert_eq!(kept.checkout.as_deref(), Some(slot.as_path()));
    let legacy = evaluation
        .tasks
        .iter()
        .find(|task| task.branch == "agent/agent-2")
        .expect("the legacy checkout was adopted");
    assert_eq!(legacy.state, TaskStateView::Parked);
    assert_eq!(legacy.label, "agent-2");
    assert_eq!(
        fs::read_to_string(project.join(".worktrees/agent-2/half-done.rs")).unwrap(),
        "unfinished\n",
        "parked, with every file preserved"
    );
    let next = engine.app().workspace().place_new_agent(&project, &[]);
    assert!(
        next.cwd != project.join(".worktrees/agent-2"),
        "a parked slot is never handed to a new agent"
    );
}

#[test]
fn pr_publishes_against_a_fake_forge_without_pulling_the_local_target() {
    let mut engine = Engine::start("  completion: pr\n");
    let project = engine.project().to_path_buf();
    let origin = engine.env.root().join("origin.git");
    engine.git(
        &project,
        &[
            "init",
            "--quiet",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    engine.git(
        &project,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    engine.git(&project, &["push", "--quiet", "-u", "origin", "main"]);
    let gh_log = engine.env.root().join("gh.log");
    let gh = engine.env.fake_bin.join("gh");
    fs::write(
        &gh,
        format!(
            "#!/bin/sh\necho \"$@\" > '{}'\necho https://example.invalid/pull/1\n",
            gh_log.display()
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let (id, _) = engine.launch(&commit_script("feature.rs", "feature\\n"));
    wait_for_states(&engine, &[&id], &TaskStateView::Ready);
    let local_target = engine.git(&project, &["rev-parse", "main"]);
    let report = engine
        .app()
        .workspace()
        .deliver_task(&project, &id)
        .unwrap();
    let DeliveryOutcome::Published { branch, request } = &report.outcome else {
        panic!("{report:?}");
    };
    assert_eq!(request.as_deref(), Some("https://example.invalid/pull/1"));
    let remote = engine.git(&project, &["ls-remote", "--heads", "origin"]);
    assert!(remote.contains(&format!("refs/heads/{branch}")), "{remote}");
    let invocation = fs::read_to_string(&gh_log).unwrap();
    assert!(invocation.contains("--base main"), "{invocation}");
    assert_eq!(
        engine.git(&project, &["rev-parse", "main"]),
        local_target,
        "never pulled"
    );
}

/// One server, two clients: each keeps its own selection, and a `uze`
/// started inside a pane opens a space in the server instead of a client
/// inside a client.
#[test]
fn two_clients_keep_their_own_focus_and_a_nested_launch_opens_a_space() {
    let mut engine = Engine::start("  completion: handoff\n");
    let project = engine.project().to_path_buf();
    let first_space = engine.session.as_ref().unwrap().workspace.selected_space;
    assert_eq!(
        engine.session.as_ref().unwrap().selected_space().root,
        project.canonicalize().unwrap_or(project.clone()),
        "the launch directory is the first space's root"
    );

    // A second client attaches for another directory: it gets a space of
    // its own, selected for it alone.
    let other = engine.env.root().join("other-project");
    fs::create_dir_all(&other).unwrap();
    let (mut second, mut second_reader) = connect(&other);
    let second_view = loop {
        match read_event(&mut second_reader).unwrap() {
            Some(ClientEvent::Attached { session }) => break session,
            Some(ClientEvent::Error { message }) => panic!("{message}"),
            Some(_) => {}
            None => panic!("hung up"),
        }
    };
    assert_ne!(second_view.workspace.selected_space, first_space);
    assert_eq!(
        second_view.selected_space().root,
        other.canonicalize().unwrap_or(other.clone())
    );
    assert_eq!(second_view.workspace.spaces.len(), 2);

    // The first client's own view did not move.
    engine.wait_for_session_where("two spaces exist", |session| {
        session.workspace.spaces.len() == 2
    });
    assert_eq!(
        engine.session.as_ref().unwrap().workspace.selected_space,
        first_space,
        "another client's attach never moves this client's selection"
    );

    // Selecting in one client is invisible to the other.
    send_request(
        &mut second,
        &ClientRequest::SelectSpace { space: first_space },
    )
    .unwrap();
    let moved = loop {
        match read_event(&mut second_reader).unwrap() {
            Some(ClientEvent::SessionUpdated { session }) => break session,
            Some(_) => {}
            None => panic!("hung up"),
        }
    };
    assert_eq!(moved.workspace.selected_space, first_space);
    engine.wait_for_session();
    assert_eq!(
        engine.session.as_ref().unwrap().workspace.selected_space,
        first_space
    );
    let _ = send_request(&mut second, &ClientRequest::Detach);

    // A nested launch: what `uze` does when UZE_PANE is set.
    let nested = engine.env.root().join("nested-project");
    fs::create_dir_all(&nested).unwrap();
    let label = open_space(&nested).expect("the running server opens a space");
    assert_eq!(label, "nested-project");
    engine.wait_for_session_where("three spaces exist", |session| {
        session.workspace.spaces.len() == 3
    });
    let session = engine.session.as_ref().unwrap();
    assert!(
        session
            .workspace
            .spaces
            .iter()
            .any(|space| space.label == "nested-project")
    );
    assert_eq!(
        session.workspace.selected_space, first_space,
        "a space opened from a pane does not steal this client's focus"
    );
}
