//! The one place the YAML document library is named.
//!
//! `agents.yaml` is the user's file, so a write must change the bytes it
//! means to change and leave every other byte — comments included —
//! exactly where they were. A serde round-trip cannot do that: it emits
//! from the struct, and a comment has no field to live in. So reading goes
//! through serde (typed, validated) and writing goes through this module,
//! which edits a concrete syntax tree in place.
//!
//! It exists as its own module for the same reason `uze-git` exists: the
//! library is named here and nowhere else, so replacing it later is one
//! file rather than every call site. That isolation is also what makes
//! depending on a `0.0.x` crate defensible — see `AGENTS.md`'s dependency
//! rule.

use std::path::{Path, PathBuf};

use noyalib::{
    compat::serde_yaml,
    cst::{Document, parse_document},
};

use crate::{Result, UzeError, persistence::write_atomic};

/// A manifest open for editing: the authored bytes plus the tree that
/// knows where each declaration sits in them.
pub struct ManifestDocument {
    path: PathBuf,
    document: Document,
}

impl ManifestDocument {
    /// Opens `path` for editing. A file that is not YAML is refused here
    /// rather than at the first write, so a malformed manifest never gets
    /// half-edited.
    pub fn open(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| UzeError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_source(path, &text)
    }

    /// The in-memory constructor, so the guards can be tested without a
    /// filesystem.
    pub fn from_source(path: &Path, text: &str) -> Result<Self> {
        reject_shapes_we_will_not_edit(path, text)?;
        let document = parse_document(text).map_err(|error| malformed(path, error.to_string()))?;
        Ok(Self {
            path: path.to_path_buf(),
            document,
        })
    }

    /// A new, empty document — what `uze init` starts from.
    pub fn empty(path: &Path) -> Result<Self> {
        Self::from_source(path, "")
    }

    pub fn source(&self) -> &str {
        self.document.source()
    }

    /// Replaces the value at `path` (a dotted path such as
    /// `worktrees.completion`), creating neither the key nor its parents:
    /// a path that is not there is an error, not a silent insert.
    pub fn set(&mut self, path: &str, value: &str) -> Result<()> {
        self.document
            .set(path, value)
            .map_err(|error| self.refusal(path, error.to_string()))
    }

    /// Adds `key` to the mapping at `mapping_path`, or replaces its value
    /// when it is already there. The mapping itself is created when
    /// absent, since a manifest that has never declared a plugin has no
    /// `plugins:` key to insert into.
    ///
    /// The value is a *typed* value, never a fragment of YAML text. The
    /// library emits it in the file's own style and verifies the spliced
    /// document loads back as exactly this value, which is what a caller
    /// spelling `{ a: b }` by hand could never guarantee: a path holding a
    /// comma, a ref holding a brace, a version that looks like a float —
    /// each one is the emitter's problem now, not a quoting rule this
    /// module has to remember.
    pub fn upsert(
        &mut self,
        mapping_path: &str,
        key: &str,
        value: &serde_yaml::Value,
    ) -> Result<()> {
        if self.document.get(mapping_path).is_none() {
            return self.append_mapping(mapping_path, key, value);
        }
        // An entry already there is removed rather than overwritten:
        // growing a scalar (`flow: ours`) into a block (`flow:` + fields)
        // is not an in-place edit, and the library refuses it as one. When
        // it was the mapping's only entry the parent goes with it, and the
        // re-insert takes the create path above.
        if self
            .document
            .get(&format!("{mapping_path}.{key}"))
            .is_some()
        {
            self.remove(mapping_path, key)?;
            return self.upsert(mapping_path, key, value);
        }
        self.document
            .insert_entry_value(mapping_path, key, value)
            .map_err(|error| self.refusal(mapping_path, error.to_string()))
    }

    /// Appends `item` to the sequence at `sequence_path`, creating the
    /// sequence when the entry has none, and doing nothing when the item is
    /// already there. Declaring a plugin twice is a no-op, not a second
    /// line.
    pub fn push_unique(&mut self, sequence_path: &str, item: &str) -> Result<()> {
        let wanted = serde_yaml::Value::String(item.to_owned());
        match self.document.get(sequence_path) {
            Some(_) => {
                if self.items(sequence_path).iter().any(|held| held == item) {
                    return Ok(());
                }
                self.document
                    .push_back_value(sequence_path, &wanted)
                    .map_err(|error| self.refusal(sequence_path, error.to_string()))
            }
            None => {
                let (parent, key) = split_path(sequence_path);
                self.document
                    .insert_entry_value(parent, key, &serde_yaml::Value::Sequence(vec![wanted]))
                    .map_err(|error| self.refusal(sequence_path, error.to_string()))
            }
        }
    }

    /// Removes `item` from the sequence at `sequence_path`. When it was the
    /// last one the sequence's own key goes with it: a `plugins: []` left
    /// behind reads as a list that was made and emptied, which is not what
    /// happened — the entry above it simply has nothing taken from it now.
    pub fn remove_from(&mut self, sequence_path: &str, item: &str) -> Result<()> {
        let held = self.items(sequence_path);
        let Some(index) = held.iter().position(|candidate| candidate == item) else {
            return Ok(());
        };
        if held.len() == 1 {
            return self
                .document
                .remove(sequence_path)
                .map_err(|error| self.refusal(sequence_path, error.to_string()));
        }
        self.document
            .remove(&format!("{sequence_path}[{index}]"))
            .map_err(|error| self.refusal(sequence_path, error.to_string()))
    }

    /// The sequence's items as strings. A non-sequence, or an item that is
    /// not a scalar, reads as empty: the schema refuses both, and this is
    /// only ever asked to locate an entry to edit.
    fn items(&self, sequence_path: &str) -> Vec<String> {
        // Walked segment by segment: `Value::get` reads one key, so a
        // dotted path handed to it whole would look for a key literally
        // named `marketplaces.acme.plugins` and quietly find nothing.
        let root = self.document.as_value();
        let mut value = Some(&*root);
        for segment in sequence_path.split('.') {
            value = value.and_then(|held| held.get(segment));
        }
        value
            .and_then(|value| value.as_sequence())
            .map(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().unwrap_or_default().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The create path: `mapping_path` has never been declared, so there is
    /// no mapping to insert into and nothing above to preserve. Emitting
    /// the whole `key: value` block at once keeps the emitter's hands on
    /// the quoting here too.
    fn append_mapping(
        &mut self,
        mapping_path: &str,
        key: &str,
        value: &serde_yaml::Value,
    ) -> Result<()> {
        let mut entry = serde_yaml::Mapping::new();
        entry.insert(key, value.clone());
        let mut block = serde_yaml::Mapping::new();
        block.insert(mapping_path, serde_yaml::Value::Mapping(entry));
        let mut text = serde_yaml::to_string(&serde_yaml::Value::Mapping(block))
            .map_err(|error| self.refusal(mapping_path, error.to_string()))?;
        // The emitter does not terminate its last line; a file UZE writes
        // always ends with one.
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.append_block(&text)
    }

    /// Removes `key` from the mapping at `mapping_path`. When it was the
    /// last entry, the mapping's own key goes with it: `plugins:` left
    /// holding `{}` reads as a declaration that was made and emptied,
    /// which is not what happened.
    pub fn remove(&mut self, mapping_path: &str, key: &str) -> Result<()> {
        let full = format!("{mapping_path}.{key}");
        if self.document.get(&full).is_none() {
            return Ok(());
        }
        let last_one = self
            .document
            .as_value()
            .get(mapping_path)
            .and_then(|mapping| mapping.as_mapping().map(|entries| entries.len()))
            == Some(1);
        if last_one {
            self.document
                .remove(mapping_path)
                .map_err(|error| self.refusal(mapping_path, error.to_string()))?;
            return Ok(());
        }
        self.document
            .remove(&full)
            .map_err(|error| self.refusal(&full, error.to_string()))
    }

    /// Appends a block verbatim — used only when writing a manifest UZE
    /// itself authored, where there is no user formatting to preserve.
    pub fn append_block(&mut self, block: &str) -> Result<()> {
        let mut source = self.document.source().to_owned();
        if !source.is_empty() && !source.ends_with('\n') {
            source.push('\n');
        }
        source.push_str(block);
        self.document =
            parse_document(&source).map_err(|error| malformed(&self.path, error.to_string()))?;
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        write_atomic(&self.path, self.document.source().as_bytes())
    }

    fn refusal(&self, path: &str, reason: String) -> UzeError {
        UzeError::MalformedManifest {
            path: self.path.clone(),
            reason: format!(
                "`{path}` could not be written ({reason}); edit `{}` by hand and run the command \
                 again",
                self.path.display()
            ),
        }
    }
}

/// A dotted path split into the mapping that holds the last key, and that
/// key. `marketplaces.acme.plugins` is `plugins` inside `marketplaces.acme`.
fn split_path(path: &str) -> (&str, &str) {
    match path.rsplit_once('.') {
        Some((parent, key)) => (parent, key),
        None => ("", path),
    }
}

fn malformed(path: &Path, reason: String) -> UzeError {
    UzeError::MalformedManifest {
        path: path.to_path_buf(),
        reason,
    }
}

/// YAML a targeted edit cannot reason about locally. Each of these makes a
/// byte range mean something elsewhere in the file, so splicing one place
/// can change another — refused before an edit is attempted rather than
/// after the file is mangled.
fn reject_shapes_we_will_not_edit(path: &Path, text: &str) -> Result<()> {
    for (number, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        let refuse = |what: &str| {
            Err(malformed(
                path,
                format!(
                    "line {} uses {what}, which UZE will not edit around; declare the manifest \
                     without it, or edit the file by hand",
                    number + 1
                ),
            ))
        };
        if line.starts_with("---") || line.starts_with("...") {
            return refuse("a document separator");
        }
        if trimmed.starts_with("<<:") {
            return refuse("a merge key");
        }
        if line.contains('\t') {
            return refuse("a tab");
        }
        let before_comment = trimmed.split(" #").next().unwrap_or(trimmed);
        if let Some((_, after_colon)) = before_comment.split_once(": ") {
            let value = after_colon.trim_start();
            if value.starts_with('&') || value.starts_with('*') {
                return refuse("a YAML anchor or alias");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTHORED: &str = "\
# O ambiente de agentes deste projeto.
worktrees:
  completion: pr        # entrega via pull request
  link: [.env.local]

plugins:
  # pinado ate o fix do #431 subir
  flow: { marketplace: ai, ref: v0.3.1 }
";

    fn open(text: &str) -> ManifestDocument {
        ManifestDocument::from_source(Path::new("/p/agents.yaml"), text).unwrap()
    }

    fn value(yaml: &str) -> serde_yaml::Value {
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn reading_and_writing_back_untouched_is_byte_identical() {
        assert_eq!(open(AUTHORED).source(), AUTHORED);
    }

    #[test]
    fn setting_a_value_changes_only_that_value() {
        let mut document = open(AUTHORED);
        document.set("worktrees.completion", "merge").unwrap();
        assert_eq!(
            document
                .source()
                .replace("completion: merge", "completion: pr"),
            AUTHORED
        );
    }

    #[test]
    fn adding_a_plugin_keeps_the_comment_above_its_neighbour() {
        let mut document = open(AUTHORED);
        document.upsert("plugins", "git", &value("ai")).unwrap();
        let after = document.source();
        assert!(after.contains("# pinado ate o fix do #431 subir"));
        assert!(after.contains("flow: { marketplace: ai, ref: v0.3.1 }"));
        assert!(after.contains("git: ai"), "{after}");
    }

    #[test]
    fn adding_the_first_plugin_creates_the_mapping() {
        let mut document = open("worktrees:\n  completion: pr\n");
        document
            .upsert("plugins", "flow", &value("marketplace: ai\nref: v0.3.1\n"))
            .unwrap();
        assert_eq!(
            document.source(),
            "worktrees:\n  completion: pr\nplugins:\n  flow:\n    marketplace: ai\n    ref: \
             v0.3.1\n"
        );
    }

    #[test]
    fn a_scalar_entry_can_grow_into_a_block_without_losing_its_neighbours() {
        let mut document = open("plugins:\n  flow: ai\n  review: ai\n");
        document
            .upsert("plugins", "flow", &value("marketplace: ai\nref: v0.4.0\n"))
            .unwrap();
        let after = document.source();
        assert!(after.contains("ref: v0.4.0"), "{after}");
        assert!(after.contains("review: ai"), "{after}");
    }

    #[test]
    fn adding_a_plugin_that_is_already_there_replaces_its_value() {
        let mut document = open(AUTHORED);
        document
            .upsert("plugins", "flow", &value("marketplace: ai\nref: v0.4.0\n"))
            .unwrap();
        assert!(document.source().contains("v0.4.0"));
        assert!(!document.source().contains("v0.3.1"));
    }

    /// The emitter, not this module, decides how a value is spelled — so a
    /// path holding a comma stays one entry instead of becoming two.
    #[test]
    fn a_value_that_would_break_the_syntax_is_quoted_by_the_emitter() {
        let mut document = open("worktrees:\n  completion: pr\n");
        document
            .upsert(
                "marketplaces",
                "odd",
                &value("path: \"../my, dir\"\nref: \"release/{next}\"\n"),
            )
            .unwrap();
        let reparsed: serde_yaml::Value = serde_yaml::from_str(document.source()).unwrap();
        assert_eq!(
            reparsed["marketplaces"]["odd"]["path"].as_str(),
            Some("../my, dir")
        );
        assert_eq!(
            reparsed["marketplaces"]["odd"]["ref"].as_str(),
            Some("release/{next}")
        );
    }

    #[test]
    fn removing_the_last_plugin_removes_the_mapping_rather_than_leaving_it_empty() {
        let mut document = open(AUTHORED);
        document.remove("plugins", "flow").unwrap();
        let after = document.source();
        assert!(!after.contains("{}"), "left an empty mapping: {after}");
        assert!(!after.contains("plugins:"));
        assert!(after.contains("completion: pr"));
    }

    #[test]
    fn removing_something_absent_is_not_an_error() {
        let mut document = open(AUTHORED);
        document.remove("plugins", "never-declared").unwrap();
        assert_eq!(document.source(), AUTHORED);
    }

    #[test]
    fn an_edit_that_would_produce_invalid_yaml_leaves_the_file_untouched() {
        let mut document = open(AUTHORED);
        let outcome = document.set("worktrees.completion", "\"unterminated");
        assert!(outcome.is_err());
        assert_eq!(document.source(), AUTHORED);
    }

    #[test]
    fn anchors_aliases_merge_keys_tabs_and_streams_are_refused_up_front() {
        for (what, text) in [
            ("anchor", "a: &base 1\nb: 2\n"),
            ("alias", "a: 1\nb: *base\n"),
            ("merge key", "a:\n  <<: *base\n"),
            ("tab", "a:\n\tb: 1\n"),
            ("stream", "---\na: 1\n"),
        ] {
            let outcome = ManifestDocument::from_source(Path::new("/p/agents.yaml"), text);
            assert!(outcome.is_err(), "{what} should have been refused");
        }
    }

    #[test]
    fn a_hash_inside_a_comment_is_not_mistaken_for_a_shape_we_refuse() {
        let text = "# uses <<: and &anchors in prose\nworktrees:\n  completion: pr\n";
        assert_eq!(open(text).source(), text);
    }
}
