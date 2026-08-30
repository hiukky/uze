use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, BufRead, BufReader, Write},
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
    vte::ansi::{Color as EngineColor, NamedColor, Processor},
};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::{
    CellAttributes, ClientEvent, ClientRequest, Cursor, PROTOCOL_VERSION, PaneDamage, PaneId,
    PaneSnapshot, RenderCell, Session, TerminalColor, WorkspaceId,
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
    fs::write(&endpoint.pid, std::process::id().to_string())?;
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

fn start_server(root: &Path, endpoint: &Endpoint) -> Result<(), RuntimeError> {
    let executable = env::current_exe()?;
    let child = std::process::Command::new(executable)
        .args(["terminal", "serve", "--root"])
        .arg(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    fs::write(&endpoint.pid, child.id().to_string())?;
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
    let Ok(text) = fs::read_to_string(pid_path) else {
        return Ok(false);
    };
    let Ok(pid) = text.trim().parse::<libc::pid_t>() else {
        return Ok(false);
    };
    // `kill(pid, 0)` only inspects whether this process is addressable; it
    // does not send a signal. This is the proof required before stale socket
    // cleanup can remove the old endpoint.
    Ok(unsafe { libc::kill(pid, 0) == 0 })
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
        let session = Session::new(WorkspaceId(workspace_identity(&root)), root, 80, 24);
        let (damage, damage_events) = mpsc::channel();
        let server = Self {
            session: Mutex::new(session),
            panes: Mutex::new(BTreeMap::new()),
            clients: Mutex::new(Vec::new()),
            stopped: Mutex::new(false),
            endpoint,
            damage,
        };
        let first = server
            .session
            .lock()
            .expect("session poisoned")
            .selected_tab()
            .focus
            .pane;
        server.spawn_pane(first)?;
        Ok((server, damage_events))
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
                } => {
                    let pane = self
                        .session
                        .lock()
                        .expect("session poisoned")
                        .add_tab(label, columns, rows);
                    if self.spawn_pane(pane).is_err() {
                        let _ = events.send(ClientEvent::Error {
                            message: "could not create terminal pane".into(),
                        });
                    }
                    self.broadcast_session();
                }
                ClientRequest::SelectTab { tab } => {
                    let mut session = self.session.lock().expect("session poisoned");
                    if session
                        .workspace
                        .tabs
                        .iter()
                        .any(|candidate| candidate.id == tab)
                    {
                        session.workspace.selected_tab = tab;
                    }
                    drop(session);
                    self.broadcast_session();
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

    fn spawn_pane(&self, pane_id: PaneId) -> Result<(), RuntimeError> {
        let pane = find_pane(&self.session.lock().expect("session poisoned"), pane_id)
            .ok_or_else(|| RuntimeError::Protocol("unknown pane".into()))?;
        let runtime = PaneRuntime::spawn(
            pane_id,
            pane.cwd,
            pane.columns,
            pane.rows,
            self.damage.clone(),
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
    /// The last snapshot actually sent to clients, so
    /// [`PaneRuntime::damage_since_last`] can diff against what they
    /// already have instead of resending every cell on every PTY read.
    last_sent: Mutex<Option<PaneSnapshot>>,
}

#[derive(Clone)]
struct ReplySink(mpsc::Sender<Vec<u8>>);

impl EventListener for ReplySink {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(reply) = event {
            let _ = self.0.send(reply.into_bytes());
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
    ) -> Result<Self, RuntimeError> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| RuntimeError::Pty(error.to_string()))?;
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut command = CommandBuilder::new(shell);
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
        let comm = std::fs::read_to_string(format!("/proc/{pgid}/comm")).ok()?;
        Some((cwd, comm.trim().to_owned()))
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
            changed,
        };
        *last_sent = Some(current);
        damage
    }
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
    PaneSnapshot {
        pane,
        columns,
        rows,
        cursor: Cursor {
            column: content.cursor.point.column.0 as u16,
            row: content.cursor.point.line.0 as u16,
        },
        alternate_screen: content.mode.contains(TermMode::ALT_SCREEN),
        cells,
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
        .tabs
        .iter()
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
pub fn read_event<R: BufRead>(reader: &mut R) -> Result<Option<ClientEvent>, RuntimeError> {
    read_message(reader)
}
fn write_message<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<(), RuntimeError> {
    serde_json::to_writer(&mut *writer, value)
        .map_err(|error| RuntimeError::Protocol(error.to_string()))?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}
fn read_message<R: BufRead, T: DeserializeOwned>(
    reader: &mut R,
) -> Result<Option<T>, RuntimeError> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    serde_json::from_str(&line)
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
    use super::{PaneRuntime, ReplySink, snapshot, workspace_identity};
    use crate::{PaneId, TerminalColor};
    use alacritty_terminal::{
        Term,
        term::{Config, test::TermSize},
        vte::ansi::Processor,
    };
    use std::{
        path::{Path, PathBuf},
        thread,
        time::Duration,
    };
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
        let pane = PaneRuntime::spawn(PaneId(9), PathBuf::from("/tmp"), 80, 24, damage).unwrap();
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
        let pane = PaneRuntime::spawn(PaneId(7), PathBuf::from("/tmp"), 80, 24, damage).unwrap();
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
        let pane = PaneRuntime::spawn(PaneId(11), PathBuf::from("/tmp"), 80, 24, damage).unwrap();
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
}
