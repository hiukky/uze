//! The questions the CLI asks a person.
//!
//! One module, because every question after the first is a chance for two
//! prompts to disagree — on the toggle mark, on what `esc` means, on
//! whether an empty answer is a cancel. `dialoguer` supplies the keyboard
//! handling; this supplies the vocabulary, resolving the same `uze_theme`
//! symbols and tokens `progress` renders reports with, so a question looks
//! like the product that asked it.

use std::{collections::BTreeMap, fmt};

use dialoguer::theme::Theme as DialoguerTheme;
use uze_theme::Symbol;

use crate::progress;

/// One line of a multiple-choice question.
pub struct Choice {
    /// What the option is called — the only part a person reads to decide.
    pub label: String,
    /// Muted context beside the label: what makes this option worth
    /// picking, never its identity.
    pub hint: String,
    /// The answer the question opens on: checked, for a question that
    /// takes several; where the cursor starts, for one that takes one.
    pub selected: bool,
}

/// Whether a question can be asked at all.
///
/// A pipe, a CI job or an image build must never block on a prompt, so a
/// caller asks this first and answers from its own defaults when the
/// answer is no. Both ends matter: a terminal that cannot show the
/// question is as unanswerable as one that cannot carry the keystroke.
pub fn interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// What a person did with a multiple-choice question.
///
/// Declining and answering with nothing are separate on purpose: a caller
/// that folded them together could not tell "none of them" from "I did not
/// mean to run this", and those deserve different endings.
pub enum Answer {
    Chosen(Vec<usize>),
    /// `esc`, `q` or `ctrl-c` — the question was withdrawn, not answered.
    Declined,
}

/// Asks which of `choices` to act on, pre-checking the ones marked
/// selected. Answers in indices into `choices`.
pub fn multi_select(question: &str, choices: &[Choice]) -> Answer {
    if choices.is_empty() {
        return Answer::Chosen(Vec::new());
    }
    let labels = items(choices);
    let defaults: Vec<bool> = choices.iter().map(|choice| choice.selected).collect();
    let theme = PromptTheme::over(choices);

    match dialoguer::MultiSelect::with_theme(&theme)
        .with_prompt(question)
        .items(&labels)
        .defaults(&defaults)
        .interact_opt()
    {
        Ok(Some(picked)) => Answer::Chosen(picked),
        // A terminal that will not hand over keys is not a person saying
        // yes: withdrawing the question is the only safe reading.
        Ok(None) | Err(_) => Answer::Declined,
    }
}

/// Asks which one of `choices` to act on, opening on the first marked
/// selected. Answers in an index into `choices`; `None` is the question
/// withdrawn. There is no empty answer here to tell a withdrawal apart
/// from, which is why this needs none of [`Answer`].
pub fn select(question: &str, choices: &[Choice]) -> Option<usize> {
    if choices.is_empty() {
        return None;
    }
    let labels = items(choices);
    let theme = PromptTheme::over(choices);

    dialoguer::Select::with_theme(&theme)
        .with_prompt(question)
        .items(&labels)
        .default(
            choices
                .iter()
                .position(|choice| choice.selected)
                .unwrap_or(0),
        )
        .interact_opt()
        .ok()
        .flatten()
}

/// Asks a yes/no question, `default` being what `enter` alone means.
///
/// `None` is the question withdrawn — `esc`, `ctrl-c`, or a terminal that
/// will not hand over keys. A caller that folds that into "no" is
/// reporting an answer nobody gave, which for a question about trust is
/// the one thing it must not do.
pub fn confirm(question: &str, default: bool) -> Option<bool> {
    dialoguer::Confirm::with_theme(&PromptTheme::over(&[]))
        .with_prompt(question)
        .default(default)
        .interact_opt()
        .ok()
        .flatten()
}

/// Asks for a line of text, trimmed. `None` is the question withdrawn;
/// `Some("")` is a person who answered with nothing — a different thing,
/// and only the caller knows what to make of it. An empty line has to be
/// an answer rather than a rejected one, or a person who changed their
/// mind is left pressing `enter` at a question that keeps coming back.
pub fn ask(question: &str) -> Option<String> {
    dialoguer::Input::<String>::with_theme(&PromptTheme::over(&[]))
        .with_prompt(question)
        .allow_empty(true)
        .interact_text()
        .ok()
        .map(|answer| answer.trim().to_owned())
}

/// What the widget is handed for each choice: the label, and nothing else.
/// See `PromptTheme` for why anything decorative has to wait for the draw.
fn items(choices: &[Choice]) -> Vec<&str> {
    choices.iter().map(|choice| choice.label.as_str()).collect()
}

/// The keys the question answers to: moving and ending are every list's,
/// `own` is what this one adds. Stated here rather than in each caller's
/// help text, because `dialoguer` — not the caller — is what decides them.
///
/// The key carries the accent every literal a person types carries in this
/// CLI — the same one `clap_styles` gives a flag — and only what it *does*
/// is muted. A line where the key and its verb read alike is a line nobody
/// scans twice.
fn keys(own: &[(&str, &str)]) -> String {
    let mut all = vec![(
        format!(
            "{}{}",
            progress::glyph(Symbol::ArrowUp),
            progress::glyph(Symbol::ArrowDown)
        ),
        "move",
    )];
    all.extend(own.iter().map(|(key, does)| ((*key).to_owned(), *does)));
    all.push(("enter".to_owned(), "confirm"));
    all.push(("esc".to_owned(), "cancel"));
    all.iter()
        .map(|(key, does)| format!("{} {}", progress::accent(key), progress::label(does)))
        .collect::<Vec<_>>()
        .join(&progress::label(progress::glyph(Symbol::HintSeparator)))
}

/// Colour and marks for one question.
///
/// The hints live here rather than in the item text `dialoguer` is handed,
/// because that same text is what it measures the list's height against: an
/// item carrying escape sequences measures far wider than it draws, and the
/// over-count eats the question's own lines on the first redraw. So an item
/// stays the bare label, and everything decorative is added at the moment
/// of drawing.
struct PromptTheme {
    /// Keyed by label: a question with two identically labelled choices
    /// gives a person no way to tell them apart on screen either.
    hints: BTreeMap<String, String>,
    label_width: usize,
}

impl PromptTheme {
    fn over(choices: &[Choice]) -> Self {
        Self {
            hints: choices
                .iter()
                .filter(|choice| !choice.hint.is_empty())
                .map(|choice| (choice.label.clone(), choice.hint.clone()))
                .collect(),
            label_width: choices
                .iter()
                .map(|choice| choice.label.chars().count())
                .max()
                .unwrap_or(0),
        }
    }

    fn decorated(&self, label: &str) -> String {
        let Some(hint) = self.hints.get(label) else {
            return progress::title(label);
        };
        let padding = " ".repeat(self.label_width.saturating_sub(label.chars().count()));
        format!(
            "{}{padding}  {}",
            progress::title(label),
            progress::label(hint)
        )
    }
}

impl PromptTheme {
    /// Every question opens the same way: the mark, the question, and the
    /// keys that answer it.
    fn question_line(&self, f: &mut dyn fmt::Write, question: &str, keys: &str) -> fmt::Result {
        // Both blanks belong to the question, not to what surrounds it:
        // `dialoguer` clears the block it drew by counting the lines it was
        // handed, so a gap opened here closes again with the question, and
        // the answer line below opens the same one to land in the same
        // place. The leading one matters most — without it the question sits
        // against the command the person just typed, two prompt marks deep.
        write!(
            f,
            "\n{} {}\n  {keys}\n",
            progress::accent(progress::glyph(Symbol::Prompt)),
            progress::title(question)
        )
    }

    /// And every one leaves the same line behind — the record of what the
    /// run that follows is about to act on.
    fn answer_line(&self, f: &mut dyn fmt::Write, question: &str, answer: &str) -> fmt::Result {
        write!(
            f,
            "\n{} {} {answer}",
            progress::success_icon(),
            progress::label(format!("{question}:"))
        )
    }

    /// The gutter every list row starts with. An inactive row pads instead
    /// of drawing, so a cursor moving down the list never shifts it
    /// sideways.
    fn cursor(active: bool) -> String {
        if active {
            progress::accent(progress::glyph(Symbol::ChevronRight))
        } else {
            " ".repeat(progress::glyph_width(Symbol::ChevronRight))
        }
    }
}

impl DialoguerTheme for PromptTheme {
    fn format_multi_select_prompt(&self, f: &mut dyn fmt::Write, prompt: &str) -> fmt::Result {
        self.question_line(f, prompt, &keys(&[("space", "toggle"), ("a", "all")]))
    }

    /// A question that takes one answer has nothing to toggle: naming keys
    /// that do nothing here is how a person learns to stop reading the
    /// line.
    fn format_select_prompt(&self, f: &mut dyn fmt::Write, prompt: &str) -> fmt::Result {
        self.question_line(f, prompt, &keys(&[]))
    }

    fn format_multi_select_prompt_item(
        &self,
        f: &mut dyn fmt::Write,
        text: &str,
        checked: bool,
        active: bool,
    ) -> fmt::Result {
        let mark = if checked {
            progress::success_text(progress::glyph(Symbol::MarkToggleOn))
        } else {
            progress::label(progress::glyph(Symbol::MarkToggleOff))
        };
        write!(
            f,
            "{} {mark} {}",
            Self::cursor(active),
            self.decorated(text)
        )
    }

    /// No mark: where exactly one answer leaves with the question, the
    /// cursor already says which, and a checkbox beside it would offer a
    /// second one that does not exist.
    fn format_select_prompt_item(
        &self,
        f: &mut dyn fmt::Write,
        text: &str,
        active: bool,
    ) -> fmt::Result {
        write!(f, "{} {}", Self::cursor(active), self.decorated(text))
    }

    fn format_multi_select_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        selections: &[&str],
    ) -> fmt::Result {
        let chosen = if selections.is_empty() {
            progress::label("nothing")
        } else {
            selections
                .iter()
                .map(progress::title)
                .collect::<Vec<_>>()
                .join(&progress::glyph(Symbol::HintSeparator))
        };
        self.answer_line(f, prompt, &chosen)
    }

    fn format_select_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        selection: &str,
    ) -> fmt::Result {
        self.answer_line(f, prompt, &progress::title(selection))
    }

    /// A yes/no question is answered where it is asked, so it stays on one
    /// line and ends in the space the answer is typed into.
    fn format_confirm_prompt(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        default: Option<bool>,
    ) -> fmt::Result {
        let suffix = match default {
            Some(true) => "[Y/n]",
            Some(false) => "[y/N]",
            None => "[y/n]",
        };
        write!(
            f,
            "\n{} {} {} ",
            progress::accent(progress::glyph(Symbol::Prompt)),
            progress::title(prompt),
            progress::label(suffix)
        )
    }

    fn format_confirm_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        selection: Option<bool>,
    ) -> fmt::Result {
        let answer = match selection {
            Some(true) => progress::title("yes"),
            Some(false) => progress::title("no"),
            // The question was withdrawn. It still leaves a line, because
            // the run that follows acts on the withdrawal too.
            None => progress::label("cancelled"),
        };
        self.answer_line(f, prompt, &answer)
    }

    fn format_input_prompt(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        default: Option<&str>,
    ) -> fmt::Result {
        let suffix = match default {
            Some(default) => progress::label(format!("[{default}]")),
            None => String::new(),
        };
        write!(
            f,
            "\n{} {} {suffix}",
            progress::accent(progress::glyph(Symbol::Prompt)),
            progress::title(prompt)
        )
    }

    fn format_input_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        selection: &str,
    ) -> fmt::Result {
        self.answer_line(f, prompt, &progress::title(selection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(label: &str, hint: &str, selected: bool) -> Choice {
        Choice {
            label: label.to_owned(),
            hint: hint.to_owned(),
            selected,
        }
    }

    fn catalog() -> Vec<Choice> {
        vec![
            choice("Claude Code", "installed 2.1.0", true),
            choice("Codex", "not installed", false),
        ]
    }

    fn row(theme: &PromptTheme, label: &str, checked: bool, active: bool) -> String {
        let mut out = String::new();
        theme
            .format_multi_select_prompt_item(&mut out, label, checked, active)
            .unwrap();
        out
    }

    #[test]
    fn an_item_handed_to_the_widget_carries_no_decoration() {
        // The load-bearing one: `dialoguer` measures the list's height from
        // the item text it was given, so an escape sequence in there counts
        // as width, and the over-count clears the question off the screen
        // on the first keystroke.
        let choices = catalog();
        assert_eq!(items(&choices), vec!["Claude Code", "Codex"]);
        let theme = PromptTheme::over(&choices);
        assert_ne!(
            theme.decorated("Codex"),
            "Codex",
            "the hint has to reach the screen somewhere — at the draw, not in the item"
        );
    }

    #[test]
    fn a_hint_never_pushes_a_label_out_of_its_column() {
        let choices = catalog();
        let theme = PromptTheme::over(&choices);
        let columns: Vec<usize> = choices
            .iter()
            .map(|choice| {
                let drawn = theme.decorated(&choice.label);
                drawn.find(&choice.hint).expect("hint drawn")
            })
            .collect();
        assert!(
            columns.windows(2).all(|pair| pair[0] == pair[1]),
            "hints start at different columns: {columns:?}"
        );
    }

    #[test]
    fn a_choice_without_a_hint_is_just_its_label() {
        let theme = PromptTheme::over(&[choice("Codex", "", false)]);
        assert_eq!(theme.decorated("Codex"), progress::title("Codex"));
    }

    #[test]
    fn checked_and_unchecked_rows_are_told_apart_by_their_marks() {
        let theme = PromptTheme::over(&catalog());
        let active = uze_theme::active();
        assert!(row(&theme, "Codex", true, false).contains(active.glyph(Symbol::MarkToggleOn)));
        assert!(row(&theme, "Codex", false, false).contains(active.glyph(Symbol::MarkToggleOff)));
    }

    #[test]
    fn the_cursor_keeps_its_column_on_every_row() {
        // An inactive row pads instead of drawing: a cursor that changed
        // the row's width would make the whole list shift as it moves.
        let theme = PromptTheme::over(&catalog());
        let width = progress::glyph_width(Symbol::ChevronRight);
        assert!(row(&theme, "Codex", false, false).starts_with(&" ".repeat(width)));
        let active = uze_theme::active();
        assert!(row(&theme, "Codex", false, true).contains(active.glyph(Symbol::ChevronRight)));
    }

    #[test]
    fn a_row_that_takes_the_whole_answer_carries_no_toggle_mark() {
        // A checkbox on a question that ends with the first answer offers a
        // second gesture that does not exist.
        let theme = PromptTheme::over(&catalog());
        let mut drawn = String::new();
        theme
            .format_select_prompt_item(&mut drawn, "Codex", true)
            .unwrap();
        let active = uze_theme::active();
        assert!(!drawn.contains(active.glyph(Symbol::MarkToggleOn)));
        assert!(!drawn.contains(active.glyph(Symbol::MarkToggleOff)));
        assert!(drawn.contains(active.glyph(Symbol::ChevronRight)));
    }

    #[test]
    fn only_a_question_that_toggles_names_the_toggle_keys() {
        assert!(keys(&[("space", "toggle")]).contains("toggle"));
        assert!(!keys(&[]).contains("toggle"));
        for line in [keys(&[]), keys(&[("space", "toggle")])] {
            assert!(line.contains("confirm") && line.contains("cancel"));
        }
    }

    #[test]
    fn a_withdrawn_question_still_leaves_the_line_that_says_so() {
        // The run that follows acts on the withdrawal too, so the record of
        // what was asked has to survive it.
        let theme = PromptTheme::over(&[]);
        let mut drawn = String::new();
        theme
            .format_confirm_prompt_selection(&mut drawn, "Trust and install?", None)
            .unwrap();
        assert!(drawn.contains("Trust and install?"));
        assert!(drawn.contains("cancelled"));
    }

    #[test]
    fn a_question_with_no_choices_is_answered_without_asking() {
        // Guards the caller contract, not the widget: `dialoguer` errors on
        // an empty item list, and an error reads here as `Declined` — which
        // would report a cancellation nobody performed.
        assert!(
            matches!(multi_select("Harnesses", &[]), Answer::Chosen(picked) if picked.is_empty())
        );
    }
}
