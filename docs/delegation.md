# Delegation

An agent in a pane can ask for a second agent to work on something. It writes
a handoff first: a Markdown file that says what the subagent is to do and what
the asking agent already knows, since the subagent starts with none of it.

```markdown
## Goal
Write unit tests for the http client in src/http.rs.

## Context
The client wraps ureq. Retries live in retry.rs and are already tested; the
timeout path is not. I tried mocking with httpmock and dropped it: the
tests there need a real socket.

## Constraints
Do not change src/http.rs. No new dependencies.

## Done when
`cargo test -p client` passes and covers timeouts and a 500 response.

## Report back
The names of the tests added, and anything in http.rs that looks wrong.
```

```sh
dispatch delegate --handoff h.md
```

All five sections are required, each once and each with text under it ("None"
is fine for Constraints). `dispatch delegate --template` prints an empty
handoff, and `--handoff -` reads one from standard input. A bare string, as in
`dispatch delegate "write the tests"`, is refused with exit 64 and the template,
so an agent that tries it learns the shape. The file may hold other `##`
headings and text before the first one; they go through as written. The limit is
64 KiB.

Dispatch asks you first, every time — unless you have approved that pane
wholesale with `A`, which lasts until the daemon stops. The prompt shows any text
above the first heading, then the five sections, Goal and Done when first. The
subagent runs as a pane under the one that asked, and the caller gets its report,
or its output and exit code, when it finishes. Focus stays where it was:
`Ctrl p s` (or `^a s`) brings the subagent into the grid to watch it, and
`Ctrl p c` (or `^a c`) puts it back.

The subagent receives the handoff word for word, after a short preamble that
says who started it, that nobody can answer questions while it works, and how to
report.

## Reporting back

The subagent is told to finish by running `dispatch report`, with its report as
a file or on standard input:

```sh
dispatch report report.md
dispatch report - <<'EOF'
Added 6 tests. http.rs ignores the timeout when retrying.
EOF
```

The caller's `dispatch delegate` prints that report on stdout and exits 0, at
once, without waiting for the subagent's process to end. A report is cut at
1 MiB. Without one, `dispatch delegate` falls back as before: the subagent's
exit code and the tail of its output, plus a line on stderr saying no report was
sent. The report is a message the daemon receives, not text scraped off a
screen, which is why an interactive subagent can use it too.

Delegation needs two things. The daemon must own the panes (`--attach`), because
it is what starts the subagent; and the harness must declare a non-interactive
form, since an interactive agent never exits on its own:

```toml
# ~/.config/dispatch/harnesses/claude.toml
[task]
args = ["-p", "{task}"]

# cmd.exe would run the task's & and % as commands, so on Windows the task
# is written to a file and redirected into the agent's standard input.
[task.platform.windows]
args = ["/d", "/v:off", "/c", "claude", "-p", "<%DISPATCH_TASK_FILE%"]
input = "file"

# For --interactive: the agent's own interface, with the task as its first
# prompt. There is no Windows form.
[task.interactive]
args = ["{task}"]
```

`claude` and `codex` ship with a `[task]` form. A subagent starts on the daemon's own
machine and follows that machine's saved settings, auto-approve included,
so it can use its tools without waiting on a prompt nobody would see —
not a one-off choice made in the pane that asked for it, and not another
machine's saved file. On Windows the
daemon refuses a form that would put the task on `cmd.exe`'s
command line, and says which file to fix. Caps live in `config.toml`,
and refuse rather than prompt:

```toml
[delegation]
max_depth = 1              # a subagent cannot delegate
max_live_per_parent = 4    # counts requests not yet finished, not processes
request_timeout_secs = 600 # no value disables this; 0 refuses on the next tick
interactive_on_done = "close"   # or "ask"
```

There is deliberately no way to turn the deadline off: an agent on an unattended
daemon would otherwise wait for a person who is not there. To wait longer, raise
the number.

The approval prompt takes `a` to approve, `d` to deny, `A` to approve everything
from that pane for this daemon's lifetime, and `Esc` to defer. The status line
reports how many are waiting and which key reopens them — that key is `^a a`,
or wherever `[keys]` has moved it.

## Watching a subagent

`--interactive` starts the agent's own interface instead of its one-shot form,
with the handoff as its first prompt, so you can watch it work and type into it
(`Ctrl p s` brings it into the grid). A one-shot run prints nothing until it has
finished, so there is nothing to watch.

```sh
dispatch delegate --handoff h.md --interactive
```

It needs the harness's `[task.interactive]` form, shown above; `claude`, `codex`,
`agy` and `opencode` ship with one. A harness without it refuses `--interactive`
with exit 78. Windows has no interactive form: the first prompt would have to sit
on `cmd.exe`'s command line, and an interactive agent cannot read it from a
redirected file because its standard input is the terminal. The daemon refuses
one that tries, and says why.

When the subagent runs `dispatch report`, what happens to its pane follows
`interactive_on_done`. `"close"`, the default, closes the pane once the report
has been delivered, as `Ctrl p x` would. `"ask"` keeps it open, marked as
reported in the sidebar, and the status line's reopen key asks "Close this
pane?": `y` closes it, `n` leaves it as an ordinary pane you can keep talking to,
in every window. Focusing the pane does not ask, since focus follows the mouse.
Dispatch never closes an interactive subagent under you for any other reason, not
even if the agent that asked has gone. If that agent has gone before the report,
`dispatch report` exits 75 and the pane stays. If the interface exits without
reporting, the caller gets exit 75 and no tail, because a full-screen interface's
raw bytes are not readable output.

An interactive subagent that never reports keeps its caller waiting, until the
24-hour backstop in `dispatch delegate`. You will see it sitting idle in the
sidebar and can nudge it.

## Output and fan-out

The report, or the output, goes to stdout and every status line to stderr, so
`dispatch delegate --handoff h.md > result.md` captures the work and nothing
else. Fan-out needs no feature: the agent's own shell does it:

```sh
dispatch delegate --handoff tests.md > tests.out &
dispatch delegate --handoff docs.md  > docs.out  &
wait
```

Exit codes follow `sysexits(3)` so an agent can branch without parsing prose.
Dispatch's own codes (69, 75, 77, 78) sit inside the same 0–125 band a subagent's
own exit code comes from, so a one-shot subagent that exits 78 is
indistinguishable from a refusal. An agent branching on exit codes should keep
that in mind. A subagent that reported always gives 0.

`dispatch delegate`:

| Code | Meaning |
|---|---|
| 0–125 | the subagent's own exit code; 0 when it reported |
| 64 | no `--handoff`, or the handoff lacks a section, has an empty one, or is over 64 KiB; the template is printed |
| 66 | the handoff file cannot be read |
| 69 | no daemon is listening |
| 75 | timed out, or the connection dropped, or the daemon does not know the asking pane, or the subagent was stopped before it finished, or an interactive subagent finished without a report |
| 77 | denied by the user |
| 78 | refused: caps, or the harness has no `[task]` (or, with `--interactive`, `[task.interactive]`) form |

`dispatch report`:

| Code | Meaning |
|---|---|
| 0 | delivered |
| 64 | the report is empty |
| 66 | the report file cannot be read |
| 69 | no daemon is listening |
| 75 | the agent that asked is no longer waiting, or the daemon does not know this pane |
| 78 | this pane is not a subagent with an open request, or it has already reported |

[Back to the README](../README.md)
