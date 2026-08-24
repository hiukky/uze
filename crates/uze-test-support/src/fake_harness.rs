//! Reusable fake process boundary: `FakeHarness`.
//!
//! Instead of each test inventing its own temporary shell script, a fake
//! executable is declared as a rule table:
//!
//! ```ignore
//! let claude = fake_harness::FakeHarness::new(&env.fake_bin, "claude")
//!     .version_line("9.9.9 (Fake Claude)")
//!     .on_prefix(["plugin", "list"], Action::stdout(r#"{"imports":[]}"#))
//!     .on_prefix(["mcp", "add"], Action::mcp_entry_mark(&state_dir))
//!     .build();
//! ```
//!
//! Every invocation is appended to a per-harness log inside the fake bin
//! directory; [`FakeHarness::invocations`] reads it back so tests can assert
//! which commands a workflow actually shelled out to (`assert_calls`-shaped
//! evidence) instead of trusting that a script "probably ran".
//!
//! Rule semantics: first matching rule wins (rules are matched in insertion
//! order); the fallback is `exit 0`. Scripts are POSIX `sh` — the same
//! mechanism the existing tests used, but centralized and self-describing.
//! On non-UNIX platforms the builder panics rather than silently producing a
//! script the test environment cannot execute.

use std::path::{Path, PathBuf};
use std::process::Command;

/// One rule's behavior when its pattern matches.
pub enum Action {
    /// Print `stdout` (with a trailing newline) and exit 0.
    Stdout(String),
    /// Exit with `code` and no output.
    Exit(i32),
    /// Raw POSIX `sh` lines inserted verbatim into the `case` arm (the
    /// escape hatch for vendor shapes too odd to model; the arm must end in
    /// `exit` or fall through to the case end).
    Script(String),
    /// Touch a marker file and exit 0.
    TouchFile(PathBuf),
    /// Simulate a vendor `mcp add` state transition: skip
    /// `--scope <x>`/`--transport <y>`/`--`, take the next token as the
    /// entry name, touch `<state_dir>/<name>`, exit 0.
    McpEntryMark(PathBuf),
    /// Simulate a vendor `plugin install <source>`: copy the source
    /// directory (argv position `arg_index`, 1-based shell position) into
    /// `<dest>/<basename>` and exit 0.
    CliPluginInstall { dest: PathBuf, arg_index: usize },
    /// The full vendor plugin-marketplace lifecycle as a state machine:
    /// `plugin marketplace add|list`, `plugin install|add <sel>`,
    /// `plugin list`, `plugin uninstall|remove <sel>`, with persisted
    /// state under `state_dir` (so install and inspection agree across
    /// separate `uze` invocations). Shape differs per vendor: Claude
    /// answers arrays of `{name, path}` / `{id, enabled}`, Codex answers
    /// `{marketplaces:[{name,root}]}` / `{installed:[{pluginId,...}]}`.
    VendorMarketplace {
        state_dir: PathBuf,
        vendor: MarketplaceVendor,
    },
    /// Antigravity's stub-install lifecycle: `plugin install <root>`
    /// stages a byte copy under `dest/<basename>` and `plugin list`
    /// answers `{"imports":[{"name":...}]}` from persisted state.
    VendorAgy { state_dir: PathBuf, dest: PathBuf },
}

/// Vendor flavor for [`Action::VendorMarketplace`].
#[derive(Clone, Copy)]
pub enum MarketplaceVendor {
    Claude,
    Codex,
}

impl Action {
    /// Convenience: `Stdout` from a string literal.
    pub fn stdout(text: impl Into<String>) -> Action {
        Action::Stdout(text.into())
    }
}

enum Pattern {
    /// Exact match of the full argv joined by spaces.
    Exact(String),
    /// Prefix match: argv starts with these tokens.
    Prefix(String),
    /// Special-case `--version` (the default behavior is a version echo).
    Version,
}

struct Rule {
    pattern: Pattern,
    action: Action,
}

/// Mutable rule-table builder; `.build()` materializes the executable.
pub struct FakeHarnessBuilder {
    name: String,
    bin_dir: PathBuf,
    invocations_dir: PathBuf,
    rules: Vec<Rule>,
    version_line: String,
}

impl FakeHarnessBuilder {
    /// `on` with an exact full-argv match.
    pub fn on(mut self, argv: impl AsRef<[&'static str]>, action: Action) -> Self {
        self.rules.push(Rule {
            pattern: Pattern::Exact(argv.as_ref().join(" ")),
            action,
        });
        self
    }

    /// `on_prefix` with a leading-token match (`["mcp", "add"]` matches
    /// `mcp add name -- cmd ...`).
    pub fn on_prefix(mut self, argv: impl AsRef<[&'static str]>, action: Action) -> Self {
        self.rules.push(Rule {
            pattern: Pattern::Prefix(argv.as_ref().join(" ")),
            action,
        });
        self
    }

    /// Overrides the `--version` answer (default: the harness name).
    pub fn version_line(mut self, line: impl Into<String>) -> Self {
        self.version_line = line.into();
        self
    }

    /// Writes the executable into `bin_dir` with mode 0o755 and returns the
    /// ready-to-assert handle.
    pub fn build(self) -> FakeHarness {
        #[cfg(not(unix))]
        panic!(
            "FakeHarness generates POSIX sh scripts; supported on Unix only ({})",
            self.name
        );

        let (script_path, log_path) = {
            let script_path = self.bin_dir.join(&self.name);
            let log_path = self.invocations_dir.join(format!("{}.log", self.name));
            (script_path, log_path)
        };

        let mut script = format!(
            "#!/bin/sh\n# fake harness '{}' generated by uze-test-support\n\
             echo \"$*\" >> '{}'\ncase \"$*\" in\n",
            self.name,
            log_path.display()
        );

        // `--version` echoes the declared line unless a rule claims it first.
        let version_rule = Rule {
            pattern: Pattern::Version,
            action: Action::stdout(self.version_line.clone()),
        };
        for rule in std::iter::once(version_rule).chain(self.rules) {
            match rule.pattern {
                Pattern::Version => {
                    script.push_str("  --version)\n");
                }
                Pattern::Exact(expected) => {
                    // Quotes are mandatory: the pattern is a single shell
                    // word (dash rejects space-separated words), and our
                    // argv joins tokens with spaces.
                    script.push_str(&format!("  \"{expected}\")\n"));
                }
                Pattern::Prefix(prefix) => {
                    // `"tokens"*)` — one shell word: quoted literals plus an
                    // UNQUOTED wildcard (dash treats a wholly-quoted `*` as
                    // a literal, which would never match).
                    script.push_str(&format!("  \"{prefix}\"*)\n"));
                }
            }
            script.push_str(&emit_action(&rule.action));
            script.push_str("  ;;\n");
        }

        script.push_str("esac\nexit 0\n");

        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&script_path, script).unwrap_or_else(|error| {
            panic!(
                "FakeHarness: failed to write {}: {error}",
                script_path.display()
            )
        });
        let mut permissions = std::fs::metadata(&script_path)
            .unwrap_or_else(|error| {
                panic!(
                    "FakeHarness: failed to stat {}: {error}",
                    script_path.display()
                )
            })
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script_path, permissions).unwrap();

        FakeHarness {
            name: self.name,
            script_path,
            invocations_dir: self.invocations_dir,
        }
    }
}

fn emit_action(action: &Action) -> String {
    match action {
        Action::Stdout(text) => format!("    echo '{}'\n    exit 0\n", text.replace('\'', "'\\''")),
        Action::Exit(code) => format!("    exit {code}\n"),
        Action::Script(lines) => {
            let mut block = String::new();
            for line in lines.lines() {
                block.push_str("    ");
                block.push_str(line);
                block.push('\n');
            }
            block
        }
        Action::TouchFile(path) => format!("    touch '{}'\n    exit 0\n", path.display()),
        Action::McpEntryMark(state_dir) => {
            let mut block = String::from("    shift 2\n");
            block.push_str("    name=\"\"\n");
            block.push_str("    while [ \"$#\" -gt 0 ]; do\n");
            block.push_str("      case \"$1\" in\n");
            block.push_str("        --scope|--transport) shift 2 ;;\n");
            block.push_str("        --) shift ; break ;;\n");
            block.push_str("        *) name=\"$1\" ; shift ;;\n");
            block.push_str("      esac\n");
            block.push_str("    done\n");
            block.push_str(&format!(
                "    [ -n \"$name\" ] && touch '{}/'\"$name\"\n",
                state_dir.display()
            ));
            block.push_str("    exit 0\n");
            block
        }
        Action::CliPluginInstall { dest, arg_index } => format!(
            "    plugin_dest='{}'\n    mkdir -p \"$plugin_dest\"\n    cp -R \"${}\" \"$plugin_dest/$(basename \"${}\")\" 2>/dev/null || true\n    exit 0\n",
            dest.display(),
            arg_index,
            arg_index
        ),
        Action::VendorMarketplace { state_dir, vendor } => {
            emit_vendor_marketplace(state_dir, *vendor)
        }
        Action::VendorAgy { state_dir, dest } => emit_vendor_agy(state_dir, dest),
    }
}

/// Generates the Antigravity stub-install state-machine script.
fn emit_vendor_agy(state_dir: &Path, dest: &Path) -> String {
    let mut block = String::new();
    block.push_str(&format!("    state_dir='{}'\n", state_dir.display()));
    block.push_str(&format!("    dest='{}'\n", dest.display()));
    block.push_str("    mkdir -p \"$state_dir\"\n");
    block.push_str("    case \"$*\" in\n");
    block.push_str("      \"plugin install\"*)\n");
    block.push_str("        root=\"$3\"\n");
    block.push_str("        id=$(basename \"$root\")\n");
    block.push_str("        mkdir -p \"$dest\"\n");
    block.push_str("        cp -R \"$root\" \"$dest/$id\" 2>/dev/null || true\n");
    block.push_str("        printf '%s\\n' \"$id\" >> \"$state_dir/installed\"\n");
    block.push_str("        exit 0\n        ;;\n");
    block.push_str("      \"plugin list\"*)\n");
    block.push_str("        out=\"\"\n");
    block.push_str("        while IFS= read -r id; do\n");
    block.push_str("          out=\"$out{\\\"name\\\":\\\"$id\\\"},\"\n");
    block.push_str("        done < \"$state_dir/installed\" 2>/dev/null\n");
    block.push_str("        printf '{\"imports\":[%s]}' \"${out%,}\"\n");
    block.push_str("        exit 0\n        ;;\n");
    block.push_str("      \"plugin uninstall\"*)\n");
    block.push_str("        id=\"$3\"\n");
    block.push_str(
        "        [ -n \"$id\" ] && sed -i \"\\|^$id$|d\" \"$state_dir/installed\" 2>/dev/null\n",
    );
    block.push_str("        rm -rf \"$dest/$id\"\n");
    block.push_str("        exit 0\n        ;;\n");
    block.push_str("    esac\n");
    block
}

/// Generates the vendored marketplace state-machine script for
/// [`Action::VendorMarketplace`].
fn emit_vendor_marketplace(state_dir: &Path, vendor: MarketplaceVendor) -> String {
    let state = format!("'{}'", state_dir.display());
    let mut block = String::new();
    block.push_str(&format!("    state_dir={state}\n"));
    block.push_str("    mkdir -p \"$state_dir\"\n");
    // `plugin marketplace add <root>`: remember root and marketplace name
    // (read out of the catalogue UZE itself wrote; default `uze-store`).
    block.push_str("    case \"$*\" in\n");
    block.push_str("      \"plugin marketplace add\"*)\n");
    block.push_str("        root=\"\"\n");
    block.push_str("        for arg in \"$@\"; do root=\"$arg\"; done\n");
    block.push_str("        printf '%s' \"$root\" > \"$state_dir/root\"\n");
    block.push_str(
        "        name=$(sed -n 's/.*\"name\" *: *\"\\([^\"]*\\)\".*/\\1/p' \"$root/marketplace.json\" \"$root/.claude-plugin/marketplace.json\" \"$root/.agents/plugins/marketplace.json\" 2>/dev/null | head -1)\n",
    );
    block.push_str("        [ -n \"$name\" ] || name=\"uze-store\"\n");
    block.push_str("        printf '%s' \"$name\" > \"$state_dir/name\"\n");
    block.push_str("        exit 0\n");
    block.push_str("        ;;\n");
    // `plugin install|add <selector>`: record the selector.
    block.push_str("      \"plugin install\"*|\"plugin add\"*)\n");
    block.push_str("        sel=\"$3\"\n");
    block.push_str(
        "        [ -n \"$sel\" ] && printf '%s\\n' \"$sel\" >> \"$state_dir/installed\"\n",
    );
    block.push_str("        id=\"${sel%%@*}\"\n");
    block.push_str("        root=$(cat \"$state_dir/root\" 2>/dev/null)\n");
    block.push_str("        [ -n \"$root\" ] && mkdir -p \"$root/$id\"\n");
    block.push_str("        exit 0\n");
    block.push_str("        ;;\n");
    match vendor {
        MarketplaceVendor::Claude => {
            block.push_str("      \"plugin marketplace list\"*)\n");
            block.push_str("        root=$(cat \"$state_dir/root\" 2>/dev/null)\n");
            block.push_str("        name=$(cat \"$state_dir/name\" 2>/dev/null)\n");
            block.push_str("        if [ -n \"$root\" ]; then\n");
            block.push_str(
                "          printf '[{\"name\":\"%s\",\"path\":\"%s\"}]' \"$name\" \"$root\"\n",
            );
            block.push_str("        else\n          printf '[]'\n        fi\n");
            block.push_str("        exit 0\n        ;;\n");
            block.push_str("      \"plugin list\"*)\n");
            block.push_str("        out=\"\"\n");
            block.push_str("        while IFS= read -r sel; do\n");
            block.push_str(
                "          entry=$(printf '{\"id\":\"%s\",\"enabled\":true}' \"$sel\")\n",
            );
            block.push_str("          out=\"$out$entry,\"\n");
            block.push_str("        done < \"$state_dir/installed\" 2>/dev/null\n");
            block.push_str("        printf '[%s]' \"${out%,}\"\n");
            block.push_str("        exit 0\n        ;;\n");
            block.push_str("      \"plugin uninstall\"*)\n");
            block.push_str("        sel=\"$3\"\n");
            block.push_str("        [ -n \"$sel\" ] && sed -i \"\\|^$sel$|d\" \"$state_dir/installed\" 2>/dev/null\n");
            block.push_str("        exit 0\n        ;;\n");
        }
        MarketplaceVendor::Codex => {
            block.push_str("      \"plugin marketplace list\"*)\n");
            block.push_str("        root=$(cat \"$state_dir/root\" 2>/dev/null)\n");
            block.push_str("        name=$(cat \"$state_dir/name\" 2>/dev/null)\n");
            block.push_str("        if [ -n \"$root\" ]; then\n");
            block.push_str(
                "          printf '{\"marketplaces\":[{\"name\":\"%s\",\"root\":\"%s\"}]}' \"$name\" \"$root\"\n",
            );
            block.push_str("        else\n          printf '{\"marketplaces\":[]}'\n        fi\n");
            block.push_str("        exit 0\n        ;;\n");
            block.push_str("      \"plugin list\"*)\n");
            block.push_str("        out=\"\"\n");
            block.push_str("        root=$(cat \"$state_dir/root\" 2>/dev/null)\n");
            block.push_str("        name=$(cat \"$state_dir/name\" 2>/dev/null)\n");
            block.push_str("        while IFS= read -r sel; do\n");
            block.push_str("          id=\"${sel%%@*}\"\n");
            block.push_str("          entry=$(printf '{\"pluginId\":\"%s\",\"enabled\":true,\"installed\":true,\"marketplaceName\":\"%s\",\"path\":\"%s/%s\"}' \"$sel\" \"$name\" \"$root\" \"$id\")\n");
            block.push_str("          out=\"$out$entry,\"\n");
            block.push_str("        done < \"$state_dir/installed\" 2>/dev/null\n");
            block.push_str("        printf '{\"installed\":[%s]}' \"${out%,}\"\n");
            block.push_str("        exit 0\n        ;;\n");
            block.push_str("      \"plugin remove\"*)\n");
            block.push_str("        sel=\"$3\"\n");
            block.push_str("        [ -n \"$sel\" ] && sed -i \"\\|^$sel$|d\" \"$state_dir/installed\" 2>/dev/null\n");
            block.push_str("        exit 0\n        ;;\n");
        }
    }
    block.push_str("    esac\n");
    block
}

/// Materialized fake executable plus its invocation log.
pub struct FakeHarness {
    name: String,
    script_path: PathBuf,
    invocations_dir: PathBuf,
}

impl FakeHarness {
    /// A builder for a fake executable named `name` inside `bin_dir` (see
    /// [`FakeHarness::build`] — the builder's `new` is the entry point; the
    /// materialized handle comes from `.build()`).
    #[allow(clippy::new_ret_no_self)] // the fluent builder shape is the point
    pub fn new(bin_dir: &Path, name: &str) -> FakeHarnessBuilder {
        std::fs::create_dir_all(bin_dir).expect("FakeHarness: bin dir must be creatable");
        std::fs::create_dir_all(bin_dir.join(".invocations"))
            .expect("FakeHarness: invocation log dir must be creatable");
        FakeHarnessBuilder {
            name: name.to_owned(),
            bin_dir: bin_dir.to_path_buf(),
            invocations_dir: bin_dir.join(".invocations"),
            rules: Vec::new(),
            version_line: format!("{name} fake 0.0.0"),
        }
    }

    /// The executable path (for `PATH`-based resolution checks).
    pub fn path(&self) -> PathBuf {
        self.script_path.clone()
    }

    /// A `Command` that spawns this fake with the caller-supplied args.
    pub fn command(&self) -> Command {
        Command::new(&self.script_path)
    }

    /// Parsed invocation log: one entry per call, tokens split on
    /// whitespace. Empty when nothing was invoked.
    pub fn invocations(&self) -> Vec<Vec<String>> {
        let log = self.invocations_dir.join(format!("{}.log", self.name));
        match std::fs::read_to_string(&log) {
            Ok(contents) => contents
                .lines()
                .map(|line| line.split_whitespace().map(str::to_owned).collect())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// How many times the fake was invoked at all.
    pub fn call_count(&self) -> usize {
        self.invocations().len()
    }

    /// True when some invocation matches `argv` exactly.
    pub fn was_called_with(&self, argv: &[&str]) -> bool {
        self.invocations().iter().any(|call| {
            call.len() == argv.len()
                && call
                    .iter()
                    .zip(argv.iter())
                    .all(|(actual, expected)| actual == expected)
        })
    }

    /// True when some invocation starts with `argv`.
    pub fn was_called_with_prefix(&self, argv: &[&str]) -> bool {
        self.invocations().iter().any(|call| {
            call.len() >= argv.len()
                && call
                    .iter()
                    .zip(argv.iter())
                    .all(|(actual, expected)| actual == expected)
        })
    }
}
