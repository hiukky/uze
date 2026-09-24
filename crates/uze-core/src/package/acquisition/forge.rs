//! What a repository URL *names*, as opposed to how it is reached.
//!
//! A project file has to name a repository in a way every machine reads the
//! same, and no machine's configuration may change. So the name is decided
//! by the URL's shape alone: `git@host:owner/repo.git`,
//! `ssh://git@host/owner/repo` and `https://host/owner/repo.git` are one
//! repository, spelled `https://host/owner/repo`. How this machine then
//! reaches it — anonymously, with the operator's credentials, over SSH — is
//! [`transports`], and never recorded.
//!
//! This is also where the forges UZE knows by name live, because an alias
//! is the only thing here that names a host: `git.rs` beside it names none.

use std::path::{Path, PathBuf};

use crate::error::{Result, UzeError};

/// The hosts every machine can name by alias, and the one a bare
/// `owner/repo` resolves against until the operator chooses another.
pub const BUILT_IN_HOSTS: &[(&str, &str)] = &[
    ("github", "https://github.com"),
    ("gitlab", "https://gitlab.com"),
    ("codeberg", "https://codeberg.org"),
    ("bitbucket", "https://bitbucket.org"),
];

pub const DEFAULT_HOST: &str = "github";

/// The identity a URL names: `https://<host>/<path>` for every HTTPS or
/// default SSH spelling, the URL as written for anything else.
///
/// An SSH URL with a user other than `git`, or with an explicit port, has no
/// HTTPS spelling UZE could honestly claim, so it stays as written. A local
/// path, `file://` and `git://` are not forge URLs at all.
pub fn canonical(url: &str) -> String {
    let url = url.trim();
    if let Some((scheme, rest)) = url.split_once("://") {
        return match scheme {
            "https" | "http" => {
                let (authority, path) = split_authority(rest);
                join(scheme, &authority.to_lowercase(), path)
            }
            "ssh" => {
                let (authority, path) = split_authority(rest);
                match authority.split_once('@') {
                    Some(("git", host)) if !host.contains(':') => {
                        join("https", &host.to_lowercase(), path)
                    }
                    _ => url.to_owned(),
                }
            }
            _ => url.to_owned(),
        };
    }
    match scp(url) {
        Some(("git", host, path)) => join("https", &host.to_lowercase(), path),
        _ => url.to_owned(),
    }
}

/// An identity as a person reads it: `github.com/hiukky/ai`, with no
/// scheme; anything that is not a URL, as it is.
pub fn shown(identity: &str) -> String {
    identity
        .split_once("://")
        .map_or(identity, |(_, rest)| rest)
        .to_owned()
}

/// Whether two identities name the same repository.
///
/// Canonical forms that match are one repository. So is one directory
/// spelled two honest ways — `file:///srv/market` when it was registered as
/// a URL, and `/srv/market` when a checkout with no remote answers for
/// itself — which is a question for the filesystem rather than for the
/// spelling.
pub fn same_repository(left: &str, right: &str) -> bool {
    if canonical(left) == canonical(right) {
        return true;
    }
    let as_local = |identity: &str| {
        let path = identity.strip_prefix("file://").unwrap_or(identity);
        Path::new(path).canonicalize().ok()
    };
    match (as_local(left), as_local(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// The host of an identity, lowercased, without a port.
pub fn host_of(identity: &str) -> Option<String> {
    let (_, rest) = identity.split_once("://")?;
    let (authority, _) = split_authority(rest);
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = match host.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    };
    (!host.is_empty()).then(|| host.to_lowercase())
}

/// Whether a URL reaches only this machine. Plain HTTP is admitted there and
/// nowhere else: it is what lets a test stand a real server up without TLS,
/// and it cannot carry anything to another machine.
pub fn is_loopback(url: &str) -> bool {
    host_of(url).is_some_and(|host| matches!(host.as_str(), "127.0.0.1" | "::1" | "localhost"))
}

/// How one attempt reaches a repository.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// A directory on this machine, or `file://`: nothing to authenticate.
    Local,
    /// No credential, no agent, no `.netrc`.
    Anonymous,
    /// The operator's own credential helper, or their SSH agent and keys.
    Credentialed,
}

/// One way to reach a repository: the URL handed to Git, and what it may
/// carry.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Transport {
    pub url: String,
    pub access: Access,
}

impl Transport {
    /// How a person reads this attempt in a failure.
    pub fn label(&self) -> &'static str {
        match (
            self.access,
            self.url.contains("://") && !self.url.starts_with("ssh://"),
        ) {
            (Access::Local, _) => "local",
            (Access::Anonymous, _) => "https (anonymous)",
            (Access::Credentialed, true) => "https (credentials)",
            (Access::Credentialed, false) => "ssh",
        }
    }

    /// Whether Git may speak plain HTTP for this attempt.
    pub fn plain_http(&self) -> bool {
        self.url.starts_with("http://") && is_loopback(&self.url)
    }
}

/// Every way this machine may try to reach `url`, in the order it tries
/// them: anonymous HTTPS, HTTPS with the operator's credentials, and SSH at
/// `git@<host>:<path>.git` — always the same host and the same path.
///
/// An identity kept as written over SSH is tried once, as written, with the
/// operator's credentials; a local one once, with nothing. Plain HTTP to
/// anything but this machine is refused.
pub fn transports(url: &str) -> Result<Vec<Transport>> {
    let identity = canonical(url);
    let is_http = identity.starts_with("http://");
    if identity.starts_with("https://") || is_http {
        if is_http && !is_loopback(&identity) {
            return Err(UzeError::AcquisitionFailed(format!(
                "{identity} is plain HTTP; UZE speaks HTTP only to this machine, so use https://"
            )));
        }
        let mut attempts = vec![
            Transport {
                url: identity.clone(),
                access: Access::Anonymous,
            },
            Transport {
                url: identity.clone(),
                access: Access::Credentialed,
            },
        ];
        if let Some(ssh) = ssh_endpoint(&identity) {
            attempts.push(Transport {
                url: ssh,
                access: Access::Credentialed,
            });
        }
        return Ok(attempts);
    }
    let access = if identity.starts_with("ssh://") || scp(&identity).is_some() {
        Access::Credentialed
    } else if identity.starts_with("git://") {
        Access::Anonymous
    } else {
        Access::Local
    };
    Ok(vec![Transport {
        url: identity,
        access,
    }])
}

/// The SSH spelling of an HTTPS identity. Port and path prefix are not
/// guessed: a forge that serves SSH elsewhere is `~/.ssh/config`'s to say.
pub fn ssh_endpoint(identity: &str) -> Option<String> {
    let rest = identity
        .strip_prefix("https://")
        .or_else(|| identity.strip_prefix("http://"))?;
    let (_, path) = split_authority(rest);
    let host = host_of(identity)?;
    (!path.is_empty()).then(|| format!("git@{host}:{path}.git"))
}

/// What `ssh` is pointed at to reach an SSH URL — `user@host`, with
/// `-p <port>` before it when the URL names a port — for a person asked to
/// accept a host's key once.
pub fn ssh_destination(url: &str) -> Option<String> {
    if let Some(rest) = url.strip_prefix("ssh://") {
        let (authority, _) = split_authority(rest);
        let (user, host) = authority.rsplit_once('@').unwrap_or(("git", authority));
        // A colon inside `[…]` belongs to an IPv6 address, not to a port.
        let port = host
            .rsplit_once(':')
            .filter(|(address, _)| !address.contains('[') || address.ends_with(']'));
        return Some(match port {
            Some((host, port)) => format!("-p {port} {user}@{host}"),
            None => format!("{user}@{host}"),
        });
    }
    scp(url).map(|(user, host, _)| format!("{user}@{host}"))
}

/// `user@host:path`, Git's `scp`-like spelling of an SSH URL. A colon after
/// the first slash is part of a path, not a host separator.
fn scp(url: &str) -> Option<(&str, &str, &str)> {
    if url.contains("://") {
        return None;
    }
    let (before, path) = url.split_once(':')?;
    if before.contains('/') {
        return None;
    }
    let (user, host) = before.split_once('@')?;
    (!user.is_empty() && !host.is_empty() && !path.is_empty()).then_some((user, host, path))
}

fn split_authority(rest: &str) -> (&str, &str) {
    match rest.split_once('/') {
        Some((authority, path)) => (authority, path),
        None => (rest, ""),
    }
}

fn join(scheme: &str, authority: &str, path: &str) -> String {
    let path = path.trim_matches('/');
    let path = path
        .strip_suffix(".git")
        .unwrap_or(path)
        .trim_end_matches('/');
    if path.is_empty() {
        format!("{scheme}://{authority}")
    } else {
        format!("{scheme}://{authority}/{path}")
    }
}

/// What the operator typed, read before anything is fetched.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Locator {
    /// A directory on this machine, as typed (not yet resolved).
    Path(PathBuf),
    /// A repository: its URL as it will be recorded, and whether it came
    /// from a short form — which is what decides whether a failure suggests
    /// the other hosts.
    Remote {
        url: String,
        reference: Option<String>,
        subdirectory: Option<PathBuf>,
        short: bool,
    },
}

/// Why the operator's input names nothing UZE can read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocatorRefusal {
    /// `ai`: one segment is a name, never a path.
    BareWord {
        word: String,
        directory_exists: bool,
    },
    /// `owner/repo` that is also a directory here.
    Ambiguous { input: String, alias: String },
    /// `gitlub:owner/repo`, or `host:path` with no user.
    UnknownPrefix {
        prefix: String,
        aliases: Vec<String>,
    },
    /// Nothing after the alias.
    Empty,
}

impl std::fmt::Display for LocatorRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BareWord {
                word,
                directory_exists: true,
            } => write!(
                f,
                "`{word}` is not a path; a path is spelled as one — use `./{word}`"
            ),
            Self::BareWord { word, .. } => write!(
                f,
                "`{word}` names no repository: use `owner/{word}` for a repository, or `./{word}` \
                 for a directory"
            ),
            Self::Ambiguous { input, alias } => write!(
                f,
                "`{input}` is both a directory here and a repository name; write `./{input}` for \
                 the directory or `{alias}:{input}` for the repository"
            ),
            Self::UnknownPrefix { prefix, aliases } => write!(
                f,
                "`{prefix}:` is not a host alias on this machine (known: {}); add one with \
                 `uze market host {prefix} <https-url>`",
                aliases.join(", ")
            ),
            Self::Empty => write!(f, "nothing names a repository here"),
        }
    }
}

/// The hosts a locator may name: alias → HTTPS base, and the default.
pub trait HostAliases {
    fn base(&self, alias: &str) -> Option<String>;
    fn default_alias(&self) -> String;
    fn aliases(&self) -> Vec<String>;
}

/// Reads what the operator typed.
///
/// A path must look like one (`/`, `./`, `../`, `~`, `.`, `..`); a URL has a
/// scheme or is `user@host:path`; `alias:owner/repo` names a host; a bare
/// `owner/repo` names the default host; one bare word is refused, because
/// every package tool reads it as a name and so will a person.
pub fn parse_locator(
    input: &str,
    hosts: &dyn HostAliases,
    is_directory: &dyn Fn(&str) -> bool,
) -> std::result::Result<Locator, LocatorRefusal> {
    let input = input.trim();
    if input.is_empty() {
        return Err(LocatorRefusal::Empty);
    }
    if looks_like_path(input) {
        return Ok(Locator::Path(expand_home(input)));
    }
    let (locator, subdirectory) = match input.split_once('#') {
        Some((locator, sub)) => (locator, Some(PathBuf::from(sub))),
        None => (input, None),
    };
    let remote = |url: String, reference: Option<String>, short: bool| Locator::Remote {
        url,
        reference,
        subdirectory: subdirectory.clone(),
        short,
    };

    if let Some(scheme_end) = locator.find("://") {
        let (url, reference) = split_reference(locator, scheme_end + 3);
        return Ok(remote(url.to_owned(), reference, false));
    }
    if let Some((user, rest)) = locator.split_once('@')
        && !user.contains('/')
        && !user.contains(':')
        && rest.contains(':')
    {
        let at = user.len() + 1 + rest.find(':').unwrap_or_default() + 1;
        let (url, reference) = split_reference(locator, at);
        return Ok(remote(url.to_owned(), reference, false));
    }
    if let Some((prefix, path)) = locator.split_once(':')
        && !prefix.contains('/')
    {
        let Some(base) = hosts.base(prefix) else {
            return Err(LocatorRefusal::UnknownPrefix {
                prefix: prefix.to_owned(),
                aliases: hosts.aliases(),
            });
        };
        let (path, reference) = split_reference(path, 0);
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Err(LocatorRefusal::Empty);
        }
        return Ok(remote(
            canonical(&format!("{}/{path}", base.trim_end_matches('/'))),
            reference,
            true,
        ));
    }
    let (path, reference) = split_reference(locator, 0);
    if !path.contains('/') {
        return Err(LocatorRefusal::BareWord {
            word: path.to_owned(),
            directory_exists: is_directory(path),
        });
    }
    let alias = hosts.default_alias();
    if is_directory(path) {
        return Err(LocatorRefusal::Ambiguous {
            input: path.to_owned(),
            alias,
        });
    }
    let base = hosts
        .base(&alias)
        .unwrap_or_else(|| "https://github.com".to_owned());
    Ok(remote(
        canonical(&format!(
            "{}/{}",
            base.trim_end_matches('/'),
            path.trim_matches('/')
        )),
        reference,
        true,
    ))
}

fn looks_like_path(input: &str) -> bool {
    matches!(input, "." | ".." | "~")
        || ["/", "./", "../", "~/"]
            .iter()
            .any(|prefix| input.starts_with(prefix))
}

fn expand_home(input: &str) -> PathBuf {
    match input.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            std::env::var_os("HOME").map(PathBuf::from).map_or_else(
                || PathBuf::from(input),
                |home| home.join(rest.trim_start_matches('/')),
            )
        }
        _ => PathBuf::from(input),
    }
}

/// `url@ref`, where the `@` must come after `from` — so the `git@` of an
/// `scp` URL, or userinfo in an authority, is never read as a reference.
fn split_reference(locator: &str, from: usize) -> (&str, Option<String>) {
    let tail = &locator[from..];
    let after_path = tail.find('/').unwrap_or(0);
    match tail[after_path..].rfind('@') {
        Some(at) => {
            let at = from + after_path + at;
            (&locator[..at], Some(locator[at + 1..].to_owned()))
        }
        None => (locator, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Table(Vec<(&'static str, &'static str)>, &'static str);

    impl HostAliases for Table {
        fn base(&self, alias: &str) -> Option<String> {
            self.0
                .iter()
                .find(|(name, _)| *name == alias)
                .map(|(_, base)| (*base).to_owned())
        }
        fn default_alias(&self) -> String {
            self.1.to_owned()
        }
        fn aliases(&self) -> Vec<String> {
            self.0.iter().map(|(name, _)| (*name).to_owned()).collect()
        }
    }

    fn built_in() -> Table {
        Table(BUILT_IN_HOSTS.to_vec(), DEFAULT_HOST)
    }

    fn parse(input: &str) -> std::result::Result<Locator, LocatorRefusal> {
        parse_locator(input, &built_in(), &|_| false)
    }

    fn remote(url: &str) -> Locator {
        Locator::Remote {
            url: url.to_owned(),
            reference: None,
            subdirectory: None,
            short: false,
        }
    }

    #[test]
    fn every_spelling_of_one_repository_is_one_identity() {
        for spelling in [
            "https://github.com/hiukky/ai",
            "https://github.com/hiukky/ai.git",
            "https://github.com/hiukky/ai/",
            "https://GitHub.com/hiukky/ai",
            "git@github.com:hiukky/ai.git",
            "git@github.com:hiukky/ai",
            "ssh://git@github.com/hiukky/ai.git",
        ] {
            assert_eq!(
                canonical(spelling),
                "https://github.com/hiukky/ai",
                "{spelling}"
            );
        }
    }

    #[test]
    fn the_path_keeps_its_case_and_its_groups() {
        assert_eq!(
            canonical("git@GitLab.com:Group/Sub/Plugins.git"),
            "https://gitlab.com/Group/Sub/Plugins"
        );
    }

    #[test]
    fn a_host_no_table_knows_is_reduced_the_same_way() {
        assert_eq!(
            canonical("git@git.acme.io:team/plugins.git"),
            "https://git.acme.io/team/plugins"
        );
    }

    #[test]
    fn an_ssh_url_with_a_port_or_another_user_is_kept() {
        for kept in [
            "ssh://git@git.internal:2222/team/plugins",
            "ssh://deploy@git.internal/team/plugins",
            "ssh://git.internal/team/plugins",
            "deploy@git.internal:team/plugins",
        ] {
            assert_eq!(canonical(kept), kept);
        }
    }

    #[test]
    fn local_and_non_forge_urls_are_kept() {
        for kept in ["/srv/market", "file:///srv/market", "git://host/repo"] {
            assert_eq!(canonical(kept), kept);
        }
    }

    #[test]
    fn same_repository_matches_spellings_and_directories() {
        assert!(same_repository(
            "git@github.com:hiukky/ai.git",
            "https://github.com/hiukky/ai"
        ));
        assert!(!same_repository(
            "https://github.com/hiukky/ai",
            "https://gitlab.com/hiukky/ai"
        ));
        let root = uze_testkit::temp::scratch("forge-same-dir");
        let spelled = format!("file://{}", root.display());
        assert!(same_repository(&spelled, &root.to_string_lossy()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_https_identity_is_tried_anonymously_then_with_credentials_then_over_ssh() {
        let attempts = transports("git@github.com:hiukky/ai.git").unwrap();
        assert_eq!(
            attempts,
            vec![
                Transport {
                    url: "https://github.com/hiukky/ai".to_owned(),
                    access: Access::Anonymous
                },
                Transport {
                    url: "https://github.com/hiukky/ai".to_owned(),
                    access: Access::Credentialed
                },
                Transport {
                    url: "git@github.com:hiukky/ai.git".to_owned(),
                    access: Access::Credentialed
                },
            ]
        );
        let labels: Vec<_> = attempts.iter().map(Transport::label).collect();
        assert_eq!(labels, ["https (anonymous)", "https (credentials)", "ssh"]);
    }

    #[test]
    fn an_ssh_identity_kept_as_written_is_tried_once() {
        let attempts = transports("ssh://git@git.internal:2222/team/plugins").unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].access, Access::Credentialed);
        assert_eq!(attempts[0].label(), "ssh");
    }

    #[test]
    fn an_ssh_destination_keeps_its_user_and_port() {
        assert_eq!(
            ssh_destination("git@github.com:hiukky/ai.git").as_deref(),
            Some("git@github.com")
        );
        assert_eq!(
            ssh_destination("ssh://deploy@git.internal:2222/team/plugins").as_deref(),
            Some("-p 2222 deploy@git.internal")
        );
        assert_eq!(ssh_destination("https://github.com/hiukky/ai"), None);
    }

    #[test]
    fn a_local_repository_is_reached_with_nothing() {
        for url in ["/srv/market", "file:///srv/market"] {
            let attempts = transports(url).unwrap();
            assert_eq!(attempts.len(), 1);
            assert_eq!(attempts[0].access, Access::Local);
        }
    }

    #[test]
    fn plain_http_reaches_only_this_machine() {
        assert!(transports("http://git.acme.io/team/plugins").is_err());
        let attempts = transports("http://127.0.0.1:8080/team/plugins").unwrap();
        assert!(attempts[0].plain_http());
        assert_eq!(attempts[2].url, "git@127.0.0.1:team/plugins.git");
    }

    #[test]
    fn a_short_locator_names_the_default_host() {
        assert_eq!(
            parse("hiukky/ai").unwrap(),
            Locator::Remote {
                url: "https://github.com/hiukky/ai".to_owned(),
                reference: None,
                subdirectory: None,
                short: true
            }
        );
        let gitlab = Table(BUILT_IN_HOSTS.to_vec(), "gitlab");
        let Locator::Remote { url, .. } = parse_locator("hiukky/ai", &gitlab, &|_| false).unwrap()
        else {
            panic!("a repository")
        };
        assert_eq!(url, "https://gitlab.com/hiukky/ai");
    }

    #[test]
    fn a_prefix_names_its_host_and_keeps_reference_and_subdirectory() {
        assert_eq!(
            parse("gitlab:group/sub/plugins@v2#market").unwrap(),
            Locator::Remote {
                url: "https://gitlab.com/group/sub/plugins".to_owned(),
                reference: Some("v2".to_owned()),
                subdirectory: Some(PathBuf::from("market")),
                short: true
            }
        );
    }

    #[test]
    fn an_alias_for_an_organisation_resolves_under_its_base() {
        let table = Table(vec![("oss", "https://codeberg.org/my-org")], "oss");
        let Locator::Remote { url, .. } = parse_locator("oss:plugins", &table, &|_| false).unwrap()
        else {
            panic!("a repository")
        };
        assert_eq!(url, "https://codeberg.org/my-org/plugins");
        assert_eq!(
            ssh_endpoint(&url).as_deref(),
            Some("git@codeberg.org:my-org/plugins.git")
        );
    }

    #[test]
    fn an_scp_url_is_remote_and_its_user_is_not_a_reference() {
        assert_eq!(
            parse("git@github.com:hiukky/ai.git").unwrap(),
            remote("git@github.com:hiukky/ai.git")
        );
        assert_eq!(
            parse("git@github.com:hiukky/ai.git@v1").unwrap(),
            Locator::Remote {
                url: "git@github.com:hiukky/ai.git".to_owned(),
                reference: Some("v1".to_owned()),
                subdirectory: None,
                short: false
            }
        );
    }

    #[test]
    fn a_url_keeps_its_reference_after_the_path() {
        assert_eq!(
            parse("https://github.com/hiukky/ai@main").unwrap(),
            Locator::Remote {
                url: "https://github.com/hiukky/ai".to_owned(),
                reference: Some("main".to_owned()),
                subdirectory: None,
                short: false
            }
        );
    }

    #[test]
    fn a_path_looks_like_one() {
        for input in ["/srv/market", "./ai", "../ai", ".", ".."] {
            assert_eq!(parse(input).unwrap(), Locator::Path(PathBuf::from(input)));
        }
    }

    #[test]
    fn a_bare_word_is_refused_with_the_path_spelling() {
        assert_eq!(
            parse_locator("ai", &built_in(), &|_| true),
            Err(LocatorRefusal::BareWord {
                word: "ai".to_owned(),
                directory_exists: true
            })
        );
        let message = LocatorRefusal::BareWord {
            word: "ai".to_owned(),
            directory_exists: true,
        }
        .to_string();
        assert!(message.contains("./ai"), "{message}");
    }

    #[test]
    fn a_short_locator_that_is_also_a_directory_is_ambiguous() {
        let refusal = parse_locator("hiukky/ai", &built_in(), &|path| path == "hiukky/ai")
            .unwrap_err()
            .to_string();
        assert!(refusal.contains("./hiukky/ai"), "{refusal}");
        assert!(refusal.contains("github:hiukky/ai"), "{refusal}");
    }

    #[test]
    fn an_unknown_prefix_is_refused() {
        for input in ["gitlub:hiukky/ai", "github.com:hiukky/ai"] {
            assert!(
                matches!(parse(input), Err(LocatorRefusal::UnknownPrefix { .. })),
                "{input}"
            );
        }
    }
}
