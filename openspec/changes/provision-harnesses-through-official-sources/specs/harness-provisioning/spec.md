## ADDED Requirements

### Requirement: Explicit setup provisions a selected supported harness

When a user invokes `uze setup <harness>`, UZE SHALL use only that
integration's documented official installation or update route to ensure its
CLI executable is available at the latest stable channel, then verify the
executable before preparing UZE-owned integration state.

After a verified setup, UZE SHALL create or refresh the generic runtime shim
for that harness. The shim SHALL be created only by explicit `uze setup`, and
SHALL be a transparent launch boundary unless the owning integration declares
additional runtime contribution.

#### Scenario: Harness executable is absent

- **WHEN** `uze setup opencode` is invoked and OpenCode is not detected
- **THEN** UZE runs OpenCode's official platform-appropriate install route
- **AND** verifies that `opencode --version` succeeds before marking setup
  ready
- **AND** reports an actionable provision failure without claiming the
  integration was prepared.

#### Scenario: Harness executable already exists

- **WHEN** `uze setup codex` is invoked and Codex is detected
- **THEN** UZE uses Codex's official update route for the supported platform
- **AND** records the verified version after the update attempt
- **AND** prepares UZE-owned integration prerequisites only after verification.

### Requirement: Setup uses vendor-owned provisioning semantics

UZE SHALL keep vendor installer URLs, commands, platform restrictions, and
version parsing inside the owning integration. UZE Core, Store,
CapabilityRouter, PackageExposurePlan, and attachment ledger SHALL not
interpret them.

#### Scenario: A platform has no supported official automatic route

- **WHEN** a selected integration has no documented official automatic route
  for the current platform
- **THEN** setup returns a structured unsupported/blocking result
- **AND** it identifies the official manual route when known
- **AND** it SHALL NOT run an unofficial installer or package-manager command.

### Requirement: Plugin add never provisions harness executables implicitly

`uze add <source>` SHALL prepare and attach to already detected harnesses but
SHALL NOT install, update, or remove a harness executable.

#### Scenario: A supported harness is absent during plugin add

- **WHEN** a plugin is installed and a harness executable is absent
- **THEN** UZE installs the package once in its Store
- **AND** reports that delivery to that harness is pending
- **AND** does not invoke networked provision commands.

### Requirement: Setup exposes already installed packages

After a selected harness is verified and UZE preparation succeeds, setup
SHALL plan and deliver every stored package to that harness through the
existing package-first exposure rules. Native package delivery SHALL continue
to suppress duplicate capability attachments.

#### Scenario: Package was added before the harness existed

- **WHEN** a package is already in the Store and its target harness becomes
  available through `uze setup <harness>`
- **THEN** setup exposes the package using that integration's delivery plan
- **AND** records normal attachment receipts for persistent artifacts.

### Requirement: Provisioning provenance is separate from attachment ownership

UZE SHALL persist secret-free facts about a provisioning attempt separately
from attachment receipts. A provisioning record is evidence that UZE invoked
a route; it SHALL NOT be treated as evidence that a live executable or a
harness-managed artifact is safe to remove.

#### Scenario: Future harness removal considers a manually installed executable

- **WHEN** no UZE provisioning record proves UZE initiated an installation
- **THEN** a future removal operation MUST preserve the executable.

### Requirement: Setup conformance covers every registered harness

The deterministic conformance suite SHALL exercise `uze setup <harness>` for
every integration registered by the product. Each scenario SHALL prove that
setup dispatches the integration's documented official route, resolves a real
vendor executable rather than UZE's runtime shim, and records a verified
result and default runtime shim without invoking a network installer. Adding
a registered harness without its setup scenario SHALL fail the suite.

#### Scenario: A legacy executable has incompatible update syntax

- **WHEN** only OpenCode's legacy `opencode2` executable is detected
- **THEN** `uze setup opencode` uses OpenCode's documented installer route
- **AND** it SHALL NOT invoke `opencode2 upgrade`
- **AND** setup verifies the legacy executable before it prepares UZE state.
