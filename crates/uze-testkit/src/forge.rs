//! A forge on loopback: the three things a repository is reached through —
//! smart HTTP, SSH and a proxy — with nothing leaving this machine.
//!
//! Each stands in for its real counterpart only where the product's
//! behaviour depends on it: [`GitHttpServer`] answers anonymously or demands
//! Basic auth the way GitHub does (401, never a prompt), [`FakeSsh`] serves
//! `git-upload-pack` and refuses without an agent socket, and
//! [`RecordingProxy`] forwards and remembers. Every listener binds port 0,
//! so no two tests share a port and none waits on another.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

/// How the server answers.
#[derive(Clone, Debug)]
pub enum Answer {
    /// `git http-backend` over the repositories under the root.
    Git,
    /// A login page with status 200, as a forge that never says 401 does.
    LoginPage,
    /// A redirect to the same path under another base.
    RedirectTo(String),
}

/// A smart-HTTP Git server over the bare repositories under `root`.
pub struct GitHttpServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
    answer: Arc<Mutex<Answer>>,
}

impl GitHttpServer {
    /// Serves `root`, anonymously when `credentials` is `None`.
    pub fn start(root: &Path, credentials: Option<(&str, &str)>, answer: Answer) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("a bound address");
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let expected = credentials.map(|(user, password)| {
            format!("Basic {}", base64(format!("{user}:{password}").as_bytes()))
        });
        let root = root.to_path_buf();
        let answer = Arc::new(Mutex::new(answer));
        {
            let stop = stop.clone();
            let requests = requests.clone();
            let answer = answer.clone();
            thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let root = root.clone();
                    let expected = expected.clone();
                    let answer = answer.lock().expect("answer").clone();
                    let requests = requests.clone();
                    thread::spawn(move || {
                        let _ = serve(stream, &root, expected.as_deref(), &answer, &requests);
                    });
                }
            });
        }
        Self {
            address,
            stop,
            requests,
            answer,
        }
    }

    /// Answers every later request this way — a forge that starts serving
    /// a login page where it served a repository.
    pub fn answer_with(&self, answer: Answer) {
        *self.answer.lock().expect("answer") = answer;
    }

    /// `http://127.0.0.1:<port>/<path>`.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}/{}", self.address, path.trim_start_matches('/'))
    }

    /// Every request line this server received, in order, marked
    /// `[authorized]` when it carried credentials.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests").clone()
    }
}

impl Drop for GitHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
    }
}

struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn read_request(reader: &mut BufReader<TcpStream>) -> std::io::Result<Request> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((key, value)) = header.split_once(':') {
            headers.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    let mut request = Request {
        method,
        target,
        headers,
        body: Vec::new(),
    };
    if request
        .header("Transfer-Encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
    {
        loop {
            let mut size = String::new();
            reader.read_line(&mut size)?;
            let size = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
            let mut chunk = vec![0; size + 2];
            reader.read_exact(&mut chunk)?;
            if size == 0 {
                break;
            }
            request.body.extend_from_slice(&chunk[..size]);
        }
    } else if let Some(length) = request.header("Content-Length") {
        let mut body = vec![0; length.parse().unwrap_or(0)];
        reader.read_exact(&mut body)?;
        request.body = body;
    }
    Ok(request)
}

fn serve(
    stream: TcpStream,
    root: &Path,
    expected: Option<&str>,
    answer: &Answer,
    requests: &Mutex<Vec<String>>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let request = read_request(&mut reader)?;
    let authorized = if request.header("Authorization").is_some() {
        " [authorized]"
    } else {
        ""
    };
    requests
        .lock()
        .expect("requests")
        .push(format!("{} {}{authorized}", request.method, request.target));
    let mut stream = stream;
    if let Some(expected) = expected
        && request.header("Authorization") != Some(expected)
    {
        stream.write_all(
            b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"forge\"\r\n\
              Content-Length: 0\r\nConnection: close\r\n\r\n",
        )?;
        return stream.shutdown(Shutdown::Both);
    }
    match answer {
        Answer::LoginPage => {
            let body = b"<html><body>Sign in</body></html>";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            )?;
            stream.write_all(body)?;
        }
        Answer::RedirectTo(base) => {
            let path = request.target.trim_start_matches('/');
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: {}/{path}\r\nContent-Length: 0\r\n\
                 Connection: close\r\n\r\n",
                base.trim_end_matches('/')
            )?;
        }
        Answer::Git => backend(&mut stream, root, &request)?,
    }
    stream.shutdown(Shutdown::Both)
}

fn backend(stream: &mut TcpStream, root: &Path, request: &Request) -> std::io::Result<()> {
    let (path, query) = request
        .target
        .split_once('?')
        .unwrap_or((request.target.as_str(), ""));
    let mut command = Command::new("git");
    command
        .arg("http-backend")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_PROJECT_ROOT", root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "uploadpack.allowFilter")
        .env("GIT_CONFIG_VALUE_0", "true")
        .env("GIT_CONFIG_KEY_1", "uploadpack.allowAnySHA1InWant")
        .env("GIT_CONFIG_VALUE_1", "true")
        .env("REQUEST_METHOD", &request.method)
        .env("PATH_INFO", path)
        .env("QUERY_STRING", query)
        .env("REMOTE_ADDR", "127.0.0.1")
        .env("CONTENT_LENGTH", request.body.len().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(kind) = request.header("Content-Type") {
        command.env("CONTENT_TYPE", kind);
    }
    if let Some(encoding) = request.header("Content-Encoding") {
        command.env("HTTP_CONTENT_ENCODING", encoding);
    }
    if let Some(protocol) = request.header("Git-Protocol") {
        command.env("GIT_PROTOCOL", protocol);
    }
    let mut child = command.spawn()?;
    let body = request.body.clone();
    let mut stdin = child.stdin.take().expect("piped stdin");
    let writer = thread::spawn(move || {
        let _ = stdin.write_all(&body);
    });
    let output = child.wait_with_output()?;
    let _ = writer.join();
    let raw = output.stdout;
    let split = find(&raw, b"\r\n\r\n")
        .map(|at| (at, 4))
        .or_else(|| find(&raw, b"\n\n").map(|at| (at, 2)));
    let (head, body) = match split {
        Some((at, width)) => (&raw[..at], &raw[at + width..]),
        None => (&raw[..0], &raw[..]),
    };
    let head = String::from_utf8_lossy(head);
    let mut status = "200 OK".to_owned();
    let mut headers = String::new();
    for line in head.lines() {
        match line.split_once(':') {
            Some((key, value)) if key.eq_ignore_ascii_case("Status") => {
                status = value.trim().to_owned();
            }
            Some(_) => {
                headers.push_str(line.trim_end());
                headers.push_str("\r\n");
            }
            None => {}
        }
    }
    write!(
        stream,
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let bytes = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                out.push(ALPHABET[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// An `ssh` placed first on `PATH` that serves `git-upload-pack` for the
/// bare repositories under `root`, whatever host it is asked for.
///
/// It refuses unless `SSH_AUTH_SOCK` is set — which is how a test proves
/// the operator's agent socket crossed UZE's stripped environment — and
/// records every argument line it was given, so the options UZE passed can
/// be read back.
pub struct FakeSsh {
    bin: PathBuf,
    log: PathBuf,
}

impl FakeSsh {
    pub fn install(directory: &Path, root: &Path) -> Self {
        let bin = directory.join("fake-ssh-bin");
        std::fs::create_dir_all(&bin).expect("bin directory");
        let log = directory.join("fake-ssh.log");
        let script = format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> '{log}'
[ "$1" = "-G" ] && exit 0
for argument; do
  case "$argument" in
    *.invalid)
      echo "ssh: Could not resolve hostname ${{argument#*@}}: Name or service not known" >&2
      exit 255 ;;
  esac
done
if [ -z "$SSH_AUTH_SOCK" ]; then
  echo "git@forge: Permission denied (publickey)." >&2
  exit 255
fi
for last; do :; done
path=${{last#* }}
path=$(printf '%s' "$path" | tr -d "'")
path=${{path#/}}
[ -d '{root}'/"$path" ] || path=${{path%.git}}
exec git -c uploadpack.allowFilter=true -c uploadpack.allowAnySHA1InWant=true upload-pack '{root}'/"$path"
"#,
            log = log.display(),
            root = root.display(),
        );
        let path = bin.join("ssh");
        std::fs::write(&path, script).expect("fake ssh");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("executable");
        }
        Self { bin, log }
    }

    /// `PATH` with this `ssh` first.
    pub fn path(&self) -> std::ffi::OsString {
        let mut paths = vec![self.bin.clone()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        std::env::join_paths(paths).expect("a joinable PATH")
    }

    /// Every argument line `ssh` was called with.
    pub fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

/// A forward HTTP proxy that remembers every request line it carried.
pub struct RecordingProxy {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    carried: Arc<Mutex<Vec<String>>>,
}

impl RecordingProxy {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("a bound address");
        let stop = Arc::new(AtomicBool::new(false));
        let carried = Arc::new(Mutex::new(Vec::new()));
        {
            let stop = stop.clone();
            let carried = carried.clone();
            thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let carried = carried.clone();
                    thread::spawn(move || {
                        let _ = forward(stream, &carried);
                    });
                }
            });
        }
        Self {
            address,
            stop,
            carried,
        }
    }

    /// `http://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    pub fn carried(&self) -> Vec<String> {
        self.carried.lock().expect("carried").clone()
    }
}

impl Drop for RecordingProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
    }
}

/// Carries one absolute-URI request to its origin, rewritten to the origin
/// form, and the answer back.
fn forward(client: TcpStream, carried: &Mutex<Vec<String>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(client.try_clone()?);
    let request = read_request(&mut reader)?;
    carried
        .lock()
        .expect("carried")
        .push(format!("{} {}", request.method, request.target));
    let rest = request
        .target
        .strip_prefix("http://")
        .unwrap_or(&request.target);
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let mut origin = TcpStream::connect(authority)?;
    write!(origin, "{} /{path} HTTP/1.1\r\n", request.method)?;
    for (key, value) in &request.headers {
        if key.eq_ignore_ascii_case("Transfer-Encoding")
            || key.eq_ignore_ascii_case("Proxy-Connection")
            || key.eq_ignore_ascii_case("Connection")
            || key.eq_ignore_ascii_case("Content-Length")
        {
            continue;
        }
        write!(origin, "{key}: {value}\r\n")?;
    }
    write!(
        origin,
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        request.body.len()
    )?;
    origin.write_all(&request.body)?;
    let mut answer = Vec::new();
    origin.read_to_end(&mut answer)?;
    let mut client = client;
    client.write_all(&answer)?;
    client.shutdown(Shutdown::Both)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_examples() {
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"user:secret"), "dXNlcjpzZWNyZXQ=");
    }
}
