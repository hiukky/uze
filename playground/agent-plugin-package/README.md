# Agent Plugin Store Fixture

This is the clean external Agent Plugins 1.0 package used by the UZE store
and integration conformance tests. It contains only `plugin.json` and
`skills/`; it deliberately has no `.agents`, `.claude`, `.codex`, or other
harness configuration.

The UZE E2E installs this package once into a temporary `$UZE_HOME`, composes
its `SKILL.md` through `UzeEngine`, and lets the selected integration choose an
explicit exposure mechanism.
