# Security Policy

## Supported versions

UZE is pre-1.0 alpha software. The public contract can change between
releases, and there are no maintenance branches: **only the latest release
receives fixes.** If you are running anything older, the first step of any
security response is to upgrade.

| Version | Supported |
| --- | --- |
| Latest `0.y.z-alpha.N` release | ✅ |
| Any earlier release | ❌ |
| `main` between releases | Best effort |

## Reporting a vulnerability

**Do not open a public issue, pull request or discussion for a
vulnerability.** A public report is a disclosure, and it is the one step
that cannot be undone.

Report through GitHub's private vulnerability reporting:
**[Security → Report a vulnerability](https://github.com/hiukky/uze/security/advisories/new)**.

It opens a draft advisory only you and the maintainer can read, keeps the
discussion attached to the eventual fix, and issues the CVE from the same
place if one is warranted. It needs a GitHub account and nothing else — no
address to find, none to go stale, and nothing published until there is a
fix to publish alongside it.

A report that can be acted on contains:

- the version (`uze --version`) and the operating system,
- what an attacker gains — read a file they should not, run code, escalate
  from plugin bytes to the host,
- **a reproduction**: the smallest sequence of commands, plugin manifest or
  repository state that shows the behaviour.

A report without a reproduction is not ignored, but it is triaged after
every report that has one.

## What happens next

1. **Acknowledgement.** You get a reply confirming the report was received
   and whether it is reproducible.
2. **Assessment.** The affected versions and the impact are established, and
   you are told which of the two it is: a vulnerability, or behaviour that is
   working as intended and documented.
3. **Fix.** Developed privately, with a regression test, and released.
4. **Disclosure.** Coordinated with you, after the fix is available. You are
   credited unless you ask not to be.

Nothing is disclosed publicly before a fix is out, and you are asked to hold
to the same until then.

## Scope

UZE writes into harness configuration and delivers third-party plugin bytes
to harnesses that execute them. Reports at those boundaries are in scope and
particularly welcome:

- a plugin escaping the delivery model — reaching state UZE never handed it,
  or being executed by a harness that was not the delivery target,
- a receipt or lock file that authorises a destructive mutation it should
  have blocked,
- the runtime PATH shim being made to invoke something other than the
  harness it stands in for,
- an acquisition path that trusts a remote it should not.

Out of scope: vulnerabilities in the harnesses themselves (report those to
their vendors), and in dependencies where UZE only passes data through —
though a report explaining how UZE's use makes an upstream issue exploitable
is in scope.
