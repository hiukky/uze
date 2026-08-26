# Test fixtures

Every persistent test input lives here, in four kinds. Fixtures are
*inputs*, never `$UZE_HOME`: each test installs them into an isolated
[`TestEnvironment`](`crates/uze-testkit`) Store.

| Kind | What it holds | Who consumes it |
|---|---|---|
| `canonical/` | small, stable, valid UZE-authored packages | unit/component/conformance tests (any level) |
| `foreign/` | vendor-native formats integrations must translate | integration conformance (`tests/integrations/**`) |
| `scenarios/` | deliberate broken/edge *static* states | acceptance + negative tests |
| `golden/` | the one evolving "North Star" environment | acceptance only (`tests/acceptance/**`) |

## canonical/

Valid `marketplace.json`-authorable inputs. Keep them SMALL — one fixture proves
one shape; adding "just one more feature" to every fixture is what made the
old `packages/` set hard to reason about.

- `skill-plugin/` — one standard Agent Skill (`skills/uze-e2e`), no vendor
  envelope, no MCP. The ubiquitous baseline package.
- `mcp-plugin/` — MCP-only package (`mcp.json`, no `skills/`); keeps the
  Skill and MCP discovery paths independent by construction (ADR-007
  non-goal). `mcp.json`'s command placeholder
  (`__UZE_MCP_FIXTURE_BINARY__`) is resolved by tests to
  `CARGO_BIN_EXE_uze-mcp-conformance-fixture` — see its `README.md`.
- `multi-mcp-plugin/` — two MCP servers in one package; proves one package
  composes into N named resources.
- `instructions-a/`, `instructions-b/` — packages contributing `AGENTS.md`
  instruction regions; the two-package region/bridge scenarios use them.
- `flow/` — `flow` package with one Skill `commit`; the canonical
  marketplace/consumer example used all over lifecycle tests.
- `workflow/` — `workflow` package with one Skill `review`; the
  invocation-policy carrier (its `SKILL.md` defines `invoke` blocks).

## foreign/

Vendor-native layouts. These are deliberately NOT canonical: they model
what a vendor's own tooling ships, which UZE must *translate*, never treat
as its own format. One per vendor, minimal:

- `claude/native-plugin/` — `.claude-plugin/plugin.json` envelope.
- `codex/native-plugin/` — `.codex-plugin/plugin.json` envelope (copied
  from the conformance lab's legacy plugin-first-conformance shape).
- `opencode/native-plugin/` — OpenCode has no plugin envelope; its native
  surface is the shared `.agents/skills` layout.
- `antigravity/native-plugin/` — Antigravity reads the canonical
  `plugin.json` itself as its manifest; this fixture carries vendor
  metadata fields on top.

## scenarios/

- `eval/` — the agentic model-behavioral evaluation set (healthy-portable,
  drifted-region, …). L4 evidence: exercised by
  `docs/capabilities/uze-skill.md`'s manual/agentic eval, never CI.
- `malformed-lock/` — `agents.lock` referencing a marketplace that does not
  exist.
- `malformed-marketplace/` — `marketplace.json` with a plugin lacking required
  fields.
- `nested-workspace/` — workspace root with a nested project (`apps/web`).

Scenarios that depend on UZE-generated state (drifted receipts, projection
conflicts, corrupted stores) cannot be static files — receipts carry
machine-specific absolute paths. They are built by the scenario builders in
`crates/uze-testkit` (apply UZE, then mutate), and named by the
behavior they exercise.

## golden/

The one evolving acceptance environment. `marketplace/` is the canonical
marketplace (its `plugins/flow` mirrors `canonical/flow`); `project/` is
the canonical project content. The acceptance suite materializes the
marketplace into a fresh `TestEnvironment` and lets UZE write `agents.lock`
itself — a checked-in lock could not carry a portable absolute path. It
proves the current canonical product story end-to-end (see
`tests/acceptance/fresh_project.rs::golden_environment_is_healthy`).

## Adding a fixture

Only add a fixture a test actually consumes, and say why in this file. If a
fixture loses every consumer, delete it — do not keep history in fixtures.

## control/ e bin/

`tests/_fixtures/` é a árvore de fixtures dos testes Rust determinísticos,
consumida via `uze-testkit`. O Lab mantém o seu marketplace final isolado em
`conformance/_fixtures/marketplace/`: ele contém a combinação evolutiva de
Skills e MCP exercida pelos harnesses reais, sem alterar os inputs pequenos e
estáveis desta árvore.

- `control/native-skill-discovery/` — dado do **control C1 do Lab**: um
  projeto com `.agents/skills/uze-e2e/` e **UZE ausente**. Não é canônico
  (não é um pacote UZE-authored); mede o caminho harness/provider isolado.
- `bin/mcp_conformance_fixture.rs` — **fonte do binário de teste**
  `uze-mcp-conformance-fixture` (não um dado): compilado pelo `Cargo.toml`
  raiz e usado tanto pelo MCP fixture determinístico quanto pelo marketplace
  isolado do Lab.
