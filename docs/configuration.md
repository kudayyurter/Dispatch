# Configuring Dispatch

Where settings live, preferences, the shell panes open, per-agent harness settings, status rules, and motion. Rebinding keys is under [Keys in usage.md](usage.md#keys).

## The configuration directory

Dispatch keeps its settings in one directory:

| System | Directory |
|---|---|
| Linux | `~/.config/dispatch` |
| macOS | `~/Library/Application Support/dispatch` |
| Windows | `%APPDATA%\dispatch\config` |

`DISPATCH_CONFIG_DIR` points Dispatch at another directory. The examples below
show the Linux path. The directory holds `config.toml`, a `harnesses/`
directory with one file per agent (written on first run), your saved
`harness-settings.toml`, your `ui.toml`, and the project and machine lists.

## Interface state

How you left the interface lives in `ui.toml`, beside `config.toml`:

```toml
# ~/.config/dispatch/ui.toml
sidebar_width = 41        # 20 to 60 columns; 34 until you change it
sidebar_collapsed = true  # folded away with Alt s
```

Dispatch writes it, when you fold the sidebar or finish dragging its edge, so
`config.toml` keeps its comments. A width outside 20 to 60 is brought into
range, and a value that is not a number, or a file that is not TOML, falls
back to the default.

## Preferences

What you choose in Settings (see [Settings in usage.md](usage.md#settings)) is
kept in `preferences.toml`, beside `config.toml`:

```toml
# ~/.config/dispatch/preferences.toml
[interface]
motion = false
focus_follows_pointer = true
hover_claims_panes = true

[appearance]
theme = "light"       # "terminal" (the default), "dark" or "light"
accent = "teal"       # terminal (Theme default), violet, blue, teal, green, amber, red, pink, or "#rrggbb"
icons = "plain"       # "nerd" (the default) or "plain"
```

Every key is optional. A value is read as `preferences.toml` over
`config.toml`'s `[interface]` over the built-in default, so a key you have never
chosen in Settings keeps following `config.toml`. Settings writes only
`preferences.toml`; `config.toml` is never rewritten, so its comments stay.
Appearance has no `config.toml` setting.

A value that is wrong, such as `theme = "sepia"`, costs only itself: it is
logged and that setting falls back to the next layer, and the rest of the file
still applies. A file that is not TOML at all is left as it is for you to mend,
and Settings says so rather than overwriting it. Preferences are read when
Dispatch starts, and again when Settings opens.

## Shell panes

The picker's first entry is your own shell — `Shell · zsh`, or whatever
`$SHELL` is on the machine the project is on — so `Enter` opens a terminal
in the project's directory. It starts the way your terminal starts it (a
login shell on macOS, a plain interactive one elsewhere), so your rc file
runs and your prompt, Starship or otherwise, looks as it does anywhere else.
To choose it yourself:

```toml
# ~/.config/dispatch/config.toml
[shell]
command = "/usr/bin/fish"   # default: $SHELL, then your login record, then /bin/sh
args = []
login = "auto"              # auto | always | never
```

`[shell]` is read when the daemon (or a standalone Dispatch) starts, so
restart the daemon after changing it.

Every pane is told it is in Dispatch's terminal — `TERM=xterm-256color`,
`COLORTERM=truecolor`, `TERM_PROGRAM=dispatch` — and not the one Dispatch
runs in, so a program never sends it another terminal's private sequences.
A harness's own `env` still wins.

## Harness settings

Every agent Dispatch ships starts **without its permission prompts**;
[security-model.md](security-model.md#agents-start-without-permission-prompts)
lists each agent's flag and what it lets the agent do. Claude Code refuses
that mode when it runs as root, so on such a machine save another
**Permissions** mode for Claude (`e`, then `s`), or, only when the machine
really is a sandbox such as a container, set `IS_SANDBOX = "1"` in that
machine's `claude.toml` under `[env]`, which Claude reads as permission to
bypass.

In the new-pane picker, `e` opens the highlighted harness's settings:

| Key | Does |
|---|---|
| `↑` `↓` | move between settings |
| `←` `→` | step through a setting's values; `Space` flips one that is on or off |
| `Enter` | open a pane with what is shown, saving nothing |
| `s` | save what is shown as the harness's default |
| `Esc` | back to the picker |

A model that is not on the list is typed on the setting's **Custom…** value.
**agent default** passes nothing, and the agent decides as its own
configuration says. Beside each harness, the picker shows what you have saved
that differs from its file.

opencode takes its model from a variable its shared background service never
sees, so an opencode pane with a chosen model runs a private opencode server
of its own.

Saved defaults live beside `config.toml`:

```toml
# ~/.config/dispatch/harness-settings.toml
[claude]
model = "opus"
permissions = ""   # agent default: Claude asks, as it does outside Dispatch

[codex]
bypass = false     # Codex's prompts and sandbox are back
```

These are per machine: a subagent follows the saved defaults of the machine
its daemon runs on, as [delegation.md](delegation.md) explains.

A harness file says what its settings are, and how each becomes a flag, so a
harness you add can have them as well:

```toml
# ~/.config/dispatch/harnesses/claude.toml
[[settings]]
key = "effort"
label = "Effort"
kind = "choice"                 # choice | text | bool
options = ["low", "medium", "high", "xhigh", "max"]
custom = false                  # true adds a typed value to a choice
args = ["--effort", "{value}"]  # after the launch's own; nothing when unset
# env = { NAME = "{value}" }    # or a variable

[[settings]]
key = "permissions"
label = "Permissions"
kind = "choice"
options = ["bypassPermissions", "auto", "acceptEdits", "plan", "manual"]
default = "bypassPermissions"
args = ["--permission-mode", "{value}"]
```

An option can be a table instead, to show a clean name, and one choice can
limit another. While a model is chosen, a setting `limited_by` the model
offers only the values that model lists for it, and moves to the highest of
them when the one set is not among them. A model that lists none fades the
row out, and passes nothing for it. A model typed in by hand leaves it free.

```toml
# ~/.config/dispatch/harnesses/agy.toml
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = [
  { value = "gemini-3.1-pro", label = "Gemini 3.1 Pro", effort = ["low", "high"] },
  { value = "claude-sonnet-4-6", label = "Claude Sonnet 4.6" },  # no effort
]
custom = true
args = ["--model", "{value}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
limited_by = "model"
options = [{ value = "low", label = "Low" }, { value = "high", label = "High" }]
args = ["--effort", "{value}"]
```

A `bool` adds its `args` when it is on. A value may hold only letters, digits
and `. _ : / @ # + -`, and may not start with `-`: on Windows it sits on
`cmd.exe`'s command line. The daemon reads harness files when it starts, so
restart it after editing one, and after upgrading Dispatch too: a daemon
started before this release ignores the settings a newer client sends, and
keeps the harness files it already loaded.

A harness file you have edited is left as it is when Dispatch upgrades: it
keeps its prompts and offers no settings until you delete it (Dispatch
writes the current one back on its next start) or add `[[settings]]` to it
yourself.

## Status rules

A pane's state comes from its terminal: output arriving means it is working,
and quiet means idle -- though not the echo of your own typing, or its repaint
after a resize. Each harness's rules recognise what activity alone cannot -- a
spinner in the title, a permission prompt. `claude`, `codex`, `opencode` and
`agy` have rules built in, adapted from
[herdr](https://github.com/herdrdev/herdr)'s detection manifests. A
harness's own TOML can carry its own:

```toml
# ~/.config/dispatch/harnesses/claude.toml
# Claude Code's permission prompt: the question, with a numbered yes under it.
[[status.rules]]
state = "blocked"
region = "bottom:15"
contains = ["do you want to proceed?"]
regex = ['(?i)^\s*❯?\s*1\.\s*yes\b']
priority = 990
```

- `state` is `working`, `idle` or `blocked`.
- `region` is where to look: `title`, the title the program last set, spinner
  and all; `progress`, its last `OSC 9;4` progress report, after the `9;`;
  `bottom:N`, the last N non-blank lines of the screen; or `screen`, all of it.
- `contains` must all appear, `any` at least one, and `not` none; all three
  ignore case. `regex` must match some line of the region, and is
  case-sensitive unless it says `(?i)`. A rule needs at least one of
  `contains`, `any` or `regex`.
- `reason` is an optional short text on a `blocked` rule, saying what the pane
  is waiting on. It shows in the list of waiting agents and on the focused
  pane's border; "Needs approval" stands in when a rule has none.
- `priority` orders the rules, highest first, ties in file order, and the
  first that matches decides. An `idle` rule does not outrank output still
  arriving.

A harness's own `[status]` replaces the built-ins for it rather than adding to
them, so start from a copy of them in
[`crates/dispatch-config/src/status/builtin.rs`](../crates/dispatch-config/src/status/builtin.rs). `[status]` with `rules = []`
means activity alone decides; with no `[status]` at all, the built-ins for its
id apply, and a harness with none goes by activity alone. A rule with an
unknown state or region, a regex that does not compile, or nothing to match on
is logged and skipped, and the harness loads without it.

## Motion

Working panes spin, a pane that wants you pulses its row, focus eases from one
border to the next, a new pane draws its border in and a closed one retracts
it, and the active tab's tint slides across. To keep the screen still:

```toml
# ~/.config/dispatch/config.toml
[interface]
motion = false   # default true
```

Every change then shows at once, a working pane shows a still play glyph, and
nothing pulses; every state is still shown.

Settings can override this; see [Preferences](#preferences).

## Pointer

A click on a pane gives it the keyboard, and a click on its header does too
without sending anything to the program inside; a double-click on the header
zooms the pane. Resting the pointer on a pane does not move the keyboard, so
a pointer crossing the grid never redirects typing. To have it follow the
pointer instead:

```toml
# ~/.config/dispatch/config.toml
[interface]
focus_follows_pointer = true   # default false
```

This is separate from `hover_claims_panes`, which only decides whether the
window under the pointer sets the size of panes shared between windows (see
[daemon.md](daemon.md)). Neither implies the other. Scroll mode ignores
`focus_follows_pointer`, since its keys act on the pane it was entered on.

Both can also be set in Settings, which overrides `config.toml`; see
[Preferences](#preferences).

[Back to the README](../README.md)
