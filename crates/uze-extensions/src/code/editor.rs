//! The buffer being typed into: a file split into lines, a caret, and the
//! edits that move both.
//!
//! Nothing here knows about the tree beside it or the view around it — it
//! is the text and what happens to the text, which is also why it is the
//! part of this extension with the most tests per line.

use std::{cell::RefCell, path::PathBuf};

use super::request::LoadedFile;
use crate::view::Command;
use crate::view::{Caret, ContentLine, Rgb};

/// How the file terminated its lines when it was read.
///
/// Carried because saving must reproduce it. An editor that rejoins with
/// `\n` turns opening a CRLF file into a whole-file diff the operator
/// never asked for, and one that always appends a terminator does the
/// same, one character at a time, to a file that deliberately ends
/// without one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct LineEndings {
    /// Every terminated line ended `\r\n`. False for a file that mixes
    /// them, whose `\r`s are then left in the line text rather than
    /// dropped — nothing this buffer saves may lose a byte it was given.
    carriage_return: bool,
    /// The text ended with a terminator.
    trailing: bool,
}

impl Default for LineEndings {
    /// What a buffer with no file behind it yet writes: the convention of
    /// every platform UZE runs on, and a final newline.
    fn default() -> Self {
        Self {
            carriage_return: false,
            trailing: true,
        }
    }
}

/// The file's lines without their terminators, and the terminators it
/// used — the two halves [`OpenFile::contents`] needs to put it back.
fn split_lines(text: &str) -> (Vec<String>, LineEndings) {
    let trailing = text.ends_with('\n');
    let mut pieces: Vec<&str> = text.split('\n').collect();
    if trailing {
        pieces.pop();
    }
    let terminated = if trailing {
        pieces.len()
    } else {
        pieces.len().saturating_sub(1)
    };
    let carriage_return =
        terminated > 0 && pieces[..terminated].iter().all(|line| line.ends_with('\r'));
    let lines = pieces
        .into_iter()
        .map(|line| match carriage_return {
            true => line.strip_suffix('\r').unwrap_or(line).to_owned(),
            false => line.to_owned(),
        })
        .collect();
    (
        lines,
        LineEndings {
            carriage_return,
            trailing,
        },
    )
}

/// The file being shown, and the state of editing it.
pub(super) struct OpenFile {
    pub(super) path: PathBuf,
    /// The editable truth. Split into lines because that is the unit both
    /// the caret and the renderer address; rejoined on save.
    pub(super) lines: Vec<String>,
    /// What rejoining them has to put back.
    endings: LineEndings,
    /// One entry per line of `lines`, kept in step through every edit.
    /// An empty entry is a line nothing has coloured yet, which the
    /// renderer draws as the text it is.
    pub(super) highlighted: Vec<Vec<(Rgb, String)>>,
    /// Whether some of `lines` is still uncoloured — what makes a second
    /// pass worth asking for (see [`super::FileRequest::Colour`]).
    pub(super) partial: bool,
    pub(super) theme: String,
    pub(super) caret: Caret,
    pub(super) editing: bool,
    /// Edited since the last save. What makes closing ask twice.
    pub(super) modified: bool,
    /// Whether the read that fills this in has landed yet.
    pub(super) loading: bool,
    /// Why it could not be shown, when it could not be.
    pub(super) error: Option<String>,
    /// How many times the text has changed. The identity the rendered
    /// preview is kept against — a number rather than the text itself,
    /// because comparing a document to decide whether to re-render it
    /// costs what rendering it was supposed to save.
    revision: u64,
    /// The last rendering of this buffer as the document it describes,
    /// and the revision and theme it was rendered from.
    ///
    /// Rendering a document is a parse and a highlighter per fenced
    /// block — a frame's whole budget for a file of any size, paid again
    /// on every frame because the preview is drawn from the buffer
    /// rather than from the read. So it is done when the buffer changes,
    /// not when the screen does. Behind a cell because a view is drawn
    /// through a shared reference: the host asks what to draw, and
    /// producing that answer is not a change to anything the viewer can
    /// see.
    preview: RefCell<Option<(u64, String, Vec<ContentLine>)>>,
}

impl OpenFile {
    pub(super) fn opening(path: PathBuf) -> Self {
        Self {
            path,
            lines: Vec::new(),
            endings: LineEndings::default(),
            highlighted: Vec::new(),
            partial: false,
            theme: String::new(),
            caret: Caret::default(),
            editing: false,
            modified: false,
            loading: true,
            error: None,
            revision: 0,
            preview: RefCell::new(None),
        }
    }

    /// The character `cell` display cells into `line`, clamped to its
    /// end.
    ///
    /// The other half of [`crate::view::ViewHit::PlaceCaret`]: the host
    /// counted cells because that is what it drew; this counts characters
    /// because that is what the text is made of. A double-width glyph is
    /// two cells and one character, and only this side can tell.
    pub(super) fn column_at_cell(&self, line: usize, cell: usize) -> usize {
        let Some(text) = self.lines.get(line) else {
            return 0;
        };
        let mut cells = 0usize;
        for (column, character) in text.chars().enumerate() {
            let width = unicode_width::UnicodeWidthChar::width(character)
                .unwrap_or(1)
                .max(1);
            // Landing anywhere inside a wide glyph means that glyph, not
            // the one after it.
            if cell < cells + width {
                return column;
            }
            cells += width;
        }
        text.chars().count()
    }

    /// Installs a file the host read, keeping a line to put the caret on
    /// out of it — an empty file still has one line, or there is nowhere
    /// to start typing.
    pub(super) fn install(&mut self, loaded: LoadedFile) {
        // Taken apart in one move, here and nowhere else: a buffer
        // holding half of one file and half of another is the one state
        // none of this can recover from.
        let LoadedFile {
            text,
            highlighted,
            complete,
            theme,
        } = loaded;
        let (lines, endings) = split_lines(&text);
        self.lines = lines;
        self.endings = endings;
        self.highlighted = highlighted;
        self.theme = theme;
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        // The highlighter walks the text with `str::lines`, which sees no
        // line at all in an empty file where the caret still needs one —
        // and it stops at a glance's worth of a long one. Either way every
        // line has an entry afterwards, because an edit inserts and
        // removes in both lists at once and they cannot drift apart.
        self.partial = !complete;
        self.highlighted.resize(self.lines.len(), Vec::new());
        self.modified = false;
        self.error = None;
        self.changed();
    }

    /// Records that the text is not what it was, so anything kept from
    /// the old one is known to be stale.
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// This buffer as the document it describes: how many lines it comes
    /// to, and `count` of them from `first`.
    ///
    /// Rendered once per change to the buffer rather than once per frame,
    /// and copied out a window at a time, because the two together are
    /// what keep a long document's preview off the frame's budget.
    pub(super) fn preview(&self, first: usize, count: usize) -> (usize, Vec<ContentLine>) {
        let mut cached = self.preview.borrow_mut();
        let fresh = cached
            .as_ref()
            .is_some_and(|(revision, theme, _)| *revision == self.revision && theme == &self.theme);
        if !fresh {
            *cached = Some((
                self.revision,
                self.theme.clone(),
                super::markdown::render(&self.contents(), &self.theme),
            ));
        }
        let (_, _, lines) = cached.as_ref().expect("rendered just above");
        (
            lines.len(),
            lines.iter().skip(first).take(count).cloned().collect(),
        )
    }

    /// Puts the caret on `line`, clamped to what the file has.
    pub(super) fn place_caret(&mut self, line: usize) {
        let line = line.min(self.lines.len().saturating_sub(1));
        self.caret = Caret {
            line,
            column: self.caret.column.min(self.line_len(line)),
        };
    }

    pub(super) fn line_len(&self, line: usize) -> usize {
        self.lines
            .get(line)
            .map(|text| text.chars().count())
            .unwrap_or(0)
    }

    /// Recolours the caret's line after typing changed it.
    ///
    /// From a highlighter with no history, unlike the whole-file pass in
    /// [`fulfill`]: keeping syntect's state per line boundary would let a
    /// block comment opened above colour this one correctly, and it would
    /// also mean holding a parse state per line of every open file. The
    /// line being typed is the one place that approximation shows, and
    /// the next save re-reads the file and colours all of it properly —
    /// so the error is bounded in both size and lifetime.
    pub(super) fn recolour_caret_line(&mut self) {
        self.changed();
        let Some(text) = self.lines.get(self.caret.line) else {
            return;
        };
        let mut highlighter = crate::code::highlight::highlighter(&self.path, &self.theme);
        let spans = crate::code::highlight::line(&mut highlighter, text);
        if let Some(slot) = self.highlighted.get_mut(self.caret.line) {
            *slot = spans;
        }
    }

    pub(super) fn insert(&mut self, character: char) {
        let line = self.caret.line;
        let Some(text) = self.lines.get_mut(line) else {
            return;
        };
        let at = byte_offset(text, self.caret.column);
        text.insert(at, character);
        self.caret.column += 1;
        self.modified = true;
        self.recolour_caret_line();
    }

    pub(super) fn split_line(&mut self) {
        let line = self.caret.line;
        let Some(text) = self.lines.get_mut(line) else {
            return;
        };
        let at = byte_offset(text, self.caret.column);
        let tail = text.split_off(at);
        self.lines.insert(line + 1, tail);
        self.highlighted.insert(line + 1, Vec::new());
        self.recolour_caret_line();
        self.caret = Caret {
            line: line + 1,
            column: 0,
        };
        self.recolour_caret_line();
        self.modified = true;
    }

    pub(super) fn backspace(&mut self) {
        if self.caret.column > 0 {
            let line = self.caret.line;
            let Some(text) = self.lines.get_mut(line) else {
                return;
            };
            let at = byte_offset(text, self.caret.column - 1);
            text.remove(at);
            self.caret.column -= 1;
            self.modified = true;
            self.recolour_caret_line();
        } else if self.caret.line > 0 {
            let removed = self.lines.remove(self.caret.line);
            self.highlighted.remove(self.caret.line);
            self.caret.line -= 1;
            self.caret.column = self.line_len(self.caret.line);
            if let Some(text) = self.lines.get_mut(self.caret.line) {
                text.push_str(&removed);
            }
            self.modified = true;
            self.recolour_caret_line();
        }
    }

    /// Forward delete: the character under the caret, or the line break
    /// after it when there is no character left on this line.
    pub(super) fn delete_forward(&mut self) {
        if self.caret.column < self.line_len(self.caret.line) {
            let line = self.caret.line;
            let column = self.caret.column;
            let Some(text) = self.lines.get_mut(line) else {
                return;
            };
            let at = byte_offset(text, column);
            text.remove(at);
            self.modified = true;
            self.recolour_caret_line();
        } else if self.caret.line + 1 < self.lines.len() {
            let removed = self.lines.remove(self.caret.line + 1);
            self.highlighted.remove(self.caret.line + 1);
            if let Some(text) = self.lines.get_mut(self.caret.line) {
                text.push_str(&removed);
            }
            self.modified = true;
            self.recolour_caret_line();
        }
    }

    /// Moves the caret the way `command` means. A command, never a key:
    /// which key reaches this is the host's business.
    pub(super) fn move_caret(&mut self, command: Command) {
        match command {
            Command::CaretLeft if self.caret.column > 0 => self.caret.column -= 1,
            // Left at the start of a line is the end of the one above —
            // a caret walks the text, not the line it happens to be on.
            Command::CaretLeft if self.caret.line > 0 => {
                self.caret.line -= 1;
                self.caret.column = self.line_len(self.caret.line);
            }
            Command::CaretRight if self.caret.column < self.line_len(self.caret.line) => {
                self.caret.column += 1;
            }
            Command::CaretRight if self.caret.line + 1 < self.lines.len() => {
                self.caret.line += 1;
                self.caret.column = 0;
            }
            Command::SelectPrevious if self.caret.line > 0 => {
                self.caret.line -= 1;
                self.caret.column = self.caret.column.min(self.line_len(self.caret.line));
            }
            Command::SelectNext if self.caret.line + 1 < self.lines.len() => {
                self.caret.line += 1;
                self.caret.column = self.caret.column.min(self.line_len(self.caret.line));
            }
            Command::CaretLineStart => self.caret.column = 0,
            Command::CaretLineEnd => self.caret.column = self.line_len(self.caret.line),
            _ => {}
        }
    }

    /// The text as it would be written: the lines rejoined with the
    /// terminator they arrived with, and a final one only if the file had
    /// one. Saving must not be a rewrite — a file opened and saved
    /// unedited is byte-for-byte what it was.
    pub(super) fn contents(&self) -> String {
        let terminator = match self.endings.carriage_return {
            true => "\r\n",
            false => "\n",
        };
        let mut text = self.lines.join(terminator);
        if self.endings.trailing {
            text.push_str(terminator);
        }
        text
    }
}

/// The byte offset of character `column` in `text`, clamped to its end.
fn byte_offset(text: &str, column: usize) -> usize {
    text.char_indices()
        .nth(column)
        .map(|(at, _)| at)
        .unwrap_or(text.len())
}
