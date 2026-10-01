# Delegation handoff, reports, and interactive subagents

Status: design, approved 2026-09-30. Not yet implemented.
Date: 2026-09-30.
Branch: `daemon`, cut from `main` at 0d4f9b6.

## The problem

`dispatch delegate "…"` sends one string. The subagent starts fresh with that
string as its whole brief: none of what the parent read, decided or ruled out
goes with it unless the parent happens to write it into the one line. And what
comes back is the last 8 KiB of the subagent's terminal, not an answer shaped
for the parent to use.

The user wants two things:

1. **A proper handoff.** Before delegating, the parent writes a structured
   handoff, and the subagent receives it. The point is better work from the
   subagent.
2. **A subagent they can watch.** Invisible stays the default, but an option
   runs the subagent in an ordinary interactive pane, the agent's real
   interface, that the user can watch and type into.

A subagent today is already a pane (`Ctrl p s` brings it into the grid), but
it runs the harness's one-shot form, and `claude -p` prints nothing until it
has finished. Measured on 2026-09-30: a print-mode run that slept 8 seconds
and ran a command showed 0 bytes for 14 seconds, then only `DONE`. There is
nothing to watch.

### Later, and shaping this now

The user plans a feature that passes one pane's output to another to build
workflows. Nothing here builds it, but the result of a delegation becomes a
message the daemon receives (a report), not bytes scraped off a screen, so
that feature has something clean to route.

### Not in this piece of work

- **Teaching agents that delegation exists.** No skill, no system prompt, no
  `AGENTS.md`. The user tells agents to delegate, as today. The handoff
  format is taught by the command itself: its `--help`, `--template`, and the
  error a bad handoff gets.
- **`[task]` forms for `agy` and `opencode`.** Both have one-shot modes
  (`agy --print`, `opencode run`), so both could gain one. A separate change.
- **Routing output between panes.** The workflow feature above.
- **A reminder for a subagent that never reports.** See "Gaps".

## Decisions taken with the user

- **Approach B: a handoff file with required sections**, over flags per
  section (shell quoting of long multi-line text is fragile for agents) and
  over free text with a template in `--help` (nothing enforced, which is
  today's problem).
- **The bare string form is refused.** `dispatch delegate "task"` exits 64
  and prints the template, so the agent can retry in the right shape.
- **Invisible one-shot by default; `--interactive` opts in** to a full
  interactive pane.
- **An interactive subagent closes itself once it has reported, by default.**
  `[delegation] interactive_on_done = "ask"` instead keeps it open, marked
  "reported", and asks before closing.
- **The subagent says it is done by running `dispatch report`.** Chosen over
  "the result file exists and the pane is idle": pane status (working, idle,
  blocked) is read off the screen by the interface client
  (`dispatch-tui/src/activity.rs`), and the daemon keeps only raw bytes. Judging
  "idle" in the daemon would mean a terminal emulator per pane and would not
  work with no window open. An explicit command is exact, works the same for
  every harness, and needs no file to clean up.

## The handoff

A Markdown file with five required level-two headings, in any order:

```markdown
## Goal
What the subagent is to achieve, in a sentence or two.

## Context
What the parent already knows: the files that matter, decisions made, what
was tried and ruled out.

## Constraints
What must not change, tools or approaches to avoid. "None" is allowed.

## Done when
How the subagent knows it has finished: tests that pass, a file that exists.

## Report back
What the report should contain, and in what shape.
```

Rules, checked by `dispatch delegate` before it connects and again by the
daemon (anything that can reach the socket can send a request):

- Each of the five headings appears exactly once, matched case-insensitively
  after trimming.
- Each section has non-blank text under it.
- Text before the first heading, and other `##` headings, are allowed and
  passed through as they are. Other headings belong to the section above them,
  so a parent can structure a long Context.
- The whole file is at most 64 KiB. The wrapped handoff becomes one command
  argument, and Linux limits one argument to 128 KiB.

### The command

```sh
dispatch delegate --handoff h.md [--harness codex] [--interactive] [--size 80x24]
dispatch delegate --handoff -           # read the handoff from standard input
dispatch delegate --template            # print an empty handoff and exit 0
```

`--handoff` is required. A positional task string is refused with exit 64, the
reason, and the template, all on stderr.

### What the subagent receives

The daemon wraps the handoff in one fixed preamble, the same for every harness,
and substitutes the result for `{task}` in the harness's form:

```text
You are a subagent started by Dispatch for the agent in pane <parent>.
Nobody can answer questions while you work: where something is unclear,
choose, and say what you assumed in your report.

When you have finished, run `dispatch report` with your report, either as a
file (`dispatch report report.md`) or on standard input
(`dispatch report -`). Your work is not delivered until you do.

<the handoff, verbatim>
```

The exact wording is settled in implementation and pinned by a test, so a
change to it is deliberate.

## Reports

```sh
dispatch report report.md
dispatch report -
```

`dispatch report` runs inside the subagent's pane. Every pane already has
`DISPATCH_PANE` and the `dispatch` binary on its `PATH` (`session.rs`
`pane_env`), including subagents, so every harness can run it as an ordinary
shell command.

It sends a new `ClientMessage::DelegateReport { pane, report }`. The daemon
accepts it when the pane is a subagent with an open request (the pane's
`request` and `caller` are still set). It then:

1. sends the caller `DelegateFinished` with `exit: 0` and the new
   `report: Some(text)` field, straight away, without waiting for the process
   to exit;
2. clears the pane's `request`, so the process exiting later sends nothing
   more;
3. for an interactive subagent, applies `interactive_on_done`.

`dispatch delegate` prints the report to stdout when there is one, and the
tail otherwise, as today.

A one-shot subagent that exits without reporting is answered as today, with
its exit code and tail, and `dispatch delegate` adds a line on stderr:
`[dispatch] no report was sent; returning the output tail`.

Reports are at most 1 MiB. A longer one is cut there, with a line saying so
appended. The protocol's frame limit is 64 MiB.

## Interactive subagents

### The harness form

Each harness file may declare `[task.interactive]` next to `[task]`, with the
same shape (`args`, `env`, per-platform overrides, `input`). It starts the
agent's ordinary interface with the wrapped handoff as its first prompt. The
shipped forms, taken from each CLI's `--help` on 2026-09-30 and each to be
checked by hand before release:

| Harness | `args` |
|---|---|
| claude | `["{task}"]` |
| codex | `["{task}"]` |
| agy | `["-i", "{task}"]` |
| opencode | `["--prompt", "{task}"]` |

Saved settings (model, effort, permissions) are appended as for any launch. A
harness without `[task.interactive]` refuses `--interactive` with exit 78.

On Windows, the same unsafe-form check as `[task]` applies. An interactive
agent cannot take its first prompt from a redirected standard input (standard
input is its terminal), so a form that would put the task on `cmd.exe`'s
command line is refused, with the reason given. The shipped harnesses get no
Windows interactive form until one is shown to be safe.

### Lifecycle

- The pane starts like any subagent: under its parent in the sidebar, focus
  unchanged, `Ctrl p s` to bring it into the grid.
- **On report**, with `interactive_on_done = "close"` (the default): the
  daemon sends `DelegateFinished` first, then closes the pane the way
  `Ctrl p x` does.
- **On report**, with `"ask"`: the pane stays open and is marked
  **reported**. It gets its own glyph and colour in the sidebar, and the status
  row counts reported panes as it counts blocked ones. Focusing it, or the
  status row's reopen key, shows: "Subagent finished and sent its report.
  Close this pane?" `y` closes it. `n` clears the mark and leaves an ordinary
  pane the user can go on talking to.
- **Never closed under the user.** An interactive subagent is never ended
  because its caller went away. One-shot subagents still are, as today
  (`abandon`).
- **If its caller has gone** (the `dispatch delegate` connection dropped)
  before the report, the report is refused: `dispatch report` exits 75 with
  "the agent that asked is no longer waiting", and the pane stays open,
  unmarked.
- **If the agent exits** before reporting, the caller gets exit 75 and
  "finished without a report". The tail is not sent: a full-screen
  interface's raw bytes are not readable output.

"Reported" is carried by its own server message (`SubagentReported { pane }`),
not a new `PaneStatus` variant, so an older client that cannot decode it loses
only the mark. The plan confirms how an older client treats an unknown server
message and adjusts if that would cost more than the mark.

### The live cap

`max_live_per_parent` counts a parent's **unfinished requests** (subagent
panes whose `request` is still set), not running processes. Otherwise an
interactive pane kept open after reporting, under `"ask"`, would hold a slot
for as long as it stays open.

## Protocol

All new fields carry `#[serde(default)]`, following `message.rs`.

- `ClientMessage::DelegateRequest` gains `handoff: Option<Handoff>` and
  `interactive: bool`. `task` stays, and a request carrying only `task` is
  refused with the same reason the command gives.
- `Handoff` holds the five sections and the full text, so the approval prompt
  can show sections and the subagent gets the file verbatim.
- New `ClientMessage::DelegateReport { pane: PaneId, report: String }`.
- `ServerMessage::DelegateFinished` gains `report: Option<String>`.
- `ServerMessage::DelegatePending` gains `handoff: Option<Handoff>` and
  `interactive: bool`, for the approval prompt.
- New `ServerMessage::SubagentReported { pane: PaneId }`, for `"ask"`.
- The protocol version stays 1.1: `dispatch-proto` bumps it only for a change
  an older peer cannot safely ignore, and optional fields and new messages are
  not such a change.

## Approval prompt

The prompt shows the five sections under their headings, in a fixed order
(Goal, Done when, Constraints, Context, Report back: what the user needs to
judge first comes first), and marks an interactive request as such. A long
handoff wraps and scrolls as a long task does today.

## Configuration

```toml
[delegation]
interactive_on_done = "close"   # or "ask"
```

An unknown value is a configuration error naming the two accepted values.

## Exit codes

`dispatch delegate`:

| Code | Meaning |
|---|---|
| 0–125 | the subagent's own exit code (one-shot, no report); 0 when it reported |
| 64 | no `--handoff`, or the handoff is missing a section, has an empty one, or is over 64 KiB; the template is printed |
| 66 | the handoff file cannot be read |
| 69 | no daemon is listening |
| 75 | as today, and: an interactive subagent finished without a report |
| 77 | denied |
| 78 | as today, and: no `[task.interactive]` form, or an unsafe interactive form on Windows |

`dispatch report`:

| Code | Meaning |
|---|---|
| 0 | delivered |
| 64 | empty report |
| 66 | the report file cannot be read |
| 69 | no daemon is listening |
| 75 | the agent that asked is no longer waiting, or the daemon does not know this pane |
| 78 | this pane is not a subagent with an open request, or it has already reported |

## Gaps, left open on purpose

- **An interactive subagent that never reports** keeps its caller waiting
  until the 24-hour backstop in `dispatch delegate`. The user sees it idle in
  the sidebar and can nudge it. No automatic reminder (Dispatch typing into the
  pane) yet.
- **A sandbox that blocks the socket** stops `dispatch report`, as it already
  stops `dispatch delegate`. Codex ships with its sandbox off.

## Testing

- **Unit:** handoff parsing and checking (each missing, empty, repeated and
  extra heading; text before the first heading; the 64 KiB limit); the
  preamble's exact text; which refusal wins when several apply;
  `[task.interactive]` parsing and its Windows refusal;
  `interactive_on_done` parsing.
- **Daemon session:** a report becomes `DelegateFinished` with `report` set and
  exit 0; a one-shot exit without one still sends the tail; a second report is
  refused; a report from a non-subagent is refused; `"close"` closes only
  after `DelegateFinished` is sent; `"ask"` sends `SubagentReported` and keeps
  the pane; a dropped caller never ends an interactive subagent; the cap
  counts unfinished requests.
- **End to end:** `dispatch/tests/delegate_shim.rs` gains a fake harness that
  runs `dispatch report`, and covers exit 64 (bad handoff), 66 (unreadable
  file), the report reaching stdout, and `--template`.
- **By hand:** each shipped `[task.interactive]` form (claude, codex, agy,
  opencode) starts its interface with the handoff as the first prompt, and the
  agent can run `dispatch report` from it.
