# Native discovery baseline fixture

Deliberately a plain project directory containing `.agents/skills/uze-e2e/`.
It never touches the UZE store, engine, integrations, or runtime bridge.

It is the **control** for the behavior tier — but note carefully what it does
and does not control for.

## What it measures

Whether a harness discovers a skill placed in the **project's own**
`.agents/skills/`, with UZE absent.

## What it does not measure

UZE's actual delivery surface. UZE installs a package once into the Store and
attaches it at **user scope**, leaving the caller's project untouched. The
behavior tier's workspace contains no skill file at all, so a behavior pass
can only have come from that user-scope route.

A baseline pass therefore does **not** weaken a behavior pass. The two probe
different paths to the same capability.

## How to read it

| Baseline | Behavior | Reading |
|---|---|---|
| pass | pass | The harness supports both routes. UZE's adds reach without per-project files. |
| **fail** | **pass** | UZE's user-scope route is the harness's *only* path to that capability. Strongest evidence for UZE. |
| fail | fail | Ambiguous. The harness may not support this capability shape at all — investigate before blaming UZE's delivery. |
| pass | fail | UZE's delivery is broken: the harness can reach the capability, just not the way UZE attached it. |

Measured on 2026-08-21: Claude Code fails the baseline ("Unknown skill:
uze-e2e"); Codex and OpenCode pass it. That last row is the one this fixture
exists to make detectable at all.
