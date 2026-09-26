# take-back-a-whole-marketplace Specification

## MODIFIED Requirements

### Requirement: Marketplace list and remove manage registry
The system SHALL list registered marketplaces, and `market remove <name>` SHALL be the marketplace's teardown on this machine: every package the Store holds from that marketplace is removed first, each through the same machine-removal path a per-package removal runs (inspect-before-detach, receipt-owned teardown, drift safety); the registry entry is removed last, only when none remains.

#### Scenario: List marketplaces
- **WHEN** user runs `uze marketplace list`
- **THEN** system shows `name`, `source` (path/URL), and plugin count

#### Scenario: Removing a marketplace takes its packages with it
- **WHEN** two packages installed from a marketplace and `uze market remove <name>` runs
- **THEN** both are detached (receipt-owned teardown each), their Store bytes are deleted, and the registry entry is removed

#### Scenario: A blocked package keeps the marketplace registered
- **WHEN** `uze market remove <name>` runs and one of its packages' teardown is blocked by drift
- **THEN** the marketplace stays registered, the answer names the blocked package and the ones that came off, and the exit status says the teardown did not complete

#### Scenario: The marketplace that installs nothing removes in one step
- **WHEN** `uze market remove <name>` runs with no package from it installed
- **THEN** only the registry entry (and any link it carries) is removed

#### Scenario: Remove marketplace
- **WHEN** user runs `uze market remove <name>` and no plugin from it is installed
- **THEN** registry entry is removed

## ADDED Requirements

### Requirement: Detach follows the receipt ledger, not detection
The per-package removal SHALL undo every harness artifact the receipts ledger owns for the package, whether or not the harness's vendor binary is detected in the context the command runs in: the ledger is what says UZE delivered something, so an undetected-but-delivered harness is a receipt to tear down through its integration, never a harness to skip.

#### Scenario: An undetected harness's delivered bytes come back
- **WHEN** a package was delivered to a harness whose vendor binary is not on the PATH the command runs with, and the package is removed
- **THEN** that harness's delivered bytes are torn down through its integration, from the receipts
