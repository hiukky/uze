//! The operator's own settings: one TOML file, one section per concern.
//!
//! Each choice used to be a JSON record of its own under `state/`, most of
//! them a line long, in a directory an operator is right to treat as not
//! theirs to edit. These are the opposite: choices someone makes and may
//! want to make by hand, so they live beside `keys.json` and
//! `theme-overrides.json` at the root, in a format with comments.
//!
//! This module knows files and sections, never what a setting means: a
//! concern (`theme_state`, `notification_state`) owns its section and reads
//! and writes it through here. A write touches only the key it names and
//! leaves the rest of the document — the operator's comments, ordering and
//! whatever sections this build does not know — exactly as it was.
//!
//! A file that does not parse is an error on read *and* on write: reading
//! on as if it were empty would silently undo every choice in it, and
//! writing over it would destroy the text the operator was in the middle of
//! fixing.

use std::fs;

use toml_edit::{DocumentMut, Item, Table, value};

use crate::{
    error::{Result, UzeError},
    home::UzeHome,
};

/// The string at `section.key`, or `None` when the operator never set it.
pub fn get(home: &UzeHome, section: &str, key: &str) -> Result<Option<String>> {
    let document = read(home)?;
    Ok(document
        .get(section)
        .and_then(|table| table.get(key))
        .and_then(Item::as_str)
        .map(str::to_owned))
}

pub fn set(home: &UzeHome, section: &str, key: &str, setting: &str) -> Result<()> {
    let mut document = read(home)?;
    let table = document
        .entry(section)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| malformed(home, format!("`{section}` is not a table")))?;
    table.insert(key, value(setting));
    write(home, &document)
}

fn read(home: &UzeHome) -> Result<DocumentMut> {
    let path = home.config_path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => return Err(UzeError::Read { path, source }),
    };
    text.parse::<DocumentMut>()
        .map_err(|error| malformed(home, error.to_string()))
}

fn write(home: &UzeHome, document: &DocumentMut) -> Result<()> {
    home.ensure_layout()?;
    crate::persistence::write_atomic(&home.config_path(), document.to_string().as_bytes())
}

fn malformed(home: &UzeHome, reason: String) -> UzeError {
    UzeError::MalformedConfig {
        path: home.config_path(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(label: &str) -> UzeHome {
        UzeHome::at(uze_testkit::temp::scratch(label))
    }

    #[test]
    fn an_unwritten_setting_is_unset() {
        let home = home("config-none");
        assert_eq!(get(&home, "appearance", "theme").expect("readable"), None);
    }

    #[test]
    fn a_write_keeps_everything_it_was_not_asked_to_change() {
        let home = home("config-preserve");
        fs::create_dir_all(home.config_path().parent().unwrap()).unwrap();
        let written = "# mine\n[appearance]\ntheme = \"nocturne\" # dark\n\n[future]\nknob = 3\n";
        fs::write(home.config_path(), written).unwrap();

        set(&home, "notifications", "agent_finished", "always").expect("written");

        let text = fs::read_to_string(home.config_path()).unwrap();
        assert!(
            text.starts_with(written),
            "the operator's text moved:\n{text}"
        );
        assert_eq!(
            get(&home, "notifications", "agent_finished").expect("readable"),
            Some("always".to_owned())
        );
    }

    #[test]
    fn a_file_that_does_not_parse_is_neither_read_through_nor_written_over() {
        let home = home("config-malformed");
        fs::create_dir_all(home.config_path().parent().unwrap()).unwrap();
        fs::write(home.config_path(), "[appearance\ntheme = ").unwrap();

        assert!(get(&home, "appearance", "theme").is_err());
        assert!(set(&home, "appearance", "theme", "default").is_err());
        assert_eq!(
            fs::read_to_string(home.config_path()).unwrap(),
            "[appearance\ntheme = "
        );
    }
}
