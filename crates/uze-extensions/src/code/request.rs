//! What the explorer needs the machine to do, and what came back.
//!
//! Its own module because it is the whole of this extension's contact with
//! the outside: everything else here is a pure function of what these
//! answers carried. Keeping the reach in one file is what makes "none of
//! this may run on the thread that draws" a property one call site has,
//! rather than a rule spread over the extension.

use std::path::PathBuf;

use crate::{DirEntry, Host, view::Rgb};

/// One thing the view needs done to the filesystem.
///
/// Named rather than performed: see the module doc. The host takes these
/// one at a time from [`ExplorerView::take_request`], runs [`fulfill`]
/// wherever it likes, and hands the result to [`ExplorerView::absorb`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileRequest {
    /// Read a directory's entries — on opening it, and again after a
    /// delete, so the tree says what is there rather than what was.
    List(PathBuf),
    /// Read and highlight a file.
    Read(PathBuf),
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
    pub(super) highlighted: Vec<Vec<(Rgb, String)>>,
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
            let theme = host.syntax_theme();
            // Whatever the host could not give us — a binary, a broken
            // symlink, one the operator may not read, one too large to
            // hold — travels as the sentence the view puts where the
            // content would be. Only the host can tell them apart.
            let file = host.read_file(&path).map(|text| {
                let highlighted = crate::shared::highlight::lines(&text, &path, &theme);
                LoadedFile {
                    text,
                    highlighted,
                    theme,
                }
            });
            FileAnswer::Read { path, file }
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

#[cfg(test)]
impl LoadedFile {
    /// Uncoloured text, for a test that is about what an answer *does*
    /// rather than about how it looks.
    pub(super) fn of(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            highlighted: Vec::new(),
            theme: String::new(),
        }
    }
}

impl LoadedFile {
    /// Hands the three pieces to whoever installs them. The fields stay
    /// private so nothing but the buffer can hold half of a file.
    pub(super) fn into_parts(self) -> (String, Vec<Vec<(Rgb, String)>>, String) {
        (self.text, self.highlighted, self.theme)
    }
}
