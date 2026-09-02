## 1. Architecture suite

- [x] 1.1 Add `tests/architecture/` with rules as data: scope, forbidden prefix, reason, sanctioned list, per-file debt budget.
- [x] 1.2 Encode the layering rule for `src/` against `uze_core::`, freezing today's production violations as budget.
- [x] 1.3 Encode the `uze_integrations::` rule, distinguishing the sanctioned composition-root consumers from debt.
- [x] 1.4 Encode the extension-purity rule: `uze-extensions` never names the domain or orchestration crates.
- [x] 1.5 Make every failure self-explanatory — the rule, the reason, the offending file, and what to do.

## 2. One transport for Git

- [x] 2.1 Add `uze-git` with the spawn convention, per-command exit-code classification, and separate read/write entry points.
- [~] 2.2 Porcelain parsers stay with their single caller: the transport is what had two, and a parser moves when a second caller exists — the same rule the view vocabulary follows.
- [x] 2.3 Port `uze-core::worktree` onto it without behaviour change.
- [x] 2.4 Port `uze-extensions::git_diff` onto it, preserving the `diff`-returns-1 semantics as a command classification rather than a caller quirk.

## 3. Extension view model

- [x] 3.1 Define the view model: the smallest vocabulary `git_diff` needs, serialisable, no ratatui types.
- [x] 3.2 Replace `git_diff::render` with `view`, returning that model; delete the duplicated palette.
- [x] 3.3 Move rendering and geometry to the host, deriving hits from what it drew.
- [x] 3.4 Namespace `ExtensionHit` per extension.
- [x] 3.5 Keep syntax highlighting as semantic roles mapped by the host.

## 4. Trust and documentation

- [x] 4.1 Record the ADR: extension code is a distinct trust class from plugin bytes.
- [x] 4.2 Add the invariants each new test proves.
- [x] 4.3 Restate the layering rules in `AGENTS.md` as enforced facts naming their tests.
