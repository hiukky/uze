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
    grid::Dimensions,
    index::{Column, Line},
    term::{Config, TermMode, cell::Flags, test::TermSize},
    vte::ansi::{Color as EngineColor, NamedColor, Processor, Rgb},
};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::{
    CellAttributes, ClientEvent, ClientRequest, Cursor, MouseMode, PROTOCOL_VERSION, PaneDamage,
    PaneId, PaneSnapshot, RenderCell, Session, SpaceSeed, TabSeed, TerminalColor, WorkspaceId,
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

pub fn attach(root: &Path, _columns: u16, _rows: u16) -> Result<UnixStream, RuntimeError> {
    let endpoint = Endpoint::for_root(root)?;
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

pub fn stop(root: &Path) -> Result<(), RuntimeError> {
    let endpoint = Endpoint::for_root(root)?;
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

pub fn serve(root: PathBuf) -> Result<(), RuntimeError> {
    let endpoint = Endpoint::for_root(&root)?;
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
    fn for_root(root: &Path) -> Result<Self, RuntimeError> {
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
        let identity = workspace_identity(root);
        Ok(Self {
            socket: runtime.join(format!("uze-{identity}.sock")),
            pid: runtime.join(format!("uze-{identity}.pid")),
        })
    }
}

/// Where a server persists this workspace's space/tab shape between runs —
/// deliberately not `Endpoint::for_root`'s `XDG_RUNTIME_DIR`/temp directory
/// (that's routinely wiped on reboot, exactly the case this needs to
/// survive). `$UZE_HOME` (or `$HOME/.uze`) is resolved directly rather than
/// through `uze-core`'s `UzeHome` so this crate's own dependency footprint
/// stays untouched; `state/terminal/<identity>.json` mirrors the
/// `state/…json` layout `UzeHome::state_dir()` already uses for everything
/// else UZE persists. `None` only when neither env var resolves, which
/// mirrors `UzeHome::from_env`'s own failure case.
fn persisted_state_path(root: &Path) -> Option<PathBuf> {
    let home = env::var_os("UZE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".uze")))?;
    Some(
        home.join("state")
            .join("terminal")
            .join(format!("{}.json", workspace_identity(root))),
    )
}

/// What gets written to [`persisted_state_path`] — deliberately its own
/// shape, not the wire-protocol `Session`: clients never need this (it
/// carries a tab's original spawn `command`, which is respawned, never
/// displayed), and keeping it separate means restoring what a workspace
/// looked like never has to touch `PROTOCOL_VERSION`.
#[derive(Default, Serialize, serde::Deserialize)]
struct PersistedWorkspace {
    spaces: Vec<PersistedSpace>,
}

#[derive(Serialize, serde::Deserialize)]
struct PersistedSpace {
    label: String,
    tabs: Vec<PersistedTab>,
}

#[derive(Serialize, serde::Deserialize)]
struct PersistedTab {
    label: String,
    cwd: PathBuf,
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
fn load_persisted_workspace(root: &Path) -> Option<PersistedWorkspace> {
    let path = persisted_state_path(root)?;
    let bytes = fs::read(path).ok()?;
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

struct Server {
    session: Mutex<Session>,
    panes: Mutex<BTreeMap<PaneId, Arc<PaneRuntime>>>,
    clients: Mutex<Vec<mpsc::Sender<ClientEvent>>>,
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
        let persisted = load_persisted_workspace(&root);
        let seeds: Vec<SpaceSeed> = persisted
            .as_ref()
            .map(|workspace| {
                workspace
                    .spaces
                    .iter()
                    .map(|space| SpaceSeed {
                        label: space.label.clone(),
                        tabs: space
                            .tabs
                            .iter()
                            .map(|tab| TabSeed {
                                label: tab.label.clone(),
                                cwd: tab.cwd.clone(),
                            })
                            .collect(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let restoring = !seeds.is_empty();
        let session = if restoring {
            Session::restore(
                WorkspaceId(workspace_identity(&root)),
                root.clone(),
                80,
                24,
                seeds,
            )
        } else {
            Session::new(WorkspaceId(workspace_identity(&root)), root, 80, 24)
        };
        let (damage, damage_events) = mpsc::channel();
        let server = Self {
            session: Mutex::new(session),
            panes: Mutex::new(BTreeMap::new()),
            clients: Mutex::new(Vec::new()),
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
        let Some(path) = persisted_state_path(
            &self
                .session
                .lock()
                .expect("session poisoned")
                .workspace
                .root,
        ) else {
            return;
        };
        let panes = self.panes.lock().expect("panes poisoned");
        let session = self.session.lock().expect("session poisoned");
        let workspace = PersistedWorkspace {
            spaces: session
                .workspace
                .spaces
                .iter()
                .map(|space| PersistedSpace {
                    label: space.label.clone(),
                    tabs: space
                        .tabs
                        .iter()
                        .filter_map(|tab| {
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
                ..
            })) if version == PROTOCOL_VERSION => {
                self.resize_selected(columns, rows);
                self.clients
                    .lock()
                    .expect("clients poisoned")
                    .push(events.clone());
                let _ = events.send(ClientEvent::Attached {
                    session: self.session.lock().expect("session poisoned").clone(),
                });
                self.broadcast_snapshot();
                true
            }
            Ok(Some(ClientRequest::Attach { .. })) => {
                let _ = events.send(ClientEvent::Error {
                    message: "incompatible terminal runtime protocol".into(),
                });
                false
            }
            _ => false,
        };
        if !attached {
            return;
        }

        while let Ok(Some(request)) = read_message::<_, ClientRequest>(&mut reader) {
            match request {
                ClientRequest::Detach => {
                    let _ = events.send(ClientEvent::Detached);
                    break;
                }
                ClientRequest::Input { pane, bytes } => self.write_input(pane, &bytes),
                ClientRequest::Resize {
                    pane,
                    columns,
                    rows,
                } => self.resize_pane(pane, columns, rows),
                ClientRequest::CreateTab {
                    label,
                    columns,
                    rows,
                    cwd,
                    command,
                } => {
                    let pane = {
                        let mut session = self.session.lock().expect("session poisoned");
                        let cwd = cwd.unwrap_or_else(|| session.workspace.root.clone());
                        session.add_tab(label, columns, rows, cwd)
                    };
                    if self.spawn_pane(pane, command.as_deref()).is_err() {
                        let _ = events.send(ClientEvent::Error {
                            message: "could not create terminal pane".into(),
                        });
                    }
                    self.broadcast_session();
                }
                ClientRequest::SelectTab { tab } => {
                    let changed = self
                        .session
                        .lock()
                        .expect("session poisoned")
                        .select_tab(tab);
                    if changed {
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
                    columns,
                    rows,
                } => {
                    let pane = self
                        .session
                        .lock()
                        .expect("session poisoned")
                        .add_space(label, columns, rows);
                    if self.spawn_pane(pane, None).is_err() {
                        let _ = events.send(ClientEvent::Error {
                            message: "could not create terminal pane".into(),
                        });
                    }
                    self.broadcast_session();
                }
                ClientRequest::SelectSpace { space } => {
                    let changed = self
                        .session
                        .lock()
                        .expect("session poisoned")
                        .select_space(space);
                    if changed {
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

    fn write_input(&self, pane: PaneId, bytes: &[u8]) {
        if let Some(runtime) = self.panes.lock().expect("panes poisoned").get(&pane) {
            runtime.write(bytes);
        }
    }

    fn resize_selected(&self, columns: u16, rows: u16) {
        let pane = self
            .session
            .lock()
            .expect("session poisoned")
            .selected_tab()
            .focus
            .pane;
        self.resize_pane(pane, columns, rows);
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
            .retain(|sender| sender.send(ClientEvent::Damage(damage.clone())).is_ok());
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
            .retain(|sender| {
                sender
                    .send(ClientEvent::SessionUpdated {
                        session: session.clone(),
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
            .retain(|sender| {
                sender
                    .send(ClientEvent::Snapshot {
                        session: session.clone(),
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
    let mut cells = Vec::with_capacity(usize::from(columns) * usize::from(rows));
    for row in 0..rows {
        for column in 0..columns {
            let cell = &terminal.grid()[Line(row as i32)][Column(column as usize)];
            cells.push(RenderCell {
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
            });
        }
    }
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

fn workspace_identity(root: &Path) -> String {
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
        REPLY_FOREGROUND, ReplySink, Server, persisted_state_path, relaunch_command_for_process,
        replace_incompatible_server, server_protocol_version, snapshot, workspace_identity,
    };
    use crate::{MouseMode, PaneId, TerminalColor};
    use alacritty_terminal::{
        Term,
        term::{Config, test::TermSize},
        vte::ansi::Processor,
    };
    use std::{
        path::{Path, PathBuf},
        sync::Mutex,
        thread,
        time::Duration,
    };

    /// `UZE_HOME` is process-global; cargo runs this crate's tests in
    /// parallel threads by default, so any two of the persistence tests
    /// below setting it concurrently would race and read/write each
    /// other's scratch directory. Every test that touches `UZE_HOME` locks
    /// this for its entire body (guard held via the returned lock's scope)
    /// so at most one of them is ever in flight at a time.
    static UZE_HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn endpoint_identity_is_project_specific() {
        assert_eq!(
            workspace_identity(Path::new("/tmp/a")),
            workspace_identity(Path::new("/tmp/a"))
        );
        assert_ne!(
            workspace_identity(Path::new("/tmp/a")),
            workspace_identity(Path::new("/tmp/b"))
        );
    }

    /// A pid file with no recorded version — exactly what a server built
    /// before `write_pid_file` existed leaves behind — must read as
    /// "unknown", not "compatible": see `attach`'s pre-connect check, which
    /// treats this the same as a version that actively mismatches.
    #[test]
    fn server_protocol_version_is_unknown_without_a_recorded_version() {
        let scratch = std::env::temp_dir().join(format!(
            "uze-terminal-protocol-version-{}",
            std::process::id()
        ));
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
        let scratch = std::env::temp_dir().join(format!(
            "uze-terminal-replace-incompatible-{}",
            std::process::id()
        ));
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
        let (damage, _damage_events) = std::sync::mpsc::channel();
        let pane =
            PaneRuntime::spawn(PaneId(11), PathBuf::from("/tmp"), 80, 24, damage, None).unwrap();
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let expected_name = Path::new(&shell)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("sh")
            .to_owned();

        let mut status = None;
        for _ in 0..50 {
            status = pane.foreground_status();
            if status.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        pane.stop();

        let (cwd, process) = status.expect("foreground process must be observable on Linux");
        assert_eq!(cwd, PathBuf::from("/tmp"));
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
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let bin_dir = std::env::temp_dir().join(format!(
            "uze-shim-identity-test-{}-{nonce}",
            std::process::id()
        ));
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
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = std::env::temp_dir().join(format!(
            "uze-terminal-persist-{}-{nonce}",
            std::process::id()
        ));
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&uze_home).unwrap();

        // See `UZE_HOME_ENV_LOCK`: held for the rest of this test so no
        // other test's own `UZE_HOME` scoping can interleave with this
        // one's. Restored exactly, not just cleared, on the way out.
        let _env_guard = UZE_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous_uze_home = std::env::var_os("UZE_HOME");
        unsafe { std::env::set_var("UZE_HOME", &uze_home) };

        let endpoint = Endpoint::for_root(&project).unwrap();
        let (first, _damage) = Server::new(project.clone(), endpoint.clone()).unwrap();
        let agent_pane =
            first
                .session
                .lock()
                .expect("session poisoned")
                .add_space("frontend".into(), 80, 24);
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

        match previous_uze_home {
            Some(value) => unsafe { std::env::set_var("UZE_HOME", value) },
            None => unsafe { std::env::remove_var("UZE_HOME") },
        }
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
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = std::env::temp_dir().join(format!(
            "uze-terminal-persist-typed-{}-{nonce}",
            std::process::id()
        ));
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&uze_home).unwrap();
        let _env_guard = UZE_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous_uze_home = std::env::var_os("UZE_HOME");
        unsafe { std::env::set_var("UZE_HOME", &uze_home) };

        let endpoint = Endpoint::for_root(&project).unwrap();
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

        match previous_uze_home {
            Some(value) => unsafe { std::env::set_var("UZE_HOME", value) },
            None => unsafe { std::env::remove_var("UZE_HOME") },
        }
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A persisted command is a guess — the agent binary it names may have
    /// been uninstalled or renamed since. That must degrade to a plain
    /// shell in that one tab, never take the whole restored workspace down
    /// with it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_persisted_command_that_no_longer_resolves_falls_back_to_a_plain_shell() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = std::env::temp_dir().join(format!(
            "uze-terminal-persist-stale-{}-{nonce}",
            std::process::id()
        ));
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&uze_home).unwrap();
        let _env_guard = UZE_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous_uze_home = std::env::var_os("UZE_HOME");
        unsafe { std::env::set_var("UZE_HOME", &uze_home) };

        let path = persisted_state_path(&project).expect("resolvable under a scoped UZE_HOME");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let stale = PersistedWorkspace {
            spaces: vec![PersistedSpace {
                label: "space 1".into(),
                tabs: vec![PersistedTab {
                    label: "shell".into(),
                    cwd: project.clone(),
                    command: Some(vec!["definitely-not-a-real-binary-xyz".to_owned()]),
                }],
            }],
        };
        std::fs::write(&path, serde_json::to_vec(&stale).unwrap()).unwrap();

        let endpoint = Endpoint::for_root(&project).unwrap();
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

        match previous_uze_home {
            Some(value) => unsafe { std::env::set_var("UZE_HOME", value) },
            None => unsafe { std::env::remove_var("UZE_HOME") },
        }
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
