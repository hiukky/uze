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
    CellAttributes, ClientEvent, ClientRequest, Cursor, MouseMode, NewSpace, PROTOCOL_VERSION,
    Palette, PaneDamage, PaneId, PaneSnapshot, RenderCell, Seating, Session, SpaceId, SpaceSeat,
    TabId, TerminalColor,
    launch::Launch,
    process_probe,
    state::{OpenedSpace, PLACEHOLDER_PANE_SIZE, SpaceSeed, TabSeed},
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
/// with its first space at `seat`, which only matters for a server that has
/// nothing persisted yet. The caller then sends `Attach` naming the seat it
/// wants a space for.
pub fn attach(seat: &SpaceSeat) -> Result<UnixStream, RuntimeError> {
    let _span = tracing::info_span!("terminal.attach", root = %seat.root.display()).entered();
    let socket = socket_path()?;
    // Settled before connecting, and by who is behind the socket rather than
    // by what it would say: a server of another build is alive, so a connect
    // succeeds, and one built to another framing may never answer an
    // `Attach` well enough to refuse it.
    match arrival(workspace_is_claimed(), listener_at(&socket)) {
        // The claim says a server is alive. `connect_waiting` gives it the
        // two seconds its endpoint watch needs to put a wiped socket back;
        // past that the server is alive somewhere this build cannot reach
        // — an endpoint named by rules an older one used — and the
        // workspace is shut until it ends. Ending it is not a loss: the
        // shape of every space and pane is persisted, so the server that
        // replaces it restores them and its agents resume their own
        // conversations.
        Arrival::Connect => match connect_waiting(&socket) {
            Ok(stream) => return Ok(stream),
            Err(cause) => match claim_holder() {
                Some(pid) => {
                    tracing::warn!(
                        pid,
                        socket = %socket.display(),
                        "a server holds this workspace and answers nowhere this build looks; \
                         retiring it"
                    );
                    retire(pid, &socket);
                }
                None => return Err(unreachable(&socket, Some(cause))),
            },
        },
        // Another image is not another protocol. A `make install` over a
        // running server is the ordinary state of this repository's own
        // development, and it leaves every later `uze` looking at a server
        // whose binary is no longer the one on disk — while that server is
        // running somebody's agents, whose processes end with it. So it is
        // asked rather than assumed: a server that answers this build's
        // handshake can serve this build, whatever it was built from, and
        // only one that cannot is retired.
        Arrival::Ask(pid) => {
            // A server that answered a moment ago and cannot be reached
            // now is one that has just gone: taken as an unanswered
            // question rather than as an error, since the answer to that
            // is the same server started fresh.
            if serves_this_build(&socket)
                && let Ok(stream) = connect_waiting(&socket)
            {
                return Ok(stream);
            }
            tracing::warn!(
                pid,
                socket = %socket.display(),
                "the server here does not answer this build's handshake; retiring it"
            );
            retire(pid, &socket);
        }
        Arrival::Replace(pid) => retire(pid, &socket),
        Arrival::Start => {}
    }
    start_server(seat)?;
    connect_waiting(&socket)
}

/// How long the server at the endpoint has to answer whether it can serve
/// this client. Bounded because the alternative to an answer is retiring
/// it, and a server that says nothing in two seconds is a server no
/// attach can wait on.
const ANSWERS_WITHIN: Duration = Duration::from_secs(2);

/// Whether the server at `socket` can serve this build, asked the one way
/// that can answer it: by handshaking with it.
///
/// Its image says which binary it was started from and nothing about what
/// it speaks — [`PROTOCOL_VERSION`] is what says that, and a server
/// carrying this one is a server this client can talk to however its
/// binary was built. A snapshot in answer is the proof: the server read
/// this build's `Attach`, accepted its version and described the workspace
/// in a shape this build could read back. An error, a hang-up, silence,
/// or bytes this build cannot read are each the opposite — and are what a
/// server built to another framing answers, which is the state this
/// question exists to find.
///
/// Attaches for the answer and leaves, the way [`open_space`] does: no
/// size, so no pane anybody is looking at is resized, and `Detach` before
/// the caller's own connection is made.
fn serves_this_build(socket: &Path) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return false;
    };
    // Both directions: an attach must not be able to hang on a server that
    // accepted the connection and then stopped reading it, which is the
    // same failure as one that never answers.
    if stream.set_read_timeout(Some(ANSWERS_WITHIN)).is_err()
        || stream.set_write_timeout(Some(ANSWERS_WITHIN)).is_err()
    {
        return false;
    }
    let asked = send_request(
        &mut stream,
        &ClientRequest::Attach {
            version: PROTOCOL_VERSION,
            columns: 0,
            rows: 0,
            seating: Seating::WhereItLeftOff,
        },
    );
    if asked.is_err() {
        return false;
    }
    // A read timeout is per read, so a server dribbling events would renew
    // it forever: the whole question is bounded, not each answer to it.
    let deadline = Instant::now() + ANSWERS_WITHIN;
    let served = loop {
        match read_event(&mut stream) {
            Ok(Some(ClientEvent::Snapshot { .. })) => break true,
            Ok(Some(ClientEvent::Error { .. })) | Ok(None) | Err(_) => break false,
            // Anything else is a server talking, which is neither answer
            // yet — a repaint can reach a client before its snapshot does.
            Ok(Some(_)) if Instant::now() < deadline => {}
            Ok(Some(_)) => break false,
        }
    };
    let _ = send_request(&mut stream, &ClientRequest::Detach);
    served
}

/// Says what a failed connect to a claimed workspace actually means,
/// which the error from the socket cannot.
///
/// Only reached where the claim names nobody this process can act on: a
/// server older than [`record_claimant`], or a pid the process table no
/// longer vouches for. Whoever holds the workspace then has to be found
/// by hand, so the message says so rather than reporting `No such file
/// or directory` about a path the operator never typed.
fn unreachable(socket: &Path, cause: Option<RuntimeError>) -> RuntimeError {
    let because = cause.map_or_else(String::new, |cause| format!(" ({cause})"));
    RuntimeError::Protocol(format!(
        "a uze is serving this workspace and answers nowhere this build looks — not at \
         {}{because} — and the claim does not name it, so it is older than this build's \
         record of who serves. Find it with `pgrep -fa \'uze terminal serve\'`, end it, and \
         open uze again.",
        socket.display()
    ))
}

/// What [`attach`] does about the endpoint it found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Arrival {
    Connect,
    Start,
    /// Connect to the server at this pid if it can serve this build, and
    /// only otherwise end it and start one.
    Ask(u32),
    /// End the server at this pid, then start one.
    Replace(u32),
}

/// The decision [`attach`] makes, from the two facts that can prove it: who
/// holds the workspace claim, and who the kernel says is listening.
///
/// Nothing here unlinks an endpoint — only a server holding the claim does
/// (see [`serve`]) — so a listener nobody can vouch for is connected to
/// rather than taken down: a platform that cannot read the process table
/// must never cost a live session its socket. Nothing here ends a live
/// server of this workspace either: that is [`serves_this_build`]'s
/// answer to give, and only after the server has failed to give it.
fn arrival(claimed: bool, listener: Listener) -> Arrival {
    match (claimed, listener) {
        // Alive, serving this workspace, and built from another image.
        // Which of those matters is settled by asking it, never by the
        // image: retiring a live server costs every agent it runs its
        // process, and the image is no evidence that it had to be paid.
        (true, Listener::AnotherBuild(pid)) => Arrival::Ask(pid),
        (true, _) => Arrival::Connect,
        // A server claims the workspace before it binds, so a `uze`
        // listening with the claim free is serving a workspace deleted
        // under it — `$UZE_HOME` removed while it ran. Attaching to it would
        // show spaces from a world that no longer exists.
        (false, Listener::ThisBuild(pid) | Listener::AnotherBuild(pid)) => Arrival::Replace(pid),
        (false, Listener::Nobody | Listener::Unrecognized) => Arrival::Start,
    }
}

/// Who answers at the endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Listener {
    Nobody,
    /// This very executable, and so compiled with this `PROTOCOL_VERSION`.
    ThisBuild(u32),
    /// A `uze` running another image — a `make install` over a running
    /// server, which also leaves the image it was started from reading as
    /// deleted. Which protocol it speaks is a separate question, and the
    /// only one that decides anything: see [`serves_this_build`].
    AnotherBuild(u32),
    /// A process the process table cannot vouch for as `uze`: another
    /// program, or a platform that cannot say.
    Unrecognized,
}

fn listener_at(socket: &Path) -> Listener {
    listening_peer(socket).map_or(Listener::Nobody, identify)
}

fn identify(pid: u32) -> Listener {
    if runs_this_executable(pid) {
        Listener::ThisBuild(pid)
    } else if signalable(pid).is_some_and(runs_uze) {
        Listener::AnotherBuild(pid)
    } else {
        Listener::Unrecognized
    }
}

/// Where the user's server listens — one per `UZE_HOME`, which is what
/// "user" means to UZE: a second home is a second world, with a server of
/// its own.
///
/// The endpoint goes beside the workspace it serves, under `$UZE_HOME`,
/// for the two reasons [`workspace_lock_path`] gives for the claim: a
/// cleaner that can reach it has taken the workspace too, and it is the
/// same path for every terminal, whatever their environment says.
///
/// Both halves of that were costing sessions. `XDG_RUNTIME_DIR` is
/// somebody else's variable — on WSL it is routinely set to a
/// `/run/user/<uid>` that does not exist — so the endpoint fell to the
/// temp dir, where `systemd-tmpfiles` takes it out from under a live
/// server (see [`spawn_endpoint_watch`]); and two terminals whose
/// environments disagree about `XDG_RUNTIME_DIR` or `TMPDIR` computed
/// two different endpoints for one workspace, so the second one found
/// nothing listening at a path the first had never used.
///
/// The three older candidates remain, for the one thing `$UZE_HOME`
/// cannot promise: length. `sockaddr_un.sun_path` is ~100 bytes and a
/// home is wherever the operator put it, so a path that would not fit
/// steps to the runtime directory, the system temp dir, and `/tmp` in
/// turn. Falling back does not weaken isolation: the socket is named
/// after a hash of `UZE_HOME`, so two homes stay two endpoints wherever
/// they land.
pub fn socket_path() -> Result<PathBuf, RuntimeError> {
    let identity = identity_of(&uze_home_dir());
    let named = |root: &Path| root.join(format!("uze-{identity}.sock"));
    let owner = current_uid();

    let candidates = [
        uze_home_dir().join("state").join("terminal"),
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
            match fs::create_dir_all(candidate).and_then(|()| private_directory(candidate, owner)) {
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
    Ok(named(&runtime))
}

/// Asks the running server for a space at `seat` — created when
/// none is — and answers with its label. For a `uze` started inside one of
/// the server's own panes: it must not open a client inside a client, so
/// it opens a space in the one it is already in and leaves. An error when
/// no server is running.
pub fn open_space(seat: SpaceSeat) -> Result<String, RuntimeError> {
    let _span = tracing::info_span!("terminal.open_space", root = %seat.root.display()).entered();
    let mut stream = UnixStream::connect(socket_path()?)
        .map_err(|_| RuntimeError::Protocol("no running uze to open a space in".into()))?;
    send_request(
        &mut stream,
        &ClientRequest::Attach {
            version: PROTOCOL_VERSION,
            columns: 0,
            rows: 0,
            seating: crate::Seating::Open(seat),
        },
    )?;
    let label = loop {
        match read_event(&mut stream)? {
            Some(ClientEvent::Snapshot { session }) => {
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
pub fn stop() -> Result<(), RuntimeError> {
    let _span = tracing::info_span!("terminal.stop").entered();
    let socket = socket_path()?;
    let mut stream = match UnixStream::connect(&socket) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            // Nothing answers here, which is not the same as nothing
            // running: a server on an endpoint this build no longer names
            // still holds the workspace, and a `stop` that reported
            // success while it did is what left restarting the machine as
            // the only way out. What the claim names is what is stopped;
            // a claim that names nobody — a server older than the record
            // — is said, never reported as success.
            return match claim_holder() {
                Some(pid) => {
                    retire(pid, &socket);
                    Ok(())
                }
                None if workspace_is_claimed() => Err(unreachable(&socket, None)),
                None => Ok(()),
            };
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

/// Serves the user's one workspace. `seat` is the first space when nothing
/// is persisted yet, and is otherwise ignored.
pub fn serve(seat: SpaceSeat) -> Result<(), RuntimeError> {
    let _span = tracing::info_span!("terminal.serve", root = %seat.root.display()).entered();
    let socket = socket_path()?;
    // The workspace is claimed before the endpoint is: `Server::new` takes
    // the lock that makes this the one server restoring these spaces, so by
    // the time the socket is bound no other live server of this workspace
    // can own it — whatever sits at the path is a crashed server's
    // leftover or a server of a workspace deleted under it, and is
    // reclaimed; and a socket that later stops being the one bound here is
    // something [`spawn_endpoint_watch`] reclaims rather than a peer's.
    let (server, damage) = Server::new(seat, socket.clone())?;
    let state = Arc::new(server);
    let listener = bind_endpoint(&socket)?;
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
        let _ = fs::remove_file(&socket);
    }
    accepted
}

/// Binds the endpoint over whatever sits at its path — only ever called by
/// the server holding the workspace claim, for which nothing there can be a
/// live peer. The socket is created inside a directory [`socket_path`] has
/// already proven to be this user's and unreachable by anyone else, so the
/// moment between `bind` and the mode below is not a window anything can
/// walk through.
fn bind_endpoint(socket: &Path) -> Result<UnixListener, RuntimeError> {
    match fs::remove_file(socket) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let listener = UnixListener::bind(socket)?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
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

/// How long a Unix-domain socket path may be, with room to spare.
///
/// `sockaddr_un.sun_path` holds 104 bytes on macOS and 108 on Linux, and the
/// whole path has to fit or `bind` fails with `SUN_LEN` — an error naming the
/// limit and nothing about which directory exhausted it. The smaller of the
/// two, less a little, is what [`socket_path`] holds itself to, so the
/// same directory is usable on either platform.
const MAX_SOCKET_PATH: usize = 100;

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
/// deliberately not [`socket_path`]'s `XDG_RUNTIME_DIR`/temp directory
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
/// restoring — and persisting over — the same workspace, and the one proof
/// of liveness [`attach`] trusts.
///
/// The endpoint alone cannot give that proof. `systemd-tmpfiles` wiping
/// `/tmp` under a live server takes the socket with it, so the next
/// `attach` reads "no server", starts a second one, and both restore the
/// same `workspace.json`: every agent exists twice in the same checkout,
/// and the two servers persist over each other. This lock lives where the
/// workspace lives, so a cleaner that can reach it has taken the workspace
/// too.
///
/// `flock` and not a pid file: the kernel releases it when the holder dies,
/// however it dies — a crash, a `kill -9`, a zombie nobody reaped — so a
/// stale claim is impossible by construction. A server holds it exclusively;
/// [`workspace_is_claimed`] asks with a shared lock. Panes never inherit it:
/// the descriptor is opened close-on-exec, and `portable-pty` closes every
/// descriptor above stdio in the child before `exec` besides.
struct WorkspaceLock {
    _file: fs::File,
}

impl WorkspaceLock {
    fn acquire() -> Result<Self, RuntimeError> {
        let mut file = open_workspace_lock()?;
        loop {
            match flock(&file, libc::LOCK_EX | libc::LOCK_NB) {
                Ok(()) => {
                    record_claimant(&mut file);
                    return Ok(Self { _file: file });
                }
                Err(LockRefusal::Interrupted) => {}
                Err(LockRefusal::Unsupported(error)) => return Err(RuntimeError::Io(error)),
                Err(LockRefusal::Contended) => {
                    if held_by_a_server(&file)? {
                        return Err(RuntimeError::Protocol(
                            "another uze terminal server is already serving this workspace".into(),
                        ));
                    }
                    // A client was asking; it lets go as soon as it has its
                    // answer.
                    thread::yield_now();
                }
            }
        }
    }
}

/// Writes this server's pid into the claim it has just taken.
///
/// The lock alone proves a server is alive and says nothing about which
/// one, and `flock` names no holder. A client that cannot reach the
/// endpoint then has no way to end what is holding the workspace — the
/// state an operator lands in whenever the endpoint's own rules change
/// between builds, where `uze terminal stop` looked at the new endpoint,
/// found nothing, and reported nothing to stop while the old server held
/// the workspace shut. Restarting the machine was the only way out.
///
/// Best-effort by construction: the claim is the lock, never this. What
/// is written here is a lead, and every reader corroborates it against
/// the process table before acting on it (see [`claim_holder`]).
fn record_claimant(file: &mut fs::File) {
    let pid = std::process::id();
    let _ = file.set_len(0);
    let _ = write!(file, "{pid}");
    let _ = file.flush();
}

/// The pid recorded in the claim, when the process table still says it is
/// a `uze`. `None` where nothing was recorded, the pid died, or it was
/// recycled by something else — in which case the claim is either free or
/// held by a server that predates this record, and the caller has to say
/// so rather than signal a stranger.
fn claim_holder() -> Option<u32> {
    let recorded = fs::read_to_string(workspace_lock_path()).ok()?;
    let pid: u32 = recorded.trim().parse().ok()?;
    signalable(pid).filter(|target| runs_uze(*target))?;
    Some(pid)
}

/// Whether a live server holds the workspace claim. A filesystem that
/// cannot lock answers "claimed": the lock proves nothing there, and
/// replacing a server on no evidence would end a live session.
fn workspace_is_claimed() -> bool {
    open_workspace_lock()
        .and_then(|file| held_by_a_server(&file))
        .unwrap_or(true)
}

/// Whether the lock on `file` is held exclusively — by a server — asked by
/// taking it shared for an instant.
///
/// Shared is how a client asks, so any number of clients ask at once, and a
/// server starting while one does is not refused as though it had met
/// another server: its exclusive attempt fails beside the asker, and this
/// question, granted beside an asker and refused beside a server, is what
/// tells the two apart.
fn held_by_a_server(file: &fs::File) -> io::Result<bool> {
    loop {
        match flock(file, libc::LOCK_SH | libc::LOCK_NB) {
            Ok(()) => {
                let _ = flock(file, libc::LOCK_UN);
                return Ok(false);
            }
            Err(LockRefusal::Interrupted) => {}
            Err(LockRefusal::Contended) => return Ok(true),
            Err(LockRefusal::Unsupported(error)) => return Err(error),
        }
    }
}

fn open_workspace_lock() -> io::Result<fs::File> {
    let path = workspace_lock_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
}

fn flock(file: &fs::File, operation: libc::c_int) -> Result<(), LockRefusal> {
    // SAFETY: `file` owns the descriptor for the whole call, and a lock it
    // takes is released by the kernel when the descriptor closes.
    if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    Err(classify_lock_refusal(error))
}

/// Why `flock` said no.
///
/// Only one of its answers means another holder. A signal arriving mid-call
/// is not an answer at all, and a filesystem that cannot lock — `ENOLCK`,
/// and the `EOPNOTSUPP`/`ENOSYS` some NFS, FUSE and 9p mounts give — is a
/// different failure entirely: reading either as contention told the person
/// to go and stop a server that does not exist, permanently, with no
/// command that could clear it.
#[derive(Debug)]
enum LockRefusal {
    Interrupted,
    Contended,
    Unsupported(io::Error),
}

fn classify_lock_refusal(error: io::Error) -> LockRefusal {
    match error.raw_os_error() {
        Some(libc::EINTR) => LockRefusal::Interrupted,
        // The same number on Linux, two names elsewhere; both mean held.
        Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN => LockRefusal::Contended,
        _ => LockRefusal::Unsupported(error),
    }
}

#[derive(Serialize, serde::Deserialize)]
struct PersistedWorkspace {
    /// The shape this document is in. Read before the document, out of
    /// the one field every version of it carries, so a workspace written
    /// by another build is *named* rather than parsed into silence: the
    /// spaces of version 1 carried a kind per space, which this build has
    /// no field for and `serde` would drop without a word.
    #[serde(default = "first_workspace_schema")]
    schema_version: u32,
    spaces: Vec<SpaceSeed>,
}

/// A document with no version is the one written before this field
/// existed — version 1 by definition, for every kind of document UZE
/// owns.
fn first_workspace_schema() -> u32 {
    uze_document::FIRST_SHAPE
}

/// What this build writes, and the only version it restores.
const WORKSPACE_SCHEMA_VERSION: u32 = 2;

impl uze_document::Shaped for PersistedWorkspace {
    const SHAPE: u32 = WORKSPACE_SCHEMA_VERSION;
    const KIND: &'static str = "workspace";

    /// Shape 1 gave every space a kind of its own — isolated or not — and
    /// `add-space-kinds` moved that choice onto the agent, where the domain
    /// had already put it. Nothing else about a space moved: the root, the
    /// tabs, each tab's directory and launch are the same fields.
    ///
    /// This is the rung whose absence cost an operator their spaces on
    /// 2026-09-19. `serde` would have ignored the extra field on its own;
    /// what set the document aside was the version guard having no way to
    /// say "that difference does not matter". Dropping it explicitly is how
    /// the next reader learns what the difference *was*.
    fn ladder() -> uze_document::Ladder {
        &[uze_document::Step {
            from: 1,
            to: 2,
            climb: |mut document| {
                if let Some(spaces) = document
                    .get_mut("spaces")
                    .and_then(serde_json::Value::as_array_mut)
                {
                    for space in spaces {
                        if let Some(space) = space.as_object_mut() {
                            space.remove("kind");
                        }
                    }
                }
                Ok(document)
            },
        }]
    }
}

/// Best-effort: a workspace with nothing persisted yet (first run, or the
/// file is missing/unreadable/corrupt) is not an error — [`Server::new`]
/// falls back to its ordinary fresh-bootstrap path exactly as if this
/// returned `None` from the start.
/// The workspace at `path`, and what reading it cost.
///
/// A shape this build knows is carried across and the spaces survive —
/// that is the first answer, and it is silent. Only a workspace that
/// cannot be climbed at all is set aside: the bytes are kept under a name
/// nothing reads as a workspace, the server starts from the seat it was
/// given, and the caller is handed the [`SetAside`] so the operator can be
/// told at the screen rather than in a log nobody turned on.
///
/// A workspace from a *newer* build is never touched. Two builds on one
/// machine is ordinary here, and taking a newer one's record would have
/// them destroying each other's in turn.
fn load_persisted_workspace_at(
    path: &Path,
) -> (Option<PersistedWorkspace>, Option<uze_document::SetAside>) {
    match uze_document::read::<PersistedWorkspace>(path) {
        Ok(uze_document::Carried::Absent) => (None, None),
        Ok(uze_document::Carried::Current(workspace)) => (Some(workspace), None),
        Ok(uze_document::Carried::Climbed { record, from }) => {
            tracing::debug!(
                workspace = %path.display(),
                from,
                to = WORKSPACE_SCHEMA_VERSION,
                "the persisted workspace was carried across"
            );
            (Some(record), None)
        }
        Err(reason) if uze_document::may_be_set_aside(&reason) => {
            match uze_document::set_aside(
                path,
                <PersistedWorkspace as uze_document::Shaped>::KIND,
                &reason,
            ) {
                Ok(moved) => (None, Some(moved)),
                Err(error) => {
                    tracing::warn!(%error, "the persisted workspace could not be set aside");
                    (None, None)
                }
            }
        }
        Err(reason) => {
            tracing::warn!(%reason, "the persisted workspace was written by a newer build; left as it is");
            (None, None)
        }
    }
}

/// Replaces `path`'s contents in one step, so a reader only ever sees the
/// old file or the new one.
///
/// The whole workspace is rewritten on every structural change, and a plain
/// write truncates before it fills: a crash, a `kill -9`, a full disk or a
/// power loss in that window leaves a half-written file, which
/// [`load_persisted_workspace_at`] cannot parse and therefore reads as
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

/// A best-effort relaunch command for a pane that was spawned as a shell
/// (see [`PaneRuntime::launch`]) but whose last-
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

/// The binary to start a server with: this one, unless this one is no
/// longer on disk.
///
/// `current_exe` reads `/proc/self/exe`, and a binary replaced under a
/// running process — a `make install` while a client is up, which is the
/// ordinary state of this repository's own development — resolves to
/// `<path> (deleted)`. Spawning that answers `No such file or directory`,
/// from a command that never named a file: the operator is told a path is
/// missing and given no way to tell which. [`runs_this_executable`]
/// already knows this state exists; this is the other half of knowing it.
///
/// The replacement is `uze` as `PATH` resolves it — the same binary the
/// operator just installed over this one, which is the one they want
/// serving anyway.
fn server_executable() -> Result<PathBuf, RuntimeError> {
    let current = env::current_exe()?;
    if current.exists() {
        return Ok(current);
    }
    which_uze().ok_or_else(|| {
        RuntimeError::Protocol(format!(
            "{} is gone (replaced while it ran) and no `uze` on PATH replaces it",
            current.display()
        ))
    })
}

/// `uze` as `PATH` resolves it, resolved here rather than left to the
/// shell: `Command::new("uze")` would search the *server's* environment,
/// and the server is spawned with the client's.
fn which_uze() -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|directory| directory.join("uze"))
        .find(|candidate| candidate.is_file())
}

fn start_server(seat: &SpaceSeat) -> Result<(), RuntimeError> {
    use std::os::unix::process::CommandExt;

    let executable = server_executable()?;
    std::process::Command::new(executable)
        .args(["terminal", "serve", "--root"])
        .arg(&seat.root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        // A process group of its own, or the server sits in the launching
        // terminal's: a `SIGHUP` when that terminal closes, or a `Ctrl+C`
        // to its foreground group, would take down every pane — precisely
        // the property this runtime exists to hold (ADR-038).
        .process_group(0)
        .spawn()?;
    Ok(())
}

/// How long a client waits for a server to answer: one that is still
/// restoring its panes, or one whose socket a cleaner took and whose
/// [`spawn_endpoint_watch`] has yet to put it back.
const READY_WITHIN: Duration = Duration::from_secs(2);

fn connect_waiting(socket: &Path) -> Result<UnixStream, RuntimeError> {
    let deadline = Instant::now() + READY_WITHIN;
    loop {
        match UnixStream::connect(socket) {
            Ok(stream) => return Ok(stream),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(25))
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) =>
            {
                return Err(RuntimeError::Protocol(
                    "terminal server did not become ready".into(),
                ));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Whether `pid` is running `uze` — asked again right before a signal, since
/// the kernel's answer about a socket's peer is a moment old by then.
///
/// The *path* is expected to differ (an upgrade moving the binary is the
/// ordinary reason a server is being replaced), so the image's file name is
/// what is compared — through Linux's marker for a binary replaced
/// underneath a live process, which is exactly the state the server being
/// replaced is in. A process the table cannot read — a zombie, a platform
/// [`process_probe`] does not answer for — is not `uze`.
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

/// Whether `pid` runs the same executable image as this process. The image
/// stops resolving to this path once the binary is replaced underneath a
/// live server (a `cargo install --force` mid-session), which is exactly
/// the state a server being replaced is in.
fn runs_this_executable(pid: u32) -> bool {
    let Some(mine) = env::current_exe().ok() else {
        return false;
    };
    process_probe::executable_of(pid).is_some_and(|image| image == mine)
}

/// The pid listening on `socket`. The kernel stamps the listener's
/// credentials onto the connection, so this is the listener's own and not
/// something a connection could claim. `None` when nobody
/// answers, or when the platform cannot say.
fn listening_peer(socket: &Path) -> Option<u32> {
    let stream = UnixStream::connect(socket).ok()?;
    process_probe::peer_pid(&stream)
}

/// `pid` as something `kill(2)` may be given — only where it names one
/// process. `kill(0, …)` is the caller's own process group and a negative
/// pid is a group too, `-1` every process the user owns.
fn signalable(pid: u32) -> Option<libc::pid_t> {
    libc::pid_t::try_from(pid).ok().filter(|pid| *pid > 0)
}

/// How long a server being replaced has to let go before it is made to.
const RETIRE_WITHIN: Duration = Duration::from_secs(1);

/// Ends a server this client cannot use: a cooperative `SIGTERM` first —
/// its persisted workspace is what lets the fresh server restore the same
/// tabs — and `SIGKILL` only if it has not let go of the endpoint and the
/// claim promptly. A pid that is not running `uze` by the time it would be
/// signalled is left alone.
fn retire(pid: u32, socket: &Path) {
    let Some(target) = signalable(pid) else {
        return;
    };
    let released = || listening_peer(socket) != Some(pid) && !workspace_is_claimed();
    for signal in [libc::SIGTERM, libc::SIGKILL] {
        if !runs_uze(target) {
            return;
        }
        // SAFETY: `target` is a positive pid (`signalable` refuses 0 and
        // negatives, which would address a group or every process) that
        // was just confirmed to run `uze`.
        unsafe { libc::kill(target, signal) };
        let deadline = Instant::now() + RETIRE_WITHIN;
        while !released() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        if released() {
            return;
        }
    }
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
    events: Arc<Outbox>,
    selection: Selection,
}

/// How many events a client may have waiting on its socket before it is
/// treated as stale. A frame is at most one pane's repaint, so this bounds
/// what a client that stopped reading can make the server hold.
const OUTBOX_CAPACITY: usize = 256;

/// The events waiting for one client's socket.
///
/// Bounded, because a client that stops reading — a suspended `uze`, a
/// stalled socket — would otherwise have every repaint of every pane
/// queued for it for as long as it stays attached. What overflows is not
/// kept: the client is marked stale and, once its queue has drained, is
/// sent the whole workspace again (see [`Server::resync_stale_clients`]).
/// Nothing is lost by dropping repaints — each one carries absolute cells,
/// and the resync supersedes all of them.
struct Outbox {
    sender: mpsc::SyncSender<ClientEvent>,
    backlog: Arc<Backlog>,
}

/// How far behind one client is — the only part of its [`Outbox`] the
/// writer thread is given.
///
/// It is split out because the sender must not be: a writer holding an
/// `Outbox` holds a sender to the very channel it is blocked on, so
/// `recv` could never report the client gone and the thread outlived
/// the connection by the life of the server. Every probe of the endpoint
/// then cost a thread and a descriptor permanently, and a machine that
/// had run for a day could no longer `fork`.
struct Backlog {
    pending: std::sync::atomic::AtomicUsize,
    stale: std::sync::atomic::AtomicBool,
}

impl Backlog {
    fn delivered(&self) {
        self.pending
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Outbox {
    fn new() -> (Self, mpsc::Receiver<ClientEvent>) {
        let (sender, receiver) = mpsc::sync_channel(OUTBOX_CAPACITY);
        let outbox = Self {
            sender,
            backlog: Arc::new(Backlog {
                pending: std::sync::atomic::AtomicUsize::new(0),
                stale: std::sync::atomic::AtomicBool::new(false),
            }),
        };
        (outbox, receiver)
    }

    /// A handle for the writer thread serving this client.
    fn backlog(&self) -> Arc<Backlog> {
        Arc::clone(&self.backlog)
    }

    /// Queues a broadcast without ever waiting, and answers whether the
    /// client is still there. A stale client is sent nothing until it is
    /// resynchronized.
    fn offer(&self, event: ClientEvent) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        if self.backlog.stale.load(Relaxed) {
            return true;
        }
        match self.sender.try_send(event) {
            Ok(()) => {
                self.backlog.pending.fetch_add(1, Relaxed);
                true
            }
            Err(mpsc::TrySendError::Full(_)) => {
                self.backlog.stale.store(true, Relaxed);
                true
            }
            Err(mpsc::TrySendError::Disconnected(_)) => false,
        }
    }

    /// Queues an answer to this client's own request. It may wait: only
    /// the thread serving this client is held, and an answer is not
    /// something a resync could stand in for.
    fn reply(&self, event: ClientEvent) {
        if self.sender.send(event).is_ok() {
            self.backlog
                .pending
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Whether this client missed broadcasts and has since caught up with
    /// everything it was sent, so a resync would reach it.
    fn awaits_resync(&self) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        self.backlog.stale.load(Relaxed) && self.backlog.pending.load(Relaxed) == 0
    }

    /// Sends the whole workspace to a stale client, and answers whether the
    /// client is still there. A resync that overflows again leaves the
    /// client stale; the next one starts from a fresh `Snapshot`.
    fn resync(&self, session: Session, repaints: &[PaneDamage]) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        self.backlog.stale.store(false, Relaxed);
        let events = std::iter::once(ClientEvent::Snapshot { session })
            .chain(repaints.iter().cloned().map(ClientEvent::Damage));
        for event in events {
            if !self.offer(event) {
                return false;
            }
            if self.backlog.stale.load(Relaxed) {
                break;
            }
        }
        true
    }
}

struct Server {
    session: Mutex<Session>,
    panes: Mutex<BTreeMap<PaneId, Arc<PaneRuntime>>>,
    clients: Mutex<Vec<Client>>,
    next_client: std::sync::atomic::AtomicU64,
    stopped: Mutex<bool>,
    socket: PathBuf,
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
    /// The workspace this runtime could not carry across, held until a
    /// client is there to be told.
    ///
    /// Held rather than logged, and held rather than dropped: the runtime
    /// starts before any client attaches, and the one time this happened
    /// the only record was a `tracing::warn!` to a file sink that is off
    /// unless `UZE_LOG` is set. An operator watched every space disappear
    /// with no sentence anywhere.
    set_aside: Mutex<Option<uze_document::SetAside>>,
}

impl Server {
    fn new(
        seat: SpaceSeat,
        socket: PathBuf,
    ) -> Result<(Self, mpsc::Receiver<PaneId>), RuntimeError> {
        // Taken before anything is read: restoring a workspace a live
        // server already holds is what turns one set of agents into two.
        let workspace_lock = WorkspaceLock::acquire()?;
        // A previous run's shape, if this workspace has one — see
        // `persisted_state_path` for why a crash, a `kill -9`, or a reboot
        // still leaves this behind even though nothing else about a pane's
        // running state survives any of those.
        let (restored, set_aside) = load_persisted_workspace_at(&persisted_state_path());
        let (session, launches) = restored
            .and_then(|persisted| Session::restore(persisted.spaces))
            .unwrap_or_else(|| {
                let (columns, rows) = PLACEHOLDER_PANE_SIZE;
                let session = Session::new(seat, columns, rows);
                let bootstrap = session.selected_tab().pane.id;
                (session, vec![(bootstrap, Launch::Shell)])
            });
        let (damage, damage_events) = mpsc::channel();
        let server = Self {
            session: Mutex::new(session),
            panes: Mutex::new(BTreeMap::new()),
            clients: Mutex::new(Vec::new()),
            next_client: std::sync::atomic::AtomicU64::new(1),
            stopped: Mutex::new(false),
            socket,
            _workspace: workspace_lock,
            persisting: Mutex::new(()),
            damage,
            palette: Arc::new(Mutex::new(Palette::default())),
            set_aside: Mutex::new(set_aside),
        };
        for (pane, launch) in launches {
            // A persisted program is a guess (an agent binary that may
            // since be uninstalled or renamed, or a best-effort relaunch
            // built from a live process name — see
            // `relaunch_command_for_process`) — one bad guess must never
            // keep the rest of a restored workspace from coming back, so a
            // failed program retries as a shell; a shell failing to spawn is
            // fatal.
            let guessed = matches!(launch, Launch::Program { .. });
            let spawned = server.spawn_pane(pane, launch);
            if spawned.is_err() && guessed {
                let _ = server.spawn_pane(pane, Launch::Shell);
            } else {
                spawned?;
            }
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
        let workspace =
            PersistedWorkspace {
                schema_version: WORKSPACE_SCHEMA_VERSION,
                spaces: session
                    .workspace
                    .spaces
                    .iter()
                    .map(|space| SpaceSeed {
                        label: space.label.clone(),
                        root: space.root.clone(),
                        tabs: space
                            .tabs
                            .iter()
                            .map(|tab| {
                                // By position, since a restored tab is minted a
                                // fresh id — and against this same list, which
                                // is the one `Session::restore` will rebuild.
                                let agent = tab.agent.and_then(|agent| {
                                    space.tabs.iter().position(|other| other.id == agent)
                                });
                                // A shell with something other than a shell now
                                // running in it (someone typed `claude` straight
                                // into it) had an agent as much as a launched
                                // one did — restoring it to a bare shell would
                                // silently drop that. What was typed was
                                // launched by nobody, so it carries no
                                // environment.
                                let launch =
                                    match panes.get(&tab.pane.id).map(|runtime| &runtime.launch) {
                                        Some(launch @ Launch::Program { .. }) => launch.clone(),
                                        _ => relaunch_command_for_process(&tab.pane.process)
                                            .map_or(Launch::Shell, |argv| Launch::Program {
                                                argv,
                                                env: Vec::new(),
                                            }),
                                    };
                                TabSeed {
                                    label: tab.label.clone(),
                                    cwd: tab.pane.cwd.clone(),
                                    agent,
                                    launch,
                                }
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
        let (outbox, receiver) = Outbox::new();
        let events = Arc::new(outbox);
        let backlog = events.backlog();
        thread::spawn(move || forward_events(stream, &receiver, &backlog));

        // A deadline on the handshake only — see [`HANDSHAKE_DEADLINE`] —
        // and a frame limit sized for what a handshake actually says rather
        // than for the largest repaint this wire ever carries: nothing has
        // vouched for this peer yet.
        let mut reader = BufReader::new(Handshake::new(reader_stream, HANDSHAKE_DEADLINE));
        let first = read_message_within::<_, ClientRequest>(&mut reader, MAX_HANDSHAKE_FRAME);
        let attached = match first {
            // Stopping needs no client and no session: the workspace claim
            // makes a live server refuse every replacement of the same
            // build, so `uze terminal stop` failing to be heard by one
            // nobody attached to left no way back in but a manual `kill`.
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
                seating,
            })) if version == PROTOCOL_VERSION => {
                let client = self
                    .next_client
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let drawn = (columns > 0 && rows > 0)
                    .then(|| (within_pane_bounds(columns), within_pane_bounds(rows)));
                let mut selection = Selection::default();
                let mut sized_at_creation = false;
                match seating {
                    // Nothing to say about where to land: the session's own
                    // selection answers.
                    Seating::WhereItLeftOff => {}
                    // A place, not a request. A seat with no space is a
                    // workspace this client has nothing to add to, so it
                    // lands where the session already is.
                    Seating::At(seat) => {
                        selection.space = self
                            .session
                            .lock()
                            .expect("session poisoned")
                            .space_for(&seat);
                    }
                    Seating::Open(seat) => {
                        match self.ensure_space(&seat, drawn.unwrap_or(PLACEHOLDER_PANE_SIZE)) {
                            Ok(OpenedSpace::Existing(space)) => selection.space = Some(space),
                            Ok(OpenedSpace::Created(NewSpace { space, .. })) => {
                                selection.space = Some(space);
                                sized_at_creation = true;
                            }
                            Err(error) => {
                                events.reply(ClientEvent::Error {
                                    message: format!(
                                        "could not open a space at {}: {error}",
                                        seat.root.display()
                                    ),
                                });
                            }
                        }
                    }
                }
                self.clients.lock().expect("clients poisoned").push(Client {
                    id: client,
                    events: Arc::clone(&events),
                    selection,
                });
                if let Some((columns, rows)) = drawn
                    && !sized_at_creation
                {
                    self.resize_pane(self.selected_pane_of(client), columns, rows);
                }
                self.broadcast_snapshot();
                // Said once, to the first client that arrives. The runtime
                // starts before anyone is watching, so this waits rather
                // than going to a log nobody turned on — which is how an
                // operator once watched every space disappear with no
                // sentence anywhere.
                if let Some(moved) = self.set_aside.lock().expect("set-aside poisoned").take() {
                    events.reply(ClientEvent::WorkspaceSetAside {
                        kept_at: moved.path,
                        reason: moved.reason,
                    });
                }
                Some(client)
            }
            Ok(Some(ClientRequest::Attach { .. })) => {
                events.reply(ClientEvent::Error {
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
                    events.reply(ClientEvent::Detached);
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
                    env,
                } => {
                    // Refused before anything is created: a tab that exists
                    // without the launch it was asked for is worse than no
                    // tab, and a shell carrying a launch environment would
                    // report as an agent it is not.
                    let launch = match crate::launch::validate(command, env) {
                        Ok(launch) => launch,
                        Err(refusal) => {
                            events.reply(ClientEvent::Error {
                                message: refusal.to_string(),
                            });
                            continue;
                        }
                    };
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
                    if self.spawn_pane(pane, launch).is_err() {
                        events.reply(ClientEvent::Error {
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
                            self.stop_runtimes(&panes);
                            self.broadcast_session();
                        }
                        None => {
                            events.reply(ClientEvent::Error {
                                // An agent's shells go with it, so what is
                                // refused is a close that would empty the
                                // space — not always a single tab.
                                message: "cannot close a space's last tabs".into(),
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
                ClientRequest::ReorderSpace { space, before } => {
                    let changed = self
                        .session
                        .lock()
                        .expect("session poisoned")
                        .reorder_space(space, before);
                    if changed {
                        self.broadcast_session();
                    }
                }
                ClientRequest::CreateSpace {
                    label,
                    seat,
                    columns,
                    rows,
                } => {
                    // Always a new space, even over a directory another
                    // space already holds (see `Session::create_space`):
                    // the prompt asked for a space, and one repository is
                    // routinely worth two — one per branch, one per thing
                    // being tried. `ensure_space` is the other question.
                    let root = seat.root.clone();
                    let (created, named) = {
                        let mut session = self.session.lock().expect("session poisoned");
                        let created = session.create_space(
                            label,
                            seat,
                            within_pane_bounds(columns),
                            within_pane_bounds(rows),
                        );
                        let named = session
                            .space(created.space)
                            .map(|space| space.label.clone())
                            .unwrap_or_default();
                        (created, named)
                    };
                    // The one record of a space appearing, with the name it
                    // ended up with: a repeated label is numbered here, and
                    // "where did `project 2` come from" is a question only
                    // this line can answer after the fact.
                    tracing::info!(root = %root.display(), label = %named, "a space was created");
                    self.spawn_new_space(client, created, &events);
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
                ClientRequest::CloseSpace {
                    space,
                    replacement,
                    columns,
                    rows,
                } => {
                    let removed = self.session.lock().expect("session poisoned").remove_space(
                        space,
                        replacement,
                        within_pane_bounds(columns),
                        within_pane_bounds(rows),
                    );
                    let Some(removed) = removed else {
                        continue;
                    };
                    self.stop_runtimes(&removed.panes);
                    if let Some(created) = removed.replacement {
                        self.spawn_new_space(client, created, &events);
                    }
                    self.broadcast_session();
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
                    events.reply(ClientEvent::Stopped);
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

    /// The space at `seat`, created — with its first shell pane, spawned at
    /// `size` — when none is.
    fn ensure_space(
        &self,
        seat: &SpaceSeat,
        (columns, rows): (u16, u16),
    ) -> Result<OpenedSpace, RuntimeError> {
        let opened =
            self.session
                .lock()
                .expect("session poisoned")
                .open_space(seat.clone(), columns, rows);
        if let OpenedSpace::Created(NewSpace { pane, .. }) = opened {
            self.spawn_pane(pane, Launch::Shell)?;
        }
        Ok(opened)
    }

    /// Starts a space a client just brought into being and puts that client
    /// in it.
    fn spawn_new_space(&self, client: u64, created: NewSpace, events: &Outbox) {
        if self.spawn_pane(created.pane, Launch::Shell).is_err() {
            events.reply(ClientEvent::Error {
                message: "could not create terminal pane".into(),
            });
        }
        self.update_selection(client, |selection| selection.space = Some(created.space));
    }

    fn stop_runtimes(&self, panes: &[PaneId]) {
        let mut runtimes = self.panes.lock().expect("panes poisoned");
        for pane in panes {
            if let Some(runtime) = runtimes.remove(pane) {
                runtime.stop();
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
        self.view_of(client).selected_tab().pane.id
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

    fn spawn_pane(&self, pane_id: PaneId, launch: Launch) -> Result<(), RuntimeError> {
        let pane = self
            .session
            .lock()
            .expect("session poisoned")
            .pane(pane_id)
            .cloned()
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
            launch,
            Arc::clone(&self.palette),
        )?;
        {
            let mut session = self.session.lock().expect("session poisoned");
            // The tab reports the launch the server made, from the one
            // place that makes it: a respawn as a plain shell clears it.
            session.record_launch(pane_id, runtime.launch.env().to_vec());
            // Best-effort: label the sidebar tree with the real shell name
            // immediately instead of leaving the "shell" placeholder until
            // the next status tick.
            if let Some((cwd, process)) = runtime.foreground_status() {
                session.update_pane_status(pane_id, cwd, process);
            }
        }
        self.panes
            .lock()
            .expect("panes poisoned")
            .insert(pane_id, Arc::new(runtime));
        // The reader is live before the pane is registered, and damage for
        // a pane the broadcaster cannot find yet is dropped. A program that
        // prints once and then waits — a harness's banner, then its prompt —
        // would never be drawn, so the pane is flushed once it can be found.
        let _ = self.damage.send(pane_id);
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
            if self.spawn_pane(pane, Launch::Shell).is_ok() {
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
            .retain(|client| client.events.offer(ClientEvent::Damage(damage.clone())));
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
                client.events.offer(ClientEvent::SessionUpdated {
                    session: view_for(&session, &client.selection),
                })
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
    /// The `Snapshot` goes out first: it is what tells a client to forget
    /// the panes it has, and the repaints that follow are what give it the
    /// new ones. They are ordinary `Damage` frames naming every cell, which
    /// is what a client already applies to a pane it has never heard of
    /// (and what a resize already sends).
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
                client.events.offer(ClientEvent::Snapshot {
                    session: view_for(&session, &client.selection),
                })
            });
        for pane in panes {
            let repaint = whole_pane(pane.snapshot_and_remember());
            self.clients
                .lock()
                .expect("clients poisoned")
                .retain(|client| client.events.offer(ClientEvent::Damage(repaint.clone())));
        }
    }

    /// Sends the whole workspace to every client that fell behind and has
    /// since drained what it was sent. Built from `snapshot`, not
    /// `snapshot_and_remember`: the damage baseline is shared by every
    /// client, and resetting it for one would cost the others a change.
    fn resync_stale_clients(&self) {
        let stale = self
            .clients
            .lock()
            .expect("clients poisoned")
            .iter()
            .any(|client| client.events.awaits_resync());
        if !stale {
            return;
        }
        let session = self.session.lock().expect("session poisoned").clone();
        let repaints: Vec<PaneDamage> = self
            .panes
            .lock()
            .expect("panes poisoned")
            .values()
            .map(|pane| whole_pane(pane.snapshot()))
            .collect();
        self.clients
            .lock()
            .expect("clients poisoned")
            .retain(|client| {
                !client.events.awaits_resync()
                    || client
                        .events
                        .resync(view_for(&session, &client.selection), &repaints)
            });
    }

    /// Ends every pane, and returns once each has been reaped: the server
    /// exits right after, and a reaper still running when it does would
    /// leave the pane's process group behind. All at once, so shutdown
    /// waits for the slowest pane rather than for their sum.
    fn stop_panes(&self) {
        let reapers: Vec<_> = self
            .panes
            .lock()
            .expect("panes poisoned")
            .values()
            .map(|pane| pane.stop())
            .collect();
        for reaper in reapers {
            let _ = reaper.join();
        }
    }

    /// Takes the server down: no new work, no live panes, and one
    /// connection of its own so [`accept_connections`] wakes from `accept`
    /// and reads the flag instead of blocking until somebody happens to
    /// attach.
    fn shut_down(&self) {
        *self.stopped.lock().expect("stop state poisoned") = true;
        self.stop_panes();
        let _ = UnixStream::connect(&self.socket);
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
            server.resync_stale_clients();
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
/// ownership and mode [`socket_path`] demanded of it in the first place — a
/// cleaner that took the socket usually took the directory too.
fn restore_endpoint_directory(socket: &Path) -> io::Result<()> {
    let Some(directory) = socket.parent() else {
        return Ok(());
    };
    fs::create_dir_all(directory)?;
    private_directory(directory, current_uid())
}

/// The process group `pid` leads, when it leads one of its own: a pane's
/// program is started in a session of its own, so its group is its pid.
/// `None` for anything else — above all this process's own group, which a
/// group signal must never reach.
fn own_process_group(pid: u32) -> Option<libc::pid_t> {
    let pid = libc::pid_t::try_from(pid).ok().filter(|pid| *pid > 1)?;
    // SAFETY: `getpgid` reads the group of a positive pid and touches no
    // memory of ours; `getpgrp` takes no arguments and cannot fail.
    let (group, ours) = unsafe { (libc::getpgid(pid), libc::getpgrp()) };
    (group == pid && group != ours).then_some(group)
}

/// The real user id of this process.
fn current_uid() -> libc::uid_t {
    // SAFETY: `getuid` takes no arguments, cannot fail, and touches no
    // memory of ours.
    unsafe { libc::getuid() }
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
/// seconds after login, taking the socket out from under a live server. The
/// server never notices — it holds an open listener on an unlinked inode —
/// and a client finding no socket would start a second one. The
/// [`WorkspaceLock`] tells that client a server is alive, and stops a
/// second server from restoring the same workspace; this is the other half:
/// the original reappears where clients look for it.
///
/// Reclaiming rather than yielding is safe *because* of that lock. This
/// process holds it, so anything now sitting at the path is not another
/// server of this workspace.
///
/// The stop flag is held across the whole check-and-rebind, not merely
/// read at the top. Reading it and then rebinding races the teardown at
/// the end of [`serve`]: a shutdown landing between the two leaves a server
/// on its way out rebinding a socket nothing will ever remove, and an
/// endpoint nobody serves. `serve` takes the same lock before it
/// clears the endpoint, which makes "rebound, then cleared" and "stopped,
/// so never rebound" the only two orderings there are.
fn spawn_endpoint_watch(server: Arc<Server>) {
    thread::spawn(move || {
        let mut bound = socket_identity(&server.socket);
        loop {
            thread::sleep(STATUS_PROBE_INTERVAL);
            let stopped = server.stopped.lock().expect("stop state poisoned");
            if *stopped {
                break;
            }
            if socket_identity(&server.socket) == bound {
                continue;
            }
            match restore_endpoint_directory(&server.socket)
                .map_err(RuntimeError::from)
                .and_then(|()| bind_endpoint(&server.socket))
            {
                Ok(listener) => {
                    tracing::warn!(
                        socket = %server.socket.display(),
                        "the terminal endpoint vanished under a live server; rebound it"
                    );
                    bound = socket_identity(&server.socket);
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
    /// Shared with the thread that reaps it once the pane is stopped.
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    terminal: Arc<Mutex<Term<ReplySink>>>,
    /// What this pane was spawned as — kept so a workspace restart can
    /// respawn the same launch in the same tab (see [`Server::persist`]),
    /// and so a finished program can be told from a shell.
    launch: Launch,
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
        launch: Launch,
        palette: Arc<Mutex<Palette>>,
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
        let mut command = match launch.argv().split_first() {
            Some((program, args)) => {
                let mut builder = CommandBuilder::new(program);
                builder.args(args);
                builder
            }
            None => CommandBuilder::new(env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())),
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
        for inherited in crate::launch::STAMPED_VARIABLES {
            command.env_remove(inherited);
        }
        for (name, value) in launch.env() {
            command.env(name, value);
        }
        // What tells a `uze` started inside this pane that it is inside one,
        // so it opens a space here instead of a client within a client.
        command.env(crate::launch::PANE_VARIABLE, id.0.to_string());
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
            child: Arc::new(Mutex::new(child)),
            terminal,
            launch,
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
    /// Ends the pane's process and reaps it, on a thread of its own.
    ///
    /// `kill` is a SIGHUP with a grace period of up to a fifth of a second
    /// and then a SIGKILL nobody waits on: done inline it held the request
    /// that closed the tab for that long, and left every pane whose program
    /// outlived the grace period a zombie for the life of the server.
    fn stop(&self) -> thread::JoinHandle<()> {
        let child = Arc::clone(&self.child);
        thread::spawn(move || {
            let mut child = child.lock().expect("child poisoned");
            let group = child.process_id().and_then(own_process_group);
            // Waited on whether or not the signal landed: a program that
            // already exited is exactly the zombie this is here to reap.
            let _ = child.kill();
            // What the leader started and left behind: a harness's workers
            // that ignore the hangup would otherwise outlive the pane, and
            // hold its terminal open so its reader never ends either.
            if let Some(group) = group {
                // SAFETY: `group` is a positive process-group id that is
                // the pane's own and not this process's (`own_process_group`),
                // so the negation addresses exactly that group.
                unsafe { libc::kill(-group, libc::SIGKILL) };
            }
            let _ = child.wait();
        })
    }

    fn finished_agent(&self) -> bool {
        matches!(self.launch, Launch::Program { .. })
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
            whole_pane(current.clone()).changed
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
///
/// The name is accepted only from the process the shim stamped it on.
/// `UZE_SHIM_NAME` is an ordinary environment variable: every child of a
/// shimmed agent inherits it, so a shell running *under* one would
/// otherwise answer with its ancestor's identity. `UZE_SHIM_PID` carries
/// the pid the stamp was made for — the shim `exec`s, so that pid is the
/// agent's own — and an inherited pair no longer names the process it is
/// read from.
fn shim_launched_name(pgid: libc::pid_t) -> Option<String> {
    let stamped: libc::pid_t =
        process_probe::environment_value_of(pgid, crate::launch::SHIM_PID_VARIABLE)?
            .trim()
            .parse()
            .ok()?;
    if stamped != pgid {
        return None;
    }
    process_probe::environment_value_of(pgid, crate::launch::SHIM_NAME_VARIABLE)
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
///
/// "No more" is the channel hanging up, which happens once the last
/// [`Outbox`] is dropped — by the handler that served this client, and by
/// [`Server::clients`]. That is why this is handed a [`Backlog`] and not
/// the outbox itself: see the note on `Backlog`.
fn forward_events(mut socket: UnixStream, events: &mpsc::Receiver<ClientEvent>, backlog: &Backlog) {
    while let Ok(event) = events.recv() {
        backlog.delivered();
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
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
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
/// realistic multi-tab session, which is what made attaching a client
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
        ANSWERS_WITHIN, Arrival, Launch, Listener, MAX_FRAME, MAX_PANE_DIMENSION, MAX_SOCKET_PATH,
        Outbox, PaneRuntime, PersistedWorkspace, ReplySink, RuntimeError, Selection, Server,
        WORKSPACE_SCHEMA_VERSION, WorkspaceLock, arrival, bind_endpoint, forward_events,
        held_by_a_server, identify, identity_of, listener_at, load_persisted_workspace_at,
        persisted_state_path, read_event, read_message, relaunch_command_for_process, retire,
        send_request, serves_this_build, signalable, snapshot, socket_path, view_for,
        workspace_is_claimed, workspace_lock_path, write_atomically, write_message,
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
    use crate::state::{PLACEHOLDER_PANE_SIZE, SpaceSeed, TabSeed};
    use crate::{MouseMode, PaneId, TerminalColor};
    use crate::{Session, SpaceId, TabId};
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
        let mut session = Session::new(seat_at(Path::new("/tmp/a")), 80, 24);
        let first_space = session.workspace.selected_space;
        session.create_space(
            Some("b".into()),
            crate::SpaceSeat {
                root: "/tmp/b".into(),
            },
            80,
            24,
        );
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

    /// One workspace, one endpoint, whatever each terminal's environment
    /// says. `XDG_RUNTIME_DIR` and `TMPDIR` are set per session and can
    /// differ between two terminals of one login — and then the two
    /// computed two different sockets for one `UZE_HOME`. The second
    /// found nothing listening at a path the first had never bound, while
    /// the claim beside the workspace told it a server was alive, so it
    /// connected to nothing and answered `No such file or directory`.
    #[test]
    fn two_terminals_that_disagree_about_the_environment_share_one_endpoint() {
        let home = uze_testkit::temp::socket_scratch("endpoint-home");
        let elsewhere = uze_testkit::temp::socket_scratch("endpoint-xdg");
        let one = {
            let mut env = uze_testkit::env::scope();
            env.set("UZE_HOME", &home);
            env.set("XDG_RUNTIME_DIR", &elsewhere);
            socket_path().expect("an endpoint can always be named")
        };
        let other = {
            let mut env = uze_testkit::env::scope();
            env.set("UZE_HOME", &home);
            env.remove("XDG_RUNTIME_DIR");
            socket_path().expect("an endpoint can always be named")
        };

        assert_eq!(one, other, "the workspace decides, not the session");
        assert!(
            one.starts_with(&home),
            "and it sits beside the workspace it serves, where no cleaner \
             reaches it without taking the workspace too: {}",
            one.display()
        );

        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }

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
        // Both of the candidates that come before `/tmp`: a home is
        // wherever the operator put it, and so is somebody else's
        // runtime directory.
        env.set("UZE_HOME", &deep);
        env.set("XDG_RUNTIME_DIR", &deep);

        let socket = socket_path().expect("a too-long runtime directory is not fatal");
        assert!(
            socket.as_os_str().len() <= MAX_SOCKET_PATH,
            "the chosen socket path must fit sun_path, got {} bytes: {}",
            socket.as_os_str().len(),
            socket.display()
        );
        assert!(
            !socket.starts_with(&deep),
            "the directory that could not hold the socket must not have been chosen"
        );
        // Binding is the only real proof: the length rule exists to make this
        // call succeed, so the test performs it rather than trusting the
        // arithmetic.
        let _ = std::fs::remove_file(&socket);
        let listener = std::os::unix::net::UnixListener::bind(&socket)
            .expect("the chosen path must actually bind");
        drop(listener);
        let _ = std::fs::remove_file(&socket);
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
        // The endpoint follows `UZE_HOME`, and `stop` ends whatever serves
        // it: without a scratch home this test would stop the developer's
        // own session.
        env.set("UZE_HOME", &scratch);
        env.set("XDG_RUNTIME_DIR", &scratch);

        let socket = socket_path().expect("an endpoint can always be named");
        let _ = std::fs::remove_file(&socket);
        assert!(
            super::stop().is_ok(),
            "no socket at all is nothing to stop, not a failure"
        );

        // The shape a cleaner leaves: the file is there, the server is not.
        leave_a_stale_socket(&socket);
        assert!(
            super::stop().is_ok(),
            "a socket nobody answers is nothing to stop either"
        );

        let _ = std::fs::remove_file(&socket);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The kernel names whoever listens on a socket, and the process table
    /// says what that process runs: this very executable, a `uze` of
    /// another build, or something nobody can vouch for as `uze` at all.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn the_listener_is_told_apart_by_what_it_runs() {
        let scratch = uze_testkit::temp::socket_scratch("identify");
        std::fs::create_dir_all(&scratch).unwrap();
        let socket = scratch.join("test.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert_eq!(
            listener_at(&socket),
            Listener::ThisBuild(std::process::id()),
            "a listener running this very executable is this build"
        );
        drop(listener);

        let stranger = ReadyProcess::spawn(Path::new("/bin/sh"));
        assert_eq!(identify(stranger.pid()), Listener::Unrecognized);
        stranger.finish();

        let uze_home = scratch.join("home");
        std::fs::create_dir_all(&uze_home).unwrap();
        let another_build =
            ClaimHolder::spawn_as(&another_build_of_this_binary(&scratch), &uze_home);
        let pid = another_build.pid();
        assert_eq!(identify(pid), Listener::AnotherBuild(pid));
        another_build.release();

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A socket file nobody listens on names nobody, so there is nobody to
    /// signal.
    #[test]
    fn a_socket_nobody_listens_on_names_nobody() {
        let scratch = uze_testkit::temp::socket_scratch("nobody");
        std::fs::create_dir_all(&scratch).unwrap();
        let socket = scratch.join("test.sock");
        leave_a_stale_socket(&socket);

        assert_eq!(listener_at(&socket), Listener::Nobody);

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The server holding the workspace claim binds over whatever it finds
    /// at the socket path: a crashed server's leftover file is not a peer,
    /// and refusing to bind over it left the workspace unreachable.
    #[test]
    fn a_stale_socket_is_reclaimed_by_the_server_that_binds() {
        let scratch = uze_testkit::temp::socket_scratch("reclaim");
        std::fs::create_dir_all(&scratch).unwrap();
        let socket = scratch.join("test.sock");
        leave_a_stale_socket(&socket);
        assert!(std::os::unix::net::UnixStream::connect(&socket).is_err());

        let listener = bind_endpoint(&socket).expect("a stale socket is bound over");
        assert!(
            std::os::unix::net::UnixStream::connect(&socket).is_ok(),
            "the reclaimed endpoint answers"
        );

        drop(listener);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A server of another build — a `make install` over a running one — is
    /// ended, and not merely abandoned: it holds the workspace claim, and a
    /// fresh server cannot restore the workspace until it lets go.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_server_of_another_build_is_retired_and_lets_go_of_the_workspace() {
        let scratch = uze_testkit::temp::socket_scratch("retire");
        let uze_home = scratch.join("home");
        std::fs::create_dir_all(&uze_home).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);

        let mut old = ClaimHolder::spawn_as(&another_build_of_this_binary(&scratch), &uze_home);
        assert!(workspace_is_claimed());

        retire(old.pid(), &scratch.join("test.sock"));

        assert!(
            !workspace_is_claimed(),
            "the retired server let go of the workspace before retire returned"
        );
        assert!(!old.process.wait().unwrap().success(), "it was ended");

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A server holding the workspace at an endpoint this build does not
    /// name — what every change to the endpoint's own rules leaves behind
    /// — is still what `stop` stops. It used to look at the new endpoint,
    /// find nothing, and report success while the workspace stayed shut:
    /// the operator was told there was nothing to stop, could not open
    /// uze, and restarting the machine was the only way out.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_server_answering_at_no_endpoint_this_build_names_is_still_stopped() {
        let scratch = uze_testkit::temp::socket_scratch("stop-claimed");
        let uze_home = scratch.join("home");
        std::fs::create_dir_all(&uze_home).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);

        let mut old = ClaimHolder::spawn_as(&another_build_of_this_binary(&scratch), &uze_home);
        assert!(workspace_is_claimed());
        assert_eq!(
            super::claim_holder(),
            Some(old.pid()),
            "the claim names who holds it, which `flock` cannot"
        );
        // It bound no endpoint at all, which is what an endpoint named by
        // other rules looks like from here.
        assert!(!socket_path().unwrap().exists());

        assert!(super::stop().is_ok(), "stopping it is not a failure");

        assert!(
            !workspace_is_claimed(),
            "and the workspace is free for the next server"
        );
        assert!(!old.process.wait().unwrap().success(), "it was ended");

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The server every machine already has: one from a release that
    /// predates the claim's record of who holds it. It answers at an
    /// endpoint this build does not compute and names nobody, so nothing
    /// here can end it — and saying "nothing to stop" is what sent an
    /// operator to restart their machine. It is said instead.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_claim_this_build_cannot_name_is_reported_rather_than_called_stopped() {
        let scratch = uze_testkit::temp::socket_scratch("stop-unnamed");
        let uze_home = scratch.join("home");
        std::fs::create_dir_all(&uze_home).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);

        let holder = ClaimHolder::spawn_as(&another_build_of_this_binary(&scratch), &uze_home);
        // What a server older than `record_claimant` leaves: the lock held,
        // and nothing written in it.
        std::fs::write(super::workspace_lock_path(), b"").unwrap();
        assert!(workspace_is_claimed());
        assert_eq!(super::claim_holder(), None);

        let refused = super::stop().expect_err("a claim nobody can name is not a clean stop");
        let said = refused.to_string();
        assert!(
            said.contains("serving this workspace") && said.contains("pgrep"),
            "the message names the situation and how to end it: {said}"
        );
        assert!(
            workspace_is_claimed(),
            "and nothing was signalled on a claim this build cannot vouch for"
        );

        holder.release();
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
        let (damage, damage_events) = std::sync::mpsc::channel();
        let pane = PaneRuntime::spawn(
            PaneId(9),
            PathBuf::from("/tmp"),
            80,
            24,
            damage,
            Launch::Shell,
            Arc::new(Mutex::new(Palette::default())),
        )
        .unwrap();
        // Baseline covers every cell — a fresh client has nothing to diff against.
        let baseline = pane.damage_since_last();
        assert_eq!(baseline.changed.len(), 80 * 24);

        pane.write(b"printf uze-diff-probe\\r");
        let probe = std::iter::from_fn(|| damage_events.recv_timeout(Duration::from_secs(10)).ok())
            .map(|_| pane.damage_since_last())
            .find(|damage| {
                damage
                    .changed
                    .iter()
                    .any(|(_, _, cell)| cell.character == 'u')
            });
        pane.stop();
        let probe = probe.expect("the echoed command never reached the grid");
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
        let (damage, damage_events) = std::sync::mpsc::channel();
        let pane = PaneRuntime::spawn(
            PaneId(7),
            PathBuf::from("/tmp"),
            80,
            24,
            damage,
            Launch::Shell,
            Arc::new(Mutex::new(Palette::default())),
        )
        .unwrap();
        pane.write(b"printf uze-runtime-live\\r");
        let rendered =
            std::iter::from_fn(|| damage_events.recv_timeout(Duration::from_secs(10)).ok()).any(
                |_| {
                    pane.snapshot()
                        .cells
                        .into_iter()
                        .map(|cell| cell.character)
                        .collect::<String>()
                        .contains("uze-runtime-live")
                },
            );
        pane.stop();
        assert!(rendered, "the printed line never reached the grid");
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
            Launch::Shell,
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
        uze_testkit::process::install_executable(
            &versioned_binary,
            &std::fs::read("/bin/sleep").unwrap(),
        );

        let (damage, _damage_events) = std::sync::mpsc::channel();
        let pane = PaneRuntime::spawn(
            PaneId(13),
            PathBuf::from("/tmp"),
            80,
            24,
            damage,
            Launch::Program {
                argv: vec![
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
                ],
                env: Vec::new(),
            },
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

    /// What a client says about where it is deciding *where it lands*, and
    /// only [`Seating::Open`] may bring a space into being.
    ///
    /// This is the server half of what makes closing a space stick.
    /// Naming a seat on every attach reopened the space closed just
    /// before it — within a run, when the runtime went away and the
    /// client attached again, and across runs, where the launch directory
    /// remade a space the operator had removed and quit. Both are the
    /// same failure: a close that does not stay closed is
    /// indistinguishable from a close that did not work.
    #[test]
    fn only_asking_to_open_a_space_may_create_one() {
        let scratch = uze_testkit::temp::socket_scratch("attach-seating");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        let elsewhere = scratch.join("elsewhere");
        let runtime_dir = scratch.join("runtime");
        for directory in [&uze_home, &project, &elsewhere, &runtime_dir] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home)
            .set("XDG_RUNTIME_DIR", &runtime_dir);
        let (server, _damage) = Server::new(seat_at(&project), socket_path().unwrap()).unwrap();
        let server = Arc::new(server);

        let attach = |seating: crate::Seating| {
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
                    columns: 80,
                    rows: 24,
                    seating,
                },
            )
            .unwrap();
            let landed = std::iter::from_fn(|| read_event(&mut reader).unwrap())
                .find_map(|event| match event {
                    crate::ClientEvent::Snapshot { session } => {
                        Some(session.selected_space().root.clone())
                    }
                    _ => None,
                })
                .expect("the client attached");
            let _ = send_request(&mut writer, &crate::ClientRequest::Detach);
            let _ = serving.join();
            landed
        };
        let spaces = || {
            server
                .session
                .lock()
                .expect("session poisoned")
                .workspace
                .spaces
                .len()
        };

        assert_eq!(spaces(), 1, "the bootstrap space, and nothing else");

        let landed = attach(crate::Seating::At(seat_at(&elsewhere)));
        assert_eq!(spaces(), 1, "landing somewhere unopened created a space");
        assert_eq!(
            landed, project,
            "and the client lands where the session already was"
        );

        let landed = attach(crate::Seating::WhereItLeftOff);
        assert_eq!(spaces(), 1, "saying nothing created a space");
        assert_eq!(landed, project);

        let landed = attach(crate::Seating::At(seat_at(&project)));
        assert_eq!(spaces(), 1, "a seat that is already open opens nothing");
        assert_eq!(landed, project, "and is what the client lands on");

        let landed = attach(crate::Seating::Open(seat_at(&elsewhere)));
        assert_eq!(spaces(), 2, "asking to open a space did not open one");
        assert_eq!(landed, elsewhere, "and the client lands in it");
    }

    /// A tab is opened in the space the client selected last, so selecting
    /// a space and then asking for a tab of its own lands the tab there —
    /// the pair a click on a space whose shells all became agents sends.
    #[test]
    fn a_tab_asked_for_after_selecting_a_space_opens_in_that_space() {
        let scratch = uze_testkit::temp::socket_scratch("select-then-create");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        let elsewhere = scratch.join("elsewhere");
        let runtime_dir = scratch.join("runtime");
        for directory in [&uze_home, &project, &elsewhere, &runtime_dir] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home)
            .set("XDG_RUNTIME_DIR", &runtime_dir);
        let (server, _damage) = Server::new(seat_at(&project), socket_path().unwrap()).unwrap();
        let server = Arc::new(server);
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
                columns: 80,
                rows: 24,
                seating: crate::Seating::Open(seat_at(&elsewhere)),
            },
        )
        .unwrap();
        let first = std::iter::from_fn(|| read_event(&mut reader).unwrap())
            .find_map(|event| match event {
                crate::ClientEvent::Snapshot { session } => session
                    .workspace
                    .spaces
                    .iter()
                    .find(|space| space.root == project)
                    .map(|space| space.id),
                _ => None,
            })
            .expect("the client attached beside the project's space");

        for request in [
            crate::ClientRequest::SelectSpace { space: first },
            crate::ClientRequest::CreateTab {
                label: "shell 1".into(),
                agent: None,
                columns: 80,
                rows: 24,
                cwd: Some(project.clone()),
                command: None,
                env: Vec::new(),
            },
        ] {
            send_request(&mut writer, &request).unwrap();
        }
        let opened = std::iter::from_fn(|| read_event(&mut reader).unwrap())
            .find_map(|event| match event {
                crate::ClientEvent::SessionUpdated { session } => {
                    let tabs = |root: &Path| {
                        session
                            .workspace
                            .spaces
                            .iter()
                            .find(|space| space.root == root)
                            .map_or(0, |space| space.tabs.len())
                    };
                    (tabs(&project) + tabs(&elsewhere) == 3)
                        .then(|| (tabs(&project), tabs(&elsewhere)))
                }
                _ => None,
            })
            .expect("the tab was opened");
        let _ = send_request(&mut writer, &crate::ClientRequest::Detach);
        let _ = serving.join();

        assert_eq!(
            opened,
            (2, 1),
            "the tab opened in the space selected, not the one attached to"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    /// Stopping a pane ends what its program left behind in its process
    /// group, not only the program: a worker deaf to the hangup would
    /// otherwise outlive the pane. The worker holds the only writer of a
    /// FIFO, so its death is the FIFO hanging up — no polling for it.
    #[test]
    fn a_stopped_pane_takes_its_process_group_with_it() {
        let scratch = uze_testkit::temp::scratch("pane-group");
        std::fs::create_dir_all(&scratch).unwrap();
        let fifo = scratch.join("worker");
        let path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `path` is a NUL-terminated string that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);

        let (damage, damage_events) = std::sync::mpsc::channel();
        let pane = PaneRuntime::spawn(
            PaneId(8),
            PathBuf::from("/tmp"),
            80,
            24,
            damage,
            Launch::Program {
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    format!(
                        "trap '' HUP; sleep 300 > '{}' & echo worker-started; wait",
                        fifo.display()
                    ),
                ],
                env: Vec::new(),
            },
            Arc::new(Mutex::new(Palette::default())),
        )
        .unwrap();
        // Opening for reading waits for the worker to open its end.
        let reader = std::fs::File::open(&fifo).unwrap();
        let started = damage_events.iter().any(|_| {
            pane.snapshot()
                .cells
                .iter()
                .map(|cell| cell.character)
                .collect::<String>()
                .contains("worker-started")
        });
        assert!(started, "the worker never started");

        pane.stop().join().expect("the reaper finished");

        // A read that ends is the worker's end of the FIFO closing, which
        // only its death does. On a thread, so a survivor fails the test
        // instead of hanging it; not `poll`, which macOS does not answer
        // for a FIFO.
        let (ended, hung_up) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = reader;
            let _ = std::io::Read::read_to_end(&mut reader, &mut Vec::new());
            let _ = ended.send(());
        });
        assert!(
            hung_up.recv_timeout(Duration::from_secs(10)).is_ok(),
            "the worker outlived its pane"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A client that stops reading is not buffered for without limit: once
    /// its queue is full it is marked stale and sent nothing more, and once
    /// it has caught up it is sent the whole workspace again.
    #[test]
    fn a_client_that_stops_reading_is_bounded_and_resynchronized() {
        let scratch = uze_testkit::temp::socket_scratch("stalled-client");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        let runtime_dir = scratch.join("runtime");
        for directory in [&uze_home, &project, &runtime_dir] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home)
            .set("XDG_RUNTIME_DIR", &runtime_dir);
        let (server, _damage) = Server::new(seat_at(&project), socket_path().unwrap()).unwrap();
        let server = Arc::new(server);

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
                columns: 80,
                rows: 24,
                seating: crate::Seating::WhereItLeftOff,
            },
        )
        .unwrap();
        let outbox = || {
            let clients = server.clients.lock().expect("clients poisoned");
            Arc::clone(&clients.first().expect("the client attached").events)
        };
        let attached = std::iter::from_fn(|| read_event(&mut reader).unwrap())
            .any(|event| matches!(event, crate::ClientEvent::Snapshot { .. }));
        assert!(attached, "the client never attached");

        // Far more than its queue and its socket's buffer can hold. Damage,
        // not session updates: those persist the workspace on every call.
        let pane = server
            .session
            .lock()
            .expect("session poisoned")
            .selected_tab()
            .pane
            .id;
        for _ in 0..super::OUTBOX_CAPACITY * 64 {
            server.broadcast_pane_damage(pane);
        }
        let waiting = outbox()
            .backlog
            .pending
            .load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            waiting <= super::OUTBOX_CAPACITY,
            "{waiting} events queued for a client that reads nothing"
        );
        assert!(
            outbox()
                .backlog
                .stale
                .load(std::sync::atomic::Ordering::Relaxed)
        );

        let caught_up = std::iter::from_fn(|| read_event(&mut reader).unwrap())
            .any(|_| outbox().awaits_resync());
        assert!(caught_up, "the client never drained its queue");
        server.resync_stale_clients();
        let resynchronized = std::iter::from_fn(|| read_event(&mut reader).unwrap())
            .any(|event| matches!(event, crate::ClientEvent::Snapshot { .. }));
        assert!(
            resynchronized,
            "the stale client was never sent the workspace again"
        );
        assert!(
            !outbox()
                .backlog
                .stale
                .load(std::sync::atomic::Ordering::Relaxed)
        );

        let _ = send_request(&mut writer, &crate::ClientRequest::Detach);
        drop(writer);
        drop(reader);
        let _ = serving.join();
        server.stop_panes();
    }

    /// A stopped pane's process is reaped, not left a zombie for the life of
    /// the server: once the reaper is done, its pid names no process at all
    /// — a zombie would still answer `kill(pid, 0)`.
    #[test]
    fn a_stopped_pane_leaves_no_zombie() {
        let (damage, damage_events) = std::sync::mpsc::channel();
        let pane = PaneRuntime::spawn(
            PaneId(7),
            PathBuf::from("/tmp"),
            80,
            24,
            damage,
            // Deaf to the SIGHUP `kill` tries first, so it takes the SIGKILL
            // that nothing used to wait on.
            Launch::Program {
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "trap '' HUP; echo deaf-to-hangup; exec sleep 30".into(),
                ],
                env: Vec::new(),
            },
            Arc::new(Mutex::new(Palette::default())),
        )
        .unwrap();
        // Only once the trap is in place does the hangup go unheard; a
        // signal that beat it would kill the shell and prove nothing.
        let deaf = damage_events.iter().any(|_| {
            pane.snapshot()
                .cells
                .iter()
                .map(|cell| cell.character)
                .collect::<String>()
                .contains("deaf-to-hangup")
        });
        assert!(deaf, "the program never reported its trap");
        let pid = pane
            .child
            .lock()
            .expect("child poisoned")
            .process_id()
            .expect("a spawned child has a pid");

        pane.stop().join().expect("the reaper finished");

        // SAFETY: signal 0 only asks whether `pid` is addressable; nothing
        // is delivered, and `pid` is the positive id of our own child.
        let addressable = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
        assert!(!addressable, "pid {pid} survived as a zombie");
    }

    /// A program's first output can land before its pane is registered,
    /// and damage for a pane nobody can find is dropped. Registration is
    /// what flushes it, so a pane is delivered even when its program
    /// prints nothing more — here, nothing at all.
    #[test]
    fn a_spawned_pane_is_flushed_once_it_is_registered() {
        let scratch = uze_testkit::temp::socket_scratch("flushed-on-spawn");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        let runtime_dir = scratch.join("runtime");
        for directory in [&uze_home, &project, &runtime_dir] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home)
            .set("XDG_RUNTIME_DIR", &runtime_dir);

        let (server, damage) = Server::new(seat_at(&project), socket_path().unwrap()).unwrap();
        let pane = server
            .session
            .lock()
            .expect("session poisoned")
            .create_space(None, seat_at(&project), 80, 24)
            .pane;
        let silent = Launch::Program {
            argv: vec!["sleep".into(), "30".into()],
            env: Vec::new(),
        };
        server.spawn_pane(pane, silent).unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let flushed = std::iter::from_fn(|| {
            damage
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .ok()
        })
        .any(|notified| notified == pane);
        server.stop_panes();
        assert!(
            flushed,
            "the silent pane was never offered to the broadcaster"
        );
    }

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

        let socket = socket_path().unwrap();
        let (server, _damage) = Server::new(seat_at(&project), socket).unwrap();
        let server = Arc::new(server);
        // A second space, so closing the launch space leaves a survivor
        // rather than opening a replacement.
        let pane = server
            .session
            .lock()
            .expect("session poisoned")
            .create_space(
                Some("other".into()),
                crate::SpaceSeat {
                    root: other.clone(),
                },
                80,
                24,
            )
            .pane;
        server.spawn_pane(pane, Launch::Shell).unwrap();
        let launch = {
            let mut session = server.session.lock().expect("session poisoned");
            let launch = session
                .space_for(&seat_at(&project))
                .expect("the bootstrap space is rooted at the launch directory");
            let seat = crate::SpaceSeat {
                root: other.clone(),
            };
            assert!(
                session.remove_space(launch, seat, 80, 24).is_some(),
                "space closed"
            );
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
                columns: 80,
                rows: 24,
                seating: crate::Seating::WhereItLeftOff,
            },
        )
        .unwrap();
        let attached = loop {
            match read_event(&mut reader).unwrap() {
                Some(crate::ClientEvent::Snapshot { session }) => break session,
                Some(_) => {}
                None => panic!("the server hung up before attaching"),
            }
        };
        assert_eq!(
            attached.space_for(&seat_at(&project)),
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

        let socket = socket_path().unwrap();
        let (first, _damage) = Server::new(seat_at(&project), socket.clone()).unwrap();
        let agent_pane = first
            .session
            .lock()
            .expect("session poisoned")
            .create_space(
                Some("frontend".into()),
                crate::SpaceSeat {
                    root: project.clone(),
                },
                80,
                24,
            )
            .pane;
        first.spawn_pane(agent_pane, sleep_five()).unwrap();
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

        let (second, _damage2) = Server::new(seat_at(&project), socket).unwrap();
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
                .get(&tab.pane.id)
                .expect("restored tab's pane was actually spawned");
            assert_eq!(
                runtime.launch,
                sleep_five(),
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

        let socket = socket_path().unwrap();
        let (server, _damage) = Server::new(seat_at(&project), socket).unwrap();
        let pane = server
            .session
            .lock()
            .expect("session poisoned")
            .create_space(
                Some("agent".into()),
                crate::SpaceSeat {
                    root: project.clone(),
                },
                80,
                24,
            )
            .pane;
        server.spawn_pane(pane, exits_at_once(Vec::new())).unwrap();

        for _ in 0..40 {
            server.restore_finished_agent_panes();
            let restored = server
                .panes
                .lock()
                .expect("panes poisoned")
                .get(&pane)
                .is_some_and(|runtime| runtime.launch == Launch::Shell);
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
                .is_some_and(|runtime| runtime.launch == Launch::Shell),
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
    /// launch of its own), where someone then typed an agent
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

        let socket = socket_path().unwrap();
        let (first, _damage) = Server::new(seat_at(&project), socket.clone()).unwrap();
        let pane_id = first
            .session
            .lock()
            .expect("session poisoned")
            .selected_tab()
            .pane
            .id;
        first
            .session
            .lock()
            .expect("session poisoned")
            .update_pane_status(pane_id, project.clone(), "sleep".to_owned());
        first.persist();
        first.stop_panes();
        drop(first);

        let (second, _damage2) = Server::new(seat_at(&project), socket).unwrap();
        {
            let session = second.session.lock().expect("session poisoned");
            let tab = session.selected_tab();
            let panes = second.panes.lock().expect("panes poisoned");
            let runtime = panes
                .get(&tab.pane.id)
                .expect("restored tab's pane was actually spawned");
            assert_eq!(
                runtime.launch,
                Launch::Program {
                    argv: vec!["sleep".to_owned()],
                    env: Vec::new(),
                },
                "a process typed straight into a plain shell tab still relaunches on restore"
            );
        }
        second.stop_panes();

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The previous release wrote a kind per space. This build has no
    /// field for one, and `serde` would have dropped it without a word —
    /// which is why the version is read before the document.
    ///
    /// What that guard used to do about it was set the whole file aside,
    /// and on 2026-09-19 that cost an operator every space they had open
    /// over a difference of one field. The guard now climbs the rung
    /// instead: the kind is dropped deliberately, every other field is
    /// carried, and the spaces open.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_workspace_from_the_previous_release_is_carried_across_rather_than_set_aside() {
        let scratch = uze_testkit::temp::socket_scratch("persprev");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&uze_home).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);

        let path = persisted_state_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Version 1 as the previous release wrote it: a kind per space.
        // The space carries a tab, because a space with none is not a
        // workspace anybody lost — `Session::restore` drops those, and the
        // claim being proven here is that a space with work in it survives.
        let kept = project.join("kept");
        std::fs::create_dir_all(&kept).unwrap();
        std::fs::write(
            &path,
            format!(
                r#"{{"spaces":[{{"label":"demo","root":"{root}","kind":"worktree","tabs":[
                     {{"label":"shell","cwd":"{root}","agent":null,"launch":"Shell"}}]}}]}}"#,
                root = kept.display()
            )
            .as_bytes(),
        )
        .unwrap();

        let (restored, _) = load_persisted_workspace_at(&path);
        let restored = restored.expect("the previous release's spaces survive");
        assert_eq!(
            restored.spaces.len(),
            1,
            "the space is carried across, not thrown away"
        );
        assert_eq!(restored.spaces[0].label, "demo");
        assert!(
            path.exists(),
            "and the workspace keeps its own name: nothing was set aside"
        );
        let beside: Vec<String> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("unreadable"))
            .collect();
        assert!(beside.is_empty(), "nothing was set aside: {beside:?}");

        let socket = socket_path().unwrap();
        let (server, _damage) = Server::new(seat_at(&project), socket).expect("server");
        {
            let session = server.session.lock().expect("session poisoned");
            assert_eq!(
                session.workspace.spaces[0].root, kept,
                "the server opens on the workspace it was left, not on the seat"
            );
        }
        server.stop_panes();

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

        let socket = socket_path().unwrap();
        let (server, _damage) = Server::new(seat_at(&project), socket).expect("server");
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
            schema_version: WORKSPACE_SCHEMA_VERSION,
            spaces: vec![SpaceSeed {
                label: "space 1".into(),
                root: project.clone(),
                tabs: vec![TabSeed {
                    label: "shell".into(),
                    cwd: project.clone(),
                    agent: None,
                    launch: Launch::Program {
                        argv: vec!["definitely-not-a-real-binary-xyz".to_owned()],
                        env: Vec::new(),
                    },
                }],
            }],
        };
        std::fs::write(&path, serde_json::to_vec(&stale).unwrap()).unwrap();

        let socket = socket_path().unwrap();
        let (server, _damage) = Server::new(seat_at(&project), socket)
            .expect("a stale persisted command must not fail server startup");
        let session = server.session.lock().expect("session poisoned");
        let tab = session.selected_tab();
        let panes = server.panes.lock().expect("panes poisoned");
        assert!(
            panes.contains_key(&tab.pane.id),
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

        let socket = socket_path().unwrap();
        let (server, _damage) = Server::new(seat_at(&project), socket).unwrap();
        let server = Arc::new(server);
        let pane = server
            .session
            .lock()
            .expect("session poisoned")
            .selected_tab()
            .pane
            .id;

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
                columns: 0,
                rows: 0,
                seating: crate::Seating::WhereItLeftOff,
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
            Launch::Shell,
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

    fn read_when_written(path: &Path) -> String {
        for _ in 0..500 {
            if let Ok(content) = std::fs::read_to_string(path) {
                return content;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("{} was never written", path.display())
    }

    /// The command a launch runs to report what its environment carries:
    /// one file, one value, then exit. The value lands on a name of its own
    /// and is renamed onto the reported path: a redirection creates the file
    /// before the shell writes into it, so a reader watching for the path to
    /// appear would otherwise be free to read the empty half of that window.
    fn report_variable(variable: &str, into: &Path) -> Vec<String> {
        let partial = into.with_extension("partial");
        vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            format!(
                "printf %s \"${{{variable}-unset}}\" > \"{partial}\" && mv \"{partial}\" \"{reported}\"",
                partial = partial.display(),
                reported = into.display()
            ),
        ]
    }

    fn seat_at(root: &Path) -> crate::SpaceSeat {
        crate::SpaceSeat {
            root: root.to_path_buf(),
        }
    }

    fn sleep_five() -> Launch {
        Launch::Program {
            argv: vec!["sleep".to_owned(), "5".to_owned()],
            env: Vec::new(),
        }
    }

    /// `/bin/sh -c 'exit 0'`, not `/bin/true`: macOS keeps `true` in
    /// `/usr/bin` and has no `/bin/true` at all. `/bin/sh` is the one path
    /// POSIX actually promises, and what this needs is any process that
    /// exits at once.
    fn exits_at_once(env: Vec<(String, String)>) -> Launch {
        Launch::Program {
            argv: vec!["/bin/sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()],
            env,
        }
    }

    fn stamp(id: &str) -> Vec<(String, String)> {
        vec![(
            crate::launch::AGENT_IDENTITY_VARIABLE.to_owned(),
            id.to_owned(),
        )]
    }

    #[test]
    fn a_launch_environment_reaches_the_first_process() {
        let scratch = uze_testkit::temp::scratch("launchenv");
        let report = scratch.join("report");
        let (damage, _damage_events) = std::sync::mpsc::channel();
        let pane = PaneRuntime::spawn(
            PaneId(31),
            scratch.clone(),
            80,
            24,
            damage,
            Launch::Program {
                argv: report_variable(crate::launch::AGENT_IDENTITY_VARIABLE, &report),
                env: stamp("agent-31"),
            },
            Arc::new(Mutex::new(Palette::default())),
        )
        .unwrap();
        assert_eq!(read_when_written(&report), "agent-31");
        assert_eq!(pane.launch.env(), stamp("agent-31"));
        pane.stop();
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A pane carries only what its own launch put there: a server that was
    /// itself started inside an agent's pane does not hand that agent's
    /// identity to the panes it opens, whatever they run.
    #[test]
    fn a_pane_does_not_inherit_the_servers_agent_identity() {
        let scratch = uze_testkit::temp::scratch("inheritagent");
        let report = scratch.join("report");
        let mut env = uze_testkit::env::scope();
        env.set(crate::launch::AGENT_IDENTITY_VARIABLE, "the-servers-own");
        let (damage, _damage_events) = std::sync::mpsc::channel();
        let pane = PaneRuntime::spawn(
            PaneId(32),
            scratch.clone(),
            80,
            24,
            damage,
            Launch::Program {
                argv: report_variable(crate::launch::AGENT_IDENTITY_VARIABLE, &report),
                env: Vec::new(),
            },
            Arc::new(Mutex::new(Palette::default())),
        )
        .unwrap();
        assert_eq!(read_when_written(&report), "unset");
        pane.stop();
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_launch_environment_survives_a_restart() {
        let scratch = uze_testkit::temp::socket_scratch("envrestart");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        let report = scratch.join("report");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&uze_home).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);

        let path = persisted_state_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let persisted = PersistedWorkspace {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            spaces: vec![SpaceSeed {
                label: "space 1".into(),
                root: project.clone(),
                tabs: vec![TabSeed {
                    label: "agent 1".into(),
                    cwd: project.clone(),
                    agent: None,
                    launch: Launch::Program {
                        argv: report_variable(crate::launch::AGENT_IDENTITY_VARIABLE, &report),
                        env: stamp("agent-restarted"),
                    },
                }],
            }],
        };
        std::fs::write(&path, serde_json::to_vec(&persisted).unwrap()).unwrap();

        let socket = socket_path().unwrap();
        let (server, _damage) = Server::new(seat_at(&project), socket).unwrap();
        assert_eq!(read_when_written(&report), "agent-restarted");
        let session = server.session.lock().expect("session poisoned");
        let tab = session.selected_tab();
        assert_eq!(
            tab.env,
            stamp("agent-restarted"),
            "the restored tab reports the launch it was respawned with"
        );
        drop(session);
        server.stop_panes();
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_shell_respawn_carries_no_launch_environment() {
        let scratch = uze_testkit::temp::socket_scratch("envshell");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        let runtime_dir = scratch.join("runtime");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&uze_home).unwrap();
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home)
            .set("XDG_RUNTIME_DIR", &runtime_dir);

        let socket = socket_path().unwrap();
        let (server, _damage) = Server::new(seat_at(&project), socket).unwrap();
        let pane = server
            .session
            .lock()
            .expect("session poisoned")
            .create_space(
                Some("agent".into()),
                crate::SpaceSeat {
                    root: project.clone(),
                },
                80,
                24,
            )
            .pane;
        server
            .spawn_pane(pane, exits_at_once(stamp("agent-done")))
            .unwrap();
        let launched = server
            .session
            .lock()
            .expect("session poisoned")
            .selected_tab()
            .env
            .clone();
        assert_eq!(launched, stamp("agent-done"), "the tab reports the launch");

        for _ in 0..40 {
            server.restore_finished_agent_panes();
            let restored = server
                .panes
                .lock()
                .expect("panes poisoned")
                .get(&pane)
                .is_some_and(|runtime| runtime.launch == Launch::Shell);
            if restored {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        let panes = server.panes.lock().expect("panes poisoned");
        let runtime = panes.get(&pane).expect("the pane was respawned");
        assert_eq!(runtime.launch, Launch::Shell);
        drop(panes);
        let session = server.session.lock().expect("session poisoned");
        assert!(
            session.selected_tab().env.is_empty(),
            "a tab respawned as a plain shell reports no launch"
        );
        drop(session);
        server.stop_panes();
        let _ = std::fs::remove_dir_all(&scratch);
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
        uze_testkit::process::install_executable(
            &versioned_binary,
            &std::fs::read("/bin/sleep").unwrap(),
        );

        let (damage, _damage_events) = std::sync::mpsc::channel();
        let pane = PaneRuntime::spawn(
            PaneId(23),
            PathBuf::from("/tmp"),
            80,
            24,
            damage,
            Launch::Program {
                argv: vec![
                    "/bin/sh".to_owned(),
                    "-c".to_owned(),
                    format!(
                        "export UZE_SHIM_NAME=claude UZE_SHIM_PID=1; exec {} 5",
                        versioned_binary.display()
                    ),
                ],
                env: Vec::new(),
            },
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

    /// The pid behind a socket is the kernel's answer, but a moment old by
    /// the time it would be signalled, and pids are recycled: a process that
    /// is not running `uze` is never signalled — an editor, a build, another
    /// agent of the person's own.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_process_that_is_not_uze_is_never_signalled() {
        let scratch = uze_testkit::temp::socket_scratch("bystander");
        std::fs::create_dir_all(&scratch).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &scratch);

        let bystander = ReadyProcess::spawn(Path::new("/bin/sh"));
        retire(bystander.pid(), &scratch.join("test.sock"));

        assert!(
            bystander.finish(),
            "a process that is not uze must be left running"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A server the client started and never reaped is a zombie: still
    /// addressable by `kill(pid, 0)`, which once left the endpoint held
    /// hostage for the whole remaining life of that client. A zombie holds
    /// no descriptor, so it holds no claim.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_crashed_server_nobody_reaped_holds_no_claim() {
        let scratch = uze_testkit::temp::scratch("terminal-zombie");
        std::fs::create_dir_all(&scratch).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &scratch);

        let holder = ClaimHolder::spawn(&scratch);
        assert!(workspace_is_claimed(), "a running server holds its claim");

        let zombie = holder.crash();
        assert!(
            !workspace_is_claimed(),
            "an unreaped dead server must not hold its workspace hostage"
        );

        zombie.reap();
        let _ = std::fs::remove_dir_all(&scratch);
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
        let owner = super::current_uid();
        let candidate = xdg.join(format!("uze-runtime-{owner}"));
        std::os::unix::fs::symlink(&elsewhere, &candidate).unwrap();

        let mut env = uze_testkit::env::scope();
        // `UZE_HOME` leads the candidates, so it is pointed somewhere too
        // long to hold a socket: what is under test is the runtime
        // directory behind it.
        env.set("UZE_HOME", scratch.join("h".repeat(120)));
        env.set("XDG_RUNTIME_DIR", &xdg);
        let socket = socket_path().expect("a bad candidate is stepped over, not fatal");
        assert!(
            !socket.starts_with(&xdg),
            "a symlinked candidate must not be adopted, got {}",
            socket.display()
        );

        // A directory this user genuinely owns is theirs to correct rather
        // than to refuse — a permissive umask on first run is the ordinary
        // way one is created too open.
        std::fs::remove_file(&candidate).unwrap();
        std::fs::create_dir_all(&candidate).unwrap();
        std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o777)).unwrap();
        let socket = socket_path().expect("our own directory is usable");
        assert!(socket.starts_with(&candidate));
        let mode = std::fs::metadata(&candidate).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "the mode is corrected, not inherited");

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The recorded WSL case: `/tmp` wiped under a live server takes the
    /// socket with it, and a second server started then would restore the
    /// same `workspace.json` — every agent twice in the same checkout, both
    /// servers persisting over each other.
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

        let socket = socket_path().unwrap();
        assert!(
            workspace_lock_path().starts_with(&uze_home),
            "the claim must live beside the workspace, never in a wipeable temp directory"
        );

        let first = ClaimHolder::spawn(&uze_home);
        let second = Server::new(seat_at(&project), socket.clone());
        assert!(
            matches!(second, Err(RuntimeError::Protocol(_))),
            "a workspace a live server holds must not be restored a second time"
        );

        first.release();
        let (third, _damage3) =
            Server::new(seat_at(&project), socket).expect("the claim is released with its holder");
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

        let holder = ClaimHolder::spawn(&scratch);
        match WorkspaceLock::acquire() {
            Err(RuntimeError::Protocol(refusal)) => assert!(
                refusal.contains("already serving this workspace"),
                "contention has to name the server that holds it, not an errno: {refusal}"
            ),
            Err(other) => panic!("a held claim must read as contention, not as {other}"),
            Ok(_) => panic!("a held claim must not be granted twice"),
        }
        holder.release();
        WorkspaceLock::acquire().expect("released with its holder");

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A client asks for the claim shared and a server takes it exclusively,
    /// so a server starting while a client asks can tell the asker from a
    /// server — and gives up only for a server.
    #[test]
    fn an_asker_is_never_mistaken_for_a_server() {
        let scratch = uze_testkit::temp::scratch("terminal-lock-asker");
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("workspace.lock");
        let open = || {
            std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
                .unwrap()
        };
        let starting = open();

        let asker = open();
        super::flock(&asker, libc::LOCK_SH | libc::LOCK_NB).expect("an asker takes it shared");
        assert!(!held_by_a_server(&starting).unwrap());
        drop(asker);

        let server = open();
        super::flock(&server, libc::LOCK_EX | libc::LOCK_NB)
            .expect("a server takes it exclusively");
        assert!(held_by_a_server(&starting).unwrap());
        drop(server);

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A server whose `$UZE_HOME` was deleted while it ran still holds its
    /// lock — on a file that no longer exists. What attach reads is the
    /// workspace as it is now: nobody holds it, so the listener is serving
    /// a world that is gone and is replaced rather than attached to.
    #[test]
    fn a_workspace_deleted_under_its_server_reads_as_unclaimed() {
        let scratch = uze_testkit::temp::scratch("terminal-workspace-orphaned");
        std::fs::create_dir_all(&scratch).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &scratch);

        let holder = ClaimHolder::spawn(&scratch);
        assert!(workspace_is_claimed(), "a live claim is a claim");

        std::fs::remove_dir_all(scratch.join("state")).unwrap();
        assert!(
            !workspace_is_claimed(),
            "a claim on a deleted file holds nothing anybody can reach"
        );

        holder.release();
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Where the process [`leave_a_stale_socket`] runs binds its socket.
    const STALE_SOCKET: &str = "UZE_TERMINAL_TEST_STALE_SOCKET";

    /// The process side of [`leave_a_stale_socket`]. Ignored so the suite
    /// never runs it on its own, and guarded by [`STALE_SOCKET`] so
    /// `--include-ignored` binds nothing.
    #[test]
    #[ignore = "binds a socket and exits, run by leave_a_stale_socket in a process of its own"]
    fn binds_a_socket_and_exits() {
        if let Some(path) = std::env::var_os(STALE_SOCKET) {
            std::os::unix::net::UnixListener::bind(path).expect("the socket path binds");
        }
    }

    /// A socket file nobody listens on — the shape a crashed server leaves.
    ///
    /// Bound by a process of its own that has exited before this returns:
    /// a listener this process bound and dropped is copied into every child
    /// a sibling test forks until that child's `exec`, and answers a
    /// connect for as long.
    fn leave_a_stale_socket(path: &Path) {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "runtime::tests::binds_a_socket_and_exits",
            ])
            .env(STALE_SOCKET, path)
            .stdout(std::process::Stdio::null())
            .status()
            .expect("this test binary runs itself");
        assert!(status.success() && path.exists(), "{status}");
    }

    /// Set on the process that plays the other server in the claim tests.
    const CLAIM_HOLDER: &str = "UZE_TERMINAL_TEST_HOLDS_CLAIM";
    /// What that process says once the claim is its.
    const CLAIM_HELD: &str = "workspace claim held";
    /// Set, to a project root, on the holder that is to be a whole server —
    /// endpoint bound and handshakes answered — rather than a claim and
    /// nothing else.
    const SERVING_HOLDER: &str = "UZE_TERMINAL_TEST_SERVES";

    /// The other server in the two claim tests: a process of its own that
    /// takes the workspace claim under the `UZE_HOME` it is given, says so,
    /// and keeps it for as long as its stdin stays open.
    ///
    /// A claim is made against a *process* — `flock` lives on the open file
    /// description, which a `fork` shares with the child until the child's
    /// own `exec` closes it. A test that held the claim itself and dropped
    /// it could therefore find it still held, for an instant, by a process
    /// a sibling test had just forked; a claim this process never took is
    /// one nothing it forked can be keeping. Ignored so the suite never
    /// runs it on its own — [`ClaimHolder`] runs it, by name, in a process
    /// of its own — and guarded by [`CLAIM_HOLDER`] so `--include-ignored`
    /// cannot sit it on a terminal's stdin.
    #[test]
    #[ignore = "the holder side of the workspace-claim tests, run by them in a process of their own"]
    fn holds_the_workspace_claim_while_its_stdin_is_open() {
        if std::env::var_os(CLAIM_HOLDER).is_none() {
            return;
        }
        // Asked to be a whole server: `Server::new` takes the claim, the
        // endpoint is bound, and clients are answered — everything a
        // running server of another build is, since what an attach does
        // about one is decided by what it answers.
        let _held = match std::env::var_os(SERVING_HOLDER) {
            Some(root) => {
                let socket = socket_path().expect("the holder's endpoint");
                let (server, _damage) = Server::new(seat_at(Path::new(&root)), socket.clone())
                    .expect("the holder serves");
                let server = Arc::new(server);
                let listener = bind_endpoint(&socket).expect("the holder binds its endpoint");
                std::thread::spawn(move || {
                    for stream in listener.incoming().flatten() {
                        let server = Arc::clone(&server);
                        std::thread::spawn(move || server.handle_client(stream));
                    }
                });
                None
            }
            None => Some(WorkspaceLock::acquire().expect("the holder's claim is granted")),
        };
        println!("{CLAIM_HELD}");
        let mut until_eof = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut until_eof);
    }

    /// A separate process holding the workspace claim under one `UZE_HOME`
    /// — this test binary, re-run on the one ignored test above.
    struct ClaimHolder {
        process: std::process::Child,
        /// Kept open until the holder has exited, so its test harness has
        /// somewhere to write its own closing lines.
        _output: std::io::BufReader<std::process::ChildStdout>,
    }

    impl ClaimHolder {
        /// Returns once the holder says the claim is its.
        fn spawn(uze_home: &Path) -> Self {
            Self::spawn_as(&std::env::current_exe().unwrap(), uze_home)
        }

        /// The same holder, run from `executable` — a copy of this test
        /// binary, for a server of another build.
        fn spawn_as(executable: &Path, uze_home: &Path) -> Self {
            Self::spawn_with(executable, uze_home, None)
        }

        /// A holder that is a whole server: it binds this `UZE_HOME`'s
        /// endpoint over `project` and answers handshakes, which is what an
        /// attach asks of a server before it decides anything about it.
        fn spawn_serving(executable: &Path, uze_home: &Path, project: &Path) -> Self {
            Self::spawn_with(executable, uze_home, Some(project))
        }

        fn spawn_with(executable: &Path, uze_home: &Path, serving: Option<&Path>) -> Self {
            let mut command = std::process::Command::new(executable);
            if let Some(project) = serving {
                command.env(SERVING_HOLDER, project);
            }
            let mut process = command
                .args([
                    "--ignored",
                    "--exact",
                    "--nocapture",
                    "runtime::tests::holds_the_workspace_claim_while_its_stdin_is_open",
                ])
                .env("UZE_HOME", uze_home)
                .env(CLAIM_HOLDER, "1")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("this test binary runs itself");
            let mut output = std::io::BufReader::new(process.stdout.take().unwrap());
            let mut line = String::new();
            loop {
                line.clear();
                match std::io::BufRead::read_line(&mut output, &mut line) {
                    Ok(0) => panic!("the holder exited without taking the claim"),
                    Ok(_) if line.trim_end() == CLAIM_HELD => break,
                    Ok(_) => continue,
                    Err(error) => panic!("reading the holder: {error}"),
                }
            }
            Self {
                process,
                _output: output,
            }
        }

        fn pid(&self) -> u32 {
            self.process.id()
        }

        /// Lets the holder exit and waits for it: the kernel releases the
        /// claim with the process, so once this returns nothing holds it.
        fn release(mut self) {
            drop(self.process.stdin.take());
            let status = self.process.wait().expect("the holder is waited on");
            assert!(status.success(), "the holder's own run failed: {status}");
        }

        /// Kills the holder and leaves it unreaped — what a client that
        /// started a server and never waited on it is left with.
        #[cfg(target_os = "linux")]
        fn crash(mut self) -> Zombie {
            self.process.kill().expect("the holder is killed");
            let mut exited: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // `WNOWAIT` blocks until the holder has exited and leaves it a
            // zombie rather than reaping it.
            let waited = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.pid(),
                    &mut exited,
                    libc::WEXITED | libc::WNOWAIT,
                )
            };
            assert_eq!(waited, 0, "{}", std::io::Error::last_os_error());
            Zombie(self)
        }
    }

    #[cfg(target_os = "linux")]
    struct Zombie(ClaimHolder);

    #[cfg(target_os = "linux")]
    impl Zombie {
        fn reap(mut self) {
            let _ = self.0.process.wait();
        }
    }

    /// This test binary under the name `uze` at another path: to the
    /// process table, a `uze` that is not this build.
    fn another_build_of_this_binary(scratch: &Path) -> PathBuf {
        let directory = scratch.join("another-build");
        std::fs::create_dir_all(&directory).unwrap();
        let copy = directory.join("uze");
        // Copied by a child that has exited before the copy runs, so no
        // descriptor open for writing on it lingers in a process a sibling
        // test forked — the kernel's `ETXTBSY`.
        let copied = std::process::Command::new("cp")
            .arg(std::env::current_exe().unwrap())
            .arg(&copy)
            .status()
            .expect("cp runs");
        assert!(copied.success());
        copy
    }

    /// A shell that has certainly `exec`ed — it said so — and waits to be
    /// told to finish.
    struct ReadyProcess {
        process: std::process::Child,
        output: std::io::BufReader<std::process::ChildStdout>,
    }

    impl ReadyProcess {
        fn spawn(shell: &Path) -> Self {
            let mut process = std::process::Command::new(shell)
                .args(["-c", "echo ready; read line; echo alive"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("the shell runs");
            let mut output = std::io::BufReader::new(process.stdout.take().unwrap());
            let mut line = String::new();
            std::io::BufRead::read_line(&mut output, &mut line).unwrap();
            assert_eq!(line.trim_end(), "ready");
            Self { process, output }
        }

        fn pid(&self) -> u32 {
            self.process.id()
        }

        /// Tells it to finish, and says whether it was still there to.
        fn finish(mut self) -> bool {
            let told = self
                .process
                .stdin
                .take()
                .is_some_and(|mut stdin| std::io::Write::write_all(&mut stdin, b"\n").is_ok());
            let mut line = String::new();
            let answered = told
                && std::io::BufRead::read_line(&mut self.output, &mut line).is_ok()
                && line.trim_end() == "alive";
            let _ = self.process.wait();
            answered
        }
    }

    /// The incident of 2026-09-19: a release moved the workspace's shape,
    /// the spaces were set aside, and the operator started from nothing.
    /// The difference was one field — `kind` per space, which moved onto
    /// the agent — and every other field mapped one to one.
    #[test]
    fn a_workspace_that_gave_every_space_a_kind_opens_on_this_build() {
        let scratch = uze_testkit::temp::scratch("terminal-workspace-shape-1");
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("workspace.json");
        std::fs::write(
            &path,
            br#"{"spaces":[
                 {"label":"uze","root":"/tmp/uze","kind":"worktree","tabs":[
                   {"label":"shell","cwd":"/tmp/uze","agent":null,"launch":"Shell"}]},
                 {"label":"home","root":"/tmp/home","kind":"plain","tabs":[]}]}"#,
        )
        .unwrap();

        let (workspace, set_aside) = load_persisted_workspace_at(&path);
        assert!(
            set_aside.is_none(),
            "a shape this build knows is carried across, not set aside"
        );
        let workspace = workspace.expect("the spaces survive the upgrade");
        assert_eq!(workspace.schema_version, WORKSPACE_SCHEMA_VERSION);
        let roots: Vec<_> = workspace
            .spaces
            .iter()
            .map(|space| space.root.display().to_string())
            .collect();
        assert_eq!(roots, ["/tmp/uze", "/tmp/home"], "every space, in order");
        assert_eq!(
            workspace.spaces[0].tabs.len(),
            1,
            "and every tab the space carried"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Nothing persisted at all is a first run, not a loss.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_first_run_reports_nothing() {
        let scratch = uze_testkit::temp::socket_scratch("setaside-first");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&uze_home).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);

        let socket = socket_path().unwrap();
        let (server, _damage) = Server::new(seat_at(&project), socket).expect("server");
        assert!(
            server
                .set_aside
                .lock()
                .expect("set-aside poisoned")
                .is_none(),
            "there was nothing to lose, so there is nothing to say"
        );
        server.stop_panes();
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The runtime starts before anyone is watching, and it is a different
    /// process from the screen. So what it could not carry waits for the
    /// first client and is said there — not in a log that is off unless
    /// `UZE_LOG` is set, which is where the one that mattered went.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_client_is_told_what_the_runtime_could_not_carry() {
        let scratch = uze_testkit::temp::socket_scratch("setaside-told");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&uze_home).unwrap();
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);

        let path = persisted_state_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not a workspace at all").unwrap();

        let socket = socket_path().unwrap();
        let (server, _damage) = Server::new(seat_at(&project), socket).expect("server");
        let server = std::sync::Arc::new(server);

        // A socket pair stands in for the endpoint: what is being proven
        // is what a client is told once it attaches, not how it got there.
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
                columns: 80,
                rows: 24,
                seating: crate::Seating::WhereItLeftOff,
            },
        )
        .unwrap();

        let mut told = None;
        for _ in 0..8 {
            match read_event(&mut reader) {
                Ok(Some(crate::ClientEvent::WorkspaceSetAside { kept_at, .. })) => {
                    told = Some(kept_at);
                    break;
                }
                Ok(Some(_)) => {}
                _ => break,
            }
        }
        let kept_at = told.expect("the client is told, rather than a log nobody turned on");
        assert!(kept_at.exists(), "and told where the bytes were kept");
        assert!(
            !path.exists(),
            "the workspace itself is out of the way, under a name nothing reads as one"
        );

        let _ = send_request(&mut writer, &crate::ClientRequest::Detach);
        drop(writer);
        let _ = serving.join();
        server.stop_panes();
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Two builds on one machine is the ordinary state of this repository.
    /// A workspace a newer build wrote is not this one's to move.
    #[test]
    fn a_workspace_from_a_newer_build_is_left_exactly_as_it_is() {
        let scratch = uze_testkit::temp::scratch("terminal-workspace-newer");
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("workspace.json");
        let bytes = br#"{"schema_version":99,"spaces":[]}"#;
        std::fs::write(&path, bytes).unwrap();

        let (workspace, set_aside) = load_persisted_workspace_at(&path);
        assert!(
            workspace.is_none(),
            "this build starts from the seat it was given"
        );
        assert!(
            set_aside.is_none(),
            "and takes nothing away from the newer one"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "the bytes stay exactly where the newer build put them"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Bytes that are not a workspace at all are the one case that still
    /// sets aside — and the bytes are kept, never deleted.
    #[test]
    fn a_workspace_that_cannot_be_read_is_kept_and_reported() {
        let scratch = uze_testkit::temp::scratch("terminal-workspace-garbage");
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("workspace.json");
        std::fs::write(&path, b"not a workspace").unwrap();

        let (workspace, set_aside) = load_persisted_workspace_at(&path);
        assert!(workspace.is_none());
        let set_aside = set_aside.expect("the runtime can say what it could not carry");
        assert!(!path.exists(), "the workspace is out of the way");
        assert!(set_aside.path.exists(), "and its bytes are kept");

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

        let socket = socket_path().unwrap();
        let (server, _damage) = Server::new(seat_at(&roots[0]), socket).unwrap();
        let server = Arc::new(server);
        for root in &roots[1..] {
            server
                .ensure_space(&seat_at(root), PLACEHOLDER_PANE_SIZE)
                .expect("a space per root");
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
                columns: 0,
                rows: 0,
                seating: crate::Seating::WhereItLeftOff,
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
        let (outbox, receiver) = super::Outbox::new();
        let events = Arc::new(outbox);
        let writing = {
            let backlog = events.backlog();
            std::thread::spawn(move || super::forward_events(socket, &receiver, &backlog))
        };

        events.reply(crate::ClientEvent::Error {
            message: "x".repeat(MAX_FRAME as usize + 1),
        });

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

        let socket = socket_path().unwrap();
        let (served, serving) = std::sync::mpsc::channel();
        let serve_root = project.clone();
        std::thread::spawn(move || {
            let _ = served.send(super::serve(seat_at(&serve_root)));
        });

        let mut ready = false;
        for _ in 0..200 {
            if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
                ready = true;
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        assert!(ready, "the server must be listening before it is stopped");

        super::stop().expect("a running server must acknowledge stop");
        serving
            .recv_timeout(Duration::from_secs(30))
            .expect("the stopped server must leave its accept loop")
            .expect("and leave it cleanly");
        assert!(
            !socket.exists(),
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
        let refusal =
            |errno| super::classify_lock_refusal(std::io::Error::from_raw_os_error(errno));
        assert!(matches!(
            refusal(libc::EWOULDBLOCK),
            super::LockRefusal::Contended
        ));
        assert!(matches!(
            refusal(libc::EINTR),
            super::LockRefusal::Interrupted
        ));
        for unsupported in [libc::ENOLCK, libc::EOPNOTSUPP, libc::ENOSYS, libc::EBADF] {
            assert!(
                matches!(refusal(unsupported), super::LockRefusal::Unsupported(_)),
                "errno {unsupported} is a filesystem that cannot lock, not a server that holds one"
            );
        }
    }

    /// `kill(2)` reads `0` as the caller's own process group and a negative
    /// pid as a group, `-1` as every process the user owns. A pid that does
    /// not fit is never turned into one of those.
    #[test]
    fn a_pid_that_does_not_name_one_process_is_never_signalled() {
        assert_eq!(signalable(0), None);
        assert_eq!(signalable(u32::MAX), None, "which would read as -1");
        assert_eq!(signalable(i32::MAX as u32 + 1), None);
        assert_eq!(signalable(4192325), Some(4192325));
    }

    /// Only two facts decide what an attach does to the endpoint: who holds
    /// the workspace claim, and who the kernel and the process table say is
    /// listening. A claimed workspace is never taken from a listener
    /// nobody can vouch for — without a readable process table that is
    /// every listener, and the one serving is alive.
    #[test]
    fn an_attach_replaces_only_a_server_it_can_name() {
        let pid = 4242;
        for (claimed, listener, expected) in [
            (true, Listener::ThisBuild(pid), Arrival::Connect),
            (true, Listener::Unrecognized, Arrival::Connect),
            (true, Listener::Nobody, Arrival::Connect),
            // Alive and serving this workspace: asked, never ended on the
            // strength of the image it was started from.
            (true, Listener::AnotherBuild(pid), Arrival::Ask(pid)),
            (false, Listener::ThisBuild(pid), Arrival::Replace(pid)),
            (false, Listener::AnotherBuild(pid), Arrival::Replace(pid)),
            (false, Listener::Unrecognized, Arrival::Start),
            (false, Listener::Nobody, Arrival::Start),
        ] {
            assert_eq!(
                arrival(claimed, listener),
                expected,
                "claimed: {claimed}, listener: {listener:?}"
            );
        }
    }

    /// The writer thread serving a client must end when the client does.
    ///
    /// It once could not: it was handed the client's whole `Outbox`, so it
    /// held a sender to the channel it was blocked on and `recv` never
    /// reported the hang-up. Every connection the endpoint ever accepted —
    /// each `uze` attaching, and each probe asking who listens here — then
    /// cost one thread and one descriptor for the life of the server, which
    /// is how two servers reached fourteen thousand threads apiece and left
    /// a machine unable to `fork`. The socket is deliberately still open on
    /// the peer's side, so what ends the thread can only be the channel.
    #[test]
    fn a_clients_writer_thread_ends_with_the_client() {
        let (_peer, socket) = std::os::unix::net::UnixStream::pair().unwrap();
        let (outbox, receiver) = Outbox::new();
        let outbox = Arc::new(outbox);
        let backlog = outbox.backlog();
        let writer = thread::spawn(move || forward_events(socket, &receiver, &backlog));

        drop(outbox);

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !writer.is_finished() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            writer.is_finished(),
            "the last outbox is gone, so nothing can reach this client again"
        );
        writer.join().unwrap();
    }

    /// And the same seen from the wire: a peer the server turns away is
    /// hung up on, not held. Reading to end-of-file is the proof that no
    /// thread inside the server still owns a copy of this connection.
    #[test]
    fn a_peer_the_server_refuses_is_hung_up_on() {
        let scratch = uze_testkit::temp::socket_scratch("refused-peer-hangs-up");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        for directory in [&uze_home, &project] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);
        let socket = scratch.join("test.sock");
        let (server, _damage) = Server::new(seat_at(&project), socket.clone()).unwrap();
        let server = Arc::new(server);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let serving = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            server.handle_client(stream);
        });

        let mut peer = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        peer.set_read_timeout(Some(ANSWERS_WITHIN)).unwrap();
        send_request(
            &mut peer,
            &crate::ClientRequest::Attach {
                version: crate::PROTOCOL_VERSION + 1,
                columns: 80,
                rows: 24,
                seating: crate::Seating::WhereItLeftOff,
            },
        )
        .unwrap();
        assert!(
            matches!(
                read_event(&mut peer.try_clone().unwrap()),
                Ok(Some(crate::ClientEvent::Error { .. }))
            ),
            "a peer speaking another protocol is told so"
        );

        let mut rest = Vec::new();
        std::io::Read::read_to_end(&mut peer, &mut rest).expect("the server hangs up");
        assert!(rest.is_empty(), "nothing follows the refusal");

        let _ = serving.join();
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A `make install` over a running server leaves every later `uze`
    /// looking at a server built from another image — and ending one costs
    /// every agent it runs its process, mid-conversation, whether or not
    /// the two builds could have talked. They usually could: the image
    /// says which binary a server came from, and `PROTOCOL_VERSION` says
    /// what it speaks. A server that answers this build's handshake is
    /// attached to, and the panes it is running go on running.
    #[test]
    fn a_server_that_answers_this_builds_handshake_serves_it() {
        let scratch = uze_testkit::temp::socket_scratch("serves-answered");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        for directory in [&uze_home, &project] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);
        let socket = scratch.join("test.sock");
        let (server, _damage) = Server::new(seat_at(&project), socket.clone()).unwrap();
        let server = Arc::new(server);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let serving = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            server.handle_client(stream);
        });

        assert!(
            serves_this_build(&socket),
            "a server that reads this build's attach and describes the workspace can serve it"
        );

        let _ = serving.join();
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The state the question exists to find: something is listening at the
    /// endpoint of a claimed workspace and cannot be talked to — a server
    /// built to another framing, one that refuses this build's version, one
    /// that hung up, one that has stopped answering at all. Each is a "no",
    /// and the silent one is a "no" within a bound rather than an attach
    /// that waits on it forever.
    #[test]
    fn a_server_that_cannot_answer_is_never_taken_for_one_that_can() {
        let scratch = uze_testkit::temp::socket_scratch("serves-refused");
        std::fs::create_dir_all(&scratch).unwrap();

        let refusing = scratch.join("refusing.sock");
        let listener = std::os::unix::net::UnixListener::bind(&refusing).unwrap();
        let answering = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = write_message(
                &mut stream,
                &crate::ClientEvent::Error {
                    message: "incompatible terminal runtime protocol".into(),
                },
            );
        });
        assert!(
            !serves_this_build(&refusing),
            "a server that refuses this build's version cannot serve it"
        );
        let _ = answering.join();

        let hanging_up = scratch.join("hanging-up.sock");
        let listener = std::os::unix::net::UnixListener::bind(&hanging_up).unwrap();
        let dropping = std::thread::spawn(move || drop(listener.accept().unwrap()));
        assert!(
            !serves_this_build(&hanging_up),
            "and neither can one that hangs up on the handshake"
        );
        let _ = dropping.join();

        let silent = scratch.join("silent.sock");
        let listener = std::os::unix::net::UnixListener::bind(&silent).unwrap();
        let (answered, asked) = std::sync::mpsc::channel::<()>();
        let holding = std::thread::spawn(move || {
            let held = listener.accept().unwrap();
            let _ = asked.recv();
            drop(held);
        });
        let began = std::time::Instant::now();
        assert!(!serves_this_build(&silent), "nor one that says nothing");
        assert!(
            began.elapsed() < ANSWERS_WITHIN * 2,
            "and the silence is bounded: an attach cannot wait on it"
        );
        drop(answered);
        let _ = holding.join();

        assert!(
            !serves_this_build(&scratch.join("nobody.sock")),
            "an endpoint nothing is behind answers nothing either"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// What all of this is for, end to end: a second terminal opened while
    /// agents are running in the first attaches to the server already
    /// serving them, even though a `make install` has made that server
    /// "another build" in the meantime. It used to be retired on sight —
    /// every pane it held killed with it, mid-conversation, because a
    /// binary had been replaced on disk.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_second_client_attaches_to_a_live_server_of_another_build() {
        let scratch = uze_testkit::temp::socket_scratch("attach-another-build");
        let uze_home = scratch.join("home");
        let project = scratch.join("project");
        for directory in [&uze_home, &project] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mut env = uze_testkit::env::scope();
        env.set("UZE_HOME", &uze_home);
        let serving = ClaimHolder::spawn_serving(
            &another_build_of_this_binary(&scratch),
            &uze_home,
            &project,
        );
        let socket = socket_path().unwrap();
        assert_eq!(
            listener_at(&socket),
            Listener::AnotherBuild(serving.pid()),
            "the server at the endpoint was started from another image"
        );

        let mut stream = super::attach(&seat_at(&project)).expect("the client attaches");

        assert_eq!(
            super::claim_holder(),
            Some(serving.pid()),
            "to the server that was already there, which still holds the workspace"
        );
        send_request(
            &mut stream,
            &crate::ClientRequest::Attach {
                version: crate::PROTOCOL_VERSION,
                columns: 80,
                rows: 24,
                seating: crate::Seating::WhereItLeftOff,
            },
        )
        .unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let described = std::iter::from_fn(|| read_event(&mut reader).unwrap())
            .find_map(|event| match event {
                crate::ClientEvent::Snapshot { session } => Some(session),
                _ => None,
            })
            .expect("and it serves this client");
        assert!(!described.workspace.spaces.is_empty());

        let _ = send_request(&mut stream, &crate::ClientRequest::Detach);
        serving.release();
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
            columns: 200,
            rows: 50,
            seating: crate::Seating::Open(seat_at(Path::new("/some/ordinary/project/path"))),
        })
        .unwrap();
        assert!(
            attach.len() < super::MAX_HANDSHAKE_FRAME as usize,
            "and the bound still has to fit what a handshake actually says"
        );
    }
}
