# Changelog

All notable changes to UZE are documented here, generated from Conventional
Commits by `git-cliff` (`make changelog`). Until v1, UZE ships only SemVer
pre-releases — see `docs/versioning.md`.
## [Unreleased]

### CI

- Add GitHub Actions quality pipeline and hide target in VS Code (416c714)


### Chore

- Exclude conformance build artifacts (941be63)

- Add local development make targets (00e1bec)

- Add cross-distro WSL install helper (9492946)

- Archive four completed changes (44a0b53)

- Add lefthook for local pre-commit/pre-push checks (4fdf534)


### Documentation

- Define standards-first UZE architecture (17c682f)

- Characterize the MCP headless approval gap precisely (86d9fd1)

- Map harness ecosystem for local conformance (ad6d53d)

- Pin conformance inference image digest (1a4ec67)

- Explain how tests are organized and where a new one goes (1238ed0)

- Define official harness provisioning boundary (9df36fe)

- Research the M3 capability landscape (2322373)


### Features

- Add Rust project composition CLI (580bdb7)

- Add OpenCode peer integration conformance (4b90428)

- Compose stored environments through integrations (9c17a52)

- Compose project and store environments (5032ccd)

- Enable transparent harness attachment for Claude Code and Codex (4dc9cb8)

- Close Agent Skills behavioral E2E for Claude Code and Codex (c013d6a)

- Enable MCP as UZE's second capability (7780963)

- Add package-centric UZE application lifecycle (bce9fba)

- Add package-centric terminal UI (736b426)

- Add an experimental fourth harness to test the extracted core (d426023)

- Add package provenance, git sources and a consent boundary (45601f6)

- Refine package-centric terminal interface (d7ec530)

- Provision harnesses through integration routes (f217a12)

- Stream official installer progress (d436298)

- Deploy portable test plugin to WSL lab (9ac4a7b)

- Reconcile portable AGENTS.md across all four harnesses (2e8467c)

- Add portable project context reconciliation (bed0794)

- Add portable uze skill and project status (2f90337)

- Short naming, legacy receipt reuse and collision handling (4e36d45)

- Seed builtin official uze skill globally (935cde8)

- Official marketplace contract + generic default-plugin bootstrap (822bf19)

- Rebuild TUI as sidebar-navigated product surface, wire update_plugin (31a37f6)

- Harness compatibility table, drop redundant hints, version in footer (94f39b7)

- Add experimental PATH shim for Claude runtime context projection (f05f0dc)


### Fixes

- Consolidate package lifecycle safety (6bb48f1)

- Make routed conformance gateway ready and identifiable (c338d34)

- Point tooling paths at e2e/ and add OpenCode routed-gateway smoke (4fd0304)

- Prepare detected harnesses during plugin add (1c5b8b8)

- Repair broken test build and opencode provisioning gambiarra (f180ebd)

- Consolidate shared skill naming and polish the TUI (6c771f1)

- Make harness detection deterministic for CI (708c0b0)

- Compact remove confirmation and protect official marketplace plugins (0decfa9)


### Other

- Merge pull request #2 from hiukky/refactor/vendor-neutral-core

feat: consolidate package-centric UZE v0 (e502c06)


### Refactor

- Separate peer harness integrations from core (e515060)

- Make the product suite deterministic end to end (49f2e3a)

- Move vendor knowledge out of the Store and UzeHome (1431760)

- Extract harness-agnostic uze core (0d05758)

- Split application and integrations layers (82f063d)

- Shorten skill and MCP names for real distribution (afc3eb9)


### Style

- Apply cargo fmt (e4bed03)


### Tests

- Add real harness skill conformance playground (a58ff59)

- Consolidate fixtures by test boundary (9a40767)

- Run harness conformance with economical models (a32b754)

- Add isolated harness lab spike (9d4c14c)

- Add minimal local inference compose contract (d5e87c1)

- Add dynamic conformance proof evidence (6d93517)

- Route conformance through isolated LiteLLM gateway (e013d0f)

- Classify blocked Groq conformance smoke (0c16d60)

- Relocate harness conformance lab to e2e/ and add OpenCode E2E driver (3bd4eb1)

- Separate conformance evidence by determinism (3189a19)


