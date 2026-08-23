## 1. Capture vendor CLI output

- [x] 1.1 Add `shared::process` helpers (`capture`, `run_quiet`, `failed_message`) with unit tests
- [x] 1.2 Switch every mutating `codex` call site (plugin/marketplace add, plugin remove, mcp add/remove) to captured output
- [x] 1.3 Switch every mutating `claude` call site (plugin install, marketplace add, uninstall, mcp add/remove) to captured output
- [x] 1.4 Switch every mutating `gemini` call site (extensions link, mcp add, run_gemini) to captured output
- [x] 1.5 Verify inspection calls stay `.output()` (unchanged) and no vendor stdio is inherited anywhere

## 2. UZE-owned compact report

- [x] 2.1 Add global `--verbose` flag (`Cli` + `ShorthandArgs`)
- [x] 2.2 Implement `render_add_report` (one line per harness; attachments without a plan keep their own line)
- [x] 2.3 Use it in `plugin install` and the project shorthand `add`; keep JSON untouched
- [x] 2.4 Update `tests/cli.rs` assertions to the compact lines

## 3. Validation

- [x] 3.1 `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --no-fail-fast`
- [x] 3.2 `openspec validate --all --strict`
- [x] 3.3 Dogfood `uze plugin install flow@ai` (default: compact, no vendor text; `--verbose`: evidence per harness)
