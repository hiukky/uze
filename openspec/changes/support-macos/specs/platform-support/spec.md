## ADDED Requirements

### Requirement: A platform is supported only where it is proven

UZE SHALL treat a platform as supported only when its automated checks build,
run, lint, test and perform the product journeys on that platform's own
hardware. Compiling for a target SHALL NOT by itself make it supported, and
the installer and documentation SHALL NOT offer a platform that no run has
exercised.

#### Scenario: A target that only cross-compiles is not offered

- **WHEN** the workspace type-checks for a target but no run has executed its
  suite on that platform
- **THEN** the installer refuses that platform by name and the documentation
  says it is unsupported, rather than offering a download that cannot be
  produced

#### Scenario: A supported platform runs the same journeys

- **WHEN** the product journeys run on a supported platform
- **THEN** they perform the same suite through the same definition as every
  other supported platform, so a differing result is a difference between the
  platforms rather than between two ways of running them

### Requirement: Kernel-owned facts are asked once per platform

UZE SHALL obtain facts that only the operating system holds about a process
it did not spawn — the peer of a socket connection, the executable image a
pid runs, its working directory, and its inherited environment — through a
single per-platform boundary, and SHALL express the decisions built on those
facts once for all platforms.

An absent answer SHALL mean *unknown* and SHALL NOT be treated as a negative
answer.

#### Scenario: An unanswerable probe does not destroy live state

- **WHEN** the platform cannot report which process holds a runtime endpoint
- **THEN** the endpoint keeps the state it already had, and no running server
  is torn down on the strength of the missing answer

#### Scenario: An unanswerable probe reports no status rather than a guess

- **WHEN** the platform cannot report the working directory or command of the
  process in a pane
- **THEN** the pane reports no foreground status, and never a fabricated one

#### Scenario: A new platform is added at the boundary

- **WHEN** support for a further platform is added
- **THEN** the per-platform boundary gains an implementation and the decisions
  built on it are unchanged

### Requirement: The installer resolves an asset from the host and fails closed

The installer SHALL derive the release asset from the host's own operating
system and architecture, SHALL verify the asset's SHA-256 against the
published checksums before installing it, and SHALL refuse — before
downloading anything — a host for which no asset is published.

The installer SHALL depend only on tools the supported platforms actually
carry, resolving between equivalent spellings where they differ.

#### Scenario: An architecture the host names differently still resolves

- **WHEN** the host reports an architecture under a name other than the one
  the asset uses
- **THEN** the installer maps it and downloads the asset for that
  architecture

#### Scenario: An unsupported operating system is refused before download

- **WHEN** the installer runs on an operating system with no published asset
- **THEN** it exits non-zero naming that operating system, and downloads
  nothing

#### Scenario: A checksum tool present under either name is used

- **WHEN** the host provides only one of the two conventional SHA-256 tools
- **THEN** the installer uses whichever is present, and refuses up front when
  neither is
