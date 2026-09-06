//! Every registered harness, as a conformance subject.
//!
//! The subject list is built from `IntegrationRegistry::isolated` — the
//! product's own composition root — so a harness cannot be added to UZE and
//! silently skipped here. That is the whole point of this module: the
//! per-harness conformance surface used to be hand-copied, and a hand-copied
//! matrix cannot tell a deliberate exemption from a forgotten one.
//!
//! A harness with no vendor binding below fails `bindings_for` by name. A
//! harness that genuinely has no surface for a question declares
//! [`Support::NotApplicable`] with the reason, which the suite records
//! instead of quietly asserting nothing.

use std::path::PathBuf;

use uze_core::{home::UzeHome, integration::IntegrationPort};
use uze_integrations::registry::IntegrationRegistry;

use super::fixtures::temp;

/// Something a harness either has, or explicitly declares it does not —
/// with the reason. Never an omission.
pub(crate) enum Support<T> {
    Has(T),
    NotApplicable(&'static str),
}

impl<T> Support<T> {
    pub(crate) fn get(&self) -> Option<&T> {
        match self {
            Self::Has(value) => Some(value),
            Self::NotApplicable(_) => None,
        }
    }

    pub(crate) fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Has(_) => None,
            Self::NotApplicable(reason) => Some(reason),
        }
    }
}

/// How one vendor spells an explicit native envelope inside a package.
/// Content, not shape: every assertion taken against these is phrased as an
/// outcome, never as "the JSON must look like X".
pub(crate) struct Envelope {
    /// Where the envelope lives, relative to the package root.
    pub(crate) manifest: &'static str,
    /// An envelope this vendor cannot parse.
    pub(crate) malformed: &'static str,
    /// An envelope whose declared path leaves the package.
    pub(crate) escaping: Support<&'static str>,
    /// An envelope whose declared path is absolute — the destructive-
    /// normalization regression (a leading `/` stripped until an absolute
    /// declaration matched a real relative resource).
    pub(crate) absolute: Support<&'static str>,
    /// The same absolute declaration, padded with whitespace: a normalizer
    /// that trims before deciding whether a path is absolute reopens the
    /// hole `absolute` closes.
    pub(crate) absolute_padded: Support<&'static str>,
    /// An envelope declaring the same resource more than once, spelled
    /// differently each time. `NotApplicable` where the vendor's declaration
    /// cannot hold a repeat.
    pub(crate) duplicate: Support<&'static str>,
    /// An envelope declaring a strict subset of what the package holds, and
    /// which of the fixture's resources it declares. What each vendor
    /// declares differs in kind — Claude and Codex declare a skill, and
    /// Antigravity declares MCP servers — which is why the subset is named
    /// here rather than assumed by the assertion.
    pub(crate) partial: Partial,
}

/// The explicit-precedence case: the envelope files to add, and the fixture
/// resources that envelope declares (`skill`, `mcp-a`, `mcp-b`).
pub(crate) struct Partial {
    pub(crate) files: &'static [(&'static str, &'static str)],
    pub(crate) declares: &'static [&'static str],
}

pub(crate) struct Bindings {
    pub(crate) envelope: Support<Envelope>,
}

/// The vendor knowledge this suite is allowed to hold, in one place.
fn bindings_for(id: &str) -> &'static Bindings {
    match id {
        "claude-code" => &Bindings {
            envelope: Support::Has(Envelope {
                manifest: ".claude-plugin/plugin.json",
                malformed: "{not json",
                escaping: Support::Has(
                    r#"{"name":"flow","skills":["../../etc","/absolute-that-still-resolves-relative"]}"#,
                ),
                absolute: Support::Has(r#"{"name":"flow","skills":["/skills/commit"]}"#),
                absolute_padded: Support::Has(r#"{"name":"flow","skills":["  /skills/commit  "]}"#),
                duplicate: Support::Has(
                    r#"{"name":"flow","skills":["./skills/commit","./skills/commit","skills/commit"]}"#,
                ),
                partial: Partial {
                    files: &[(
                        ".claude-plugin/plugin.json",
                        r#"{"name":"flow","skills":["./skills/commit"]}"#,
                    )],
                    // The package also ships a root `mcp.json` generation
                    // would have picked up; presence of the envelope must
                    // keep coverage at what the envelope itself declares.
                    declares: &["skill"],
                },
            }),
        },
        "codex" => &Bindings {
            envelope: Support::Has(Envelope {
                manifest: ".codex-plugin/plugin.json",
                malformed: "{not json",
                escaping: Support::Has(r#"{"name":"flow","skills":"../../etc"}"#),
                absolute: Support::Has(r#"{"name":"flow","skills":"/skills/commit"}"#),
                absolute_padded: Support::Has(r#"{"name":"flow","skills":"  /skills/commit  "}"#),
                duplicate: Support::NotApplicable(
                    "the declaration is a single directory string, which cannot hold a repeat",
                ),
                partial: Partial {
                    files: &[(
                        ".codex-plugin/plugin.json",
                        r#"{"name":"flow","skills":"./skills/"}"#,
                    )],
                    // Codex's explicit `mcpServers` field points at an
                    // external file, never the root `mcp.json` generation
                    // reads, so an envelope declaring only skills truly
                    // cannot see it.
                    declares: &["skill"],
                },
            }),
        },
        "antigravity" => &Bindings {
            envelope: Support::Has(Envelope {
                // The canonical `plugin.json` *is* this vendor's manifest, so
                // an unreadable one is an unreadable envelope.
                manifest: "plugin.json",
                malformed: "{not json",
                escaping: Support::NotApplicable(
                    "coverage is structural: no manifest-declared path field to escape with \
                     (see antigravity/plugin.rs)",
                ),
                absolute: Support::NotApplicable(
                    "coverage is structural: no manifest-declared path field to make absolute",
                ),
                absolute_padded: Support::NotApplicable(
                    "coverage is structural: no manifest-declared path field to pad",
                ),
                duplicate: Support::NotApplicable(
                    "coverage is structural: nothing is declared, so nothing can be declared twice",
                ),
                partial: Partial {
                    // An author-shipped vendor MCP config declaring a subset
                    // keeps the explicit route; generation, which would
                    // translate the whole canonical mcp.json, must never be
                    // consulted to top it up.
                    files: &[(
                        "mcp_config.json",
                        r#"{"mcpServers":{"mcp-b":{"command":"b"}}}"#,
                    )],
                    declares: &["skill", "mcp-b"],
                },
            }),
        },
        "opencode" => &Bindings {
            envelope: Support::NotApplicable(
                "no package-level delivery at all: skills are discovered from the shared agent \
                 skill root, so there is no envelope to declare coverage with",
            ),
        },
        other => panic!(
            "harness {other:?} is registered in the product but has no conformance binding. Add \
             one to tests/integrations/subjects.rs, or declare the questions it cannot answer as \
             Support::NotApplicable with the reason — never leave it out, which is how a matrix \
             stops meaning anything."
        ),
    }
}

pub(crate) struct Subject {
    pub(crate) id: String,
    /// The throwaway root this subject's harness home and UZE home live under.
    pub(crate) root: PathBuf,
    pub(crate) home: UzeHome,
    pub(crate) integration: Box<dyn IntegrationPort>,
    pub(crate) bindings: &'static Bindings,
}

impl Subject {
    pub(crate) fn envelope(&self) -> &Support<Envelope> {
        &self.bindings.envelope
    }
}

impl Drop for Subject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// One subject per registered harness, each in a throwaway world of its own.
/// `label` distinguishes one test's worlds from another's.
pub(crate) fn subjects(label: &str) -> Vec<Subject> {
    // `ids()` off the isolated registry, the same way
    // `tests/cli/machine.rs::setup_conformance_matrix_covers_every_registered_harness`
    // reads the harness set: one source for "which harnesses exist".
    let ids: Vec<&'static str> = {
        let root = temp(&format!("{label}-ids"));
        let home = UzeHome::at(root.join("uze"));
        let ids = IntegrationRegistry::isolated(&root, &home).ids();
        let _ = std::fs::remove_dir_all(root);
        ids
    };

    ids.into_iter()
        .map(|id| {
            let root = temp(&format!("{label}-{id}"));
            let home = UzeHome::at(root.join("uze"));
            let registry = IntegrationRegistry::isolated(&root, &home);
            let integration = registry
                .into_inner()
                .into_iter()
                .find(|candidate| candidate.id() == id)
                .expect("the id came from this same registry");
            let bindings = bindings_for(id);
            Subject {
                id: id.to_owned(),
                root,
                home,
                integration,
                bindings,
            }
        })
        .collect()
}
