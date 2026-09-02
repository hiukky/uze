//! What an extension shows, as data.
//!
//! An extension answers with its content; the host draws it. That split is
//! the whole point of this module, and it buys three things at once:
//!
//! - **The host's design system stays the host's.** Chrome colour is a
//!   [`Role`], resolved against the palette the rest of the TUI already
//!   uses, so an extension can neither drift from it nor need a copy of it.
//! - **Geometry has one owner.** The host laid the rows out, so the host
//!   knows which row a click landed on. An extension never computes a
//!   rectangle and never hit-tests.
//! - **Nothing here is tied to this process.** Every type is plain data. If
//!   an extension is ever authored somewhere else, this is already the
//!   contract; today it just happens to be passed by value.
//!
//! The vocabulary is deliberately small and grows only when a *second*
//! extension needs the same primitive. One extension wanting a widget is a
//! special case; two are evidence.

/// The space the host has for the view. Advisory: the extension uses it to
/// decide how much to produce, not where to put it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    pub width: u16,
    pub height: u16,
}

/// Semantic colour. The host maps these onto its own palette, which is why
/// an extension never names one.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Role {
    #[default]
    Default,
    /// De-emphasised supporting text.
    Muted,
    /// A heading or label above content.
    Secondary,
    /// The selected or otherwise foremost item.
    Bright,
    /// An unselected navigable item.
    Inactive,
    Accent,
    /// The badge hue — distinct from every state colour, so a mark that
    /// classifies rather than warns cannot be misread as one.
    Info,
    Success,
    Warning,
    Danger,
}

/// Colour an extension supplies itself, as `(r, g, b)`.
///
/// Reserved for content that carries its own palette — syntax highlighting
/// comes from a theme the extension ships, the way an image carries its own
/// pixels, and flattening it into [`Role`] would throw the highlighting
/// away. Chrome never uses this: an extension colouring its own borders or
/// selection is exactly the drift [`Role`] exists to prevent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Span {
    pub text: String,
    pub role: Role,
    pub color: Option<Rgb>,
    pub bold: bool,
}

impl Span {
    pub fn new(text: impl Into<String>, role: Role) -> Self {
        Self {
            text: text.into(),
            role,
            color: None,
            bold: false,
        }
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}

/// A full-frame extension view: a titled surface with a navigator beside
/// its content and a hint row underneath.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct View {
    pub title: String,
    /// `None` when there is nothing to navigate — an error leaves the
    /// column empty rather than showing an empty list with a zero beside
    /// it, which reads as "no changes" when the truth is "we could not
    /// look".
    pub navigator: Option<Navigator>,
    pub content: Content,
    pub footer_hint: String,
}

/// The left-hand list of things to choose between.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Navigator {
    pub heading: String,
    /// Right-aligned beside the heading — a count, usually.
    pub badge: String,
    /// Whether keyboard focus is here, which the host renders more
    /// strongly than mere selection.
    pub focused: bool,
    pub rows: Vec<NavigatorRow>,
    /// The row that must stay on screen when the list is taller than the
    /// space. The host scrolls to keep it visible; the extension does not
    /// know how many rows fit.
    pub anchor: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NavigatorRow {
    /// A heading that groups the rows under it. Not selectable.
    Group { name: String, depth: usize },
    Item {
        /// The extension's own identifier, handed back verbatim in
        /// [`ViewHit::SelectItem`]. Opaque to the host.
        id: usize,
        name: String,
        depth: usize,
        /// A short status mark before the name.
        marker: Span,
        selected: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Content {
    /// Nothing to show, and why.
    Message { text: String, role: Role },
    /// Numbered lines with a gutter — a diff, a log, a file.
    Lines {
        heading: String,
        /// First line to show. The host clamps it to what exists.
        scroll: u16,
        lines: Vec<ContentLine>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentLine {
    /// One character before the number: `+`, `-`, or blank.
    pub gutter: String,
    pub number: String,
    pub tone: LineTone,
    pub spans: Vec<Span>,
}

/// What a line means, which the host turns into a background wash. Naming
/// the meaning rather than the colour is what keeps the wash consistent
/// with the rest of the TUI's surfaces.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LineTone {
    #[default]
    Neutral,
    Added,
    Removed,
}

/// Something the viewer did, in the view's own terms. The host produces
/// these from what it drew, so an extension never sees a coordinate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewHit {
    /// The `id` of a [`NavigatorRow::Item`].
    SelectItem(usize),
    /// The divider between navigator and content, dragged.
    ResizeNavigator,
    Close,
}

/// Which half of the view the pointer was over. Routing a wheel by where
/// the cursor is rather than by keyboard focus is what everything else
/// does; resolving *where* is the host's job, since it owns the layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollTarget {
    Navigator,
    Content,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollDirection {
    Up,
    Down,
}
