# Delegation

An agent in a pane can ask for a second agent to work on something:

```sh
dispatch delegate "write the tests for the http client"
```

Dispatch asks you first, every time — unless you have approved that pane
wholesale with `A`, which lasts until the daemon stops. The subagent runs as a
pane under the one that asked, and the caller gets its output and exit code when
it finishes. Focus stays where it was: open the subagent with `Ctrl p s` (or
`^a s`) to watch it.

Delegation needs two things. The daemon must own the panes (`--attach`), because
it is what starts the subagent; and the harness must declare a non-interactive
form, since an interactive agent never exits:

```toml
# ~/.config/dispatch/harnesses/claude.toml
[task]
args = ["-p", "{task}"]

# cmd.exe would run the task's & and % as commands, so on Windows the task
# is written to a file and redirected into the agent's standard input.
[task.platform.windows]
args = ["/d", "/v:off", "/c", "claude", "-p", "<%DISPATCH_TASK_FILE%"]
input = "file"
```

`claude` and `codex` ship with one. A subagent starts on the daemon's own
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
max_live_per_parent = 4
request_timeout_secs = 600 # no value disables this; 0 refuses on the next tick
```

There is deliberately no way to turn the deadline off: an agent on an unattended
daemon would otherwise wait for a person who is not there. To wait longer, raise
the number.

The approval prompt takes `a` to approve, `d` to deny, `A` to approve everything
from that pane for this daemon's lifetime, and `Esc` to defer. The status line
reports how many are waiting and which key reopens them — that key is `^a a`,
or wherever `[keys]` has moved it.

There are also keyboard bindings to open and close a subagent pane: `^a s` expands
the focused pane's next child into the tiled grid, and `^a c` collapses it back out.

The subagent's output goes to stdout and every status line to stderr, so
`dispatch delegate "…" > result.md` captures the work and nothing else. Fan-out
needs no feature: the agent's own shell does it:

```sh
dispatch delegate "write the tests" > tests.md &
dispatch delegate "write the docs"  > docs.md  &
wait
```

Exit codes follow `sysexits(3)` so an agent can branch without parsing prose.
Dispatch's own codes (69, 75, 77, 78) sit inside the same 0–125 band a subagent's
own exit code comes from, so a subagent that exits 78 is indistinguishable from a
refusal. An agent branching on exit codes should keep that in mind.

| Code | Meaning |
|---|---|
| 0–125 | the subagent's own exit code |
| 69 | no daemon is listening |
| 75 | timed out, or the connection dropped, or the daemon does not know the asking pane, or the subagent was stopped before it finished |
| 77 | denied by the user |
| 78 | refused: caps, or the harness has no `[task]` form |

[Back to the README](../README.md)
