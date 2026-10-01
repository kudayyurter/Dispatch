# Using Dispatch

The panes, tabs, keys and sidebar, in full. Settings and harness files are in [configuration.md](configuration.md).

## The grid

Every pane is drawn inside a thin border carrying its title, so one agent's
output cannot be mistaken for the next one's or for the sidebar.

At most four panes are tiled at once, on a tab. Tabs are yours: a new pane
opens on the tab you are on, and a fifth on a full tab opens the next one.
Closing a pane never moves panes on other tabs. The sidebar always lists
every pane, whichever tab it is on.

A pane whose process exits gives its tile back straight away and the remaining
panes spread into the space. It stays in the sidebar, where selecting it shows
what it printed — `Ctrl p x` (or `^a x`) is what removes it for good.

On Linux and macOS, a pane whose program has exited keeps its process as a
`<defunct>` entry under `dispatchd` until you close the pane; closing it
clears the entry.

## Tabs

The row across the top names each tab after its first pane's title, or the
name you give it, with `+` at its end for a new one. Click a tab to go to
it. When there are more tabs than fit, the row scrolls to keep yours in view.

`Ctrl t` enters tab mode, and its keys are:

| Key | Does |
|---|---|
| `n` | new tab: the picker, with your shell first |
| `r` | rename the tab (empty goes back to the first pane's title) |
| `x` | close the tab and every pane on it, after a y/n |
| `←` `→` / `h` `l` / `k` `j` | previous / next tab |
| `[` `]` | move the focused pane to the previous / next tab (a new one past the last) |
| `i` `o` | move the tab left / right |
| `1`–`9` | go to a tab by position |
| `Tab` | the tab you were on before |
| `Esc` / `Enter` | leave tab mode |
| `Ctrl t` | send `Ctrl t` itself to the pane (Claude Code and fzf use it) |

A daemon keeps its projects' tabs, so they survive detaching and look the
same from every client. A daemon older than tabs still works: its panes are
grouped four at a time, as before.

## Keys

Keys work the way zellij's do: a `Ctrl` key enters a mode for one kind of
thing, the footer names the mode you are in, and `Esc` leaves it. Every
other key goes to the pane. The footer does not list keys: `Alt /` does, in
command help.

| Key | Mode |
|---|---|
| `Ctrl p` | pane: `n` new, `x` close, `f`/`z` zoom, `h` `j` `k` `l` or arrows to move focus, `p` next pane, `s` open a subagent, `c` collapse it |
| `Ctrl t` | tab: see [Tabs](#tabs) above |
| `Ctrl s` | scroll: `j` `k` a line, `d` `u` half a page, `PageDown` `PageUp` (or `Ctrl f` `Ctrl b`, `l` `h`) a page, `g` `G` the oldest and newest output; `Esc` returns to live output |
| `Ctrl o` | session: `p` projects, `o` open a project, `m` add a machine, `H` harnesses, `a` approvals, `f` fold, `q` quit, `w` list the agents waiting on you, `b` fold the sidebar, `<` `>` narrow or widen it, `?` command help |
| `Ctrl g` | lock: every key goes to the pane until `Ctrl g` again |
| `Ctrl a` | the prefix: one command key, as in tmux — every `^a` command still works, and `^a [` opens scroll mode; `^a w` goes to the next agent waiting on you, `^a b` folds the sidebar, `^a ?` opens command help |

## The footer

The last row says what is going on, left to right:

- `Connected` when Dispatch is attached to a daemon, or, when a machine is down,
  `laptop unreachable — its agents are still running`. Standalone never says
  `Connected`.
- A message, when there is one. A message that reports success goes after five
  seconds. An error stays until another message replaces it, or you click it.
- `3 working` for the panes that are running.
- `2 need attention` for the panes waiting on you; click it to pick one.
- `1 delegation waiting` for a request from an agent; click it to decide.
- `[ Activity ]`, then `[ Settings ]` at the right edge. Both are buttons; Activity lists every pane with what it is doing, and Enter goes to the one you choose.

A message stands in for `Connected` and the working count. When the row is too
narrow, parts go in this order: the working count, `Connected`, `[ Activity ]`,
and then the message is cut with `…`. What needs you, and `[ Settings ]`, are
kept while they fit.

In a mode the footer shows the mode's name (`PANE`, `TAB`, `SESSION`) and a
`[ Done ]` button that leaves it. Scroll mode shows `Scrollback · 120 lines above
live` and `[ Return to live ]`. The prefix shows `PREFIX`. Locked shows
`LOCKED · Ctrl g unlocks`, with no button: only the key leaves it.

Some keys work without a mode: `Alt n` opens a new pane on this tab,
`Alt a` goes to the next agent waiting on you, in any project,
`Alt s` folds the sidebar away and back,
`Alt /` opens command help: every command with the keys that reach it. Type
to search, `Enter` runs the one highlighted and `Esc` closes it.
`Alt i` / `Alt o` move the tab, and `Alt` with an arrow or `h` `j` `k` `l`
moves focus, going on to the next tab at the grid's edge. As in zellij, a
quick `Esc` followed by a letter (as in vim) can reach Dispatch as `Alt` and
that letter.

A project with nothing open shows how to start: the keys for a new agent, for a
shell and for command help, as you have them bound.

Four of these keys are newly taken from panes: `Ctrl p` is a shell's
previous-history key, `Ctrl s` is XON/XOFF flow control's stop, `Ctrl o` is
bash's operate-and-get-next, and `Ctrl g` is emacs's cancel. `Ctrl t` was
already taken, by fzf's file finder and Claude Code's task list. A mode's
key pressed twice goes to the pane instead (`Ctrl s Ctrl s` gives a shell
its `Ctrl s`), lock mode gives a program every key until you unlock, and
`"none"` in `[keys]` gives a key back for good. `Ctrl q` does not quit on
its own — standalone, quitting ends every agent — so quit is `Ctrl o q` or
`^a q`.

To change a key, say only what differs in `config.toml`:

```toml
[keys.normal]
"Ctrl q" = "quit"        # bind something new
"Ctrl a" = "none"        # give Ctrl a back to the shell…
"Ctrl b" = "prefix"      # …and use tmux's key for the prefix

[keys.pane]
"w" = "close_pane"       # a second key for a command
"x" = "none"             # unbind a default
```

The tables are `normal`, `prefix`, `pane`, `tab`, `scroll`, `session` and
`lock`, and a key is written as zellij writes one: `"Ctrl t"`, `"Alt n"`,
`"x"`, `"H"`, `"Shift Tab"`, `"PageUp"`, `"F5"`. `clear = true` in a table
drops that mode's defaults. A command is named in snake_case — `new_pane`,
`close_pane`, `zoom`, `focus_left`, `focus_next`, `new_tab`, `rename_tab`,
`next_tab`, `go_to_tab_1`, `scroll_half_down`, `scroll_top`, `project_picker`,
`next_attention`, `attention_picker`, `activity`, `settings`, `toggle_sidebar`,
`sidebar_narrower`, `sidebar_wider`, `help`, `quit`, `pane_mode`, `lock`, `prefix`,
and so on. A mistake is logged by name and skipped, and the rest still applies.
`Esc` always leaves a mode, lock always has a key that unlocks, and a config
that leaves no key to quit is logged. Keys are read when Dispatch starts.

`Ctrl z` in an agent's pane closes that pane and ends the agent, as `Ctrl p x`
would. An agent cannot be suspended in a pane -- there is no shell behind it to
bring it back with `fg` -- so the key never reaches it. In a shell pane,
`Ctrl z` reaches the shell as usual: it suspends the job the shell is running,
and the shell and the pane carry on.

## Settings

`[ Settings ]` at the right of the footer opens Settings, and so does `Alt ,`
(or `,` in session mode and after the prefix). It is a dialog over the grid:
nothing you type reaches a pane while it is open.

It has three categories, listed down the left (or as a `‹ Appearance ›`
switcher in a window under 72 columns wide):

- **Mouse & layout**: focus follows pointer, hover claims shared panes, the
  sidebar's width, showing the sidebar, and a reset of its width.
- **Appearance**: theme (follow the terminal, dark or light), accent (Theme
  default, a preset or your own `#rrggbb`), motion, and icons (Nerd Font or
  plain). Theme default is the terminal's own accent while following the
  terminal, and the preset's under Dark or Light. Dark and Light paint their
  own background behind the sidebar, the tab row, the footer, dialogs, menus
  and Settings, so they read the same on any terminal; what runs in a pane
  keeps the terminal's colours.
- **Advanced**: where the configuration file, preferences, `ui.toml`, harnesses
  and log file are, the version, and whether Dispatch is standalone or attached.
  It is read-only.

Each row shows its value between `‹` and `›` and where the value comes from
(`Built-in`, `config.toml` or `Settings`). A value set in Settings reads
`Settings ↺`: `Backspace` (or `Delete`), or a click on it, resets it to
default, so it follows `config.toml` and the built-in again, and Apply removes
it from `preferences.toml`. Stepping a value back to what it would inherit
counts as no change. `Tab` and `Shift Tab` move between the search box, the
categories, the rows and the buttons; the arrow keys move and change values,
`Enter` toggles or activates, and `Esc` closes the innermost thing first.
Everything is clickable too, and the wheel scrolls without changing a value.

Appearance is previewed in this window as you edit it. Nothing is written until
you press `[ Apply ]`, which writes only the section you changed to
`preferences.toml`. `[ Discard ]` puts back what was last applied. Moving to
another category, or closing, with changes pending asks whether to Apply,
Discard or Keep editing. The footer counts the changes, as in
`2 changes in Appearance`. If `preferences.toml` was changed by something else
since Settings opened, Apply writes nothing and marks the fields concerned
`changed elsewhere`. Your edit stays pending: apply again to replace the newer
value with yours, or discard to take the newer one.

The sidebar rows are the exception: they act at once and are saved to
`ui.toml`, as dragging the edge and `Alt s` do, and they are never pending. In
a narrow window `Show sidebar` opens or closes the drawer, like the key.

Typing in the search box filters every category by label, description and
category name; `Enter` or a click on a result goes to it in its own category.
Settings reopens on the row you left it on. In a window under 40 columns by 10
rows it shows only a request to make the window larger; there, `Esc` or `[×]`
closes it and discards any changes not yet applied.

## The sidebar

The project list is framed on the left. It is a tree: each project carries a
twisty, and so does any pane running subagents. Clicking a project's row moves
the view to it, and clicking its chevron folds its panes away; clicking a
pane's twisty folds its subagents, and clicking anywhere else on a pane's row
focuses it. `^a f` folds from the keyboard, for a terminal with no mouse
reporting: the focused pane's subagents, or the project above it when that pane
has none. The project the grid is showing is highlighted across the full width
of the row.

`Alt s` folds the sidebar away and back, giving the panes its columns.
`Ctrl o <` and `Ctrl o >` make it narrower or wider by two columns, and so does
dragging its right edge with the mouse, and double-clicking that edge restores
its width; it stays between 20 and 60 columns.
How you left it is kept in `ui.toml`. Under 80 columns the sidebar takes none:
the panes have the whole window, and `Alt s` opens the sidebar as a drawer over
them. Picking a pane or project, `Esc`, or a click beside the drawer puts it
away, and so does widening the window.

A row is marked on both sides. On the left, a project shows a folder -- open
while you are looking inside it, shut while its panes are folded away or it has
none -- with a git mark beside it when its root is a repository. A pane shows
the icon of the harness running in it -- the `icon` key in that harness's TOML, so a harness you
register yourself can have one too. On the right, one glyph says what the pane
is doing, read off its terminal as it runs:

| Glyph | The pane is |
|---|---|
| a spinner | working: output is arriving, or its rules say it is busy |
| a faded pause | idle: waiting for you to give it something |
| a yellow warning | blocked: waiting on a decision only you can make, such as a permission prompt |
| an accent check-circle | done: it finished while you were looking elsewhere |
| an hourglass | starting |
| a faded check, a red cross | exited cleanly, exited badly |
| a faded ban | closed, and still listed for the sake of a subagent under it |

A pane is marked done when it goes from working to idle, or rings the bell,
while another pane has the focus; its row pulses, as does one that turns
blocked out of sight. The mark stays until you look: focusing the pane clears
it. Nothing is marked in a pane's first three seconds, so reattaching to a
daemon, which replays every pane's recent output, does not bring them all
back done. A subagent reads as working from the moment it starts until it
exits, unless it is blocked: its one-shot task prints little before its
answer, and quiet is not finished.

A folded project's row carries the most urgent state among its panes --
blocked, then done, then working -- and each tab is prefixed the same way, so
a pane that needs you shows from anywhere. The footer counts the blocked
panes too: `2 need attention`.

These glyphs are Nerd Font ones by default, which want a patched font in the
terminal. Settings > Appearance > Icons > Plain draws single ASCII characters
instead -- `-` idle, `!` blocked, `*` finished out of sight, `+` and `x`
exited cleanly and badly, `>` working with motion off, `@` a repository and
`/` a folder -- and a harness as the first letter of its name. The spinner is
the same in both.

## Menus

Right-click a pane's row in the sidebar, a pane's top border, a project's row or
a tab, or click the `…` near the right of a pane's top border (on panes at least
12 columns wide), and a short menu opens there. Each item shows the keys that do
the same, and choosing one runs exactly what those keys run. `Up`/`k` and
`Down`/`j` move, `Enter` chooses, `Esc` closes, and so does a click anywhere
outside it; nothing is sent to a pane while it is open. An item that cannot be
chosen now is dimmed and skipped.

| Menu | Items |
|---|---|
| Pane | Zoom (or Restore), Move to previous tab, Move to next tab, Close pane and stop agent (or end shell) |
| Project | New pane here, Fold (or Unfold), Remove from list (only when it has no panes) |
| Tab | Rename, Move left, Move right, Close tab (which still asks) |

Right-clicking a pane's row or a tab changes nothing until an item is chosen.

## Dialogs

Every dialog takes the mouse, and while one is open nothing is sent to a pane:
no click, paste or key. Click a row of a list to select it, and double-click it
(or press its button) to act. Each dialog draws its buttons along its bottom
edge, and a button does exactly what its key does; where Enter acts, the
button it presses is drawn in the accent colour (the delegation request and the
close-tab question have none, since Enter does nothing there). Press a button
and slide off it before letting go to call the press back. A box too small for
its buttons leaves them out, and the keys still work.

| Dialog | Click | Double-click | Buttons |
|---|---|---|---|
| New pane, Project, Add harness, Open on, Waiting on you | select the row | choose it | Open, Cancel |
| Commands (command help) | select the row | run it | Run, Cancel |
| Directory browser | select the entry | open it, as `Enter` | Open, Cancel |
| Harness settings | focus the row; `‹` or `›` step its value | | Open pane, Save as default (`s`), Cancel |
| Delegation request | | | Approve (`a`), Deny (`d`), Always (`A`), Later (`Esc`) |
| Rename tab, Open on a path, Add a machine | | | OK, Cancel |
| Close tab | | | Close, Cancel |

The wheel over a list moves its selection one row a notch, and never scrolls a
pane behind it. A click outside a dialog does nothing; a click outside a menu
closes it.

In lock mode the mouse works exactly as it does in normal mode: locking hands
every key to the pane, and leaves the pointer to Dispatch.

## Keeping projects

The sidebar is the list of projects you keep, not the one directory Dispatch
was started in. Opening Dispatch in a directory adds it to that list, and it is
there on every later start, whichever directory you started in. The list lives
in `projects.toml` beside the rest of the configuration.

Beside it, and beside `machines.toml`, a change leaves an empty
`projects.toml.lock` or `machines.toml.lock`: the lock two Dispatches take in
turn to change the list. It is safe to ignore; delete one only while no
Dispatch is running, or two of them can each take a lock of their own and
write over each other.

`^a o` opens a directory browser: arrows walk it, `→` steps into a directory
and `←` back out, typing filters the listing, and a typed path with a `/` in it
is read as a path instead -- Tab completes it. `^g` lists every git repository
under the current directory, three levels deep, so a directory of checkouts
answers in one keystroke. `Enter` opens what is highlighted, or the path you
typed, as a project.

`^a p` opens the list; `d` on a row drops that project for good. A project with
panes is not dropped -- close them first, or its agents would carry on running
with no row left to reach them by. Attached, the daemon is asked to forget it
too, since it is the daemon that hands a client its projects on every connect.

Nothing scans your disk, and nothing is kept that you did not open.

[Back to the README](../README.md)
