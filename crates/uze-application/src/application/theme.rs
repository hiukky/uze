//! Selecting a theme.
//!
//! The facade's whole interest in appearance is an id and a list of files.
//! Resolving that id into colours and glyphs is the design system's job, and
//! deliberately not routed through here: a read model that carried resolved
//! colours would put the vocabulary in the domain's public surface, where
//! every token added later becomes a change to this crate.

use serde::Serialize;
use uze_core::{Result, theme_state};

use super::services::Themes;

/// A theme the operator can select.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ThemeSummary {
    /// What `uze theme use` takes. A file's own stem, or a built-in's name.
    pub id: String,
    pub active: bool,
    /// `None` for a theme UZE carries rather than one someone wrote.
    pub path: Option<std::path::PathBuf>,
}

impl Themes<'_> {
    /// Every theme this machine can load: the ones UZE carries, then the
    /// ones the operator wrote. A file that shadows a built-in's name wins,
    /// the way a local override should — and is listed once, as theirs.
    pub fn list(&self, builtin: &[&str]) -> Result<Vec<ThemeSummary>> {
        let active = self.active()?;
        let written = theme_state::available(&self.0.home)?;
        let shadowed: Vec<&str> = written.iter().map(|(id, _)| id.as_str()).collect();
        let summaries = builtin
            .iter()
            .filter(|id| !shadowed.contains(*id))
            .map(|id| ThemeSummary {
                id: (*id).to_owned(),
                active: false,
                path: None,
            })
            .chain(written.iter().map(|(id, path)| ThemeSummary {
                id: id.clone(),
                active: false,
                path: Some(path.clone()),
            }))
            .map(|summary| ThemeSummary {
                active: active.as_deref() == Some(summary.id.as_str()),
                ..summary
            });
        Ok(summaries.collect())
    }

    /// The selected theme's id, or `None` while the operator has not chosen.
    pub fn active(&self) -> Result<Option<String>> {
        theme_state::active(&self.0.home)
    }

    /// The file a written theme lives in, or `None` when the id names a
    /// built-in (or nothing at all).
    pub fn path_of(&self, id: &str) -> Result<Option<std::path::PathBuf>> {
        Ok(theme_state::available(&self.0.home)?
            .into_iter()
            .find(|(candidate, _)| candidate == id)
            .map(|(_, path)| path))
    }

    /// Records the selection. Does not validate that the id names anything:
    /// only the design system can say whether a theme resolves, and it says
    /// so by loading it.
    pub fn select(&self, id: &str) -> Result<()> {
        theme_state::set_active(&self.0.home, id)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::{UzeApplication, UzeHome};

    /// Named by `src/command_performance.rs` for `theme list`/`use`/`show`.
    ///
    /// Nothing here probes a harness or resolves a theme, so what this
    /// actually guards is that it stays that way: the day selecting a theme
    /// starts touching detection, this is what says so.
    #[test]
    fn theme_selection_meets_the_performance_budget() {
        const BUDGET: Duration = Duration::from_millis(50);

        let root = uze_testkit::temp::scratch("theme-perf");
        let home = UzeHome::at(&root);
        std::fs::create_dir_all(home.themes_dir()).expect("themes dir");
        for index in 0..32 {
            std::fs::write(home.themes_dir().join(format!("theme-{index}.json")), "{}")
                .expect("theme file");
        }
        let app = UzeApplication::new(home, Vec::new());
        app.themes().select("theme-7").expect("selected");

        let started = Instant::now();
        let listed = app.themes().list(&["default", "ascii"]).expect("listed");
        let active = app.themes().active().expect("active");
        let path = app.themes().path_of("theme-7").expect("path");
        let elapsed = started.elapsed();

        assert_eq!(listed.len(), 34);
        assert_eq!(active.as_deref(), Some("theme-7"));
        assert!(path.is_some());
        assert!(
            elapsed < BUDGET,
            "theme selection took {elapsed:?}, budget is {BUDGET:?}"
        );
    }

    #[test]
    fn a_theme_the_operator_wrote_shadows_a_builtin_of_the_same_name() {
        let root = uze_testkit::temp::scratch("theme-shadow");
        let home = UzeHome::at(&root);
        std::fs::create_dir_all(home.themes_dir()).expect("themes dir");
        std::fs::write(home.themes_dir().join("ascii.json"), "{}").expect("theme file");
        let app = UzeApplication::new(home, Vec::new());

        let listed = app.themes().list(&["default", "ascii"]).expect("listed");
        let ascii: Vec<&super::ThemeSummary> =
            listed.iter().filter(|theme| theme.id == "ascii").collect();
        assert_eq!(ascii.len(), 1, "listed twice: {listed:?}");
        assert!(ascii[0].path.is_some(), "the built-in won over their file");
    }
}
