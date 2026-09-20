//! What the files half needs the machine to do, and what came back.
//!
//! Its own module because it is the whole of this extension's contact with
//! the outside: everything else here is a pure function of what these
//! answers carried. Keeping the reach in one file is what makes "none of
//! this may run on the thread that draws" a property one call site has,
//! rather than a rule spread over the extension.

use std::path::{Path, PathBuf};

use crate::{DirEntry, Host, view::Rgb};

/// How much of a file is coloured before it is shown.
///
/// Colouring costs about a tenth of a millisecond per line — a rate
/// nobody notices over a screenful and half a second over a large file,
/// which is how opening one came to feel like waiting for it. So the
/// read colours a glance's worth and the surface draws that; the rest
/// arrives through [`FileRequest::Colour`], which runs while the file is
/// already on screen and is asked for only when the source is what the
/// viewer is looking at.
///
/// Generous rather than exact: it covers several screenfuls, so the
/// second pass has landed long before scrolling could reach the end of
/// the first.
const GLANCE: usize = 400;

/// One thing the view needs done to the filesystem.
///
/// Named rather than performed: see the module doc. The host takes these
/// one at a time from [`super::CodeView::take_request`], runs [`fulfill`]
/// wherever it likes, and hands the result to [`super::CodeView::absorb`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileRequest {
    /// Read a directory's entries — on opening it, and again after a
    /// delete, so the tree says what is there rather than what was.
    List(PathBuf),
    /// Read a file, colouring a glance's worth of it.
    Read(PathBuf),
    /// Colour all of one already read — the rest of the work
    /// [`FileRequest::Read`] left, asked for once the source is what is
    /// being shown.
    Colour(PathBuf),
    Save {
        path: PathBuf,
        contents: String,
    },
    Delete(PathBuf),
}

/// What the host did about a [`FileRequest`].
///
/// Each carries the path it is about, so an answer that arrives after the
/// viewer moved on is dropped rather than drawn over what they moved to.
#[derive(Clone, Debug)]
pub enum FileAnswer {
    Listed {
        path: PathBuf,
        entries: Result<Vec<DirEntry>, String>,
    },
    Read {
        path: PathBuf,
        file: Result<LoadedFile, String>,
    },
    /// The same file, coloured all the way through.
    ///
    /// Its own variant rather than a second [`FileAnswer::Read`] because
    /// the two answer different questions, and a failure means different
    /// things: a read that failed is a file the viewer cannot see, which
    /// the surface says out loud; a colouring that failed leaves a file
    /// they are already reading exactly as it is, and is worth nothing
    /// but silence.
    Coloured {
        path: PathBuf,
        file: Result<LoadedFile, String>,
    },
    Saved {
        path: PathBuf,
        outcome: Result<(), String>,
    },
    Deleted {
        path: PathBuf,
        outcome: Result<(), String>,
    },
}

/// A file as the view holds it: the text, and that text highlighted.
///
/// Both, because they answer different questions — the text is what an
/// edit changes and what a save writes; the highlighting is what the host
/// draws, and producing it is the expensive half that must not happen on
/// the frame's thread.
#[derive(Clone, Debug)]
pub struct LoadedFile {
    pub(super) text: String,
    /// One entry per line, for as many lines as were coloured. Shorter
    /// than the text when only a glance was — the renderer draws a line
    /// with no entry as it stands, which is what makes a partial
    /// colouring a complete file.
    pub(super) highlighted: Vec<Vec<(Rgb, String)>>,
    /// Whether `highlighted` reaches the last line.
    pub(super) complete: bool,
    /// The theme it was highlighted against, kept so a line changed by
    /// typing can be recoloured in the same palette as its neighbours.
    pub(super) theme: String,
}

/// What a request that never ran answers.
///
/// The surface reserves itself against a second request while one is out
/// and releases it on the answer, so a host whose read ended without
/// answering would leave the files half accepting nothing for the rest of
/// the session. Every variant here is a state the view already draws —
/// the refusal shape of the request that was asked.
pub fn unanswered(request: &FileRequest, reason: &str) -> FileAnswer {
    match request {
        FileRequest::List(path) => FileAnswer::Listed {
            path: path.clone(),
            entries: Err(reason.to_owned()),
        },
        FileRequest::Read(path) => FileAnswer::Read {
            path: path.clone(),
            file: Err(reason.to_owned()),
        },
        FileRequest::Colour(path) => FileAnswer::Coloured {
            path: path.clone(),
            file: Err(reason.to_owned()),
        },
        FileRequest::Save { path, .. } => FileAnswer::Saved {
            path: path.clone(),
            outcome: Err(reason.to_owned()),
        },
        FileRequest::Delete(path) => FileAnswer::Deleted {
            path: path.clone(),
            outcome: Err(reason.to_owned()),
        },
    }
}

/// Runs one [`FileRequest`] against the host.
///
/// The only function in this module that reaches anything, which is what
/// makes "this must not run on the drawing thread" a property of one call
/// site rather than a rule to remember.
pub fn fulfill(host: &dyn Host, request: FileRequest) -> FileAnswer {
    match request {
        FileRequest::List(path) => FileAnswer::Listed {
            entries: host.list_dir(&path),
            path,
        },
        FileRequest::Read(path) => {
            // A document opens as the document it is (see
            // `CodeView::read_selection_as_what_it_is`), and the preview
            // colours its own fenced blocks — so colouring its markup
            // here is work for a screen nobody asked for. Asking to read
            // it as source is what pays for that, through `Colour`.
            let glance = match crate::code::markdown::is_markdown(&path) {
                true => 0,
                false => GLANCE,
            };
            let file = read_and_colour(host, &path, glance);
            FileAnswer::Read { path, file }
        }
        FileRequest::Colour(path) => {
            let file = read_and_colour(host, &path, usize::MAX);
            FileAnswer::Coloured { path, file }
        }
        FileRequest::Save { path, contents } => FileAnswer::Saved {
            outcome: host.write_file(&path, &contents),
            path,
        },
        FileRequest::Delete(path) => FileAnswer::Deleted {
            outcome: host.delete_file(&path),
            path,
        },
    }
}

/// Reads `path` and colours its first `lines` lines.
///
/// Whatever the host could not give us — a binary, a broken symlink, one
/// the operator may not read, one too large to hold — travels as the
/// sentence the view puts where the content would be. Only the host can
/// tell them apart.
fn read_and_colour(host: &dyn Host, path: &Path, lines: usize) -> Result<LoadedFile, String> {
    let theme = host.syntax_theme();
    host.read_file(path).map(|text| {
        let highlighted = crate::code::highlight::lines(&text, path, &theme, lines);
        LoadedFile {
            complete: highlighted.len() >= text.lines().count(),
            text,
            highlighted,
            theme,
        }
    })
}

#[cfg(test)]
impl LoadedFile {
    /// Uncoloured text, for a test that is about what an answer *does*
    /// rather than about how it looks.
    pub(super) fn of(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            highlighted: Vec::new(),
            complete: true,
            theme: String::new(),
        }
    }
}
