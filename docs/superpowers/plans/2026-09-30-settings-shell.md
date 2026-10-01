# Settings Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sub-project B of the mouse and Settings handoff:
- messages that expire;
- a quiet status footer with `[Activity]` and `[Settings]`;
- an Activity list;
- `preferences.toml`, with precedence, provenance and per-section Apply;
- a glyph table with a plain fallback;
- theme presets and accents;
- a Settings workspace with Mouse & layout, Appearance and Advanced categories.

**Architecture:**
- **Pure pieces** go in the crates:
  - `dispatch-config::preferences` (types, effective values, apply with conflicts);
  - `dispatch-tui::glyphs`, `dispatch-tui::settings_view` (view state, layout, rendering);
  - theme presets in `dispatch-tui::theme`;
  - `dispatch-pty::VtTerminal::scrollbar`.
- **App wiring** goes in new child modules of `dispatch/src/app/`: `footer.rs`, `settings.rs`. `app.rs` only gains overlay variants and hooks, following the `app/pointer_routing.rs` pattern.

**Tech Stack:** Rust 2024 (MSRV 1.89), ratatui 0.30, crossterm, serde/toml, libghostty-vt (FFI).

**Spec:** `docs/superpowers/specs/2026-09-30-settings-shell-design.md`. Read it before each task. It is the binding authority.

## Global Constraints

- **Branch:** `tui-settings`, stacked on `TUI`. Stage files by name. Never run `git add -A`.
- **Gate:** every task ends green on:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --no-fail-fast`
- **Style:** MSRV 1.89. Comments say *why*, in full sentences, British spelling, no "we".
- **Commits:** end with exactly these two lines, after a blank line:
  `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_0176ZvPVLNdCCP5gh3a8m9m4`
- **Messages:**
  - `MESSAGE_FOR` = 5 s. Info messages expire. Errors stay until replaced or clicked.
- **Footer:**
  - Normal-mode parts are joined by ` · `, in this order: Connection | Unreachable, Message, Working, Attention, Delegations; then `[Activity]`, then `[Settings]` right-aligned.
  - Narrow drop order: Working, then Connection, then `[Activity]`, then the message is cut with `…`. Attention, Delegations and `[Settings]` are never dropped while they fit.
  - Lock has no exit button.
- **Keys:** Settings is on `Alt ,` (normal), `,` (session) and `,` (prefix). Activity has no default key.
- **Files:** `preferences.toml` lives beside `ui.toml`. `config.toml` is never written. Precedence is built-in < `config.toml` `[interface]` < `preferences.toml`.
- **Light palette:** background `#fafafa`, foreground `#383a42`, accent `#a626a4`.
- **Accent presets:** Violet `#b4a0f0`, Blue `#61afef`, Teal `#56b6c2`, Green `#98c379`, Amber `#e5c07b`, Red `#e06c75`, Pink `#ff79c6`.
- **Plain glyphs:**
  - starting `~`, running `>`, idle `-`, blocked `!`, done `+`, failed `x`, closed `#`, unseen `*`;
  - twisty open `v`, shut `>`, leaf ` `;
  - repository `@`, folder `/`.
  - A plain harness icon is the first letter of its display name, upper case.
- **Settings sizing:**
  - ≥100×28: centred, at most 110×34, with a 1-cell margin.
  - Smaller: the whole body.
  - Under 72 columns: a category picker.
  - Under 40×10: a resize message and `[×]` only.
- **Isolation:** nothing reaches a child PTY while any overlay is open. Widgets never write files or spawn panes.

## Review Focus

1. **A hand-edited `preferences.toml` with one bad value** (`accent = "chartreuse"`, `theme = 3`). The other values still load, the bad one is logged and treated as absent, and nothing panics. The test is in Task 5.
2. **Apply after another Dispatch changed the same field.** The result is a conflict and no write. A changed *other* field merges untouched. The test is in Task 5.
3. **Discard after previewing.** The theme and glyphs return exactly to the committed values, including when Settings is closed by `[×]` or Esc with "Discard". The test is in Task 9.
4. **The footer at 30–40 columns with attention, a delegation and an error all present.** No panic, `[Settings]` is still clickable, and attention is still shown. The test is in Task 3.
5. **An Info message showing while the app is otherwise idle.** `next_frame` returns the time left until expiry, not `None`, so the message disappears without input. The test is in Task 1.

---

## File map

| File | Change |
|---|---|
| `dispatch/src/app.rs` | `Status` (T1); hooks for footer, Activity and Settings (T3, T4, T9) |
| `dispatch/src/app/footer.rs` (new) | footer drawing, parts, hits (T3) |
| `dispatch/src/app/settings.rs` (new) | Settings overlay routing, preview, Apply (T9, T10) |
| `dispatch/src/app/pointer_routing.rs` | `Target::Footer` activation (T3); Settings hits (T9) |
| `dispatch/src/pointer.rs` | `FooterHit`, new `DialogHit` variants (T3, T8) |
| `dispatch/src/main.rs` | preferences loading and effective values at start (T5, T7) |
| `crates/dispatch-pty/src/{vt,sys}.rs` | `Scrollbar`, `VtTerminal::scrollbar` (T2) |
| `crates/dispatch-config/src/preferences.rs` (new, +tests) | T5 |
| `crates/dispatch-tui/src/glyphs.rs` (new, +tests) | T6 |
| `crates/dispatch-tui/src/sidebar.rs` | `with_glyphs` (T6) |
| `crates/dispatch-tui/src/theme.rs` | `Palette::LIGHT`, `Accent` presets, `Theme::palette()`/`depth()` (T7) |
| `crates/dispatch-tui/src/settings_view.rs` (new, +tests) | T8 |
| `crates/dispatch-tui/src/{keymap,input}.rs` | `Command::{Settings, Activity}` (T4, T9) |
| `crates/dispatch-tui/src/button.rs` | new `ButtonId`s (T4, T8) |
| `docs/usage.md`, `docs/configuration.md` | T3, T4, T10 |

Test helpers already in `dispatch/src/app.rs` `mod tests`: `attached_app`, `spawn_several`, `spawned`, `click`, `mouse_at`, `press`, `press_alt`, `rebind`, `drawn`, `a_wide_terminal`, `rendered_text`, `bottom_row`, `hand_clock`, `advance`, `send_tabs`, `sidebar_row_of`, `app_with_one_pending`, `track_mouse`, `drain`, `writes_by_pane`. Read each one before using it.

---

### Task 1: Messages that expire

**Files:** Modify `dispatch/src/app.rs`, `dispatch/src/app/pointer_routing.rs`. Tests go in `app.rs` `mod tests`.

**Interfaces:**
- Produces:
  - `enum StatusKind { Info, Error }` and `struct Status { text: String, kind: StatusKind, at: Instant }`;
  - `impl Deref<Target = str> for Status`, `impl PartialEq<&str> for Status` and `impl PartialEq<str>`, plus `Debug`;
  - `Status::clear(&mut self)`, `Status::is_error(&self) -> bool`;
  - `App::{say(&mut self, impl Into<String>), warn(&mut self, impl Into<String>), clear_status(&mut self)}`;
  - `const MESSAGE_FOR: Duration = Duration::from_secs(5)`.

- [ ] **Step 1: Failing tests.**

```rust
    #[test]
    fn an_info_message_expires_and_asks_for_the_frame_that_clears_it() {
        let (mut app, _project, _daemon, _sent) = attached_app();
        let clock = hand_clock(&mut app);
        app.animations.sweep(app.now() + Duration::from_secs(10));
        app.say("registered claude");
        assert_eq!(app.status, "registered claude");
        let left = app.next_frame(app.now()).expect("the expiry needs a frame");
        assert!(left <= MESSAGE_FOR && left > Duration::from_secs(4), "{left:?}");

        advance(&clock, MESSAGE_FOR + Duration::from_millis(1));
        app.expire_status();
        assert!(app.status.is_empty());
        assert_eq!(app.next_frame(app.now()), None, "nothing left to clear");
    }

    #[test]
    fn an_error_stays_until_replaced() {
        let (mut app, _project, _daemon, _sent) = attached_app();
        let clock = hand_clock(&mut app);
        app.warn("laptop is unreachable");
        advance(&clock, Duration::from_secs(60));
        app.expire_status();
        assert_eq!(app.status, "laptop is unreachable");
        assert!(app.status.is_error());
        app.say("ok");
        assert!(!app.status.is_error());
    }
```

`expire_status(&mut self)` clears an Info message whose time is up. `draw` calls it first, so expiry is seen on the frame `next_frame` asked for.

Run `cargo test -p dispatch message`. Expected: FAIL to compile.

- [ ] **Step 2: Implement.**

```rust
/// How long a message that reports success stays on the footer.
const MESSAGE_FOR: Duration = Duration::from_secs(5);

/// Whether a message reports something done or something wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusKind {
    /// Done; it goes after [`MESSAGE_FOR`].
    Info,
    /// Wrong; it stays until something replaces it or it is clicked away.
    Error,
}

/// The footer's message.
#[derive(Debug, Clone)]
struct Status {
    text: String,
    kind: StatusKind,
    at: Instant,
}
```

- `Default` is an empty Info at `Instant::now()`.
- `Deref<Target = str>` returns `&self.text`, so `is_empty`, `starts_with` and `contains` keep working in the existing tests.
- `PartialEq<&str>` and `PartialEq<str>` compare `text`.
- `clear()` empties `text`.
- `is_error()` is `kind == Error && !text.is_empty()`.

On `App`: `status: Status`.
- `say` sets `Status { text, kind: Info, at: self.now() }`.
- `warn` sets the same with `Error`.
- `clear_status` clears it.
- `expire_status` clears it when `kind == Info` and `now - at >= MESSAGE_FOR`.

Call `expire_status()` at the top of `draw`.

`next_frame`, after the tween check:

```rust
        // A success message goes by itself: the frame that clears it is due
        // when it expires, even with nothing else moving.
        let expiry = (!self.status.is_empty() && !self.status.is_error()).then(|| {
            MESSAGE_FOR.saturating_sub(_now.saturating_duration_since(self.status.at))
        });
        let spin = (self.motion && self.spinner_drawn).then_some(SPIN_FRAME);
        match (spin, expiry) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
```

Rename `_now` to `now` now that it is used.

Replace every `self.status = …` assignment in `app.rs` and `app/pointer_routing.rs` (`grep -n "self.status = " dispatch/src/app.rs dispatch/src/app/*.rs`) with `self.say(…)` or `self.warn(…)`:
- **`warn`** for refusals, failures, "unreachable", "gone", "cannot", "failed", and anything that tells the user why something did not happen;
- **`say`** for confirmations and neutral information ("registered …", "scrolled back …", "starting …", "nothing waiting on you").

Replace `self.status.clear()` with `self.clear_status()`. `pub fn set_status` becomes `self.warn(status)`. Each existing test that assigns `app.status = …` must use `say` or `warn`.

- [ ] **Step 3: Run the tests.** Run `cargo test --workspace`. Expected: PASS. Existing assertions such as `assert_eq!(app.status, "…")` compile through `PartialEq<&str>`.

- [ ] **Step 4: Gate and commit.**

```bash
git add dispatch/src/app.rs dispatch/src/app/pointer_routing.rs
git commit -m "feat(tui): messages that report success expire; errors stay"
```

---

### Task 2: `VtTerminal::scrollbar`

**Files:** Modify `crates/dispatch-pty/src/sys.rs`, `crates/dispatch-pty/src/vt.rs` (and its tests), `crates/dispatch-pty/src/lib.rs` (export `Scrollbar`).

**Interfaces:**
- Produces `pub struct Scrollbar { pub total: u64, pub offset: u64, pub len: u64 }`, with `Scrollbar::above_live(&self) -> u64` and `VtTerminal::scrollbar(&self) -> Result<Scrollbar, VtError>`.

- [ ] **Step 1: Failing test** (in vt.rs's test module, beside the scrollback test around line 539):

```rust
    #[test]
    fn the_scrollbar_counts_the_lines_above_live() {
        let mut terminal = VtTerminal::new(Size::new(20, 5)).expect("a terminal");
        for line in 0..30 {
            terminal.feed(format!("line {line}\r\n").as_bytes());
        }
        let at_bottom = terminal.scrollbar().expect("a scrollbar");
        assert_eq!(at_bottom.above_live(), 0);
        assert_eq!(at_bottom.len, 5);
        assert!(at_bottom.total > 5);

        terminal.scroll(ScrollTo::Delta(-3));
        assert_eq!(terminal.scrollbar().expect("a scrollbar").above_live(), 3);
    }
```

Run `cargo test -p dispatch-pty scrollbar`. Expected: FAIL to compile.

- [ ] **Step 2: Implement.** In `sys.rs`, under `pub mod data`:

```rust
    /// The viewport's place in the scrollback. Output type
    /// `GhosttyTerminalScrollbar *`.
    pub const SCROLLBAR: i32 = 9;
```

and a mirror of the C struct:

```rust
/// Mirrors `GhosttyTerminalScrollbar`, whose layout is frozen.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalScrollbar {
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}
```

In `vt.rs`:

```rust
/// Where the viewport is in the scrollback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scrollbar {
    /// Rows in the whole scrollable area.
    pub total: u64,
    /// The first row the viewport shows.
    pub offset: u64,
    /// Rows the viewport shows.
    pub len: u64,
}

impl Scrollbar {
    /// How many rows of newer output lie below the viewport.
    #[must_use]
    pub fn above_live(&self) -> u64 {
        self.total.saturating_sub(self.offset).saturating_sub(self.len)
    }
}

    /// Where the viewport is in the scrollback, for saying how far back it is.
    pub fn scrollbar(&self) -> Result<Scrollbar, VtError> {
        let mut raw = sys::TerminalScrollbar::default();
        // SAFETY: the terminal is live, and `raw` is the type the SCROLLBAR
        // selector documents.
        let code = unsafe {
            sys::ghostty_terminal_get(self.handle, sys::data::SCROLLBAR, (&raw mut raw).cast::<c_void>())
        };
        VtError::check("ghostty_terminal_get(SCROLLBAR)", code)?;
        Ok(Scrollbar { total: raw.total, offset: raw.offset, len: raw.len })
    }
```

Check the header (`target/debug/build/dispatch-pty-*/out/libghostty-vt/include/ghostty/vt/terminal.h`, `GhosttyTerminalScrollbar`) for the field order, and `above_live`'s meaning against what the test observes. If `offset` turns out to count from the bottom, adjust `above_live` and its doc to match, and keep the test.

- [ ] **Step 3: Run the tests, gate, and commit.**

```bash
git add crates/dispatch-pty/src/sys.rs crates/dispatch-pty/src/vt.rs crates/dispatch-pty/src/lib.rs
git commit -m "feat(pty): read the viewport's place in the scrollback"
```

---

### Task 3: The quiet footer

**Files:**
- Create `dispatch/src/app/footer.rs`, and declare `mod footer;` in `app.rs` beside `mod pointer_routing;`.
- Modify `dispatch/src/app.rs`: remove the old `draw_status` body and call `self.draw_footer(frame, area, &mut hits)`.
- Modify `dispatch/src/pointer.rs` (`FooterHit`, `Target::Footer`), `dispatch/src/app/pointer_routing.rs` (activation), `docs/usage.md`.

**Interfaces:**
- Produces:
  - `pointer::FooterHit { Attention, Delegations, Message, Activity, Settings, ReturnToLive, LeaveMode }` and `Target::Footer(FooterHit)`;
  - `fn draw_footer(&mut self, frame, area: Rect, hits: &mut HitMap)` in `footer.rs`;
  - `fn footer_parts(&self, width: u16) -> Vec<Part>`, where `struct Part { text: String, style: Style, hit: Option<FooterHit> }`. It is pure over `&self`, so it can be tested without drawing.
- Consumes: `pane_order`, `is_waiting`, `pending`, `status`, `router.key_mode()`, `keymap.lock_help()`, `scrolling` with `VtTerminal::scrollbar` (T2), and `open_attention_picker` / `open_next_approval`.
- Opening Activity and Settings is wired in T4 and T9. Until then, clicking `[Activity]` or `[Settings]` is a no-op with a `// T4` / `// T9` comment.

- [ ] **Step 1: Failing tests** (app tests):

```rust
    fn footer(app: &mut App, width: u16) -> String {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 20))
            .expect("a test backend can be created");
        drawn(app, &mut terminal);
        bottom_row(&terminal)
    }

    #[test]
    fn the_footer_says_what_is_going_on_not_which_keys_to_press() {
        let (mut app, project, daemon, _sent) = attached_app();
        let panes = spawn_several(&mut app, &daemon, project, 3);
        app.state.set_pane_status(panes[0], PaneStatus::Running).expect("exists");
        app.state.set_pane_status(panes[1], PaneStatus::Blocked).expect("exists");
        let row = footer(&mut app, 120);
        assert!(row.contains("Connected · 1 working · 1 need attention"), "{row}");
        assert!(row.trim_end().ends_with("[ Settings ]"), "{row}");
        assert!(row.contains("[ Activity ]"), "{row}");
        assert!(!row.contains("Ctrl"), "no shortcut wall: {row}");
        assert!(!row.contains("help"), "{row}");
    }

    #[test]
    fn a_standalone_footer_claims_no_connection() {
        let (mut app, _pane) = app_with_a_pane("/tmp/solo", "shell", "Shell");
        assert!(!footer(&mut app, 120).contains("Connected"));
    }

    #[test]
    fn a_narrow_footer_keeps_attention_and_settings() {
        let (mut app, project, daemon, _sent) = attached_app();
        let panes = spawn_several(&mut app, &daemon, project, 2);
        app.state.set_pane_status(panes[0], PaneStatus::Running).expect("exists");
        app.state.set_pane_status(panes[1], PaneStatus::Blocked).expect("exists");
        app.warn("something went wrong with a long explanation of why");
        for width in [80, 60, 40, 30] {
            let row = footer(&mut app, width);
            assert!(row.contains("need attention"), "{width}: {row}");
            assert!(row.contains("Settings"), "{width}: {row}");
        }
        let row = footer(&mut app, 40);
        assert!(!row.contains("working"), "routine counts go first: {row}");
    }

    #[test]
    fn the_footer_parts_do_what_they_say_when_clicked() {
        // Blocked pane → click "need attention" → the attention picker opens.
        // An Error → click it → the message is gone.
        // Find each part's column with `row.find(...)`, then `click(app, col, row_y)`.
    }

    #[test]
    fn every_mode_has_a_compact_footer() {
        // Scroll mode on a pane with scrollback (print 60 lines, enter scroll
        // mode, scroll up 3): "Scrollback · 3 lines above live" and
        // "[ Return to live ]"; clicking it leaves scroll mode.
        // Ctrl p: "PANE" and "[ Done ]"; clicking Done → KeyMode::Normal.
        // Ctrl a: "PREFIX". Ctrl g: "LOCKED · Ctrl g unlocks" and no "[".
    }
```

Write the two sketched tests in full. Remove or rewrite the existing tests that assert the old footer text, under the rule that each test must keep its intent:
- `the_normal_row_lists_the_mode_keys`: delete it. The spec removes the key list.
- The `… — Alt a` / `— Ctrl b a` suffix tests: rewrite them to assert the count without a suffix.
- The 80-column reminder test: keep it, and assert the counts.
- `the_help_chip_survives_a_long_status_message`: delete it, since the chip is removed. Replace it with `a_long_message_never_pushes_settings_off_the_footer`.

Run `cargo test -p dispatch footer mode`. Expected: FAIL.

- [ ] **Step 2: Implement `footer.rs`.**
- **Normal mode.** Build the parts in the spec's order (§2 table). Styles:
  - Connection, Working and an Info message: `chrome.secondary`;
  - an Error message: `Color::Red`;
  - Attention and Delegations: `chrome.accent`.

  Then fit them to `row.width - settings_width - 1`:
  1. reserve `[ Settings ]` at the right;
  2. drop Working, then Connection, then `[ Activity ]` until the rest fits;
  3. cut the message with `sidebar::truncate`;
  4. if Attention and Delegations still do not fit, cut them too, never below their first word.

  Record each drawn part with a `hit` as `Target::Footer(hit)` over its rectangle, and the two buttons likewise.
- **Scroll mode.** The pane is `self.scrolling`. Lines come from `self.panes[&id].backend.terminal().scrollbar()`. An `Err`, or `total == len` (the alternate screen), shows `Scrollback`. Otherwise show `Scrollback · {n} lines above live` (`1 line` for one), plus a right-aligned `[ Return to live ]` with hit `ReturnToLive`.
- **Pane, tab and session modes.** Show `KeyMode::title()`, then the message, then a right-aligned `[ Done ]` with hit `LeaveMode`.
- **Prefix:** `PREFIX`.
- **Lock:** `keymap.lock_help()` reworded to `LOCKED · {key} unlocks`, built from `chords_for(KeyMode::Lock, Command::Unlock)`, or `LOCKED` when unbound. Lock gets no button.
- **Background.** A key mode keeps today's `tab` background with bold text.

  Activation, in `pointer_routing.rs` `activate` on a left release inside the same target:

  | Hit | Action |
  |---|---|
  | `Attention` | `self.open_attention_picker()` |
  | `Delegations` | `self.open_next_approval()` |
  | `Message` | `self.clear_status()`, only when `status.is_error()` |
  | `ReturnToLive` | the scroll-mode leave path `Esc` uses (`self.router.leave_mode()`, then `settle_scroll_mode`) |
  | `LeaveMode` | `self.router.leave_mode()` |
  | `Activity`, `Settings` | no-op until T4 and T9 |

  A footer press goes through the usual gesture path, so a right-click does nothing.

  Update `docs/usage.md`: the footer section describes the parts and buttons. Delete the sentence about the help key on the footer, and add that keys are listed in command help (`Alt /`).

- [ ] **Step 3: Run the tests, gate, and commit.**

```bash
git add dispatch/src/app.rs dispatch/src/app/footer.rs dispatch/src/app/pointer_routing.rs dispatch/src/pointer.rs docs/usage.md
git commit -m "feat(tui): a footer that says what is going on, not which keys to press"
```

---

### Task 4: Activity

**Files:** Modify `crates/dispatch-tui/src/keymap.rs` (+tests), `crates/dispatch-tui/src/input.rs`, `crates/dispatch-tui/src/button.rs`, `dispatch/src/app.rs`, `dispatch/src/app/pointer_routing.rs`, `docs/usage.md`.

**Interfaces:**
- Produces:
  - `Command::Activity` (`activity`, label `activity`, describe `"Show every pane's state"`, no default key);
  - `Action::Activity`;
  - `ButtonId::Go`;
  - `Overlay::Activity(Picker)` (tag `"activity"`);
  - `App::open_activity`.

- [ ] **Step 1: Failing tests.**

```rust
    #[test]
    fn activity_lists_every_live_pane_with_its_state_and_goes_to_one() {
        let (mut app, first, daemon, _sent) = attached_app();
        let project = Project::new("/tmp/second", ProjectSource::LocalDir);
        let second = project.id;
        daemon.send(ServerMessage::ProjectOpened { project }).expect("listening");
        app.poll_daemon();
        let ours = spawn_several(&mut app, &daemon, first, 1);
        app.select_project(second);
        let theirs = spawn_several(&mut app, &daemon, second, 1);
        app.state.set_pane_status(theirs[0], PaneStatus::Running).expect("exists");
        app.select_project(first);

        app.open_activity();
        let Some(Overlay::Activity(picker)) = &app.overlay else { panic!("activity is open") };
        assert_eq!(picker.items().len(), 2);
        assert!(picker.items()[1].detail.as_deref().is_some_and(|d| d.starts_with("Working")));

        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.state.selected_project(), Some(second));
        assert_eq!(app.state.focused_pane(), Some(theirs[0]));
        let _ = ours;
    }

    #[test]
    fn the_footer_activity_button_opens_activity() {
        // Draw at 120 columns, find "[ Activity ]" on the bottom row, click it:
        // Overlay::Activity is open.
    }
```

Write the second test in full. Add a keymap test that `Command::from_name("activity")` round-trips, and that no default chord is bound to it in any mode.

- [ ] **Step 2: Implement.**
- Add the command and action in the existing pattern: `NAMED`, `label`, `describe`, `action`. Put no binding in `defaults()`.
- `perform`: `Action::Activity => self.open_activity()`.
- `open_activity` builds the same rows as `open_attention_picker`, but over every live pane in `pane_order()`. Detail is `status_text(..)` plus ` · {waited}` when `blocked_since` has the pane. It uses `Picker::new("Activity", items).with_buttons(vec![Button { id: ButtonId::Cancel, label: "Cancel", default: false }, Button { id: ButtonId::Go, label: "Go", default: true }])`.
- `OverlayKind::Activity`, with `choose` going to the chosen pane through `go_to_pane` and guarded for a stale id as Attention is.
- Add Activity everywhere the compiler requires: `picker`, `picker_mut`, `kind`, `tag`, `set_chrome`, `desired_width`, `dialog_layout`, `press_button`. `ButtonId::Go` is pressed like `Open`.
- The footer's `[Activity]` hit calls `open_activity`.
- `docs/usage.md`: one sentence on Activity.

- [ ] **Step 3: Run the tests, gate, and commit.**

```bash
git add crates/dispatch-tui/src/keymap.rs crates/dispatch-tui/src/keymap/tests.rs crates/dispatch-tui/src/input.rs crates/dispatch-tui/src/button.rs dispatch/src/app.rs dispatch/src/app/pointer_routing.rs dispatch/src/app/footer.rs docs/usage.md
git commit -m "feat(tui): an Activity list of every pane and what it is doing"
```

---

### Task 5: `preferences.toml`

**Files:**
- Create `crates/dispatch-config/src/preferences.rs` and `crates/dispatch-config/src/preferences/tests.rs`. Add `pub mod preferences;` to `lib.rs`.
- Modify `dispatch/src/main.rs` and `dispatch/src/app.rs`, adding `App::keep_preferences_in(dir)`.

**Interfaces:**
- Produces:
  - `Preferences`, `InterfacePrefs`, `AppearancePrefs` (as spec §4);
  - `ThemeChoice { Terminal, Dark, Light }`;
  - `Accent { Terminal, Violet, Blue, Teal, Green, Amber, Red, Pink, Custom(Rgb8) }`, where `Rgb8(u8, u8, u8)`;
  - `IconSet { Nerd, Plain }`;
  - `Source { BuiltIn, ConfigFile, Preferences }`;
  - `Sourced<T> { value: T, source: Source }`;
  - `Effective { motion, focus_follows_pointer, hover_claims_panes: Sourced<bool>, theme: Sourced<ThemeChoice>, accent: Sourced<Accent>, icons: Sourced<IconSet> }`;
  - `Section { Mouse, Appearance }`;
  - `ApplyError { Conflict(Vec<&'static str>), Unreadable(PathBuf), Io(ConfigError) }`;
  - `pub fn load(dir: &Path) -> Result<Preferences, ConfigError>`;
  - `pub fn load_or_default(dir: &Path) -> Preferences`, which logs and defaults;
  - `pub fn effective(config: &InterfaceConfig, interface_set: &InterfaceKeysPresent, prefs: &Preferences) -> Effective`;
  - `pub fn apply(dir: &Path, section: Section, base: &Preferences, draft: &Preferences) -> Result<(), ApplyError>`;
  - `pub const FILE: &str = "preferences.toml"`.

**Telling "set in config.toml" from "default".** `Config` fills defaults, so it cannot say whether a key was written. Add to `config.rs` a `pub struct InterfaceKeysPresent { pub motion: bool, pub focus_follows_pointer: bool, pub hover_claims_panes: bool }`, filled by `Config::load_reporting` from the parsed TOML table (whether `[interface]` holds each key), and returned in `LoadedConfig` as `interface_present`. Default all false. This is the provenance source for `ConfigFile`.

- [ ] **Step 1: Failing tests** (`preferences/tests.rs`):

```rust
use super::*;
use crate::testing::TempDir; // the crate's existing temp-dir helper; check its name

#[test]
fn absent_keys_inherit_and_present_keys_win() {
    let mut config = crate::InterfaceConfig::default();
    config.motion = false;
    let present = crate::InterfaceKeysPresent { motion: true, ..Default::default() };
    let mut prefs = Preferences::default();
    let effective = effective(&config, &present, &prefs);
    assert_eq!(effective.motion, Sourced { value: false, source: Source::ConfigFile });
    assert_eq!(effective.focus_follows_pointer.source, Source::BuiltIn);
    assert_eq!(effective.theme, Sourced { value: ThemeChoice::Terminal, source: Source::BuiltIn });

    prefs.interface.motion = Some(true);
    prefs.appearance.theme = Some(ThemeChoice::Light);
    let effective = effective(&config, &present, &prefs);
    assert_eq!(effective.motion, Sourced { value: true, source: Source::Preferences });
    assert_eq!(effective.theme.value, ThemeChoice::Light);
}

#[test]
fn values_round_trip_through_the_file() {
    let dir = TempDir::new("prefs-round-trip");
    let base = Preferences::default();
    let mut draft = base.clone();
    draft.appearance.accent = Some(Accent::Custom(Rgb8(0x12, 0x34, 0x56)));
    draft.appearance.icons = Some(IconSet::Plain);
    apply(dir.path(), Section::Appearance, &base, &draft).expect("applied");
    let text = std::fs::read_to_string(dir.path().join(FILE)).expect("written");
    assert!(text.contains("accent = \"#123456\""), "{text}");
    assert_eq!(load(dir.path()).expect("loads"), draft);
}

#[test]
fn one_bad_value_costs_only_itself() {
    let dir = TempDir::new("prefs-bad");
    std::fs::write(dir.path().join(FILE), "[appearance]\naccent = \"chartreuse\"\ntheme = \"light\"\nicons = 3\n").expect("written");
    let prefs = load_or_default(dir.path());
    assert_eq!(prefs.appearance.theme, Some(ThemeChoice::Light));
    assert_eq!(prefs.appearance.accent, None);
    assert_eq!(prefs.appearance.icons, None);
}

#[test]
fn applying_one_section_leaves_the_other_alone() {
    let dir = TempDir::new("prefs-sections");
    std::fs::write(dir.path().join(FILE), "[interface]\nhover_claims_panes = true\n").expect("written");
    let base = load(dir.path()).expect("loads");
    let mut draft = base.clone();
    draft.appearance.theme = Some(ThemeChoice::Dark);
    apply(dir.path(), Section::Appearance, &base, &draft).expect("applied");
    let now = load(dir.path()).expect("loads");
    assert_eq!(now.interface.hover_claims_panes, Some(true));
    assert_eq!(now.appearance.theme, Some(ThemeChoice::Dark));
}

#[test]
fn a_field_changed_on_disk_since_opening_is_a_conflict_and_nothing_is_written() {
    let dir = TempDir::new("prefs-conflict");
    let base = Preferences::default();
    // Another Dispatch sets the theme after Settings opened.
    let mut other = base.clone();
    other.appearance.theme = Some(ThemeChoice::Light);
    apply(dir.path(), Section::Appearance, &base, &other).expect("the other applied");

    let mut draft = base.clone();
    draft.appearance.theme = Some(ThemeChoice::Dark);
    draft.appearance.icons = Some(IconSet::Plain);
    let result = apply(dir.path(), Section::Appearance, &base, &draft);
    assert!(matches!(result, Err(ApplyError::Conflict(ref fields)) if fields == &vec!["theme"]), "{result:?}");
    let now = load(dir.path()).expect("loads");
    assert_eq!(now.appearance.theme, Some(ThemeChoice::Light), "theirs kept");
    assert_eq!(now.appearance.icons, None, "nothing of ours written");
}

#[test]
fn an_unreadable_file_is_never_replaced() {
    let dir = TempDir::new("prefs-unreadable");
    std::fs::write(dir.path().join(FILE), "this is [ not toml").expect("written");
    let draft = Preferences { appearance: AppearancePrefs { theme: Some(ThemeChoice::Dark), ..Default::default() }, ..Default::default() };
    let result = apply(dir.path(), Section::Appearance, &Preferences::default(), &draft);
    assert!(matches!(result, Err(ApplyError::Unreadable(_))), "{result:?}");
    assert_eq!(std::fs::read_to_string(dir.path().join(FILE)).expect("still there"), "this is [ not toml");
}

#[test]
fn resetting_a_field_removes_it_from_the_file() {
    let dir = TempDir::new("prefs-reset");
    std::fs::write(dir.path().join(FILE), "[appearance]\ntheme = \"dark\"\n").expect("written");
    let base = load(dir.path()).expect("loads");
    let mut draft = base.clone();
    draft.appearance.theme = None;
    apply(dir.path(), Section::Appearance, &base, &draft).expect("applied");
    assert!(!std::fs::read_to_string(dir.path().join(FILE)).expect("there").contains("theme"));
}
```

Also add `config.rs` tests: `interface_present` is true for keys written under `[interface]` and false otherwise.

Run `cargo test -p dispatch-config preferences`. Expected: FAIL to compile.

- [ ] **Step 2: Implement `preferences.rs`.**
- **Serde.**
  - `ThemeChoice` and `IconSet` use `#[serde(rename_all = "lowercase")]`.
  - `Accent` serialises as a string: `"terminal"`, a preset name in lower case, or `"#rrggbb"`. Hand-write `Serialize` and `Deserialize` through a `String`. Parse `#` + 6 hex digits, case-insensitive, and reject anything else.
- **Per-field tolerance.** Do not deserialise into `Preferences` directly. Parse the file into a `toml::Table`. For each known field, try to convert its value to the field's type. On failure, `tracing::warn!(field, "ignoring a preference that cannot be read")` and leave it `None`. `load` returns `Err` only when the TOML itself does not parse.
- **`apply`.** Run it inside `store::update(dir, FILE, |table: &mut toml::Table| …)`. If `store` is typed over `T: Default + Serialize + DeserializeOwned`, a `toml::Table` qualifies. An unparseable file makes `store::read` return `ConfigError::Toml`; map it to `ApplyError::Unreadable(path)`. For each field of `section` where `base != draft`:
  - read the table's current value of that field, under the same tolerance;
  - if it is not `base`'s value, collect the field name as a conflict.

  With any conflicts, return `Ok(((), false))` from the closure, so nothing is written, then return `Err(Conflict)`. Otherwise set or remove exactly those keys, and return `changed = true`.
- **Sections.**
  - `Mouse` covers `interface.focus_follows_pointer` and `interface.hover_claims_panes`.
  - `Appearance` covers `appearance.theme`, `appearance.accent`, `appearance.icons` and `interface.motion`, the motion switch the spec puts under Appearance.
- **App and start-up.**
  - Add `preferences_dir: Option<PathBuf>` and `fn keep_preferences_in(&mut self, dir)` to `App`, following `keep_ui_in`.
  - Store `self.preferences: Preferences` and `self.interface_config: InterfaceConfig` / `interface_present` for Settings to compute provenance.
  - In `main.rs`, after loading config: `app.keep_preferences_in(&config_dir);`.
  - Compute `effective(...)`, and call `set_motion`, `set_focus_follows_pointer` and `set_hover_claims_panes` with the effective values, in place of the direct `loaded.config.interface.*` calls.
  - Keep the appearance values for Task 7.

- [ ] **Step 3: Run the tests, gate, and commit.**

```bash
git add crates/dispatch-config/src/preferences.rs crates/dispatch-config/src/preferences/tests.rs crates/dispatch-config/src/lib.rs crates/dispatch-config/src/config.rs crates/dispatch-config/src/tests.rs dispatch/src/main.rs dispatch/src/app.rs
git commit -m "feat(config): preferences.toml, with sources, per-section apply and conflict detection"
```

---

### Task 6: Glyphs, with a plain set

**Files:**
- Create `crates/dispatch-tui/src/glyphs.rs` and `crates/dispatch-tui/src/glyphs/tests.rs`. Add `pub mod glyphs;` to `lib.rs`.
- Modify `crates/dispatch-tui/src/sidebar.rs` (and tests), `dispatch/src/app.rs` (tab chips, sidebar construction, `App.glyphs`).

**Interfaces:**
- Produces:
  - `pub struct Glyphs { pub starting, running, idle, blocked, done, failed, closed, unseen, open, shut, leaf, repository, folder, default_icon: &'static str, pub nerd: bool }`;
  - `Glyphs::NERD` and `Glyphs::PLAIN`;
  - `Glyphs::harness_icon(&self, def: Option<&HarnessDef>, display: &str) -> String`;
  - `Sidebar::with_glyphs(self, glyphs: &'a Glyphs)`;
  - `Rollup::glyph_with(self, spinner, theme, glyphs)`, with the existing `glyph` delegating to `NERD`;
  - `App.glyphs: &'static Glyphs` (default `&Glyphs::NERD`) and `App::set_glyphs(&mut self, &'static Glyphs)`.

- [ ] **Step 1: Failing tests** (`glyphs/tests.rs` and a sidebar test):

```rust
use super::*;

#[test]
fn the_plain_set_is_ascii_only() {
    let plain = &Glyphs::PLAIN;
    for glyph in [plain.starting, plain.running, plain.idle, plain.blocked, plain.done, plain.failed,
                  plain.closed, plain.unseen, plain.open, plain.shut, plain.leaf, plain.repository,
                  plain.folder, plain.default_icon] {
        assert!(glyph.is_ascii(), "{glyph:?}");
        assert_eq!(glyph.chars().count(), 1, "{glyph:?}");
    }
}

#[test]
fn the_nerd_set_is_what_the_sidebar_drew_before() {
    assert_eq!(Glyphs::NERD.blocked, crate::sidebar::BLOCKED);
    assert_eq!(Glyphs::NERD.repository, crate::sidebar::REPOSITORY);
}

#[test]
fn a_plain_harness_icon_is_its_first_letter() {
    assert_eq!(Glyphs::PLAIN.harness_icon(None, "claude code"), "C");
    assert_eq!(Glyphs::PLAIN.harness_icon(None, ""), Glyphs::PLAIN.default_icon);
}
```

In `sidebar/tests.rs`, add `a_plain_sidebar_draws_no_private_use_glyphs`: render the existing two-project state with a folded project, a pane of each status and a subagent, using `.with_glyphs(&Glyphs::PLAIN)`. Assert that no cell's symbol contains a char in `'\u{e000}'..='\u{f8ff}'`.

Run `cargo test -p dispatch-tui glyph plain`. Expected: FAIL to compile.

- [ ] **Step 2: Implement.**
- **`Glyphs::NERD`** takes its values from the existing `sidebar.rs` consts, which stay: `STARTING`, `RUNNING`, `IDLE`, `BLOCKED`, `DONE`, `FAILED`, `CLOSED`, `UNSEEN`, the private `OPEN`/`SHUT`/`LEAF` (make them `pub(crate)`), `REPOSITORY`, `SHUT_FOLDER`, and `dispatch_config::harness::DEFAULT_ICON`.
- **`PLAIN`** uses the constants table's values. Its `default_icon` is `"*"`.
- **`harness_icon`.** With `nerd: true`, return `def.map_or(DEFAULT_ICON, HarnessDef::icon)`. With `nerd: false`, return the display name's first char upper-cased, or `default_icon` when the name is empty.
- **Sidebar.** Add a `glyphs: &'a Glyphs` field, default `&Glyphs::NERD`. Route every glyph read in `state_glyph`, `twisty`, `source_icon`, `icon` and `Rollup::glyph` through it. Thread it as a parameter where those are free functions.
- **The spinner** frames stay braille in both sets. The still running glyph, with motion off, comes from the set.
- **App.** `draw_sidebar` passes `.with_glyphs(self.glyphs)`. `draw_tabs` uses `rollup.glyph_with(spinner, &self.theme, self.glyphs)`.

- [ ] **Step 3: Run the tests, gate, and commit.**

```bash
git add crates/dispatch-tui/src/glyphs.rs crates/dispatch-tui/src/glyphs/tests.rs crates/dispatch-tui/src/lib.rs crates/dispatch-tui/src/sidebar.rs crates/dispatch-tui/src/sidebar/tests.rs dispatch/src/app.rs
git commit -m "feat(tui): one glyph table, with a plain ASCII set beside the Nerd Font one"
```

---

### Task 7: Theme presets, accents, and appearance at start-up

**Files:** Modify `crates/dispatch-tui/src/theme.rs` (+tests), `dispatch/src/app.rs`, `dispatch/src/main.rs`.

**Interfaces:**
- Consumes: T5's `ThemeChoice`, `Accent`, `IconSet`, `Effective`; T6's `Glyphs`.
- Produces:
  - `Palette::LIGHT`;
  - `pub fn accent_rgb(accent: &Accent, terminal: Rgb) -> Rgb` in `theme.rs`, which takes `dispatch_config::preferences::Accent`. Add `dispatch-config` as a dependency of `dispatch-tui` if it is not already one. It is: `sidebar.rs` uses `dispatch_config::harness`.
  - `Theme::palette(&self) -> Palette` and `Theme::depth(&self) -> Depth`;
  - `App.terminal_palette: Palette`;
  - `App::apply_appearance(&mut self, theme: ThemeChoice, accent: &Accent, icons: IconSet)`.

- [ ] **Step 1: Failing tests.**

```rust
#[test]
fn the_light_palette_reads_well() {
    let theme = Theme::new(Palette::LIGHT, Depth::TrueColor);
    assert!(contrast(theme.rgb(Role::Faded), Palette::LIGHT.background) >= READABLE);
}

#[test]
fn an_accent_replaces_the_palettes() {
    use dispatch_config::preferences::{Accent, Rgb8};
    let terminal = Rgb(1, 2, 3);
    assert_eq!(accent_rgb(&Accent::Terminal, terminal), terminal);
    assert_eq!(accent_rgb(&Accent::Blue, terminal), Rgb(0x61, 0xaf, 0xef));
    assert_eq!(accent_rgb(&Accent::Custom(Rgb8(9, 8, 7)), terminal), Rgb(9, 8, 7));
}
```

App test, `appearance_rebuilds_the_theme_and_glyphs`:
- `app.terminal_palette = Palette::FALLBACK`;
- `apply_appearance(ThemeChoice::Light, &Accent::Red, IconSet::Plain)`;
- assert `app.theme == Theme::new(Palette { accent: Rgb(0xe0,0x6c,0x75), ..Palette::LIGHT }, depth)` and `std::ptr::eq(app.glyphs, &Glyphs::PLAIN)`.

Run `cargo test --workspace accent light appearance`. Expected: FAIL.

- [ ] **Step 2: Implement.**
- `Palette::LIGHT` uses the constants' values.
- `accent_rgb` matches the preset table.
- `Theme::palette()` and `depth()` return the private fields.
- **`apply_appearance`:**
  1. `base = match theme { Terminal => self.terminal_palette, Dark => Palette::FALLBACK, Light => Palette::LIGHT }`;
  2. `palette = Palette { accent: accent_rgb(accent, base.accent), ..base }`;
  3. `self.theme = Theme::new(palette, self.theme.depth())`;
  4. `self.glyphs = if icons == IconSet::Plain { &Glyphs::PLAIN } else { &Glyphs::NERD }`.
- **`set_theme`** also stores `self.terminal_palette = theme.palette()`, so the palette the terminal reported is kept.
- **`main.rs`**, after `app.set_theme(guard.theme())`: `app.apply_appearance(effective.theme.value, &effective.accent.value, effective.icons.value);`.

- [ ] **Step 3: Run the tests, gate, and commit.**

```bash
git add crates/dispatch-tui/src/theme.rs crates/dispatch-tui/src/theme/tests.rs dispatch/src/app.rs dispatch/src/main.rs
git commit -m "feat(tui): light and dark themes, accent presets, and appearance from preferences"
```

---

### Task 8: The `SettingsView` widget

**Files:**
- Create `crates/dispatch-tui/src/settings_view.rs` and `crates/dispatch-tui/src/settings_view/tests.rs`. Add `pub mod settings_view;` to `lib.rs`.
- Modify `crates/dispatch-tui/src/button.rs` (new ids) and `dispatch/src/pointer.rs` (new `DialogHit` variants).

**Interfaces:**
- Produces:
  - `FieldKind { Toggle, Choice(Vec<String>), Number { min: i64, max: i64 }, Text, Action(ButtonId), ReadOnly }`;
  - `FieldView { pub id: &'static str, pub label: String, pub description: String, pub category: usize, pub kind: FieldKind, pub value: String, pub source: String, pub applies: &'static str, pub changed: bool, pub conflict: bool }`;
  - `Focus { Search, Categories, Fields, Buttons }`;
  - `SettingsView`, with:
    - `new(categories: Vec<String>, fields: Vec<FieldView>)`;
    - `set_chrome`, `set_fields(Vec<FieldView>)` (rebuilt by the app after each edit), `set_pending(Option<String>)` (the footer text, e.g. "2 changes in Appearance");
    - `set_prompt(Option<Vec<ButtonId>>)` (the leave-with-edits prompt);
    - `category()`, `select_category(usize)`;
    - `selected_field() -> Option<&FieldView>` (in the current category, or among search results), `select_field(index)`;
    - `focus()`, `focus_next()`, `focus_previous()`, `move_up()`, `move_down()`;
    - `search()`, `push_search(char)`, `pop_search()`, `clear_search()`;
    - `scroll_by(i32)`, `set_hovered(Option<usize>)`, `set_pressed(Option<ButtonId>)`;
    - `layout(area: Rect) -> SettingsLayout`;
    - `impl Widget for &SettingsView`.
  - `SettingsLayout { pub rect: Rect, pub too_small: bool, pub compact: bool, pub close: Rect, pub search: Rect, pub categories: Vec<(Rect, usize)>, pub fields: Vec<(Rect, usize)>, pub steps: Vec<(Rect, usize, bool)>, pub buttons: Vec<(Rect, ButtonId)> }`. `fields` indices are positions in the *shown* list.
  - `ButtonId::{Apply, Discard, KeepEditing, ClearSearch, ResetSidebar}`.
  - `DialogHit::{Category(usize), Field(usize), FieldStep(usize, bool), Search, Close}`.

- [ ] **Step 1: Failing tests** (`settings_view/tests.rs`):

```rust
use super::*;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

fn view() -> SettingsView {
    let field = |id, label: &str, category| FieldView {
        id, label: label.into(), description: format!("about {label}"), category,
        kind: FieldKind::Choice(vec!["One".into(), "Two".into()]), value: "One".into(),
        source: "Built-in".into(), applies: "Applies now", changed: false, conflict: false,
    };
    SettingsView::new(
        vec!["Mouse & layout".into(), "Appearance".into(), "Advanced".into()],
        vec![field("focus", "Focus follows pointer", 0), field("theme", "Theme", 1), field("icons", "Icons", 1)],
    )
}

fn draw(view: &SettingsView, w: u16, h: u16) -> (Buffer, SettingsLayout) {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    (buf, view.layout(area))
}

#[test]
fn a_large_window_centres_a_capped_box() {
    let (_, layout) = draw(&view(), 160, 50);
    assert_eq!((layout.rect.width, layout.rect.height), (110, 34));
    assert_eq!(layout.rect.x, (160 - 110) / 2);
    assert!(!layout.compact && !layout.too_small);
}

#[test]
fn a_smaller_window_uses_all_of_it_and_a_narrow_one_turns_compact() {
    let (_, layout) = draw(&view(), 90, 24);
    assert_eq!(layout.rect, Rect::new(0, 0, 90, 24));
    let (_, layout) = draw(&view(), 60, 18);
    assert!(layout.compact, "under 72 columns the categories become a picker");
}

#[test]
fn a_tiny_window_shows_only_how_to_close() {
    let (buf, layout) = draw(&view(), 30, 8);
    assert!(layout.too_small);
    assert!(!layout.close.is_empty(), "the close control is always there");
    let text: String = buf.content().iter().map(|c| c.symbol()).collect();
    assert!(text.contains("larger"), "{text}");
}

#[test]
fn it_never_panics_and_close_is_always_reachable() {
    for (w, h) in [(120, 40), (100, 28), (80, 24), (60, 18), (40, 10), (20, 6), (1, 1)] {
        let (_, layout) = draw(&view(), w, h);
        if w >= 4 && h >= 3 {
            assert!(!layout.close.is_empty(), "{w}x{h}");
        }
    }
}

#[test]
fn search_finds_across_categories_and_offers_clear_when_empty() {
    let mut view = view();
    for c in "icon".chars() { view.push_search(c); }
    assert_eq!(view.selected_field().map(|f| f.id), Some("icons"));
    for c in "zzz".chars() { view.push_search(c); }
    let (buf, layout) = draw(&view, 120, 40);
    let text: String = buf.content().iter().map(|c| c.symbol()).collect();
    assert!(text.contains("No settings match"));
    assert!(layout.buttons.iter().any(|(_, id)| *id == ButtonId::ClearSearch));
}

#[test]
fn tab_walks_search_categories_fields_buttons_and_back() {
    let mut view = view();
    assert_eq!(view.focus(), Focus::Categories);
    view.focus_next(); assert_eq!(view.focus(), Focus::Fields);
    view.focus_next(); assert_eq!(view.focus(), Focus::Buttons);
    view.focus_next(); assert_eq!(view.focus(), Focus::Search);
    view.focus_previous(); assert_eq!(view.focus(), Focus::Buttons);
}

#[test]
fn the_layout_matches_what_is_drawn() {
    let view = view();
    let (buf, layout) = draw(&view, 120, 40);
    let row_text = |r: Rect| (r.x..r.x + r.width).map(|x| buf.cell((x, r.y)).map_or("", |c| c.symbol())).collect::<String>();
    let (rect, index) = layout.categories[1];
    assert!(row_text(rect).contains("Appearance"), "category {index}");
    let (rect, _) = layout.fields[0];
    assert!(row_text(rect).contains("Focus follows pointer"));
    assert!(layout.buttons.iter().all(|(r, _)| row_text(*r).contains('[')));
}
```

Run `cargo test -p dispatch-tui settings_view`. Expected: FAIL to compile.

- [ ] **Step 2: Implement `settings_view.rs`.** Follow spec §5's drawing. Rules:
- **`layout`** is the single source of truth, and `render` calls it.
  - The box sizing follows the constants.
  - The title row is ` Settings `, with `[×]` at the top-right inside the frame. `close` is that 3-cell rect.
  - The search row is the first inner row: `Search settings…` placeholder, or `> {query}▏` when focused or filled.
  - Then a horizontal rule. Then the category column (width = longest category name + 4) and the field column.
  - The last inner row is the footer: the pending text on the left, buttons on the right through `button::lay_out`.
  - The footer buttons are `[Discard] [Apply]` while `pending` is set, `[Clear search]` while search has no result, and the prompt's buttons while a prompt is open.
- **Compact mode:**
  - The category column is replaced by a single row `‹ Appearance ›` at the top of the field column. Its arrows are `steps` on index `usize::MAX`, so the app can tell them apart. `categories` holds the one rect.
  - A field page has a `‹ Back` at the start of the search row.
- **Field rows:**
  - label in `chrome.text`;
  - value as `‹ {value} ›` for Toggle, Choice and Number (whose arrows are the `steps` rects), `{value}▏` for Text while being edited, `[ {label} ]` for Action, and plain for ReadOnly;
  - source right-aligned in `chrome.secondary`;
  - a changed field marked `•` before its label;
  - a conflict marked `!` in `Color::Red`, with "changed elsewhere" as its source text.

  Below the selected field, one line shows `{description} · {applies}` in `chrome.secondary`.
- **Styles.** The selected field is in `chrome.selection` when `focus == Fields`, and underlined otherwise. Hover is underlined `chrome.text`, never a bar.
- **Scrolling** is persistent, as in the picker: a `Cell<usize>` offset that moves only when the selection leaves the visible rows.
- **Search results** list matches (label, description, category name, case-insensitive) as `label  (Category)`. `selected_field` indexes into them.

- [ ] **Step 3: Run the tests, gate, and commit.**

```bash
git add crates/dispatch-tui/src/settings_view.rs crates/dispatch-tui/src/settings_view/tests.rs crates/dispatch-tui/src/lib.rs crates/dispatch-tui/src/button.rs dispatch/src/pointer.rs
git commit -m "feat(tui): a Settings view with categories, search, fields and one shared layout"
```

---

### Task 9: The Settings workspace in the app (with Appearance)

**Files:**
- Create `dispatch/src/app/settings.rs`, and declare `mod settings;` in `app.rs`.
- Modify `crates/dispatch-tui/src/{keymap,input}.rs` (`Command::Settings`, `Action::Settings`), `dispatch/src/app.rs` (the `Overlay::Preferences` variant and hooks), `dispatch/src/app/pointer_routing.rs` (Settings hits), `dispatch/src/app/footer.rs` (`[Settings]` opens it).

**Interfaces:**
- Consumes: T5's `preferences::{apply, effective, Preferences, Section, ApplyError}`; T7's `apply_appearance`; T8's `SettingsView`.
- Produces:
  - `Command::Settings` (`settings`, label `settings`, describe `"Open Settings"`): `Alt ,` in normal mode, `,` in session mode and the prefix;
  - `Overlay::Preferences(SettingsWorkspace)` (tag `"preferences"`);
  - `struct SettingsWorkspace { view: SettingsView, base: Preferences, draft: Preferences, conflicts: Vec<&'static str> }`;
  - `App::{open_settings_workspace, settings_key, settings_pointer, apply_settings, discard_settings}`;
  - `App.settings_memory: (usize, usize)`, the category and field to reopen on.

- [ ] **Step 1: Failing tests** (app tests). They cover the spec's acceptance checks with the Appearance category.

```rust
    fn with_preferences(app: &mut App) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dispatch-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("a directory");
        app.keep_preferences_in(&dir);
        dir
    }

    #[test]
    fn mouse_only_a_theme_change_is_applied_and_kept() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        let dir = with_preferences(&mut app);
        let mut terminal = a_wide_terminal();
        drawn(&mut app, &mut terminal);
        // [Settings] on the footer
        let row = bottom_row(&terminal);
        let x = u16::try_from(row.find("[ Settings ]").expect("drawn")).expect("a column") + 2;
        click(&mut app, x, terminal.backend().buffer().area.height - 1);
        drawn(&mut app, &mut terminal);
        let layout = app.settings_layout().expect("Settings is open");
        let appearance = layout.categories.iter().find(|(_, i)| *i == 1).expect("Appearance").0;
        click(&mut app, appearance.x + 1, appearance.y);
        drawn(&mut app, &mut terminal);
        let layout = app.settings_layout().expect("open");
        // Theme is the first Appearance field; its › is its forward step. Terminal → Dark → Light.
        for _ in 0..2 {
            let step = layout.steps.iter().find(|(_, i, forward)| *i == 0 && *forward).expect("a step").0;
            click(&mut app, step.x, step.y);
            drawn(&mut app, &mut terminal);
        }
        let apply = app.settings_layout().expect("open").buttons.iter()
            .find(|(_, id)| *id == ButtonId::Apply).expect("Apply").0;
        click(&mut app, apply.x + 1, apply.y);
        drawn(&mut app, &mut terminal);
        let close = app.settings_layout().expect("open").close;
        click(&mut app, close.x + 1, close.y);

        assert!(app.overlay.is_none());
        let saved = dispatch_config::preferences::load(&dir).expect("loads");
        assert_eq!(saved.appearance.theme, Some(dispatch_config::preferences::ThemeChoice::Light));
        assert_eq!(app.theme.palette().background, dispatch_tui::theme::Palette::LIGHT.background);
    }

    #[test]
    fn keyboard_only_the_same_change() {
        // Alt , opens it; Down selects Appearance; Tab to Fields; Right twice
        // (Terminal → Dark → Light); Tab to Buttons; Enter on Apply (Apply is
        // the default button); Esc closes. Same assertions as above.
    }

    #[test]
    fn discard_puts_the_previewed_theme_back() {
        // Open, Appearance, step Theme to Dark: app.theme is the dark one
        // (preview). Click [Discard]: app.theme is what it was before opening,
        // and the file is untouched (no preferences.toml written).
    }

    #[test]
    fn closing_with_unapplied_changes_asks_and_discard_restores() {
        // Step Theme to Light, press Esc: a prompt with Apply / Discard / Keep
        // editing shows (layout.buttons has the three); click Discard: the
        // overlay closes and app.theme is back.
    }

    #[test]
    fn a_conflict_is_shown_and_nothing_is_written() {
        // Open Settings (base snapshot taken), then write preferences.toml
        // behind its back with theme = "dark", step Theme in the workspace,
        // Apply: the status is an Error naming the conflict, the Theme field
        // shows conflict = true, and the file still says "dark".
    }

    #[test]
    fn settings_reopens_where_it_was() {
        // Open, select Appearance, close; reopen: Appearance is selected.
    }

    #[test]
    fn nothing_reaches_a_child_while_settings_is_open() {
        // Mouse-tracking focused pane; open Settings; type letters (search),
        // click inside and outside the workspace, paste: no WritePane for the pane.
    }
```

Write the sketched tests in full. Use `settings_layout()`, a new `pub(crate) fn settings_layout(&self) -> Option<SettingsLayout>`, rather than hard-coded coordinates. If `uuid` is not available in tests, use the temp-dir pattern the existing tests use.

Run `cargo test -p dispatch settings`. Expected: FAIL.

- [ ] **Step 2: Implement `app/settings.rs`.**
- **`open_settings_workspace`:**
  - load `base = preferences::load_or_default(dir)`, with an empty `Preferences` when this client keeps none (in tests);
  - set `draft = base.clone()`;
  - build fields with `fields(&self, &draft) -> Vec<FieldView>`, which uses `effective(&self.interface_config, &self.interface_present, &draft)` for values and sources, and the base snapshot for `changed`;
  - restore `settings_memory`;
  - set `self.overlay = Some(Overlay::Preferences(..))`.

  Source labels: `Built-in`, `config.toml` and `Settings`.
- **Appearance fields:**
  - Theme: Choice of Follow terminal, Dark, Light;
  - Accent: Choice of Terminal, Violet, …, Pink, Custom…;
  - Custom accent: Text, present only when Accent is Custom;
  - Motion: Toggle;
  - Icons: Choice of Nerd Font, Plain.

  Every Appearance field applies "Applies now". Mouse & layout and Advanced are filled in by T10; until then their categories show their fields as empty lists.
- **Editing.** `step(field, forward)` and `toggle(field)` change `draft`. Each edit calls `self.preview()`, which runs `apply_appearance` from `draft`'s effective appearance and `set_motion` from its motion, rebuilds the fields, and updates `pending` to `"{n} changes in {category}"` (n counts the fields of the category's section where draft ≠ base).
- **Apply.** `preferences::apply(dir, section, &base, &draft)`:
  - `Ok`: set `base = draft.clone()`, rebuild the fields, and `say("Settings applied")`.
  - `Conflict(fields)`: mark them on the view and `warn("changed elsewhere since you opened Settings: …")`.
  - `Unreadable(path)`: `warn` naming the path.
  - `Io`: `warn` with the error.
- **Discard.** Set `draft = base.clone()`, call `preview()` (which restores the committed theme, glyphs and motion), and rebuild the fields.
- **Leaving with edits.** When the category changes or Settings is closed (Esc or `[×]`) while `pending` is set, call `view.set_prompt(Some(vec![KeepEditing, Discard, Apply]))`. Its buttons do what they say, then finish the leave.
- **Keys** (`settings_key`):
  - Tab and Shift-Tab (`BackTab`) call `focus_next` / `focus_previous`.
  - **Search focus:** chars go to the search, Backspace edits it, and Down moves to fields.
  - **Categories:** Up and Down select a category (asking first with edits pending), and Enter or Right moves to Fields.
  - **Fields:** Up and Down select, Left and Right step, Enter toggles or activates an Action, and Text fields edit inline (chars, Backspace, Enter confirm, Esc cancel).
  - **Buttons:** Left and Right move, and Enter presses.
  - **Esc** closes the innermost thing first: an inline edit, then search focus, then the workspace (asking with edits pending).
  - Chords with Ctrl or Alt are ignored.
  - Nothing reaches a child.
- **Mouse** (`settings_pointer`, called from `pointer_routing.rs` when the overlay is `Preferences`):
  - Push hit entries from `settings_layout()` in `draw`: `Dialog(Area)`, then `Category(i)`, `Field(i)`, `FieldStep(i, fwd)`, `Search`, `Close` and `Button(id)`.
  - A press on Category, Field or Search selects or focuses at once. Steps, buttons and Close act on release inside, through the gesture path.
  - The wheel scrolls the field list.
  - A Down outside is consumed and does nothing.
  - Hover goes through `view.set_hovered`.
- **`Command::Settings`.** Add it to the keymap (with the defaults above, and the session `mode_help` test string gaining `  , settings`), `Action::Settings`, and `perform`. The footer's `[Settings]` calls `open_settings_workspace`.
- **The help list** gains Settings automatically, through `NAMED` and `describe`.

- [ ] **Step 3: Run the tests, gate, and commit.**

```bash
git add dispatch/src/app/settings.rs dispatch/src/app.rs dispatch/src/app/pointer_routing.rs dispatch/src/app/footer.rs crates/dispatch-tui/src/keymap.rs crates/dispatch-tui/src/keymap/tests.rs crates/dispatch-tui/src/input.rs
git commit -m "feat(tui): a Settings workspace, with Appearance previewed live and applied per section"
```

---

### Task 10: Mouse & layout and Advanced categories, and docs

**Files:** Modify `dispatch/src/app/settings.rs`, `docs/usage.md`, `docs/configuration.md`.

**Interfaces:**
- Consumes: T9's workspace; `toggle_sidebar`, `resize_sidebar`, `save_ui`, `set_focus_follows_pointer`, `set_hover_claims_panes`, and the `dispatch_os::paths` functions.

- [ ] **Step 1: Failing tests.**

```rust
    #[test]
    fn mouse_and_layout_applies_focus_and_hover_to_preferences() {
        // Open Settings, Mouse & layout, toggle "Focus follows pointer" (on),
        // Apply: preferences.toml has focus_follows_pointer = true and a
        // Moved event over a pane now focuses it.
    }

    #[test]
    fn sidebar_fields_act_at_once_and_are_not_pending() {
        // Step "Sidebar width" forward: app.sidebar_width grew by 2, ui.toml has
        // it (with keep_ui_in), and the footer pending text is still None.
        // Press [Reset sidebar width]: back to DEFAULT_SIDEBAR.
    }

    #[test]
    fn advanced_shows_paths_read_only() {
        // Advanced lists "Configuration file", "Preferences", "ui.toml",
        // "Harnesses", "Log file", "Version", "Mode"; stepping or Enter on any
        // changes nothing (no pending text, no file written).
    }
```

Write all three in full.

- [ ] **Step 2: Implement.**
- **Mouse & layout fields:**
  - Focus follows pointer and Hover claims shared panes: Toggles, section `Mouse`, `Applies now`.
  - Sidebar width: `Number { min: 20, max: 60 }`, value `self.sidebar_width`, source `ui.toml`. Stepping calls `resize_sidebar(±2)`.
  - Show sidebar: Toggle of `!self.sidebar_collapsed`. Toggling calls `toggle_sidebar`. In a narrow window this toggles the drawer, which matches what the key does.
  - Reset sidebar width: `Action(ButtonId::ResetSidebar)`, which sets `DEFAULT_SIDEBAR` and saves.

  The three sidebar fields are never counted in `pending`.
- **Apply for Mouse** calls `preferences::apply(.., Section::Mouse, ..)`, then `set_focus_follows_pointer` and `set_hover_claims_panes` from the new effective values.
- **Advanced fields** are `ReadOnly`, with value and source as below; `applies` is `Read only`:
  - Configuration file: `paths::config_file()`;
  - Preferences: `{config_dir}/preferences.toml`;
  - ui.toml: `{config_dir}/ui.toml`;
  - Harnesses: `paths::harnesses_dir()`;
  - Log file: `paths::log_file()`;
  - Version: `env!("CARGO_PKG_VERSION")`;
  - Mode: `Standalone`, or `Attached to {n} machine(s)`.

  A path that fails to resolve shows `unknown`.
- **Docs:**
  - `docs/usage.md`: a "Settings" section (opening it, categories, Apply and Discard, preview, search) and the footer and Activity text if not done.
  - `docs/configuration.md`: `preferences.toml`, its keys, the precedence (built-in < config.toml < preferences.toml), that Settings writes it and `config.toml` is never rewritten, and that a bad value costs only itself.

- [ ] **Step 3: Run the tests, gate, and commit.**

```bash
git add dispatch/src/app/settings.rs docs/usage.md docs/configuration.md
git commit -m "feat(tui): Mouse & layout and Advanced in Settings, and the docs for preferences.toml"
```

---

## After the last task

- [ ] Full gate on a clean tree.
- [ ] `cargo build --release`, then check by hand:
  - the footer in each mode;
  - Settings at a big and a small window size;
  - a Light theme preview, then Discard and Apply;
  - Plain icons;
  - Activity.
- [ ] Update the `open-follow-ups.md` memory with anything parked, and note C, D and E as next.
