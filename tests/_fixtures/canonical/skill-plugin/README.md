# Agent Plugin Package Fixture

This is the clean external Agent Plugins 1.0 package used by the UZE store
and integration conformance tests. It contains only `plugin.json` and
`skills/`; it deliberately has no `.agents`, `.claude`, `.codex`, or other
harness configuration.

The UZE E2E installs this package once into a temporary `$UZE_HOME`, reads
its `SKILL.md` as a package resource (`engine::package_resources`), and passes the
same stored resource identity to peer integrations. The proof token is
behavioral evidence that the skill was exposed and followed; it is not a
cryptographic proof. Native discovery fixtures are deliberately separate.
