//! What the workspace client grants an extension.
//!
//! An extension holds no machine access of its own (see
//! `uze_extensions::Host`): it names what it needs, and this decides
//! whether to oblige. Today it always obliges, in this process, so nothing
//! observable changes — the point is that the grant now has a single, named
//! place, which is where a real capability model would go if extensions
//! were ever authored elsewhere.
//!
//! Read-only throughout. Nothing reachable from here writes anything.

use std::path::Path;

/// The workspace client's grant. Zero-sized: the capabilities are the
/// host's own, not per-extension state.
pub(crate) struct WorkspaceHost;

impl uze_extensions::Host for WorkspaceHost {
    /// Through `uze-git`'s read path, so an overlay refreshing every few
    /// seconds cannot contend with an agent writing in a sibling checkout.
    ///
    /// Exit `1` is an answer rather than a failure: `git diff` uses it for
    /// "there are differences", which is the ordinary case here.
    fn git(&self, root: &Path, args: &[&str]) -> Result<String, String> {
        uze_git::read(root, args)
            .map_err(|error| error.to_string())?
            .or_exit(1)
    }

    fn read_file(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn display_path(&self, path: &Path) -> String {
        crate::ui::display_project_path(path)
    }

    /// The active theme's, so highlighted content is drawn for the same
    /// background the chrome around it is.
    fn syntax_theme(&self) -> String {
        uze_theme::active().syntax_theme().to_owned()
    }
}
