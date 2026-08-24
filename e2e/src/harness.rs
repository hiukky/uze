//! The harness registry: the only place a new harness is declared.
//!
//! Adding a harness must not mean writing a new runner: L2 scenarios are
//! generic over this table. Each entry declares what the harness reports for
//! each vendor-facing surface, plus the optional L4 route.
//!
//! Deliberately absent: any expected attachment name. Scenario runners read
//! the names UZE actually attached out of `uze inspect --format json` and
//! hand them to the probes, so a fixture rename can never silently pass
//! against a stale constant here.
//!
//! Delivery shapes reflect the CURRENT projection (ADR-020/021/030):
//! Claude and Codex receive generated native packages, OpenCode a shared
//! skill-root symlink, Antigravity a staged plugin copy.

/// The vendor-facing surface a probe asks about.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum ProbeCapability {
    Skill,
    Mcp,
    Package,
}

/// One deterministic probe: a harness subcommand that reports what the
/// harness itself sees, without starting a model turn.
///
/// `required` fragments must appear in the probe's output *in addition to*
/// the attached name the runner discovered. They separate "the harness lists
/// the entry" from "the harness reports it as usable".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Probe {
    pub arguments: &'static [&'static str],
    /// Fragments that must appear. `{mcp_binary}` is substituted with the
    /// resolved fixture server path, which ties a probe to this package even
    /// when the harness renames the capability out of UZE's naming.
    pub required: &'static [&'static str],
    /// Whether the discovered attached name must also appear. A harness that
    /// decomposes a delivered package into capabilities under its own names
    /// cannot be asked about UZE's name, so such a probe identifies the
    /// attachment through `required` instead.
    pub matches_attached_name: bool,
    /// Exactly what a pass establishes, echoed into the evidence record.
    pub claim: &'static str,
}

/// L2 invocation-policy probe (scenario R2): what the harness reports about
/// who may invoke a Skill, model-free. `None` records `Unverified`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyProbe {
    /// Subcommand that renders the model-visible prompt input.
    pub arguments: &'static [&'static str],
    /// Fragments that must appear for the normal Skill (model-visible).
    pub normal_visible: &'static [&'static str],
    /// Fragments that must appear for the user-only Skill (hidden).
    pub user_only_marker: &'static [&'static str],
    /// Exactly what a pass on this probe establishes.
    pub claim: &'static str,
}

/// Files a harness needs in its disposable HOME before an L4 route resolves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderConfig {
    pub relative_path: &'static str,
    pub contents: &'static str,
    /// Directories copied into HOME before the config is written, as
    /// `(absolute source, destination relative to HOME)`.
    pub seed: &'static [(&'static str, &'static str)],
}

/// L4 invocation shape. Placeholders substituted at run time:
/// `{model}`, `{prompt}`, `{workspace}`, `{gateway}`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct L4Spec {
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
    /// L2 probes per vendor-facing capability. `None` for a surface the
    /// harness offers no model-free way to report; the scenario records it
    /// as unavailable rather than inventing evidence.
    pub probes: &'static [(ProbeCapability, Probe)],
    /// L2 invocation-policy evidence (R2); `None` → `Unverified`.
    pub policy: Option<PolicyProbe>,
    /// L2 runtime-shim evidence (R6): run this subcommand *through* the shim
    /// and expect the real executable to answer. `None` → `Unverified`.
    pub shim_probe: Option<&'static [&'static str]>,
    /// `None` for a harness with no gateway-routable L4 route.
    pub l4: Option<L4Spec>,
}

impl HarnessSpec {
    pub fn probe_for(&self, capability: ProbeCapability) -> Option<&'static Probe> {
        self.probes
            .iter()
            .find(|(declared, _)| *declared == capability)
            .map(|(_, probe)| probe)
    }
}

pub const HARNESSES: &[HarnessSpec] = &[
    HarnessSpec {
        id: "claude",
        uze_name: "claude-code",
        executable: "claude",
        // Claude receives the canonical package as a generated native
        // package (ADR-020): the vendor-facing result is one marketplace
        // entry under UZE's generated-only marketplace, reported by Claude's
        // own plugin machinery. The MCP capability is registered through the
        // generated envelope's manifest and checked for connectivity.
        probes: &[
            (
                ProbeCapability::Package,
                Probe {
                    arguments: &["plugin", "marketplace", "list", "--json"],
                    required: &["\"name\"", "\"path\""],
                    matches_attached_name: false,
                    claim: "Claude Code lists UZE's generated marketplace entry in its own plugin catalogue",
                },
            ),
            (
                ProbeCapability::Skill,
                Probe {
                    arguments: &["plugin", "list", "--json"],
                    required: &["\"enabled\": true"],
                    matches_attached_name: true,
                    claim: "Claude Code reports the UZE-generated package containing the Skill as enabled",
                },
            ),
            (
                ProbeCapability::Mcp,
                Probe {
                    // Claude renames plugin-scoped MCP servers to
                    // `plugin:<pkg>:<server>` — the server is identified by
                    // its resolved fixture binary and connection state, not
                    // by UZE's name.
                    arguments: &["mcp", "list"],
                    required: &["Connected", "{mcp_binary}"],
                    matches_attached_name: false,
                    claim: "Claude Code lists the UZE-registered MCP server (plugin-scoped name) and reports it connected",
                },
            ),
        ],
        policy: None,
        shim_probe: Some(&["--version"]),
        l4: Some(L4Spec {
            // `--permission-mode bypassPermissions` is required, not
            // convenience: Claude Code's default mode denies every MCP tool
            // call when no interactive approver exists, which burns turns and
            // surfaces as `error_max_turns` rather than as a permission
            // problem. The container is already externally sandboxed
            // (read-only root, tmpfs-only writes, cap_drop ALL, no provider
            // egress), and this level asserts capability wiring, not the
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
            model: "uze-conformance-reasoning",
            provider_config: None,
        }),
    },
    HarnessSpec {
        id: "codex",
        uze_name: "codex",
        executable: "codex",
        // Codex receives the whole package natively (generated envelope);
        // its receipt is an integration-owned artifact. Codex derives
        // capability names from the installed envelope, which is why the
        // MCP probe identifies the server by its resolved binary path.
        probes: &[
            (
                ProbeCapability::Package,
                Probe {
                    arguments: &["plugin", "list", "--json"],
                    required: &["\"enabled\": true", "\"installed\": true"],
                    matches_attached_name: true,
                    claim: "Codex reports the UZE marketplace plugin installed and enabled",
                },
            ),
            (
                ProbeCapability::Skill,
                Probe {
                    arguments: &["plugin", "list", "--json"],
                    required: &["\"enabled\": true"],
                    matches_attached_name: true,
                    claim: "Codex reports the UZE package (containing the Skill) as installed and enabled",
                },
            ),
            // No MCP probe: `codex mcp list --json` returns no entries for
            // marketplace-plugin servers on the pinned 0.148.0 (and on
            // 0.149.1). Codex offers no model-free enumeration of plugin MCP
            // servers — the scenario records `Unverified` with this reason,
            // never a guessed pass.
        ],
        // `codex debug prompt-input` renders the model-visible prompt without
        // invoking a model. A user-only Skill must not appear there.
        policy: Some(PolicyProbe {
            arguments: &["debug", "prompt-input"],
            normal_visible: &[],
            user_only_marker: &[],
            claim: "Codex's model-visible prompt input includes the default Skill and excludes the user-only Skill",
        }),
        shim_probe: Some(&["--version"]),
        l4: Some(L4Spec {
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
                // The outer boundary is already read-only, tmpfs-only,
                // cap_drop ALL and without provider egress, so Codex's own
                // sandbox is off inside it.
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
        }),
    },
    HarnessSpec {
        id: "opencode",
        uze_name: "opencode",
        // The latest channel installs the V2 preview binary (`opencode2`);
        // the product detects either alias.
        executable: "opencode2",
        // OpenCode has no package-level native concept: Skills are consumed
        // from the shared `.agents/skills` discovery root (symlink) and MCP
        // through the managed `opencode.json` config.
        probes: &[
            (
                ProbeCapability::Skill,
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
            (
                ProbeCapability::Mcp,
                Probe {
                    arguments: &["mcp", "list"],
                    required: &["connected"],
                    matches_attached_name: true,
                    claim: "OpenCode lists the UZE-registered MCP server and reports it connected",
                },
            ),
        ],
        policy: None,
        shim_probe: Some(&["--version"]),
        l4: Some(L4Spec {
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
        }),
    },
    HarnessSpec {
        id: "antigravity",
        uze_name: "antigravity",
        executable: "agy",
        // Antigravity receives the whole package as a staged byte copy
        // (`agy plugin install`); `agy plugin list` is machine-readable
        // JSON (verified against 1.1.19) — the plugin name appears inside
        // `imports` if and only if the install registered it.
        probes: &[
            (
                ProbeCapability::Package,
                Probe {
                    arguments: &["plugin", "list"],
                    required: &["\"imports\"", "\"name\""],
                    matches_attached_name: true,
                    claim: "Antigravity lists the UZE-staged plugin registration in its own import manifest",
                },
            ),
            (
                ProbeCapability::Skill,
                Probe {
                    arguments: &["plugin", "list"],
                    required: &["\"imports\"", "\"name\""],
                    matches_attached_name: true,
                    claim: "Antigravity lists the UZE-staged plugin (carrying the Skill) as imported",
                },
            ),
            (
                ProbeCapability::Mcp,
                Probe {
                    // `agy plugin list` reports the per-plugin import
                    // manifest, which carries the components the staged
                    // plugin registered — including `mcpServers`. `agy mcp
                    // list` only shows *global* servers, so per-plugin MCP
                    // registration is the strongest model-free evidence.
                    arguments: &["plugin", "list"],
                    required: &["mcpServers"],
                    matches_attached_name: true,
                    claim: "Antigravity reports the MCP server component inside the staged plugin's own import manifest (per-plugin registration; `agy mcp list` only lists global servers)",
                },
            ),
        ],
        policy: None,
        shim_probe: Some(&["--version"]),
        l4: None,
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
            if let Some(policy) = harness.policy {
                for fragment in policy
                    .normal_visible
                    .iter()
                    .chain(policy.user_only_marker.iter())
                {
                    assert!(
                        !fragment.contains("uze-"),
                        "{} hardcodes an attachment name fragment in its policy probe: {fragment}",
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
    fn every_l2_probe_capability_has_at_most_one_probe() {
        for harness in HARNESSES {
            let mut seen: Vec<ProbeCapability> =
                harness.probes.iter().map(|(kind, _)| *kind).collect();
            seen.sort_unstable();
            let before = seen.len();
            seen.dedup();
            assert_eq!(
                seen.len(),
                before,
                "{} declares two probes for the same capability",
                harness.id
            );
        }
    }

    #[test]
    fn l4_specs_carry_no_provider_credential() {
        for harness in HARNESSES {
            for (name, value) in harness.l4.iter().flat_map(|spec| spec.environment) {
                assert!(
                    !value.starts_with("sk-"),
                    "{} would ship a literal provider key in {name}",
                    harness.id
                );
            }
        }
    }
}
