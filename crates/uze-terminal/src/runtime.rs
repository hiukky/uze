use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, BufReader, Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    os::unix::io::AsRawFd,
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
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
    CellAttributes, ClientEvent, ClientRequest, Cursor, MouseMode, OpenedSpace, PROTOCOL_VERSION,
    Palette, PaneDamage, PaneId, PaneSnapshot, RenderCell, Session, SpaceId, SpaceSeed, TabId,
    TabSeed, TerminalColor, WorkspaceId, process_probe,
};

/// ADR-038: the endpoint is local and user-private; no network transport is
/// exposed by this runtime.
#[derive(Debug, Error)]
pub enum RuntimeError {
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
    let _span = tracing::info_span!("terminal.attach", root = %root.display()).entered();
    let endpoint = Endpoint::global()?;
    // A server left running from a previous build (e.g. a `cargo install
    // --force` while it was still up) is *alive*, so the connect below
    // would succeed — this has to be caught before that, not after, since
    // some `PROTOCOL_VERSION` bumps changed the wire framing itself (see
    // its doc comment); there's no guarantee an incompatible server can
    // even parse an `Attach` request enough to answer with a clean
    // `ClientEvent::Error` rather than hanging the connection.
    if endpoint.socket.exists() {
        // The pid file is trusted on its own only where it is corroborated
        // by the process table: it names a live `uze` and records the
        // version this client speaks. Anything else — a recycled pid, a
        // version that does not match, a file that says nothing — is
        // settled by asking the socket who is behind it, which is the one
        // answer nothing can forge and the one that can also rescue a live
        // server of this build whose pid file a cleaner took away.
        match recorded_compatibility(&endpoint.pid) {
            Compatibility::Known if pid_file_names_a_server(&endpoint.pid) => {}
            _ => match probe_server(&endpoint) {
                Probe::Speaks { pid } => heal_pid_file(&endpoint, pid),
                Probe::Foreign { peer } => replace_incompatible_server(&endpoint, peer)?,
            },
        }
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
    let _span = tracing::info_span!("terminal.open_space", root = %root.display()).entered();
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

/// Stops the user's server, and says so when there was nothing to stop.
///
/// "Nothing is running" is the ordinary state of this command, not a
/// failure: after a reboot, after the server exited, and — on WSL — after
/// a `/tmp` cleaner took the socket out from under a server that was
/// running. A missing socket and a socket nobody is listening on are both
/// that state, and reporting them as errors made every teardown script and
/// journey run end on a failure it was right to ignore.
pub fn stop(_root: &Path) -> Result<(), RuntimeError> {
    let _span = tracing::info_span!("terminal.stop", root = %_root.display()).entered();
    let endpoint = Endpoint::global()?;
    let mut stream = match UnixStream::connect(&endpoint.socket) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
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
    let _span = tracing::info_span!("terminal.serve", root = %root.display()).entered();
    let endpoint = Endpoint::global()?;
    // The workspace is claimed before the endpoint is: `Server::new` takes
    // the lock that makes this the one server restoring these spaces, so by
    // the time the socket is bound no other live server can own it — which
    // is what lets [`spawn_endpoint_watch`] treat a socket that is no longer
    // the one bound here as something to reclaim rather than a peer's.
    let (server, damage) = Server::new(root, endpoint.clone())?;
    let state = Arc::new(server);
    recover_stale_endpoint(&endpoint)?;
    let listener = bind_endpoint(&endpoint)?;
    spawn_damage_broadcaster(Arc::clone(&state), damage);
    spawn_status_ticker(Arc::clone(&state));
    spawn_endpoint_watch(Arc::clone(&state));

    let accepted = accept_connections(listener, Arc::clone(&state));
    state.stop_panes();
    {
        // Under the same flag [`spawn_endpoint_watch`] holds while it
        // decides whether to rebind, so this clears an endpoint the watch
        // cannot then put back — and the watch, if it is mid-rebind,
        // finishes before the clearing rather than after it. Set here too
        // because `accept_connections` can also return on an accept error,
        // with nobody having asked the server to stop.
        let mut stopped = state.stopped.lock().expect("stop state poisoned");
        *stopped = true;
        let _ = fs::remove_file(&endpoint.socket);
        let _ = fs::remove_file(&endpoint.pid);
    }
    accepted
}

/// Binds the endpoint and records who is behind it. The socket is created
/// inside a directory [`Endpoint::global`] has already proven to be this
/// user's and unreachable by anyone else, so the moment between `bind` and
/// the mode below is not a window anything can walk through.
fn bind_endpoint(endpoint: &Endpoint) -> Result<UnixListener, RuntimeError> {
    let listener = UnixListener::bind(&endpoint.socket)?;
    fs::set_permissions(&endpoint.socket, fs::Permissions::from_mode(0o600))?;
    write_pid_file(&endpoint.pid, std::process::id())?;
    Ok(listener)
}

fn accept_connections(listener: UnixListener, server: Arc<Server>) -> Result<(), RuntimeError> {
    for stream in listener.incoming() {
        let stream = stream?;
        let client_state = Arc::clone(&server);
        thread::spawn(move || client_state.handle_client(stream));
        if server
            .stopped
            .lock()
            .expect("stop state poisoned")
            .to_owned()
        {
            break;
        }
    }
    Ok(())
}

#[derive(Clone)]
struct Endpoint {
    socket: PathBuf,
    pid: PathBuf,
}

/// How long a Unix-domain socket path may be, with room to spare.
///
/// `sockaddr_un.sun_path` holds 104 bytes on macOS and 108 on Linux, and the
/// whole path has to fit or `bind` fails with `SUN_LEN` — an error naming the
/// limit and nothing about which directory exhausted it. The smaller of the
/// two, less a little, is what [`Endpoint::global`] holds itself to, so the
/// same directory is usable on either platform.
const MAX_SOCKET_PATH: usize = 100;

impl Endpoint {
    /// One endpoint per user — per `UZE_HOME`, which is what "user" means
    /// to UZE: a second home is a second world, with a server of its own.
    ///
    /// The directory is whichever of three candidates can hold the socket:
    /// the runtime directory the session names, an owner-scoped directory in
    /// the system temp dir, and `/tmp`. Two things disqualify one — being
    /// unwritable, and being too long.
    ///
    /// Length matters more than it looks. `XDG_RUNTIME_DIR` is somebody
    /// else's variable and can be arbitrarily deep, and the system temp dir
    /// on macOS is a per-user `/var/folders/<hash>/T` that already spends
    /// half the budget before UZE adds anything. Falling back does not
    /// weaken isolation: the socket is named after a hash of `UZE_HOME`, so
    /// two homes stay two endpoints wherever they land.
    fn global() -> Result<Self, RuntimeError> {
        let identity = identity_of(&uze_home_dir());
        let named = |root: &Path| root.join(format!("uze-{identity}.sock"));
        let owner = unsafe { libc::getuid() };

        let candidates = [
            env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(env::temp_dir)
                .join(format!("uze-runtime-{owner}")),
            env::temp_dir().join(format!("uze-runtime-{owner}")),
            PathBuf::from("/tmp").join(format!("uze-runtime-{owner}")),
        ];

        let mut refused = None;
        let runtime = candidates
            .into_iter()
            .find(|candidate| {
                if named(candidate).as_os_str().len() > MAX_SOCKET_PATH {
                    return false;
                }
                // A sandboxed terminal can expose a runtime directory while
                // denying writes below it, and a directory that already
                // exists may be somebody else's — either way the next
                // candidate is tried rather than the whole attach failing.
                match fs::create_dir_all(candidate)
                    .and_then(|()| private_directory(candidate, owner))
                {
                    Ok(()) => true,
                    Err(error) => {
                        refused = Some(error);
                        false
                    }
                }
            })
            .ok_or_else(|| {
                refused.unwrap_or_else(|| {
                    io::Error::other(
                        "no runtime directory short enough for a socket path; \
                         set XDG_RUNTIME_DIR to a shorter one",
                    )
                })
            })?;

        Ok(Self {
            socket: named(&runtime),
            pid: runtime.join(format!("uze-{identity}.pid")),
        })
    }
}

/// Proves `candidate` is a directory `owner` owns and nobody else can reach
/// into — the condition for putting a socket in it that carries every
/// pane's contents and accepts input into every agent.
///
/// Existing is not evidence of anything. `create_dir_all` answers `Ok(())`
/// for a path that is already there, *including a symlink to a directory*,
/// and `set_permissions` follows symlinks. Where no `XDG_RUNTIME_DIR` is
/// set — WSL, containers, CI, any non-logind shell — the runtime directory
/// lands in a world-writable temp dir under a name any local user can
/// predict and create first. `symlink_metadata` is what asks about the
/// entry itself rather than about whatever it points at.
fn private_directory(candidate: &Path, owner: libc::uid_t) -> io::Result<()> {
    let metadata = fs::symlink_metadata(candidate)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::other(format!(
            "{} is not a directory",
            candidate.display()
        )));
    }
    if metadata.uid() != owner {
        return Err(io::Error::other(format!(
            "{} belongs to another user",
            candidate.display()
        )));
    }
    // Ours, so a mode that lets anyone else in is ours to correct rather
    // than to refuse — this is the ordinary first-run path when the umask
    // is permissive.
    if metadata.mode() & 0o077 != 0 {
        fs::set_permissions(candidate, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
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

/// The file whose advisory lock says which process is serving the
/// persisted workspace — beside the workspace itself, under `$UZE_HOME`,
/// never in the runtime directory a `/tmp` cleaner can take away.
fn workspace_lock_path() -> PathBuf {
    persisted_state_path().with_extension("lock")
}

/// Held for the life of a server: the proof that no other process is
/// restoring — and persisting over — the same workspace.
///
/// The endpoint alone cannot give that proof. `systemd-tmpfiles` wiping
/// `/tmp` under a live server takes the socket *and* the pid file with it,
/// so the next `attach` reads "no server", starts a second one, and both
/// restore the same `workspace.json`: every agent exists twice in the same
/// checkout, and the two servers persist over each other. This lock lives
/// where the workspace lives, so a cleaner that can reach it has taken the
/// workspace too.
///
/// `flock` and not a pid file: the kernel releases it when the holder dies,
/// however it dies, so a crashed server leaves nothing to clean up and a
/// stale claim is impossible by construction.
struct WorkspaceLock {
    _file: fs::File,
}

impl WorkspaceLock {
    fn acquire() -> Result<Self, RuntimeError> {
        let path = workspace_lock_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;
        loop {
            // SAFETY: `file` owns the descriptor for the whole call, and the
            // lock it takes is released by the kernel when this process exits.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Self { _file: file });
            }
            let refusal = io::Error::last_os_error();
            match classify_lock_refusal(refusal.raw_os_error()) {
                LockRefusal::Interrupted => continue,
                LockRefusal::Contended => {
                    return Err(RuntimeError::Protocol(
                        "another uze terminal server is already serving this workspace".into(),
                    ));
                }
                LockRefusal::Unsupported => return Err(RuntimeError::Io(refusal)),
            }
        }
    }
}

/// Why `flock` said no.
///
/// Only one of its answers means another server holds the workspace. A
/// signal arriving mid-call is not an answer at all, and a filesystem that
/// cannot lock — `ENOLCK`, and the `EOPNOTSUPP`/`ENOSYS` some NFS, FUSE and
/// 9p mounts give — is a different failure entirely: reading either as
/// contention told the person to go and stop a server that does not exist,
/// permanently, with no command that could clear it.
enum LockRefusal {
    Interrupted,
    Contended,
    Unsupported,
}

fn classify_lock_refusal(errno: Option<i32>) -> LockRefusal {
    match errno {
        Some(libc::EINTR) => LockRefusal::Interrupted,
        // The same number on Linux, two names elsewhere; both mean held.
        Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN => LockRefusal::Contended,
        _ => LockRefusal::Unsupported,
    }
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

/// Replaces `path`'s contents in one step, so a reader only ever sees the
/// old file or the new one.
///
/// The whole workspace is rewritten on every structural change, and a plain
/// write truncates before it fills: a crash, a `kill -9`, a full disk or a
/// power loss in that window leaves a half-written file, which
/// [`load_persisted_workspace`] cannot parse and therefore reads as
/// "nothing persisted yet" — every space, tab and agent the person had,
/// gone, with no error anywhere. The temporary is a sibling so the rename
/// stays inside one filesystem, and the bytes reach the disk before the
/// name does.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

/// The widest and tallest a pane may be told it is.
///
/// `columns` and `rows` arrive from a peer as `u16` and go into
/// `Term::resize`, which allocates a cell per grid position and clamps
/// nothing of its own: 65535×65535 asks for about 137 GB, and Rust aborts
/// the process on an allocation it cannot serve — taking the server and
/// every live agent pane with it, from one malformed frame a buggy client
/// can send as easily as a hostile one. A merely large size survives the
/// allocation and then serializes a multi-gigabyte repaint, which is the
/// same outage more slowly.
const MAX_PANE_DIMENSION: u16 = 1000;

/// Brings a client-supplied dimension inside what a pane can be. Zero keeps
/// the meaning it has at every call site — "leave this pane's dimensions
/// alone" — so it is passed through rather than raised to one.
fn within_pane_bounds(dimension: u16) -> u16 {
    dimension.min(MAX_PANE_DIMENSION)
}

/// The same bound where a pane is actually created, which has no "leave it
/// alone" to express: a grid of nothing is not a terminal.
fn spawnable_pane_bounds(dimension: u16) -> u16 {
    dimension.clamp(1, MAX_PANE_DIMENSION)
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
/// A name, never a path. What this reads is the *name a live process
/// reports*, and a process can choose what that says — `UZE_SHIM_NAME` is
/// an ordinary environment variable, so a script run once in a pane can
/// set it to anything. Whatever comes back here is persisted and then
/// spawned by the server on the next restart, so a candidate carrying a
/// separator (`/tmp/payload`) is refused: relaunching resolves a command
/// through `PATH` like a person typing it, and never a path this pane
/// chose.
fn relaunch_command_for_process(process: &str) -> Option<Vec<String>> {
    let trimmed = process.trim();
    if trimmed.is_empty() || trimmed.contains('/') || PLAIN_SHELL_PROCESS_NAMES.contains(&trimmed) {
        return None;
    }
    Some(vec![trimmed.to_owned()])
}

fn start_server(root: &Path, endpoint: &Endpoint) -> Result<(), RuntimeError> {
    use std::os::unix::process::CommandExt;

    let executable = env::current_exe()?;
    let child = std::process::Command::new(executable)
        .args(["terminal", "serve", "--root"])
        .arg(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        // A process group of its own, or the server sits in the launching
        // terminal's: a `SIGHUP` when that terminal closes, or a `Ctrl+C`
        // to its foreground group, would take down every pane — precisely
        // the property this runtime exists to hold (ADR-038).
        .process_group(0)
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
    if unsafe { libc::kill(pid, 0) } != 0 {
        return Ok(false);
    }
    // A zombie is addressable too. The client that starts a server never
    // reaps it, so a server that crashed stays addressable for the rest of
    // that client's life — which left this answering "alive" and
    // `recover_stale_endpoint` refusing to clear an endpoint nothing was
    // serving, until the person quit `uze` itself. A process that has
    // actually exited no longer has an executable image, which is the
    // difference the probe can see; it is also what makes a *recycled* pid
    // running something else fail this check.
    Ok(!platform_reads_processes() || process_probe::executable_of(pid as u32).is_some())
}

/// Whether [`process_probe`] can answer about a live process at all here,
/// established by asking it about this one. Where it cannot, `None` means
/// "cannot say" and never "dead", and every reading built on it has to
/// fall back to what `kill(pid, 0)` alone can prove.
fn platform_reads_processes() -> bool {
    process_probe::executable_of(std::process::id()).is_some()
}

/// Whether `pid` is running `uze` — the proof required before signalling a
/// process a file claims is a server.
///
/// A pid file outlives the process that wrote it: under `/tmp` it survives
/// a reboot, and pids are recycled, so the number in it is a claim rather
/// than a fact. The *path* is expected to differ (an upgrade moving the
/// binary is the ordinary reason a server is being replaced), so the image's
/// file name is what is compared — through Linux's marker for a binary
/// replaced underneath a live process, which is exactly the state the
/// server being replaced is in.
fn runs_uze(pid: libc::pid_t) -> bool {
    let Some(image) = process_probe::executable_of(pid as u32) else {
        return false;
    };
    let Some(name) = image.file_name() else {
        return false;
    };
    let name = name.to_string_lossy();
    name.strip_suffix(" (deleted)").unwrap_or(&name) == "uze"
}

/// Whether the process table corroborates a pid file's claim that `pid` is
/// a server. Where nothing can corroborate it — a platform
/// [`process_probe`] cannot answer for, or Linux with `/proc` unmounted —
/// the answer is no, not "take the file's word for it": the only thing this
/// gates is signalling a process, and declining to signal loses nothing,
/// because unlinking the endpoint files is the whole recovery there anyway.
fn corroborated_as_server(pid: libc::pid_t) -> bool {
    platform_reads_processes() && runs_uze(pid)
}

/// Whether the pid file's claim is good enough to skip the probe in
/// [`attach`] — a different question from [`corroborated_as_server`]'s, and
/// deliberately the more forgiving of the two. Being wrong here costs one
/// probe; being wrong there costs a process. So where the process table
/// cannot answer, the file stands, exactly as it did before there was a
/// process table to ask — reading "cannot say" as "not a server" would send
/// every attach down the replace path and unlink the endpoint of a server
/// that is alive and serving.
fn pid_file_names_a_server(pid_path: &Path) -> bool {
    read_pid(pid_path).is_some_and(|pid| !platform_reads_processes() || runs_uze(pid))
}

/// A pid file's first line — see [`write_pid_file`] — and only where it
/// names one process.
///
/// `libc::pid_t` is signed, so `-1` parses, and `kill(-1, …)` is not a
/// process: it is every process the user owns. UZE never writes such a
/// file, but this one is read from a world a `/tmp` cleaner, a crash and a
/// text editor all reach, and the number in it goes straight to
/// [`libc::kill`]. `0` is the caller's own process group, refused for the
/// same reason.
fn read_pid(pid_path: &Path) -> Option<libc::pid_t> {
    let text = fs::read_to_string(pid_path).ok()?;
    text.lines()
        .next()?
        .trim()
        .parse::<libc::pid_t>()
        .ok()
        .filter(|pid| *pid > 0)
}

/// The `PROTOCOL_VERSION` the server holding this pid file was compiled
/// with, from the file's second line — `None` for a pid file that is
/// missing, unreadable or corrupt. `None` is silence, not an answer: what
/// [`attach`] makes of it is [`Compatibility::Unrecorded`]'s business.
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

/// What the pid file says about the server holding the endpoint.
enum Compatibility {
    /// It recorded the version this client speaks.
    Known,
    /// It recorded a different one: alive, and not worth connecting to.
    Mismatched,
    /// It records nothing — missing or unreadable. Silence is not evidence
    /// of a mismatch: a runtime directory that fell back to `/tmp` can lose
    /// its pid file to the distro's own cleaner while the server it named is
    /// still serving, and reading that as a mismatch unlinks a live session's
    /// socket and strands its panes behind a server nothing can reach.
    Unrecorded,
}

fn recorded_compatibility(pid_path: &Path) -> Compatibility {
    match server_protocol_version(pid_path) {
        Some(version) if version == PROTOCOL_VERSION => Compatibility::Known,
        Some(_) => Compatibility::Mismatched,
        None => Compatibility::Unrecorded,
    }
}

/// Who is behind the socket, asked of the socket itself because the pid
/// file could not say.
enum Probe {
    /// The listener is running this very executable, and so was compiled
    /// with this `PROTOCOL_VERSION` — a proof the wire cannot give more
    /// cheaply, since a server built to another framing may never answer
    /// the handshake that would ask it (see [`attach`]).
    Speaks { pid: u32 },
    /// Another build, another program, or nobody at all — carrying whoever
    /// the kernel says is actually behind the socket, when anyone is. That
    /// pid is the one piece of evidence a pid file cannot forge and a
    /// recycled pid cannot survive, so [`replace_incompatible_server`]
    /// takes its kill decision from it rather than from the file.
    Foreign { peer: Option<u32> },
}

fn probe_server(endpoint: &Endpoint) -> Probe {
    let peer = listening_peer(&endpoint.socket);
    match peer.filter(|pid| runs_this_executable(*pid)) {
        Some(pid) => Probe::Speaks { pid },
        None => Probe::Foreign { peer },
    }
}

/// The pid listening on `socket`. The kernel stamps the peer's credentials
/// onto the connection, so this is the listener's own and not something a
/// connection could claim. `None` when nobody answers, or when the
/// platform cannot say.
fn listening_peer(socket: &Path) -> Option<u32> {
    let stream = UnixStream::connect(socket).ok()?;
    process_probe::peer_pid(&stream)
}

/// Whether `pid` runs the same executable image as this process — and so
/// was compiled with this `PROTOCOL_VERSION`. The image stops resolving to
/// this path once the binary is replaced underneath a live server (a
/// `cargo install --force` mid-session), which is exactly the state a
/// server being replaced is in.
fn runs_this_executable(pid: u32) -> bool {
    let Some(mine) = env::current_exe().ok() else {
        return false;
    };
    process_probe::executable_of(pid).is_some_and(|image| image == mine)
}

/// Records what the probe established, so the next attach reads the answer
/// instead of deriving it again. Best-effort: a write that fails leaves
/// exactly the state just recovered from, and the probe stands without it.
fn heal_pid_file(endpoint: &Endpoint, pid: u32) {
    let _ = write_pid_file(&endpoint.pid, pid);
}

/// Tears down a server that's alive but speaking a `PROTOCOL_VERSION` this
/// client can't talk to, so `attach`'s subsequent connect lands on a fresh
/// one instead. Unlike [`recover_stale_endpoint`] (a dead owner, socket
/// already unusable), this owner is alive and mid-session, so it gets a
/// cooperative `SIGTERM` first — its own persisted-workspace snapshot (see
/// `persist`) is what lets the fresh server restore the same tabs — with
/// `SIGKILL` only as a last resort if it doesn't exit promptly.
fn replace_incompatible_server(endpoint: &Endpoint, peer: Option<u32>) -> Result<(), RuntimeError> {
    // Two independent witnesses have to name the same process before it is
    // signalled, because what follows is fatal to whatever that pid names —
    // an editor, a build, another agent — on an upgrade path a person takes
    // deliberately. `peer` is the kernel's own answer to "who is behind
    // this socket", which nothing can forge and a recycled pid cannot
    // survive; the pid file is a claim that outlives its writer (see
    // [`runs_uze`]), and `uze` is not a distinguishing name — a second
    // server under a second `UZE_HOME`, or an in-flight `uze install`, is
    // one too. Where the two do not agree, or where nobody is listening at
    // all, clearing the endpoint files below is the whole recovery: the
    // next connect then lands on a fresh server.
    let named_by_both = peer
        .and_then(|peer| libc::pid_t::try_from(peer).ok())
        .filter(|peer| read_pid(&endpoint.pid) == Some(*peer));
    if let Some(pid) = named_by_both.filter(|pid| corroborated_as_server(*pid)) {
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
    /// Held for as long as this server exists — see [`WorkspaceLock`].
    _workspace: WorkspaceLock,
    /// Serializes [`Server::persist`], so two structural changes landing at
    /// once cannot rename an older picture of the workspace over a newer
    /// one.
    persisting: Mutex<()>,
    /// Cloned into every [`PaneRuntime`] so its PTY reader thread can report
    /// new output; [`spawn_damage_broadcaster`] owns the matching receiver.
    damage: mpsc::Sender<PaneId>,
    /// What a pane's own program is told when it asks the terminal what
    /// colours it is drawn in. Shared with every pane already running, so a
    /// client changing theme changes the answer everywhere at once rather
    /// than only for panes opened afterwards.
    palette: Arc<Mutex<Palette>>,
}

impl Server {
    fn new(
        root: PathBuf,
        endpoint: Endpoint,
    ) -> Result<(Self, mpsc::Receiver<PaneId>), RuntimeError> {
        // Taken before anything is read: restoring a workspace a live
        // server already holds is what turns one set of agents into two.
        let workspace_lock = WorkspaceLock::acquire()?;
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
            _workspace: workspace_lock,
            persisting: Mutex::new(()),
            damage,
            palette: Arc::new(Mutex::new(Palette::default())),
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
        let _writing = self.persisting.lock().expect("persist state poisoned");
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
        match serde_json::to_vec(&workspace) {
            Ok(json) => {
                if let Err(error) = write_atomically(&path, &json) {
                    tracing::warn!(path = %path.display(), %error, "could not persist the workspace");
                }
            }
            Err(error) => {
                tracing::warn!(%error, "could not describe the workspace to persist it")
            }
        }
    }

    fn handle_client(self: Arc<Self>, stream: UnixStream) {
        let _span = tracing::info_span!("terminal.client").entered();
        let reader_stream = match stream.try_clone() {
            Ok(value) => value,
            Err(_) => return,
        };
        let (events, receiver) = mpsc::channel();
        thread::spawn(move || forward_events(stream, &receiver));

        // A deadline on the handshake only — see [`HANDSHAKE_DEADLINE`] —
        // and a frame limit sized for what a handshake actually says rather
        // than for the largest repaint this wire ever carries: nothing has
        // vouched for this peer yet.
        let mut reader = BufReader::new(Handshake::new(reader_stream, HANDSHAKE_DEADLINE));
        let first = read_message_within::<_, ClientRequest>(&mut reader, MAX_HANDSHAKE_FRAME);
        let attached = match first {
            // Stopping needs no client and no session, and a server whose
            // pid file or corroboration has gone is one only this can
            // reach: the workspace lock makes a survivor refuse every
            // replacement, so `uze terminal stop` failing to be heard left
            // no way back in but a manual `kill`.
            Ok(Some(ClientRequest::Stop)) => {
                // Answered on the socket rather than through the writer
                // thread: the acknowledgement has to be on the wire before
                // the accept loop is woken, or the process can exit out
                // from under a frame still sitting in a channel. Nothing
                // else is ever sent on this connection — no client was
                // registered — so there is nothing for this to interleave
                // with.
                let answered = write_message(reader.get_mut().socket(), &ClientEvent::Stopped);
                if let Err(error) = answered {
                    tracing::warn!(%error, "could not acknowledge a stop request");
                }
                self.shut_down();
                return;
            }
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
                    self.resize_pane(
                        self.selected_pane_of(client),
                        within_pane_bounds(columns),
                        within_pane_bounds(rows),
                    );
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
        // Attached, so silence is a person reading rather than a peer
        // holding threads it never intends to use.
        reader.get_mut().attached();

        while let Ok(Some(request)) = read_message::<_, ClientRequest>(&mut reader) {
            // A keystroke is a request too, and there are thousands: debug
            // level, so a trace of the server is what a person did to it
            // unless they asked for every byte.
            let _span =
                tracing::debug_span!("terminal.request", kind = request.kind(), client).entered();
            match request {
                ClientRequest::Detach => {
                    let _ = events.send(ClientEvent::Detached);
                    break;
                }
                ClientRequest::SetPalette(palette) => self.set_palette(palette),
                ClientRequest::Input { pane, bytes } => self.write_input(pane, &bytes),
                ClientRequest::Scroll { pane, lines } => self.scroll_pane(pane, lines),
                ClientRequest::Resize {
                    pane,
                    columns,
                    rows,
                } => self.resize_pane(pane, within_pane_bounds(columns), within_pane_bounds(rows)),
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
                        let pane = session.add_tab(
                            space,
                            label,
                            agent,
                            within_pane_bounds(columns),
                            within_pane_bounds(rows),
                            cwd,
                        );
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
                ClientRequest::ReorderTab { tab, before } => {
                    let changed = self
                        .session
                        .lock()
                        .expect("session poisoned")
                        .reorder_tab(tab, before);
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
                    // Always a new space, even over a directory another
                    // space already holds (see `Session::create_space`):
                    // the prompt asked for a space, and one repository is
                    // routinely worth two — one per branch, one per thing
                    // being tried. `ensure_space` is the other question.
                    let (space, pane) = {
                        let mut session = self.session.lock().expect("session poisoned");
                        let pane = session.create_space(
                            label,
                            root,
                            within_pane_bounds(columns),
                            within_pane_bounds(rows),
                        );
                        (session.workspace.selected_space, pane)
                    };
                    if self.spawn_pane(pane, None).is_err() {
                        let _ = events.send(ClientEvent::Error {
                            message: "could not create terminal pane".into(),
                        });
                    }
                    self.update_selection(client, |selection| selection.space = Some(space));
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
                    let _ = events.send(ClientEvent::Stopped);
                    self.shut_down();
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
        let opened = {
            let mut session = self.session.lock().expect("session poisoned");
            session.open_space(None, root.to_path_buf(), 80, 24)
        };
        match opened {
            OpenedSpace::Existing(space) => Ok(space),
            OpenedSpace::Created { space, pane } => {
                self.spawn_pane(pane, None)?;
                Ok(space)
            }
        }
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
    /// Takes the attached client's palette. Every pane shares the one
    /// `Arc`, so panes that were already running answer with it too — a
    /// theme switch that only reached panes opened afterwards would leave
    /// the older ones telling their programs a colour nobody draws.
    fn set_palette(&self, palette: Palette) {
        if let Ok(mut held) = self.palette.lock() {
            *held = palette;
        }
    }

    fn spawn_pane(&self, pane_id: PaneId, command: Option<&[String]>) -> Result<(), RuntimeError> {
        let pane = find_pane(&self.session.lock().expect("session poisoned"), pane_id)
            .ok_or_else(|| RuntimeError::Protocol("unknown pane".into()))?;
        let runtime = PaneRuntime::spawn(
            pane_id,
            pane.cwd,
            // The session's own record of a pane's size is bounded here too,
            // not only where a request arrives: it can come back from a
            // persisted workspace written by an older build that never
            // clamped one.
            spawnable_pane_bounds(pane.columns),
            spawnable_pane_bounds(pane.rows),
            self.damage.clone(),
            command,
            Arc::clone(&self.palette),
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

    /// Repaints every pane on every attached client, as one frame per pane.
    ///
    /// One frame carrying them all is what [`MAX_FRAME`] does *not* bound:
    /// the cap is tied to a repaint of the largest single pane a client may
    /// ask for, so three panes at [`MAX_PANE_DIMENSION`] — a size
    /// `within_pane_bounds` permits, and one that is persisted across
    /// restarts — made the frame unsendable and every attached client sit
    /// frozen on live-looking chrome. Per pane, that cap is the real bound
    /// again.
    ///
    /// The `Snapshot` goes out first with no panes on it: it is what tells
    /// a client to forget the panes it has, and the repaints that follow
    /// are what give it the new ones. They are ordinary `Damage` frames
    /// naming every cell, which is what a client already applies to a pane
    /// it has never heard of (and what a resize already sends), so no
    /// client has to learn anything to read this.
    fn broadcast_snapshot(&self) {
        let session = self.session.lock().expect("session poisoned").clone();
        // The runtimes, not their grids: a repaint is built and handed on
        // one pane at a time, so the largest thing alive at once stays one
        // pane's worth rather than the whole workspace's.
        let panes: Vec<Arc<PaneRuntime>> = self
            .panes
            .lock()
            .expect("panes poisoned")
            .values()
            .cloned()
            .collect();
        self.clients
            .lock()
            .expect("clients poisoned")
            .retain(|client| {
                client
                    .events
                    .send(ClientEvent::Snapshot {
                        session: view_for(&session, &client.selection),
                        panes: Vec::new(),
                    })
                    .is_ok()
            });
        for pane in panes {
            let repaint = whole_pane(pane.snapshot_and_remember());
            self.clients
                .lock()
                .expect("clients poisoned")
                .retain(|client| {
                    client
                        .events
                        .send(ClientEvent::Damage(repaint.clone()))
                        .is_ok()
                });
        }
    }

    fn stop_panes(&self) {
        for pane in self.panes.lock().expect("panes poisoned").values() {
            pane.stop();
        }
    }

    /// Takes the server down: no new work, no live panes, and one
    /// connection of its own so [`accept_connections`] wakes from `accept`
    /// and reads the flag instead of blocking until somebody happens to
    /// attach.
    fn shut_down(&self) {
        *self.stopped.lock().expect("stop state poisoned") = true;
        self.stop_panes();
        let _ = UnixStream::connect(&self.endpoint.socket);
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

/// Puts back the directory the endpoint lives in, held to the same
/// ownership and mode [`Endpoint::global`] demanded of it in the first
/// place — a cleaner that took the socket usually took the directory too.
fn restore_endpoint_directory(endpoint: &Endpoint) -> io::Result<()> {
    let Some(directory) = endpoint.socket.parent() else {
        return Ok(());
    };
    fs::create_dir_all(directory)?;
    private_directory(directory, unsafe { libc::getuid() })
}

/// What identifies the socket a server bound, so a later look at the same
/// path can tell "still the one I am listening on" from "gone".
fn socket_identity(path: &Path) -> Option<(u64, u64)> {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.dev(), metadata.ino()))
}

/// Puts the server back at its endpoint when the endpoint stops being the
/// one it bound.
///
/// The recorded WSL case: `systemd-tmpfiles` wipes `/tmp` about forty
/// seconds after login, taking the socket and the pid file out from under a
/// live server. The server never notices — it holds an open listener on an
/// unlinked inode — so the next `attach` reads "no socket, no pid" as "no
/// server" and starts a second one. The [`WorkspaceLock`] now stops that
/// second server from restoring the same workspace, and this is the other
/// half: the original reappears where clients look for it.
///
/// Reclaiming rather than yielding is safe *because* of that lock. This
/// process holds it, so anything now sitting at the path is not another
/// server of this workspace.
///
/// The stop flag is held across the whole check-and-rebind, not merely
/// read at the top. Reading it and then rebinding races the teardown at
/// the end of [`serve`]: a shutdown landing between the two leaves a server
/// on its way out rebinding a socket nothing will ever remove, and an
/// endpoint pointing at a dead pid. `serve` takes the same lock before it
/// clears the endpoint, which makes "rebound, then cleared" and "stopped,
/// so never rebound" the only two orderings there are.
fn spawn_endpoint_watch(server: Arc<Server>) {
    thread::spawn(move || {
        let mut bound = socket_identity(&server.endpoint.socket);
        loop {
            thread::sleep(STATUS_PROBE_INTERVAL);
            let stopped = server.stopped.lock().expect("stop state poisoned");
            if *stopped {
                break;
            }
            if socket_identity(&server.endpoint.socket) == bound {
                continue;
            }
            let _ = fs::remove_file(&server.endpoint.socket);
            match restore_endpoint_directory(&server.endpoint)
                .map_err(RuntimeError::from)
                .and_then(|()| bind_endpoint(&server.endpoint))
            {
                Ok(listener) => {
                    tracing::warn!(
                        socket = %server.endpoint.socket.display(),
                        "the terminal endpoint vanished under a live server; rebound it"
                    );
                    bound = socket_identity(&server.endpoint.socket);
                    let accepting = Arc::clone(&server);
                    // The listener this replaces is left blocked in
                    // `accept` on an inode nothing can reach any more, so it
                    // costs one idle thread and answers nobody.
                    thread::spawn(move || {
                        if let Err(error) = accept_connections(listener, accepting) {
                            tracing::warn!(%error, "the rebound terminal endpoint stopped accepting");
                        }
                    });
                }
                Err(error) => {
                    tracing::warn!(%error, "could not rebind the terminal endpoint")
                }
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

/// Answers a pane's own program, including its OSC 10/11 colour queries.
///
/// The palette is shared rather than copied: a client that changes theme
/// sends the new one, and every pane already running has to start answering
/// with it. Two hardcoded colours used to live here, transcribed from the
/// TUI's palette — a program asking what the background is would have been
/// told a colour nobody was drawing the moment either copy moved.
#[derive(Clone)]
struct ReplySink {
    replies: mpsc::Sender<Vec<u8>>,
    palette: Arc<Mutex<Palette>>,
}

impl ReplySink {
    fn new(replies: mpsc::Sender<Vec<u8>>, palette: Arc<Mutex<Palette>>) -> Self {
        Self { replies, palette }
    }

    fn color(&self, index: usize) -> Option<Rgb> {
        let palette = self.palette.lock().ok()?;
        let (r, g, b) = if index == NamedColor::Foreground as usize {
            palette.foreground
        } else if index == NamedColor::Background as usize {
            palette.background
        } else {
            *palette.ansi.get(index)?
        };
        Some(Rgb { r, g, b })
    }
}

impl EventListener for ReplySink {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(reply) => {
                let _ = self.replies.send(reply.into_bytes());
            }
            // `Term::dynamic_color_sequence` (OSC 10/11/12 queries) never
            // sends a `PtyWrite` itself — it hands back a formatting
            // closure expecting the *caller* to resolve the color and
            // write the reply. Left unhandled, a query like Codex's OSC 11
            // background probe just hangs until it times out server-side,
            // so the query answers here instead of falling through.
            Event::ColorRequest(index, format) => {
                if let Some(color) = self.color(index) {
                    let _ = self.replies.send(format(color).into_bytes());
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
        palette: Arc<Mutex<Palette>>,
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
        // `CommandBuilder` seeds a pane from *this* process's environment,
        // and this process is the server — started by whatever `uze`
        // invocation first needed one, which in this project is routinely a
        // `uze` run from inside a shimmed agent. Without this every plain
        // shell would inherit that agent's identity stamp, report as the
        // agent in the sidebar, persist as one, and be relaunched as one on
        // the next restart. A pane's environment may only carry what that
        // pane's own launch put there.
        for inherited in SHIM_IDENTITY_VARIABLES {
            command.env_remove(inherited);
        }
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
            ReplySink::new(reply_sender, palette),
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
    /// in the foreground of this pane — the same two facts `tmux` shows as
    /// `pane_current_path`/`pane_current_command`, asked of the kernel
    /// through [`process_probe`]. `None` when the platform cannot answer, or
    /// when the process exited between the group-leader lookup and the read.
    fn foreground_status(&self) -> Option<(PathBuf, String)> {
        let pgid = self
            .master
            .lock()
            .expect("master poisoned")
            .process_group_leader()?;
        let cwd = process_probe::current_directory_of(pgid)?;
        let process = shim_launched_name(pgid).or_else(|| process_probe::command_name_of(pgid))?;
        Some((cwd, process))
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

/// What uze's PATH shim (`src/shim.rs`) stamps on the process it `exec`s
/// into, and therefore what every descendant of that process inherits.
const SHIM_IDENTITY_VARIABLES: [&str; 2] = ["UZE_SHIM_NAME", "UZE_SHIM_PID"];

/// The alias uze's PATH shim (`src/shim.rs`) launched this process group's
/// leader under, if any — read from `UZE_SHIM_NAME` in its live
/// environment. The shim sets this immediately before `exec`ing into the
/// real binary, so it survives on the same pid for the rest of the
/// process's life, unlike `comm`, which a harness is free to overwrite
/// (Claude Code sets its own title to its version string, erasing the name
/// a person actually typed). `None` for anything not launched through the
/// shim — a bypassed launch, a harness that isn't shimmed, or a plain
/// shell — in which case `foreground_status` falls back to `comm`.
///
/// The name is accepted only from the process the shim stamped it on.
/// `UZE_SHIM_NAME` is an ordinary environment variable: every child of a
/// shimmed agent inherits it, so a shell running *under* one would
/// otherwise answer with its ancestor's identity. `UZE_SHIM_PID` carries
/// the pid the stamp was made for — the shim `exec`s, so that pid is the
/// agent's own — and an inherited pair no longer names the process it is
/// read from.
fn shim_launched_name(pgid: libc::pid_t) -> Option<String> {
    let stamped: libc::pid_t = process_probe::environment_value_of(pgid, "UZE_SHIM_PID")?
        .trim()
        .parse()
        .ok()?;
    if stamped != pgid {
        return None;
    }
    process_probe::environment_value_of(pgid, "UZE_SHIM_NAME")
}

fn cell_coordinates(index: usize, columns: u16, cell: RenderCell) -> (u16, u16, RenderCell) {
    let row = (index / usize::from(columns)) as u16;
    let column = (index % usize::from(columns)) as u16;
    (row, column, cell)
}

/// Writes one client's events onto its socket until there are no more, or
/// until one of them cannot be written.
///
/// A frame that will not go out ends the connection rather than the writer
/// alone. Dropping only this thread's dup of the socket leaves the reader
/// half open, so the peer sees no EOF: its `Attach` succeeded, nothing
/// follows, and it sits on chrome that still looks live while the events it
/// will never read pile up in a channel nobody drains. Shutting both
/// halves is what makes the failure arrive where a client can act on it —
/// as the disconnect it actually is.
fn forward_events(mut socket: UnixStream, events: &mpsc::Receiver<ClientEvent>) {
    while let Ok(event) = events.recv() {
        if let Err(error) = write_message(&mut socket, &event) {
            tracing::warn!(%error, "dropping a terminal client an event could not reach");
            let _ = socket.shutdown(std::net::Shutdown::Both);
            return;
        }
    }
}

/// A pane's whole grid said as damage — every cell "changed" — which is
/// how [`Server::broadcast_snapshot`] repaints one pane in one frame. The
/// same thing a resize already sends, and the shape the frame limit is
/// measured against (`a_full_repaint_of_the_largest_pane_fits_in_one_frame`
/// weighs a damage cell, the widest of the two).
fn whole_pane(snapshot: PaneSnapshot) -> PaneDamage {
    let columns = snapshot.columns;
    PaneDamage {
        pane: snapshot.pane,
        columns,
        rows: snapshot.rows,
        cursor: snapshot.cursor,
        alternate_screen: snapshot.alternate_screen,
        mouse: snapshot.mouse,
        bracketed_paste: snapshot.bracketed_paste,
        changed: snapshot
            .cells
            .into_iter()
            .enumerate()
            .map(|(index, cell)| cell_coordinates(index, columns, cell))
            .collect(),
    }
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
/// The largest frame either side of this wire will send or accept.
///
/// Both directions are bounded by the same number, so the two can never
/// disagree about what is sendable — and the number is chosen against the
/// largest thing this protocol legitimately carries: a full repaint of the
/// largest pane it allows, [`MAX_PANE_DIMENSION`] squared cells. A cap
/// below that would disconnect a client at the moment it resized, which is
/// why the two constants are tied rather than each picked on its own
/// (`a_full_repaint_of_the_largest_pane_fits_in_one_frame` holds them
/// together). Everything else on this wire is orders of magnitude smaller.
const MAX_FRAME: u32 = 64 * 1024 * 1024;

/// How long a connection may stay silent before it has said who it is.
///
/// Until a client sends `Attach` it holds a reader thread, a writer thread
/// and whatever it has allocated, and nothing caps how many such
/// connections there are. A peer with nothing to say is dropped instead of
/// held forever; once attached, silence is ordinary — a person is reading.
const HANDSHAKE_DEADLINE: Duration = Duration::from_secs(10);

/// The largest first frame a peer may send.
///
/// [`MAX_FRAME`] is sized for the largest *repaint* this wire carries, and
/// a repaint is a thing the server sends to a client it already knows. The
/// first frame is the opposite: nobody has vouched for the peer, and the
/// only two things it may legitimately say — `Attach`, naming a workspace
/// and a root, or `Stop` — are hundreds of bytes. Bounding it here rather
/// than at 64 MiB is the difference between a stranger reserving a path
/// and a stranger reserving memory.
const MAX_HANDSHAKE_FRAME: u32 = 64 * 1024;

/// Bounds the whole handshake, rather than each read that makes it up.
///
/// `SO_RCVTIMEO` restarts on every successful read, so a peer dribbling one
/// byte just inside the timeout holds a reader thread, a writer thread and
/// whatever it has allocated for as long as it likes — which is precisely
/// what [`HANDSHAKE_DEADLINE`] exists to prevent. One deadline over the
/// whole exchange is what that actually takes. [`Handshake::attached`]
/// disarms it once the peer has said who it is.
struct Handshake {
    socket: UnixStream,
    deadline: Option<Instant>,
}

impl Handshake {
    fn new(socket: UnixStream, within: Duration) -> Self {
        Self {
            socket,
            deadline: Some(Instant::now() + within),
        }
    }

    fn socket(&mut self) -> &mut UnixStream {
        &mut self.socket
    }

    /// Silence is a person reading from here on, not a peer holding
    /// threads it never intends to use.
    fn attached(&mut self) {
        self.deadline = None;
        let _ = self.socket.set_read_timeout(None);
    }
}

impl Read for Handshake {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if let Some(deadline) = self.deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the peer never said who it is",
                ));
            }
            // Best-effort: a platform that will not take one leaves the
            // read blocking, which is where it was before.
            let _ = self.socket.set_read_timeout(Some(remaining));
        }
        self.socket.read(buffer)
    }
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
        .ok()
        .filter(|len| *len <= MAX_FRAME)
        .ok_or_else(|| oversized_frame(bytes.len() as u64, MAX_FRAME))?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
fn read_message<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<Option<T>, RuntimeError> {
    read_message_within(reader, MAX_FRAME)
}

/// The same read held to a smaller bound than the wire's own — see
/// [`MAX_HANDSHAKE_FRAME`].
fn read_message_within<R: Read, T: DeserializeOwned>(
    reader: &mut R,
    limit: u32,
) -> Result<Option<T>, RuntimeError> {
    let mut len_bytes = [0u8; 4];
    match reader.read_exact(&mut len_bytes) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let len = u32::from_le_bytes(len_bytes);
    // Refused before it is allocated, not after. This prefix is the first
    // thing a peer says and the protocol version lives *inside* the frame
    // it describes, so nothing has vouched for the peer yet — and the
    // allocation is whatever the four bytes claim, up to 4 GiB.
    if len > limit {
        return Err(oversized_frame(u64::from(len), limit));
    }
    let mut buffer = vec![0u8; len as usize];
    reader.read_exact(&mut buffer)?;
    bincode::deserialize(&buffer)
        .map(Some)
        .map_err(|error| RuntimeError::Protocol(error.to_string()))
}

fn oversized_frame(len: u64, limit: u32) -> RuntimeError {
    RuntimeError::Protocol(format!(
        "frame of {len} bytes exceeds the {limit}-byte limit"
    ))
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
        Compatibility, Endpoint, MAX_FRAME, MAX_PANE_DIMENSION, MAX_SOCKET_PATH, PaneRuntime,
        PersistedSpace, PersistedTab, PersistedWorkspace, Probe, ReplySink, RuntimeError,
        Selection, Server, WorkspaceLock, corroborated_as_server, heal_pid_file, identity_of,
        persisted_state_path, platform_reads_processes, probe_server, read_event, read_message,
        recorded_compatibility, relaunch_command_for_process, replace_incompatible_server,
        runtime_process_is_alive, send_request, server_protocol_version, snapshot, view_for,
        workspace_lock_path, write_atomically, write_message, write_pid_file,
    };
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Mutex};

    // Several tests below carry
    // `#[cfg(any(target_os = "linux", target_os = "macos"))]`. That is not a
    // list of platforms anybody chose; it is the set `process_probe` can
    // answer on, and these are the tests that start a real server, read a
    // real pane's foreground status, or relaunch a persisted one — all of
    // which need the kernel to say where a process is standing and what it
    // is running. On a platform where the probe returns `None` they would
    // assert against an answer nothing can give. Widen the gate by teaching
    // `process_probe` a new platform, never by widening it here.

    use crate::Palette;

    /// A sink over the default palette, for the tests that only need a
    /// terminal to parse into.
    fn reply_sink(sender: std::sync::mpsc::Sender<Vec<u8>>) -> ReplySink {
        ReplySink::new(sender, Arc::new(Mutex::new(Palette::default())))
    }
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
    /// goes and asks the socket rather than trusting a file that is silent.
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

    /// The three readings `attach` acts on. The load-bearing one is the
    /// last: an absent or silent pid file must never read as a mismatch,
    /// because the only recovery for a mismatch kills the process the file
    /// names and unlinks the socket it was serving on.
    #[test]
    fn a_pid_file_speaks_for_its_server_only_where_it_recorded_a_version() {
        let scratch = uze_testkit::temp::scratch("terminal-recorded-compatibility");
        std::fs::create_dir_all(&scratch).unwrap();
        let pid_path = scratch.join("test.pid");

        assert!(matches!(
            recorded_compatibility(&pid_path),
            Compatibility::Unrecorded
        ));

        std::fs::write(&pid_path, "4242").unwrap();
        assert!(matches!(
            recorded_compatibility(&pid_path),
            Compatibility::Unrecorded
        ));

        std::fs::write(&pid_path, format!("4242\n{}", super::PROTOCOL_VERSION + 1)).unwrap();
        assert!(matches!(
            recorded_compatibility(&pid_path),
            Compatibility::Mismatched
        ));

        std::fs::write(&pid_path, format!("4242\n{}", super::PROTOCOL_VERSION)).unwrap();
        assert!(matches!(
            recorded_compatibility(&pid_path),
            Compatibility::Known
        ));

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The rescue itself: a server of this build whose pid file vanished —
    /// the shape a `/tmp` cleaner leaves behind — is recognized through the
    /// socket and given its file back, so the next attach reads the answer
    /// instead of asking again. Without this the endpoint's live owner is
    /// killed and whatever was running in its panes goes with it.
    /// `XDG_RUNTIME_DIR` is somebody else's variable and can be arbitrarily
    /// deep. A socket path that does not fit `sun_path` fails at `bind` with
    /// an error naming the limit and not the directory — which reached a
    /// user as `could not acquire package: terminal runtime I/O error: path
    /// must be shorter than SUN_LEN`, from a command that has nothing to do
    /// with sockets.
    #[test]
    fn a_runtime_directory_too_long_for_a_socket_is_stepped_over() {
        let deep = uze_testkit::temp::socket_scratch("deep").join("a".repeat(120));
        std::fs::create_dir_all(&deep).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("XDG_RUNTIME_DIR", &deep);

        let endpoint = Endpoint::global().expect("a too-long runtime directory is not fatal");
        assert!(
            endpoint.socket.as_os_str().len() <= MAX_SOCKET_PATH,
            "the chosen socket path must fit sun_path, got {} bytes: {}",
            endpoint.socket.as_os_str().len(),
            endpoint.socket.display()
        );
        assert!(
            !endpoint.socket.starts_with(&deep),
            "the directory that could not hold the socket must not have been chosen"
        );
        // Binding is the only real proof: the length rule exists to make this
        // call succeed, so the test performs it rather than trusting the
        // arithmetic.
        let _ = std::fs::remove_file(&endpoint.socket);
        let listener = std::os::unix::net::UnixListener::bind(&endpoint.socket)
            .expect("the chosen path must actually bind");
        drop(listener);
        let _ = std::fs::remove_file(&endpoint.socket);
        let _ = std::fs::remove_dir_all(&deep);
    }

    /// "Nothing is running" is the ordinary state of `uze terminal stop`,
    /// and it used to exit non-zero: a machine that has not opened the TUI
    /// since boot has no socket, and a `/tmp` cleaner taking the socket out
    /// from under a live server leaves one nobody answers. Both reached the
    /// operator as `could not acquire package: terminal runtime I/O error`,
    /// from a command that stops a terminal.
    #[test]
    fn stopping_a_runtime_that_is_not_running_is_not_a_failure() {
        let scratch = uze_testkit::temp::socket_scratch("stop-idempotent");
        let mut env = uze_testkit::env::scope();
        env.set("XDG_RUNTIME_DIR", &scratch);

        let endpoint = Endpoint::global().expect("an endpoint can always be named");
        let _ = std::fs::remove_file(&endpoint.socket);
        assert!(
            super::stop(&scratch).is_ok(),
            "no socket at all is nothing to stop, not a failure"
        );

        // The shape a cleaner leaves: the file is there, the server is not.
        let listener = std::os::unix::net::UnixListener::bind(&endpoint.socket)
            .expect("the endpoint path binds");
        drop(listener);
        assert!(
            endpoint.socket.exists(),
            "dropping the listener leaves the socket file behind, which is the case under test"
        );
        assert!(
            super::stop(&scratch).is_ok(),
            "a socket nobody answers is nothing to stop either"
        );

        let _ = std::fs::remove_file(&endpoint.socket);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_server_of_this_build_is_adopted_when_its_pid_file_vanishes() {
        let scratch = uze_testkit::temp::socket_scratch("adopt");
        std::fs::create_dir_all(&scratch).unwrap();
        let endpoint = Endpoint {
            socket: scratch.join("test.sock"),
            pid: scratch.join("test.pid"),
        };
        // This process holds the socket, and is by construction running the
        // executable the probe compares against — the same relationship a
        // real server has to a client built from the same binary.
        let listener = std::os::unix::net::UnixListener::bind(&endpoint.socket).unwrap();

        let Probe::Speaks { pid } = probe_server(&endpoint) else {
            panic!("a listener running this very executable must answer the probe");
        };
        assert_eq!(pid, std::process::id());

        heal_pid_file(&endpoint, pid);
        assert!(
            matches!(recorded_compatibility(&endpoint.pid), Compatibility::Known),
            "the healed pid file must answer the next attach on its own"
        );

        drop(listener);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// An endpoint file nobody answers on is not a server to adopt: the
    /// probe has to say so, or a leftover socket would be healed into a pid
    /// file naming a process that never existed.
    #[test]
    fn a_socket_no_one_listens_on_is_foreign() {
        let scratch = uze_testkit::temp::scratch("terminal-probe-foreign");
        std::fs::create_dir_all(&scratch).unwrap();
        let endpoint = Endpoint {
            socket: scratch.join("test.sock"),
            pid: scratch.join("test.pid"),
        };
        std::fs::write(&endpoint.socket, b"placeholder").unwrap();

        assert!(
            matches!(probe_server(&endpoint), Probe::Foreign { peer: None }),
            "with nobody listening there is no peer to name, and so nobody to signal"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The actual recovery `attach` relies on when it finds a live server
    /// it can no longer talk to: it must terminate that process and clear
    /// both endpoint files, so the caller's next connect lands on a fresh
    /// server instead of the one it just gave up on.
    ///
    /// The victim is a copy of `sleep` named `uze`, and the probe's peer
    /// pid names it too: those are the two independent witnesses
    /// `replace_incompatible_server` now demands before signalling anything
    /// — see the sibling tests for what a pid file naming an ordinary
    /// process does, and for what happens when the two disagree.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn replace_incompatible_server_kills_the_old_owner_and_clears_its_files() {
        let scratch = uze_testkit::temp::scratch("terminal-replace-incompatible");
        std::fs::create_dir_all(&scratch).unwrap();
        let socket = scratch.join("test.sock");
        let pid_path = scratch.join("test.pid");
        std::fs::write(&socket, b"placeholder").unwrap();

        let server_binary = scratch.join("uze");
        std::fs::copy("/bin/sleep", &server_binary).unwrap();
        let mut child = std::process::Command::new(&server_binary)
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        // No version line: the exact shape `replace_incompatible_server` is
        // meant to react to.
        std::fs::write(&pid_path, pid.to_string()).unwrap();
        // The replace signals only a pid the process table corroborates —
        // and `spawn` returns before the child's image has necessarily been
        // swapped for the copied binary: on a GitHub runner the probe read
        // this test's own executable for the child's pid a moment after
        // the spawn. A server being replaced has been running for ages, so
        // production never sees that window; the test waits it out.
        let exec_deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !corroborated_as_server(pid as libc::pid_t) {
            assert!(
                std::time::Instant::now() < exec_deadline,
                "the process table must corroborate the copied `uze` (pid {pid}): \
                 executable_of = {:?}, platform reads processes = {}",
                crate::process_probe::executable_of(pid),
                platform_reads_processes()
            );
            thread::sleep(Duration::from_millis(10));
        }

        let endpoint = Endpoint {
            socket: socket.clone(),
            pid: pid_path.clone(),
        };
        // The peer the probe would have learned from `SO_PEERCRED`, which
        // here agrees with the file.
        replace_incompatible_server(&endpoint, Some(pid)).unwrap();

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
        let mut terminal = Term::new(Config::default(), &TermSize::new(12, 3), reply_sink(sender));
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
        let mut terminal = Term::new(Config::default(), &TermSize::new(12, 3), reply_sink(sender));
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
        let mut terminal = Term::new(Config::default(), &TermSize::new(12, 3), reply_sink(sender));
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
        // A palette no default would ever produce, so the reply can only be
        // coming from what the client set.
        let palette = Arc::new(Mutex::new(Palette {
            foreground: (0x11, 0x22, 0x33),
            background: (0x44, 0x55, 0x66),
            ..Palette::default()
        }));
        let mut terminal = Term::new(
            Config::default(),
            &TermSize::new(12, 3),
            ReplySink::new(sender, Arc::clone(&palette)),
        );
        let mut parser: Processor = Processor::new();

        parser.advance(&mut terminal, b"\x1b]10;?\x1b\\");
        assert_eq!(
            receiver.try_recv().expect("OSC 10 reply"),
            b"\x1b]10;rgb:1111/2222/3333\x1b\\".to_vec()
        );

        parser.advance(&mut terminal, b"\x1b]11;?\x1b\\");
        assert_eq!(
            receiver.try_recv().expect("OSC 11 reply"),
            b"\x1b]11;rgb:4444/5555/6666\x1b\\".to_vec()
        );

        // A theme changed after the pane started reaches it too: the palette
        // is shared, not copied into the sink.
        palette.lock().expect("palette").background = (0xaa, 0xbb, 0xcc);
        parser.advance(&mut terminal, b"\x1b]11;?\x1b\\");
        assert_eq!(
            receiver
                .try_recv()
                .expect("OSC 11 reply after a theme change"),
            b"\x1b]11;rgb:aaaa/bbbb/cccc\x1b\\".to_vec()
        );
    }

    #[test]
    fn resize_changes_snapshot_dimensions() {
        let (sender, _receiver) = std::sync::mpsc::channel();
        let mut terminal = Term::new(Config::default(), &TermSize::new(8, 2), reply_sink(sender));
        terminal.resize(TermSize::new(20, 4));
        let rendered = snapshot(PaneId(1), &terminal);
        assert_eq!((rendered.columns, rendered.rows), (20, 4));
    }

    #[test]
    fn snapshot_renders_the_scrollback_viewport() {
        let (sender, _receiver) = std::sync::mpsc::channel();
        let mut terminal = Term::new(Config::default(), &TermSize::new(8, 2), reply_sink(sender));
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
        let pane = PaneRuntime::spawn(
            PaneId(9),
            PathBuf::from("/tmp"),
            80,
            24,
            damage,
            None,
            Arc::new(Mutex::new(Palette::default())),
        )
        .unwrap();
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
        let pane = PaneRuntime::spawn(
            PaneId(7),
            PathBuf::from("/tmp"),
            80,
            24,
            damage,
            None,
            Arc::new(Mutex::new(Palette::default())),
        )
        .unwrap();
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

    #[cfg(any(target_os = "linux", target_os = "macos"))]
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
        // Canonicalized, because the assertion below compares this against
        // what the kernel reports, and the kernel answers with the real
        // path: `/tmp` is a symlink to `/private/tmp` on macOS, so spawning
        // in `/tmp` and expecting `/tmp` back never matches there.
        let pane_cwd = PathBuf::from("/tmp")
            .canonicalize()
            .expect("the system temp directory must resolve");
        let pane = PaneRuntime::spawn(
            PaneId(11),
            pane_cwd.clone(),
            80,
            24,
            damage,
            None,
            Arc::new(Mutex::new(Palette::default())),
        )
        .unwrap();
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
        // Five seconds, not five hundred milliseconds: what is being waited
        // on is another process being scheduled and reaching `exec`, and the
        // assertion below is about *what* it reports, never about how fast.
        // Under the full workspace suite on a small machine the old budget
        // ran out before the shell was up, turning a loaded runner into a
        // red build — which is why `make coverage` already skips this test
        // by name instead of trusting it.
        //
        // Waited on by *identity*, not by directory. The pane's child already
        // stands in `pane_cwd` between `fork` and `exec` — that is when the
        // cwd is set — while still carrying the name it forked from. A loop
        // that stopped at the first matching directory therefore accepted a
        // process mid-spawn and read this test binary's own name back out of
        // it, which is exactly what a macOS runner caught. Waiting for the
        // shell to have `exec`ed also promotes the directory from a filter to
        // an assertion, which is what it should have been.
        let mut status = None;
        let mut last_seen = None;
        for _ in 0..500 {
            let reading = pane.foreground_status();
            if let Some((_, process)) = &reading
                && *process == expected_name
            {
                status = reading;
                break;
            }
            last_seen = reading.or(last_seen);
            thread::sleep(Duration::from_millis(10));
        }
        pane.stop();

        let (cwd, process) = status.unwrap_or_else(|| {
            panic!(
                "the spawned shell must own the PTY foreground group; \
                 waited for {expected_name:?} and last saw {last_seen:?}"
            )
        });
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
    #[cfg(any(target_os = "linux", target_os = "macos"))]
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
                // `$$` is the shell's own pid, and `exec` keeps it — the
                // same relationship `src/shim.rs` has to the harness it
                // replaces itself with, which is what makes the stamp
                // belong to the process that carries it.
                format!(
                    "export UZE_SHIM_NAME=claude UZE_SHIM_PID=$$; exec {} 5",
                    versioned_binary.display()
                ),
            ]),
            Arc::new(Mutex::new(Palette::default())),
        )
        .unwrap();

        // Five seconds, and only a matching reading is kept — the same two
        // properties as the sibling test above, and for the same two
        // reasons: what is being waited on is another process reaching
        // `exec`, and a reading taken before it does names the process this
        // one forked from.
        let mut status = None;
        let mut last_seen = None;
        for _ in 0..500 {
            let reading = pane.foreground_status();
            if let Some((_, process)) = &reading
                && process == "claude"
            {
                status = reading;
                break;
            }
            last_seen = reading.or(last_seen);
            thread::sleep(Duration::from_millis(10));
        }
        pane.stop();
        let _ = std::fs::remove_dir_all(&bin_dir);

        let (_, process) = status.unwrap_or_else(|| {
            panic!("the shim identity must reach the foreground; last saw {last_seen:?}")
        });
        assert_eq!(process, "claude");
    }

    /// A client that attaches without naming a root takes the session as
    /// it stands. Nothing is created for it, and nothing the operator
    /// closed comes back.
    ///
    /// This is the server half of what makes closing a space stick: the
    /// workspace client detaches and attaches again on every Ctrl+O round
    /// trip to management, and an attach that named the launch directory
    /// every time reopened the space closed just before it.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn attaching_without_a_root_neither_creates_nor_reopens_a_space() {
        let scratch = uze_testkit::temp::socket_scratch("rootless");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        let other = scratch.join("other");
        let runtime_dir = scratch.join("runtime");
        for directory in [&uze_home, &project, &other, &runtime_dir] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home)
            .set("XDG_RUNTIME_DIR", &runtime_dir);

        let endpoint = Endpoint::global().unwrap();
        let (server, _damage) = Server::new(project.clone(), endpoint).unwrap();
        let server = Arc::new(server);
        // A second space, because the last one standing cannot be closed.
        let pane = server.session.lock().expect("session poisoned").add_space(
            "other".into(),
            other.clone(),
            80,
            24,
        );
        server.spawn_pane(pane, None).unwrap();
        let launch = {
            let mut session = server.session.lock().expect("session poisoned");
            let launch = session
                .space_for_root(&project)
                .expect("the bootstrap space is rooted at the launch directory");
            assert!(session.remove_space(launch).is_some(), "space closed");
            launch
        };

        let (client, driver) = std::os::unix::net::UnixStream::pair().unwrap();
        let serving = {
            let server = Arc::clone(&server);
            std::thread::spawn(move || server.handle_client(client))
        };
        let mut writer = driver.try_clone().unwrap();
        let mut reader = std::io::BufReader::new(driver);
        send_request(
            &mut writer,
            &crate::ClientRequest::Attach {
                version: crate::PROTOCOL_VERSION,
                workspace: WorkspaceId("rootless".into()),
                columns: 80,
                rows: 24,
                root: None,
            },
        )
        .unwrap();
        let attached = loop {
            match read_event(&mut reader).unwrap() {
                Some(crate::ClientEvent::Attached { session }) => break session,
                Some(_) => {}
                None => panic!("the server hung up before attaching"),
            }
        };
        assert_eq!(
            attached.space_for_root(&project),
            None,
            "a rootless attach left the closed space closed"
        );
        assert_eq!(attached.workspace.spaces.len(), 1);
        assert_ne!(attached.workspace.selected_space, launch);

        let _ = send_request(&mut writer, &crate::ClientRequest::Detach);
        drop(writer);
        drop(reader);
        let _ = serving.join();
        server.stop_panes();

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The whole point of persistence: a server that starts with nothing
    /// running (simulating a reboot, a crash, `kill -9` — anything that
    /// left no chance for a clean stop) still comes back with the same
    /// spaces and tabs a previous instance for this same `root` had, each
    /// tab's pane relaunched with whatever it was last spawned with —
    /// `None` for a plain shell, the recorded `argv` for an agent.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_restarted_server_relaunches_the_same_spaces_tabs_and_agent_commands() {
        let scratch = uze_testkit::temp::socket_scratch("persist");
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
        // Restarting means the first server is *gone*: it holds the
        // workspace lock while it exists, and a second one restoring the
        // same spaces behind its back is the duplicate-agent failure that
        // lock is there to refuse.
        drop(first);

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

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_finished_direct_agent_is_replaced_by_a_shell_in_its_pane() {
        let scratch = uze_testkit::temp::socket_scratch("agentexit");
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
            // `/bin/sh -c 'exit 0'`, not `/bin/true`: macOS keeps `true` in
            // `/usr/bin` and has no `/bin/true` at all. `/bin/sh` is the one
            // path POSIX actually promises, and what this needs is any
            // process that exits at once.
            .spawn_pane(
                pane,
                Some(&["/bin/sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()]),
            )
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
        // Whatever this reads is persisted and then spawned by the server
        // on the next restart, and the name it reads is one a process can
        // choose for itself (`UZE_SHIM_NAME` is an ordinary variable) — so
        // a candidate naming a file rather than a command is refused.
        assert_eq!(relaunch_command_for_process("/tmp/payload"), None);
        assert_eq!(relaunch_command_for_process("./payload"), None);
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
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_plain_shell_tab_running_a_recognized_process_relaunches_as_that_process() {
        let scratch = uze_testkit::temp::socket_scratch("typed");
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
        drop(first);

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
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn the_snapshot_names_a_tabs_agent_by_position() {
        let scratch = uze_testkit::temp::socket_scratch("persagent");
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

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_persisted_command_that_no_longer_resolves_falls_back_to_a_plain_shell() {
        let scratch = uze_testkit::temp::socket_scratch("perstale");
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

    /// The four bytes a peer sends first become an allocation before
    /// anything inside the frame — the protocol version included — can be
    /// read, so the prefix is the one number that has to be distrusted on
    /// its own. `0xffffffff` asks for 4 GiB.
    #[test]
    fn a_length_prefix_past_the_frame_limit_is_refused_before_it_is_allocated() {
        for refused in [u32::MAX, MAX_FRAME + 1] {
            let mut wire: &[u8] = &refused.to_le_bytes();
            assert!(
                matches!(
                    read_message::<_, crate::ClientRequest>(&mut wire),
                    Err(RuntimeError::Protocol(_))
                ),
                "a {refused}-byte frame must be refused, not allocated"
            );
        }
        // And the limit itself is a size the wire accepts, not one it
        // refuses: a cap that fired one byte early would disconnect a
        // client at the moment it resized. Truncated after the prefix, so
        // what this proves is that the read got past the bound and went
        // looking for the bytes.
        let mut wire: &[u8] = &MAX_FRAME.to_le_bytes();
        assert!(
            matches!(
                read_message::<_, crate::ClientRequest>(&mut wire),
                Err(RuntimeError::Io(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof
            ),
            "a frame of exactly the limit is one the wire allows"
        );
    }

    /// The same bound on the way out, so the two sides cannot disagree
    /// about what is sendable — and nothing half-written reaches the wire.
    #[test]
    fn a_frame_past_the_limit_is_never_written_either() {
        let framed = |payload: usize| crate::ClientRequest::Input {
            pane: PaneId(1),
            bytes: vec![0u8; payload],
        };
        let overhead = bincode::serialized_size(&framed(0)).unwrap() as usize;

        let mut wire = Vec::new();
        assert!(matches!(
            write_message(&mut wire, &framed(MAX_FRAME as usize + 1 - overhead)),
            Err(RuntimeError::Protocol(_))
        ));
        assert!(
            wire.is_empty(),
            "nothing may reach the wire that the other side would refuse"
        );

        // Exactly the limit is sendable, and the reader accepts it: the two
        // sides agree on the boundary itself, not merely on numbers well
        // past it.
        write_message(&mut wire, &framed(MAX_FRAME as usize - overhead)).unwrap();
        assert_eq!(wire.len(), MAX_FRAME as usize + 4, "prefix plus the frame");
        let mut sent: &[u8] = &wire;
        assert!(
            read_message::<_, crate::ClientRequest>(&mut sent)
                .unwrap()
                .is_some(),
            "a frame of exactly the limit round-trips"
        );
    }

    /// [`MAX_FRAME`] and [`MAX_PANE_DIMENSION`] are one decision in two
    /// constants: a repaint of the largest pane a client may ask for has to
    /// fit, or the cap would disconnect a client at the moment it resized.
    /// Measured from one worst-case cell rather than by building the grid —
    /// every field in it is fixed-width, so the arithmetic is exact.
    #[test]
    fn a_full_repaint_of_the_largest_pane_fits_in_one_frame() {
        let widest_cell = (
            u16::MAX,
            u16::MAX,
            crate::RenderCell {
                character: '\u{10ffff}',
                foreground: TerminalColor::Rgb {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
                background: TerminalColor::Rgb {
                    red: 4,
                    green: 5,
                    blue: 6,
                },
                attributes: crate::CellAttributes {
                    bold: true,
                    dim: true,
                    italic: true,
                    underline: true,
                    inverse: true,
                    hidden: true,
                    strikeout: true,
                },
            },
        );
        let per_cell = bincode::serialized_size(&widest_cell).expect("a cell has a size");
        let cells = u64::from(MAX_PANE_DIMENSION) * u64::from(MAX_PANE_DIMENSION);
        assert!(
            per_cell * cells < u64::from(MAX_FRAME),
            "a {MAX_PANE_DIMENSION}x{MAX_PANE_DIMENSION} repaint is {} bytes, past the \
             {MAX_FRAME}-byte frame limit",
            per_cell * cells
        );
    }

    /// `columns`/`rows` arrive from a peer and go into `Term::resize`,
    /// which allocates a cell per position and clamps nothing: 65535×65535
    /// is ~137 GB, and a failed allocation aborts the process that owns
    /// every live agent pane. One malformed frame must not be able to do
    /// that, from a buggy client as easily as a hostile one.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_resize_to_the_largest_number_on_the_wire_leaves_the_server_answering() {
        let scratch = uze_testkit::temp::socket_scratch("resizemax");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        let runtime_dir = scratch.join("runtime");
        for directory in [&uze_home, &project, &runtime_dir] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home)
            .set("XDG_RUNTIME_DIR", &runtime_dir);

        let endpoint = Endpoint::global().unwrap();
        let (server, _damage) = Server::new(project.clone(), endpoint).unwrap();
        let server = Arc::new(server);
        let pane = server
            .session
            .lock()
            .expect("session poisoned")
            .selected_tab()
            .focus
            .pane;

        let (client, driver) = std::os::unix::net::UnixStream::pair().unwrap();
        let serving = {
            let server = Arc::clone(&server);
            std::thread::spawn(move || server.handle_client(client))
        };
        let mut writer = driver.try_clone().unwrap();
        // So a server that stops answering fails this test instead of
        // hanging it.
        driver
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let mut reader = std::io::BufReader::new(driver);
        send_request(
            &mut writer,
            &crate::ClientRequest::Attach {
                version: crate::PROTOCOL_VERSION,
                workspace: WorkspaceId("resizemax".into()),
                columns: 0,
                rows: 0,
                root: None,
            },
        )
        .unwrap();
        send_request(
            &mut writer,
            &crate::ClientRequest::Resize {
                pane,
                columns: u16::MAX,
                rows: u16::MAX,
            },
        )
        .unwrap();

        // Attaching repaints every pane first (see
        // [`Server::broadcast_snapshot`]), so the event this test is about
        // is the one that reports a size the pane did not start at.
        let resized = loop {
            match read_event(&mut reader).expect("the server must still be speaking") {
                Some(crate::ClientEvent::Damage(damage))
                    if damage.pane == pane && (damage.columns, damage.rows) != (80, 24) =>
                {
                    break damage;
                }
                Some(_) => {}
                None => panic!("the server hung up rather than bounding the resize"),
            }
        };
        assert_eq!(
            (resized.columns, resized.rows),
            (MAX_PANE_DIMENSION, MAX_PANE_DIMENSION),
            "the resize is bounded and still honoured, not refused"
        );

        let _ = send_request(&mut writer, &crate::ClientRequest::Detach);
        drop(writer);
        drop(reader);
        let _ = serving.join();
        server.stop_panes();

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// `CommandBuilder` seeds a pane from the *server's* environment, and
    /// the server is started by whatever `uze` first needed one — in this
    /// project, routinely a `uze` run from inside a shimmed agent. A plain
    /// shell that inherited that stamp reports as the agent, persists as
    /// one, and is relaunched as one on the next restart.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_pane_does_not_inherit_the_servers_shim_identity() {
        let mut env = uze_testkit::env::scope();
        env.set("UZE_SHIM_NAME", "claude")
            .set("UZE_SHIM_PID", std::process::id().to_string());

        let (damage, _damage_events) = std::sync::mpsc::channel();
        let pane = PaneRuntime::spawn(
            PaneId(21),
            PathBuf::from("/tmp").canonicalize().unwrap(),
            80,
            24,
            damage,
            None,
            Arc::new(Mutex::new(Palette::default())),
        )
        .unwrap();
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let expected_name = Path::new(&shell)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("sh")
            .to_owned();

        // Waited on by identity, for the reason the sibling tests spell
        // out: a reading taken before the shell has `exec`ed names the
        // process it forked from, which here is this test binary.
        let mut reported = None;
        for _ in 0..500 {
            let reading = pane.foreground_status();
            if let Some((_, process)) = &reading
                && *process == expected_name
            {
                reported = reading;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let leader = pane
            .master
            .lock()
            .expect("master poisoned")
            .process_group_leader();
        pane.stop();

        assert!(
            reported.is_some(),
            "the pane must report the shell it actually spawned, not the identity of the \
             session that happened to start the server"
        );
        let leader = leader.expect("the spawned shell owns the PTY foreground group");
        assert_eq!(
            crate::process_probe::environment_value_of(leader, "UZE_SHIM_NAME"),
            None,
            "a pane's environment may only carry what that pane's own launch put there"
        );
    }

    /// The other half of the identity rule: an *inherited* stamp names an
    /// ancestor, not the process it is read from, so it must be ignored.
    /// Every child of a shimmed agent carries `UZE_SHIM_NAME`.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn foreground_status_ignores_a_shim_identity_stamped_for_another_process() {
        let bin_dir = uze_testkit::temp::scratch("shim-inherited-test");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let versioned_binary = bin_dir.join("2.1.251");
        std::fs::copy("/bin/sleep", &versioned_binary).unwrap();

        let (damage, _damage_events) = std::sync::mpsc::channel();
        let pane = PaneRuntime::spawn(
            PaneId(23),
            PathBuf::from("/tmp"),
            80,
            24,
            damage,
            // `UZE_SHIM_PID=1` is the shape of an inherited pair: a name
            // stamped for a process that is not this one.
            Some(&[
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                format!(
                    "export UZE_SHIM_NAME=claude UZE_SHIM_PID=1; exec {} 5",
                    versioned_binary.display()
                ),
            ]),
            Arc::new(Mutex::new(Palette::default())),
        )
        .unwrap();

        let mut reported = None;
        let mut last_seen = None;
        for _ in 0..500 {
            let reading = pane.foreground_status();
            if let Some((_, process)) = &reading
                && process == "2.1.251"
            {
                reported = reading;
                break;
            }
            assert!(
                !matches!(&reading, Some((_, process)) if process == "claude"),
                "a stamp made for another process must never be read as this one's identity"
            );
            last_seen = reading.or(last_seen);
            thread::sleep(Duration::from_millis(10));
        }
        pane.stop();
        let _ = std::fs::remove_dir_all(&bin_dir);

        assert!(
            reported.is_some(),
            "the kernel's own name for the process is what is left; last saw {last_seen:?}"
        );
    }

    /// The pid in an endpoint file is a claim, not a fact: the file
    /// survives a reboot under `/tmp`, and pids are recycled. Acting on it
    /// unchecked let an ordinary upgrade kill an unrelated process of the
    /// person's own — an editor, a build, another agent.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn replace_incompatible_server_leaves_a_process_that_is_not_a_server_running() {
        let scratch = uze_testkit::temp::scratch("terminal-replace-unrelated");
        std::fs::create_dir_all(&scratch).unwrap();
        let endpoint = Endpoint {
            socket: scratch.join("test.sock"),
            pid: scratch.join("test.pid"),
        };
        std::fs::write(&endpoint.socket, b"placeholder").unwrap();

        let mut bystander = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        std::fs::write(&endpoint.pid, bystander.id().to_string()).unwrap();

        // Both witnesses name it, and it is still not a server: `sleep` is
        // not `uze`, which is the reading the process table settles.
        replace_incompatible_server(&endpoint, Some(bystander.id())).unwrap();

        assert!(
            bystander.try_wait().unwrap().is_none(),
            "a pid file naming somebody else's process must not get it killed"
        );
        assert!(!endpoint.socket.exists(), "the endpoint is still cleared");
        assert!(!endpoint.pid.exists());

        let _ = bystander.kill();
        let _ = bystander.wait();
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A server the client started and never reaped is a zombie: still
    /// addressable by `kill(pid, 0)`, which left `recover_stale_endpoint`
    /// refusing to clear an endpoint nothing was serving — for the whole
    /// remaining life of that client, so the person could not reattach
    /// without quitting `uze` itself.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_crashed_server_nobody_reaped_does_not_count_as_alive() {
        let scratch = uze_testkit::temp::scratch("terminal-zombie");
        std::fs::create_dir_all(&scratch).unwrap();
        let pid_path = scratch.join("test.pid");

        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        write_pid_file(&pid_path, child.id()).unwrap();
        assert!(
            runtime_process_is_alive(&pid_path).unwrap(),
            "a running server is alive"
        );

        // Killed and deliberately not waited on — exactly the relationship
        // a client has to the server it spawned.
        let _ = child.kill();
        let mut zombie = false;
        for _ in 0..80 {
            if !runtime_process_is_alive(&pid_path).unwrap() {
                zombie = true;
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&scratch);

        assert!(
            zombie,
            "an unreaped dead server must not hold its endpoint hostage"
        );
    }

    /// The endpoint directory decides where a socket carrying every pane's
    /// contents lives. `create_dir_all` answers `Ok(())` for a path that is
    /// already there — a symlink to somewhere else included — and
    /// `set_permissions` follows symlinks, so "it exists" is not evidence
    /// of anything where the name is one any local user can predict.
    #[test]
    fn a_runtime_directory_that_is_not_ours_to_own_is_stepped_over() {
        let scratch = uze_testkit::temp::socket_scratch("dirowner");
        let xdg = scratch.join("xdg");
        let elsewhere = scratch.join("elsewhere");
        std::fs::create_dir_all(&xdg).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let owner = unsafe { libc::getuid() };
        let candidate = xdg.join(format!("uze-runtime-{owner}"));
        std::os::unix::fs::symlink(&elsewhere, &candidate).unwrap();

        let mut env = uze_testkit::env::scope();
        env.set("XDG_RUNTIME_DIR", &xdg);
        let endpoint = Endpoint::global().expect("a bad candidate is stepped over, not fatal");
        assert!(
            !endpoint.socket.starts_with(&xdg),
            "a symlinked candidate must not be adopted, got {}",
            endpoint.socket.display()
        );

        // A directory this user genuinely owns is theirs to correct rather
        // than to refuse — a permissive umask on first run is the ordinary
        // way one is created too open.
        std::fs::remove_file(&candidate).unwrap();
        std::fs::create_dir_all(&candidate).unwrap();
        std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o777)).unwrap();
        let endpoint = Endpoint::global().expect("our own directory is usable");
        assert!(endpoint.socket.starts_with(&candidate));
        let mode = std::fs::metadata(&candidate).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "the mode is corrected, not inherited");

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The recorded WSL case: `/tmp` wiped under a live server takes the
    /// socket and the pid file with it, the next attach reads "no server",
    /// and a second one restores the same `workspace.json` — every agent
    /// twice in the same checkout, both servers persisting over each other.
    /// The claim lives beside the workspace, under `$UZE_HOME`, so a
    /// cleaner that can reach it has taken the workspace too.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_second_server_refuses_to_restore_a_workspace_another_one_holds() {
        let scratch = uze_testkit::temp::socket_scratch("wslock");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        std::fs::create_dir_all(&uze_home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);

        let endpoint = Endpoint::global().unwrap();
        let (first, _damage) = Server::new(project.clone(), endpoint.clone()).unwrap();
        assert!(
            workspace_lock_path().starts_with(&uze_home),
            "the claim must live beside the workspace, never in a wipeable temp directory"
        );

        let second = Server::new(project.clone(), endpoint.clone());
        assert!(
            matches!(second, Err(RuntimeError::Protocol(_))),
            "a workspace a live server holds must not be restored a second time"
        );

        first.stop_panes();
        drop(first);
        let (third, _damage3) =
            Server::new(project, endpoint).expect("the claim is released with its holder");
        third.stop_panes();

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Held by the kernel, so a crash releases it: nothing to clean up, and
    /// a stale claim is impossible by construction.
    #[test]
    fn a_workspace_claim_is_exclusive_and_released_with_its_holder() {
        let scratch = uze_testkit::temp::scratch("terminal-workspace-lock");
        std::fs::create_dir_all(&scratch).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &scratch);

        let held = WorkspaceLock::acquire().expect("the first claim is granted");
        match WorkspaceLock::acquire() {
            Err(RuntimeError::Protocol(refusal)) => assert!(
                refusal.contains("already serving this workspace"),
                "contention has to name the server that holds it, not an errno: {refusal}"
            ),
            Err(other) => panic!("a held claim must read as contention, not as {other}"),
            Ok(_) => panic!("a held claim must not be granted twice"),
        }
        drop(held);
        WorkspaceLock::acquire().expect("released with its holder");

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The whole workspace is rewritten on every structural change, and a
    /// plain write truncates before it fills. A reader must see the old
    /// file or the new one, never half of either.
    #[test]
    fn the_persisted_workspace_is_replaced_in_one_step() {
        let scratch = uze_testkit::temp::scratch("terminal-atomic-write");
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("workspace.json");
        std::fs::write(&path, b"{\"spaces\":[]}").unwrap();

        write_atomically(&path, b"{\"spaces\":[{}]}").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"spaces\":[{}]}");
        assert!(
            !scratch.join("workspace.json.tmp").exists(),
            "the temporary is renamed over the target, not left beside it"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// [`MAX_FRAME`] bounds a repaint of *one* pane at
    /// [`MAX_PANE_DIMENSION`]. A snapshot carrying every pane in a single
    /// frame is therefore bounded by nothing a client cannot exceed: three
    /// panes at a size `within_pane_bounds` permits — and that a restart
    /// restores — made the frame unsendable, and every attached client sat
    /// frozen on chrome that still looked live.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn every_pane_reaches_a_client_when_one_frame_could_not_have_carried_them_all() {
        let scratch = uze_testkit::temp::socket_scratch("bigsnap");
        let uze_home = scratch.join("home");
        let runtime_dir = scratch.join("runtime");
        let roots: Vec<PathBuf> = (0..3)
            .map(|index| scratch.join(format!("p{index}")))
            .collect();
        for directory in [&uze_home, &runtime_dir].into_iter().chain(roots.iter()) {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home)
            .set("XDG_RUNTIME_DIR", &runtime_dir);

        let endpoint = Endpoint::global().unwrap();
        let (server, _damage) = Server::new(roots[0].clone(), endpoint).unwrap();
        let server = Arc::new(server);
        for root in &roots[1..] {
            server.ensure_space(root).expect("a space per root");
        }

        // Filled through the pane's own parser, so what the client is sent
        // is a real grid and not a hand-built one. The character is
        // four bytes of UTF-8 — the widest a cell can carry, and what makes
        // three of these panes exceed one frame rather than merely approach
        // it.
        let widest = '\u{1d54f}';
        let mut painted = Vec::new();
        for row in 0..MAX_PANE_DIMENSION {
            if row > 0 {
                painted.extend_from_slice(b"\r\n");
            }
            for _ in 0..MAX_PANE_DIMENSION {
                let mut encoded = [0u8; 4];
                painted.extend_from_slice(widest.encode_utf8(&mut encoded).as_bytes());
            }
        }
        let pane_ids: Vec<PaneId> = server
            .panes
            .lock()
            .expect("panes poisoned")
            .keys()
            .copied()
            .collect();
        assert_eq!(pane_ids.len(), 3, "one pane per space");
        for pane in &pane_ids {
            server.resize_pane(*pane, MAX_PANE_DIMENSION, MAX_PANE_DIMENSION);
        }
        for runtime in server.panes.lock().expect("panes poisoned").values() {
            let mut parser: Processor = Processor::new();
            parser.advance(
                &mut *runtime.terminal.lock().expect("terminal poisoned"),
                &painted,
            );
        }

        let (client, driver) = std::os::unix::net::UnixStream::pair().unwrap();
        let serving = {
            let server = Arc::clone(&server);
            std::thread::spawn(move || server.handle_client(client))
        };
        let mut writer = driver.try_clone().unwrap();
        driver
            .set_read_timeout(Some(Duration::from_secs(120)))
            .unwrap();
        let mut reader = std::io::BufReader::new(driver);
        send_request(
            &mut writer,
            &crate::ClientRequest::Attach {
                version: crate::PROTOCOL_VERSION,
                workspace: WorkspaceId("bigsnap".into()),
                columns: 0,
                rows: 0,
                root: None,
            },
        )
        .unwrap();

        let mut repainted = std::collections::BTreeSet::new();
        while repainted.len() < pane_ids.len() {
            match read_event(&mut reader)
                .expect("every pane has to reach the client, one frame at a time")
            {
                Some(crate::ClientEvent::Damage(damage)) => {
                    assert_eq!(
                        (damage.columns, damage.rows),
                        (MAX_PANE_DIMENSION, MAX_PANE_DIMENSION)
                    );
                    assert_eq!(
                        damage.changed.len(),
                        usize::from(MAX_PANE_DIMENSION) * usize::from(MAX_PANE_DIMENSION),
                        "a repaint names every cell"
                    );
                    assert_eq!(
                        damage.changed[0].2.character, widest,
                        "the cells arrive as the pane actually holds them"
                    );
                    repainted.insert(damage.pane);
                }
                Some(_) => {}
                None => panic!("the server hung up instead of repainting every pane"),
            }
        }
        assert_eq!(
            repainted,
            pane_ids
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
        );

        let _ = send_request(&mut writer, &crate::ClientRequest::Detach);
        drop(writer);
        drop(reader);
        let _ = serving.join();
        server.stop_panes();

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A frame that will not go out has to end the connection, not just
    /// the thread that tried to write it. Dropping only the writer's dup
    /// leaves the peer's read half open: no EOF, no error, and a client
    /// sitting on chrome that still looks live while events it will never
    /// see pile up behind it.
    #[test]
    fn a_client_an_event_cannot_reach_is_disconnected_rather_than_frozen() {
        let (peer, socket) = std::os::unix::net::UnixStream::pair().unwrap();
        let (events, receiver) = std::sync::mpsc::channel();
        let writing = std::thread::spawn(move || super::forward_events(socket, &receiver));

        events
            .send(crate::ClientEvent::Error {
                message: "x".repeat(MAX_FRAME as usize + 1),
            })
            .unwrap();

        let mut read = peer;
        read.set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let mut byte = [0u8; 1];
        assert_eq!(
            std::io::Read::read(&mut read, &mut byte).unwrap(),
            0,
            "the peer must see EOF, which is what runs its disconnected path"
        );
        drop(events);
        writing.join().unwrap();
    }

    /// `uze terminal stop` is the documented way out of a server that has
    /// to go — and, since the workspace lock makes a survivor refuse every
    /// replacement, the only one short of a manual `kill`. It has to be
    /// heard by a server no client has ever attached to, which is where it
    /// was being dropped: `Stop` as a first frame fell through to "not an
    /// `Attach`" and the connection was closed without an answer.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn stop_is_heard_as_a_first_frame_by_a_server_nobody_attached_to() {
        let scratch = uze_testkit::temp::socket_scratch("stopfirst");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        let runtime_dir = scratch.join("runtime");
        for directory in [&uze_home, &project, &runtime_dir] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home)
            .set("XDG_RUNTIME_DIR", &runtime_dir);

        let endpoint = Endpoint::global().unwrap();
        let (served, serving) = std::sync::mpsc::channel();
        let serve_root = project.clone();
        std::thread::spawn(move || {
            let _ = served.send(super::serve(serve_root));
        });

        let mut ready = false;
        for _ in 0..200 {
            if std::os::unix::net::UnixStream::connect(&endpoint.socket).is_ok() {
                ready = true;
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        assert!(ready, "the server must be listening before it is stopped");

        super::stop(&project).expect("a running server must acknowledge stop");
        serving
            .recv_timeout(Duration::from_secs(30))
            .expect("the stopped server must leave its accept loop")
            .expect("and leave it cleanly");
        assert!(
            !endpoint.socket.exists() && !endpoint.pid.exists(),
            "a stopped server clears the endpoint it was reached at"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// `flock` says no for reasons that are not contention, and reading
    /// them all as contention told the person to go and stop a server that
    /// does not exist — permanently, on an `$UZE_HOME` that happens to sit
    /// on NFS, FUSE or a 9p mount, with no command that could clear it.
    #[test]
    fn only_a_held_lock_reads_as_another_server() {
        assert!(matches!(
            super::classify_lock_refusal(Some(libc::EWOULDBLOCK)),
            super::LockRefusal::Contended
        ));
        assert!(matches!(
            super::classify_lock_refusal(Some(libc::EINTR)),
            super::LockRefusal::Interrupted
        ));
        for unsupported in [libc::ENOLCK, libc::EOPNOTSUPP, libc::ENOSYS, libc::EBADF] {
            assert!(
                matches!(
                    super::classify_lock_refusal(Some(unsupported)),
                    super::LockRefusal::Unsupported
                ),
                "errno {unsupported} is a filesystem that cannot lock, not a server that holds one"
            );
        }
    }

    /// The number in a pid file goes straight to [`libc::kill`], and
    /// `libc::pid_t` is signed: `-1` is not a process, it is every process
    /// the user owns. UZE never writes such a file, but it reads one from a
    /// world a `/tmp` cleaner, a crash and a text editor all reach.
    #[test]
    fn a_pid_file_that_does_not_name_one_process_names_none() {
        let scratch = uze_testkit::temp::scratch("terminal-pid-negative");
        std::fs::create_dir_all(&scratch).unwrap();
        let pid_path = scratch.join("test.pid");

        for refused in ["-1", "0", "-4192325", "not-a-pid", ""] {
            std::fs::write(&pid_path, refused).unwrap();
            assert_eq!(
                super::read_pid(&pid_path),
                None,
                "{refused:?} must never reach kill(2)"
            );
        }
        std::fs::write(&pid_path, "4192325\n11").unwrap();
        assert_eq!(super::read_pid(&pid_path), Some(4192325));

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Corroboration gates one thing only: signalling a process. Where
    /// nothing can corroborate — a platform [`process_probe`] cannot answer
    /// for, or Linux with `/proc` unmounted — declining to signal loses
    /// nothing, because clearing the endpoint files is the whole recovery
    /// there. Taking the file's word for it instead is how a pid file could
    /// have aimed a `SIGKILL`.
    #[test]
    fn nothing_is_signalled_where_nothing_can_corroborate_it() {
        assert_eq!(
            corroborated_as_server(std::process::id() as libc::pid_t),
            platform_reads_processes() && super::runs_uze(std::process::id() as libc::pid_t),
            "corroboration is the process table's answer, never the absence of one"
        );
        if !platform_reads_processes() {
            assert!(
                !corroborated_as_server(std::process::id() as libc::pid_t),
                "a platform that cannot read processes corroborates nothing"
            );
        }
    }

    /// `SO_PEERCRED` names the process actually behind the socket, and the
    /// kernel stamps it — a pid file cannot forge it and a recycled pid
    /// cannot survive it. A file naming somebody else than the peer is the
    /// recycled-pid case itself, and `uze` is not a distinguishing name: a
    /// second server under a second `$UZE_HOME`, or an in-flight
    /// `uze install`, is one too.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_pid_file_that_disagrees_with_the_peer_gets_nobody_signalled() {
        let scratch = uze_testkit::temp::scratch("terminal-replace-disagree");
        std::fs::create_dir_all(&scratch).unwrap();
        let endpoint = Endpoint {
            socket: scratch.join("test.sock"),
            pid: scratch.join("test.pid"),
        };
        std::fs::write(&endpoint.socket, b"placeholder").unwrap();

        let server_binary = scratch.join("uze");
        std::fs::copy("/bin/sleep", &server_binary).unwrap();
        let mut named = std::process::Command::new(&server_binary)
            .arg("30")
            .spawn()
            .unwrap();
        std::fs::write(&endpoint.pid, named.id().to_string()).unwrap();

        // Everything the old rule asked for is true of the file — it names
        // a live process the table corroborates as `uze`. The peer is
        // somebody else, and that is the whole difference.
        assert!(corroborated_as_server(named.id() as libc::pid_t));
        replace_incompatible_server(&endpoint, Some(named.id() + 1)).unwrap();
        assert!(
            named.try_wait().unwrap().is_none(),
            "a pid file the socket's own peer contradicts must not get anything killed"
        );

        // And with no peer at all — a socket nobody listens on — there is
        // nothing to agree with, so there is nothing to signal either.
        std::fs::write(&endpoint.socket, b"placeholder").unwrap();
        replace_incompatible_server(&endpoint, None).unwrap();
        assert!(named.try_wait().unwrap().is_none());
        assert!(!endpoint.socket.exists(), "the endpoint is still cleared");
        assert!(!endpoint.pid.exists());

        let _ = named.kill();
        let _ = named.wait();
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// `SO_RCVTIMEO` restarts on every successful read, so a deadline
    /// spelled with it alone is no deadline at all: a peer dribbling a byte
    /// just inside it holds a reader thread, a writer thread and whatever
    /// it has allocated for as long as it likes — the very thing
    /// [`HANDSHAKE_DEADLINE`] says it prevents.
    #[test]
    fn a_dribbling_peer_runs_out_of_handshake_rather_than_restarting_it() {
        let (peer, socket) = std::os::unix::net::UnixStream::pair().unwrap();
        let dribbling = std::thread::spawn(move || {
            let mut peer = peer;
            for _ in 0..40 {
                if std::io::Write::write_all(&mut peer, &[0u8]).is_err() {
                    return;
                }
                thread::sleep(Duration::from_millis(60));
            }
        });

        let began = std::time::Instant::now();
        let mut reader =
            std::io::BufReader::new(super::Handshake::new(socket, Duration::from_millis(150)));
        let refused = super::read_message_within::<_, crate::ClientRequest>(
            &mut reader,
            super::MAX_HANDSHAKE_FRAME,
        );
        let waited = began.elapsed();

        assert!(
            refused.is_err(),
            "a peer that never finishes saying who it is has to be let go"
        );
        assert!(
            waited < Duration::from_secs(2),
            "the deadline bounds the whole handshake, not each read of it (waited {waited:?})"
        );
        drop(reader);
        let _ = dribbling.join();
    }

    /// The first frame is the one nothing has vouched for, and the only
    /// two things it may say are hundreds of bytes. Sizing it by the
    /// largest repaint this wire ever carries let a stranger reserve
    /// 64 MiB by writing four bytes.
    #[test]
    fn a_first_frame_is_bounded_by_what_a_handshake_says_not_by_a_repaint() {
        const { assert!(super::MAX_HANDSHAKE_FRAME < MAX_FRAME) };
        let mut wire: &[u8] = &(super::MAX_HANDSHAKE_FRAME + 1).to_le_bytes();
        assert!(
            matches!(
                super::read_message_within::<_, crate::ClientRequest>(
                    &mut wire,
                    super::MAX_HANDSHAKE_FRAME
                ),
                Err(RuntimeError::Protocol(_))
            ),
            "a handshake frame past the handshake's own bound is refused"
        );

        let attach = bincode::serialize(&crate::ClientRequest::Attach {
            version: crate::PROTOCOL_VERSION,
            workspace: WorkspaceId("a-workspace".into()),
            columns: 200,
            rows: 50,
            root: Some(PathBuf::from("/some/ordinary/project/path")),
        })
        .unwrap();
        assert!(
            attach.len() < super::MAX_HANDSHAKE_FRAME as usize,
            "and the bound still has to fit what a handshake actually says"
        );
    }
}
