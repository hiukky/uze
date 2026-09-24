//! The host aliases a person types instead of a URL.
//!
//! Input only: `gitlab:group/repo` and a bare `owner/repo` become full URLs
//! at the prompt, and the table plays no part in what a repository is
//! called ([`forge::canonical`]) or how it is reached
//! ([`forge::transports`]). So no project may depend on a machine holding
//! a particular alias, and the table lives with the machine.
//!
//! The built-ins are code, not bytes: the record holds only what the
//! operator added, and which alias is the default.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    acquisition::forge::{self, BUILT_IN_HOSTS, DEFAULT_HOST, HostAliases},
    error::{Result, UzeError},
    home::UzeHome,
};

/// Words a URL's scheme is spelled with, which an alias would shadow at the
/// one place it is read: before the `:`.
const RESERVED: &[&str] = &["http", "https", "ssh", "git", "file"];

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct HostRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    aliases: BTreeMap<String, String>,
}

/// A record: which forge a person meant by an alias is known nowhere else.
impl uze_document::Shaped for HostRecord {
    const SHAPE: u32 = uze_document::FIRST_SHAPE;
    const KIND: &'static str = "hosts";
}

/// One alias, as the table lists it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HostEntry {
    pub alias: String,
    pub base: String,
    pub built_in: bool,
    pub default: bool,
}

/// The aliases this machine resolves: the built-ins and the operator's own.
#[derive(Clone, Debug)]
pub struct Hosts {
    record: HostRecord,
}

impl Hosts {
    pub fn entries(&self) -> Vec<HostEntry> {
        let default = self.default_alias();
        BUILT_IN_HOSTS
            .iter()
            .map(|(alias, base)| ((*alias).to_owned(), (*base).to_owned(), true))
            .chain(
                self.record
                    .aliases
                    .iter()
                    .map(|(alias, base)| (alias.clone(), base.clone(), false)),
            )
            .map(|(alias, base, built_in)| HostEntry {
                default: alias == default,
                alias,
                base,
                built_in,
            })
            .collect()
    }
}

impl HostAliases for Hosts {
    fn base(&self, alias: &str) -> Option<String> {
        BUILT_IN_HOSTS
            .iter()
            .find(|(name, _)| *name == alias)
            .map(|(_, base)| (*base).to_owned())
            .or_else(|| self.record.aliases.get(alias).cloned())
    }

    fn default_alias(&self) -> String {
        self.record
            .default
            .clone()
            .filter(|alias| self.base(alias).is_some())
            .unwrap_or_else(|| DEFAULT_HOST.to_owned())
    }

    fn aliases(&self) -> Vec<String> {
        self.entries()
            .into_iter()
            .map(|entry| entry.alias)
            .collect()
    }
}

pub fn load(home: &UzeHome) -> Result<Hosts> {
    Ok(Hosts {
        record: read(home)?,
    })
}

/// Makes `alias` the one a bare `owner/repo` resolves against.
pub fn set_default(home: &UzeHome, alias: &str) -> Result<()> {
    let mut record = read(home)?;
    if (Hosts {
        record: record.clone(),
    })
    .base(alias)
    .is_none()
    {
        return Err(UzeError::HostAlias(format!(
            "`{alias}` is not a host alias on this machine; define it with \
             `uze market host {alias} <https-url>`"
        )));
    }
    record.default = (alias != DEFAULT_HOST).then(|| alias.to_owned());
    write(home, &record)
}

/// Defines `alias` as `base`, or repoints an alias the operator defined.
pub fn define(home: &UzeHome, alias: &str, base: &str) -> Result<()> {
    validate_alias(alias)?;
    let base = validate_base(base)?;
    let mut record = read(home)?;
    record.aliases.insert(alias.to_owned(), base);
    write(home, &record)
}

/// Removes an alias the operator defined. `Ok(true)` when it was the
/// default, which is then `github` again: a default naming nothing would
/// fail every short locator later instead of now.
pub fn remove(home: &UzeHome, alias: &str) -> Result<bool> {
    if is_built_in(alias) {
        return Err(UzeError::HostAlias(format!(
            "`{alias}` is built into UZE and cannot be removed"
        )));
    }
    let mut record = read(home)?;
    if record.aliases.remove(alias).is_none() {
        return Err(UzeError::HostAlias(format!(
            "`{alias}` is not a host alias on this machine"
        )));
    }
    let was_default = record.default.as_deref() == Some(alias);
    if was_default {
        record.default = None;
    }
    write(home, &record)?;
    Ok(was_default)
}

fn is_built_in(alias: &str) -> bool {
    BUILT_IN_HOSTS.iter().any(|(name, _)| *name == alias)
}

fn validate_alias(alias: &str) -> Result<()> {
    if is_built_in(alias) {
        return Err(UzeError::HostAlias(format!(
            "`{alias}` is built into UZE and cannot be redefined"
        )));
    }
    if RESERVED.contains(&alias) {
        return Err(UzeError::HostAlias(format!(
            "`{alias}` is how a URL's scheme is spelled, so it cannot be an alias"
        )));
    }
    if alias.is_empty()
        || !alias
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(UzeError::HostAlias(format!(
            "`{alias}` is not an alias name; use lowercase letters, digits and `-`"
        )));
    }
    Ok(())
}

/// An HTTPS base, or plain HTTP to this machine, with no credential in it.
fn validate_base(base: &str) -> Result<String> {
    let base = base.trim().trim_end_matches('/');
    let https = base.starts_with("https://");
    let loopback_http = base.starts_with("http://") && forge::is_loopback(base);
    if !https && !loopback_http {
        return Err(UzeError::HostAlias(format!(
            "`{base}` is not an https:// URL; an alias names where a forge serves HTTPS"
        )));
    }
    crate::acquisition::git::reject_inline_credentials(base)?;
    if forge::host_of(base).is_none() {
        return Err(UzeError::HostAlias(format!("`{base}` names no host")));
    }
    Ok(base.to_owned())
}

fn read(home: &UzeHome) -> Result<HostRecord> {
    Ok(uze_document::read::<HostRecord>(&home.hosts_path())?.or_default())
}

fn write(home: &UzeHome, record: &HostRecord) -> Result<()> {
    home.ensure_layout()?;
    let payload = serde_json::to_vec_pretty(record).expect("a host record is serializable");
    crate::persistence::write_atomic(&home.hosts_path(), &payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(label: &str) -> UzeHome {
        UzeHome::at(uze_testkit::temp::scratch(label))
    }

    #[test]
    fn a_fresh_machine_has_the_built_ins_and_github_as_default() {
        let hosts = load(&home("hosts-fresh")).unwrap();
        assert_eq!(hosts.default_alias(), "github");
        assert_eq!(hosts.base("gitlab").as_deref(), Some("https://gitlab.com"));
    }

    #[test]
    fn an_alias_is_defined_chosen_and_removed() {
        let home = home("hosts-own");
        define(&home, "work", "https://git.acme.io/").unwrap();
        set_default(&home, "work").unwrap();
        let hosts = load(&home).unwrap();
        assert_eq!(hosts.base("work").as_deref(), Some("https://git.acme.io"));
        assert_eq!(hosts.default_alias(), "work");

        assert!(remove(&home, "work").unwrap(), "it was the default");
        let hosts = load(&home).unwrap();
        assert_eq!(hosts.base("work"), None);
        assert_eq!(hosts.default_alias(), "github");
    }

    #[test]
    fn a_built_in_is_neither_repointed_nor_removed() {
        let home = home("hosts-built-in");
        assert!(define(&home, "github", "https://git.evil.example").is_err());
        assert!(remove(&home, "github").is_err());
        assert_eq!(
            load(&home).unwrap().base("github").as_deref(),
            Some("https://github.com")
        );
    }

    #[test]
    fn a_built_in_can_be_the_default() {
        let home = home("hosts-default-built-in");
        set_default(&home, "gitlab").unwrap();
        assert_eq!(load(&home).unwrap().default_alias(), "gitlab");
    }

    #[test]
    fn names_and_bases_are_validated() {
        let home = home("hosts-validate");
        for alias in ["Work", "my_host", "https", "git", ""] {
            assert!(
                define(&home, alias, "https://git.acme.io").is_err(),
                "{alias}"
            );
        }
        for base in [
            "http://git.acme.io",
            "git.acme.io",
            "https://user:token@git.acme.io",
        ] {
            assert!(define(&home, "work", base).is_err(), "{base}");
        }
        define(&home, "local", "http://127.0.0.1:8080").unwrap();
    }
}
