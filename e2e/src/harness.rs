//! The harness registry: the only place a new harness is declared.
//!
//! Adding a harness must not mean writing a new runner. Every tier is generic
//! over this table, so a new entry — plus whatever `IntegrationPort` the
//! product already provides — is the whole cost of covering it.
//!
//! Deliberately absent: any expected attachment name. Tier 1 reads the names
//! UZE actually attached out of `uze inspect --format json` and hands them to
//! Tier 2, so a fixture rename can never silently pass against a stale
//! constant here.

/// The delivery shapes UZE can produce, as reported in an attachment
/// receipt's `artifact` field. A harness declares a deterministic probe per
/// kind it actually receives, because the name a harness can be asked about
/// differs by route: a vendor-config entry is named by UZE, while a natively
/// installed package is named by the harness's own plugin system and carries
/// its capabilities implicitly.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum ArtifactKind {
    VendorConfigEntry,
    SymlinkReference,
    IntegrationOwned,
}

impl ArtifactKind {
    /// The tag UZE serializes for this variant.
    pub fn tag(self) -> &'static str {
        match self {
            Self::VendorConfigEntry => "VENDOR_CONFIG_ENTRY",
            Self::SymlinkReference => "SYMLINK_REFERENCE",
            Self::IntegrationOwned => "INTEGRATION_OWNED",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "VENDOR_CONFIG_ENTRY" => Some(Self::VendorConfigEntry),
            "SYMLINK_REFERENCE" => Some(Self::SymlinkReference),
            "INTEGRATION_OWNED" => Some(Self::IntegrationOwned),
            _ => None,
        }
    }
}

/// One deterministic probe: a harness subcommand that reports what the
/// harness itself can see, without starting a model turn.
///
/// `required` fragments must appear in the probe's output *in addition to*
/// the attached name Tier 1 discovered. They separate "the harness lists the
/// entry" from "the harness reports it as usable" — a config entry pointing
/// at a dead binary is still listed, so listing alone is not discovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Probe {
    pub arguments: &'static [&'static str],
    /// Fragments that must appear. `{mcp_binary}` is substituted with the
    /// resolved fixture server path, which ties a probe to this package even
    /// when the harness renames the capability out of UZE's naming.
    pub required: &'static [&'static str],
    /// Whether the attached name Tier 1 discovered must also appear. A
    /// harness that decomposes a delivered package into capabilities under
    /// its own names cannot be asked about UZE's name, so such a probe
    /// identifies the attachment through `required` instead.
    pub matches_attached_name: bool,
    /// Exactly what a pass on this probe establishes, echoed into the
    /// evidence record. Two harnesses can both report `DiscoveryVerified`
    /// while proving different depths — Codex offers no MCP health check —
    /// and a reader must not have to infer that from the argv.
    pub claim: &'static str,
}

/// Files a harness needs in its disposable HOME before a Tier 3 route
/// resolves. Declarative so a new harness adds data, not runner code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderConfig {
    /// Written relative to the run's HOME. `{gateway}` is substituted.
    pub relative_path: &'static str,
    pub contents: &'static str,
    /// Directories copied into HOME before the config is written, as
    /// `(absolute source, destination relative to HOME)`. OpenCode resolves
    /// its official runtime plugin packages on first use; they are baked into
    /// the image so no tier depends on reaching npm.
    pub seed: &'static [(&'static str, &'static str)],
}

/// Tier 3 invocation shape. Placeholders substituted at run time:
/// `{model}`, `{prompt}`, `{workspace}`, `{gateway}`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BehaviorSpec {
    pub arguments: &'static [&'static str],
    pub environment: &'static [(&'static str, &'static str)],
    /// Gateway alias this harness's protocol route resolves against.
    pub model: &'static str,
    pub provider_config: Option<ProviderConfig>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessSpec {
    /// Stable id used on the command line and in emitted evidence.
    pub id: &'static str,
    /// Argument accepted by `uze setup`, and the value the product reports in
    /// an attachment receipt's `integration` field.
    pub uze_name: &'static str,
    pub executable: &'static str,
    /// Deterministic discovery probe per artifact kind. `None` means this
    /// harness offers no non-model way to report that kind; the tier records
    /// it as unavailable rather than inventing evidence.
    pub probes: &'static [(ArtifactKind, Probe)],
    pub behavior: BehaviorSpec,
}

impl HarnessSpec {
    pub fn probes_for(&self, kind: ArtifactKind) -> Vec<&'static Probe> {
        self.probes
            .iter()
            .filter(|(declared, _)| *declared == kind)
            .map(|(_, probe)| probe)
            .collect()
    }
}

pub const HARNESSES: &[HarnessSpec] = &[
    HarnessSpec {
        id: "claude",
        uze_name: "claude-code",
        executable: "claude",
        probes: &[
            (
                ArtifactKind::VendorConfigEntry,
                Probe {
                    arguments: &["mcp", "list"],
                    required: &["Connected"],
                    matches_attached_name: true,
                    claim: "Claude Code lists the UZE-registered MCP server and reports it connected",
                },
            ),
            (
                ArtifactKind::SymlinkReference,
                Probe {
                    arguments: &["plugin", "list"],
                    required: &["loaded"],
                    matches_attached_name: true,
                    claim: "Claude Code loads the UZE-managed skill reference from its user scope",
                },
            ),
        ],
        behavior: BehaviorSpec {
            // `--permission-mode bypassPermissions` is required, not
            // convenience: Claude Code's default mode denies every MCP tool
            // call when no interactive approver exists, which burns turns and
            // surfaces as `error_max_turns` rather than as a permission
            // problem. The container is already externally sandboxed
            // (read-only root, tmpfs-only writes, cap_drop ALL, no provider
            // egress), and this tier asserts capability wiring, not the
            // harness's own approval UX.
            arguments: &[
                "-p",
                "--output-format",
                "json",
                "--no-session-persistence",
                "--permission-mode",
                "bypassPermissions",
                "--max-turns",
                "6",
                "--model",
                "{model}",
                "{prompt}",
            ],
            environment: &[
                ("ANTHROPIC_BASE_URL", "{gateway}"),
                ("ANTHROPIC_API_KEY", "not-required-inside-isolated-lab"),
            ],
            // Claude Code sends `reasoning.effort` for any model name it does
            // not recognize, which a plain chat model rejects with a 400.
            model: "uze-conformance-reasoning",
            provider_config: None,
        },
    },
    HarnessSpec {
        id: "codex",
        uze_name: "codex",
        executable: "codex",
        // Codex receives the whole package natively, so its receipt is an
        // integration-owned artifact the Core does not interpret. UZE names
        // the package and Codex derives capability names from the installed
        // envelope, which is why no UZE-named vendor-config entry exists to
        // probe for.
        probes: &[(
            ArtifactKind::IntegrationOwned,
            Probe {
                arguments: &["plugin", "list"],
                required: &["installed", "enabled"],
                matches_attached_name: true,
                claim: "Codex reports the UZE marketplace plugin installed and enabled",
            },
        ),
        (
            // Second probe on the same delivery: `plugin list` proves the
            // package installed, but says nothing about the capabilities
            // inside it. This one proves Codex decomposed the envelope into
            // a real MCP server registration. Codex names that server from
            // the package's own manifest rather than from UZE, so the
            // resolved binary path is what ties it back to this fixture.
            //
            // Codex runs no health check here, so this is registration
            // evidence, not connectivity evidence — Claude and OpenCode are
            // probed one level deeper. That asymmetry is a real coverage
            // gap, recorded rather than papered over.
            ArtifactKind::IntegrationOwned,
            Probe {
                arguments: &["mcp", "list", "--json"],
                required: &["{mcp_binary}", "\"enabled\": true"],
                matches_attached_name: false,
                claim: "Codex decomposed the plugin envelope into an enabled MCP server registration (registration only: Codex runs no health check)",
            },
        )],
        behavior: BehaviorSpec {
            // Codex 0.148.0 removed `wire_api = "chat"` for custom model
            // providers, so the gateway route only resolves declared as
            // "responses". `model_reasoning_summary="none"` avoids a provider
            // 400 demanding a verified organization for reasoning summaries.
            arguments: &[
                "-c",
                "model_providers.uze_gateway.name=\"UZE Conformance Gateway\"",
                "-c",
                "model_providers.uze_gateway.base_url=\"{gateway}/v1\"",
                "-c",
                "model_providers.uze_gateway.env_key=\"UZE_GATEWAY_KEY\"",
                "-c",
                "model_providers.uze_gateway.wire_api=\"responses\"",
                "-c",
                "model_provider=uze_gateway",
                "-c",
                "model_reasoning_summary=\"none\"",
                "--model",
                "{model}",
                // Codex's bundled bubblewrap needs an unprivileged user
                // namespace, which Docker's default seccomp profile blocks.
                // Relaxing seccomp so a sandbox can run inside a container
                // that is already read-only, tmpfs-only, cap_drop ALL and
                // without provider egress trades real isolation for none, so
                // the outer boundary is the sandbox and Codex's own is off.
                "--dangerously-bypass-approvals-and-sandbox",
                "exec",
                "--skip-git-repo-check",
                "--ephemeral",
                "--cd",
                "{workspace}",
                "{prompt}",
            ],
            environment: &[("UZE_GATEWAY_KEY", "not-required-inside-isolated-lab")],
            model: "uze-conformance-reasoning",
            provider_config: None,
        },
    },
    HarnessSpec {
        id: "opencode",
        uze_name: "opencode",
        executable: "opencode",
        probes: &[
            (
                ArtifactKind::VendorConfigEntry,
                Probe {
                    arguments: &["mcp", "list"],
                    required: &["connected"],
                    matches_attached_name: true,
                    claim: "OpenCode lists the UZE-registered MCP server and reports it connected",
                },
            ),
            (
                ArtifactKind::SymlinkReference,
                Probe {
                    // Emits JSON per discovered skill, including the resolved
                    // `location`, which proves the managed symlink resolves
                    // rather than merely existing.
                    arguments: &["debug", "skill"],
                    required: &[],
                    matches_attached_name: true,
                    claim: "OpenCode resolves the UZE-managed skill symlink into its discovered skill set",
                },
            ),
        ],
        behavior: BehaviorSpec {
            // The selected model is fully declared in the per-run config the
            // caller writes. `OPENCODE_DISABLE_MODELS_FETCH` keeps OpenCode
            // from refreshing its optional models.dev catalog, which no tier
            // may depend on.
            arguments: &[
                "run",
                "--pure",
                "--model",
                "uze-gateway/{model}",
                "{prompt}",
            ],
            environment: &[
                ("OPENCODE_DISABLE_MODELS_FETCH", "1"),
                ("OPENCODE_MODELS_PATH", "/opt/uze-e2e/opencode-models.json"),
            ],
            model: "uze-conformance",
            provider_config: Some(ProviderConfig {
                relative_path: ".config/opencode/opencode.json",
                contents: r#"{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "uze-gateway": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "UZE Conformance Gateway",
      "options": {
        "baseURL": "{gateway}/v1",
        "apiKey": "not-required-inside-isolated-lab"
      },
      "models": { "uze-conformance": { "name": "UZE Conformance" } }
    }
  }
}"#,
                seed: &[("/opt/opencode-runtime", ".config/opencode")],
            }),
        },
    },
];

pub fn lookup(id: &str) -> Option<&'static HarnessSpec> {
    HARNESSES.iter().find(|harness| harness.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_harness_id_is_unique_and_resolvable() {
        for harness in HARNESSES {
            assert_eq!(lookup(harness.id).map(|found| found.id), Some(harness.id));
        }
        let mut ids: Vec<&str> = HARNESSES.iter().map(|harness| harness.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "harness ids must be unique");
    }

    #[test]
    fn no_harness_declares_a_hardcoded_attachment_name() {
        // Attachment names come from `uze inspect` at run time. A literal
        // `uze-` fragment here would let a stale expectation pass.
        for harness in HARNESSES {
            for (_, probe) in harness.probes {
                for fragment in probe.required {
                    assert!(
                        !fragment.contains("uze-"),
                        "{} hardcodes an attachment name fragment: {fragment}",
                        harness.id
                    );
                }
            }
        }
    }

    #[test]
    fn a_probe_that_skips_the_attached_name_must_identify_itself_some_other_way() {
        for harness in HARNESSES {
            for (_, probe) in harness.probes {
                assert!(
                    probe.matches_attached_name || !probe.required.is_empty(),
                    "{} declares a probe that asserts nothing",
                    harness.id
                );
                assert!(
                    !probe.claim.is_empty(),
                    "{} declares a probe with no stated claim",
                    harness.id
                );
            }
        }
    }

    #[test]
    fn every_declared_probe_kind_is_reachable() {
        for harness in HARNESSES {
            for (kind, _) in harness.probes {
                assert!(
                    !harness.probes_for(*kind).is_empty(),
                    "{} declares an unreachable {kind:?} probe",
                    harness.id
                );
            }
        }
    }

    #[test]
    fn behavior_specs_carry_no_provider_credential() {
        for harness in HARNESSES {
            for (name, value) in harness.behavior.environment {
                assert!(
                    !value.starts_with("sk-"),
                    "{} would ship a literal provider key in {name}",
                    harness.id
                );
            }
        }
    }

    #[test]
    fn artifact_kind_tags_round_trip() {
        for kind in [
            ArtifactKind::VendorConfigEntry,
            ArtifactKind::SymlinkReference,
            ArtifactKind::IntegrationOwned,
        ] {
            assert_eq!(ArtifactKind::from_tag(kind.tag()), Some(kind));
        }
        assert_eq!(ArtifactKind::from_tag("SOMETHING_ELSE"), None);
    }
}
