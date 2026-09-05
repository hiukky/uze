//! Which theme is active, and where a user's own themes live.
//!
//! Only the *selection* lives here. What a theme is — tokens, symbols, the
//! file format, how a partial one resolves — belongs to the design system,
//! which this crate knows nothing about: an id and a directory listing is
//! the whole of the domain's interest in appearance. That split is what
//! lets the vocabulary grow without the domain hearing about it.
//!
//! Machine-scoped, like Profiles: a project does not get to decide what the
//! operator's terminal looks like.

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, UzeError},
    home::UzeHome,
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ThemeSelection {
    active: Option<String>,
}

/// The theme the operator chose, or `None` while they have not chosen —
/// which is not an error state: the built-in default is what a fresh
/// installation draws with, and choosing is how you leave it.
pub fn active(home: &UzeHome) -> Result<Option<String>> {
    let path = home.active_theme_path();
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    let selection: ThemeSelection =
        serde_json::from_slice(&bytes).map_err(|source| UzeError::Json { path, source })?;
    Ok(selection.active)
}

pub fn set_active(home: &UzeHome, id: &str) -> Result<()> {
    home.ensure_layout()?;
    let payload = serde_json::to_vec_pretty(&ThemeSelection {
        active: Some(id.to_owned()),
    })
    .expect("theme selection serialization is infallible");
    crate::persistence::write_atomic(&home.active_theme_path(), &payload)
}

/// The theme files the operator has written, as `(id, path)` sorted by id.
/// The id is the file's own stem, which is what makes a theme selectable
/// without a registry to keep in step with the directory.
///
/// A themes directory that does not exist is an empty list, not an error:
/// having written no themes is the ordinary case.
pub fn available(home: &UzeHome) -> Result<Vec<(String, PathBuf)>> {
    let directory = home.themes_dir();
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(UzeError::Read {
                path: directory,
                source,
            });
        }
    };
    let mut themes: Vec<(String, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|path| {
            let id = path.file_stem()?.to_str()?.to_owned();
            Some((id, path))
        })
        .collect();
    themes.sort();
    Ok(themes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(label: &str) -> UzeHome {
        UzeHome::at(uze_testkit::temp::scratch(label))
    }

    #[test]
    fn no_selection_is_not_an_error() {
        let home = home("theme-none");
        assert_eq!(active(&home).expect("readable"), None);
    }

    #[test]
    fn a_selection_survives_being_written_and_read_back() {
        let home = home("theme-selection");
        set_active(&home, "nocturne").expect("written");
        assert_eq!(
            active(&home).expect("readable").as_deref(),
            Some("nocturne")
        );
        set_active(&home, "ascii").expect("written");
        assert_eq!(active(&home).expect("readable").as_deref(), Some("ascii"));
    }

    #[test]
    fn an_absent_themes_directory_lists_nothing_rather_than_failing() {
        let home = home("theme-empty");
        assert!(available(&home).expect("listable").is_empty());
    }

    #[test]
    fn a_theme_is_identified_by_its_own_filename() {
        let home = home("theme-listing");
        fs::create_dir_all(home.themes_dir()).expect("themes dir");
        fs::write(home.themes_dir().join("nocturne.json"), "{}").expect("theme");
        fs::write(home.themes_dir().join("dawn.json"), "{}").expect("theme");
        // Not a theme, and not listed as one.
        fs::write(home.themes_dir().join("notes.txt"), "").expect("stray file");

        let available = available(&home).expect("listable");
        let ids: Vec<&str> = available.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["dawn", "nocturne"]);
    }
}
