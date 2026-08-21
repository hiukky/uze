# Commands / Actions — research

Companion to [landscape.md](landscape.md). Part 9 of the M3 brief. Lighter
depth than [hooks.md](hooks.md) by design.

| | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| Dedicated concept exists | Legacy only | Yes | Yes | Yes |
| Still recommended | **No — officially merged into Skills** | Yes, coexists with Skills | Yes, coexists with Skills | Yes, coexists with Skills |
| Format | `.claude/commands/*.md` (legacy; identical files now also work as Skills) | Built-ins (~29, e.g. `/plan`, `/skills`, `/fork`, `/review`); third-party extensibility unconfirmed | Markdown, `$ARGUMENTS`/`$1`/`$2`, plus `` !`shell` `` injection at prompt-build time | TOML, namespaced by directory path (`/gcp:deploy`) |
| Package-native | Yes (legacy layout) | Unconfirmed for third-party commands | Yes | Yes, ships in `gemini-extension.json`, **lowest precedence** — auto-prefixed on name conflict |
| Scope | User, project, plugin-namespaced | User/project (built-ins are global) | Global (`~/.config/opencode/commands/`), project (`.opencode/commands/`) | User, project, extension |
| Arguments | Yes, `$ARGUMENTS` | Unresearched | Yes, `$ARGUMENTS`/positional | Unresearched in depth |
| Can execute tools | Yes | Yes | Yes | Yes |

## Central question: portable future primitive, or converging into Skills?

Official Claude Code documentation states outright: *"Custom commands have
been merged into skills. A file at `.claude/commands/deploy.md` and a skill
at `.claude/skills/deploy/SKILL.md` both create `/deploy` and work the same
way."* This is the strongest signal in the entire M3 pass for any single
capability's trajectory — one harness has already completed the convergence
the brief asked whether to expect.

Codex, OpenCode and Gemini CLI have not made the same move: each still treats
Commands as a distinct, first-class concept coexisting with Skills, with real
differences in how they're authored (TOML vs. Markdown vs. built-in-only) and
in scope/precedence rules that share no common shape (Gemini's
lowest-precedence auto-prefixing has no analogue elsewhere researched).

**Recommendation: do not model Commands as a UZE capability.** Not because no
harness has it — three of four do — but because:

1. The harness furthest along (Claude Code) has already collapsed it into a
   capability UZE already delivers (Skills), which means any UZE Commands
   work for Claude Code specifically would be modeling a capability its own
   vendor no longer distinguishes.
2. The other three harnesses' Command formats and precedence rules do not
   converge with each other any more than Hooks' formats do, and none of the
   research this pass turned up suggests Commands are on the same trajectory
   toward a shared open standard that Skills/MCP/AGENTS.md already achieved.
3. A package that wants a callable, argument-taking entry point is already
   served by Skills for the one harness where the distinction has
   disappeared, and by native pass-through for the others — no gap exists
   that a new `CapabilityKind` would close.

This does not mean Commands should be removed from any future research pass
— if a second harness follows Claude Code's lead and folds Commands into
Skills, this section should be revisited, since that would be evidence of the
same standards-first convergence pattern ADR-001/003 already trust for
Skills/MCP/AGENTS.md.
