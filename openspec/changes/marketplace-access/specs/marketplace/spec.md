## MODIFIED Requirements

### Requirement: Marketplace registry stores generic Git/local sources
The system SHALL store marketplace entries as `{name, source: Git|Local}` in `~/.uze/state/marketplaces.json`, where Git is a generic URL (not GitHub-specific) and Local is a filesystem path. The registry SHALL NOT copy plugin bytes. A Git source SHALL be recorded as its canonical identity (see the `marketplace-access` capability), not as the spelling the operator typed: a short locator (`owner/repo`, `<alias>:owner/repo`), an `scp`-style locator (`git@host:owner/repo`) and an `ssh://git@host/…` URL with no port all record the same `https://` URL, and a source already registered in another spelling of the same repository SHALL be matched, not reported as a conflict. A local path SHALL be recognised only when it is spelled as one: starting with `/`, `./`, `../` or `~`, or a bare `.` or `..`. A single segment with no prefix (`ai`) SHALL be refused, suggesting `./ai` when that directory exists. A two-or-more-segment argument with no prefix is a short locator, and SHALL be refused as ambiguous when it is also an existing directory.

#### Scenario: Add local marketplace
- **WHEN** user runs `uze market add /home/hiukky/ai`
- **THEN** system records `ai → Local{path:/home/hiukky/ai}` and `marketplace.json` is readable

#### Scenario: Add Git marketplace
- **WHEN** user runs `uze market add https://github.com/hiukky/ai`
- **THEN** system records `ai → Git{url:https://github.com/hiukky/ai}` without cloning plugins

#### Scenario: Add the current directory
- **WHEN** user runs `uze market add .` inside a marketplace checkout
- **THEN** system records the checkout as a Local source, exactly as its absolute path would

#### Scenario: A bare word is not a path
- **WHEN** user runs `uze market add ai` and `./ai` is a marketplace checkout
- **THEN** system records nothing and fails suggesting `uze market add ./ai`

#### Scenario: Add Git marketplace by short locator
- **WHEN** user runs `uze market add hiukky/ai` and the machine's default host is `github`
- **THEN** system records `ai → Git{url:https://github.com/hiukky/ai}`, the same entry the full URL records

#### Scenario: Add Git marketplace by scp-style locator
- **WHEN** user runs `uze market add git@github.com:hiukky/ai.git`
- **THEN** system records `ai → Git{url:https://github.com/hiukky/ai}` and does not read the argument as a local path

#### Scenario: Short locator that is also a directory
- **WHEN** user runs `uze market add hiukky/ai` and `./hiukky/ai` exists in the working directory
- **THEN** system records nothing and fails naming both readings, `./hiukky/ai` for the directory and `github:hiukky/ai` for the repository
