//! Keeping the binary the installer placed current, and saying so.
//!
//! Only a binary `install.sh` placed is ever replaced. The installer leaves
//! a receipt (`state/install.json`) naming the file it wrote, and a binary
//! running from anywhere else — `cargo install`, `make install`, a package
//! manager, `target/debug` — belongs to whatever put it there: the most this
//! module does for it is say that a newer release exists. That receipt is
//! what ADR-034 was guarding when it kept a self-update out of scope; a
//! replacement that cannot tell whose file it is replacing is the thing to
//! refuse, and the receipt answers the question.
//!
//! The replacement is a rename over the old file, so a process already
//! running from it — the terminal server, a pane's shim, this very client —
//! keeps the inode it started from, and the next launch is the first to run
//! the new release. Nothing here restarts anything.
//!
//! `UZE_AUTOUPDATE=off` stops the check entirely, `notify` checks without
//! ever replacing, and `CI` being set means off unless the variable says
//! otherwise: a disposable machine has no use for a newer binary than the
//! one it was handed.

use std::{
    env, fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest as _, Sha256};
use uze_application::UzeHome;

/// Where releases are published, and where a notice's link points.
const RELEASES: &str = "https://github.com/hiukky/uze/releases";

/// How long an answer about the latest release is trusted. Every client
/// that opens runs the check, and this is what keeps a second terminal from
/// asking again a minute after the first one did.
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

const RUNNING: &str = env!("CARGO_PKG_VERSION");

/// What the sidebar says about releases, when it says anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Notice {
    /// A newer release replaced this binary on disk; the next launch runs it.
    Installed(String),
    /// This process is a release the updater put in place, and nobody has
    /// looked at what it changed yet.
    Updated(String),
    /// A newer release exists and this binary is not the updater's to
    /// replace — or replacing it was turned off, or did not work.
    Available(String),
}

impl Notice {
    pub(crate) fn version(&self) -> &str {
        match self {
            Self::Installed(version) | Self::Updated(version) | Self::Available(version) => version,
        }
    }

    pub(crate) fn heading(&self) -> &'static str {
        match self {
            Self::Installed(_) => "update installed",
            Self::Updated(_) => "updated",
            Self::Available(_) => "update available",
        }
    }

    /// The one line under the heading, which is also what a click does:
    /// every notice opens the release's own notes.
    pub(crate) fn line(&self) -> &'static str {
        match self {
            Self::Installed(_) => "restart uze to use it",
            Self::Updated(_) | Self::Available(_) => "what's new",
        }
    }

    pub(crate) fn notes(&self) -> String {
        format!("{RELEASES}/tag/v{}", self.version())
    }
}

/// The notice as it stands, and a counter that moves whenever it does —
/// so a client can ask "anything new since?" every tick for the price of
/// one atomic load.
static NOTICE: Mutex<Option<Notice>> = Mutex::new(None);
static REVISION: AtomicU64 = AtomicU64::new(0);

/// The notice, when it changed since `seen`.
pub(crate) fn since(seen: u64) -> Option<(u64, Option<Notice>)> {
    let revision = REVISION.load(Ordering::Acquire);
    (revision != seen).then(|| {
        let notice = NOTICE.lock().map(|notice| notice.clone()).unwrap_or(None);
        (revision, notice)
    })
}

fn publish(notice: Option<Notice>) {
    if let Ok(mut current) = NOTICE.lock() {
        if *current == notice {
            return;
        }
        *current = notice;
    }
    REVISION.fetch_add(1, Ordering::AcqRel);
}

/// Starts the check for this process: now, and once every [`CHECK_EVERY`]
/// after, on a thread of its own. Nothing a client draws waits on it.
pub(crate) fn watch(home: UzeHome) {
    let policy = Policy::current();
    if policy == Policy::Off {
        return;
    }
    // Resolved once, before anything is replaced: on Linux a running
    // binary whose file was renamed over reports itself as `… (deleted)`,
    // and every pass after the first install would otherwise decide this
    // process was never the installer's.
    let this = this_binary();
    let releases = Published::current();
    let parent = tracing::Span::current();
    thread::spawn(move || {
        let _parent = parent.enter();
        loop {
            let notice = pass(&home, policy, this.as_deref(), &releases, unix_now(), false);
            publish(notice);
            thread::sleep(CHECK_EVERY);
        }
    });
}

/// The check a CLI command handed off, run to the end in the process it
/// was handed to — `uze self-update`, which nobody types.
pub fn check_now(home: &UzeHome) {
    let policy = Policy::current();
    if policy != Policy::Off {
        let this = this_binary();
        pass(
            home,
            policy,
            this.as_deref(),
            &Published::current(),
            unix_now(),
            true,
        );
    }
}

/// What a CLI command says on its way out, if anything.
///
/// Nothing here touches the network: a command is budgeted in
/// milliseconds, and a download cannot outlive the process that started it
/// on a thread. When the last answer has gone stale the check is handed to
/// a detached `uze self-update` instead, and what it finds is what the
/// *next* command says — the same trade `gh` and npm's notifier make.
pub fn after_command(home: &UzeHome) -> Option<String> {
    if Policy::current() == Policy::Off {
        return None;
    }
    let now = unix_now();
    let ledger = read_json::<Ledger>(&ledger_path(home)).unwrap_or_default();
    if now.saturating_sub(ledger.checked_at) >= CHECK_EVERY.as_secs() {
        // Claimed before it is handed off, so two commands a second apart
        // start one check rather than two.
        amend_ledger(home, |stored| stored.checked_at = now);
        hand_off_check();
    }
    let this = this_binary();
    let on_disk = read_json::<Receipt>(&receipt_path(home))
        .filter(|receipt| {
            this.as_deref()
                .is_some_and(|this| is_same_file(this, &receipt.binary))
        })
        .map(|receipt| receipt.version);
    let notice = decide(RUNNING, on_disk.as_deref(), &ledger)?;
    let version = notice.version().to_owned();
    if ledger.told.as_deref() == Some(version.as_str()) {
        return None;
    }
    amend_ledger(home, |stored| stored.told = Some(version.clone()));
    Some(match notice {
        Notice::Installed(_) | Notice::Updated(_) => {
            format!(
                "uze was updated to {version} · what's new: {}",
                notice.notes()
            )
        }
        Notice::Available(_) => format!("uze {version} is available · {}", notice.notes()),
    })
}

/// Starts `uze self-update` in a process group of its own, so the Ctrl+C
/// that ends the next command in this terminal cannot end it too.
fn hand_off_check() {
    use std::os::unix::process::CommandExt as _;
    let Ok(binary) = env::current_exe() else {
        return;
    };
    let _ = Command::new(binary)
        .arg("self-update")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
}

fn this_binary() -> Option<PathBuf> {
    env::current_exe().and_then(fs::canonicalize).ok()
}

/// Puts the notice about `version` away, here and in every client after.
pub(crate) fn acknowledge(home: &UzeHome, version: &str) {
    publish(None);
    let (home, version) = (home.clone(), version.to_owned());
    thread::spawn(move || {
        amend_ledger(&home, |ledger| ledger.acknowledged = Some(version));
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Policy {
    Off,
    Notify,
    Install,
}

impl Policy {
    fn current() -> Self {
        Self::from_env(
            env::var("UZE_AUTOUPDATE").ok().as_deref(),
            env::var_os("CI").is_some(),
        )
    }

    fn from_env(setting: Option<&str>, ci: bool) -> Self {
        match setting.map(str::trim) {
            Some("off" | "0" | "false" | "no") => Self::Off,
            Some("notify") => Self::Notify,
            Some("on" | "1" | "true" | "yes") => Self::Install,
            _ if ci => Self::Off,
            _ => Self::Install,
        }
    }
}

/// What `install.sh` wrote: the file it placed, and the release it was.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Receipt {
    binary: PathBuf,
    version: String,
}

/// What the updater itself remembers between runs.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Ledger {
    /// Seconds since the epoch of the last time the latest release was asked for.
    #[serde(default)]
    checked_at: u64,
    #[serde(default)]
    latest: Option<String>,
    /// The last release the updater put in place.
    #[serde(default)]
    installed: Option<String>,
    /// The last release whose notice was put away.
    #[serde(default)]
    acknowledged: Option<String>,
    /// The last release a CLI command mentioned. A line printed after every
    /// command is a line nobody reads, so each release is mentioned once.
    #[serde(default)]
    told: Option<String>,
}

/// Where releases come from. A trait so a pass can be exercised end to end
/// without a network.
trait Releases {
    fn latest(&self) -> Option<String>;
    fn install(&self, version: &str, target: &Path, scratch: &Path) -> Result<(), String>;
}

/// One check: ask what the latest release is (when told to, or when the
/// last answer has gone stale), replace the installer's binary when it is behind, and say
/// what the reader should be told.
fn pass(
    home: &UzeHome,
    policy: Policy,
    this: Option<&Path>,
    releases: &dyn Releases,
    now: u64,
    ask: bool,
) -> Option<Notice> {
    let _span = tracing::info_span!("self_update.pass", running = RUNNING).entered();
    let mut ledger = read_json::<Ledger>(&ledger_path(home)).unwrap_or_default();
    if ask || now.saturating_sub(ledger.checked_at) >= CHECK_EVERY.as_secs() {
        let latest = releases.latest().or(ledger.latest);
        // Stamped even when the question went unanswered: offline is a
        // state that lasts, and asking again on every launch changes
        // nothing about it.
        amend_ledger(home, |stored| {
            stored.checked_at = now;
            stored.latest = latest.clone();
        });
        ledger.checked_at = now;
        ledger.latest = latest;
    }

    let mut receipt = read_json::<Receipt>(&receipt_path(home))
        .filter(|receipt| this.is_some_and(|this| is_same_file(this, &receipt.binary)));
    if let (Some(owned), Some(latest), Policy::Install) = (&mut receipt, &ledger.latest, policy)
        && newer(latest, &owned.version)
    {
        let _span = tracing::info_span!("self_update.install", version = %latest).entered();
        match releases.install(latest, &owned.binary, &home.cache_dir()) {
            Ok(()) => {
                owned.version = latest.clone();
                let _ = write_json(&receipt_path(home), owned);
                amend_ledger(home, |stored| stored.installed = Some(latest.clone()));
                ledger.installed = Some(latest.clone());
            }
            Err(error) => tracing::warn!(%error, "the release could not be installed"),
        }
    }
    decide(
        RUNNING,
        receipt.as_ref().map(|receipt| receipt.version.as_str()),
        &ledger,
    )
}

/// What to say, from what is running, what the installer's file now is
/// (when this is the installer's binary at all), and what the ledger knows.
fn decide(running: &str, on_disk: Option<&str>, ledger: &Ledger) -> Option<Notice> {
    let unseen = |version: &str| ledger.acknowledged.as_deref() != Some(version);
    if let Some(on_disk) = on_disk {
        if newer(on_disk, running) {
            return unseen(on_disk).then(|| Notice::Installed(on_disk.to_owned()));
        }
        if ledger.installed.as_deref() == Some(running) && unseen(running) {
            return Some(Notice::Updated(running.to_owned()));
        }
    }
    let latest = ledger.latest.as_deref()?;
    let current = on_disk.unwrap_or(running);
    (newer(latest, current) && newer(latest, running) && unseen(latest))
        .then(|| Notice::Available(latest.to_owned()))
}

/// The releases `install.sh` downloads, fetched the way it fetches them.
struct Published {
    base: String,
}

impl Published {
    /// `UZE_BASE_URL` is the installer's own override, and means the same
    /// thing here: a mirror, or a local fixture.
    fn current() -> Self {
        Self {
            base: env::var("UZE_BASE_URL").unwrap_or_else(|_| RELEASES.to_owned()),
        }
    }
}

impl Releases for Published {
    /// Read from where `releases/latest` redirects rather than from the
    /// API: the redirect carries no rate limit and no JSON, and it is the
    /// same "latest" the installer resolves.
    fn latest(&self) -> Option<String> {
        let output = Command::new("curl")
            .args(["-fsSL", "--max-time", "15", "-o", "/dev/null", "-w"])
            .arg("%{url_effective}")
            .arg(format!("{}/latest", self.base))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        output.status.success().then_some(())?;
        tag_version(&String::from_utf8_lossy(&output.stdout))
    }

    fn install(&self, version: &str, target: &Path, scratch: &Path) -> Result<(), String> {
        let archive = asset().ok_or("no release is built for this platform")?;
        let scratch = scratch.join("release").join(version);
        let _ = fs::remove_dir_all(&scratch);
        let unpacked = scratch.join("unpacked");
        fs::create_dir_all(&unpacked).map_err(|error| error.to_string())?;
        let result = (|| {
            let download = format!("{}/download/v{version}", self.base);
            fetch(&format!("{download}/{archive}"), &scratch.join(&archive))?;
            fetch(
                &format!("{download}/SHASUMS256.txt"),
                &scratch.join("SHASUMS256.txt"),
            )?;
            let sums = fs::read_to_string(scratch.join("SHASUMS256.txt"))
                .map_err(|error| error.to_string())?;
            let expected =
                expected_sum(&sums, &archive).ok_or(format!("no checksum for {archive}"))?;
            let bytes = fs::read(scratch.join(&archive)).map_err(|error| error.to_string())?;
            if sha256(&bytes) != expected {
                return Err(format!("checksum mismatch for {archive}"));
            }
            let unpack = Command::new("tar")
                .arg("-xzf")
                .arg(scratch.join(&archive))
                .arg("-C")
                .arg(&unpacked)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|error| error.to_string())?;
            if !unpack.success() {
                return Err(format!("cannot unpack {archive}"));
            }
            replace(&unpacked.join("uze"), version, target)
        })();
        let _ = fs::remove_dir_all(&scratch);
        result
    }
}

fn fetch(url: &str, to: &Path) -> Result<(), String> {
    let status = Command::new("curl")
        .args(["-fsSL", "--max-time", "300", "-o"])
        .arg(to)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("cannot download {url}"))
}

/// Puts `staged` where `target` is, having first made it prove it is the
/// release it claims to be — the same last step `install.sh` takes, and for
/// the same reason: a file that does not run is worse than an old one.
fn replace(staged: &Path, version: &str, target: &Path) -> Result<(), String> {
    fs::set_permissions(staged, fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;
    let reported = Command::new(staged)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("the downloaded binary does not run: {error}"))?;
    if !String::from_utf8_lossy(&reported.stdout).contains(version) {
        return Err(format!("the downloaded binary is not {version}"));
    }
    // Beside the target, so the rename below never crosses a filesystem —
    // which is the only way it stays a rename rather than a copy that a
    // pane's shim could catch half-written.
    let beside = target.with_file_name(format!(".uze-update-{}", std::process::id()));
    let placed = (|| {
        fs::copy(staged, &beside)?;
        fs::set_permissions(&beside, fs::Permissions::from_mode(0o755))?;
        fs::File::open(&beside)?.sync_all()?;
        fs::rename(&beside, target)
    })();
    if placed.is_err() {
        let _ = fs::remove_file(&beside);
    }
    placed.map_err(|error| format!("cannot replace {}: {error}", target.display()))
}

/// The asset `install.sh` would pick for this machine. Where the installer
/// has to ask `ldd` which C library the system uses, a running binary
/// already knows which one it was built against.
fn asset() -> Option<String> {
    let arch = match env::consts::ARCH {
        arch @ ("x86_64" | "aarch64") => arch,
        _ => return None,
    };
    let platform = match env::consts::OS {
        "macos" => format!("{arch}-macos"),
        "linux" if cfg!(target_env = "musl") => format!("{arch}-linux-musl"),
        "linux" => format!("{arch}-linux-gnu"),
        _ => return None,
    };
    Some(format!("uze-{platform}.tar.gz"))
}

/// The version a release page's address names — `…/releases/tag/v1.2.3`.
/// Anything else, including the `…/latest` a mirror answers without
/// redirecting, names none.
fn tag_version(url: &str) -> Option<String> {
    let version = url.trim().rsplit('/').next()?.strip_prefix('v')?;
    precedence(version).map(|_| version.to_owned())
}

fn expected_sum(sums: &str, archive: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let sum = fields.next()?;
        (fields.next()?.trim_start_matches('*') == archive).then(|| sum.to_ascii_lowercase())
    })
}

fn sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut spelled, byte| {
            let _ = write!(spelled, "{byte:02x}");
            spelled
        })
}

/// Whether `candidate` is a later release than `current`, by SemVer
/// precedence — which is what tells `0.0.0-alpha.10` from `alpha.9`, and a
/// build from `main` that is ahead of the latest release from one behind it.
fn newer(candidate: &str, current: &str) -> bool {
    match (precedence(candidate), precedence(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Identifier {
    // Declared first: SemVer ranks numeric identifiers below alphanumeric ones.
    Numeric(u64),
    Alphanumeric(String),
}

#[derive(Debug, Eq, PartialEq)]
struct Precedence {
    core: [u64; 3],
    prerelease: Vec<Identifier>,
}

impl Ord for Precedence {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // A release outranks every pre-release of the same core; between
        // two pre-releases, identifiers compare left to right and a longer
        // list wins a tie — which is exactly `Vec`'s own ordering.
        self.core.cmp(&other.core).then_with(|| {
            match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => self.prerelease.cmp(&other.prerelease),
            }
        })
    }
}

impl PartialOrd for Precedence {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn precedence(version: &str) -> Option<Precedence> {
    let version = version.trim().trim_start_matches('v');
    let version = version.split('+').next()?;
    let (core, prerelease) = version.split_once('-').unwrap_or((version, ""));
    let mut numbers = core.split('.').map(|part| part.parse::<u64>().ok());
    let core = [numbers.next()??, numbers.next()??, numbers.next()??];
    if numbers.next().is_some() {
        return None;
    }
    let prerelease = if prerelease.is_empty() {
        Vec::new()
    } else {
        prerelease
            .split('.')
            .map(|part| match part.parse::<u64>() {
                Ok(number) => Some(Identifier::Numeric(number)),
                Err(_) if !part.is_empty() => Some(Identifier::Alphanumeric(part.to_owned())),
                Err(_) => None,
            })
            .collect::<Option<Vec<_>>>()?
    };
    Some(Precedence { core, prerelease })
}

fn is_same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn receipt_path(home: &UzeHome) -> PathBuf {
    home.state_dir().join("install.json")
}

fn ledger_path(home: &UzeHome) -> PathBuf {
    home.state_dir().join("update.json")
}

/// Read, changed and written back in one step rather than from a copy held
/// across a download: the notice can be put away while a pass is running,
/// and a pass writing back what it read before would bring it back.
fn amend_ledger(home: &UzeHome, change: impl FnOnce(&mut Ledger)) {
    let path = ledger_path(home);
    let mut ledger = read_json::<Ledger>(&path).unwrap_or_default();
    change(&mut ledger);
    let _ = write_json(&path, &ledger);
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn write_json(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let staged = path.with_extension(format!("json.{}", std::process::id()));
    fs::write(&staged, serde_json::to_vec_pretty(value)?)?;
    fs::rename(&staged, path)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use uze_testkit::temp::TempDir;

    #[test]
    fn precedence_is_semvers() {
        for (candidate, current) in [
            ("0.0.0-alpha.5", "0.0.0-alpha.4"),
            ("0.0.0-alpha.10", "0.0.0-alpha.9"),
            ("0.0.0", "0.0.0-alpha.9"),
            ("0.0.0-beta", "0.0.0-alpha.9"),
            ("0.0.0-alpha.1.1", "0.0.0-alpha.1"),
            ("0.0.0-alpha.x", "0.0.0-alpha.9"),
            ("0.1.0-alpha.1", "0.0.9"),
            ("1.0.0", "0.99.99"),
            ("v1.0.1", "1.0.0"),
        ] {
            assert!(newer(candidate, current), "{candidate} > {current}");
            assert!(!newer(current, candidate), "{current} < {candidate}");
        }
        assert!(!newer("0.0.0-alpha.4", "0.0.0-alpha.4"));
        assert!(
            !newer("1.0.0+build", "1.0.0"),
            "build metadata does not rank"
        );
        assert!(
            !newer("garbage", "0.0.0"),
            "what does not parse is never newer"
        );
        assert!(!newer("1.0", "0.0.0"), "a core is three numbers");
    }

    #[test]
    fn the_version_is_read_off_the_release_page_the_redirect_lands_on() {
        assert_eq!(
            tag_version("https://github.com/hiukky/uze/releases/tag/v0.0.0-alpha.4\n").as_deref(),
            Some("0.0.0-alpha.4")
        );
        assert_eq!(
            tag_version("https://mirror.test/uze/releases/latest"),
            None,
            "a mirror that did not redirect names no release"
        );
        assert_eq!(tag_version("https://x.test/releases/tag/vnext"), None);
    }

    #[test]
    fn the_asset_is_the_one_the_installer_picks() {
        let asset = asset().expect("every platform uze runs on has a release asset");
        assert!(
            asset.starts_with("uze-") && asset.ends_with(".tar.gz"),
            "{asset}"
        );
        if cfg!(target_os = "macos") {
            assert!(asset.contains("-macos"), "{asset}");
        } else {
            assert!(
                asset.contains("-linux-gnu") || asset.contains("-linux-musl"),
                "{asset}"
            );
        }
    }

    #[test]
    fn a_checksum_is_found_by_the_archive_it_names() {
        let sums = "AB12  uze-x86_64-linux-gnu.tar.gz\ncd34 *uze-aarch64-macos.tar.gz\n";
        assert_eq!(
            expected_sum(sums, "uze-x86_64-linux-gnu.tar.gz").as_deref(),
            Some("ab12")
        );
        assert_eq!(
            expected_sum(sums, "uze-aarch64-macos.tar.gz").as_deref(),
            Some("cd34")
        );
        assert_eq!(expected_sum(sums, "uze-x86_64-linux-musl.tar.gz"), None);
    }

    #[test]
    fn the_policy_is_read_from_the_environment() {
        assert_eq!(Policy::from_env(None, false), Policy::Install);
        assert_eq!(Policy::from_env(None, true), Policy::Off, "CI means off");
        assert_eq!(
            Policy::from_env(Some("on"), true),
            Policy::Install,
            "unless told"
        );
        assert_eq!(Policy::from_env(Some("notify"), false), Policy::Notify);
        assert_eq!(Policy::from_env(Some("off"), false), Policy::Off);
        assert_eq!(Policy::from_env(Some("0"), false), Policy::Off);
    }

    fn ledger(latest: Option<&str>, installed: Option<&str>, seen: Option<&str>) -> Ledger {
        Ledger {
            checked_at: 0,
            latest: latest.map(str::to_owned),
            installed: installed.map(str::to_owned),
            acknowledged: seen.map(str::to_owned),
            told: None,
        }
    }

    #[test]
    fn what_the_sidebar_says_follows_from_what_is_on_disk() {
        let installed = |version: &str| Some(Notice::Installed(version.to_owned()));
        let updated = |version: &str| Some(Notice::Updated(version.to_owned()));
        let available = |version: &str| Some(Notice::Available(version.to_owned()));

        // The installer's binary was replaced; this process is the old one.
        assert_eq!(
            decide(
                "1.0.0",
                Some("1.1.0"),
                &ledger(Some("1.1.0"), Some("1.1.0"), None)
            ),
            installed("1.1.0")
        );
        // The next launch runs it, and says so until the notice is put away.
        assert_eq!(
            decide(
                "1.1.0",
                Some("1.1.0"),
                &ledger(Some("1.1.0"), Some("1.1.0"), None)
            ),
            updated("1.1.0")
        );
        assert_eq!(
            decide(
                "1.1.0",
                Some("1.1.0"),
                &ledger(Some("1.1.0"), Some("1.1.0"), Some("1.1.0"))
            ),
            None
        );
        // Installed by hand rather than by the updater: nothing to announce.
        assert_eq!(
            decide("1.1.0", Some("1.1.0"), &ledger(Some("1.1.0"), None, None)),
            None
        );
        // Not the installer's binary: a newer release is only ever offered.
        assert_eq!(
            decide("1.0.0", None, &ledger(Some("1.1.0"), None, None)),
            available("1.1.0")
        );
        assert_eq!(
            decide("1.0.0", None, &ledger(Some("1.1.0"), None, Some("1.1.0"))),
            None,
            "until it is put away"
        );
        // The installer's binary, but the replacement did not happen.
        assert_eq!(
            decide("1.0.0", Some("1.0.0"), &ledger(Some("1.1.0"), None, None)),
            available("1.1.0")
        );
        // A build ahead of the latest release is told nothing.
        assert_eq!(
            decide("1.2.0-dev", None, &ledger(Some("1.1.0"), None, None)),
            None
        );
        // Nothing known about releases at all.
        assert_eq!(decide("1.0.0", Some("1.0.0"), &Ledger::default()), None);
    }

    struct Fake {
        latest: Option<&'static str>,
        installs: RefCell<Vec<(String, PathBuf)>>,
        fails: bool,
    }

    impl Releases for Fake {
        fn latest(&self) -> Option<String> {
            self.latest.map(str::to_owned)
        }
        fn install(&self, version: &str, target: &Path, _: &Path) -> Result<(), String> {
            self.installs
                .borrow_mut()
                .push((version.to_owned(), target.to_owned()));
            if self.fails {
                Err("offline".to_owned())
            } else {
                Ok(())
            }
        }
    }

    fn fake(latest: Option<&'static str>) -> Fake {
        Fake {
            latest,
            installs: RefCell::default(),
            fails: false,
        }
    }

    fn world() -> (TempDir, UzeHome, PathBuf) {
        let dir = TempDir::new("self-update");
        let home = UzeHome::at(dir.path().join(".uze"));
        let binary = dir.path().join("bin").join("uze");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(&binary, "#!/bin/sh\n").unwrap();
        (dir, home, binary)
    }

    fn receipt(home: &UzeHome, binary: &Path, version: &str) {
        write_json(
            &receipt_path(home),
            &Receipt {
                binary: binary.to_owned(),
                version: version.to_owned(),
            },
        )
        .unwrap();
    }

    #[test]
    fn the_installers_binary_is_replaced_and_the_next_launch_is_told() {
        let (_dir, home, binary) = world();
        receipt(&home, &binary, RUNNING);
        let releases = fake(Some("999.0.0"));

        let notice = pass(
            &home,
            Policy::Install,
            Some(&binary),
            &releases,
            10_000,
            false,
        );

        assert_eq!(
            releases.installs.borrow().as_slice(),
            [("999.0.0".to_owned(), binary.clone())]
        );
        assert_eq!(notice, Some(Notice::Installed("999.0.0".to_owned())));
        let written = read_json::<Receipt>(&receipt_path(&home)).unwrap();
        assert_eq!(written.version, "999.0.0", "the receipt follows the file");

        // Within the hour nothing is asked or installed again.
        let again = fake(Some("999.0.1"));
        pass(&home, Policy::Install, Some(&binary), &again, 10_001, false);
        assert!(again.installs.borrow().is_empty());
    }

    #[test]
    fn a_binary_the_installer_did_not_place_is_never_replaced() {
        let (dir, home, binary) = world();
        receipt(&home, &binary, RUNNING);
        let elsewhere = dir.path().join("target-debug-uze");
        fs::write(&elsewhere, "").unwrap();
        let releases = fake(Some("999.0.0"));

        let notice = pass(
            &home,
            Policy::Install,
            Some(&elsewhere),
            &releases,
            10_000,
            false,
        );

        assert!(releases.installs.borrow().is_empty());
        assert_eq!(notice, Some(Notice::Available("999.0.0".to_owned())));
    }

    #[test]
    fn notify_asks_but_never_replaces() {
        let (_dir, home, binary) = world();
        receipt(&home, &binary, RUNNING);
        let releases = fake(Some("999.0.0"));

        let notice = pass(
            &home,
            Policy::Notify,
            Some(&binary),
            &releases,
            10_000,
            false,
        );

        assert!(releases.installs.borrow().is_empty());
        assert_eq!(notice, Some(Notice::Available("999.0.0".to_owned())));
    }

    #[test]
    fn a_failed_replacement_leaves_the_receipt_and_offers_the_release() {
        let (_dir, home, binary) = world();
        receipt(&home, &binary, RUNNING);
        let releases = Fake {
            fails: true,
            ..fake(Some("999.0.0"))
        };

        let notice = pass(
            &home,
            Policy::Install,
            Some(&binary),
            &releases,
            10_000,
            false,
        );

        assert_eq!(notice, Some(Notice::Available("999.0.0".to_owned())));
        assert_eq!(
            read_json::<Receipt>(&receipt_path(&home)).unwrap().version,
            RUNNING
        );
    }

    #[test]
    fn offline_is_remembered_rather_than_retried_every_launch() {
        let (_dir, home, binary) = world();
        let offline = fake(None);
        assert_eq!(
            pass(
                &home,
                Policy::Install,
                Some(&binary),
                &offline,
                10_000,
                false
            ),
            None
        );
        let ledger = read_json::<Ledger>(&ledger_path(&home)).unwrap();
        assert_eq!(ledger.checked_at, 10_000);
    }

    #[test]
    fn a_replacement_that_does_not_run_as_the_release_is_refused() {
        let dir = TempDir::new("self-update-replace");
        let target = dir.path().join("uze");
        fs::write(&target, "old").unwrap();
        let staged = dir.path().join("staged");
        fs::write(&staged, "#!/bin/sh\necho uze 1.0.0\n").unwrap();

        assert!(replace(&staged, "2.0.0", &target).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "old", "untouched");

        replace(&staged, "1.0.0", &target).unwrap();
        assert!(fs::read_to_string(&target).unwrap().contains("uze 1.0.0"));
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".uze-update")
            })
            .collect();
        assert!(leftovers.is_empty(), "nothing is left beside it");
    }
}
