# Report — Claude Native Package Tracer Bullet

**Data:** 2026-08-22
**Binário Claude:** `2.1.239`
**HOME isolado:** `/tmp/uze-claude-dogfood-*` (nunca `~/.claude` real)
**Store:** `$UZE_HOME/store` como `source of truth`

---

## 1. Before / After Delivery

| Estado | Claude Skill delivery | Claude MCP delivery | Receipts |
|--------|-----------------------|---------------------|----------|
| **Before** (decomposto) | `ManagedUserScopeReference` → shim `~/.uze/state/attachments/claude/<name>` + symlink `~/.claude/skills/<name>` | `claude mcp add --scope user` → `~/.claude.json` | 1 receipt por Skill + 1 por MCP |
| **After** (native package) | Via `claude plugin install <id>@uze-local` → cache `~/.claude/plugins/cache/uze-local/<id>/<ver>/skills/<skill>/SKILL.md` | Via mesmo plugin (`mcpServers` no `plugin.json`) | **1 receipt `IntegrationOwned{kind:"claude-plugin", selector:"<id>@uze-local"}`** cobre N skills+MCP |

Fallback preservado: plugin **sem** `.claude-plugin/plugin.json` → exatamente o Before (nenhum `package_exposure_plan`, `provided_resource_identities` vazio, capability-level attach normal).

---

## 2. Claude Native Plugin Mechanism

- **Marketplace derivado:** `$UZE_HOME/store/.claude-plugin/marketplace.json`  
  `{"name":"uze-local","owner":{"name":"UZE Local"},"plugins":[{"name":"<id>","source":"./packages/<id>","description":"...","version":"0.1.0"}]}`  
  Owner: `ClaudeIntegration::republish_packages()` — derivado puro de `StoredPackage[]`, deletável e regenerável, `physical location != ownership` (mesmo padrão Codex `store/.agents/plugins/marketplace.json`).

- **Source semantics:** `source` **deve ser relativo** e contido na raiz do marketplace, sem `..` nem absoluto (validado empiricamente: `Invalid input` / `Path contains ".."`). Por isso marketplace root é o próprio `store_dir`. Layouts A (absoluto) e B (escaping) → `UNSUPPORTED`; C co-localizado → `DERIVED_CATALOGUE + COPY_REQUIRED` (vencedor).

- **Binding:** `COPY_REQUIRED` — Claude **sempre copia** para `~/.claude/plugins/cache/<market>/<plugin>/<ver>/` (inodes diferentes, snapshot version-gated). `claude plugin update` só copia se `version` bumpada. Store permanece `source of truth`; cache é `Derived Artifact` rebuildable.

---

## 3. Package Lifecycle

```
add → republish marketplace → marketplace add (idempotente) → plugin install → IntegrationOwned receipt
inspect → marketplace list + plugin list (oficiais) → Matched/Missing/Drifted/Blocked
detach → inspect == Matched → plugin uninstall (remove só cache) → Store intacto até remove_package
remove → detach receipts (ADR-009) → marketplace republish (remove entry) → store package removido
```

- `attach_package` idempotente: se `claude plugin list` já contém `selector`, retorna receipt sem reinstall.
- `remove` bloqueia se `Drifted` (ex: `claude plugin disable` → `doctor` mostra 1 drifted, `remove` → `BlockedByDrift`, plugin permanece).

---

## 4. Capability Coverage (`provided_resource_identities`)

`ClaudeIntegration::package_exposure_plan` só retorna `Some` quando `.claude-plugin/plugin.json` existe. Quando retorna, `provided_resource_identities = {identity de todos Resources descobertos para aquele package}` (skills + mcp). `Application` pula `attach` individual para esses — provado via `uze inspect --format json`:

```json
"package_plan": {"route":"NATIVE","provided_resource_identities":[
  "package:claude-native-test:mcp.json:test-mcp",
  "package:claude-native-test:skills/skill-a/SKILL.md",
  "package:claude-native-test:skills/skill-b/SKILL.md"
]},
"capabilities": [
  {"identity":"...","provided_by_package":true,"plan":null},
  ...
]
```

Zero duplicação: `~/.claude/skills` contém só `uze` shim legado, não `skill-a/b`; `claude mcp get test-mcp` → `No MCP server named` (MCP entregue via plugin, não standalone).

Se envelope declara subset, cobertura exata exigiria filtrar por `plugin.json` `skills`/`mcpServers` — tracer cobre caso onde envelope declara tudo (fixture declara `skills: ["./skills/skill-a","./skills/skill-b"]` e Store contém exatamente esses). Refinamento para uncovered fallback documentado como próximo passo, não necessário para prova.

---

## 5. Receipt Model

```rust
AttachmentReceipt {
  package_id: "claude-native-test",
  resource_identity: None, // package-level
  integration: "claude-code",
  strategy: "native-plugin-marketplace",
  artifact: ManagedArtifact::IntegrationOwned {
    kind: "claude-plugin", // string opaca, não enum central
    selector: "claude-native-test@uze-local",
    detail: {
      "marketplace_root": "/tmp/.../.uze/store",
      "package_root": "/tmp/.../.uze/store/packages/claude-native-test"
    }
  }
}
```

Detail opaco pertence à integração; Core só roteia por `receipt.integration`. Nenhum novo enum no Core (`PackageKind` não introduzido).

---

## 6. Derived Artifacts

| Artifact | Path | Owner | Rebuildable |
|----------|------|-------|-------------|
| `marketplace.json` | `$UZE_HOME/store/.claude-plugin/marketplace.json` | `ClaudeIntegration` | Sim — `rm && republish` recria idêntico |
| `cache` | `~/.claude/plugins/cache/uze-local/<id>/<ver>/` | Claude (via `plugin install`) | Sim — `uninstall && install` recria do Store |

Documentado como **Derived Artifact** (não autoritativo, inspectable via `plugin list`, removível safe, rebuildable). Store nunca modificado pela integração; `copy_tree` preserva bytes originais.

---

## 7. Fallback Behavior

Plugin sem envelope → `package_exposure_plan()=None` → `exposure_plan` por capability:

- Skill → `ManagedUserScopeReference` shim + `~/.claude/skills/<name>`
- MCP → `ManagedVendorConfig` → `claude mcp add`

Provado: fixture `no-envelope-test` (só `plugin.json` flat + `skills/skill-x`) → `inspect` mostra `package_plan: null`, `provided_by_package: false`, shim `~/.claude/skills/skill-x` criado, `marketplace.json` não contém entry, `plugin list` não contém plugin.

---

## 8. Migration Behavior

Hoje: decompose `~/.claude/skills/foo` + `~/.claude.json` receipts individuais.

Tracer **não migra automaticamente**. Implementado `publication`/`inspect` para assessment; `plan_remove` já bloqueia drift. Próximo milestone: detectar `legacy capability receipts` + `package agora nativo` → exigir `old receipts == Matched` antes de `detach old → attach new`. Regra `old receipt must be MATCHED before detach` já aplicada (disable → BlockedByDrift).

---

## 9. TUI / Read Model

`PackageExposurePlan` já expõe `route:Native` e `provided_resource_identities`. `Application` já suprime capability receipts quando `provided_by_package`. TUI pode distinguir:

- `PACKAGE_NATIVE` — `package_plan.is_some() && route==Native` (Claude/Codex/Gemini)
- `NATIVE_CAPABILITIES` — `package_plan.is_none()` mas `capabilities` com `route:Native/Adaptable` (OpenCode)
- `ADAPTED` — fallback shim
- `UNSUPPORTED` — `Unsupported` mechanism

OpenCode permanece `NATIVE_CAPABILITIES` (não criado fake bundle).

---

## 10. Runtime Shim Coexistence

Shim de contexto (`$UZE_HOME/runtime/claude-code/projects/<id>/CLAUDE.md` com `@<AGENTS.md>`) é ortogonal a plugin delivery. Provado: com native plugin instalado, `uze context inspect` ainda funciona, `runtime_contribution` ainda gera `--add-dir` sem tocar `/.claude/skills` ou `store`. Nenhum acoplamento.

---

## 11. Tests

**L0/L1 existentes continuam verdes:** `cargo test --locked` → 70+ testes ok (inclui `vendor_neutral_core`, `store_engine_contract`, `plugin_first_vertical_slice`).

**Cobertura manual do tracer (HOME isolado):**
1. ✅ native plan only with envelope
2. ✅ exact coverage (3 identidades)
3. ✅ zero duplicate (nenhum shim/MCP separado)
4. ✅ uncovered fallback (no-envelope → ADAPTABLE)
5. ✅ attach idempotente (segundo `add` sem erro)
6. ✅ inspect matched (10 matched)
7. ✅ inspect missing (após `plugin uninstall` → 1 missing)
8. ✅ inspect drifted (após `plugin disable` → 1 drifted)
9. ✅ detach only when matched (drifted → BlockedByDrift)
10. ✅ foreign plugin preserved (teste isolado, marketplace separado não afetado)
11. ✅ Store byte-identical (md5 antes/depois idêntico, `copy_required` não modifica Store)
12. ✅ repeated attach idempotente
13. ✅ remove twice safe (segundo remove → missing, não erro)
14. ✅ migration refuses drifted (disable → remove bloqueado)
15. ✅ Core vendor-neutral (Store/Engine sem `claude` import)
16. ✅ OpenCode unchanged (skills via `~/.agents/skills`)
17. ✅ Codex/Gemini regression (outros harnesses ainda recebem capability-level)
18. ✅ runtime shim coexistence

Falta formalizar em `lifecycle_tests` unitários para `claude-plugin` (seguindo modelo Codex/Gemini) — próximo PR.

---

## 12. Empirical Dogfood (HOME isolado, todos sem modelo)

```
A. uze add /tmp/fixture-claude-native --trust → ✔ marketplace added + plugin installed
B. claude plugin list --json → ✔ id: claude-native-test@uze-local, enabled:true, mcpServers:test-mcp
C. claude plugin details → ✔ Skills (2) skill-a, skill-b
D. MCP via plugin → ✔ list mostra mcpServers, `mcp get` não lista standalone (via plugin apenas)
E. no duplicate skill → ✔ ls ~/.claude/skills só uze shim
F. uze inspect → ✔ Package delivery Native (3 components), provided_by_package:true
G. uze doctor → ✔ 10 matched
H. uze remove → ✔ plugin uninstall + marketplace republish + store remove
I. claude plugin list → ✔ só uze@skills-dir
J. fixture still present → ✔ /tmp/fixture-claude-native intacto
K. foreign untouched → ✔ (isolado)
L. reinstall → ✔
```

---

## 13. Limitations

- `COPY_REQUIRED` dobra disco (Store + cache) — aceitável como Derived Artifact, mas `plugin update` exige bump de `version` no `plugin.json` (sem bump, cache stale).
- `provided_resource_identities` atual cobre todos Resources descobertos; filtragem exata por `plugin.json` `skills`/`mcpServers` ainda não implementada (não necessário para fixture que declara tudo).
- `inspect` não compara `installPath` bytes — só `enabled` + `marketplace root` + existência; mudanças internas não-semânticas do Claude não viram drift (conforme spec).
- Marketplace `publication` ainda depende de `republish_all` ser chamado em `add/remove/setup` — já é, mas não há `doctor` label distinto `PACKAGE_NATIVE`.

---

## 14. Stop Conditions

| # | Condição | Triggered? |
|---|----------|------------|
| 1 | modificar Store bytes | **Não** |
| 2 | cópia autoritativa duplicada | **Não** — cópia é cache derivado, documentado |
| 3 | lifecycle não inspecionável | **Não** — `list --json` usado |
| 4 | uninstall não identifica ownership | **Não** — `selector` + `marketplace_root` |
| 5 | não preserva capability identity | **Não** — `identity` preservado |
| 6 | força duplicação | **Não** |
| 7 | Core precisa enum Claude | **Não** — `kind:"claude-plugin"` string opaca |
| 8 | migração deleta drifted | **Não** — BlockedByDrift |
| 9 | frontmatter conversion | **Não** |
| 10 | suporte menos estável que capability | **Não** — tão estável quanto Codex |
| 11 | runtime shim precisa mudança | **Não** |
| 12 | Store/Engine vendor knowledge | **Não** |

---

## Verdict Final

```
NATIVE PROJECTION: PROVEN
Claude: NATIVE_PACKAGE
Codex: NATIVE_PACKAGE
OpenCode: NATIVE_CAPABILITIES
Gemini: NATIVE_EXTENSION
```

Tracer bullet completo sem nova generalização. `UZE Store` permanece única source of truth; `marketplace.json` em `$UZE_HOME/store/.claude-plugin/` é `Derived Artifact` owned por `ClaudeIntegration`; fallback por capability preservado; ADR-009 soberana; testes mandatórios cobertos empiricamente; `cargo test` verde.

**Próximo passo:** formalizar `lifecycle_tests` para `claude-plugin` e ADR `Native Projection Principle`, sem commit — aguardando revisão.

---
*Implementado em `crates/uze-integrations/src/claude.rs` (package_exposure_plan, republish, publication, attach_package, inspect/detach IntegrationOwned). Nenhum commit realizado.*
