# TUI polish: continuity, attention, room, consistency, discoverability

Status: designed, not yet implemented.
Date: 2026-09-30.
Branch: `TUI`, cut from `main` at 0d4f9b6.

Seven changes to the interface, from a review the user brought. Each one names
the code it starts from, what changes, and how it is tested. They share two
new pieces, the dialog chrome (§4) and the status text (§7), so the order of
work in §9 builds those first.

## Decisions taken with the user

- **New commands get direct Alt keys and mode keys.** `Alt a` next attention,
  `Alt /` command help, `Alt s` toggle sidebar. The same commands are bound
  in session mode and in the prefix. `a` is already Approvals in both, so the
  mode keys are `w` (waiting), `?` (help), `b` (sidebar), and `<` / `>` resize
  the sidebar in session mode. None of `Alt a`, `Alt s`, `Alt /`, or session
  and prefix `w`, `?`, `b`, `<`, `>` is bound today.
- **A blocked agent's reason comes from its status rule.** Rules get an
  optional `reason`. The built-in blocked rules fill it in. A rule without one
  reads "Needs approval".
- **The sidebar is resizable and its width is remembered.** Width and the
  collapsed state are saved to a new `ui.toml` next to `projects.toml`, never
  into `config.toml`.

## 1. Glide continuity

Starts from `App::notice_focus` (`dispatch/src/app.rs`) and
`sidebar::Glide` / `Sidebar::render_glide`
(`crates/dispatch-tui/src/sidebar.rs`).

**Today.** `Glide.from` is an `Anchor`, a row. When focus moves while a glide
is still running, the new glide starts from `last_anchor`: the row the old
glide was heading to, not where the tint is on screen. Press `↓` three times
fast and the tint jumps ahead to each target before it moves.

**Change.** The start becomes an enum:

```rust
pub enum Origin {
    /// At rest on a row.
    Row(Anchor),
    /// Part-way between two rows, frozen when the glide it came from was
    /// replaced.
    Between { from: Anchor, to: Anchor, t: f32 },
}

pub struct Glide {
    pub from: Origin,
    pub t: f32,
}
```

`App` keeps `glide_from: Option<Origin>` and the target the running glide
heads to. When the anchor changes:

- no glide running: `from = Origin::Row(last_anchor)`;
- a glide running: `from = Origin::Between { from: <its start row>, to: <its
  target>, t: <its eased value now> }`. When the running glide itself started
  from a `Between`, its start is first resolved to the nearer of its two rows
  for `t < 0.5` or the farther for `t >= 0.5`. That keeps the enum one level
  deep. The error is less than one row, and only while three moves overlap
  inside 150 ms.

`render_glide` resolves an `Origin` to a screen row every frame: `Row` is the
row's `y`; `Between` is `from_y + (to_y - from_y) * t`. Resolving it each
frame keeps it right when the sidebar scrolls under it. When either row of a
`Between` is not in the same section's view, the glide falls back to what it
does today: fade out of one row, fade into the other.

The ease stays `EASE` (150 ms). With motion off, no glide starts, as now.

**Tests.** In `dispatch/src/app/tests.rs` with the fake clock: move focus
three times, 40 ms apart, and assert that the first frame of each new glide
paints the tint on the row the previous frame painted it on (±1 row), and
that the final frame paints the target row. A unit test in sidebar tests
resolves `Between` against a scrolled section.

## 2. Next attention

Starts from the status row in `App::draw_status` (`dispatch/src/app.rs`),
which counts blocked panes into "N waiting on you".

**Commands.** Two new `keymap::Command`s:

| Command | Name in `[keys]` | Label | Default keys |
|---|---|---|---|
| `NextAttention` | `next_attention` | `attention` | `Alt a`; prefix `w` |
| `AttentionPicker` | `attention_picker` | `waiting` | session `w` |

`NextAttention` focuses the next blocked pane after the focused one, in
sidebar order (projects in their order, panes depth-first), wrapping at the
end. Getting there switches tab, unfolds the project and expands a folded
parent when needed, the same way selecting the row in the sidebar does. With
nothing blocked, it sets the status to "nothing waiting on you".

`AttentionPicker` opens `Overlay::Attention(Picker)`, one row per blocked
pane: label `"<project> · <title>"`, detail `"<reason> · <waiting>"`. The
waiting time is `12s`, `3m` or `1h`, from when the pane turned blocked. Enter
focuses the pane as `NextAttention` would, and Esc closes. With nothing
blocked it does not open, and sets the same status as above.

**Reason.** `RuleDef` gets `reason: Option<String>`. Compiled `Rule` keeps
it. `StatusRules::evaluate` returns `Option<(RuleState, Option<&str>)>`, or a
small `Match { state, reason }`. The tracker (`activity::Tracker`) keeps the
reason of the last blocked verdict, and `App` reads it from the pane's
tracker. The built-in blocked rules (`crates/dispatch-config/src/status/
builtin.rs`) get reasons from the comments already above them: "Permission
prompt", "Choice form", "Question" and similar, one per rule. A blocked
verdict with no reason reads "Needs approval". Unknown keys are still
reported as now, so a harness file with `reason` read by an older build only
logs it.

**Persistent indicator.** The pulse stays as it is. After it ends:

- The status row's reminder names the key: `"2 waiting on you — Alt a"`, from
  `keymap.path_to(Command::NextAttention)`, and left bare when nothing reaches
  it. It is shown while an overlay is open too; today it hides then. The
  lock row already carries it, and keeps it.
- Tab chips need nothing new: `Rollup` already puts the `BLOCKED` glyph on a
  chip whose tab holds a blocked pane, and on a folded project's row.

**Tests.** Cycling order across two projects and a folded one; the jump
changes tab; picker rows and their order; reason from a rule and the
fallback; the reminder with the default key, with `next_attention` unbound,
and with an overlay open.

## 3. More room in small windows

Starts from the layout at the top of `App::draw` (`dispatch/src/app.rs`),
which always takes `sidebar::WIDTH` (34) columns, and from the picker's width
clamp (`crates/dispatch-tui/src/picker.rs`).

**Picker panic.** `clamp(20, area.width)` panics when `area.width` is below
20, because the minimum is then above the maximum. `Picker::render` returns
early only below 4 columns, so widths 4 to 19 panic. The fix is
`clamp(20.min(area.width), area.width)`, as `prompt.rs` and `browser.rs`
already do. The other clamps were checked: the browser's height clamp and the
picker's are guarded by their early returns, and `centred_approval` clamps
before it takes the minimum. Test: render the picker into every width from 1
to 25 and every height from 1 to 6 without a panic.

**Sidebar width.** A new client-side `UiState { sidebar_width: u16,
sidebar_collapsed: bool }` in `dispatch-config`, read and written through
`store::read` / `store::update` as `ui.toml`, beside `projects.toml`. Width
is clamped to 20–60 on read, default 34. It is written when the width or the
collapsed state changes, not every frame. A write failure is logged and
otherwise ignored: losing the width is not worth an error on screen.

Width changes three ways:

- dragging the sidebar's right border with the mouse;
- `<` and `>` in session mode, 2 columns a press, staying in the mode;
- `Alt s` / session `b` / prefix `b`: `ToggleSidebar`, which collapses the
  sidebar to nothing and brings it back at its width.

New commands: `ToggleSidebar` (`toggle_sidebar`, label `sidebar`),
`SidebarNarrower` (`sidebar_narrower`, `narrower`), `SidebarWider`
(`sidebar_wider`, `wider`). The two resize commands stay in the mode.

Collapsed, the name in the top row gives its columns to the tabs, and the
panes take the whole body.

**Narrow windows.** When the window is under 80 columns, the sidebar takes no
columns. `ToggleSidebar` then opens it as a drawer: drawn at its width over
the left of the panes, with a right border in the chrome's border colour.
The sidebar has no keys of its own today, and the drawer adds none: a click
on a row acts as it does in the sidebar and closes the drawer, a click
outside it closes it, and Esc or the toggle key closes it. Every other key
goes where it goes now, with the drawer left open. The collapsed flag
is left alone by the drawer, so widening the window back past 80 columns
brings the sidebar back as it was. `sidebar_area` is empty while the sidebar
takes no columns, so a click on the panes is never read as a sidebar click.

**Dialogs.** `draw_overlay` centres an overlay in the panes area as now. When
the panes area is narrower than the overlay wants, it uses the whole body
instead: the window minus the top and status rows. The width the overlay
wants is a new `desired_width()` on each overlay widget, which is what
`render` already computes before it clamps.

**Tests.** Layout at 120, 80 and 60 columns with the sidebar shown,
collapsed, and as an open drawer; a drag from 34 to 40 columns; `<` at 20
stays 20 and `>` at 60 stays 60; `ui.toml` round trip, and a missing or
unreadable file giving the defaults; a picker wider than the panes area
centred in the body.

## 4. Consistent dialogs

Starts from the picker's hardcoded selection (`fg Black, bg Cyan, bold`) in
`crates/dispatch-tui/src/picker.rs`, and the `Color::Cyan` and
`Color::DarkGray` defaults in `prompt.rs`, `browser.rs` and
`settings_form.rs`.

**Change.** A `Chrome` in `crates/dispatch-tui/src/theme.rs`:

```rust
pub struct Chrome {
    /// A highlighted row: `tint` behind `text`, bold.
    pub selection: Style,
    /// What has the keyboard: a caret, a focused field.
    pub accent: Style,
    /// A dialog's frame and its hint.
    pub border: Style,
    /// Detail after a label, empty-list text, hints inside a dialog.
    pub secondary: Style,
    /// Text drawn on `tint`.
    pub text: Style,
}

impl Theme {
    pub fn chrome(&self) -> Chrome { … }
}
```

`Picker`, `SettingsForm`, `Browser`, `Prompt` and the approval widget each
take a `set_chrome(&Chrome)`. It replaces `set_border`, and
`SettingsForm::set_styles`. `draw_overlay` calls it once, for whichever
overlay is open. Each widget's default is `Theme::fallback().chrome()`, so a
test that draws a widget alone still gets colours. The `Note::Busy` yellow and
`Note::Error` red stay, as do the state glyphs' colours: those colours carry
meaning, not decoration.

**Tests.** For each widget, render with a chrome built from distinctive
colours and assert that no cell uses `Color::Cyan`, `Color::Black` or
`Color::DarkGray`, and that the selected row uses `selection`.

## 5. Discoverability

**Empty project.** When the selected project has no open panes in the grid,
the panes area draws a small card in its centre:

```
  Start an agent    Alt n
  Open a shell      Alt n, then shell
  Command help      Alt /
```

The keys come from `keymap.path_to`, so a rebinding shows. A line whose
command no key reaches is left out. "Open a shell" names the harness picker's
`shell` row, because the shell is a harness. Drawn in `secondary`, with the
keys in `accent`. When a pane is added the card goes, and nothing else changes.

**Command help.** A new `Command::Help` (`help`, label `help`), `Alt /`,
session and prefix `?`. It opens `Overlay::Help`, a picker with a filter line,
one row per `Command::NAMED` entry that has an action: label, then key path as
detail. `GoToTab` is one row, "go to tab 1–9". Commands with no key still
appear, with detail "no key". Typing filters by label, name and key path, case
insensitive. Enter closes the help and runs the command, as its key would.
Backspace edits the filter, and Esc closes.

The filter is added to `Picker` itself (`with_filter()`, off by default), not
a second widget, so the harness and project pickers can use it later.

**Always-visible way in.** The status row always ends with the help chip,
`Alt / help`, from `path_to(Command::Help)`. It is drawn right-aligned after
everything else, and reserved first: a long status message is cut short
before the chip is. In a mode, the chip is left off, because the mode row
already lists its own keys. When nothing reaches `Help`, there is no chip.

**Tests.** The card's lines under the default keymap and with `new_pane`
unbound; help rows and filtering; Enter running `ToggleSidebar` from help; the
chip surviving a 200-character status message at 80 columns.

## 6. Animation work where it is seen

Starts from `App::next_frame` (`dispatch/src/app.rs`), which asks for a
spinner frame whenever any pane anywhere is `Running`, even in a folded
project or scrolled off the sidebar.

**Change.** A pure `sidebar::spins(state, area, scroll) -> bool` walks the
same `sections` the render walks and says whether any row on screen draws a
spinner: a live `Running` pane row, or a folded project whose rollup is
`Working`. `draw_tabs` notes whether a chip it drew carries
`Rollup::Working`. `App` keeps `spinner_drawn: bool`, the two ORed, from the
last `draw`. `next_frame` asks for `SPIN_FRAME` only when it is set. A
running pane nobody can see no longer keeps Dispatch redrawing. Its row starts
spinning on the first frame that draws it, which any change of scroll, fold
or tab already causes.

Tweens keep `TWEEN_FRAME` while any runs, as now: they are short and start
from something the user did.

**Frame timing.** `draw` measures its own duration and keeps the last 256 in a
ring buffer. Every 10 s, if any frame was drawn, it logs
`tracing::debug!(p50, p95, max, frames, "frame timing")` and clears the
buffer. Nothing is shown on screen. The animation rates do not change in this
work: the numbers are there to decide that later.

**Tests.** A running subagent under a folded parent gives
`next_frame() == None`; unfold the parent, draw, and it gives
`Some(SPIN_FRAME)`; a running pane scrolled out of the sidebar's view gives
`None`; a folded project whose pane runs still spins, because its row draws
the rollup; motion off gives `None` in every case. (A folded project is not a
hidden spinner: its row carries the `Working` rollup.)

## 7. Readable states and contrast

**Status text.** One function in `crates/dispatch-tui/src/sidebar.rs` beside
the glyphs:

```rust
pub fn status_text(status: PaneStatus, reason: Option<&str>) -> Cow<'_, str>
```

| Status | Text |
|---|---|
| `Starting` | Starting |
| `Running` | Working |
| `Blocked` | the reason, or "Needs approval" |
| `Idle` | Finished |
| `Exited(0)` | Exited |
| `Exited(n)` | Exited (n) |

It is used in the attention picker's detail (§2) and on the focused pane's
border: the title stays on the left, and the status text goes right-aligned
on the top border in `secondary`. Only the focused pane gets it, so the grid
does not fill with words.

**Contrast.** `Theme::new` today sets `faded` to
`foreground.mix(background, 0.45)`, which on a low-contrast or light theme can
fall below readable. The new rule starts at the same 0.45 mix and steps the
amount down by 0.05 until the WCAG contrast ratio against the background is
at least 4.5:1. When even the plain foreground is below 4.5:1, `faded` is the
foreground. `Role::Faded` in `Theme::rgb` uses the same computed amount, so
animations blend to the colour actually drawn. In 256 colours the chosen RGB
is then mapped as now.

A `contrast(a: Rgb, b: Rgb) -> f32` helper in `theme.rs` computes the ratio
from relative luminance.

**Tests.** `faded` reaches 4.5:1 on the fallback palette, on a light palette
(`#fafafa` / `#383a42`), and on a low-contrast palette (`#2e3440` /
`#81a1c1`); falls back to the foreground on a palette whose foreground is
itself below 4.5:1; and the fallback palette's `faded` is unchanged if it
already met the ratio at 0.45.

## 8. What this does not change

- Animation rates, the ease curves and their durations.
- The daemon and the protocol. Everything here is client-side. `reason` is
  read only by the client, as `Blocked` is.
- `config.toml`. Nothing here writes to it. The new keys are overridable in
  `[keys]` like the others.
- Keys that exist today. Nothing is rebound.

## 9. Order of work

1. §3 picker panic fix, and the same clamp elsewhere.
2. §4 `Chrome`, applied to every overlay.
3. §1 glide continuity.
4. §6 spinner scheduling and frame timing.
5. §7 status text and contrast.
6. §2 reason field, next attention, attention picker, indicator.
7. §3 sidebar resize, collapse, `ui.toml`, drawer, dialogs over the body.
8. §5 empty-project card, command help with the picker filter, status-row
   chip.

Each step leaves the full test suite, `cargo clippy --all-targets` and
`cargo fmt --check` green. `docs/usage.md` and `docs/configuration.md` gain
the new keys, `reason` and `ui.toml` in the step that adds them.
