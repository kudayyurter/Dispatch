# Pointer Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One pointer router for Dispatch, with these parts:
- a hit map built by each frame;
- captured gestures;
- click-to-focus;
- the sidebar's project select separated from fold;
- right-click menus for panes, projects and tabs;
- clickable dialogs.

**Architecture:**
- **Pure pieces:** a new `dispatch/src/pointer.rs` holds the hit map, its targets, double-click detection and the gesture record. `draw` fills a `HitMap` from the same rectangles it draws, and `act_on` sends every mouse event through `App::route_pointer`, which acts on what it resolves.
- **New widgets:** `crates/dispatch-tui` gains a `Menu` widget and a button row. The existing dialogs gain `layout(area)`, which both `render` and the hit map use.

**Tech Stack:** Rust 2024 (MSRV 1.89), ratatui 0.30, crossterm, tracing.

**Spec:** `docs/superpowers/specs/2026-09-30-pointer-foundation-design.md`. Read it before each task. It is the binding authority.

## Global Constraints

- **Branch:** `tui-pointer`, stacked on `TUI`. Stage files by name. Never run `git add -A`.
- **Gate:** every task ends with all of these green:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --no-fail-fast`
- **MSRV 1.89:** no API newer than that.
- **Comments:** match the surrounding code. Full sentences that say *why*, British spelling, no "we".
- **Commits:** end with exactly these two lines, after a blank line:
  `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_0176ZvPVLNdCCP5gh3a8m9m4`
- **Constants:**
  - `DOUBLE_CLICK` = 400 ms.
  - A project row's chevron is its first two columns.
  - The `…` shows on tiles of 12 columns or more, two cells from the right corner.
  - The sidebar edge double-click resets the width to `DEFAULT_SIDEBAR` (34).
- **Buttons:** only the left button activates Dispatch controls. The right button opens a menu where the spec gives one, and does nothing elsewhere. The middle button is only forwarded to a child. **A right-click never performs a left-click action.**
- **Isolation:**
  - Nothing reaches a child's PTY while an overlay (dialog or menu) is open.
  - Widgets never write files, spawn panes or send PTY bytes. They return ids that `App` acts on.
- **No daemon or protocol change.**
- **No new keys.** Every mouse action runs the same function its key does.

## Review Focus

1. **A drag across panes.** A press in pane A, then drag and release over pane B, the sidebar or the tab row. A gets its press, motion and release, and B gets nothing. The test is in Task 3.
2. **A gesture whose owner disappears.** The pane closes, a dialog opens, the window loses focus, or it is resized. The child gets one release and nothing after it. No stuck press, and no panic on a vanished pane. The test is in Task 3.
3. **Click-through.** A click that closes a menu, or lands outside a dialog, activates nothing behind it. A click on a target drawn before an overlay opened does nothing. The test is in Tasks 2 and 7.
4. **A right-click on anything clickable.** It never selects, folds, switches tab or focuses. The test is in Tasks 3 and 7.
5. **Tiny windows with a menu or dialog open.** Every size from 1×1 to 30×10 renders without panics, and the menu stays inside the window. The test is in Tasks 6 and 8.

---

## File map

| File | Change |
|---|---|
| `dispatch/src/pointer.rs` (new; `mod pointer;` in `dispatch/src/main.rs`) | `Target`, `HitMap`, `Clicks`, `Gesture` (T1) |
| `dispatch/src/app.rs` | hit map recording (T2), `route_pointer` (T2–T5, T7, T9), menus (T7), dialog wiring (T9) |
| `dispatch/src/terminal.rs` | `EnableFocusChange` / `DisableFocusChange` (T3) |
| `crates/dispatch-tui/src/input.rs` | hover focus only with `focus_follows_pointer` (T4) |
| `crates/dispatch-config/src/config.rs` | `InterfaceConfig::focus_follows_pointer` (T4) |
| `crates/dispatch-tui/src/sidebar.rs` | `Hit::ProjectChevron` (T5) |
| `crates/dispatch-tui/src/menu.rs` (new) | the `Menu` widget (T6) |
| `crates/dispatch-tui/src/button.rs` (new) | the button row (T8) |
| `crates/dispatch-tui/src/{picker,browser,prompt,settings_form}.rs`, `dispatch/src/approval.rs` | `layout(area)`, buttons, hover (T8) |
| `docs/usage.md`, `docs/configuration.md` | mouse, sidebar, menus, `focus_follows_pointer` (T4, T5, T7, T9) |

Test helpers already in `dispatch/src/app.rs` `mod tests`: `attached_app`, `spawn_several`, `spawned`, `press`, `press_alt`, `click`, `rebind`, `drawn`, `a_wide_terminal`, `rendered_text`, `bottom_row`, `top_row`, `hand_clock`, `advance`, `send_tabs`, `sidebar_column`, `middle_of`. New panes animate open, so call `hand_clock` + `advance(&clock, Duration::from_secs(1))` before asserting on borders. Read each helper before relying on it.

---

### Task 1: The pointer module's pure pieces

**Files:**
- Create: `dispatch/src/pointer.rs` (with inline `#[cfg(test)] mod tests`)
- Modify: `dispatch/src/main.rs` (declare `mod pointer;` beside the other modules)

**Interfaces:**
- Produces:
  - `pointer::Target` (Copy, Eq, Debug);
  - `pointer::HitMap::{new(overlay: Option<&'static str>) -> Self, push(&mut self, Rect, Target), at(&self, x, y) -> Option<Target>, resolve(&self, x, y, overlay_now: Option<&'static str>) -> Option<Target>, overlay(&self) -> Option<&'static str>}`;
  - `pointer::Clicks::{default(), press(&mut self, Target, Instant) -> bool}`;
  - `pointer::Gesture { owner: Target, button: MouseButton, last: (u16, u16) }`;
  - `pointer::DOUBLE_CLICK: Duration`;
  - `pointer::DialogHit`, `pointer::MenuHit`.

- [ ] **Step 1: Write the module with its failing tests.** `Target` names what a cell belongs to. The sidebar is one region: a press inside it is resolved further by `sidebar::hit_test`. Dialog and menu hits carry indices and button ids that the open overlay understands.

```rust
//! What the pointer is over, and what a press started.
//!
//! Each frame records where it drew every clickable thing, in the order
//! drawn, so a pointer event is resolved against exactly what is on screen:
//! the last rectangle covering a cell is the one on top.

use std::time::{Duration, Instant};

use crossterm::event::MouseButton;
use dispatch_core::PaneId;
use dispatch_tui::button::ButtonId;
use ratatui::layout::Rect;

use crate::tabs::TabHit;

/// Two presses on one target this close together are a double-click.
pub const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// A row or button of the open dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogHit {
    /// A row of its list, by its index among the rows shown.
    Row(usize),
    /// One of its buttons.
    Button(ButtonId),
    /// A value's `‹` or `›` in the settings form's row: `false` for back.
    Step(usize, bool),
    /// Anywhere else inside its box.
    Area,
}

/// An item of the open menu, or its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuHit {
    /// An enabled item, by index.
    Item(usize),
    /// Anywhere else inside its box.
    Area,
}

/// What a cell of the frame belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The sidebar, docked or as the drawer; `sidebar::hit_test` says which
    /// row.
    Sidebar,
    /// The docked sidebar's right border, which drags to resize.
    SidebarEdge,
    /// A stretch of the tab row.
    Tab(TabHit),
    /// A tile's top border, but its `…`.
    PaneHeader(PaneId),
    /// The `…` at a tile's top right.
    PaneMenu(PaneId),
    /// A tile's interior: the child's.
    PaneContent(PaneId),
    /// The open menu.
    Menu(MenuHit),
    /// The open dialog.
    Dialog(DialogHit),
}

impl Target {
    /// Whether it belongs to an overlay rather than the frame beneath one.
    #[must_use]
    pub fn is_overlay(self) -> bool {
        matches!(self, Target::Menu(_) | Target::Dialog(_))
    }
}

/// Every clickable rectangle of one frame, in the order drawn.
#[derive(Debug, Default)]
pub struct HitMap {
    entries: Vec<(Rect, Target)>,
    /// The overlay open when the frame was drawn, by kind.
    overlay: Option<&'static str>,
}

impl HitMap {
    /// An empty map for a frame drawn with `overlay` open.
    #[must_use]
    pub fn new(overlay: Option<&'static str>) -> Self {
        Self {
            entries: Vec::new(),
            overlay,
        }
    }

    /// Records `target` as drawn over `rect`, above everything before it.
    pub fn push(&mut self, rect: Rect, target: Target) {
        if !rect.is_empty() {
            self.entries.push((rect, target));
        }
    }

    /// What is on top at `(x, y)`.
    #[must_use]
    pub fn at(&self, x: u16, y: u16) -> Option<Target> {
        let position = ratatui::layout::Position::new(x, y);
        self.entries
            .iter()
            .rev()
            .find(|(rect, _)| rect.contains(position))
            .map(|(_, target)| *target)
    }

    /// What a press at `(x, y)` may act on, with `overlay_now` open.
    ///
    /// When the overlay has changed since this frame was drawn, only the
    /// overlay's own targets count: a rectangle drawn under an overlay that
    /// has since opened, or over one that has since closed, must not act.
    #[must_use]
    pub fn resolve(&self, x: u16, y: u16, overlay_now: Option<&'static str>) -> Option<Target> {
        let target = self.at(x, y)?;
        if overlay_now != self.overlay {
            return None;
        }
        Some(target)
    }

    /// The overlay open when it was drawn.
    #[must_use]
    pub fn overlay(&self) -> Option<&'static str> {
        self.overlay
    }
}

/// The last press, for telling a double-click.
#[derive(Debug, Default)]
pub struct Clicks {
    last: Option<(Target, Instant)>,
}

impl Clicks {
    /// Records a left press on `target` at `now`, returning whether it is
    /// the second of a double-click. A double-click is not the first of the
    /// next: three presses are one double and one single.
    pub fn press(&mut self, target: Target, now: Instant) -> bool {
        let double = self.last.is_some_and(|(last, at)| {
            last == target && now.saturating_duration_since(at) <= DOUBLE_CLICK
        });
        self.last = if double { None } else { Some((target, now)) };
        double
    }
}

/// A press still held, and who it belongs to until it is let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gesture {
    /// What was pressed.
    pub owner: Target,
    /// Which button holds it.
    pub button: MouseButton,
    /// Where the pointer last was, for a release sent on its behalf.
    pub last: (u16, u16),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane() -> PaneId {
        PaneId::new()
    }

    #[test]
    fn the_last_rectangle_drawn_is_the_one_on_top() {
        let a = pane();
        let mut map = HitMap::new(None);
        map.push(Rect::new(0, 0, 10, 10), Target::PaneContent(a));
        map.push(Rect::new(2, 2, 3, 3), Target::Menu(MenuHit::Item(0)));

        assert_eq!(map.at(3, 3), Some(Target::Menu(MenuHit::Item(0))));
        assert_eq!(map.at(0, 0), Some(Target::PaneContent(a)));
        assert_eq!(map.at(20, 20), None);
    }

    #[test]
    fn an_empty_rectangle_is_never_hit() {
        let mut map = HitMap::new(None);
        map.push(Rect::new(5, 5, 0, 3), Target::SidebarEdge);
        assert_eq!(map.at(5, 5), None);
    }

    #[test]
    fn a_map_drawn_under_another_overlay_resolves_nothing() {
        let a = pane();
        let mut map = HitMap::new(None);
        map.push(Rect::new(0, 0, 10, 10), Target::PaneContent(a));

        assert_eq!(map.resolve(1, 1, None), Some(Target::PaneContent(a)));
        assert_eq!(map.resolve(1, 1, Some("menu")), None, "a menu opened since");

        let mut map = HitMap::new(Some("menu"));
        map.push(Rect::new(0, 0, 10, 10), Target::Menu(MenuHit::Area));
        assert_eq!(map.resolve(1, 1, None), None, "the menu has closed since");
    }

    #[test]
    fn two_quick_presses_on_one_target_are_a_double_click() {
        let a = pane();
        let start = Instant::now();
        let mut clicks = Clicks::default();

        assert!(!clicks.press(Target::PaneHeader(a), start));
        assert!(clicks.press(Target::PaneHeader(a), start + Duration::from_millis(300)));
        assert!(
            !clicks.press(Target::PaneHeader(a), start + Duration::from_millis(350)),
            "a third press starts again"
        );
    }

    #[test]
    fn a_slow_second_press_or_another_target_is_not_a_double_click() {
        let (a, b) = (pane(), pane());
        let start = Instant::now();
        let mut clicks = Clicks::default();

        clicks.press(Target::PaneHeader(a), start);
        assert!(!clicks.press(Target::PaneHeader(a), start + Duration::from_millis(401)));
        assert!(!clicks.press(Target::PaneHeader(b), start + Duration::from_millis(450)));
    }
}
```

`dispatch_tui::button::ButtonId` does not exist until Task 8. In this task, define `DialogHit::Button(ButtonId)` against a placeholder enum at the top of `pointer.rs`:

```rust
/// Stand-in until Task 8 moves the dialog buttons into `dispatch-tui`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonId {}
```

Leave out the `use dispatch_tui::button::ButtonId;` line. Task 8 replaces the stand-in with the real import.

- [ ] **Step 2: Run the tests.** Run `cargo test -p dispatch pointer::`. Expected: 5 passing. Unused-code warnings are allowed only if clippy passes. If `dead_code` fires, add `#[allow(dead_code)]` on the module with a comment saying Task 2 wires it in, and remove it in Task 2.

- [ ] **Step 3: Gate and commit.**

```bash
git add dispatch/src/pointer.rs dispatch/src/main.rs
git commit -m "feat(tui): a hit map, double-click detection and a gesture record for the pointer"
```

---

### Task 2: Each frame records its hit map, and clicks resolve against it

Behaviour does not change in this task. Every existing test must pass unchanged. What changes is that the frame records where it drew things, and `act_on` routes mouse events through one function.

**Files:**
- Modify: `dispatch/src/app.rs`
- Test: `dispatch/src/app.rs` tests

**Interfaces:**
- Consumes: T1's `HitMap`, `Target`.
- Produces:
  - `App.hits: HitMap`;
  - `Overlay::tag(&self) -> &'static str`;
  - `App::overlay_tag(&self) -> Option<&'static str>`;
  - `App::route_pointer(&mut self, mouse: &MouseEvent) -> bool`, which returns whether the event was consumed. When it was not, the router over `layout` handles it as today.

- [ ] **Step 1: Write the failing test.**

```rust
    #[test]
    fn a_frame_records_what_it_drew_where() {
        let (mut app, project, daemon, _sent) = attached_app();
        let clock = hand_clock(&mut app);
        let panes = spawn_several(&mut app, &daemon, project, 2);
        let mut terminal = a_wide_terminal();
        drawn(&mut app, &mut terminal);
        advance(&clock, Duration::from_secs(1));
        drawn(&mut app, &mut terminal);

        let (_, tile) = *app.frames.iter().find(|(id, _)| *id == panes[0]).expect("tiled");
        let inner = App::interior(tile);
        assert_eq!(app.hits.at(inner.x, inner.y), Some(pointer::Target::PaneContent(panes[0])));
        assert_eq!(app.hits.at(tile.x + 1, tile.y), Some(pointer::Target::PaneHeader(panes[0])));
        assert_eq!(app.hits.at(1, 3), Some(pointer::Target::Sidebar));
        let edge = app.sidebar_area.x + app.sidebar_area.width - 1;
        assert_eq!(app.hits.at(edge, 5), Some(pointer::Target::SidebarEdge));
        assert!(matches!(app.hits.at(app.tab_row.x + 1, 0), Some(pointer::Target::Tab(_))));
    }

    #[test]
    fn a_click_on_something_drawn_before_an_overlay_opened_does_nothing() {
        let (mut app, project, daemon, _sent) = attached_app();
        let panes = spawn_several(&mut app, &daemon, project, 2);
        app.rename(panes[0], "alpha-pane");
        let mut terminal = a_wide_terminal();
        app.focus_pane(panes[1]);
        drawn(&mut app, &mut terminal);
        let row = sidebar_row_of(&terminal, "alpha-pane");

        // An overlay opens by key, and no frame is drawn before the click.
        app.state.set_pane_status(panes[0], PaneStatus::Blocked).expect("exists");
        app.open_attention_picker();
        assert!(app.overlay.is_some());
        click(&mut app, 8, row);

        assert_eq!(app.state.focused_pane(), Some(panes[1]), "the row drawn beneath did not act");
        assert!(app.overlay.is_some(), "and the overlay is still open");
    }

    /// The screen row whose sidebar columns contain `name`.
    fn sidebar_row_of(terminal: &ratatui::Terminal<ratatui::backend::TestBackend>, name: &str) -> u16 {
        let screen = rendered_text(terminal);
        let index = screen
            .lines()
            .position(|line| sidebar_column(line).contains(name))
            .unwrap_or_else(|| panic!("{name} is drawn in the sidebar"));
        u16::try_from(index).expect("a screen row")
    }
```

Run `cargo test -p dispatch a_frame_records a_click_on_something_drawn`. Expected: FAIL to compile, because `hits` does not exist.

- [ ] **Step 2: Record the map in `draw`.**
- Add `hits: pointer::HitMap` to `App`, with the doc `/// Where the last frame drew every clickable thing.`, initialised to `pointer::HitMap::default()`.
- Add the `Overlay` tag:

```rust
    /// The overlay's kind, which a frame's hit map is stamped with.
    fn tag(&self) -> &'static str {
        match self {
            Overlay::Harness(_) => "harness",
            Overlay::Settings { .. } => "settings",
            Overlay::Project(_) => "project",
            Overlay::Register(_) => "register",
            Overlay::Browse(_) => "browse",
            Overlay::Machine(_) => "machine",
            Overlay::AddMachine(_) => "add_machine",
            Overlay::OpenOn { .. } => "open_on",
            Overlay::RenameTab { .. } => "rename_tab",
            Overlay::CloseTab { .. } => "close_tab",
            Overlay::Approval { .. } => "approval",
            Overlay::Attention(_) => "attention",
            Overlay::Help(_) => "help",
        }
    }
```

  List every variant that exists in the file, and let the compiler's exhaustiveness check catch any you missed. Add `fn overlay_tag(&self) -> Option<&'static str> { self.overlay.as_ref().map(Overlay::tag) }`.
- In `draw`, right after `self.notice_focus(now);`, start `let mut hits = pointer::HitMap::new(self.overlay_tag());`. After everything is drawn, just before the spin-flag line, set `self.hits = hits;`. Push entries in drawing order:
  1. After the docked sidebar is drawn: `hits.push(sidebar_area, Target::Sidebar)`. If `docked > 0`, also push `SidebarEdge` for the right border column, `Rect::new(sidebar_area.x + sidebar_area.width - 1, sidebar_area.y, 1, sidebar_area.height)`.
  2. After `draw_tabs`: one `Target::Tab(hit)` per entry of `self.tab_hits`, as `Rect::new(x, self.tab_row.y, width, 1)`.
  3. After `draw_panes`: for each `(id, tile)` in `self.frames`, push `PaneHeader(id)` for `Rect::new(tile.x, tile.y, tile.width, 1)`, then `PaneContent(id)` for `Self::interior(tile)`.
  4. The drawer, when open, after it is drawn: `hits.push(sidebar_area, Target::Sidebar)`. Pushed last, it sits above the panes.
  5. Overlays push nothing yet; Tasks 7 and 9 add their entries.

- [ ] **Step 3: Route through `route_pointer`.** Move the mouse blocks of `act_on` into `fn route_pointer(&mut self, mouse: &MouseEvent) -> bool`, in their current order, with behaviour unchanged:
  - drawer-outside-click;
  - tab row;
  - sidebar edge drag;
  - sidebar rows;
  - sidebar wheel;
  - drawer blank space.

  Each block's first condition becomes a lookup in the map. For a `Down`, use `let target = self.hits.resolve(mouse.column, mouse.row, self.overlay_tag());`. For example, the tab-row block becomes:

```rust
        if matches!(mouse.kind, MouseEventKind::Down(_))
            && let Some(pointer::Target::Tab(hit)) = target
        {
            match hit { /* the existing four arms */ }
            return true;
        }
```

  The sidebar-rows and wheel blocks run only when `target == Some(Target::Sidebar)`, then call `sidebar::hit_test` / `section_at` as today. The edge block keys its `Down` on `Some(Target::SidebarEdge)`. Its `Drag`/`Up` arms keep using `dragging_sidebar` until Task 3. The overlay early-return in `act_on` stays where it is, before `route_pointer`. `act_on` becomes:

```rust
        if let Event::Mouse(mouse) = event
            && self.route_pointer(mouse)
        {
            return Ok(());
        }
```

  After this, `tab_hits` is read only to fill the map. Keep the field: `draw_tabs` computes it.

- [ ] **Step 4: Run the tests.** Run `cargo test -p dispatch`. Expected: every existing test passes, plus the two new ones. The routing order is unchanged. If one fails, compare the moved block against the original and fix the move; do not change the test.

- [ ] **Step 5: Gate and commit.**

```bash
git add dispatch/src/app.rs dispatch/src/pointer.rs
git commit -m "refactor(tui): resolve every click against the hit map its frame drew"
```

---

### Task 3: Gestures — capture, release routing, early end, right-click rule

**Files:**
- Modify: `dispatch/src/app.rs`, `dispatch/src/terminal.rs`
- Test: `dispatch/src/app.rs` tests

**Interfaces:**
- Consumes: T1's `Gesture`, `Clicks`; T2's `route_pointer`.
- Produces:
  - `App.gesture: Option<pointer::Gesture>` and `App.clicks: pointer::Clicks`;
  - `App::end_gesture(&mut self)`, which sends a pane owner its release when it can be reached;
  - controls now act on **release inside the pressed target**.

`dragging_sidebar` is removed. The sidebar-edge drag becomes a gesture owned by `SidebarEdge`.

- [ ] **Step 1: Update the `click` test helper, then write the failing tests.** Controls now act on release, so a click is a press and a release. Change `click` to send `Down(Left)` then `Up(Left)` at the same cell. Add `press_at(app, kind, column, row)` for single events.

```rust
    fn mouse_at(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
        let event = Event::Mouse(dispatch_tui::input::MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
        app.handle(&event, Size::new(120, 30)).expect("the pointer is handled");
    }

    /// Turns on SGR mouse reporting in `pane`, as a mouse-aware program does.
    fn track_mouse(app: &mut App, daemon: &Sender<ServerMessage>, pane: PaneId) {
        print(app, daemon, pane, b"\x1b[?1000h\x1b[?1002h\x1b[?1006h");
    }

    #[test]
    fn a_drag_belongs_to_the_pane_it_started_in() {
        let (mut app, project, daemon, sent) = attached_app();
        let clock = hand_clock(&mut app);
        let panes = spawn_several(&mut app, &daemon, project, 2);
        let mut terminal = a_wide_terminal();
        drawn(&mut app, &mut terminal);
        advance(&clock, Duration::from_secs(1));
        drawn(&mut app, &mut terminal);
        track_mouse(&mut app, &daemon, panes[0]);
        track_mouse(&mut app, &daemon, panes[1]);
        app.focus_pane(panes[0]);
        drain(&sent);
        let (ax, ay) = middle_of(&app, panes[0]);
        let (bx, by) = middle_of(&app, panes[1]);

        use crossterm::event::MouseButton::Left;
        mouse_at(&mut app, MouseEventKind::Down(Left), ax, ay);
        mouse_at(&mut app, MouseEventKind::Drag(Left), bx, by);
        mouse_at(&mut app, MouseEventKind::Drag(Left), 2, 5); // over the sidebar
        mouse_at(&mut app, MouseEventKind::Up(Left), bx, by);

        let writes = writes_by_pane(&sent);
        assert_eq!(writes.get(&panes[0]).map(Vec::len), Some(4), "press, two drags, release");
        assert!(!writes.contains_key(&panes[1]), "the other pane got nothing");
    }

    #[test]
    fn a_drag_whose_owner_goes_away_ends_with_one_release() {
        // One case per trigger: a dialog opening, FocusLost, a resize, the pane closing.
        for trigger in ["dialog", "focus", "resize", "close"] {
            let (mut app, project, daemon, sent) = attached_app();
            let clock = hand_clock(&mut app);
            let panes = spawn_several(&mut app, &daemon, project, 2);
            let mut terminal = a_wide_terminal();
            drawn(&mut app, &mut terminal);
            advance(&clock, Duration::from_secs(1));
            drawn(&mut app, &mut terminal);
            track_mouse(&mut app, &daemon, panes[0]);
            app.focus_pane(panes[0]);
            let (ax, ay) = middle_of(&app, panes[0]);
            use crossterm::event::MouseButton::Left;
            mouse_at(&mut app, MouseEventKind::Down(Left), ax, ay);
            drain(&sent);

            match trigger {
                "dialog" => { app.state.set_pane_status(panes[1], PaneStatus::Blocked).expect("exists"); app.open_attention_picker(); app.end_gesture(); }
                "focus" => app.handle(&Event::FocusLost, Size::new(120, 30)).expect("handled"),
                "resize" => app.handle(&Event::Resize(100, 30), Size::new(100, 30)).expect("handled"),
                _ => { app.close_pane(panes[0]); app.end_gesture(); }
            }
            let after = writes_by_pane(&sent).get(&panes[0]).map_or(0, Vec::len);
            let expected = usize::from(trigger != "close");
            assert_eq!(after, expected, "{trigger}: exactly one release when the pane can still be reached");
            assert!(app.gesture.is_none(), "{trigger}: the gesture is over");

            app.overlay = None;
            mouse_at(&mut app, MouseEventKind::Drag(Left), ax, ay);
            assert_eq!(writes_by_pane(&sent).get(&panes[0]).map_or(0, Vec::len), after, "{trigger}: nothing after");
        }
    }

    #[test]
    fn a_control_acts_on_release_inside_it_and_not_when_the_pointer_slides_off() {
        let (mut app, _terminal, first, _parent, _child) = app_with_a_drawn_sidebar();
        let second = app.state.projects()[1].id;
        use crossterm::event::MouseButton::Left;

        mouse_at(&mut app, MouseEventKind::Down(Left), 8, 2);
        mouse_at(&mut app, MouseEventKind::Up(Left), 8, 20);
        assert_eq!(app.state.selected_project(), Some(first), "slid off: nothing happened");

        let row = sidebar_row_of(&_terminal, "second");
        click(&mut app, 8, row);
        assert_eq!(app.state.selected_project(), Some(second));
    }

    #[test]
    fn a_right_click_never_does_what_a_left_click_does() {
        let (mut app, _terminal, first, parent, _child) = app_with_a_drawn_sidebar();
        use crossterm::event::MouseButton::Right;
        let before = (app.state.selected_project(), app.state.focused_pane(), app.state.is_project_collapsed(first));
        for (x, y) in [(1, 2), (5, 3), (8, 3), (app.tab_row.x + 1, 0)] {
            mouse_at(&mut app, MouseEventKind::Down(Right), x, y);
            mouse_at(&mut app, MouseEventKind::Up(Right), x, y);
        }
        assert_eq!(before, (app.state.selected_project(), app.state.focused_pane(), app.state.is_project_collapsed(first)));
        let _ = parent;
    }
```

`drain` and `writes_by_pane` are new helpers over the outbox `Receiver<ClientMessage>`:
- `drain` empties it.
- `writes_by_pane` collects `ClientMessage::WritePane { pane, bytes }` into a `HashMap<PaneId, Vec<Vec<u8>>>`, one entry per write.

Check the variant's real name and fields in `dispatch-proto` (`grep -n "WritePane\|Input {" crates/dispatch-proto/src/*.rs`). `sidebar_row_of` is the helper Task 2 added.

Run `cargo test -p dispatch drag right_click control_acts`. Expected: FAIL.

- [ ] **Step 2: Implement gestures in `route_pointer`.** Fields (doc each): `gesture: Option<pointer::Gesture>` (`None`), `clicks: pointer::Clicks` (default). Remove `dragging_sidebar` and every use of it.

  At the top of `route_pointer`, before the target lookup:

```rust
        // A press still held belongs to what it pressed, wherever the pointer
        // has gone since.
        if let Some(gesture) = self.gesture {
            match mouse.kind {
                MouseEventKind::Drag(button) if button == gesture.button => {
                    self.gesture = Some(pointer::Gesture { last: (mouse.column, mouse.row), ..gesture });
                    self.drag_gesture(gesture.owner, mouse);
                    return true;
                }
                MouseEventKind::Up(button) if button == gesture.button => {
                    self.gesture = None;
                    self.release_gesture(gesture.owner, mouse);
                    return true;
                }
                // Anything else mid-gesture is noise from a terminal that
                // lost a release; the gesture ends as if it had arrived.
                MouseEventKind::Down(_) => self.end_gesture(),
                _ => return true,
            }
        }
```

`drag_gesture(owner, mouse)`:
- `SidebarEdge`: set the width, clamped, as the old Drag arm did.
- `PaneContent(id)`: forward the motion to `id`, relative to *its* interior. Use a new `fn send_mouse_to(&mut self, id, mouse, action)` that computes the offset from `self.layout`'s rect for `id`, saturating at 0. `send_mouse` already clamps past-the-edge motion and release into the pane.
- Any other owner: nothing.

`release_gesture(owner, mouse)`:
- `SidebarEdge`: `save_ui()`.
- `PaneContent(id)`: forward the release as `send_mouse_to` does.
- A control (`Sidebar`, `Tab`, `PaneHeader`, `PaneMenu`, `Menu`, `Dialog`): activate only if `self.hits.resolve(mouse.column, mouse.row, self.overlay_tag())` equals the owner. For `Sidebar`, `sidebar::hit_test` must also answer the same `Hit` at press and at release; store the press's `Hit` for that comparison in a small `pressed_sidebar: Option<sidebar::Hit>` field. Activation runs the code Task 2 had on `Down`, but only for `MouseButton::Left`.

On `Down`:
- Resolve the target, then `self.gesture = Some(Gesture { owner: target, button, last })` for every target, including a pane's content.
- For `PaneContent` with the left, middle or right button, forward the press to the child as today (Task 4 adds click-to-focus).
- Do not activate controls on `Down`. The one exception is the drawer-outside-click close, which stays on `Down` so that it closes on press, as the spec's §1 says the drawer's rules move unchanged.
- Record left presses in `self.clicks.press(target, now)` and keep the result for Task 4 and Task 5.

Wheel events do not start gestures, and they route as before: sidebar wheel, then pane.

`end_gesture`:

```rust
    /// Ends a press whose owner can no longer take its release, sending that
    /// release first to a pane that can, so no child keeps a stuck button.
    fn end_gesture(&mut self) {
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        if let pointer::Target::PaneContent(id) = gesture.owner
            && self.panes.contains_key(&id)
            && self.layout.iter().any(|(pane, _)| *pane == id)
        {
            let (column, row) = gesture.last;
            self.send_mouse_to(id, column, row, dispatch_pty::MouseAction::Release, gesture.button);
        }
        if gesture.owner == pointer::Target::SidebarEdge {
            self.save_ui();
        }
    }
```

Write `send_mouse_to` with whatever signature fits the code. It builds a `MouseInput` with coordinates relative to `id`'s interior and calls `send_mouse`.

Callers of `end_gesture`:
- `act_on`'s overlay branch, replacing the old `dragging_sidebar = false`;
- `Event::FocusLost` and `Event::Resize(..)` in `act_on`, before anything else;
- `close_pane`, when the gesture's owner is that pane (the pane is gone, so `end_gesture`'s `contains_key` check sends nothing);
- `draw`, when the owning pane is no longer in `self.frames`.

`terminal.rs`: add `EnableFocusChange` to the `execute!` in `acquire`, and `DisableFocusChange` wherever `DisableMouseCapture` runs on exit. Both come from `crossterm::event`.

  Right-click rule: a `Down(Right)` on `Sidebar` or `Tab` starts a gesture whose release activates nothing in this task. Task 7 opens menus there. A `Down(Middle)` on a control does nothing.

- [ ] **Step 3: Run the tests.** Run `cargo test -p dispatch`. Expected: PASS. Existing sidebar and tab tests now use the two-event `click`, so they still pass. A test that sent a bare `Down` and expected action must send the release too. Update it that way, and do not weaken the assertion.

- [ ] **Step 4: Gate and commit.**

```bash
git add dispatch/src/app.rs dispatch/src/terminal.rs
git commit -m "feat(tui): a press belongs to what it pressed until released, and ends safely when that goes"
```

---

### Task 4: Click-to-focus, header focus and zoom, `focus_follows_pointer`

**Files:**
- Modify: `crates/dispatch-config/src/config.rs` (+ its tests), `crates/dispatch-tui/src/input.rs` (+tests), `dispatch/src/app.rs`, `dispatch/src/main.rs`, `docs/configuration.md`

**Interfaces:**
- Produces:
  - `InterfaceConfig.focus_follows_pointer: bool` (default `false`);
  - `InputRouter::set_focus_follows_pointer(&mut self, on: bool)`;
  - `App::set_focus_follows_pointer(&mut self, on: bool)`.

- [ ] **Step 1: Failing tests.**
  - `config.rs` tests: `[interface] focus_follows_pointer = true` parses. Absent, it is `false`.
  - `input.rs` tests: a `Moved` over a pane gives `Action::None` by default, and `Action::FocusPane(id)` after `set_focus_follows_pointer(true)`. Scroll mode still gives `None` either way.
  - App tests:

```rust
    #[test]
    fn hovering_over_a_pane_does_not_focus_it() {
        let (mut app, project, daemon, _sent) = attached_app();
        let panes = spawn_several(&mut app, &daemon, project, 2);
        let mut terminal = a_wide_terminal();
        drawn(&mut app, &mut terminal);
        app.focus_pane(panes[1]);
        let (x, y) = middle_of(&app, panes[0]);
        mouse_at(&mut app, MouseEventKind::Moved, x, y);
        assert_eq!(app.state.focused_pane(), Some(panes[1]));

        app.set_focus_follows_pointer(true);
        mouse_at(&mut app, MouseEventKind::Moved, x, y);
        assert_eq!(app.state.focused_pane(), Some(panes[0]), "unless asked for");
    }

    #[test]
    fn a_first_click_focuses_and_reaches_only_a_child_that_tracks_the_mouse() {
        let (mut app, project, daemon, sent) = attached_app();
        let panes = spawn_several(&mut app, &daemon, project, 2);
        let mut terminal = a_wide_terminal();
        drawn(&mut app, &mut terminal);
        app.focus_pane(panes[1]);
        drain(&sent);
        let (x, y) = middle_of(&app, panes[0]);
        click(&mut app, x, y);
        assert_eq!(app.state.focused_pane(), Some(panes[0]));
        assert!(writes_by_pane(&sent).is_empty(), "a child not tracking the mouse gets nothing");

        app.focus_pane(panes[1]);
        track_mouse(&mut app, &daemon, panes[0]);
        drain(&sent);
        click(&mut app, x, y);
        assert_eq!(writes_by_pane(&sent).get(&panes[0]).map(Vec::len), Some(2), "press and release");
    }

    #[test]
    fn a_header_click_focuses_without_input_and_a_double_click_zooms() {
        let (mut app, project, daemon, sent) = attached_app();
        let clock = hand_clock(&mut app);
        let panes = spawn_several(&mut app, &daemon, project, 2);
        let mut terminal = a_wide_terminal();
        drawn(&mut app, &mut terminal);
        advance(&clock, Duration::from_secs(1));
        drawn(&mut app, &mut terminal);
        track_mouse(&mut app, &daemon, panes[0]);
        app.focus_pane(panes[1]);
        drain(&sent);
        let (_, tile) = *app.frames.iter().find(|(id, _)| *id == panes[0]).expect("tiled");

        click(&mut app, tile.x + 2, tile.y);
        assert_eq!(app.state.focused_pane(), Some(panes[0]));
        assert!(writes_by_pane(&sent).is_empty(), "the header is Dispatch's, not the child's");
        assert_eq!(app.state.zoomed_pane(), None);

        advance(&clock, Duration::from_millis(200));
        click(&mut app, tile.x + 2, tile.y);
        assert_eq!(app.state.zoomed_pane(), Some(panes[0]), "the second click of a double zooms");
    }
```

Run `cargo test --workspace focus_follows hovering first_click header_click`. Expected: FAIL.

- [ ] **Step 2: Implement.**
  - `config.rs`: add to `InterfaceConfig`, after `hover_claims_panes`:

```rust
    /// Whether the pointer resting on a pane gives it the keyboard. Off by
    /// default: a click focuses, and a pointer crossing the grid on its way
    /// somewhere never redirects typing.
    pub focus_follows_pointer: bool,
```

    with `focus_follows_pointer: false` in `Default`.
  - `input.rs`: add `focus_follows_pointer: bool` to `InputRouter` (default `false`) and a setter. In `handle_mouse`, the `Moved` arm becomes `MouseEventKind::Moved if self.focus_follows_pointer && self.mode != KeyMode::Scroll => return Action::FocusPane(*id),` with a new `MouseEventKind::Moved => return Action::None,` after it. Update the comment above it.
  - `set_keymap` rebuilds the router, so keep the flag across it. Store it on `App` and re-apply it in `set_keymap`.
  - `app.rs` `set_focus_follows_pointer(on)` stores the flag and calls the router's setter. `main.rs` calls `app.set_focus_follows_pointer(loaded.config.interface.focus_follows_pointer);` beside `set_hover_claims_panes`.
  - In `route_pointer`'s `Down` handling:
    - **`PaneContent(id)` with the left button while `id` is not focused:** call `self.focus_pane(id)` first, then forward the press. `send_mouse` sends bytes only when the child tracks the mouse, so a non-tracking child gets nothing. Its wheel fallback does not apply to a press.
    - **`PaneHeader(id)` or `PaneMenu(id)` with the left button:** only record the gesture. On release inside the same target, `focus_pane(id)`. If `clicks.press` said this press was a double, call `self.state.toggle_zoom()` after focusing. No bytes go to the child.
  - `docs/configuration.md`: document `focus_follows_pointer` under `[interface]`, next to `hover_claims_panes`, and say how they differ.

- [ ] **Step 3: Run the tests.** Run `cargo test --workspace`. Expected: PASS. An existing test that relied on hover focus, such as one that moves the mouse and expects focus, must call `app.set_focus_follows_pointer(true)` first. Say so in the commit body.

- [ ] **Step 4: Gate and commit.**

```bash
git add crates/dispatch-config/src/config.rs crates/dispatch-tui/src/input.rs dispatch/src/app.rs dispatch/src/main.rs docs/configuration.md
git commit -m "feat(tui): click to focus, with focus following the pointer as an option"
```

(Add the test files you touched to `git add` by name.)

---

### Task 5: Sidebar — project select without fold, chevron fold, edge reset

**Files:**
- Modify: `crates/dispatch-tui/src/sidebar.rs` (+tests), `dispatch/src/app.rs`, `docs/usage.md`

**Interfaces:**
- Produces: `sidebar::Hit::ProjectChevron(ProjectId)`.

- [ ] **Step 1: Failing tests.**
  - `sidebar/tests.rs`: `hit_test` at a project row's first two columns gives `Hit::ProjectChevron(id)`, and further along the row gives `Hit::Project(id)`. A branch row gives `Hit::Project(id)` everywhere.
  - App tests:

```rust
    #[test]
    fn a_click_on_a_project_row_selects_it_and_only_its_chevron_folds() {
        let (mut app, _terminal, first, _parent, _child) = app_with_a_drawn_sidebar();
        click(&mut app, 8, 2);
        assert_eq!(app.state.selected_project(), Some(first));
        assert!(!app.state.is_project_collapsed(first), "the row selects; it does not fold");

        click(&mut app, 1, 2);
        assert!(app.state.is_project_collapsed(first), "the chevron folds");
        click(&mut app, 1, 2);
        assert!(!app.state.is_project_collapsed(first));
    }

    #[test]
    fn a_double_click_on_the_sidebar_edge_restores_its_width() {
        let (mut app, project, daemon, _sent) = attached_app();
        let clock = hand_clock(&mut app);
        spawn_several(&mut app, &daemon, project, 1);
        app.sidebar_width = 50;
        let mut terminal = a_wide_terminal();
        drawn(&mut app, &mut terminal);
        let edge = app.sidebar_area.x + app.sidebar_area.width - 1;
        click(&mut app, edge, 5);
        advance(&clock, Duration::from_millis(100));
        click(&mut app, edge, 5);
        assert_eq!(app.sidebar_width, dispatch_config::ui_state::DEFAULT_SIDEBAR);
    }
```

  The existing test `a_click_on_a_project_row_selects_it_and_folds_its_panes` contradicts the spec. Delete it: the first new test replaces it.

- [ ] **Step 2: Implement.**
  - `sidebar.rs`: add the variant `/// A project's twisty and the blank after it: folding is all it does.  ProjectChevron(ProjectId),`. In `hit_row`, `Row::Project(id)` returns `ProjectChevron(id)` when `x < body.x + 2`, and otherwise `Project(id)`. `Row::Branch` still returns `Project(id)`. Update `Hit::Project`'s doc: a click selects the project and no longer folds it.
  - `app.rs`, where sidebar hits activate on release:
    - `Hit::Project(id)` → `self.select_project(id)` only.
    - `Hit::ProjectChevron(id)` → `self.state.toggle_project_collapsed(id)`.
    - The drawer-closing `matches!` keeps `Project` and `Pane`.
  - In `release_gesture` for `SidebarEdge`: when the press that started it was a double-click, set `self.sidebar_width = DEFAULT_SIDEBAR` and save it. `clicks.press` returned the double on `Down`; keep it on the gesture, for example with a `double: bool` field on `Gesture`, set when it starts. Otherwise save as before.
  - `docs/usage.md`: the sidebar paragraph says clicking a project's row moves the view there, and its chevron folds. It also says double-clicking the sidebar's edge restores its width.

- [ ] **Step 3: Run the tests, gate, and commit.**

```bash
git add crates/dispatch-tui/src/sidebar.rs crates/dispatch-tui/src/sidebar/tests.rs dispatch/src/app.rs dispatch/src/pointer.rs docs/usage.md
git commit -m "feat(tui): a project row selects, its chevron folds, and the edge double-click resets the width"
```

---

### Task 6: The `Menu` widget

**Files:**
- Create: `crates/dispatch-tui/src/menu.rs` and `crates/dispatch-tui/src/menu/tests.rs`; `pub mod menu;` in `crates/dispatch-tui/src/lib.rs`

**Interfaces:**
- Produces:
  - `menu::MenuItem<A> { label: String, keys: Option<String>, action: A, enabled: bool }`;
  - `menu::Menu<A>::{new(items, anchor: (u16, u16)) -> Self, set_chrome, next, previous, selected(&self) -> Option<&MenuItem<A>>, select(index), hover(Option<usize>), layout(&self, area: Rect) -> MenuLayout}`;
  - `MenuLayout { rect: Rect, items: Vec<(Rect, usize)> }`, where `items` lists enabled items only;
  - `impl<A> Widget for &Menu<A>`.

`A` is generic so the widget stays free of app types.

- [ ] **Step 1: Failing tests** (`menu/tests.rs`):

```rust
use super::*;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

fn menu(anchor: (u16, u16)) -> Menu<u8> {
    Menu::new(
        vec![
            MenuItem { label: "Zoom".into(), keys: Some("Ctrl p z".into()), action: 1, enabled: true },
            MenuItem { label: "Move to previous tab".into(), keys: None, action: 2, enabled: false },
            MenuItem { label: "Close pane and stop agent".into(), keys: Some("Ctrl p x".into()), action: 3, enabled: true },
        ],
        anchor,
    )
}

#[test]
fn arrows_skip_disabled_items() {
    let mut menu = menu((0, 0));
    assert_eq!(menu.selected().map(|item| item.action), Some(1));
    menu.next();
    assert_eq!(menu.selected().map(|item| item.action), Some(3), "the disabled item is skipped");
    menu.next();
    assert_eq!(menu.selected().map(|item| item.action), Some(1), "and it wraps");
    menu.previous();
    assert_eq!(menu.selected().map(|item| item.action), Some(3));
}

#[test]
fn only_enabled_items_are_clickable() {
    let menu = menu((2, 2));
    let layout = menu.layout(Rect::new(0, 0, 80, 24));
    let indices: Vec<usize> = layout.items.iter().map(|(_, index)| *index).collect();
    assert_eq!(indices, vec![0, 2]);
}

#[test]
fn a_menu_stays_inside_the_window_whatever_its_anchor() {
    for (w, h) in [(80, 24), (30, 10), (12, 5), (1, 1)] {
        let area = Rect::new(0, 0, w, h);
        for anchor in [(0, 0), (w.saturating_sub(1), 0), (0, h.saturating_sub(1)), (w.saturating_sub(1), h.saturating_sub(1))] {
            let menu = menu(anchor);
            let rect = menu.layout(area).rect;
            assert!(rect.x + rect.width <= w && rect.y + rect.height <= h, "{rect:?} in {area:?} at {anchor:?}");
            let mut buf = Buffer::empty(area);
            (&menu).render(area, &mut buf);
        }
    }
}

#[test]
fn a_menu_draws_its_keys_and_leaves_disabled_items_unhighlighted() {
    let mut menu = menu((0, 0));
    let chrome = crate::theme::loud_chrome();
    menu.set_chrome(chrome);
    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);
    (&menu).render(area, &mut buf);
    let text: String = buf.content().iter().map(|cell| cell.symbol()).collect();
    assert!(text.contains("Ctrl p z"));
    crate::theme::assert_no_fixed_colours(&buf);
}
```

  Run `cargo test -p dispatch-tui menu`. Expected: FAIL to compile.

- [ ] **Step 2: Implement `menu.rs`.**

```rust
//! A short list of actions, opened on something: a pane, a project, a tab.
//!
//! It is drawn at an anchor — the pointer, or a `…` — and kept inside the
//! window. Generic over what choosing an item does, so the widget knows
//! nothing of the app it serves.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Clear, Widget};

use crate::picker::write;
use crate::theme::Chrome;

/// One row of a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem<A> {
    /// What the user reads.
    pub label: String,
    /// The keys that do the same, shown dimmed on the right.
    pub keys: Option<String>,
    /// What choosing it does.
    pub action: A,
    /// Whether it can be chosen now.
    pub enabled: bool,
}

/// Where a menu was drawn, for a click to be matched against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuLayout {
    /// The whole box, frame included.
    pub rect: Rect,
    /// Each enabled item's row, with its index.
    pub items: Vec<(Rect, usize)>,
}

/// A menu with a selection.
#[derive(Debug, Clone)]
pub struct Menu<A> {
    items: Vec<MenuItem<A>>,
    anchor: (u16, u16),
    selected: usize,
    hovered: Option<usize>,
    chrome: Chrome,
}

impl<A> Menu<A> {
    /// A menu of `items` opened at `anchor`, its first enabled item selected.
    #[must_use]
    pub fn new(items: Vec<MenuItem<A>>, anchor: (u16, u16)) -> Self {
        let selected = items.iter().position(|item| item.enabled).unwrap_or(0);
        Self { items, anchor, selected, hovered: None, chrome: Chrome::default() }
    }

    /// Draws in `chrome`.
    pub fn set_chrome(&mut self, chrome: Chrome) {
        self.chrome = chrome;
    }

    /// The selected item, when it can be chosen.
    #[must_use]
    pub fn selected(&self) -> Option<&MenuItem<A>> {
        self.items.get(self.selected).filter(|item| item.enabled)
    }

    /// Selects the item at `index`, when it can be chosen.
    pub fn select(&mut self, index: usize) {
        if self.items.get(index).is_some_and(|item| item.enabled) {
            self.selected = index;
        }
    }

    /// Marks the item under the pointer, or none. It never moves the
    /// selection: hover shows, it does not choose.
    pub fn hover(&mut self, index: Option<usize>) {
        self.hovered = index.filter(|index| self.items.get(*index).is_some_and(|item| item.enabled));
    }

    /// Moves to the next enabled item, wrapping.
    pub fn next(&mut self) {
        self.step(1);
    }

    /// Moves to the previous enabled item, wrapping.
    pub fn previous(&mut self) {
        self.step(self.items.len().saturating_sub(1));
    }

    fn step(&mut self, by: usize) {
        let count = self.items.len();
        if count == 0 {
            return;
        }
        let mut index = self.selected;
        for _ in 0..count {
            index = (index + by) % count;
            if self.items[index].enabled {
                self.selected = index;
                return;
            }
        }
    }

    /// Where it draws inside `area`: at its anchor, moved left or up as far
    /// as it must to stay inside, and never larger than `area`.
    #[must_use]
    pub fn layout(&self, area: Rect) -> MenuLayout {
        let widest = self
            .items
            .iter()
            .map(|item| item.label.chars().count() + item.keys.as_ref().map_or(0, |keys| keys.chars().count() + 3))
            .max()
            .unwrap_or(0);
        let width = u16::try_from(widest + 4).unwrap_or(u16::MAX).min(area.width);
        let height = u16::try_from(self.items.len() + 2).unwrap_or(u16::MAX).min(area.height);
        let x = self.anchor.0.clamp(area.x, (area.x + area.width).saturating_sub(width));
        let y = self.anchor.1.clamp(area.y, (area.y + area.height).saturating_sub(height));
        let rect = Rect::new(x, y, width, height);

        let inner = Block::default().borders(Borders::ALL).inner(rect);
        let items = self
            .items
            .iter()
            .enumerate()
            .take(usize::from(inner.height))
            .filter(|(_, item)| item.enabled)
            .map(|(index, _)| (Rect::new(inner.x, inner.y + u16::try_from(index).unwrap_or(0), inner.width, 1), index))
            .collect();
        MenuLayout { rect, items }
    }
}

impl<A> Widget for &Menu<A> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout = self.layout(area);
        if layout.rect.width < 3 || layout.rect.height < 3 {
            return;
        }
        Clear.render(layout.rect, buf);
        let block = Block::default().borders(Borders::ALL).border_style(self.chrome.border);
        let inner = block.inner(layout.rect);
        block.render(layout.rect, buf);

        for (index, item) in self.items.iter().enumerate().take(usize::from(inner.height)) {
            let y = inner.y + u16::try_from(index).unwrap_or(0);
            let lit = item.enabled && (index == self.selected || self.hovered == Some(index));
            let style = if !item.enabled {
                self.chrome.secondary
            } else if lit {
                self.chrome.selection
            } else {
                self.chrome.text
            };
            for x in inner.x..inner.x + inner.width {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_symbol(" ");
                    cell.set_style(style);
                }
            }
            write(buf, inner, inner.x + 1, y, &item.label, style);
            if let Some(keys) = &item.keys {
                let keys_width = u16::try_from(keys.chars().count()).unwrap_or(0);
                let x = (inner.x + inner.width).saturating_sub(keys_width + 1);
                let keys_style = if lit { style } else { self.chrome.secondary };
                write(buf, inner, x, y, keys, keys_style);
            }
        }
    }
}

#[cfg(test)]
mod tests;
```

  `picker::write` is `pub(crate)`, so it is available here.

- [ ] **Step 3: Run the tests, gate, and commit.**

```bash
git add crates/dispatch-tui/src/menu.rs crates/dispatch-tui/src/menu/tests.rs crates/dispatch-tui/src/lib.rs
git commit -m "feat(tui): a menu widget that stays inside the window and skips what cannot be chosen"
```

---

### Task 7: Menus on panes, projects and tabs, and the `…`

**Files:**
- Modify: `dispatch/src/app.rs`, `docs/usage.md`

**Interfaces:**
- Consumes: T6's `Menu`; T3's release activation; `pointer::MenuHit`.
- Produces:
  - `enum MenuAction` in `app.rs`;
  - `Overlay::Menu(Menu<MenuAction>)`, tagged `"menu"`;
  - `App::{open_pane_menu, open_project_menu, open_tab_menu, choose_menu}`.

- [ ] **Step 1: Failing tests.**

```rust
    fn menu_labels(app: &App) -> Vec<String> {
        match &app.overlay {
            Some(Overlay::Menu(menu)) => menu.items().iter().map(|item| item.label.clone()).collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn a_right_click_on_a_pane_row_opens_its_menu_and_changes_nothing_else() {
        let (mut app, _terminal, first, parent, _child) = app_with_a_drawn_sidebar();
        let before = app.state.focused_pane();
        use crossterm::event::MouseButton::Right;
        mouse_at(&mut app, MouseEventKind::Down(Right), 8, 3);
        mouse_at(&mut app, MouseEventKind::Up(Right), 8, 3);

        assert_eq!(
            menu_labels(&app),
            vec!["Zoom", "Move to previous tab", "Move to next tab", "Close pane and end shell"]
        );
        assert_eq!(app.state.focused_pane(), before);
        let _ = (first, parent);
    }

    #[test]
    fn choosing_close_from_a_pane_menu_closes_that_pane() {
        // Open the pane menu on a pane that is not focused, press ↓ to reach
        // the close item, press Enter: that pane is closed, not the focused one.
    }

    #[test]
    fn a_click_outside_a_menu_closes_it_and_does_nothing_else() {
        let (mut app, mut terminal, first, _parent, _child) = app_with_a_drawn_sidebar();
        let second = app.state.projects()[1].id;
        use crossterm::event::MouseButton::Right;
        mouse_at(&mut app, MouseEventKind::Down(Right), 8, 2);
        mouse_at(&mut app, MouseEventKind::Up(Right), 8, 2);
        drawn(&mut app, &mut terminal);
        assert!(matches!(app.overlay, Some(Overlay::Menu(_))));

        let row = sidebar_row_of(&terminal, "second");
        click(&mut app, 8, row);
        assert!(app.overlay.is_none(), "the click closed the menu");
        assert_eq!(app.state.selected_project(), Some(first), "and did not reach the row beneath");
        let _ = second;
    }

    #[test]
    fn a_menu_item_acts_on_release_inside_it() {
        // Open the tab menu by right-clicking a chip, draw, press on "Rename",
        // release on it: the rename prompt opens. Repeat, pressing on "Rename"
        // and releasing on "Close tab": nothing opens and the menu stays.
    }

    #[test]
    fn the_ellipsis_opens_the_pane_menu_on_wide_tiles_only() {
        // A wide tile has `…` at tile.x + tile.width - 3 on its top row, and a
        // left click there opens the pane menu. A tile under 12 columns wide
        // draws no `…`, and its top row is all header.
    }
```

  Write out the three sketched tests in full, using the helpers from the tests above. `Menu::items()` is a new accessor that returns `&[MenuItem<A>]`; add it to `menu.rs` with a unit test.

  Run `cargo test -p dispatch menu ellipsis`. Expected: FAIL.

- [ ] **Step 2: Implement.**

```rust
/// What a menu item does. Most are a key's action on the menu's target, so
/// choosing one runs the same code that key runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    ZoomPane(PaneId),
    MovePane(PaneId, i32),
    ClosePane(PaneId),
    NewPaneIn(ProjectId),
    FoldProject(ProjectId),
    RemoveProject(ProjectId),
    RenameTab(usize),
    MoveTab(usize, i32),
    CloseTab(usize),
}
```

  `Overlay::Menu(Menu<MenuAction>)`: add it to `tag` (`"menu"`), `set_chrome`, `desired_width` (return 0, since a menu places itself), and `draw_overlay`. `draw_overlay` renders it over the whole window area, not `panes_area`, so pass `self.window` through. It is not in `picker()` or `kind()`.

  **Openers**, each building `MenuItem`s with `keys` from `self.router.keymap().path_to(<the mirrored Command>)`:
  - **`open_pane_menu(id, anchor)`:**
    - "Zoom", or "Restore" when `zoomed_pane() == Some(id)`, mirroring `Zoom`.
    - "Move to previous tab" and "Move to next tab" (`MovePaneLeft` / `MovePaneRight`). Disabled when `id` is on the first tab, for previous. Next is always enabled, because moving past the last tab makes a new one.
    - "Close pane and stop agent", or "Close pane and end shell" when `pane.harness.as_str() == SHELL`, mirroring `ClosePane`.
  - **`open_project_menu(id, anchor)`:** "New pane here" (`NewPane`), "Fold" or "Unfold" (`Fold`), "Remove from list" (disabled when the project has panes).
  - **`open_tab_menu(index, anchor)`:** "Rename", "Move left", "Move right", "Close tab". Moves are disabled at the ends.

  **`choose_menu(action)`:**
  1. Close the overlay.
  2. Focus or select the target: `focus_pane(id)` for pane actions, `select_project` for project ones, `select_tab(index)` for tab ones.
  3. Run the existing function:
     - zoom → `toggle_zoom`;
     - move pane → `move_focused_pane(by)`;
     - close → `close_pane(id)`;
     - new pane → `open_harness_picker`;
     - fold → `toggle_project_collapsed`;
     - remove → the same code as the project picker's `d`, refactored out of `drop_selected_project` into `fn drop_project(&mut self, project: ProjectId)` that both call;
     - rename → `open_rename_tab`;
     - move tab → `move_current_tab(by)`;
     - close tab → `open_close_tab`, which still asks yes/no.

  **Keys while a menu is open**, in `handle_overlay` before the generic match: `↑`/`k` previous, `↓`/`j` next, Enter chooses the selected item, Esc closes. Every other key is ignored.

  **Hit entries:** in `draw`, after the overlay is drawn, if it is a menu, push `Target::Menu(MenuHit::Area)` for `layout.rect`, then `Target::Menu(MenuHit::Item(index))` for each entry of `layout.items`.

  **Routing with a menu open:** `act_on`'s overlay branch sends `Event::Mouse` to `route_pointer` first when the overlay is a menu.
  - A `Down` outside the menu's rect closes it and returns consumed. Nothing else runs.
  - A `Down` on an item starts a gesture owned by `Target::Menu(MenuHit::Item(i))`, and its release inside the same item calls `choose_menu`.
  - `Moved` over an item calls `menu.hover(Some(i))`, and anywhere else calls `menu.hover(None)`.

  **Right-click openers:** in `release_gesture`, for a right-button gesture whose release is inside its owner:
  - `Sidebar` → `sidebar::hit_test`:
    - `Pane(id)` or `Twisty(id)` → `open_pane_menu(id, (x, y))`;
    - `Project(id)` or `ProjectChevron(id)` → `open_project_menu`;
    - `Device` → nothing.
  - `Tab(TabHit::Tab(i))` → `open_tab_menu(i, (x, y + 1))`.
  - `PaneHeader(id)` → `open_pane_menu`.

  **The `…`:**
  - In `draw_panes`, for tiles with `outer.width >= 12`, write `…` at `(outer.x + outer.width - 3, outer.y)` in the tile's border colour.
  - The status-words fit check adds 3 to `needed`: the `…` and the blanks around it.
  - In `draw`'s hit recording, push `PaneMenu(id)` for `Rect::new(outer.x + outer.width - 4, outer.y, 3, 1)` **after** the tile's `PaneHeader`, so it sits on top. That covers the `…` and a blank either side.
  - A left release on `PaneMenu(id)` calls `open_pane_menu(id, (x, y + 1))`.

  **Docs:** `docs/usage.md` gains a short "Menus" paragraph: right-click a pane row, pane header, project row or tab, or click a pane's `…`. It lists what each menu offers.

- [ ] **Step 3: Run the tests, gate, and commit.**

```bash
git add dispatch/src/app.rs crates/dispatch-tui/src/menu.rs crates/dispatch-tui/src/menu/tests.rs docs/usage.md
git commit -m "feat(tui): right-click menus for panes, projects and tabs, and a pane's … button"
```

---

### Task 8: Buttons and layouts for every dialog widget

**Files:**
- Create: `crates/dispatch-tui/src/button.rs` (+`button/tests.rs`); `pub mod button;` in `lib.rs`
- Modify: `crates/dispatch-tui/src/{picker,browser,prompt,settings_form}.rs` (+their tests), `dispatch/src/approval.rs`, `dispatch/src/pointer.rs` (replace the `ButtonId` stand-in with `pub use dispatch_tui::button::ButtonId;`)

**Interfaces:**
- Produces:
  - `button::ButtonId { Open, Run, Cancel, OpenPane, SaveDefault, Approve, Deny, Always, Later, Ok, Close }`;
  - `button::Button { id, label: &'static str, default: bool }`;
  - `button::lay_out(buttons: &[Button], row: Rect) -> Vec<(Rect, ButtonId)>`;
  - `button::render(buf, buttons: &[Button], placed: &[(Rect, ButtonId)], chrome: &Chrome, pressed: Option<ButtonId>)`.
- **Per widget:**
  - `with_buttons(Vec<Button>)` (or a constructor argument);
  - `layout(&self, area: Rect) -> DialogLayout`;
  - `set_pressed(Option<ButtonId>)`;
  - `set_hovered(Option<usize>)`, for widgets with rows.
- `pub struct DialogLayout { pub rect: Rect, pub rows: Vec<(Rect, usize)>, pub steps: Vec<(Rect, usize, bool)>, pub buttons: Vec<(Rect, ButtonId)> }` lives in `button.rs`, so every widget returns the same shape.

- [ ] **Step 1: Failing tests for `button.rs`.**

```rust
use super::*;
use ratatui::layout::Rect;

fn pair() -> Vec<Button> {
    vec![
        Button { id: ButtonId::Cancel, label: "Cancel", default: false },
        Button { id: ButtonId::Open, label: "Open", default: true },
    ]
}

#[test]
fn buttons_sit_at_the_right_end_padded_with_a_gap_between() {
    let placed = lay_out(&pair(), Rect::new(0, 10, 40, 1));
    // "[ Cancel ]" is 10 wide, "[ Open ]" 8, one blank between, one before the edge.
    assert_eq!(placed, vec![
        (Rect::new(20, 10, 10, 1), ButtonId::Cancel),
        (Rect::new(31, 10, 8, 1), ButtonId::Open),
    ]);
}

#[test]
fn buttons_that_do_not_fit_are_all_dropped() {
    assert!(lay_out(&pair(), Rect::new(0, 0, 18, 1)).is_empty());
}
```

  Run `cargo test -p dispatch-tui button`. Expected: FAIL.

- [ ] **Step 2: Implement `button.rs`.**

```rust
//! A row of buttons at the foot of a dialog, each a padded label a click
//! anywhere on lands on.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::picker::write;
use crate::theme::Chrome;

/// Which button, for the dialog's owner to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonId { Open, Run, Cancel, OpenPane, SaveDefault, Approve, Deny, Always, Later, Ok, Close }

/// One button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Button {
    /// What pressing it means.
    pub id: ButtonId,
    /// What it says.
    pub label: &'static str,
    /// Whether it is what Enter does, drawn in the accent.
    pub default: bool,
}

/// Where a dialog drew what can be clicked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogLayout {
    /// The whole box.
    pub rect: Rect,
    /// Each list row drawn, with its index among the rows shown.
    pub rows: Vec<(Rect, usize)>,
    /// A settings row's `‹` (false) or `›` (true), with the row's index.
    pub steps: Vec<(Rect, usize, bool)>,
    /// Each button drawn.
    pub buttons: Vec<(Rect, ButtonId)>,
}

/// How wide a button is drawn: its label with a blank and a bracket either side.
fn width(button: &Button) -> u16 {
    u16::try_from(button.label.chars().count() + 4).unwrap_or(u16::MAX)
}

/// Places `buttons` at the right end of `row`, one blank apart and one
/// from the edge; none at all when they do not all fit, since half a set of
/// choices is worse than the keys alone.
#[must_use]
pub fn lay_out(buttons: &[Button], row: Rect) -> Vec<(Rect, ButtonId)> {
    let total: u16 = buttons.iter().map(width).sum::<u16>() + u16::try_from(buttons.len()).unwrap_or(0);
    if buttons.is_empty() || row.height == 0 || total > row.width {
        return Vec::new();
    }
    let mut x = row.x + row.width - total;
    buttons
        .iter()
        .map(|button| {
            let rect = Rect::new(x, row.y, width(button), 1);
            x += width(button) + 1;
            (rect, button.id)
        })
        .collect()
}

/// Draws the placed buttons: the default in the accent, a held one in the
/// selection, the rest in the dialog's text.
pub fn render(buf: &mut Buffer, buttons: &[Button], placed: &[(Rect, ButtonId)], chrome: &Chrome, pressed: Option<ButtonId>) {
    for (rect, id) in placed {
        let Some(button) = buttons.iter().find(|button| button.id == *id) else { continue };
        let style = if pressed == Some(*id) {
            chrome.selection
        } else if button.default {
            chrome.accent
        } else {
            chrome.text
        };
        write(buf, *rect, rect.x, rect.y, &format!("[ {} ]", button.label), style);
    }
}

#[cfg(test)]
mod tests;
```

  Check the first test's expected x values against `lay_out`'s arithmetic (`total` = 10 + 8 + 2 = 20, so `x` starts at 40 − 20 = 20) and adjust whichever is wrong. The formula is the contract: the buttons end one blank before the right edge.

- [ ] **Step 3: Give each widget a layout and buttons.** In each widget, move the rect and row computation out of `render` into `layout(&self, area) -> DialogLayout`, and make `render` call it. There is one source of truth: whatever is drawn is what the map gets. Buttons occupy the last inner row. The list loses that row only when buttons are present and fit. Otherwise the box keeps its old shape, and the hint stays on the bottom border.
  - **`Picker`:** `with_buttons(buttons)`. `rows` are the shown rows drawn, indexed within `shown()`. Add `select_shown(index)` to select by that index, and `set_hovered(Option<usize>)`. Hovered rows draw in `chrome.selection` only while not the selected row, using a lighter look: `chrome.text` with an underline modifier, so hover and selection stay distinct as the handoff asks.
  - **`Browser`:** buttons; `rows` are visible entries, indexed in `visible()`; `select_visible(index)`.
  - **`SettingsForm`:** buttons; `rows` are its rows; `steps` are each row's `◂` (the two cells at `value_x`) and `▸` (the two cells after the value), marked false and true. Add `select_row(index)` and `step(index, forward) -> FormAction`, running the same code as ←/→.
  - **`Prompt`:** buttons in a row under the hint; the box grows one row when they are present.
  - **`Approval`** (`dispatch/src/approval.rs`): a `buttons: Vec<Button>` field. The key legend line stays in the text, and the buttons take the inner area's last row, with `render_content` given one row less. `layout(area)` returns the buttons over the rect the caller passes. The caller already centres it with `centred_approval`.

  Tests, one per widget, in each widget's test file:
  - `layout(area).buttons` is non-empty at a comfortable size, and each button's label is drawn at its rect;
  - at a size where they do not fit, `buttons` is empty and nothing overlaps the list;
  - `rows[i]` is the rect the i-th shown row's label is drawn on;
  - for every size from 1×1 to 30×10, `render` does not panic.

- [ ] **Step 4: Run the tests, gate, and commit.**

```bash
git add crates/dispatch-tui/src/button.rs crates/dispatch-tui/src/button/tests.rs crates/dispatch-tui/src/lib.rs crates/dispatch-tui/src/picker.rs crates/dispatch-tui/src/picker/tests.rs crates/dispatch-tui/src/browser.rs crates/dispatch-tui/src/browser/tests.rs crates/dispatch-tui/src/prompt.rs crates/dispatch-tui/src/prompt/tests.rs crates/dispatch-tui/src/settings_form.rs crates/dispatch-tui/src/settings_form/tests.rs dispatch/src/approval.rs dispatch/src/pointer.rs
git commit -m "feat(tui): buttons, and one layout shared by drawing and clicking, for every dialog"
```

---

### Task 9: Clickable dialogs in the app

**Files:**
- Modify: `dispatch/src/app.rs`, `docs/usage.md`

**Interfaces:**
- Consumes: T8's `DialogLayout`, `ButtonId`, and the widget setters; T3's gestures and `Clicks`.
- Produces: `App::press_button(&mut self, ButtonId, area: Size)` and `App::dialog_layout(&self) -> Option<DialogLayout>`.

- [ ] **Step 1: Failing tests**, one per row of spec §6 plus the isolation rule:

```rust
    #[test]
    fn a_picker_row_click_selects_and_a_double_click_chooses() {
        // Open the waiting list with two blocked panes, draw, click row 1:
        // the selection moves and the list stays open. Double-click row 1:
        // that pane is focused and the list is closed.
    }

    #[test]
    fn every_dialog_button_does_what_its_key_does() {
        // For each dialog, open it, draw, click each button by its rect from
        // `app.dialog_layout()`, and compare with pressing its key:
        //   pickers [Open]=Enter [Cancel]=Esc; help [Run]=Enter;
        //   browser [Open]=Enter; settings [Open pane]=Enter [Save as default]=s [Cancel]=Esc;
        //   approval [Approve]=a [Deny]=d [Always]=A [Later]=Esc (use `app_with_one_pending`);
        //   rename/open-on/add-machine [OK]=Enter [Cancel]=Esc; close tab [Close]=y [Cancel]=Esc.
    }

    #[test]
    fn nothing_reaches_a_child_while_a_dialog_is_open() {
        // With a mouse-tracking focused pane and a dialog open: a click inside
        // the dialog, a click outside it, a paste and a key — the outbox holds
        // no write for the pane, and the dialog is still open after the outside
        // click.
    }

    #[test]
    fn the_wheel_over_a_dialog_moves_its_selection_and_nothing_behind_it() {
        // Waiting list with three rows, wheel down twice over it: row 2
        // selected; the pane under it scrolled nothing (`scrolled_back` false).
    }

    #[test]
    fn a_settings_arrow_click_steps_its_value() {
        // Open the settings form for a harness with a choice setting (see the
        // existing `with_settings` helper), draw, click the row's `›`: the
        // value is what → would have made it.
    }
```

  Write each sketched test in full, driving it through `click`, `mouse_at`, `drawn` and the dialog's layout rects. Never hard-code coordinates.

  Run `cargo test -p dispatch dialog`. Expected: FAIL.

- [ ] **Step 2: Implement.**
  - **Buttons per overlay,** set where each overlay is created:
    - pickers `[Cancel] [Open]`, and help `[Cancel] [Run]`;
    - browser `[Cancel] [Open]`;
    - settings `[Cancel] [Save as default] [Open pane]`;
    - approval `[Later] [Deny] [Always] [Approve]`;
    - prompts `[Cancel] [OK]`;
    - close tab `[Cancel] [Close]`.
    The default button is the one Enter presses.
  - **`dialog_layout`:** returns the open overlay's `layout` over the same area `draw_overlay` renders it in. Store that area on `App` as `overlay_area` when drawing. The approval dialog uses `centred_approval(panes_area)`.
  - **Hit entries:** in `draw`, after the overlay, push `Dialog(Area)` for the rect, then `Dialog(Row(i))`, `Dialog(Step(i, forward))` and `Dialog(Button(id))`, in that order.
  - **Routing with a dialog open:** `act_on`'s overlay branch sends mouse events to `route_pointer` first.
    - A `Down` outside the dialog's rect is consumed and does nothing.
    - A left `Down` on `Row(i)` selects it at once, with `select_shown`, `select_visible` or `select_row`. If `clicks.press` says double, also run what Enter does for that dialog, through the same function Enter calls.
    - `Step` and `Button` start gestures. Release inside the same target acts:
      - `Step(i, forward)` → `form.step(i, forward)`, handled as `handle_settings_key` handles a `FormAction`;
      - `Button(id)` → `press_button(id, area)`.
    - While a button gesture is held over its button, call `set_pressed(Some(id))`. Clear it on release or when the pointer slides off.
    - `Moved` over a row calls `set_hovered(Some(i))`, and anywhere else calls `set_hovered(None)`.
    - The wheel inside a list dialog calls `next`/`previous` once per notch, and never reaches the panes.
    - Every other mouse event is consumed.
  - **`press_button(id, area)`:** runs exactly the key's code.
    - For pickers, `Open` is the Enter arm of the generic match. Refactor that arm into `fn choose_selected(&mut self, area: Size) -> Result<()>` so the key and the button share it.
    - `Cancel` is the Esc arm.
    - `Run` is help's Enter branch, refactored the same way into `fn run_help_selection(&mut self)`.
    - Approval: `decide(true, false)`, `decide(false, false)`, `decide(true, true)`, and `self.overlay = None` for `Later`.
    - Settings: the `FormAction` handling for `Open` and `Save`, and `back_to_picker` for `Cancel`.
    - Prompts: their Enter and Esc arms, refactored into functions the key handlers call.
    - Close tab: its confirm and cancel.
  - **Docs:** `docs/usage.md` says dialogs take clicks. Click a row to choose it, double-click or press its button to act. A click outside a dialog does nothing, and a click outside a menu closes it.

- [ ] **Step 3: Run the tests, gate, and commit.**

```bash
git add dispatch/src/app.rs docs/usage.md
git commit -m "feat(tui): every dialog answers the mouse: rows, double-click, buttons and the wheel"
```

---

## After the last task

- [ ] Full gate on a clean tree.
- [ ] `cargo build --release`, then check these by hand in Ghostty:
  - a drag from one pane to another;
  - right-click menus;
  - the `…`;
  - dialog buttons;
  - click-to-focus;
  - `focus_follows_pointer = true`;
  - lock mode with the mouse.
- [ ] Update `open-follow-ups.md` in memory with anything parked, and note sub-projects B–E as next.
