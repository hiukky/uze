## 1. The record

- [x] 1.1 Add the vendor-neutral conversation record in `uze-core::project`: a `SessionId`, a per-task document holding one entry per integration id (conversation or pending, origin, launched-at, observed-at, what preceded it), and its path at `state/conversations/<project id>/<task id>.json` — the same project key the task store uses, and a name that cannot be confused with the runtime session tree.
- [x] 1.2 Write it atomically and read an unreadable or unknown-schema document as "no record", never as an error — continuity fails open by construction, not by every caller remembering to.
- [x] 1.3 Resolve "the task this directory belongs to" from a path alone: the lexical isolated-checkout match plus the task store, with no subprocess.
- [x] 1.4 Remove a task's record where the task itself stops existing (discard), so nothing outlives the work it belonged to. Finishing is deliberately not such a place: an integrated task whose agent kept working is revived by reconciliation and must come back with its conversation.
- [x] 1.5 Cover the record: survives a checkout reset, one entry per harness, a recycled slot finding nothing, and a task removal taking its record with it.

## 2. The port contract

- [x] 2.1 Add the continuity declaration and its verbs to `IntegrationPort` — `session_continuity`, `start_session_args`, `resume_session_args`, `session_recorded_for`, `observe_session` — defaulting to unsupported so a harness declares rather than inherits.
- [x] 2.2 Add the existence check the resume path needs (`session_exists`), defaulting to "assume it does" for a harness that cannot answer cheaply.
- [x] 2.3 Cover the default: an integration that declares nothing contributes no argument and reports no conversation.

## 3. Per-harness continuity

- [x] 3.1 Claude: assignment — a minted UUID at `--session-id`, `--resume` to continue, existence by the session file the harness writes for that directory.
- [x] 3.2 Codex: observation — `resume <id>` to continue, the identifier read back from the newest rollout whose recorded cwd is the task's checkout and whose start is after the launch.
- [x] 3.3 OpenCode: observation — `--session <id>` to continue, the identifier read back from the harness's own session listing, matched on directory and creation time.
- [x] 3.4 Antigravity: observation — `--conversation <id>` to continue, the identifier read back from the directory-keyed conversation index the harness maintains, never from the conversation stores themselves.
- [x] 3.5 Cover each integration's argv and read-back against recorded fixtures of the vendors' real shapes, including a checkout whose records still name the previous task's conversation, which must not match.

## 4. The launch decision

- [x] 4.1 Teach the shim the session step: bare launch only, task resolved from the cwd, resume when the record resolves, start-and-record when it does not, nothing at all outside a managed task.
- [x] 4.2 Record the launch itself for an observed harness — task, harness, started-at, and what that harness's records already pointed at for this checkout — so the read-back can tell a new conversation from the one that was there before, and leave the hot path with no subprocess and no network.
- [x] 4.3 Drop a record whose conversation the harness no longer holds and start fresh, saying why once on the existing note channel.
- [x] 4.4 Keep `UZE_BYPASS` and any caller-supplied argument fully passing through, and keep the shim's own budget test honest about the added reads.
- [x] 4.5 Cover the decision table end to end: fresh, resume, explicit-argument passthrough, outside-a-task, unreadable state, and vanished conversation.

## 5. Launching agents through the shim

- [x] 5.1 Give the agent descriptor the path an agent is launched by — the shim when it exists, the bare name when it does not — so continuity never depends on the operator's PATH.
- [x] 5.2 Use the launcher when the operator has one and never create one on their behalf — its presence is their own opt-in — degrading to the bare name with the reason said on the tab.
- [x] 5.3 Confirm the pane's identity is unchanged by the new launch path: the tab classifies as the same agent it does today.

## 6. Reading it back

- [x] 6.1 Resolve pending launches off the render path, on the background-answer pattern the workspace client already uses, and record what comes back — dropping an answer whose launch the document no longer names, so a replaced launch is never overwritten by the previous one's read-back.
- [x] 6.2 Resolve a launch left pending by a client that was not running, at the next opportunity, before a relaunch needs it.
- [x] 6.5 Keep the record true while the agent runs: refresh it from the conversation the agent is actually in, taking the identifier from a portable hook dispatch where one reaches UZE and from the harness's own records otherwise, so a cleared, forked or switched conversation is what resumes. Where a harness's bridge does not yet carry the identifier its hook payload could (the generated OpenCode plugin passes the directory but no session), pass it through or state that this harness refreshes from its records only.
- [x] 6.3 Carry the gap in the agent descriptor — no launcher, or a harness with no mechanism — and say it on the tab when such an agent starts. No read model for the ordinary case: a conversation that carried over is the absence of anything to say.
- [x] 6.4 Cover the read-back: the newest matching session wins, an older one in the same directory never matches, and an answer arriving for a task the viewer left is dropped.

## 7. Proving it

- [x] 7.1 State the continuity outcome in `conformance/contract/` in vendor-neutral terms: a turn made, the process ended, an agent launched into the same task, the turn present.
- [x] 7.2 Bind it per harness, declaring unsupported with a reason where the vendor offers nothing.
- [x] 7.3 Add the recovery journey scene: the terminal server dies, the workspace comes back, and the agent answers from the conversation it was in — checked against the machine, never against UZE's own report.
- [x] 7.4 Record the guarded property and the test that holds it in `docs/architecture/invariants.md`.
- [x] 7.5 Derive a continuity column in the generated harness matrix from the declaration itself, so the docs site and the landing table cannot claim a mechanism the code does not implement, and the staleness check fails the push when they drift.
