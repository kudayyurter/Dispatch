# D — Keybindings: zellij-style modes, configurable

Status: approved design, not yet implemented.
Date: 2026-09-27.
Last of four UI slices: A visual refresh (merged), B live status and motion
(merged), C tabs and shell panes (PR #2), D this.
Builds on: `docs/superpowers/specs/2026-09-25-tabs-and-shells-design.md` —
it extends C's `Ctrl t` tab mode, so this branch (`ui/keybindings`) is cut
from `ui/tabs-shells`.

## The problem

The user asked, at the start of the UI overhaul, "Is there a better
keybinding system we can do?". Today almost every command sits behind one
hard-coded prefix, `Ctrl a`, read by hand-written matches in
`crates/dispatch-tui/src/input.rs` (`command_for`, plus slice C's `tab_key`
and `direct`). Nothing can be rebound, there is no real way to read
scrollback from the keyboard (`^a [` scrolls ten lines), and there is no way
to give every key to an agent that wants the ones Dispatch takes.

The user works in zellij every day with its default model (their
`~/.config/zellij/config.kdl`): a mode per domain entered by a `Ctrl` key —
`Ctrl p` pane, `Ctrl t` tab, `Ctrl s` scroll, `Ctrl o` session, `Ctrl g`
lock — plus direct `Alt` keys. Dispatch does not run inside zellij, so
nothing outside Dispatch competes for these keys. Slice C already added the
tab mode and the `Alt` keys; this slice finishes the model and makes it
configurable.

### Not in this slice

- **Resize and move modes.** Dispatch's grid lays itself out; there is
  nothing to resize or drag.
- **Search in scrollback.** Scroll mode reads scrollback; searching it is a
  later feature.
- **Reading zellij's KDL.** Bindings are written in `config.toml`, the file
  every other Dispatch setting lives in.
- **Live reload.** Bindings are read at start, as the rest of `config.toml` is.

## Decisions taken with the user

- **Modes are the primary system; `Ctrl a` stays as a tmux-style prefix**
  with every command it has today — the way zellij keeps `Ctrl b` as a tmux
  mode. Nothing already learned breaks.
- **Bindings are configurable** in a `[keys]` section of `config.toml`,
  using zellij-like key names; anything not mentioned keeps its default.
- **New modes**: pane (`Ctrl p`), scroll (`Ctrl s`), session (`Ctrl o`),
  lock (`Ctrl g`), alongside C's tab mode (`Ctrl t`).
- **One keymap table** (approach 1 of 3): every mode is an ordered list of
  chord → command, the built-in defaults and the user's `[keys]` are the same
  kind of thing, and each mode's status row is generated from its table.
- **`Ctrl q` does not quit by default.** Standalone, quitting ends every
  agent Dispatch started; a stray `Ctrl q` is too expensive. Quit stays at
  `Ctrl o q` and `^a q`, and `[keys]` can bind `Ctrl q`.

## Default bindings

### Normal mode

Every key goes to the focused pane except these:

| Chord | Command |
|---|---|
| `Ctrl p` | pane mode |
| `Ctrl t` | tab mode |
| `Ctrl s` | scroll mode |
| `Ctrl o` | session mode |
| `Ctrl g` | lock |
| `Ctrl a` | prefix (one command key, then back to normal) |
| `Alt n` | new pane (in this tab) |
| `Alt i` / `Alt o` | move this tab left / right |
| `Alt Left` / `Alt h` | focus left, crossing to the previous tab at the grid's edge |
| `Alt Right` / `Alt l` | focus right, crossing to the next tab at the grid's edge |
| `Alt Up` / `Alt k` | focus up |
| `Alt Down` / `Alt j` | focus down |

### In every mode other than normal and lock

- `Esc` and `Enter` leave the mode. They are ordinary table entries, bound
  to `leave_mode` — in scroll mode to `leave_scroll`, which also jumps back
  to live output — and a safety rail keeps `Esc` leaving whatever `[keys]`
  says.
- The chord that entered the mode — the normal-mode chord bound to that
  mode's command — pressed in it, is sent to the pane as a key and leaves
  the mode (`Ctrl p Ctrl p` gives the pane `Ctrl p`), so a
  program that uses the key can still get it. For the prefix this is today's
  `^a ^a`.
- Any other unbound key is ignored and the mode stays on, so a stray key
  never reaches an agent.
- A mouse click leaves the mode, then does what it would have anyway. A
  paste leaves the mode and goes to the pane.

### Pane mode (`Ctrl p`)

| Chord | Command | Mode after |
|---|---|---|
| `n` | new pane in this tab (the picker) | ends |
| `x` | close the focused pane | ends |
| `f`, `z` | zoom the focused pane, or restore the grid | ends |
| `h` / `Left` | focus left | stays |
| `j` / `Down` | focus down | stays |
| `k` / `Up` | focus up | stays |
| `l` / `Right` | focus right | stays |
| `p` | focus the next pane on this tab | stays |
| `s` | open the focused pane's next subagent into the grid | stays |
| `c` | collapse a subagent back out of the grid | stays |

### Tab mode (`Ctrl t`)

As slice C built it — `n` new tab, `r` rename, `x` close, `Left`/`h`
previous, `Right`/`l` next, `[` `]` move the pane, `i` `o` move the tab,
`1`–`9` go to a tab, `Tab` the last tab — plus `j` next tab and `k`
previous tab, as in the user's zellij config.

### Scroll mode (`Ctrl s`)

Scrolls the focused pane's scrollback. On the alternate screen (a program
that owns the whole screen, such as vim) there is no scrollback and the keys
change nothing; the mode can still be left the usual way.

| Chord | Command | Mode after |
|---|---|---|
| `j` / `Down` | one line down (newer) | stays |
| `k` / `Up` | one line up (older) | stays |
| `d` | half a pane down | stays |
| `u` | half a pane up | stays |
| `PageDown`, `Ctrl f`, `l`, `Right` | a pane down | stays |
| `PageUp`, `Ctrl b`, `h`, `Left` | a pane up | stays |
| `g` | the oldest output | stays |
| `G` | the newest output | stays |
| `Esc`, `Enter`, `Ctrl c` | leave, and jump back to live output | ends |

`^a [` now enters scroll mode instead of scrolling ten lines. The mouse
wheel scrolls as it does today, in any mode.

### Session mode (`Ctrl o`)

| Chord | Command |
|---|---|
| `p` | project picker |
| `o` | open a project |
| `m` | add a machine |
| `H` | harness manager |
| `a` | approvals |
| `f` | fold (the focused pane's subagents, or its project) |
| `q` | quit |

Each ends the mode.

### Lock mode (`Ctrl g`)

Every key goes to the pane until `Ctrl g` again, which unlocks. Nothing
else is looked up; the mouse still works.

### Prefix (`Ctrl a`)

Every existing `^a` command, unchanged: `n` `x` `z` `h` `j` `k` `l` `p` `H`
`a` `s` `c` `f` `o` `m` `q`, `1`–`9`, `Tab`, and `[` (now scroll mode).
One command key, then back to normal.

## The keymap

A new module, `crates/dispatch-tui/src/keymap.rs`.

- **`Chord`** — a key plus modifiers, parsed from and printed as the text a
  user writes: `"Ctrl t"`, `"Alt n"`, `"x"`, `"H"` (a capital means shift),
  `"Esc"`, `"Enter"`, `"Tab"`, `"Space"`, `"Backspace"`, `"Left"` `"Right"`
  `"Up"` `"Down"`, `"PageUp"` `"PageDown"`, `"Home"` `"End"`, `"F1"`–`"F12"`,
  and `Ctrl`/`Alt`/`Shift` combined with any of them (`"Ctrl Alt x"`). Parsing
  is case-insensitive for modifier and named-key words, case-sensitive for
  a single character. A chord prints back as it is written; the status row
  uses a short form of the same text that shows the arrow keys as `←` `→`
  `↑` `↓`.
- **`Command`** — every named thing a key can do, each with a snake_case
  name used in `[keys]`, a short label for the status row, and whether it
  keeps its mode on:
  - modes: `pane_mode`, `tab_mode`, `scroll_mode`, `session_mode`, `lock`,
    `unlock`, `prefix`, `leave_mode`, and `none` (unbinds);
  - panes: `new_pane`, `close_pane`, `zoom`, `focus_left` `focus_right`
    `focus_up` `focus_down`, `focus_left_or_tab` `focus_right_or_tab`,
    `focus_next`, `expand_child`, `collapse_child`;
  - tabs: `new_tab`, `rename_tab`, `close_tab`, `previous_tab`, `next_tab`,
    `last_tab`, `move_pane_left`, `move_pane_right`, `move_tab_left`,
    `move_tab_right`, `go_to_tab_1` … `go_to_tab_9`;
  - scroll: `scroll_down`, `scroll_up`, `scroll_half_down`,
    `scroll_half_up`, `scroll_page_down`, `scroll_page_up`, `scroll_top`,
    `scroll_bottom`, `leave_scroll`;
  - session: `project_picker`, `open_project`, `add_machine`,
    `harness_manager`, `approvals`, `fold`, `quit`.
- **`KeyMode`** — `Normal`, `Prefix`, `Pane`, `Tab`, `Scroll`, `Session`,
  `Lock`.
- **`Keymap`** — for each mode, an ordered list of (chord, command). Order
  is kept because the status row lists keys in it. `Keymap::defaults()` is
  exactly the tables above. `Keymap::with_overrides(&KeysConfig)` lays the
  user's `[keys]` over the defaults and returns the keymap plus a list of
  warnings.

`InputRouter` holds a `Keymap` and the current `KeyMode`, and turns each
event into the `Action`s the app already handles (plus the new scroll ones):

- **Normal**: a bound chord runs its command; any other key goes to the pane.
- **Prefix**: one lookup, then back to normal.
- **Pane / Tab / Scroll / Session**: a bound chord runs its command and the
  mode ends unless the command keeps it on; the mode's own entering chord
  goes to the pane and ends it; `Esc` and `Enter` end it; anything else is
  ignored.
- **Lock**: only a chord bound to `unlock` is looked up; everything else
  goes to the pane.
- A click ends any mode but lock; a paste ends any mode but lock and goes to
  the pane.

`command_for`, `tab_key`, `direct` and the `Prefix` type are replaced by
the table. The router exposes `key_mode()` and `leave_mode()` as slice C's
did.

## `[keys]` in `config.toml`

```toml
# Only what you change; everything else keeps its default.
[keys.normal]
"Ctrl q" = "quit"        # bind something new
"Ctrl a" = "none"        # give Ctrl a back to the shell…
"Ctrl b" = "prefix"      # …and use tmux's key for the prefix instead

[keys.pane]
"w" = "close_pane"       # a second key for a command
"x" = "none"             # unbind a default

[keys.lock]
"Ctrl l" = "unlock"
```

- One table per mode: `normal`, `prefix`, `pane`, `tab`, `scroll`,
  `session`, `lock`. Each maps chord text to a command name.
- A binding replaces that chord's default in that mode; `"none"` removes it.
  A new chord is appended after the mode's defaults, in file order.
- `clear = true` inside a table drops that mode's defaults first, as
  zellij's `clear-defaults` does.
- `dispatch-config` gains `KeysConfig`: the raw tables, kept as strings so a
  mistake costs one binding rather than the file. `Config` gains `keys`.
  Parsing and meaning live in `dispatch-tui`, which owns `Chord` and
  `Command`.
- **Mistakes never stop Dispatch.** An unknown mode, a chord that does not
  parse, or an unknown command name is logged by name, as unknown config
  keys are (`keys.pane."Ctrl ?": not a key`), and that binding is skipped;
  the rest still applies.
- **Safety rails.** A config cannot leave the user stuck:
  - if lock would have no `unlock` chord, the default `Ctrl g` unlock is
    kept, with a warning;
  - every mode other than normal and lock always leaves on `Esc`, whatever
    the config says;
  - a chord bound to a mode-entering command in any mode other than normal
    and prefix is refused with a warning (modes are entered from normal, or
    through the prefix, as `^a [` enters scroll mode).
- Keys are the client's; the daemon ignores `[keys]`. `unknown_keys` learns
  the `keys.<mode>` tables.

## The status row

Generated from the keymap, so it always matches the bindings:

- **Normal**: after the pane count, machine and `tab n/m` as today, the key
  help lists, in this order, the first normal-mode chord bound to each of
  `pane_mode`, `tab_mode`, `scroll_mode`, `session_mode`, `lock` and
  `prefix`, with the labels `pane`, `tabs`, `scroll`, `session`, `lock` and
  `prefix`: `Ctrl p pane  Ctrl t tabs  Ctrl s scroll  Ctrl o session  Ctrl g lock  Ctrl a prefix`.
  A command left with no chord is left out.
- **Pane / Tab / Scroll / Session**: the mode's name in capitals, then each
  command in table order with its first one or two chords and its label:
  `PANE  n new  x close  f/z zoom  h/← left  …  Esc done`. As slice C built
  it, a status message (such as a refusal) sits between the name and the
  keys, entering a mode clears a stale message, and the row is drawn in the
  highlighted style.
- **Lock**: `LOCKED  Ctrl g unlock` (the chord as bound), highlighted.
- **Prefix**: `PREFIX`, as today.

Slice C's hand-written `TAB_MODE_HELP` is replaced by the generated row; its
wording changes slightly, and the tests that pin it move with it.

## Compatibility

- With no `[keys]`, every existing `^a` binding, C's `Ctrl t` keys and the
  `Alt` keys behave as they do today, except `^a [`, which now enters
  scroll mode.
- `Ctrl p`, `Ctrl s`, `Ctrl o` and `Ctrl g` are newly taken from panes
  (shell history, XON/XOFF, operate-and-get-next, emacs cancel). Pressing
  one twice sends it through; lock mode or `"none"` in `[keys]` gives it
  back for good.
- The protocol does not change.

## Code layout

- `crates/dispatch-tui/src/keymap.rs` (+ tests): `Chord`, `Command`,
  `KeyMode`, `Keymap`, defaults, overrides, warnings, status-row help.
- `crates/dispatch-tui/src/input.rs`: the router over the keymap.
- `crates/dispatch-config/src/config.rs`: `KeysConfig`, `Config.keys`,
  `unknown_keys`.
- `dispatch/src/app.rs`: scroll-mode actions, the generated status row, and
  `App::set_keymap`.
- `dispatch/src/main.rs`: builds the keymap from `config.keys`, logs the
  warnings, hands it to the app.

## Failure cases

| Case | Behaviour |
|---|---|
| A chord in `[keys]` that does not parse | logged by name, skipped |
| An unknown command or mode in `[keys]` | logged by name, skipped |
| `[keys]` leaves lock with no unlock | default `Ctrl g` unlock kept, logged |
| `[keys]` unbinds `Esc` in a mode | `Esc` still leaves it |
| A mode-entering command bound inside a mode | refused, logged |
| Scroll mode on the alternate screen | keys change nothing; the mode still leaves |
| The focused pane closes while in scroll mode | the mode ends |
| A mode is on and an overlay opens (picker, prompt) | the command that opened it ended the mode first, so keys go to the overlay |

## Testing

- **Chords**: parse and print round-trips for every named key and modifier
  combination; bad chords give an error naming the text.
- **Keymap**: defaults equal the tables above; an override replaces, adds
  and unbinds; `clear = true`; each warning; each safety rail.
- **Router**: every default binding in every mode; which commands keep
  their mode on; a mode's own chord twice goes through; `Esc`/`Enter`;
  unbound keys ignored in modes and passed through in normal; lock passes
  everything but unlock; prefix is one key; click and paste end a mode;
  a custom binding from `[keys]` takes effect.
- **App**: each scroll command's effect on the focused pane's viewport,
  leaving jumps back to live output, the mode ends when the focused pane
  closes; the generated status row for each mode, including a status
  message and the highlighted style; `^a [` enters scroll mode.
- **Config**: `[keys]` parses, unknown modes are reported, the daemon
  ignores it.
- **End to end**: `Ctrl s` then `k` scrolls a shell's output back and `Esc`
  returns; `Ctrl g` then `Ctrl t` reaches the shell (the shell sees it), then
  `Ctrl g` unlocks; a `[keys]` rebinding in the fixture's `config.toml` takes
  effect.

## Documentation

The README's key sections (The grid, Tabs, and the `^a` list) are rewritten
from the new tables, with a `[keys]` section and the cost of the `Ctrl`
keys each mode takes.
