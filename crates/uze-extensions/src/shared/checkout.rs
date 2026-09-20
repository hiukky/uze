//! A checkout, as a surface's frame says it.
//!
//! Two surfaces open on one and name it, and this is the one part of a
//! surface a reader compares *across* surfaces: whichever one is up, the
//! answer to "which checkout am I in, and on what" has to be the same
//! sentence in the same place. Said twice, it stops being said the same
//! way, and then the reader works it out twice.
//!
//! It is two lines on two edges, not one. What the surface *is* belongs
//! at the top, where a title goes and where it is read once. Where it is
//! *open* belongs at the foot, because that is the question a reader
//! comes back to — and a title that answered both put the thing said
//! once and the thing looked up repeatedly in one run of text, at which
//! point neither is found quickly.

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

/// What the surface is: the title along the top, said once and quietly.
/// A label, not a heading — by the time it is read the surface is
/// already open, and what it names is the one thing the reader cannot be
/// in doubt about.
///
/// `surface` is the extension's own
/// [`BuiltinExtension::name`](crate::registry::BuiltinExtension::name),
/// never a string typed here: the Extensions screen, the tab strip's
/// chip and this frame are three places one extension is named, and an
/// extension named twice is an extension named two ways.
pub fn name(surface: &str) -> Vec<Span> {
    vec![Span::new(surface.to_owned(), Role::Muted)]
}

/// Which checkout the surface is open on and which branch it is at: the
/// line along the bottom edge, told apart by weight.
///
/// The parts are not equally interesting. The directories leading to the
/// checkout are context; the checkout's own name and its branch are what
/// identify it, and they are what the eye should land on. One run of
/// text gave all of it the same weight, which is how a line stops being
/// read.
pub fn caption(display_root: &str, branch: &str) -> Vec<Span> {
    let (parent, name) = match display_root.rsplit_once('/') {
        Some((parent, name)) => (format!("{parent}/"), name.to_owned()),
        None => (String::new(), display_root.to_owned()),
    };
    let mut spans = vec![
        Span::new(parent, Role::Dim),
        Span::new(name, Role::Bright).bold(),
    ];
    if !branch.is_empty() {
        spans.push(Span::new(" · ", Role::Faint));
        spans.push(Span::new(branch.to_owned(), Role::Accent).bold());
    }
    spans
}

#[cfg(test)]
mod tests {
    use crate::{architect, architect::ArchitectView, code, view::Size};

    /// Both surfaces say a checkout in the same words, and differ only in
    /// the name on top. Held because "they look alike today" is not the
    /// claim — the claim is that they are built the same way, and only
    /// that one survives an edit to either surface.
    #[test]
    fn two_surfaces_open_on_one_checkout_say_it_identically() {
        let space = Size {
            width: 80,
            height: 24,
        };
        let root = "~/uze/.worktrees/joipv0";
        let code = code::view(
            &code::CodeView::opening(
                std::path::PathBuf::from("/repo"),
                root.to_owned(),
                code::ContentMode::Contents,
            ),
            space,
        );
        let architect = architect::view(&ArchitectView::opening(root.to_owned()), space);
        assert_eq!(code.caption, architect.caption);
        assert!(!code.caption.is_empty(), "and it is actually said");
        assert_ne!(code.title, architect.title, "only the name differs");
    }
}
