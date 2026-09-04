# Authorship is the human who adopts a change, not the tool that drafted it

Status: Accepted

## Context

Most changes in this repository are drafted with coding agents; `AGENTS.md`
exists because that is the normal way of working here, not the exception.
Every such agent offers to sign its work — `Co-Authored-By` trailers,
session links, "generated with" footers — and `CONTRIBUTING.md` already
says those are stripped before a commit is pushed.

That rule was recorded without its reason, which invites two wrong
readings. The first is that the project hides its use of agents, which
would be contradicted by `AGENTS.md`, the `.agents/` directory and the
conformance Lab in the same tree. The second is that a trailer is a
formality, in which case there is no reason to strip it and the rule
erodes the first time someone is in a hurry.

The repository is also the input to two things that read commit metadata
as fact: `git-cliff` generates `CHANGELOG.md` from commit messages, and
the git history is the only record of who stands behind a line of code
when a question about it arrives later.

## Decision

**The author of a change is the human who takes responsibility for it.**

- A commit's author is the person who reviewed the change and chose to
  publish it. That person answers for the change afterwards.
- Agent-generated material is a draft. It enters the tree only once a
  human has read it, understood it and adopted it, at which point it is
  that person's work in the same sense as anything they typed.
- AI attribution trailers — `Co-Authored-By` for an agent, session links,
  generated-with footers — are removed before push. They are removed
  because they name something that cannot hold responsibility, not to
  conceal how the change was produced. How the work is produced is stated
  openly in `AGENTS.md` and `CONTRIBUTING.md`.
- Nothing here restricts the use of agents, and nothing requires
  disclosing which parts of a change one drafted.

This records the reason for a rule already stated under **Commits** in
`CONTRIBUTING.md`; that file remains the operative instruction.

## Consequences

Every line in `git log` has a person behind it who can be asked about it,
which is what makes review, `git blame` and a security response work at
all. Someone who cannot explain a change should not be adopting it, and
this makes that explicit rather than implied.

`CHANGELOG.md` stays a record of changes rather than of tooling, since
`git-cliff` sees only messages a human wrote.

The cost is manual: agents keep proposing trailers, and each has to be
removed. `lefthook` and review are where that is caught.

This record deliberately states no position on the copyright status of
generated output. It defines who is accountable for a change in this
repository, which is a question the project can answer for itself.
