# Pointer foundation: one router, click-to-focus, menus, clickable dialogs

Status: designed, not yet implemented.
Date: 2026-09-30.
Branch: `tui-pointer`, cut from `TUI` at d351503 (stacked; `TUI` is not merged).
Source: sub-project A of
[the mouse and Settings handoff](2026-09-30-mouse-settings-handoff.md), §2 of its
delivery sequence. The rest of the handoff is later sub-projects:

| | Sub-project | Depends on |
|---|---|---|
| **A** | **Pointer foundation (this spec)** | the `TUI` polish branch |
| B | Quiet footer and Settings shell: status footer with Activity and Settings, `preferences.toml`, Settings navigation and search, Appearance | A |
| C | Keyboard editor | B |
| D | Agent defaults, launch profiles, Machines and Advanced pages | B |
| E | Tab drag, weighted pane-divider resize, local text selection and clipboard | A |

## Decisions taken with the user

- The handoff is split into A–E, built in that order, each with its own spec,
  plan and review.
- The work stacks on `TUI`: this branch starts from its head.
- The design below was approved in conversation before this spec was written.

## What exists today

- **Focus follows the pointer.** `InputRouter::handle_mouse`
  (`crates/dispatch-tui/src/input.rs`) turns `MouseEventKind::Moved` over a
  pane into `Action::FocusPane`, except in scroll mode.
- **A project click folds.** In `App::act_on` (`dispatch/src/app.rs`),
  `sidebar::Hit::Project` both selects the project and toggles its fold.
- **Borders do nothing.** A pane's border is outside `App::layout` (the
  interiors), so a click on it resolves to nothing.
- **No menus.** Right-click on the sidebar or a tab acts as a left click.
  Inside a pane it goes to the child.
- **Scattered hit tests.** `act_on` checks targets in separate blocks, in this
  order: drawer, tab row (`tab_hits`), sidebar edge drag, sidebar rows
  (`sidebar::hit_test`), sidebar wheel (`sidebar::section_at`), then the
  router over `layout`.
- **Dialogs are keyboard-only.** `handle_overlay` ignores mouse events.
- **Lock is keys-only.** `InputRouter::leave_mode` never leaves lock, and the
  mouse works in lock as in normal mode. The docs say lock "gives a program
  every key", and nothing about the mouse.
- **No focus reports.** `dispatch/src/terminal.rs` enables mouse capture and
  bracketed paste, but not focus-change reporting. `Event::FocusLost` never
  arrives.

## 1. The pointer router

A new module, `dispatch/src/pointer.rs`, owns pointer resolution. `act_on`
hands it every `Event::Mouse` and acts on the result. Keys and pastes are
unchanged.

**Hit map.** Each `draw` builds a `HitMap`: a list of `(Rect, Target)` in the
order drawn, so the last entry covering a cell is the one on top. Rectangles
are the clipped ones actually drawn. `Target`:

```rust
pub enum Target {
    SidebarChevron(Anchor),      // a project's or pane's twisty
    SidebarDevice(DeviceId),     // a machine's header line
    SidebarProject(ProjectId),   // the rest of a project row
    SidebarPane(PaneId),         // the rest of a pane row
    SidebarEdge,                 // the docked sidebar's right border column
    Tab(TabHit),                 // a chip, `+`, or a scroll mark (existing TabHit)
    PaneHeader(PaneId),          // a tile's top border, except its `…`
    PaneMenu(PaneId),            // the `…` drawn at a tile's top-right
    PaneContent(PaneId),         // a tile's interior
    MenuItem(usize),             // an enabled row of the open menu
    MenuArea,                    // the rest of the open menu's box
    DialogRow(usize),            // a row of the open dialog's list
    DialogButton(Button),        // a button of the open dialog
    DialogArea,                  // the rest of the open dialog's box
}
```

`Button` names every dialog button in §6. The sidebar is one region in the
map (`Target::Sidebar`). A press inside it is resolved further by the existing
`sidebar::hit_test` and `section_at`, which walk `sidebar::sections` — the same
walk that draws the rows — and which gain the chevron/rest split for project
rows. Dialog and menu entries come from each widget's own `layout(area)`, the
function its `render` also uses, so what is clickable is what is drawn.

**Generation.** The map records which overlay was open when it was drawn: its
kind, or none. When an event arrives and the open overlay's kind differs from
the map's, only `Dialog*` and `Menu*` targets of the current overlay count.
Anything else resolves to nothing until the next frame. A stale rectangle can
therefore never activate a control after an overlay opens or closes.

**Resolution order.** The router resolves each event in this order:

1. **Captured gesture**: the owner of a press still held. See §2.
2. **Open menu or dialog.** Inside it, its targets. Outside it:
   - a menu closes, and the event goes nowhere else;
   - a dialog ignores the event.
3. **Dispatch's controls**: the sidebar, drawer, tab row, headers and `…`.
4. **Terminal content**: a tile's interior.

The drawer's existing rules move into step 3 unchanged: a click beside it
closes it, and its blank space is its own. The router returns a small
`Pointer` value that `act_on` acts on: an `Action`, a gesture change, or
nothing. Widgets and the router never write files, spawn panes or send PTY
bytes themselves.

## 2. Gestures

**Capture.** A press (`Down`) on a target starts a gesture owned by that
target, recorded as `Gesture { owner, button, at, moved }`. Until the matching
`Up`, every `Drag` and the `Up` go to the owner, whatever is under the
pointer.

- **A pane.** Its drags and release reach the child, with coordinates relative
  to that pane's interior, clamped to its edge as `send_mouse` does today. They
  do so even when the pointer is over another pane or the sidebar.
- **The sidebar edge.** The existing drag moves here. `dragging_sidebar` goes
  away and becomes this gesture.
- **A control** (sidebar row, chip, header, `…`, menu item, dialog row or
  button). It activates on `Up` inside the target it was pressed on. A
  release anywhere else cancels, and nothing happens.

**Ending a gesture early.** A gesture ends when its owner is lost:

- the owning pane closes or leaves the grid;
- a dialog or menu opens;
- the window loses focus (`Event::FocusLost`);
- the window is resized.

When the owner is a pane that can still be reached, it is first sent a release
of the held button at the last position, so a child never keeps a stuck press.
Focus-change reporting is enabled in `terminal.rs` (`EnableFocusChange`, and
disabled on exit). A terminal that never reports focus loses only that one
trigger.

**Double-click.** A second `Down` of the left button on the same target within
400 ms (`DOUBLE_CLICK`) of the first is a double-click. The router keeps the
last press's target and time.

**Buttons.** Only the left button activates Dispatch controls. The right
button opens a menu where §5 gives one, and does nothing elsewhere. The middle
button is only ever forwarded to a child. **A right-click never performs a
left-click action.**

**Modes.** A press still ends any modal key mode except lock, as
`InputRouter::leave_mode` does now. In lock, the mouse works exactly as in
normal mode. The docs gain one sentence saying so.

## 3. Click-to-focus

- **Moving the pointer no longer changes focus.** `Moved` over a pane does
  nothing unless `focus_follows_pointer` is on.
- **A press in an unfocused pane's interior focuses it.** It goes through
  `App::focus_pane`, so the glide and border ease run as for any focus change.
  The press is also forwarded to the child only when the child tracks the
  mouse: the encoder produced bytes for it. A child that does not track the
  mouse gets nothing, and so a first click is never mistaken for input.
- **A press in the focused pane's interior** goes to the child as today, and
  the wheel keeps its existing behaviour.
- **A press on a header focuses without child input.**
  - A double-click on a header toggles zoom (`state.toggle_zoom()`, after
    focusing it).
  - A header is a tile's top border row, not counting the `…`.
- **`[interface] focus_follows_pointer = false`** is new in `config.toml`.
  - Set to true, it brings back the old behaviour: hover focuses, except in
    scroll mode.
  - It is read-only config for now. Sub-project B moves it into Settings and
    `preferences.toml`.
  - It is separate from `hover_claims_panes`, which still decides only whether
    hover counts as using the window.
- **Hover highlights**, drawn in the chrome's selection style (`Chrome`), apply
  to menu items and dialog rows only. They never move focus or selection.

## 4. The sidebar

| Target | Left click | Right click |
|---|---|---|
| Project chevron | fold or unfold the project | project menu |
| Rest of a project row | select the project; **no fold** | project menu |
| Pane chevron | fold or unfold its subagents (as now) | pane menu |
| Rest of a pane row | focus the pane (as now) | pane menu |
| Machine header | fold or unfold the machine (as now) | nothing |
| Docked sidebar edge | drag to resize (as now) | nothing |
| Double-click on the edge | width back to `DEFAULT_SIDEBAR` (34), saved | |

A project row's chevron is its first two columns: the twisty glyph and the
blank after it, matching the pane twisty's hit width today. `docs/usage.md`'s
sidebar paragraph changes to match. `Fold` (`^a f`) stays the keyboard way to
fold.

## 5. Menus

**Widget.** A new `crates/dispatch-tui/src/menu.rs`:

```rust
pub struct MenuItem {
    pub label: String,
    pub keys: Option<String>,   // the key path, shown dimmed on the right
    pub action: MenuAction,     // what choosing it does; opaque to the widget
    pub enabled: bool,
}
pub struct Menu {
    items: Vec<MenuItem>,
    selected: usize,
    hovered: Option<usize>,
    chrome: Chrome,
}
```

- **Placement.** Drawn at an anchor cell: the pointer, or under a `…`. It is
  flipped or shifted to stay inside the window, and it is at most the window's
  size.
- **Drawing.** Items are in `chrome.text`, the selected or hovered item in
  `chrome.selection`, key paths in `chrome.secondary`, and disabled items in
  `chrome.secondary` with no highlight. The frame is in `chrome.border`.
- **Keys.** `↑`/`↓` skip disabled items, Enter chooses, and Esc closes.
- **Mouse.** An item activates on release inside it (§2).

**Overlay.** `Overlay::Menu { menu, for_: MenuTarget }`. A menu is modal like
any overlay: keys go to it and never to a child. The generic Enter path does
not handle it; it has its own key handling, as help does.

**Actions.** `MenuAction` is an enum in `app.rs`. Most variants wrap the
existing `Action` (`Run(Action)`), and a few carry a target (`ClosePane(PaneId)`,
`FoldProject(ProjectId)`, …) for menus opened on something other than the
focus. Choosing an item closes the menu, then acts. Before acting, a pane or
project action focuses or selects its target, so the same code path that its
key uses runs.

| Menu | Opened by | Items, in order |
|---|---|---|
| **Pane** | right-click on its row or header; left-click on its `…` | Zoom / Restore (label by `zoomed_pane`); Move to previous tab; Move to next tab; Close pane and stop agent. For a `shell` harness the last item is "Close pane and end shell". |
| **Project** | right-click on its row | New pane here; Fold / Unfold; Remove from list (the existing drop, which already refuses while the project has panes; the item is disabled then) |
| **Tab** | right-click on a chip | Rename; Move left; Move right; Close tab (opens the existing yes/no prompt, as `CloseTab` does) |

Key paths come from `keymap.path_to` for the command each item mirrors.
Move-to-tab items are disabled where the move is impossible: first tab, or last
tab with nothing to create. No confirmation is added to any item beyond what
its command already has.

**The `…`.** Each tile's top border gets a `…` two cells from its right corner,
when the tile is at least 12 columns wide. Task 5's status words, when they
fit, sit to the left of it, and its width counts in their fit check.

## 6. Clickable dialogs

A dialog consumes every pointer event inside it and ignores those outside.
Nothing reaches a child while a dialog is open: no clicks, pastes or keys.

| Dialog | Click | Double-click | Buttons |
|---|---|---|---|
| Pickers (Harness, Project, Register, Machine, Attention, Help) | select the row | choose it | **[Open]** (Help: **[Run]**), **[Cancel]** |
| Browser | select the entry | enter it (as Enter) | **[Open]**, **[Cancel]** |
| Settings form | focus the row; ‹ or › step its value as ←/→ | | **[Open pane]**, **[Save as default]** (s), **[Cancel]** |
| Approval | | | **[Approve]** (a), **[Deny]** (d), **[Always]** (A), **[Later]** (Esc) |
| Rename tab / Open on / Add machine prompts | | | **[OK]**, **[Cancel]** |
| Close tab | | | **[Close]**, **[Cancel]** |

Buttons are drawn in a row inside the box's bottom edge, above the existing
hint:

- the default button in `chrome.accent`;
- the others in `chrome.text`;
- a pressed-and-held button in `chrome.selection`.

Each button is a padded label, so a click anywhere on it counts, including
the surrounding spaces. A button does exactly what its key does, through the
same function. The wheel over a dialog's list moves the selection, one row
per notch, and never scrolls anything behind it.

A box too small for its buttons drops them and keeps the keys. Buttons are
never drawn over the list.

## 7. What this does not change

- The footer, its reminders and the help chip. Sub-project B replaces them.
- Text selection, tab dragging and pane-divider resizing. Those are
  sub-project E.
- Pane rename, a "select text" item and profile choice in New pane. They
  do not exist yet, and menus offer nothing that does not.
- The daemon and protocol. Everything here is client-side.
- Existing keys. Every mouse action has a key path already, or the menu names
  one.

## 8. Tests

- **Routing, as event sequences** (`dispatch/src/pointer.rs` and app tests):
  - Press in pane A, drag over pane B and the sidebar, release over B: A gets
    press, motion and release, and B gets nothing.
  - The same while a dialog opens mid-drag, the pane closes, `FocusLost`
    arrives, or a resize: the gesture ends, and a reachable A gets its
    release.
  - A right-click on a project row, pane row or chip opens the menu and
    selects, folds and switches nothing.
  - A click outside an open menu closes it and does not activate the target
    beneath.
  - A click, paste or key with a dialog open never reaches a child's PTY.
  - Press on a control, move off, release: nothing activates.
  - Double-click on a header zooms. Double-click on the sidebar edge restores
    34 and saves it.
  - Click-to-focus: hover does not focus. A first click focuses, and reaches
    the child only if it tracks the mouse. `focus_follows_pointer = true`
    brings hover focus back.
  - A stale hit map: an overlay opened after the last draw makes clicks on
    old targets do nothing.
- **Drawing** (buffer tests): the hit map's rectangles match what is drawn
  for the sidebar, tab row, headers and `…`, menus and dialog buttons. Menus
  stay inside the window at every corner. Dialog buttons disappear rather
  than overlap in a small box.
- **Each dialog's buttons** do what its keys do: one test per row of §6's
  table.
- **Menus**: item order and labels, disabled items skipped by arrows and
  unclickable, key paths shown as bound.

## 9. Order of work

1. `HitMap`, `Target`, the generation check and the resolution order, with
   today's behaviour moved onto them unchanged: drawer, tabs, sidebar, edge
   drag, panes.
2. Gestures: capture, release routing, early end (with `EnableFocusChange`),
   double-click, and the right-click rule.
3. Click-to-focus, header focus, header double-click zoom, and
   `focus_follows_pointer`.
4. Sidebar: project select without fold, chevron fold, edge double-click
   reset.
5. The `Menu` widget, `Overlay::Menu`, the three menus, and the `…`.
6. Clickable dialogs: rows, double-click, buttons and wheel for every dialog in
   §6.
7. Docs: `docs/usage.md` for the sidebar and mouse, and `docs/configuration.md`
   for `focus_follows_pointer`.

Each step leaves `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace --no-fail-fast` green.
