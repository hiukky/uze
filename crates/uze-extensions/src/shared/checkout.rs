//! A checkout, as a surface's title says it.
//!
//! Two surfaces open on one and name it, and a title is the one part of
//! a surface a reader compares *across* surfaces: whichever one is up,
//! the answer to "which checkout am I in, and on what" has to be the
//! same sentence in the same place. Said twice, it stops being said the
//! same way, and then the reader works it out twice.

use std::path::Path;

use crate::{
    Host,
    view::{Role, Span},
};

/// The branch the checkout is on. Answers `detached HEAD` for a checkout
/// with no branch, and nothing at all outside a repository.
pub fn branch_of(host: &dyn Host, root: &Path) -> String {
    match host.git(root, &["rev-parse", "--abbrev-ref", "HEAD"], &[]) {
        Ok(name) if !name.trim().is_empty() && name.trim() != "HEAD" => name.trim().to_owned(),
        Ok(_) => "detached HEAD".to_owned(),
        Err(_) => String::new(),
    }
}

/// What the surface is, which checkout it is on, and which branch that
/// checkout is at — in that order, and told apart by weight.
///
/// The three are not equally interesting. The name is a label and is
/// said once; the directories leading to the checkout are context; the
/// checkout's own name and its branch are what identify it, and they are
/// what the eye should land on. One run of text gave all three the same
/// weight, which is how a title stops being read.
pub fn title(surface: &str, display_root: &str, branch: &str) -> Vec<Span> {
    let (parent, name) = match display_root.rsplit_once('/') {
        Some((parent, name)) => (format!("{parent}/"), name.to_owned()),
        None => (String::new(), display_root.to_owned()),
    };
    let mut spans = vec![
        Span::new(surface.to_owned(), Role::Muted),
        Span::new(" · ", Role::Faint),
        Span::new(parent, Role::Dim),
        Span::new(name, Role::Bright).bold(),
    ];
    if !branch.is_empty() {
        spans.push(Span::new(" · ", Role::Faint));
        spans.push(Span::new(branch.to_owned(), Role::Accent).bold());
    }
    spans
}
