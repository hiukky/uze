use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{
    error::{Result, UzeError},
    store::PackageId,
};

/// The sole authority for UZE-owned filesystem locations.
///
/// The CLI resolves `UZE_HOME` once through `from_env`. Tests and embedded
/// callers should prefer `at` so they never mutate a process-global variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UzeHome {
    root: PathBuf,
}

impl UzeHome {
    pub fn from_env() -> Result<Self> {
        Self::from_values(env::var_os("UZE_HOME"), env::var_os("HOME"))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn store_dir(&self) -> PathBuf {
        self.root.join("store")
    }

    pub fn packages_dir(&self) -> PathBuf {
        self.store_dir().join("packages")
    }

    pub fn package_dir(&self, id: &PackageId) -> PathBuf {
        self.packages_dir().join(id.as_str())
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join("state")
    }

    pub fn registry_path(&self) -> PathBuf {
        self.state_dir().join("packages.json")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn runtime_dir(&self) -> PathBuf {
        self.root.join("runtime")
    }

    pub fn runtime_session_dir(&self, integration: &str, session: &str) -> PathBuf {
        self.runtime_dir().join(integration).join(session)
    }

    pub fn ensure_layout(&self) -> Result<()> {
        for directory in [
            self.packages_dir(),
            self.state_dir(),
            self.cache_dir(),
            self.runtime_dir(),
        ] {
            fs::create_dir_all(&directory).map_err(|source| UzeError::Write {
                path: directory,
                source,
            })?;
        }
        Ok(())
    }

    fn from_values(
        uze_home: Option<std::ffi::OsString>,
        home: Option<std::ffi::OsString>,
    ) -> Result<Self> {
        if let Some(path) = uze_home {
            return Ok(Self::at(path));
        }
        let home = home.ok_or(UzeError::MissingHomeDirectory)?;
        Ok(Self::at(PathBuf::from(home).join(".uze")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_uze_home_wins_over_default_home() {
        let home = UzeHome::from_values(
            Some("/tmp/explicit-uze".into()),
            Some("/tmp/user-home".into()),
        )
        .unwrap();
        assert_eq!(home.root(), Path::new("/tmp/explicit-uze"));
    }

    #[test]
    fn default_home_is_derived_only_when_uze_home_is_missing() {
        let home = UzeHome::from_values(None, Some("/tmp/user-home".into())).unwrap();
        assert_eq!(home.root(), Path::new("/tmp/user-home/.uze"));
    }
}
