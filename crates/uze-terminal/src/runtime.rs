use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, BufReader, Read, Write},
    os::unix::fs::PermissionsExt,
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use alacritty_terminal::{
    Term,
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    term::{Config, TermMode, cell::Flags, test::TermSize},
    vte::ansi::{Color as EngineColor, NamedColor, Processor, Rgb},
};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::{
    CellAttributes, ClientEvent, ClientRequest, Cursor, MouseMode, PROTOCOL_VERSION, PaneDamage,
    PaneId, PaneSnapshot, RenderCell, Session, SpaceId, SpaceSeed, TabId, TabSeed, TerminalColor,
    WorkspaceId,
};

/// ADR-038: the endpoint is local and user-private; no network transport is
/// exposed by this runtime.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("terminal runtime is only available on Unix in this release")]
    UnsupportedPlatform,
    #[error("terminal runtime protocol error: {0}")]
    Protocol(String),
    #[error("terminal runtime I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("terminal runtime PTY error: {0}")]
    Pty(String),
}

/// Connects to the user's one server, starting it when none answers —
/// rooted at `root`, which only matters for a server that has nothing
/// persisted yet. The caller then sends `Attach` naming the root it wants a
/// space for.
pub fn attach(root: &Path, _columns: u16, _rows: u16) -> Result<UnixStream, RuntimeError> {
    let endpoint = Endpoint::global()?;
    // A server left running from a previous build (e.g. a `cargo install
    // --force` while it was still up) is *alive*, so the connect below
    // would succeed — this has to be caught before that, not after, since
    // some `PROTOCOL_VERSION` bumps changed the wire framing itself (see
    // its doc comment); there's no guarantee an incompatible server can
    // even parse an `Attach` request enough to answer with a clean
    // `ClientEvent::Error` rather than hanging the connection.
    if endpoint.socket.exists() && server_protocol_version(&endpoint.pid) != Some(PROTOCOL_VERSION)
    {
        replace_incompatible_server(&endpoint)?;
    }
    match UnixStream::connect(&endpoint.socket) {
        Ok(stream) => Ok(stream),
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                || error.kind() == io::ErrorKind::ConnectionRefused =>
        {
            recover_stale_endpoint(&endpoint)?;
            start_server(root, &endpoint)?;
            connect_waiting(&endpoint.socket)
        }
        Err(error) => Err(error.into()),
    }
}

/// Where the user's server listens. For a client that must connect to a
/// server it started itself and never start one — a test driving the
/// runtime through the real binary — since [`attach`] starts a server from
/// the current executable when none answers.
pub fn socket_path(_root: &Path) -> Result<PathBuf, RuntimeError> {
    Ok(Endpoint::global()?.socket)
}

/// Asks the running server for a space rooted at `root` — created when
/// none is — and answers with its label. For a `uze` started inside one of
/// the server's own panes: it must not open a client inside a client, so
/// it opens a space in the one it is already in and leaves. An error when
/// no server is running.
pub fn open_space(root: &Path) -> Result<String, RuntimeError> {
    let endpoint = Endpoint::global()?;
    let mut stream = UnixStream::connect(&endpoint.socket)
        .map_err(|_| RuntimeError::Protocol("no running uze to open a space in".into()))?;
    send_request(
        &mut stream,
        &ClientRequest::Attach {
            version: PROTOCOL_VERSION,
            workspace: WorkspaceId("nested".into()),
            columns: 0,
            rows: 0,
            root: Some(root.to_path_buf()),
        },
    )?;
    let label = loop {
        match read_event(&mut stream)? {
            Some(ClientEvent::Attached { session }) => {
                break session.selected_space().label.clone();
            }
            Some(ClientEvent::Error { message }) => return Err(RuntimeError::Protocol(message)),
            Some(_) => {}
            None => return Err(RuntimeError::Protocol("the server hung up".into())),
        }
    };
    let _ = send_request(&mut stream, &ClientRequest::Detach);
    Ok(label)
}

pub fn stop(_root: &Path) -> Result<(), RuntimeError> {
    let endpoint = Endpoint::global()?;
    let mut stream = UnixStream::connect(&endpoint.socket)?;
    write_message(&mut stream, &ClientRequest::Stop)?;
    match read_message::<_, ClientEvent>(&mut BufReader::new(stream))? {
        Some(ClientEvent::Stopped) => Ok(()),
        Some(ClientEvent::Error { message }) => Err(RuntimeError::Protocol(message)),
        _ => Err(RuntimeError::Protocol(
            "server did not acknowledge stop".into(),
        )),
    }
}

/// Serves the user's one workspace. `root` roots the first space when
/// nothing is persisted yet, and is otherwise ignored.
pub fn serve(root: PathBuf) -> Result<(), RuntimeError> {
    let endpoint = Endpoint::global()?;
    recover_stale_endpoint(&endpoint)?;
    let listener = UnixListener::bind(&endpoint.socket)?;
    fs::set_permissions(&endpoint.socket, fs::Permissions::from_mode(0o600))?;
    write_pid_file(&endpoint.pid, std::process::id())?;
    let (server, damage) = Server::new(root, endpoint.clone())?;
    let state = Arc::new(server);
    spawn_damage_broadcaster(Arc::clone(&state), damage);
    spawn_status_ticker(Arc::clone(&state));

    for stream in listener.incoming() {
        let stream = stream?;
        let client_state = Arc::clone(&state);
        thread::spawn(move || client_state.handle_client(stream));
        if state
            .stopped
            .lock()
            .expect("stop state poisoned")
            .to_owned()
        {
            break;
        }
    }
    state.stop_panes();
    let _ = fs::remove_file(&endpoint.socket);
    let _ = fs::remove_file(&endpoint.pid);
    Ok(())
}

#[derive(Clone)]
struct Endpoint {
    socket: PathBuf,
    pid: PathBuf,
}

impl Endpoint {
    /// One endpoint per user — per `UZE_HOME`, which is what "user" means
    /// to UZE: a second home is a second world, with a server of its own.
    fn global() -> Result<Self, RuntimeError> {
        let preferred = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir)
            .join("uze-runtime");
        let runtime = match fs::create_dir_all(&preferred) {
            Ok(()) => preferred,
            // Sandboxed terminals can expose XDG_RUNTIME_DIR while denying
            // writes below it. Fall back to an owner-scoped temp directory;
            // the socket remains local and the directory is immediately
            // restricted below.
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
                ) =>
            {
                let fallback =
                    env::temp_dir().join(format!("uze-runtime-{}", unsafe { libc::getuid() }));
                fs::create_dir_all(&fallback)?;
                fallback
            }
            Err(error) => return Err(error.into()),
        };
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))?;
        let identity = identity_of(&uze_home_dir());
        Ok(Self {
            socket: runtime.join(format!("uze-{identity}.sock")),
            pid: runtime.join(format!("uze-{identity}.pid")),
        })
    }
}

/// `$UZE_HOME`, or `$HOME/.uze` — resolved directly rather than through
/// `uze-core`'s `UzeHome` so this crate's own dependency footprint stays
/// untouched. The current directory is the last resort, so a server can
/// still start in an environment with neither.
fn uze_home_dir() -> PathBuf {
    env::var_os("UZE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".uze")))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Where the server persists the workspace's space/tab shape between runs —
/// deliberately not [`Endpoint::global`]'s `XDG_RUNTIME_DIR`/temp directory
/// (that's routinely wiped on reboot, exactly the case this needs to
/// survive). One file per user under `state/terminal/`, mirroring the
/// `state/…json` layout `UzeHome::state_dir()` already uses for everything
/// else UZE persists.
fn persisted_state_path() -> PathBuf {
    uze_home_dir()
        .join("state")
        .join("terminal")
        .join("workspace.json")
}

#[derive(Default, Serialize, serde::Deserialize)]
struct PersistedWorkspace {
    spaces: Vec<PersistedSpace>,
}

#[derive(Serialize, serde::Deserialize)]
struct PersistedSpace {
    label: String,
    root: PathBuf,
    tabs: Vec<PersistedTab>,
}

#[derive(Serialize, serde::Deserialize)]
struct PersistedTab {
    label: String,
    cwd: PathBuf,
    /// The tab this one belongs with, by index into its own space's tabs
    /// (see [`crate::TabSeed::agent`]). Absent in a file written
    /// before tabs belonged with anything, which reads back as `None`.
    #[serde(default)]
    agent: Option<usize>,
    /// The `argv` this tab's pane was last spawned with (see
    /// [`PaneRuntime::spawn_command`]) — `None` for a plain shell, `Some`
    /// for whatever agent it was running, so restoring relaunches the same
    /// program rather than dropping back to a bare shell.
    command: Option<Vec<String>>,
}

/// Best-effort: a workspace with nothing persisted yet (first run, or the
/// file is missing/unreadable/corrupt) is not an error — [`Server::new`]
/// falls back to its ordinary fresh-bootstrap path exactly as if this
/// returned `None` from the start.
fn load_persisted_workspace() -> Option<PersistedWorkspace> {
    let bytes = fs::read(persisted_state_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Common interactive-shell `comm` names, plus the server's own generic
/// "shell" placeholder before a pane's first status probe resolves —
/// recognized here purely to say "not worth trying to relaunch this by
/// name", the same judgment call `orchestrator.rs`'s sidebar used to make
/// with an identical list before agent classification took it over
/// client-side. This one is unrelated to that: naming ordinary shells is
/// general POSIX-adjacent knowledge, not the specific-harness knowledge
/// `uze-core`'s vendor-neutrality rule is actually about, so it's fine for
/// this crate to hold.
const PLAIN_SHELL_PROCESS_NAMES: [&str; 8] =
    ["shell", "zsh", "bash", "sh", "dash", "fish", "ksh", "tcsh"];

/// A best-effort relaunch command for a pane that was spawned plain (no
/// explicit `argv` — see [`PaneRuntime::spawn_command`]) but whose last-
/// known foreground process isn't an ordinary shell — `Some([process])` to
/// try relaunching that same program by name on restore, `None` when it
/// looks like nothing worth relaunching was there (a plain shell, or the
/// probe never resolved). Works for a shim-launched agent typed straight
/// into a "$ shell" tab specifically *because* `PaneRuntime::foreground_status`
/// already resolves such a process to its invoked alias (`claude`, not a
/// version string) via `UZE_SHIM_NAME` — this just trusts that value.
fn relaunch_command_for_process(process: &str) -> Option<Vec<String>> {
    let trimmed = process.trim();
    if trimmed.is_empty() || PLAIN_SHELL_PROCESS_NAMES.contains(&trimmed) {
        return None;
    }
    Some(vec![trimmed.to_owned()])
}

fn start_server(root: &Path, endpoint: &Endpoint) -> Result<(), RuntimeError> {
    let executable = env::current_exe()?;
    let child = std::process::Command::new(executable)
        .args(["terminal", "serve", "--root"])
        .arg(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    write_pid_file(&endpoint.pid, child.id())?;
    Ok(())
}

fn connect_waiting(socket: &Path) -> Result<UnixStream, RuntimeError> {
    for _ in 0..40 {
        match UnixStream::connect(socket) {
            Ok(stream) => return Ok(stream),
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    || error.kind() == io::ErrorKind::ConnectionRefused =>
            {
                thread::sleep(Duration::from_millis(25))
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(RuntimeError::Protocol(
        "terminal server did not become ready".into(),
    ))
}

fn recover_stale_endpoint(endpoint: &Endpoint) -> Result<(), RuntimeError> {
    if endpoint.socket.exists() && UnixStream::connect(&endpoint.socket).is_err() {
        if runtime_process_is_alive(&endpoint.pid)? {
            return Err(RuntimeError::Protocol(
                "terminal endpoint is unavailable while its owner is still alive".into(),
            ));
        }
        fs::remove_file(&endpoint.socket)?;
    }
    if endpoint.pid.exists() && !endpoint.socket.exists() {
        fs::remove_file(&endpoint.pid)?;
    }
    Ok(())
}

fn runtime_process_is_alive(pid_path: &Path) -> Result<bool, RuntimeError> {
    let Some(pid) = read_pid(pid_path) else {
        return Ok(false);
    };
    // `kill(pid, 0)` only inspects whether this process is addressable; it
    // does not send a signal. This is the proof required before stale socket
    // cleanup can remove the old endpoint.
    Ok(unsafe { libc::kill(pid, 0) == 0 })
}

/// A pid file's first line — see [`write_pid_file`].
fn read_pid(pid_path: &Path) -> Option<libc::pid_t> {
    let text = fs::read_to_string(pid_path).ok()?;
    text.lines().next()?.trim().parse().ok()
}

/// The `PROTOCOL_VERSION` the server holding this pid file was compiled
/// with, from the file's second line — `None` for a pid file written
/// before this line existed, or one that's missing/unreadable/corrupt.
/// [`attach`] treats all of those the same as a known mismatch: it can no
/// longer assume anything about how that server speaks the wire protocol.
fn server_protocol_version(pid_path: &Path) -> Option<u16> {
    let text = fs::read_to_string(pid_path).ok()?;
    text.lines().nth(1)?.trim().parse().ok()
}

/// Pairs a server's pid with the `PROTOCOL_VERSION` it was built with, so a
/// later `attach` can tell "alive" apart from "alive and speaks a protocol
/// this client understands" without opening a connection to find out — see
/// `attach`'s pre-connect check.
fn write_pid_file(pid_path: &Path, pid: u32) -> io::Result<()> {
    fs::write(pid_path, format!("{pid}\n{PROTOCOL_VERSION}"))
}

/// Tears down a server that's alive but speaking a `PROTOCOL_VERSION` this
/// client can't talk to, so `attach`'s subsequent connect lands on a fresh
/// one instead. Unlike [`recover_stale_endpoint`] (a dead owner, socket
/// already unusable), this owner is alive and mid-session, so it gets a
/// cooperative `SIGTERM` first — its own persisted-workspace snapshot (see
/// `persist`) is what lets the fresh server restore the same tabs — with
/// `SIGKILL` only as a last resort if it doesn't exit promptly.
fn replace_incompatible_server(endpoint: &Endpoint) -> Result<(), RuntimeError> {
    if let Some(pid) = read_pid(&endpoint.pid) {
        let is_alive = || unsafe { libc::kill(pid, 0) == 0 };
        unsafe { libc::kill(pid, libc::SIGTERM) };
        for _ in 0..40 {
            if !is_alive() {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        if is_alive() {
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
    let _ = fs::remove_file(&endpoint.socket);
    let _ = fs::remove_file(&endpoint.pid);
    Ok(())
}

/// What one attached client is looking at. The session itself carries the
/// server's defaults; a client's own selection overlays them in the
/// `Session` it receives, so two terminals attached to the one server can
/// look at two different agents.
#[derive(Clone, Debug, Default)]
struct Selection {
    space: Option<SpaceId>,
    tabs: BTreeMap<SpaceId, TabId>,
}

struct Client {
    id: u64,
    events: mpsc::Sender<ClientEvent>,
    selection: Selection,
}

struct Server {
    session: Mutex<Session>,
    panes: Mutex<BTreeMap<PaneId, Arc<PaneRuntime>>>,
    clients: Mutex<Vec<Client>>,
    next_client: std::sync::atomic::AtomicU64,
    stopped: Mutex<bool>,
    endpoint: Endpoint,
    /// Cloned into every [`PaneRuntime`] so its PTY reader thread can report
    /// new output; [`spawn_damage_broadcaster`] owns the matching receiver.
    damage: mpsc::Sender<PaneId>,
}

impl Server {
    fn new(
        root: PathBuf,
        endpoint: Endpoint,
    ) -> Result<(Self, mpsc::Receiver<PaneId>), RuntimeError> {
        // A previous run's shape, if this workspace has one — see
        // `persisted_state_path` for why a crash, a `kill -9`, or a reboot
        // still leaves this behind even though nothing else about a pane's
        // running state survives any of those.
        let persisted = load_persisted_workspace();
        let seeds: Vec<SpaceSeed> = persisted
            .as_ref()
            .map(|workspace| {
                workspace
                    .spaces
                    .iter()
                    .map(|space| SpaceSeed {
                        label: space.label.clone(),
                        root: space.root.clone(),
                        tabs: space
                            .tabs
                            .iter()
                            .map(|tab| TabSeed {
                                label: tab.label.clone(),
                                cwd: tab.cwd.clone(),
                                agent: tab.agent,
                            })
                            .collect(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let restoring = !seeds.is_empty();
        let identity = WorkspaceId(identity_of(&uze_home_dir()));
        let session = if restoring {
            Session::restore(identity, root.clone(), 80, 24, seeds)
        } else {
            Session::new(identity, root, 80, 24)
        };
        let (damage, damage_events) = mpsc::channel();
        let server = Self {
            session: Mutex::new(session),
            panes: Mutex::new(BTreeMap::new()),
            clients: Mutex::new(Vec::new()),
            next_client: std::sync::atomic::AtomicU64::new(1),
            stopped: Mutex::new(false),
            endpoint,
            damage,
        };
        if restoring && let Some(persisted) = &persisted {
            // Zip the restored session's freshly-allocated tabs back up
            // against the persisted commands they came from — safe because
            // `Session::restore` walks `seeds` (built from `persisted` one
            // line above) in the same order and never drops a space that
            // came in with tabs, so the two always line up one for one.
            let spawns: Vec<(PaneId, Option<Vec<String>>)> = server
                .session
                .lock()
                .expect("session poisoned")
                .workspace
                .spaces
                .iter()
                .zip(&persisted.spaces)
                .flat_map(|(space, persisted_space)| {
                    space
                        .tabs
                        .iter()
                        .zip(&persisted_space.tabs)
                        .map(|(tab, persisted_tab)| (tab.focus.pane, persisted_tab.command.clone()))
                })
                .collect();
            for (pane, command) in spawns {
                // A persisted command is a guess (an agent binary that may
                // since be uninstalled or renamed, or a best-effort
                // relaunch built from a live process name — see
                // `relaunch_command_for_process`) — one bad guess must
                // never keep the rest of a restored workspace from coming
                // back, so a failed spawn retries as a plain shell instead
                // of propagating; a plain-shell spawn failing is the same
                // fatal condition it always was.
                let spawned = server.spawn_pane(pane, command.as_deref());
                if spawned.is_err() && command.is_some() {
                    let _ = server.spawn_pane(pane, None);
                } else {
                    spawned?;
                }
            }
        } else {
            let first = server
                .session
                .lock()
                .expect("session poisoned")
                .selected_tab()
                .focus
                .pane;
            server.spawn_pane(first, None)?;
        }
        Ok((server, damage_events))
    }

    /// Best-effort snapshot of the current space/tab shape to
    /// [`persisted_state_path`] — called from [`Server::broadcast_session`]
    /// (every structural change: a tab/space created, closed, renamed, or
    /// moved to a new cwd), so whatever's on disk is never more than one
    /// change stale, however this process eventually stops.
    fn persist(&self) {
        let path = persisted_state_path();
        let panes = self.panes.lock().expect("panes poisoned");
        let session = self.session.lock().expect("session poisoned");
        let workspace = PersistedWorkspace {
            spaces: session
                .workspace
                .spaces
                .iter()
                .map(|space| PersistedSpace {
                    label: space.label.clone(),
                    root: space.root.clone(),
                    tabs: space
                        .tabs
                        .iter()
                        .filter_map(|tab| {
                            // By position, since a restored tab is minted a
                            // fresh id — and against this same list, which
                            // is the one `Session::restore` will rebuild.
                            let agent = tab.agent.and_then(|agent| {
                                space.tabs.iter().position(|other| other.id == agent)
                            });
                            let pane = find_in_layout(&tab.layout, tab.focus.pane)?;
                            // A tab spawned plain but with something other
                            // than a shell now running in it (someone typed
                            // `claude` straight into a "$ shell" tab, never
                            // going through "+ agent" at all) is exactly as
                            // much "had an agent" as one `CreateTab` was
                            // told to launch directly — restoring it back
                            // to a bare shell would silently drop that.
                            let command = panes
                                .get(&tab.focus.pane)
                                .and_then(|runtime| runtime.spawn_command.clone())
                                .or_else(|| relaunch_command_for_process(&pane.process));
                            Some(PersistedTab {
                                label: tab.label.clone(),
                                cwd: pane.cwd,
                                agent,
                                command,
                            })
                        })
                        .collect(),
                })
                .collect(),
        };
        drop(session);
        drop(panes);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_vec(&workspace) {
            let _ = fs::write(&path, json);
        }
    }

    fn handle_client(self: Arc<Self>, stream: UnixStream) {
        let reader_stream = match stream.try_clone() {
            Ok(value) => value,
            Err(_) => return,
        };
        let (events, receiver) = mpsc::channel();
        let mut writer = stream;
        thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                if write_message(&mut writer, &event).is_err() {
                    break;
                }
            }
        });

        let mut reader = BufReader::new(reader_stream);
        let attached = match read_message::<_, ClientRequest>(&mut reader) {
            Ok(Some(ClientRequest::Attach {
                version,
                columns,
                rows,
                root,
                ..
            })) if version == PROTOCOL_VERSION => {
                let client = self
                    .next_client
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let mut selection = Selection::default();
                if let Some(root) = root {
                    match self.ensure_space(&root) {
                        Ok(space) => selection.space = Some(space),
                        Err(error) => {
                            let _ = events.send(ClientEvent::Error {
                                message: format!(
                                    "could not open a space at {}: {error}",
                                    root.display()
                                ),
                            });
                        }
                    }
                }
                self.clients.lock().expect("clients poisoned").push(Client {
                    id: client,
                    events: events.clone(),
                    selection,
                });
                if columns > 0 && rows > 0 {
                    self.resize_pane(self.selected_pane_of(client), columns, rows);
                }
                let _ = events.send(ClientEvent::Attached {
                    session: self.view_of(client),
                });
                self.broadcast_snapshot();
                Some(client)
            }
            Ok(Some(ClientRequest::Attach { .. })) => {
                let _ = events.send(ClientEvent::Error {
                    message: "incompatible terminal runtime protocol".into(),
                });
                None
            }
            _ => None,
        };
        let Some(client) = attached else {
            return;
        };

        while let Ok(Some(request)) = read_message::<_, ClientRequest>(&mut reader) {
            match request {
                ClientRequest::Detach => {
                    let _ = events.send(ClientEvent::Detached);
                    break;
                }
                ClientRequest::Input { pane, bytes } => self.write_input(pane, &bytes),
                ClientRequest::Scroll { pane, lines } => self.scroll_pane(pane, lines),
                ClientRequest::Resize {
                    pane,
                    columns,
                    rows,
                } => self.resize_pane(pane, columns, rows),
                ClientRequest::CreateTab {
                    label,
                    agent,
                    columns,
                    rows,
                    cwd,
                    command,
                } => {
                    let (pane, tab, space) = {
                        let mut session = self.session.lock().expect("session poisoned");
                        let space = self
                            .selection_of(client)
                            .space
                            .filter(|space| session.space(*space).is_some())
                            .unwrap_or(session.workspace.selected_space);
                        let cwd = cwd.unwrap_or_else(|| {
                            session
                                .space(space)
                                .map(|space| space.root.clone())
                                .unwrap_or_else(|| PathBuf::from("."))
                        });
                        let pane = session.add_tab(space, label, agent, columns, rows, cwd);
                        let tab = session
                            .space(space)
                            .expect("the space the tab was added to")
                            .selected_tab;
                        (pane, tab, space)
                    };
                    self.update_selection(client, |selection| {
                        selection.space = Some(space);
                        selection.tabs.insert(space, tab);
                    });
                    if self.spawn_pane(pane, command.as_deref()).is_err() {
                        let _ = events.send(ClientEvent::Error {
                            message: "could not create terminal pane".into(),
                        });
                    }
                    self.broadcast_session();
                }
                ClientRequest::SelectTab { tab } => {
                    let located = {
                        let mut session = self.session.lock().expect("session poisoned");
                        session.select_tab(tab);
                        session
                            .workspace
                            .spaces
                            .iter()
                            .find(|space| space.tabs.iter().any(|t| t.id == tab))
                            .map(|space| space.id)
                    };
                    if let Some(space) = located {
                        self.update_selection(client, |selection| {
                            selection.space = Some(space);
                            selection.tabs.insert(space, tab);
                        });
                        self.broadcast_session();
                    }
                }
                ClientRequest::CloseTab { tab } => {
                    let removed = self
                        .session
                        .lock()
                        .expect("session poisoned")
                        .remove_tab(tab);
                    match removed {
                        Some(panes) => {
                            let mut runtimes = self.panes.lock().expect("panes poisoned");
                            for pane in panes {
                                if let Some(runtime) = runtimes.remove(&pane) {
                                    runtime.stop();
                                }
                            }
                            drop(runtimes);
                            self.broadcast_session();
                        }
                        None => {
                            let _ = events.send(ClientEvent::Error {
                                message: "cannot close the workspace's only tab".into(),
                            });
                        }
                    }
                }
                ClientRequest::RenameTab { tab, label } => {
                    let changed = self
                        .session
                        .lock()
                        .expect("session poisoned")
                        .rename_tab(tab, label);
                    if changed {
                        self.broadcast_session();
                    }
                }
                ClientRequest::CreateSpace {
                    label,
                    root,
                    columns,
                    rows,
                } => {
                    let (pane, space) = {
                        let mut session = self.session.lock().expect("session poisoned");
                        let label = label.unwrap_or_else(|| crate::state::space_label(&root));
                        let pane = session.add_space(label, root, columns, rows);
                        (pane, session.workspace.selected_space)
                    };
                    self.update_selection(client, |selection| selection.space = Some(space));
                    if self.spawn_pane(pane, None).is_err() {
                        let _ = events.send(ClientEvent::Error {
                            message: "could not create terminal pane".into(),
                        });
                    }
                    self.broadcast_session();
                }
                ClientRequest::SelectSpace { space } => {
                    let exists = {
                        let mut session = self.session.lock().expect("session poisoned");
                        session.select_space(space);
                        session.space(space).is_some()
                    };
                    if exists {
                        self.update_selection(client, |selection| selection.space = Some(space));
                        self.broadcast_session();
                    }
                }
                ClientRequest::CloseSpace { space } => {
                    let removed = self
                        .session
                        .lock()
                        .expect("session poisoned")
                        .remove_space(space);
                    match removed {
                        Some(panes) => {
                            let mut runtimes = self.panes.lock().expect("panes poisoned");
                            for pane in panes {
                                if let Some(runtime) = runtimes.remove(&pane) {
                                    runtime.stop();
                                }
                            }
                            drop(runtimes);
                            self.broadcast_session();
                        }
                        None => {
                            let _ = events.send(ClientEvent::Error {
                                message: "cannot close the workspace's only space".into(),
                            });
                        }
                    }
                }
                ClientRequest::RenameSpace { space, label } => {
                    let changed = self
                        .session
                        .lock()
                        .expect("session poisoned")
                        .rename_space(space, label);
                    if changed {
                        self.broadcast_session();
                    }
                }
                ClientRequest::Stop => {
                    *self.stopped.lock().expect("stop state poisoned") = true;
                    self.stop_panes();
                    let _ = events.send(ClientEvent::Stopped);
                    let _ = UnixStream::connect(&self.endpoint.socket);
                    break;
                }
                ClientRequest::Attach { .. } => {}
            }
        }
        self.clients
            .lock()
            .expect("clients poisoned")
            .retain(|attached| attached.id != client);
    }

    /// The space rooted at `root`, created — with its first shell pane —
    /// when none is.
    fn ensure_space(&self, root: &Path) -> Result<SpaceId, RuntimeError> {
        let (pane, space) = {
            let mut session = self.session.lock().expect("session poisoned");
            if let Some(space) = session.space_for_root(root) {
                return Ok(space);
            }
            let label = crate::state::space_label(root);
            let pane = session.add_space(label, root.to_path_buf(), 80, 24);
            (pane, session.workspace.selected_space)
        };
        self.spawn_pane(pane, None)?;
        Ok(space)
    }

    fn selection_of(&self, client: u64) -> Selection {
        self.clients
            .lock()
            .expect("clients poisoned")
            .iter()
            .find(|attached| attached.id == client)
            .map(|attached| attached.selection.clone())
            .unwrap_or_default()
    }

    fn update_selection(&self, client: u64, change: impl FnOnce(&mut Selection)) {
        if let Some(attached) = self
            .clients
            .lock()
            .expect("clients poisoned")
            .iter_mut()
            .find(|attached| attached.id == client)
        {
            change(&mut attached.selection);
        }
    }

    /// The session as `client` sees it: the shared structure with this
    /// client's own selection overlaid wherever it still points at
    /// something that exists.
    fn view_of(&self, client: u64) -> Session {
        let selection = self.selection_of(client);
        let session = self.session.lock().expect("session poisoned");
        view_for(&session, &selection)
    }

    fn selected_pane_of(&self, client: u64) -> PaneId {
        self.view_of(client).selected_tab().focus.pane
    }
    fn spawn_pane(&self, pane_id: PaneId, command: Option<&[String]>) -> Result<(), RuntimeError> {
        let pane = find_pane(&self.session.lock().expect("session poisoned"), pane_id)
            .ok_or_else(|| RuntimeError::Protocol("unknown pane".into()))?;
        let runtime = PaneRuntime::spawn(
            pane_id,
            pane.cwd,
            pane.columns,
            pane.rows,
            self.damage.clone(),
            command,
        )?;
        // Best-effort: label the sidebar tree with the real shell name
        // immediately instead of leaving the "shell" placeholder until the
        // next status tick.
        if let Some((cwd, process)) = runtime.foreground_status() {
            self.session
                .lock()
                .expect("session poisoned")
                .update_pane_status(pane_id, cwd, process);
        }
        self.panes
            .lock()
            .expect("panes poisoned")
            .insert(pane_id, Arc::new(runtime));
        Ok(())
    }

    /// Re-probes every pane's foreground process/cwd (see
    /// [`PaneRuntime::foreground_status`]) and broadcasts the session only
    /// if the sidebar tree would actually show something different —
    /// called on a slow tick (see [`spawn_status_ticker`]), never from the
    /// input/damage hot paths.
    fn refresh_pane_status(&self) {
        self.restore_finished_agent_panes();
        let probes: Vec<(PaneId, PathBuf, String)> = self
            .panes
            .lock()
            .expect("panes poisoned")
            .iter()
            .filter_map(|(&id, runtime)| {
                runtime
                    .foreground_status()
                    .map(|(cwd, process)| (id, cwd, process))
            })
            .collect();
        if probes.is_empty() {
            return;
        }
        let mut changed = false;
        let mut session = self.session.lock().expect("session poisoned");
        for (pane, cwd, process) in probes {
            changed |= session.update_pane_status(pane, cwd, process);
        }
        drop(session);
        if changed {
            self.broadcast_session();
        }
    }

    /// An agent tab starts the agent directly as the PTY child so terminal
    /// input, including Ctrl+C, reaches it naturally. Once that child exits,
    /// there is no shell left in the PTY to accept the next command. Replace
    /// only those finished direct-agent panes with a fresh shell; ordinary
    /// shell panes intentionally stay closed when their shell exits.
    fn restore_finished_agent_panes(&self) {
        let finished: Vec<PaneId> = self
            .panes
            .lock()
            .expect("panes poisoned")
            .iter()
            .filter_map(|(&pane, runtime)| runtime.finished_agent().then_some(pane))
            .collect();
        let mut restored = false;
        for pane in finished {
            if self.spawn_pane(pane, None).is_ok() {
                restored = true;
                self.broadcast_pane_damage(pane);
            }
        }
        if restored {
            self.broadcast_session();
        }
    }

    fn write_input(&self, pane: PaneId, bytes: &[u8]) {
        if let Some(runtime) = self.panes.lock().expect("panes poisoned").get(&pane) {
            runtime.write(bytes);
        }
    }

    fn scroll_pane(&self, pane: PaneId, lines: i32) {
        let changed = self
            .panes
            .lock()
            .expect("panes poisoned")
            .get(&pane)
            .is_some_and(|runtime| runtime.scroll(lines));
        if changed {
            self.broadcast_pane_damage(pane);
        }
    }

    fn resize_pane(&self, pane: PaneId, columns: u16, rows: u16) {
        if let Some(runtime) = self.panes.lock().expect("panes poisoned").get(&pane) {
            runtime.resize(columns, rows);
        }
        // A resize doesn't guarantee new PTY output on its own (an idle
        // shell prompt emits nothing after its terminal shrinks/grows), so
        // push the new dimensions immediately instead of waiting for the
        // next damage notification.
        self.broadcast_pane_damage(pane);
    }

    /// Sends only `pane`'s changed cells to every attached client — the
    /// steady-state update path, driven by PTY output instead of a client
    /// poll. Session/tab structure is unaffected, so only this pane's cells
    /// go out, and only the ones that actually changed since the last
    /// event this pane sent (see [`PaneRuntime::damage_since_last`]).
    fn broadcast_pane_damage(&self, pane: PaneId) {
        let Some(runtime) = self
            .panes
            .lock()
            .expect("panes poisoned")
            .get(&pane)
            .cloned()
        else {
            return;
        };
        let damage = runtime.damage_since_last();
        self.clients
            .lock()
            .expect("clients poisoned")
            .retain(|client| {
                client
                    .events
                    .send(ClientEvent::Damage(damage.clone()))
                    .is_ok()
            });
    }

    /// Sends just the tab/selection structure to every attached client —
    /// used by tab create/select/close. None of those change any pane's
    /// cells, and every open pane (selected or not) already stays current
    /// through its own damage pushes, so resending every pane's whole grid
    /// here (as tab-switching did before) was pure waste: a `SelectTab`
    /// that changes nothing about pane content was serializing thousands
    /// of unchanged cells per tab, which is what made switching tabs feel
    /// slow.
    fn broadcast_session(&self) {
        self.persist();
        let session = self.session.lock().expect("session poisoned").clone();
        self.clients
            .lock()
            .expect("clients poisoned")
            .retain(|client| {
                client
                    .events
                    .send(ClientEvent::SessionUpdated {
                        session: view_for(&session, &client.selection),
                    })
                    .is_ok()
            });
    }

    fn broadcast_snapshot(&self) {
        let session = self.session.lock().expect("session poisoned").clone();
        let panes: Vec<PaneSnapshot> = self
            .panes
            .lock()
            .expect("panes poisoned")
            .values()
            .map(|pane| pane.snapshot_and_remember())
            .collect();
        self.clients
            .lock()
            .expect("clients poisoned")
            .retain(|client| {
                client
                    .events
                    .send(ClientEvent::Snapshot {
                        session: view_for(&session, &client.selection),
                        panes: panes.clone(),
                    })
                    .is_ok()
            });
    }

    fn stop_panes(&self) {
        for pane in self.panes.lock().expect("panes poisoned").values() {
            pane.stop();
        }
    }
}

/// Coalesces damage notifications from every pane's PTY reader thread and
/// broadcasts one snapshot per dirty pane at most every 8ms — bounded,
/// output-driven redraws instead of a fixed-rate client poll (the source of
/// the workspace client's earlier busy-refresh/CPU-starvation bug).
fn spawn_damage_broadcaster(server: Arc<Server>, damage: mpsc::Receiver<PaneId>) {
    thread::spawn(move || {
        let mut dirty = std::collections::BTreeSet::new();
        loop {
            match damage.recv_timeout(Duration::from_millis(8)) {
                Ok(pane) => {
                    dirty.insert(pane);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            // Absorb whatever else arrived while broadcasting the last
            // batch, without blocking — this is what keeps a continuously
            // noisy pane (e.g. `yes`) flushing on this ~8ms cadence instead
            // of starving until output goes quiet.
            while let Ok(pane) = damage.try_recv() {
                dirty.insert(pane);
            }
            for pane in std::mem::take(&mut dirty) {
                server.broadcast_pane_damage(pane);
            }
        }
    });
}

/// Drives [`Server::refresh_pane_status`] on a slow, fixed cadence — cwd and
/// foreground-process are sidebar-tree labels, not terminal content, so
/// they don't need (and shouldn't cost) damage-path freshness.
const STATUS_PROBE_INTERVAL: Duration = Duration::from_secs(1);

fn spawn_status_ticker(server: Arc<Server>) {
    thread::spawn(move || {
        loop {
            server.refresh_pane_status();
            thread::sleep(STATUS_PROBE_INTERVAL);
            if *server.stopped.lock().expect("stop state poisoned") {
                break;
            }
        }
    });
}

struct PaneRuntime {
    id: PaneId,
    master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    terminal: Arc<Mutex<Term<ReplySink>>>,
    /// The `argv` this pane was spawned with, if it wasn't the default
    /// shell — kept only so a workspace restart can respawn the same
    /// command in the same tab (see [`Server::persisted_workspace`]); never
    /// read back to change how this live pane behaves.
    spawn_command: Option<Vec<String>>,
    /// The last snapshot actually sent to clients, so
    /// [`PaneRuntime::damage_since_last`] can diff against what they
    /// already have instead of resending every cell on every PTY read.
    last_sent: Mutex<Option<PaneSnapshot>>,
}

#[derive(Clone)]
struct ReplySink(mpsc::Sender<Vec<u8>>);

/// Mirrors `src/ui.rs`'s `BASE`/`TEXT_PRIMARY` — the exact colors a cell's
/// `TerminalColor::Default{Background,Foreground}` renders as on screen (see
/// `orchestrator::color`). A pane's own program can ask the terminal what
/// its background/foreground actually is (OSC 10/11 — e.g. to pick a light-
/// or dark-adapted UI, the way Codex's input surface does); answering with
/// anything other than what's truly drawn would tell it a color that
/// doesn't match, which is exactly what happens if the query goes
/// unanswered and the asker falls back to its own default guess.
const REPLY_BACKGROUND: Rgb = Rgb {
    r: 10,
    g: 12,
    b: 13,
};
const REPLY_FOREGROUND: Rgb = Rgb {
    r: 230,
    g: 228,
    b: 222,
};

impl EventListener for ReplySink {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(reply) => {
                let _ = self.0.send(reply.into_bytes());
            }
            // `Term::dynamic_color_sequence` (OSC 10/11/12 queries) never
            // sends a `PtyWrite` itself — it hands back a formatting
            // closure expecting the *caller* to resolve the color and
            // write the reply. Left unhandled, a query like Codex's OSC 11
            // background probe just hangs until it times out server-side,
            // so the query answers here instead of falling through.
            Event::ColorRequest(index, format) => {
                let color = if index == NamedColor::Foreground as usize {
                    Some(REPLY_FOREGROUND)
                } else if index == NamedColor::Background as usize {
                    Some(REPLY_BACKGROUND)
                } else {
                    None
                };
                if let Some(color) = color {
                    let _ = self.0.send(format(color).into_bytes());
                }
            }
            _ => {}
        }
    }
}

impl PaneRuntime {
    fn spawn(
        id: PaneId,
        cwd: PathBuf,
        columns: u16,
        rows: u16,
        damage: mpsc::Sender<PaneId>,
        command: Option<&[String]>,
    ) -> Result<Self, RuntimeError> {
        let spawn_command = command
            .filter(|command| !command.is_empty())
            .map(<[String]>::to_vec);
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| RuntimeError::Pty(error.to_string()))?;
        let mut command = match command {
            Some([program, args @ ..]) => {
                let mut builder = CommandBuilder::new(program);
                builder.args(args);
                builder
            }
            Some([]) | None => {
                let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
                CommandBuilder::new(shell)
            }
        };
        command.cwd(cwd);
        // What tells a `uze` started inside this pane that it is inside one,
        // so it opens a space here instead of a client within a client.
        command.env("UZE_PANE", id.0.to_string());
        if env::var_os("TERM").is_none() {
            command.env("TERM", "xterm-256color");
        }
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| RuntimeError::Pty(error.to_string()))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| RuntimeError::Pty(error.to_string()))?;
        let writer = Arc::new(Mutex::new(
            pair.master
                .take_writer()
                .map_err(|error| RuntimeError::Pty(error.to_string()))?,
        ));
        let (reply_sender, reply_receiver) = mpsc::channel();
        let terminal = Arc::new(Mutex::new(Term::new(
            Config::default(),
            &TermSize::new(columns as usize, rows as usize),
            ReplySink(reply_sender),
        )));
        let parser_terminal = Arc::clone(&terminal);
        thread::spawn(move || {
            let mut reader = reader;
            let mut parser: Processor = Processor::new();
            let mut buffer = [0; 8192];
            loop {
                match std::io::Read::read(&mut reader, &mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        parser.advance(
                            &mut *parser_terminal.lock().expect("terminal poisoned"),
                            &buffer[..read],
                        );
                        let _ = damage.send(id);
                    }
                }
            }
        });
        let reply_writer = Arc::clone(&writer);
        thread::spawn(move || {
            while let Ok(bytes) = reply_receiver.recv() {
                if let Ok(mut writer) = reply_writer.lock() {
                    let _ = writer.write_all(&bytes);
                    let _ = writer.flush();
                }
            }
        });
        Ok(Self {
            id,
            master: Mutex::new(pair.master),
            writer,
            child: Mutex::new(child),
            terminal,
            spawn_command,
            last_sent: Mutex::new(None),
        })
    }

    fn write(&self, bytes: &[u8]) {
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.write_all(bytes);
            let _ = writer.flush();
        }
    }

    fn scroll(&self, lines: i32) -> bool {
        let mut terminal = self.terminal.lock().expect("terminal poisoned");
        let before = terminal.grid().display_offset();
        terminal.scroll_display(Scroll::Delta(lines));
        terminal.grid().display_offset() != before
    }
    fn resize(&self, columns: u16, rows: u16) {
        let _ = self
            .master
            .lock()
            .expect("master poisoned")
            .resize(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            });
        self.terminal
            .lock()
            .expect("terminal poisoned")
            .resize(TermSize::new(columns as usize, rows as usize));
    }
    fn stop(&self) {
        let _ = self.child.lock().expect("child poisoned").kill();
    }

    fn finished_agent(&self) -> bool {
        self.spawn_command.is_some()
            && self
                .child
                .lock()
                .expect("child poisoned")
                .try_wait()
                .ok()
                .flatten()
                .is_some()
    }

    /// Best-effort `(cwd, process name)` for whatever is currently running
    /// in the foreground of this pane — read straight from `/proc`, the
    /// same source `tmux`'s `pane_current_command`/`pane_current_path` use.
    /// `None` when the platform doesn't support it or the process just
    /// exited between the group-leader lookup and the `/proc` read.
    #[cfg(target_os = "linux")]
    fn foreground_status(&self) -> Option<(PathBuf, String)> {
        let pgid = self
            .master
            .lock()
            .expect("master poisoned")
            .process_group_leader()?;
        let cwd = std::fs::read_link(format!("/proc/{pgid}/cwd")).ok()?;
        let process = shim_launched_name(pgid).or_else(|| {
            std::fs::read_to_string(format!("/proc/{pgid}/comm"))
                .ok()
                .map(|comm| comm.trim().to_owned())
        })?;
        Some((cwd, process))
    }
    #[cfg(not(target_os = "linux"))]
    fn foreground_status(&self) -> Option<(PathBuf, String)> {
        None
    }

    fn snapshot(&self) -> PaneSnapshot {
        snapshot(self.id, &self.terminal.lock().expect("terminal poisoned"))
    }

    /// A full snapshot, remembered as the baseline for the next
    /// [`PaneRuntime::damage_since_last`] diff — used for the rare
    /// whole-session broadcasts (attach, tab create/select), which a newly
    /// attached client has no prior state to diff against.
    fn snapshot_and_remember(&self) -> PaneSnapshot {
        let current = self.snapshot();
        *self.last_sent.lock().expect("last_sent poisoned") = Some(current.clone());
        current
    }

    /// The steady-state update: only the cells that changed since the
    /// baseline this pane last sent (a full snapshot, or a previous
    /// damage event). Falls back to "every cell changed" the first time,
    /// or whenever dimensions moved since the baseline — a resize can't be
    /// expressed as a sparse diff against a differently-shaped grid.
    fn damage_since_last(&self) -> PaneDamage {
        let current = self.snapshot();
        let mut last_sent = self.last_sent.lock().expect("last_sent poisoned");
        let same_shape = last_sent.as_ref().is_some_and(|previous| {
            previous.columns == current.columns && previous.rows == current.rows
        });
        let changed = if same_shape {
            let previous = last_sent.as_ref().expect("checked above");
            current
                .cells
                .iter()
                .zip(previous.cells.iter())
                .enumerate()
                .filter(|(_, (new, old))| new != old)
                .map(|(index, (new, _))| cell_coordinates(index, current.columns, new.clone()))
                .collect()
        } else {
            current
                .cells
                .iter()
                .enumerate()
                .map(|(index, cell)| cell_coordinates(index, current.columns, cell.clone()))
                .collect()
        };
        let damage = PaneDamage {
            pane: self.id,
            columns: current.columns,
            rows: current.rows,
            cursor: current.cursor,
            alternate_screen: current.alternate_screen,
            mouse: current.mouse,
            bracketed_paste: current.bracketed_paste,
            changed,
        };
        *last_sent = Some(current);
        damage
    }
}

/// The alias uze's PATH shim (`src/shim.rs`) launched this process group's
/// leader under, if any — read from `UZE_SHIM_NAME` in its live
/// environment. The shim sets this immediately before `exec`ing into the
/// real binary, so it survives on the same pid for the rest of the
/// process's life, unlike `comm`, which a harness is free to overwrite
/// (Claude Code sets its own title to its version string, erasing the name
/// a person actually typed). `None` for anything not launched through the
/// shim — a bypassed launch, a harness that isn't shimmed, or a plain
/// shell — in which case `foreground_status` falls back to `comm`.
#[cfg(target_os = "linux")]
fn shim_launched_name(pgid: libc::pid_t) -> Option<String> {
    let environ = std::fs::read(format!("/proc/{pgid}/environ")).ok()?;
    environ
        .split(|&byte| byte == 0)
        .find_map(|entry| entry.strip_prefix(b"UZE_SHIM_NAME="))
        .filter(|value| !value.is_empty())
        .map(|value| String::from_utf8_lossy(value).into_owned())
}

fn cell_coordinates(index: usize, columns: u16, cell: RenderCell) -> (u16, u16, RenderCell) {
    let row = (index / usize::from(columns)) as u16;
    let column = (index % usize::from(columns)) as u16;
    (row, column, cell)
}

fn snapshot(pane: PaneId, terminal: &Term<ReplySink>) -> PaneSnapshot {
    let content = terminal.renderable_content();
    let columns = terminal.grid().columns() as u16;
    let rows = terminal.grid().screen_lines() as u16;
    let cells = terminal
        .grid()
        .display_iter()
        .map(|indexed| {
            let cell = indexed.cell;
            RenderCell {
                character: cell.c,
                foreground: color(cell.fg),
                background: color(cell.bg),
                attributes: CellAttributes {
                    bold: cell.flags.contains(Flags::BOLD),
                    dim: cell.flags.contains(Flags::DIM),
                    italic: cell.flags.contains(Flags::ITALIC),
                    underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
                    inverse: cell.flags.contains(Flags::INVERSE),
                    hidden: cell.flags.contains(Flags::HIDDEN),
                    strikeout: cell.flags.contains(Flags::STRIKEOUT),
                },
            }
        })
        .collect();
    let mode = content.mode;
    PaneSnapshot {
        pane,
        columns,
        rows,
        cursor: Cursor {
            column: content.cursor.point.column.0 as u16,
            row: content.cursor.point.line.0 as u16,
        },
        alternate_screen: mode.contains(TermMode::ALT_SCREEN),
        mouse: mouse_mode(mode),
        bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        cells,
    }
}

fn mouse_mode(mode: TermMode) -> MouseMode {
    MouseMode {
        reports_clicks: mode.intersects(
            TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION,
        ),
        reports_drag: mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION),
        sgr: mode.contains(TermMode::SGR_MOUSE),
    }
}

fn color(color: EngineColor) -> TerminalColor {
    match color {
        EngineColor::Indexed(index) => TerminalColor::Indexed(index),
        EngineColor::Spec(rgb) => TerminalColor::Rgb {
            red: rgb.r,
            green: rgb.g,
            blue: rgb.b,
        },
        EngineColor::Named(NamedColor::Background) => TerminalColor::DefaultBackground,
        EngineColor::Named(NamedColor::Foreground) => TerminalColor::DefaultForeground,
        EngineColor::Named(named) => TerminalColor::Indexed(named as u8),
    }
}

/// `session` with `selection` overlaid: the client's space when it still
/// exists, and its tab in every space where the tab still exists.
fn view_for(session: &Session, selection: &Selection) -> Session {
    let mut view = session.clone();
    if let Some(space) = selection.space
        && view.workspace.spaces.iter().any(|s| s.id == space)
    {
        view.workspace.selected_space = space;
    }
    for space in &mut view.workspace.spaces {
        if let Some(tab) = selection.tabs.get(&space.id)
            && space.tabs.iter().any(|t| t.id == *tab)
        {
            space.selected_tab = *tab;
        }
    }
    view
}

fn find_pane(session: &Session, wanted: PaneId) -> Option<crate::Pane> {
    session
        .workspace
        .spaces
        .iter()
        .flat_map(|space| &space.tabs)
        .find_map(|tab| find_in_layout(&tab.layout, wanted))
}
fn find_in_layout(layout: &crate::Layout, wanted: PaneId) -> Option<crate::Pane> {
    match layout {
        crate::Layout::Pane(pane) if pane.id == wanted => Some(pane.clone()),
        crate::Layout::Split { first, second, .. } => {
            find_in_layout(first, wanted).or_else(|| find_in_layout(second, wanted))
        }
        _ => None,
    }
}

pub fn send_request<W: Write>(writer: &mut W, value: &ClientRequest) -> Result<(), RuntimeError> {
    write_message(writer, value)
}
pub fn read_event<R: Read>(reader: &mut R) -> Result<Option<ClientEvent>, RuntimeError> {
    read_message(reader)
}
/// Length-prefixed bincode, not newline-delimited JSON: a `PaneSnapshot`
/// carries one `RenderCell` per grid cell, and JSON's per-field text
/// encoding of that (a `Snapshot`/`Damage` this size fires on every PTY
/// repaint — scrolling an agent's own transcript, not just resizes) was
/// measured spending hundreds of milliseconds in encode+decode alone on a
/// realistic multi-tab session, which is what made switching Work/Manage
/// and scrolling inside a pane both feel slow. Framing can't be
/// newline-delimited any more since the payload is binary and may contain
/// a literal `0x0A` byte anywhere in it.
fn write_message<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<(), RuntimeError> {
    let bytes =
        bincode::serialize(value).map_err(|error| RuntimeError::Protocol(error.to_string()))?;
    let len = u32::try_from(bytes.len())
        .map_err(|_| RuntimeError::Protocol("message exceeds 4GiB frame limit".into()))?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
fn read_message<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<Option<T>, RuntimeError> {
    let mut len_bytes = [0u8; 4];
    match reader.read_exact(&mut len_bytes) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let mut buffer = vec![0u8; u32::from_le_bytes(len_bytes) as usize];
    reader.read_exact(&mut buffer)?;
    bincode::deserialize(&buffer)
        .map(Some)
        .map_err(|error| RuntimeError::Protocol(error.to_string()))
}

fn identity_of(root: &Path) -> String {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let hash = canonical
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::{
        Endpoint, PaneRuntime, PersistedSpace, PersistedTab, PersistedWorkspace, REPLY_BACKGROUND,
        REPLY_FOREGROUND, ReplySink, Selection, Server, identity_of, persisted_state_path,
        relaunch_command_for_process, replace_incompatible_server, server_protocol_version,
        snapshot, view_for,
    };
    use crate::{MouseMode, PaneId, TerminalColor};
    use crate::{Session, SpaceId, TabId, WorkspaceId};
    use alacritty_terminal::{
        Term,
        grid::Scroll,
        term::{Config, test::TermSize},
        vte::ansi::Processor,
    };
    use std::collections::BTreeMap;
    use std::{
        path::{Path, PathBuf},
        thread,
        time::Duration,
    };

    /// A client's selection overlays the shared session wherever it still
    /// points at something, and falls back to the server's default where
    /// it does not — the rule that lets two terminals look at two agents.
    #[test]
    fn a_clients_view_overlays_its_own_selection_and_heals_a_stale_one() {
        let mut session = Session::new(WorkspaceId("w".into()), "/tmp/a".into(), 80, 24);
        let first_space = session.workspace.selected_space;
        session.add_space("b".into(), "/tmp/b".into(), 80, 24);
        let second_space = session.workspace.selected_space;
        session.add_tab(second_space, "extra".into(), None, 80, 24, "/tmp/b".into());
        let extra_tab = session.selected_tab().id;
        let first_tab_of_second = session.space(second_space).unwrap().tabs[0].id;

        let selection = Selection {
            space: Some(first_space),
            tabs: BTreeMap::from([(second_space, first_tab_of_second)]),
        };
        let view = view_for(&session, &selection);
        assert_eq!(view.workspace.selected_space, first_space);
        assert_eq!(
            view.space(second_space).unwrap().selected_tab,
            first_tab_of_second
        );
        assert_eq!(
            session.workspace.selected_space, second_space,
            "the shared default is untouched"
        );
        assert_eq!(session.space(second_space).unwrap().selected_tab, extra_tab);

        let stale = Selection {
            space: Some(SpaceId(99)),
            tabs: BTreeMap::from([(second_space, TabId(99))]),
        };
        let healed = view_for(&session, &stale);
        assert_eq!(healed.workspace.selected_space, second_space);
        assert_eq!(healed.space(second_space).unwrap().selected_tab, extra_tab);
    }

    #[test]
    fn endpoint_identity_is_project_specific() {
        assert_eq!(
            identity_of(Path::new("/tmp/a")),
            identity_of(Path::new("/tmp/a"))
        );
        assert_ne!(
            identity_of(Path::new("/tmp/a")),
            identity_of(Path::new("/tmp/b"))
        );
    }

    /// A pid file with no recorded version — exactly what a server built
    /// before `write_pid_file` existed leaves behind — must read as
    /// "unknown", not "compatible": see `attach`'s pre-connect check, which
    /// treats this the same as a version that actively mismatches.
    #[test]
    fn server_protocol_version_is_unknown_without_a_recorded_version() {
        let scratch = uze_testkit::temp::scratch("terminal-protocol-version");
        std::fs::create_dir_all(&scratch).unwrap();
        let pid_path = scratch.join("test.pid");

        std::fs::write(&pid_path, "4242").unwrap();
        assert_eq!(server_protocol_version(&pid_path), None);

        std::fs::write(&pid_path, format!("4242\n{}", super::PROTOCOL_VERSION)).unwrap();
        assert_eq!(
            server_protocol_version(&pid_path),
            Some(super::PROTOCOL_VERSION)
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The actual recovery `attach` relies on when it finds a live server
    /// it can no longer talk to: it must terminate that process and clear
    /// both endpoint files, so the caller's next connect lands on a fresh
    /// server instead of the one it just gave up on.
    #[test]
    fn replace_incompatible_server_kills_the_old_owner_and_clears_its_files() {
        let scratch = uze_testkit::temp::scratch("terminal-replace-incompatible");
        std::fs::create_dir_all(&scratch).unwrap();
        let socket = scratch.join("test.sock");
        let pid_path = scratch.join("test.pid");
        std::fs::write(&socket, b"placeholder").unwrap();

        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        // No version line: the exact shape `replace_incompatible_server` is
        // meant to react to.
        std::fs::write(&pid_path, pid.to_string()).unwrap();

        let endpoint = Endpoint {
            socket: socket.clone(),
            pid: pid_path.clone(),
        };
        replace_incompatible_server(&endpoint).unwrap();

        // `child` makes this test process the signaled child's parent, so
        // (unlike the real server, which has no such relationship to the
        // client that replaces it) it goes through a zombie state after
        // dying — `kill(pid, 0)` alone would stay "addressable" until
        // reaped. `try_wait` both reaps it and gives the real exit status.
        let mut exited = child.try_wait().unwrap();
        for _ in 0..40 {
            if exited.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(25));
            exited = child.try_wait().unwrap();
        }
        assert!(
            exited.is_some(),
            "old server process must not survive replace_incompatible_server"
        );
        assert!(!socket.exists());
        assert!(!pid_path.exists());

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn transcript_preserves_style_cursor_and_alternate_screen() {
        let (sender, _receiver) = std::sync::mpsc::channel();
        let mut terminal = Term::new(Config::default(), &TermSize::new(12, 3), ReplySink(sender));
        let mut parser: Processor = Processor::new();
        parser.advance(&mut terminal, b"\x1b[31mred\x1b[0m\x1b[2;5H!");
        let normal = snapshot(PaneId(1), &terminal);
        assert_eq!(normal.cells[0].character, 'r');
        assert_eq!(normal.cells[0].foreground, TerminalColor::Indexed(1));
        assert_eq!(normal.cursor.row, 1);
        assert_eq!(normal.cursor.column, 5);
        assert!(!normal.alternate_screen);
        parser.advance(&mut terminal, b"\x1b[?1049h");
        assert!(snapshot(PaneId(1), &terminal).alternate_screen);
        parser.advance(&mut terminal, b"\x1b[?1049l");
        assert!(!snapshot(PaneId(1), &terminal).alternate_screen);
    }

    #[test]
    fn mouse_mode_reflects_what_the_pane_actually_asked_for() {
        let (sender, _receiver) = std::sync::mpsc::channel();
        let mut terminal = Term::new(Config::default(), &TermSize::new(12, 3), ReplySink(sender));
        let mut parser: Processor = Processor::new();
        assert_eq!(snapshot(PaneId(1), &terminal).mouse, MouseMode::default());

        // Click reporting (1000) plus SGR extended coordinates (1006), no
        // drag/motion — a plain click-tracking app (a pager's mouse mode,
        // say), not one that also wants motion while a button is held.
        parser.advance(&mut terminal, b"\x1b[?1000h\x1b[?1006h");
        assert_eq!(
            snapshot(PaneId(1), &terminal).mouse,
            MouseMode {
                reports_clicks: true,
                reports_drag: false,
                sgr: true,
            }
        );

        // Drag reporting (1002) layers on top — the shape ratatui/textual/
        // ink-style TUIs (Codex, OpenCode) actually request for click-and-
        // drag UI like tab strips.
        parser.advance(&mut terminal, b"\x1b[?1002h");
        assert!(snapshot(PaneId(1), &terminal).mouse.reports_drag);

        parser.advance(&mut terminal, b"\x1b[?1000l\x1b[?1002l\x1b[?1006l");
        assert_eq!(snapshot(PaneId(1), &terminal).mouse, MouseMode::default());
    }

    #[test]
    fn bracketed_paste_reflects_what_the_pane_actually_asked_for() {
        // A readline-style program (Claude Code, Codex) turns this on
        // during its own startup — the client mirrors it onto the real
        // terminal so a physical paste (including a terminal's own
        // clipboard-image-to-text conversion) reaches the pane framed the
        // way the program expects, instead of arriving as a flood of
        // individual keystrokes a plain shell would.
        let (sender, _receiver) = std::sync::mpsc::channel();
        let mut terminal = Term::new(Config::default(), &TermSize::new(12, 3), ReplySink(sender));
        let mut parser: Processor = Processor::new();
        assert!(!snapshot(PaneId(1), &terminal).bracketed_paste);

        parser.advance(&mut terminal, b"\x1b[?2004h");
        assert!(snapshot(PaneId(1), &terminal).bracketed_paste);

        parser.advance(&mut terminal, b"\x1b[?2004l");
        assert!(!snapshot(PaneId(1), &terminal).bracketed_paste);
    }

    #[test]
    fn osc_background_and_foreground_queries_get_answered_instead_of_hanging() {
        // Regression: `Term::dynamic_color_sequence` (what OSC 10/11
        // queries dispatch to) never emits `Event::PtyWrite` itself — it
        // hands back a formatting closure via `Event::ColorRequest` that
        // the `EventListener` must resolve and write back. A listener that
        // only forwards `PtyWrite` (as `ReplySink` used to) silently drops
        // it, which is exactly what left a pane's own OSC 11 background
        // probe — used by adaptive TUIs like Codex to pick a light- or
        // dark-themed surface — unanswered.
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut terminal = Term::new(Config::default(), &TermSize::new(12, 3), ReplySink(sender));
        let mut parser: Processor = Processor::new();

        parser.advance(&mut terminal, b"\x1b]10;?\x1b\\");
        let foreground_reply = receiver.try_recv().expect("OSC 10 reply");
        assert_eq!(
            foreground_reply,
            format!(
                "\x1b]10;rgb:{0:02x}{0:02x}/{1:02x}{1:02x}/{2:02x}{2:02x}\x1b\\",
                REPLY_FOREGROUND.r, REPLY_FOREGROUND.g, REPLY_FOREGROUND.b
            )
            .into_bytes()
        );

        parser.advance(&mut terminal, b"\x1b]11;?\x1b\\");
        let background_reply = receiver.try_recv().expect("OSC 11 reply");
        assert_eq!(
            background_reply,
            format!(
                "\x1b]11;rgb:{0:02x}{0:02x}/{1:02x}{1:02x}/{2:02x}{2:02x}\x1b\\",
                REPLY_BACKGROUND.r, REPLY_BACKGROUND.g, REPLY_BACKGROUND.b
            )
            .into_bytes()
        );
    }

    #[test]
    fn resize_changes_snapshot_dimensions() {
        let (sender, _receiver) = std::sync::mpsc::channel();
        let mut terminal = Term::new(Config::default(), &TermSize::new(8, 2), ReplySink(sender));
        terminal.resize(TermSize::new(20, 4));
        let rendered = snapshot(PaneId(1), &terminal);
        assert_eq!((rendered.columns, rendered.rows), (20, 4));
    }

    #[test]
    fn snapshot_renders_the_scrollback_viewport() {
        let (sender, _receiver) = std::sync::mpsc::channel();
        let mut terminal = Term::new(Config::default(), &TermSize::new(8, 2), ReplySink(sender));
        let mut parser: Processor = Processor::new();
        parser.advance(&mut terminal, b"first\r\nsecond\r\nthird");

        terminal.scroll_display(Scroll::Delta(1));
        let rendered: String = snapshot(PaneId(1), &terminal)
            .cells
            .into_iter()
            .map(|cell| cell.character)
            .collect();

        assert!(rendered.contains("first"));
        assert!(rendered.contains("second"));
        assert!(!rendered.contains("third"));
    }

    #[test]
    fn damage_since_last_is_sparse_after_a_small_change() {
        let (damage, _damage_events) = std::sync::mpsc::channel();
        let pane =
            PaneRuntime::spawn(PaneId(9), PathBuf::from("/tmp"), 80, 24, damage, None).unwrap();
        // Baseline covers every cell — a fresh client has nothing to diff against.
        let baseline = pane.damage_since_last();
        assert_eq!(baseline.changed.len(), 80 * 24);

        pane.write(b"printf uze-diff-probe\\r");
        let mut probe = pane.damage_since_last();
        for _ in 0..50 {
            if probe
                .changed
                .iter()
                .any(|(_, _, cell)| cell.character == 'u')
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
            probe = pane.damage_since_last();
        }
        pane.stop();
        assert!(
            !probe.changed.is_empty(),
            "expected the echoed command to show up as changed cells"
        );
        assert!(
            probe.changed.len() < 80 * 24,
            "a one-line echo must not redescribe the whole grid, got {} changed cells",
            probe.changed.len()
        );
    }

    #[test]
    fn pane_process_keeps_output_until_explicit_stop() {
        let (damage, _damage_events) = std::sync::mpsc::channel();
        let pane =
            PaneRuntime::spawn(PaneId(7), PathBuf::from("/tmp"), 80, 24, damage, None).unwrap();
        pane.write(b"printf uze-runtime-live\\r");
        let mut rendered = String::new();
        for _ in 0..50 {
            rendered = pane
                .snapshot()
                .cells
                .into_iter()
                .map(|cell| cell.character)
                .collect();
            if rendered.contains("uze-runtime-live") {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        pane.stop();
        assert!(rendered.contains("uze-runtime-live"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn foreground_status_reports_the_spawned_shell_and_its_cwd() {
        // This is the *fallback* identity path: no shim identity present,
        // so the kernel's `comm` for the spawned shell is what gets
        // reported. The spawned child inherits this process's environment,
        // and on a dogfooding machine that environment carries the
        // `UZE_SHIM_NAME` of the session running the test suite itself —
        // which `foreground_status` rightly prefers (see the sibling test),
        // making this assertion read the developer's own session instead of
        // the shell it just spawned. Clearing it under the shared env lock
        // is what makes the fallback the thing actually under test.
        let mut env = uze_testkit::env::scope();
        env.remove("UZE_SHIM_NAME");
        let (damage, _damage_events) = std::sync::mpsc::channel();
        let pane =
            PaneRuntime::spawn(PaneId(11), PathBuf::from("/tmp"), 80, 24, damage, None).unwrap();
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let expected_name = Path::new(&shell)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("sh")
            .to_owned();

        // Poll until the *spawned shell* owns the PTY's foreground group,
        // identified by its cwd. Before it does, `process_group_leader`
        // transiently reports this test binary's own group — and reading
        // that process's `/proc/<pid>/environ` still yields the
        // `UZE_SHIM_NAME` of the session running the suite, because
        // `/proc/environ` exposes the environment block captured at `exec`
        // and is unaffected by a later `unsetenv`. Accepting the first
        // `Some` therefore made this assert against the developer's own
        // session at random.
        let pane_cwd = PathBuf::from("/tmp");
        let mut status = None;
        for _ in 0..50 {
            status = pane.foreground_status().filter(|(cwd, _)| *cwd == pane_cwd);
            if status.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        pane.stop();

        let (cwd, process) = status.expect("the spawned shell must own the PTY foreground group");
        assert_eq!(cwd, pane_cwd);
        assert_eq!(process, expected_name);
    }

    /// The kernel derives `comm` from the executed *file's own basename*,
    /// not from anything a person typed — which is exactly why a real
    /// Claude Code session reports its version number there instead of
    /// `claude`: it runs from `~/.local/share/claude/versions/<version>`.
    /// A copy of `sleep` under a version-number filename reproduces that
    /// same shape without depending on Claude Code being installed.
    /// `UZE_SHIM_NAME`, set by `src/shim.rs` right before it `exec`s into
    /// the real binary, must survive that and still be what
    /// `foreground_status` reports.
    #[cfg(target_os = "linux")]
    #[test]
    fn foreground_status_prefers_the_shim_identity_over_a_version_named_comm() {
        let bin_dir = uze_testkit::temp::scratch("shim-identity-test");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let versioned_binary = bin_dir.join("2.1.251");
        std::fs::copy("/bin/sleep", &versioned_binary).unwrap();

        let (damage, _damage_events) = std::sync::mpsc::channel();
        let pane = PaneRuntime::spawn(
            PaneId(13),
            PathBuf::from("/tmp"),
            80,
            24,
            damage,
            Some(&[
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                format!(
                    "export UZE_SHIM_NAME=claude; exec {} 5",
                    versioned_binary.display()
                ),
            ]),
        )
        .unwrap();

        let mut status = None;
        for _ in 0..50 {
            status = pane.foreground_status();
            if status
                .as_ref()
                .is_some_and(|(_, process)| process == "claude")
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        pane.stop();
        let _ = std::fs::remove_dir_all(&bin_dir);

        let (_, process) = status.expect("foreground process must be observable on Linux");
        assert_eq!(process, "claude");
    }

    /// The whole point of persistence: a server that starts with nothing
    /// running (simulating a reboot, a crash, `kill -9` — anything that
    /// left no chance for a clean stop) still comes back with the same
    /// spaces and tabs a previous instance for this same `root` had, each
    /// tab's pane relaunched with whatever it was last spawned with —
    /// `None` for a plain shell, the recorded `argv` for an agent.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_restarted_server_relaunches_the_same_spaces_tabs_and_agent_commands() {
        let scratch = uze_testkit::temp::scratch("terminal-persist");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        let runtime_dir = scratch.join("runtime");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&uze_home).unwrap();
        std::fs::create_dir_all(&runtime_dir).unwrap();

        // See `uze_testkit::env::scope`: held for the rest of this test so no
        // other test's own `UZE_HOME` scoping can interleave with this
        // one's. Restored exactly, not just cleared, on the way out.
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home)
            .set("XDG_RUNTIME_DIR", &runtime_dir);

        let endpoint = Endpoint::global().unwrap();
        let (first, _damage) = Server::new(project.clone(), endpoint.clone()).unwrap();
        let agent_pane = first.session.lock().expect("session poisoned").add_space(
            "frontend".into(),
            project.clone(),
            80,
            24,
        );
        first
            .spawn_pane(agent_pane, Some(&["sleep".to_owned(), "5".to_owned()]))
            .unwrap();
        // `CreateSpace`'s real dispatch (`runtime.rs`'s `handle_client`)
        // calls `broadcast_session`, which persists — replicated here
        // directly since this test drives `Server` without a socket.
        first.persist();
        first.stop_panes();

        let (second, _damage2) = Server::new(project.clone(), endpoint).unwrap();
        {
            let session = second.session.lock().expect("session poisoned");
            assert_eq!(session.workspace.spaces.len(), 2, "both spaces restored");
            let frontend = session
                .workspace
                .spaces
                .iter()
                .find(|space| space.label == "frontend")
                .expect("the second space's own label survived restore");
            let tab = &frontend.tabs[0];
            let panes = second.panes.lock().expect("panes poisoned");
            let runtime = panes
                .get(&tab.focus.pane)
                .expect("restored tab's pane was actually spawned");
            assert_eq!(
                runtime.spawn_command.as_deref(),
                Some(["sleep".to_owned(), "5".to_owned()].as_slice()),
                "restored tab relaunched with its original agent command"
            );
        }
        second.stop_panes();

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_finished_direct_agent_is_replaced_by_a_shell_in_its_pane() {
        let scratch = uze_testkit::temp::scratch("terminal-agent-exit");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        let runtime_dir = scratch.join("runtime");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&uze_home).unwrap();
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home)
            .set("XDG_RUNTIME_DIR", &runtime_dir);

        let endpoint = Endpoint::global().unwrap();
        let (server, _damage) = Server::new(project.clone(), endpoint).unwrap();
        let pane = server.session.lock().expect("session poisoned").add_space(
            "agent".into(),
            project.clone(),
            80,
            24,
        );
        server
            .spawn_pane(pane, Some(&["/bin/true".to_owned()]))
            .unwrap();

        for _ in 0..40 {
            server.restore_finished_agent_panes();
            let restored = server
                .panes
                .lock()
                .expect("panes poisoned")
                .get(&pane)
                .is_some_and(|runtime| runtime.spawn_command.is_none());
            if restored {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        assert!(
            server
                .panes
                .lock()
                .expect("panes poisoned")
                .get(&pane)
                .is_some_and(|runtime| runtime.spawn_command.is_none()),
            "a completed direct agent must leave an interactive shell in its existing pane"
        );
        server.stop_panes();

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn relaunch_command_for_process_recognizes_a_named_process_but_not_a_plain_shell() {
        assert_eq!(relaunch_command_for_process("zsh"), None);
        assert_eq!(relaunch_command_for_process("shell"), None);
        assert_eq!(relaunch_command_for_process(""), None);
        assert_eq!(relaunch_command_for_process("  "), None);
        assert_eq!(
            relaunch_command_for_process("claude"),
            Some(vec!["claude".to_owned()])
        );
    }

    /// The exact case that motivated `relaunch_command_for_process`: a tab
    /// opened as a plain "$ shell" (never through "+ agent", so it has no
    /// `spawn_command` of its own), where someone then typed an agent
    /// straight into it — `update_pane_status` here stands in for the
    /// status ticker's own probe reporting that live.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_plain_shell_tab_running_a_recognized_process_relaunches_as_that_process() {
        let scratch = uze_testkit::temp::scratch("terminal-persist-typed");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&uze_home).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);

        let endpoint = Endpoint::global().unwrap();
        let (first, _damage) = Server::new(project.clone(), endpoint.clone()).unwrap();
        let pane_id = first
            .session
            .lock()
            .expect("session poisoned")
            .selected_tab()
            .focus
            .pane;
        first
            .session
            .lock()
            .expect("session poisoned")
            .update_pane_status(pane_id, project.clone(), "sleep".to_owned());
        first.persist();
        first.stop_panes();

        let (second, _damage2) = Server::new(project.clone(), endpoint).unwrap();
        {
            let session = second.session.lock().expect("session poisoned");
            let tab = session.selected_tab();
            let panes = second.panes.lock().expect("panes poisoned");
            let runtime = panes
                .get(&tab.focus.pane)
                .expect("restored tab's pane was actually spawned");
            assert_eq!(
                runtime.spawn_command.as_deref(),
                Some(["sleep".to_owned()].as_slice()),
                "a process typed straight into a plain shell tab still relaunches on restore"
            );
        }
        second.stop_panes();

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A persisted command is a guess — the agent binary it names may have
    /// been uninstalled or renamed since. That must degrade to a plain
    /// shell in that one tab, never take the whole restored workspace down
    /// with it.
    /// Which tab belongs with which has to survive the process, and a
    /// `TabId` does not — the snapshot names the agent by its position in
    /// the very list `Session::restore` rebuilds.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_snapshot_names_a_tabs_agent_by_position() {
        let scratch = uze_testkit::temp::scratch("terminal-persist-agent");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&uze_home).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);

        let endpoint = Endpoint::global().unwrap();
        let (server, _damage) = Server::new(project.clone(), endpoint).expect("server");
        {
            let mut session = server.session.lock().expect("session poisoned");
            let space = session.workspace.selected_space;
            session.add_tab(space, "agent".into(), None, 80, 24, project.clone());
            let agent = session.selected_tab().id;
            session.add_tab(space, "shell".into(), Some(agent), 80, 24, project.clone());
        }
        server.persist();

        let written: PersistedWorkspace =
            serde_json::from_slice(&std::fs::read(persisted_state_path()).unwrap()).unwrap();
        let tabs = &written.spaces[0].tabs;
        assert_eq!(tabs.len(), 3, "the bootstrap shell, the agent, its shell");
        assert_eq!(tabs[2].agent, Some(1), "the shell belongs with the agent");
        assert_eq!(tabs[1].agent, None);

        server.stop_panes();
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_persisted_command_that_no_longer_resolves_falls_back_to_a_plain_shell() {
        let scratch = uze_testkit::temp::scratch("terminal-persist-stale");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&uze_home).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);

        let path = persisted_state_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let stale = PersistedWorkspace {
            spaces: vec![PersistedSpace {
                label: "space 1".into(),
                root: project.clone(),
                tabs: vec![PersistedTab {
                    label: "shell".into(),
                    cwd: project.clone(),
                    agent: None,
                    command: Some(vec!["definitely-not-a-real-binary-xyz".to_owned()]),
                }],
            }],
        };
        std::fs::write(&path, serde_json::to_vec(&stale).unwrap()).unwrap();

        let endpoint = Endpoint::global().unwrap();
        let (server, _damage) = Server::new(project.clone(), endpoint)
            .expect("a stale persisted command must not fail server startup");
        let session = server.session.lock().expect("session poisoned");
        let tab = session.selected_tab();
        let panes = server.panes.lock().expect("panes poisoned");
        assert!(
            panes.contains_key(&tab.focus.pane),
            "the tab still got a pane, spawned as a plain shell instead"
        );
        drop(panes);
        drop(session);
        server.stop_panes();

        let _ = std::fs::remove_dir_all(&scratch);
    }
}
