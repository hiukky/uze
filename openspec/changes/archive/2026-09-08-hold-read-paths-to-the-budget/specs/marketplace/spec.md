## ADDED Requirements

### Requirement: A marketplace's catalogue is served from a cache
What a marketplace registered from a Git source offers SHALL be read from
a cached checkout under `UZE_HOME`'s cache directory rather than by
cloning the source: one checkout per marketplace, recording the source it
was read from and when. A catalogue SHALL stand for a bounded time, after
which the next read clones the source again; registering the same source
again SHALL refresh it immediately; removing the marketplace SHALL drop
it. A marketplace registered from a local path SHALL be read in place on
every read. The cache is reconstructable and never authoritative: deleting
it costs one clone, never correctness.

#### Scenario: A listing does not need the repository
- **WHEN** a marketplace was registered from a Git URL and its repository
  is later unreachable or gone
- **THEN** `market list`, `market inspect`, the plugin listing and
  inspecting one of its plugins still answer from the cached catalogue,
  with no clone attempted while the entry stands

#### Scenario: A catalogue is refreshed once per window, not once per screen
- **WHEN** a cached catalogue is older than its maximum age
- **THEN** the next read clones the source once and every read after it
  is served from the new checkout until it ages out in turn

#### Scenario: Registering the same source again refreshes the catalogue
- **WHEN** `market add` is run with a source already registered under the
  same name
- **THEN** the registry is unchanged and the cached catalogue is replaced
  by the clone that registration made

#### Scenario: A refill that fails answers with the last catalogue seen
- **WHEN** a cached catalogue has aged out and the source cannot be cloned
- **THEN** the read answers with the expired catalogue rather than nothing,
  and the refill is attempted again on the next read

#### Scenario: A local marketplace is never cached
- **WHEN** a marketplace was registered from a local path
- **THEN** every read parses its `marketplace.json` where it is, and no
  cache entry is written for it

#### Scenario: Removing a marketplace drops its catalogue
- **WHEN** `market remove` succeeds
- **THEN** the marketplace's cache entry is removed with its registration
