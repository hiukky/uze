# Provision supported harnesses through official, integration-owned routes

Status: Accepted

## Context

The original meaning of `uze setup` was intentionally narrow: detect an
already installed CLI harness, create UZE-owned prerequisites, and record
integration setup. The new package-first experience makes `uze add` attach
automatically to harnesses that already exist, but a fresh machine still
requires users to discover and run each vendor's installer before UZE can
deliver anything.

UZE needs an explicit bootstrap command that can make a supported local agent
environment usable from an empty machine. This is a new responsibility with
real safety and ownership consequences. Treating installer commands as Core
knowledge would violate peer integrations; treating installation provenance as
attachment ownership could eventually delete a manually installed executable.

## Decision

`uze setup [harness]` provisions the selected registered CLI harness through
its current, documented official vendor route, targets the latest stable
channel by default, verifies the resulting executable, and only then performs
the existing UZE preparation and package delivery steps.

Provisioning remains **integration-owned**. The Application coordinates the
sequence and exposes structured outcomes. The Core, Store, CapabilityRouter,
PackageExposurePlan, package provenance, and attachment ledger do not know a
vendor installer URL, shell command, platform rule, or version schema.

`uze add` does not provision, update, or remove executables. It may prepare
already detected integrations and attach a package immediately, but remains a
predictable plugin operation with no implicit network download.

UZE persists a secret-free provisioning record separately from attachment
receipts. It records UZE's attempt/method and verified facts, not ownership of
the current executable. Harness removal is explicitly deferred: it requires a
separate decision and must never remove a harness lacking positive,
UZE-originated ownership evidence.

When a platform lacks a documented official automatic route, UZE reports a
structured, actionable block. It does not substitute an unofficial installer
or guess how an existing tool was installed.

## Consequences

Fresh-machine DX becomes coherent:

```text
uze setup opencode
uze add <plugin>
opencode
```

The same model also supports installing packages before a harness exists:
after later `uze setup`, normal package-first planning exposes the stored
packages without a separate sync command.

This adds limited operational responsibility for the explicitly registered
CLI harnesses, but does not make UZE a general SDK/version manager, package
manager, launcher, or runtime proxy. Each integration needs maintained
official-route evidence and isolated command-contract tests. Latest-stable
updates are intentional explicit side effects of `setup`; version pinning and
harness uninstall remain future work.
