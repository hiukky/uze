# Changelog

All notable changes to UZE are documented here, generated from Conventional
Commits by `git-cliff` (`make changelog`). Until v1, UZE ships only SemVer
pre-releases — see `docs/versioning.md`.
## [0.0.0-alpha.1]

### CI

- Add GitHub Actions quality pipeline and hide target in VS Code (c15bb25)

- Add audit job for RustSec vulnerability check (abfd304)

- Fix audit-check token and allow Node 20 (30480fc)

- Run all harness verticals (d7f0b25)

- Add one-click release workflow (97015f9)

- Lint and exercise the installer in CI (be04707)

- Optimize coverage pipeline speed and add local coverage target (969a343)

- Run UZE's own vertical beside the four harnesses (530ce91)

- Run the conformance unit tests (b3c7b3f)

- Run the whole workspace, not the root crate (21f560a)

- Install cargo-audit as a prebuilt binary instead of building it per run (95bf3b6)

- Scan every commit for secrets (#10) (b5ee527)

- Audit dependency licences, generate the attribution, credit the marks (#12) (84bada6)

- Version the repository rules, and close the four audit gaps (#14) (ef6a1e3)


### Chore

- Exclude conformance build artifacts (a9b6725)

- Add local development make targets (9cf8f1e)

- Add cross-distro WSL install helper (90f1623)

- Archive four completed changes (25e909b)

- Add lefthook for local pre-commit/pre-push checks (3564514)

- Add cargo-release/git-cliff release tooling (74e4013)

- Update rmcp/schemars/ratatui/crossterm, fix CI, add Dependabot (5cc2f56)

- Remove stale native package tracer report (72a44a4)

- Bump actions/upload-artifact from 4 to 7 (cf82bab)

- Bump version to 0.0.0-alpha.9 for local install (2dccaa9)

- Track this repo's own agents.lock (8089699)

- Bump version to 0.0.0-alpha.10 for local install (a656f05)

- Bump actions/checkout from 4 to 7 (5cc933b)

- Bump version to 0.0.0-alpha.12 for local install (7388717)

- Bump version to 0.0.0-alpha.13 for local install (376d45b)

- Bump version to 0.0.0-alpha.14 for local install (2aa7b8a)

- Clarify that make install is a force-rebuild without version bump (96ad5a5)

- Prefix fixture directories with underscore so they sort to the top (62a5e7b)

- Migrate openspec operator skills to the shared .agents layout (fc14bd3)

- Remove lab-watch live follower (9716ca2)

- Archive completed changes (4526fcc)

- Add fmt, lint and check aggregate targets (cd92842)

- Lint and format Python with ruff (ef65ef2)

- Bump actions/download-artifact from 7 to 8 (d50dada)

- Bump the cargo-dependencies group with 2 updates (4c00c6e)

- Exclude build artifacts from VS Code explorer and search (e228d21)

- Switch worktree completion to merge (7f39858)

- Scaffold the contributor licence agreement (#13) (09e9407)


### Documentation

- Define standards-first UZE architecture (2b7c041)

- Characterize the MCP headless approval gap precisely (39c5cbf)

- Map harness ecosystem for local conformance (bf74c51)

- Pin conformance inference image digest (cb90e82)

- Explain how tests are organized and where a new one goes (0e7b907)

- Define official harness provisioning boundary (fba7d0d)

- Research the M3 capability landscape (7aecee6)

- Rewrite README as a technical landing page (9d83b5b)

- Add evidence-graded per-harness compatibility READMEs (9008c61)

- Remove superseded implementation status pointer (a74a881)

- Rewrite AGENTS.md as the portable agent baseline (3a1f572)

- Deterministic harness x feature matrix + lean top-level readme (4666ae0)

- Regenerate harness matrix for portable hooks (21819ff)

- Document the one-click release workflow (f12dd90)

- Adopt GitHub Releases as the Linux distribution channel (7608dfe)

- Move the harness matrix and detail content into the docs site (3c8a220)

- Sync harness matrix (a483dfe)

- Add conformance-debug skill and point AGENTS.md at it (d2a93b0)

- Consolidate 39 records into 25 (2a37d20)

- Propose enforcing the architecture seams (27003df)

- State the seams as enforced facts (7aab887)

- Say why the transport is a crate, and drop a glyph that never shipped (fd63b62)

- Record the registry-driven CLI as not taken, and why (af918a9)

- Record the boundary work that happened outside a change (1ed096b)

- Record why the Codex envelope carries bytes instead of symlinks (d029b61)

- Record why the Claude and Antigravity hook phases now fail (6efbf25)

- State what the capability contract holds (458a17b)

- Close out the capability contract's tasks (ad20173)

- Teach the debug skill to check the vendor before the code (38a0fa1)

- Propose native-first hooks (9287cac)

- Propose plugin requirements (c078231)

- Plugin requirements — UZE explains, the person installs (3fd46ce)

- Propose the Antigravity signed-in Lab (ea1c2b3)

- Write down the contract a delivered hook actually speaks (c2fa2d5)

- Record what the Lab now exercises and why (fff9ff1)

- Record what the new hook delivery guarantees (d59bb31)

- Name what actually holds Antigravity's hook gate shut (e8b38de)

- Cite the vendor bug behind Antigravity's closed hook gate (1579499)

- State the two gates a delivered Antigravity hook must pass (8bb3e75)

- Record Antigravity's delivery route and its denial exit code (06265ea)

- Regenerate the harness matrix for the compiled hook delivery (dde9313)

- Evolve the worktree policy change to the slot model (efe9488)

- Archive five completed changes and record ADRs 041-043 (0ebf3b8)

- Add CONTRIBUTING.md with the rules a change has to follow (990c117)

- Describe what uze does today, and stop the matrix understating it (71d6d84)

- Describe the README video as a spec (29dc400)

- Add NOTICE, security policy and trademark notice (#11) (1860288)


### Features

- Add Rust project composition CLI (502554c)

- Add OpenCode peer integration conformance (89e3256)

- Compose stored environments through integrations (26b66c3)

- Compose project and store environments (6fed786)

- Enable transparent harness attachment for Claude Code and Codex (a09f0cc)

- Close Agent Skills behavioral E2E for Claude Code and Codex (f6b5a79)

- Enable MCP as UZE's second capability (4330325)

- Add package-centric UZE application lifecycle (e2c29b5)

- Add package-centric terminal UI (dea652b)

- Add an experimental fourth harness to test the extracted core (ee4c4a6)

- Add package provenance, git sources and a consent boundary (e38b0d0)

- Refine package-centric terminal interface (a329d30)

- Provision harnesses through integration routes (8f1e4f8)

- Stream official installer progress (9fd79bc)

- Deploy portable test plugin to WSL lab (2058196)

- Reconcile portable AGENTS.md across all four harnesses (3eabb84)

- Add portable project context reconciliation (3b45217)

- Add portable uze skill and project status (d4dc682)

- Short naming, legacy receipt reuse and collision handling (3d7ff98)

- Seed builtin official uze skill globally (2a0422d)

- Official marketplace contract + generic default-plugin bootstrap (a1922d8)

- Rebuild TUI as sidebar-navigated product surface, wire update_plugin (2af8b0b)

- Harness compatibility table, drop redundant hints, version in footer (a0c0163)

- Add experimental PATH shim for Claude runtime context projection (79027ba)

- Automate PATH integration for the harness shim (12c72ca)

- Establish native-first plugin delivery (5ffc135)

- Safe decomposed to native migration for exact coverage (ac8fe03)

- Add marketplace registry and plugin install via name@marketplace (7ae67fb)

- Implement Project Agent Environment with agents.lock (9e55f2f)

- Improve CLI help with colors and shorter descriptions (bf0154e)

- Reorganize CLI commands by category and improve help (c6081ea)

- Reorganize CLI commands, add colored help, and progress feedback (61e3ee0)

- Cache harness detection to make CLI ops millisecond-fast (45357ed)

- Support multiple marketplaces in browse, install, and add flows (fdc5012)

- Rebuild TUI to match imported design; fix harness provisioning bugs (6bd9147)

- Harness status glossary and friendlier status display (b52c4d0)

- Redesign command grammar around explicit Project/Machine boundary (435bdc0)

- Add Generated Native Package projection tier; generalize runtime-shim boundary (eb95c8c)

- Extend Generated Native Package to Codex/Gemini; add Integration Conformance Test Suite (254a6ae)

- Rename marketplace root manifest to agents.json [**breaking**] (e459bae)

- Workspace-aware Overview with semantic health states (57ddd2e)

- Add Command as a first-class capability (c3e288e)

- Expose stable namespaced invocation labels (841d683)

- Make Antigravity CLI the Google-family v0 harness (d17b565)

- Replace canonical Command with Skill + invocation policy (6eceb2e)

- Python Real-Harness + Synthetic World lab, vertical per harness (3ce45ef)

- Add opencode vertical to the Python lab (df2b0d1)

- Lab-watch becomes a live follower of the newest TUI recording (03ac435)

- Add isolated marketplace fixture (3924872)

- Migrate integration to v2 (071cfc0)

- Promote MCP to native via opencode mcp add CLI (4a5e169)

- Improve CLI header formatting (3f7e00b)

- Add native harness projections (e9abe5f)

- Add canonical hook model and discovery (78795de)

- Deliver portable hooks across all four harnesses (a8a6eec)

- Add official Linux install script (3c73d78)

- Align projections with real harness contracts and lab evidence (2cb8c8b)

- Gate evidence integrity and agent exploration modes (544265c)

- Add Fumadocs documentation site (9695781)

- Unify harness display names across CLI, TUI, and docs (e27c384)

- Redesign docs site UI/UX, add plugin-authoring guide, fix Antigravity invocation prefix (30ed520)

- Reconcile safe environment state (5acbf5f)

- Reconcile orphaned receipts left by package renames (9499ced)

- Unify workspace and harness setup (ec2648f)

- Add Profiles/Preferences vertical slice (d69a4bd)

- Add persistent terminal workspace (0978e7b)

- Add agent-launching tabs and split the TUI into per-mode modules (21576f1)

- Group workspace tabs into persistent, agent-aware Spaces (d4de376)

- Add contextual git diff overlay (91e7da6)

- Refine git worktree changes navigator (3104382)

- Polish the workspace toggle, tab strip, and git changes overlay (710f461)

- Report xterm mouse-tracking mode on pane snapshots/damage (023106b)

- Surface per-harness agent support and bracketed paste (48d6f6e)

- Animate in-flight agents and make the context menu a real menu (f3417cd)

- Add the /uze:worktree concurrent-worktree coordinator (6e81603)

- Add extensions and improve agent support (5f98f64)

- Refine catalog and detail panels (10a74c2)

- Record and browse agent prompt history (a7f9914)

- Isolate concurrent agents at launch and project the policy [**breaking**] (144e98d)

- Apply the updates uze can settle on its own when it opens (6380b42)

- Mark which isolated checkout an agent tab runs in (948aa7a)

- Make the portable tool vocabulary carry its fields (d19eb87)

- Handlers read the context from the environment, answer with an exit code [**breaking**] (a154155)

- Compile the hook into a wrapper the harness runs directly (ff727d2)

- The OpenCode plugin becomes the wrapper (8a141a3)

- Report which route delivered a hook, and what it needs (87f3379)

- Serve Antigravity's feature-flag plane in the synthetic world (cbafa36)

- Serve Antigravity's signed-in plane in the synthetic world (2dfc669)

- Take the repository write lock in write (c5603b8)

- Add the task model and retire linked-worktree discovery (b89f021)

- Reusable checkout slots with adoption and safe pruning (6625693)

- Every agent starts in a slot of its own; the seat rule is removed [**breaking**] (002fdc2)

- Readiness and delivery of a task's work (f49ea2b)

- Tasks are evaluated when a pane goes quiet and delivered with one action (7151aac)

- Project the slot model, rewrite the Skill, and prove the engine end to end (e7494cb)

- One server per user, spaces born from a root, focus per client [**breaking**] (b61b636)

- A slot belongs to its repository, and returns to the pool (fc490fd)

- The new-space prompt chooses a root from the directories that exist (54f0477)

- The open root picker has the sidebar to itself (27fe921)

- A tab belongs with the agent it was born from [**breaking**] (6376e9d)

- Status marks read by color, and say what they mean (bfdbd6f)

- Add isolation contract proving worktree scoping in a real harness (40db70f)

- Drag-to-reorder tabs within a space (8b0baaa)

- An agent outside any slot wears a glyph, its branch and an agent label (0db64d8)

- An agent in the operator's tree shows what a pull and a push would move (f8946b3)

- A space header flips between its name and its root (49bc63b)

- The overview lists recent prompts as a table grouped by age (611f522)

- The Git extension keeps a commit timeline in the sidebar (71472f5)

- Rebuild the landing page around a recording of the real TUI (06119f1)

- A task whose checkout was removed is put back on its branch (2dadf7b)

- Show the workspace running two harnesses at once, on the site and in the README (2464ca1)

- "+ new" opens a space over a directory another already holds (b2ae681)

- The browser tab carries the tagline, not just the name (a11d05a)


### Fixes

- Consolidate package lifecycle safety (72c2eae)

- Make routed conformance gateway ready and identifiable (72ea7d4)

- Point tooling paths at e2e/ and add OpenCode routed-gateway smoke (ffe6104)

- Prepare detected harnesses during plugin add (e062005)

- Repair broken test build and opencode provisioning gambiarra (675efa0)

- Consolidate shared skill naming and polish the TUI (b3a730a)

- Make harness detection deterministic for CI (9d96e51)

- Compact remove confirmation and protect official marketplace plugins (79b44c3)

- Compute exact native package coverage for Codex and Gemini (f08da26)

- Marketplace add is idempotent; complete the Project Agent Environment (12252e1)

- Reject unsafe manifest paths via shared predicate; remove dead foreign importer; fix CI test isolation (1769519)

- Use the update subcommand, not a nonexistent --upgrade flag; fix e2e Dockerfile build path (c3350a7)

- Republish derived views before attaching default plugins (4e766f7)

- Keep the titlebar health-only and make it open the Doctor page (edb4738)

- Capture vendor CLI output and own the install report (ece9556)

- Declare native package delivery as the primary route (7715137)

- Migrate legacy skill names to stable labels (c7db9d3)

- Reject unsafe CLI tokens in package and MCP names (70ceb32)

- Target the V1 channel for provisioning and delivery (12ac1bc)

- Shared-root superset keeps both vendors' invocation policy (145e626)

- Make plugin install idempotent for owned antigravity native package (4a2eca3)

- Spinner ghost log duplication on plugin/marketplace install (84dfc42)

- Auto-detect the last lab run for lab-watch instead of defaulting to antigravity (74d69d6)

- Strip terminal mouse/focus reporting from lab-watch output (dab1b80)

- Tolerate latest antigravity request order (9e8da4b)

- Preserve qualified skill labels (1a466d7)

- Preserve model-only skill policy (a0d5430)

- Make MCP fixture coverage-robust for opencode native (ed300b3)

- Stop clobbering the scripted tool with a Bash default (fd83c33)

- Ignore Playwright MCP debug output (44ce09a)

- Create default harness shims (3908e0a)

- Stop mangling generated-plugin paths for real agy (a322aba)

- Reuse collected coverage data (dcac21c)

- Adapt AGY confirmation deadlock (8ec794c)

- Inherit cwd for new agents (a915fef)

- Map auto profiles to no approvals (7eb563b)

- Sweep /proc for hook-handler stragglers sharing a killed PGID (642ac8e)

- Harden child process execution (59be367)

- Scroll alternate-screen agents without mouse tracking (660ac8a)

- Preserve scrollback in embedded panes (5f20e3f)

- Kill process groups through kill(2), not /bin/kill (aa01c57)

- Stop the user's own typing from re-arming the agent spinner (1d1fb86)

- Scope the git changes view to the tab's own checkout (34755e8)

- Read an agent tab's state from what its pane actually paints (441bbf9)

- Close the worktree policy vocabulary and say what replaced the key (dd66614)

- Mirror the generated envelope as real bytes, not Store symlinks (fa0ca86)

- Put the generated hooks.json entries at the document root (440a69d)

- A matcher names every tool its alias binds (43f0159)

- The fallback route's default reason names no packager (80426c6)

- Identity does not survive a capture inside a string (558fb1b)

- Emit Antigravity's Stop entry in the flat shape its parser accepts (133e65b)

- Read what the harness sent, and what it rendered (fd297aa)

- Deliver hooks where the harness actually reads them (9b71675)

- Sidebar agent labels, spacing, and space/agent hierarchy (e7176cd)

- An agent carries one name in the sidebar and in the strip (6146f8a)

- The task status tells the truth, and only where it has one (2118582)

- A slot's row belongs to the agent sitting in it now (22ebb37)

- A delivered task still in its slot is read again (7a492dd)

- A task that ended in its slot is still read while it sits there (f7919bc)

- A picked directory opens its project, not a slot inside it (0f65cf1)

- A delivered agent still writing keeps its slot (5ce405a)

- A delivery's outcome lands next to its own trigger (de6ac07)

- An agent outside any slot says so by its branch's hue, not a mark (49fefc2)

- The sidebar keeps what it resolved across a Ctrl+O round trip (284676c)

- A harness that was not provisioned fails setup instead of reading as ready (2fead6c)

- Assert the Antigravity API-key hook path now that 1.1.25 runs it (eadeeff)

- The status catalog opens from the task mark alone (27d9517)

- The root picker's prompt stands where the first space's header stood (10623f4)

- The plugins drawer fetches the detail of a row nobody navigated to (255b10f)

- Side panels meet the frame's right edge, and Profiles keeps the shared inset (48bc9a4)

- The archive guidance parses, so config.yaml is read again (58915dd)

- The harness drawer reports declared context support, not a project's (fadcdd3)

- A slot a pane still sits in is never handed to the next agent (cacd591)

- The capability table answers whether a route works, not whether it is native (de380b5)

- The claude first-run drive reads a screen spaced by cursor moves (c29c127)

- Drop the second #[test] on restoring_rebuilds_which_tab_belongs_with_which (8c1a7a2)


### Other

- Merge pull request #2 from hiukky/refactor/vendor-neutral-core

feat: consolidate package-centric UZE v0 (bdc9535)

- Merge pull request #4 from hiukky/dependabot/github_actions/actions/upload-artifact-7

chore(deps): bump actions/upload-artifact from 4 to 7 (4e4bb8d)

- Merge pull request #3 from hiukky/dependabot/github_actions/actions/checkout-7

chore(deps): bump actions/checkout from 4 to 7 (1412242)

- Merge pull request #5 from hiukky/dependabot/github_actions/actions/download-artifact-8

chore(deps): bump actions/download-artifact from 7 to 8 (0cc2e3f)

- Merge pull request #6 from hiukky/dependabot/cargo/cargo-dependencies-ba95eb9c1d

chore(deps): bump the cargo-dependencies group with 2 updates (3a4405f)

- Honest conformance for Codex, and honest failure for the rest (897fe9f)

- Merge pull request #8 from hiukky/feature/native-first-hooks

feat(hooks)!: compile portable hooks into native artifacts and run Antigravity signed in (57c6319)


### Performance

- Cache attachment inspection verdicts on read paths (14ff137)

- Frame the wire protocol with length-prefixed bincode (4b2033d)

- Canonicalize a PATH entry only when it could be the shims directory (8bcc9e3)


### Refactor

- Separate peer harness integrations from core (6246d48)

- Make the product suite deterministic end to end (95eacdc)

- Move vendor knowledge out of the Store and UzeHome (a393d77)

- Extract harness-agnostic uze core (38c1877)

- Split application and integrations layers (2ee26b9)

- Shorten skill and MCP names for real distribution (210a829)

- Finish native decomposition slice and rename to ui (6ad360f)

- Mod.rs -> self-named files; decompose claude into submodules (68c4f5a)

- Decompose codex/gemini/opencode into submodules (42a03c7)

- Resolve opencode2 through the PATH shim, not a symlink alias (b31416c)

- Surface-based visual redesign without emoji or borders (2cb5ac5)

- Align harness drawer key/value label column (479c6af)

- Rename the harness lab e2e/ -> conformance/ and collapse fixtures (3506ef9)

- Rename uze skill to init — /uze:init (8d89831)

- Single registry composition root, vendor-neutral application/CLI (bfa54d1)

- Restore marketplace manifest name [**breaking**] (f5a5cf7)

- Namespace plugins by marketplace [**breaking**] (16ffe16)

- Simplify architecture documentation (f764284)

- Extract built-in extensions into a uze-extensions crate (38d71fb)

- Remove project context details (1bbcf9e)

- Consolidate profile layout (2d50668)

- Refine workspace and management layout (16ceb4b)

- Polish management navigation and detail layouts (dce3bae)

- Resolve project context once, per agent (4928faf)

- Extract read models and tests out of application.rs (e2ef393)

- Split the workspace orchestrator into render, input and tests (d36fb3f)

- Give speaking to Git one owner (ce32ca0)

- An extension describes, the host draws [**breaking**] (d75fe1d)

- Drop ratatui, now that nothing draws here (e2a602b)

- Group 36 flat modules into the five concerns they already are (4d9352a)

- Scope the first capability off the god handle (aa04680)

- Scope profiles behind their own handle (d86f24b)

- Scope the marketplace behind its own handle (47ad3a7)

- Scope health checks behind their own handle (65a2429)

- Split project environment from instruction context (7b424dc)

- Finish scoping the facade into seven services [**breaking**] (885fc56)

- Consume the application instead of reaching past it (419b3ac)

- Delete the compatibility facade that hid the layer from its guard [**breaking**] (daf8dda)

- The last 55 reaches past the application, and the last operation (2527ac2)

- Give preference translation one procedure (58405c8)

- An extension reaches nothing it was not handed [**breaking**] (eeadf82)

- The workspace client reads Git from a thread, never from the frame (19b2992)


### Style

- Apply cargo fmt (84f781d)

- Run cargo fmt (119dbb0)


### Tests

- Add real harness skill conformance playground (3b37149)

- Consolidate fixtures by test boundary (f78b472)

- Run harness conformance with economical models (f8ca94a)

- Add isolated harness lab spike (c456f32)

- Add minimal local inference compose contract (5c9d43a)

- Add dynamic conformance proof evidence (02f8786)

- Route conformance through isolated LiteLLM gateway (a1afe16)

- Classify blocked Groq conformance smoke (7a99ee2)

- Relocate harness conformance lab to e2e/ and add OpenCode E2E driver (959b1ad)

- Separate conformance evidence by determinism (f7f0dff)

- Isolate PATH in the install-dispatch test (6795406)

- Domain-organized suite, isolated environments, acceptance coverage (1e8850c)

- Rework the harness lab onto the unified L2/L4 evidence model (09d7c28)

- Expect qualified shared skill wrappers (7167188)

- Align package policy coverage (decd1a8)

- Add offline fixture suite for the installer (d1a281a)

- Record AGY adaptation evidence (4c83e4c)

- Keep the deterministic suite off the real host (4641a56)

- Route every scratch directory through uze-testkit (3df0515)

- Enforce the layering the docs already claim (d84c7f3)

- Give Git-driving tests a repository with no ambient config (06e5134)

- Put the seat tests on the shared Git fixture (f98b615)

- Repair the scoping test the cherry-pick damaged (34a30fc)

- Name the second Git spawn instead of claiming there is one (849fbbd)

- Read Codex policy evidence where Codex reads it (21a7427)

- Require the relayed denial before asserting absence (f6399a7)

- Record the green Codex run on codex-cli 0.152.1 (f79f437)

- Assert one Skill contract across harnesses (a63d107)

- Bindings for OpenCode and Antigravity (5d6681a)

- Record that two harnesses cannot enforce user: false (5d66adc)

- Bindings for Claude, and the finding they produced (cacc77a)

- Assert one MCP contract across harnesses (2de5888)

- Delete from the verticals what the contract now proves (573b893)

- Give UZE's own client a vertical (87c7821)

- Colour the verdict labels and drop the bracket padding (aeb2e8e)

- Let Claude's hooks run, and keep a denial once the model saw it (12fde30)

- Make --discovery capture what the harness sent (aefa0d9)

- Measure Antigravity's hook gate; assert Claude's model-only Skill (8150dc7)

- Assert the vocabulary row the handler was handed (e4ffd9b)

- Five verticals green on the compiled hook delivery (54f65de)

- Run Antigravity's vertical signed in, and measure both gates (3d669cd)

- Assert UZE's hooks on Antigravity, and retire the declarations (de0b492)

- The headless check no longer inherits UZE_PANE (175b700)

- Read the agent's row by the agent's own name (5846a18)

- Drive the TUI's checkout-recovery flow in a container (241e989)


