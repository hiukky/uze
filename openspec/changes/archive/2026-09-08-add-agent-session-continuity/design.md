## Context

See `proposal.md` — Why. Three facts about the machine as it stands decide
this design.

An agent's launch argv is `[binary]` and nothing else: the picker builds it
from the harness descriptor, and the placement path carries it unchanged to
`CreateTab`. The terminal runtime persists that argv per pane and replays it
verbatim when a server comes back — `PersistedTab::command`, spawned before
any client attaches. So whatever decides "resume or start" must decide it
*inside the launched process*, at the moment it starts: there is no client
in the room on the path that hurts most, and the protocol carries argv but
no environment.

UZE already owns exactly that moment. `src/shim.rs` is a launch boundary on
every harness (`~/.uze/shims/<name>` → `uze`), it already asks the
integration what to add to a launch (`runtime_contribution` → `extra_args`,
`extra_env`), and it already `exec`s the real binary. It is thin on purpose:
no `UzeApplication`, no Store scan, no network.

Under the isolation policy every agent UZE launches runs in a slot, and
`worktree::isolated_checkout` answers "which slot is this, under which
primary" lexically, with no subprocess. The task store is one JSON document
per project, outside every checkout, and it already names each task's
checkout. So a process starting in a slot can find its task cheaply.

Vendor mechanics, verified against the installed binaries on 2026-09-06:

| Harness | Naming | Start | Resume | Read back |
|---|---|---|---|---|
| Claude Code | UZE names it | `--session-id <uuid>` | `--resume <uuid>` | not needed; `~/.claude/projects/<slug>/<uuid>.jsonl` exists as an existence check |
| Codex | harness names it | — | `resume <id>` | `session_meta` (`session_id`, `cwd`) on the first line of `~/.codex/sessions/**/rollout-*.jsonl` |
| OpenCode | harness names it | — | `--session <id>` | `location.directory` + `time.created` from the harness's own API (`opencode api GET /api/session`) |
| Antigravity | harness names it | — | `--conversation <id>` | `cache/last_conversations.json` under the CLI's home: a directory → conversation map the harness maintains itself |

Two of those were confirmed by running the real binaries. `claude
--session-id <uuid>` starts a conversation and refuses a second use of the
same id ("Session ID … is already in use"), while `claude --resume <uuid>`
answers from the earlier turn — the single fact that rules out a static
argv: the command that starts a conversation is not the command that
continues it, so a persisted argv is wrong on its second run whatever it
holds. Antigravity answered from its earlier turn through `--conversation
<id>`, with the id taken from the directory-keyed index the harness writes
itself — the cheapest read-back of the three that need one, and a plain
JSON file rather than a private store.

Antigravity's conversations themselves are one SQLite database each, keyed
by conversation id, holding protobuf blobs. Nothing in this design reads
them: the index is the only file UZE looks at, which is why observation for
this harness needs neither a SQLite nor a protobuf dependency.

## Goals / Non-Goals

**Goals:**
- One decision point for continuity, reached by every relaunch including the
  one with no client present.
- Vendor knowledge only in `uze-integrations`; core, application and
  presentation say "this task's conversation".
- Fail-open at every step: continuity never delays or prevents a launch.
- No new dependency, no terminal protocol change, no new state schema
  version for the task store.

**Non-Goals:**
- Continuity for an agent that could not be isolated (no task, nothing to
  key on) and for a harness a person started by hand outside a slot.
- Reading or parsing conversation content. UZE records an identifier and
  asks the vendor to do the rest.
- A UI for browsing or picking conversations. The task is the handle.

## Decisions

### The shim decides, and the client never composes a resume argv

The alternative was to have the TUI build `claude --resume <id>` when it
creates the tab. It fails exactly where the operator is hurt: the terminal
server's restore path replays the persisted argv with no client running, and
an assigned `--session-id` replayed a second time is an error, not a resume.
Making the client rewrite persisted commands would put session knowledge
into `uze-terminal`, whose whole point is that it carries no domain.

So the shim asks, per launch: *am I in a slot; whose task is it; does that
task have a conversation for me?* — and prepends the integration's own start
or resume arguments. A pane respawned by the server goes through the same
boundary and gets the same answer, which is why the restart case needs no
code in the client at all.

Cost: one lexical path match, one small JSON read, and — only on a first
launch — one small write. No subprocess, no network, nothing that scales
with the Store. That keeps the shim's stated budget.

### Agents are launched by the shim's path, not by name

`agent_options` builds `[binary]` and relies on PATH resolving it. Continuity
would then depend on the operator having put `~/.uze/shims` ahead of the
real binary — which
`transparent-harness-attachment` deliberately does not require. Launching an
agent UZE created through the absolute shim path (`shims_dir/<name>`) keeps
that requirement false: nothing on the operator's PATH changes, the shim
still `exec`s the real binary, and `UZE_SHIM_NAME` keeps the pane's identity
as it is today. Where the shim is absent (never set up, removed by hand),
the launch falls back to the bare name and continuity is simply not
available — stated, not silently missing.

### Continuity is a small port contract, not a second capability

`IntegrationPort` gains one declaration and three verbs:

- `session_continuity()` → `Assigned` | `Observed` | `Unsupported`
- `start_session_args(&SessionId)` — for `Assigned` only
- `resume_session_args(&SessionId)`
- `observe_session(cwd, since)` → `Option<SessionId>` — for `Observed` only

They are one capability's worth of surface and belong on the trait the
conformance suite already proves across four harnesses, not in a fifth
per-capability trait — the same reasoning `AGENTS.md` records for
`IntegrationPort` as a whole. Core holds `SessionId` and the record;
`Assigned` mints one (a UUID, since the only harness that accepts a name
demands that shape), `Observed` leaves it empty until read back.

`Assigned` is strictly better where it exists: the identifier is known
before the process starts, so a crash between launch and first write costs
nothing, and no vendor-private file is ever parsed. `Observed` is the
fallback the other two force.

### Observation runs where it can afford to, and leaves a breadcrumb

`observe_session` for OpenCode means running the harness's own CLI; for
Codex it means scanning a directory of rollout files. Neither belongs in the
shim's hot path. The shim therefore records the *launch* (task, harness,
started-at) and returns; the identifier is read back afterwards, off the
render path, by the same background-answer pattern the workspace client
already uses for Git reads — one `spawn_*`/`absorb_*` pair. A launch made
while no client is watching leaves its breadcrumb behind, and the next
client run resolves it before the next relaunch needs it.

What makes the read-back safe is that the launch records what the harness's
own records already said, and only a change from that counts. Where the
vendor timestamps its sessions, that is "created in this checkout after the
launch, newest wins"; where it keeps a single entry per directory
(Antigravity), it is "different from the entry that was there when we
started". Either way a conversation belonging to the task that held the slot
before this one can never be adopted by the one holding it now.

### The record is the conversation last observed, not the one assigned

An identifier written once at launch is a claim that goes stale. A harness
is free to move a running agent into another conversation. Clearing one in
Claude Code was verified to do exactly that: a session started under an
assigned identifier held only the turns before the clear, and the turns
after it went to a second, differently identified conversation in the same
directory — `--fork-session` ("create a new session ID instead of reusing
the original") is the same move, asked for deliberately. What a person
renames or retitles, by contrast, is a label and never the identifier a
resume takes: the transcript is named by the identifier, every line repeats
it, and two resumes into it appended to the same one.

So assignment decides only how the *first* conversation is named. What the
record holds is the last conversation observed for that task's checkout,
refreshed while the agent runs, which makes `Observed` the mechanism all
four harnesses use for keeping the record true and `Assigned` an optimization
for one of them at the start. A relaunch then restores where the work was
left rather than where it began.

Where UZE's own portable hooks are installed in the project, they are the
authoritative and cheapest channel for this: the normalized hook input
already carries `session_id` and `cwd` on every dispatch, so an agent that
uses a tool has told UZE which conversation it is in, from the vendor's own
supported surface, with nothing parsed. Reading the vendor's records is the
fallback for a project with no hooks.

### The record is one file per task, not a field on the task

Adding `sessions` to `Task` would put the whole per-project task document
under multi-process read-modify-write: every shim launch would rewrite a
document the TUI also writes, and two agents starting at once could drop
each other's work. A per-task file is written only by launches of that one
task, keeps the task store's schema and its single writer untouched, and is
read into the application's read model where presentation needs it. It is
UZE state, atomically written, removed with its task.

It is named `conversations`, not `sessions`: `runtime/sessions/` already
means an invocation's ephemeral tree, whose lifetime is the opposite of
this one's, and two tenants sharing a word is how a sweep eventually
deletes the wrong tree.

Binding to the task — not the directory — is also what makes slot recycling
safe: the lookup is "the live task whose checkout is this directory", so a
new task in a recycled slot finds no record and starts fresh, while the
previous task's record stays intact for a resume that gives it a slot back.

### The matrix is generated, and a new harness supplies five answers

Continuity must not become a hand-written claim about four vendors. The
harness × feature matrix is already derived from the real `IntegrationPort`
implementations and rendered into the docs site and the landing page's
table, with `--check` failing the push when either is stale, so continuity
joins it as a derived column: what a harness supports is read off the
integration that implements it, and a vendor whose mechanism changes cannot
leave a green cell behind. The Lab states the same thing as an outcome
against the real binary, and a harness that cannot deliver it declares it
through the bindings' own unsupported channel, which the run records.

That leaves exactly five answers a new vertical supplies — the declaration,
the start argv (only if it lets UZE name a conversation), the resume argv,
the read-back, and the existence check — plus one Lab binding. Nothing in
core, the application, the client, the shim or the record shape changes for
a fifth harness, which is the same bar `AGENTS.md` already sets for a new
harness against Store, Engine and Router. The three verticals named as
planned there (Cursor CLI, Muse, PI) inherit an unsupported declaration
until someone answers those five.

### What is on disk

One document per task, beside the task store it belongs to and keyed the
same way — `<UZE_HOME>/state/conversations/<project id>/<task id>.json`,
with the project id being the same digest `state/tasks/<project id>.json`
already uses:

```json
{
  "schema_version": 1,
  "task": "o10ps6",
  "harnesses": {
    "claude-code": {
      "conversation": "3b11992a-96ed-4ac2-98bc-40abc96e5bd2",
      "origin": "assigned",
      "launched_at_unix": 1788722281,
      "observed_at_unix": 1788722305,
      "preceded_by": null
    },
    "codex": {
      "conversation": null,
      "origin": "observed",
      "launched_at_unix": 1788730112,
      "observed_at_unix": null,
      "preceded_by": "01a05afb-ef77-7953-896a-aad47a268d60"
    }
  }
}
```

Keyed by integration id, so the entries are the vocabulary the registry
already publishes and a fifth harness adds a key rather than a shape.
`conversation` is null exactly while a launch is waiting to be read back —
the pending state is the absence of an answer, not a separate flag.
`origin` says whether UZE named it or the harness did, which is what the
next launch needs to decide between minting and observing.
`launched_at_unix` is the floor a read-back accepts a conversation above,
and `preceded_by` is the same guard for a harness that keeps one entry per
directory instead of timestamps: what its records pointed at for this
checkout when this task's agent started, so the previous occupant's
conversation can never be adopted. `observed_at_unix` is when the record
was last confirmed, which is what keeps a refresh cheap.

Nothing else goes in it. The checkout, the branch and the base stay in the
task store, which already owns them; duplicating a path here would create a
second answer to where a task lives.

Three writers touch it, and they cannot fight: the shim writes the entry at
launch, the client fills in what it observed, and a hook dispatch refreshes
it. The two asynchronous ones write through the launch they answer for — an
observation whose `launched_at_unix` no longer matches the document is an
answer to a launch that has been replaced, and is dropped rather than
written, which is the same rule the client already applies to a Git answer
that arrives after the viewer moved on.

Unlike the task store, an unknown `schema_version` here is read as "no
record", never as an error. The task store refuses one because guessing at
a task's state could destroy work; this document only decides whether an
agent resumes, and refusing to launch over it would trade a lost
conversation for a lost agent.

### Only a bare launch is touched

The shim prepends nothing when the caller passed any argument at all. It is
the cheapest rule that keeps every promise: the vendor's own session flags
win because UZE never competes with them, a prompt passed on the command
line still starts what the operator asked for, and `codex resume <id>` —
which is a subcommand, not a flag — is only ever prepended where there is
nothing in front of it to break. `UZE_BYPASS` keeps working as the escape
hatch it already is.

### An unusable record is a stated fallback, not an error

`exec` cannot retry: once the shim replaces the process image, a harness
refusing `--resume <gone>` is a dead pane. So a recorded conversation is
checked for existence before it is resumed, by the integration that knows
where its harness keeps them (a file check for Claude; the same records
`observe_session` already reads for the others). A record that does not
resolve is dropped and the launch starts fresh, with one line on stderr —
the shim's existing `note` channel.

## Candidate ADRs

- **The launch boundary owns session continuity** — putting the resume
  decision in the PATH shim makes the shim load-bearing for a
  user-visible guarantee, which is a boundary that is expensive to move
  later.

## Risks / Trade-offs

- **A vendor changes its resume verb or its session store.** → The
  declaration and the argv live in one integration module each, and the Lab
  contract runs against the real binary, so a break surfaces as a red
  vertical naming the harness rather than as a silent fresh conversation.
- **`Observed` read-back is vendor-private for Codex** (a rollout file's
  first line). → It is read, never written, and only as a fallback for a
  harness that offers no naming; a shape change makes the identifier
  unreadable, which degrades to a fresh conversation.
- **The shim becomes load-bearing for a user-visible guarantee**, having
  been an experimental mechanism. → Every failure path in it falls open to
  the launch that happens today; a missing shim loses continuity and nothing
  else.
- **A resumed agent lands in a checkout that moved on** (its task was
  delivered, its slot reset). → The record is keyed by task, and a task
  whose work is integrated is not relaunched into; the conversation follows
  the task, and where the two disagree the checkout wins because the agent
  reads it.
- **Two of the four read-backs are vendor caches, not APIs** (Antigravity's
  directory index, Codex's rollout files). → Both are read and never
  written, and a shape change makes an identifier unreadable, which is
  already the fresh-conversation path.

## Migration Plan

Nothing to migrate: a task with no record is exactly what a task looks like
today, and the first launch after the change records one. Rollback is
removing the shim's session step — every stored record then goes unread.

