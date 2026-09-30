# TUI Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Seven interface fixes on branch `TUI`: continuous focus glide, a way to reach blocked agents, room for small windows, one set of dialog colours, discoverable commands, spinner redraws only when a spinner is seen, and readable state words and secondary text.

**Architecture:** Everything is client-side. Shared pieces go into `crates/dispatch-tui`: `theme::Chrome`, `sidebar::Origin`, `sidebar::status_text`, `sidebar::spins`, and a filtering `Picker`. The one config change goes into `crates/dispatch-config`: the rule `reason` field and a new `ui_state` module. `dispatch/src/app.rs` wires the new pieces into layout, input, overlays and the status row. No protocol or daemon change.

**Tech Stack:** Rust 2024 (MSRV 1.89), ratatui, crossterm, serde/toml, tracing.

**Spec:** `docs/superpowers/specs/2026-09-30-tui-polish-design.md`. Read it before starting a task: this plan argues from it.

## Global Constraints

- Branch `TUI`. One commit per task, at minimum. Commit messages follow the repo's Conventional Commits style (`feat(tui): …`, `fix(tui): …`), ending with the session's attribution lines.
- Every task ends with all of these green:
  `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace --no-fail-fast`.
- MSRV 1.89: no API newer than that.
- Comments match the surrounding code: full sentences that say *why*, in British spelling (`colour`, `centre`), with no "we".
- The ease stays `EASE` (150 ms). Spinner and tween rates (`SPIN_FRAME`, `TWEEN_FRAME`) do not change.
- Nothing writes `config.toml`. New state goes in `ui.toml`, beside `projects.toml`, through `store::update`.
- No existing key is rebound. New defaults: `Alt a` next attention, `Alt /` help, `Alt s` sidebar. Prefix: `w`, `?`, `b`. Session: `w` (attention picker), `?`, `b`, `<`, `>`.
- The status-row reminder names keys through `keymap.path_to(…)`, and leaves the key out when nothing is bound to it, as the delegation reminder does today.

## Review Focus

1. **A blocked pane that exits or unblocks while the attention picker is open.** Enter on its row must do nothing harmful: no panic, no focus on a closed pane. The test goes in Task 6.
2. **A hand-edited `ui.toml`** with `sidebar_width = 500`, `= 3`, `= "wide"` or `= -1`. It loads clamped or defaulted, never fails the start. The test goes in Task 7.
3. **A window widened past 80 columns while the drawer is open.** The drawer closes, the docked sidebar comes back as it was, and nothing is drawn twice. The test goes in Task 7.
4. **Help filter with no match.** "nothing to choose" is shown, and Enter does nothing and keeps the help open. The test goes in Task 8.
5. **A window narrower than the sidebar's minimum** (for example 12 columns). No draw path panics, the drawer included. The test goes in Task 7.

---

## File map

| File | What changes |
|---|---|
| `crates/dispatch-tui/src/picker.rs` (+`picker/tests.rs`) | width clamp fix (T1); `Chrome` (T2); filter + `desired_width` (T7, T8) |
| `crates/dispatch-tui/src/theme.rs` (+tests) | `Chrome` (T2); `contrast`, readable `faded` (T5) |
| `crates/dispatch-tui/src/prompt.rs`, `browser.rs`, `settings_form.rs` (+tests) | `set_chrome` (T2); `desired_width` (T7) |
| `dispatch/src/approval.rs` | chrome instead of fixed greys (T2) |
| `crates/dispatch-tui/src/sidebar.rs` (+tests) | `Origin` glide (T3); `spins` (T4); `status_text` (T5) |
| `dispatch/src/frame_stats.rs` (new) | frame timing ring buffer (T4) |
| `crates/dispatch-config/src/status.rs`, `status/builtin.rs` (+tests) | rule `reason`, `RuleMatch` (T6) |
| `crates/dispatch-tui/src/activity.rs` (+tests) | tracker keeps the blocked reason (T6) |
| `crates/dispatch-tui/src/keymap.rs`, `input.rs` (+tests) | new commands and actions (T6, T7, T8); `Command::describe` (T8) |
| `crates/dispatch-config/src/ui_state.rs` (new, +tests) | `ui.toml` (T7) |
| `dispatch/src/app.rs` | wiring for every task |
| `dispatch/src/main.rs` | `keep_ui_in` (T7) |
| `docs/usage.md`, `docs/configuration.md` | new keys, `reason`, `ui.toml` (T6, T7, T8) |

Test helpers already in `dispatch/src/app.rs`'s `mod tests`, which are used below: `attached_app()`, `spawn_several()`, `hand_clock()`, `advance()`, `drawn()`, `rendered_text()`, `press()`, `click()`, `rebind()`, `bottom_row()`, `top_row()`, `a_wide_terminal()`, `status_of()`, `sidebar_column()`. Read each one before using it.

---

### Task 1: Picker width clamp cannot panic

**Files:**
- Modify: `crates/dispatch-tui/src/picker.rs:167-169`
- Test: `crates/dispatch-tui/src/picker/tests.rs`

**Interfaces:** Consumes nothing. Produces nothing new.

- [ ] **Step 1: Write the failing test** (append to `picker/tests.rs`)

```rust
#[test]
fn a_picker_draws_at_any_size_without_panicking() {
    // `clamp(20, width)` panics when the width is under 20, and the early
    // return only covered widths under 4.
    let picker = picker().with_hint("↑↓ choose  Enter open");
    for width in 1..=25 {
        for height in 1..=6 {
            render(&picker, width, height);
        }
    }
}
```

- [ ] **Step 2: Run it and see it fail**

Run: `cargo test -p dispatch-tui a_picker_draws_at_any_size_without_panicking`
Expected: FAIL, a panic with `assertion failed: min <= max`.

- [ ] **Step 3: Fix the clamp**

In `impl Widget for &Picker`, replace

```rust
            .unwrap_or(u16::MAX)
            .clamp(20, area.width);
```

with

```rust
            .unwrap_or(u16::MAX)
            // The floor gives way to a narrower area, as the prompt's and the
            // browser's do: `clamp` panics when its minimum is above its maximum.
            .clamp(20.min(area.width), area.width);
```

- [ ] **Step 4: Run the picker tests**

Run: `cargo test -p dispatch-tui picker`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

Run the three Global Constraints commands, then:

```bash
git add crates/dispatch-tui/src/picker.rs crates/dispatch-tui/src/picker/tests.rs
git commit -m "fix(tui): keep the picker's width clamp from panicking under 20 columns"
```

---

### Task 2: One set of dialog colours (`Chrome`)

**Files:**
- Modify: `crates/dispatch-tui/src/theme.rs`, `picker.rs`, `prompt.rs`, `browser.rs`, `settings_form.rs`
- Modify: `dispatch/src/approval.rs`, `dispatch/src/app.rs` (`Overlay::set_border`, `draw_overlay`, `approval_widget`, and every `set_border` caller)
- Test: `theme/tests.rs`, `picker/tests.rs`, `prompt/tests.rs`, `browser/tests.rs`, `settings_form/tests.rs`

**Interfaces:**
- Produces: `dispatch_tui::theme::Chrome { selection, accent, border, secondary, text: Style }`, `Theme::chrome(&self) -> Chrome`, `impl Default for Chrome` (= `Theme::fallback().chrome()`), and `set_chrome(&mut self, chrome: Chrome)` on `Picker`, `Prompt`, `Browser` and `SettingsForm`. `set_border` and `SettingsForm::set_styles` are removed.
- Test helper produced: `#[cfg(test)] pub(crate) fn loud_chrome() -> Chrome` and `#[cfg(test)] pub(crate) fn assert_no_fixed_colours(buf: &Buffer)` in `theme.rs`.

- [ ] **Step 1: Write the failing tests**

In `theme/tests.rs`:

```rust
#[test]
fn chrome_is_drawn_from_the_theme() {
    let theme = Theme::fallback();
    let chrome = theme.chrome();

    assert_eq!(chrome.selection.bg, Some(theme.tint));
    assert_eq!(chrome.selection.fg, Some(theme.text));
    assert_eq!(chrome.accent.fg, Some(theme.accent));
    assert_eq!(chrome.border.fg, Some(theme.faded));
    assert_eq!(chrome.secondary.fg, Some(theme.faded));
    assert_eq!(chrome.text.fg, Some(theme.text));
    assert_eq!(Chrome::default(), chrome);
}
```

In `picker/tests.rs`:

```rust
#[test]
fn the_picker_draws_only_in_its_chrome() {
    let mut picker = picker();
    let chrome = crate::theme::loud_chrome();
    picker.set_chrome(chrome);
    let buf = render(&picker, 40, 8);

    crate::theme::assert_no_fixed_colours(&buf);
    // The first row is selected: its bar is the chrome's selection.
    let y = (0..buf.area.height)
        .find(|&y| text(&buf).lines().nth(y as usize).is_some_and(|l| l.contains("Claude Code")))
        .expect("the first row is drawn");
    let cell = buf.cell((buf.area.width / 2, y)).expect("on screen");
    assert_eq!(cell.bg, chrome.selection.bg.expect("a background"));
}
```

In `prompt/tests.rs`:

```rust
#[test]
fn the_prompt_draws_only_in_its_chrome() {
    let mut prompt = Prompt::new("Open on laptop", "a path on that machine");
    prompt.set_note(Some(Note::Error("no such directory".into())));
    prompt.set_chrome(crate::theme::loud_chrome());
    let area = Rect::new(0, 0, 60, 12);
    let mut buf = Buffer::empty(area);
    (&prompt).render(area, &mut buf);

    crate::theme::assert_no_fixed_colours(&buf);
    assert!(
        buf.content().iter().any(|cell| cell.fg == Color::Red),
        "an error keeps its red: it means something"
    );
}
```

In `browser/tests.rs`:

```rust
#[test]
fn the_browser_draws_only_in_its_chrome() {
    let tree = Tree::new("chrome", &["alpha", "beta+git"]);
    let mut browser = Browser::new(tree.path());
    browser.set_chrome(crate::theme::loud_chrome());
    let area = Rect::new(0, 0, 60, 16);
    let mut buf = Buffer::empty(area);
    (&browser).render(area, &mut buf);

    crate::theme::assert_no_fixed_colours(&buf);
}
```

In `settings_form/tests.rs`:

```rust
#[test]
fn the_settings_form_draws_only_in_its_chrome() {
    let mut form = form();
    form.set_chrome(crate::theme::loud_chrome());
    let area = Rect::new(0, 0, 60, 12);
    let mut buf = Buffer::empty(area);
    (&form).render(area, &mut buf);

    crate::theme::assert_no_fixed_colours(&buf);
}
```

Each file's existing `use` lines decide what else to import: `Buffer`, `Rect`, `Widget`, `Color` and `Note`. Add whatever is missing, the way the file's own render helper imports them. If `Prompt::set_note`'s signature or `Note::Error`'s payload differ, follow the source.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p dispatch-tui chrome`
Expected: FAIL to compile: `Chrome`, `set_chrome`, `loud_chrome` not found.

- [ ] **Step 3: Add `Chrome` to `theme.rs`**

Change the import to `use ratatui::style::{Color, Modifier, Style};` and add after `impl Default for Theme`:

```rust
/// The colours every dialog is drawn in, so a picker, a prompt and the
/// settings form read as one interface rather than four.
///
/// Only decoration: a colour that means something — an error's red, a state
/// glyph's — stays its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chrome {
    /// A highlighted row: the tint behind the palette's text, bold.
    pub selection: Style,
    /// What has the keyboard: a caret, the path being browsed.
    pub accent: Style,
    /// A dialog's frame and the hint on it.
    pub border: Style,
    /// Detail after a label, text for an empty list, hints inside a dialog.
    pub secondary: Style,
    /// Text drawn on the tint.
    pub text: Style,
}

impl Theme {
    /// The dialog colours this theme draws.
    #[must_use]
    pub fn chrome(&self) -> Chrome {
        Chrome {
            selection: Style::default()
                .bg(self.tint)
                .fg(self.text)
                .add_modifier(Modifier::BOLD),
            accent: Style::default().fg(self.accent),
            border: Style::default().fg(self.faded),
            secondary: Style::default().fg(self.faded),
            text: Style::default().fg(self.text),
        }
    }
}

impl Default for Chrome {
    fn default() -> Self {
        Theme::fallback().chrome()
    }
}

/// A chrome no widget could arrive at by accident, for asserting that a
/// widget draws in the chrome it was given.
#[cfg(test)]
pub(crate) fn loud_chrome() -> Chrome {
    let rgb = |n: u8| Color::Rgb(n, n.wrapping_add(1), n.wrapping_add(2));
    Chrome {
        selection: Style::default().bg(rgb(10)).fg(rgb(20)),
        accent: Style::default().fg(rgb(30)),
        border: Style::default().fg(rgb(40)),
        secondary: Style::default().fg(rgb(50)),
        text: Style::default().fg(rgb(60)),
    }
}

/// Fails if any cell of `buf` is in one of the colours the dialogs used to
/// hardcode.
#[cfg(test)]
pub(crate) fn assert_no_fixed_colours(buf: &ratatui::buffer::Buffer) {
    for cell in buf.content() {
        for colour in [cell.fg, cell.bg] {
            assert!(
                !matches!(colour, Color::Cyan | Color::Black | Color::DarkGray),
                "a hardcoded {colour:?} at {:?}",
                cell.symbol()
            );
        }
    }
}
```

Re-export it: in `crates/dispatch-tui/src/lib.rs`, check what `theme` exports today and add `Chrome` the same way (`pub use theme::{…, Chrome}` if there is such a line; otherwise `dispatch_tui::theme::Chrome` is the path).

- [ ] **Step 4: Put the picker on its chrome**

In `picker.rs`: replace the field `border: Style` with `chrome: Chrome` (doc: `/// The dialog colours, set by whoever draws the overlay.`), initialise it with `Chrome::default()`, and replace `set_border` with:

```rust
    /// Draws in `chrome`.
    pub fn set_chrome(&mut self, chrome: Chrome) {
        self.chrome = chrome;
    }
```

In `render`: `.border_style(self.chrome.border)`, and the bottom hint in `self.chrome.border`. The empty-list text uses `self.chrome.secondary`. The chosen row uses `self.chrome.selection`, the other rows `Style::default()`. The detail uses `self.chrome.secondary` on unchosen rows. Import `crate::theme::Chrome`, and drop `Color` and `Modifier` from the imports if nothing else uses them.

- [ ] **Step 5: Put the prompt, browser and settings form on their chrome**

- `prompt.rs`: replace `border: Style` with `chrome: Chrome`, and `set_border` with `set_chrome`. The frame uses `chrome.border`. `"> "` and the `"▏"` caret (lines ~184, ~193) use `chrome.accent`. The hint (~202) uses `chrome.secondary`. `Note::Busy` stays `Color::Yellow` and `Note::Error` stays `Color::Red`.
- `browser.rs`: the same field and setter change. The path line style (~461) uses `chrome.accent`. The selected entry (~491) uses `chrome.selection` instead of `Modifier::REVERSED`. `KEYS` (~529) uses `chrome.secondary`.
- `settings_form.rs`: replace the three fields `border`, `highlight` and `faded` with `chrome: Chrome`. Remove `set_border` and `set_styles`, and add `set_chrome`. Every `self.border` becomes `self.chrome.border`, `self.highlight` becomes `self.chrome.selection`, and `self.faded` becomes `self.chrome.secondary`.
- Fix each file's existing tests that called `set_border` or `set_styles`: call `set_chrome` with `Chrome { border: <the style they passed>, ..Chrome::default() }`, or `set_chrome(Chrome::default())` where the style was incidental.

- [ ] **Step 6: Put the approval widget on the chrome**

In `dispatch/src/approval.rs`: replace `pub border: Style` with `pub chrome: dispatch_tui::theme::Chrome`. The frame uses `chrome.border`. Every `Style::default().fg(Color::DarkGray)` (~49, 51, 53, 67) becomes `self.chrome.secondary`. The bold key letters stay as they are. In `app.rs` `approval_widget`, set `chrome: self.theme.chrome()` in place of `border: …`. Fix `Approval { … }` literals in its tests the same way.

- [ ] **Step 7: Wire it in `app.rs`**

Rename `Overlay::set_border(&mut self, style: Style)` to `set_chrome(&mut self, chrome: Chrome)`. The body is the same match, calling `set_chrome(chrome)` on each widget; `add.prompt_mut().set_chrome(chrome)`. In `draw_overlay`, replace the opening:

```rust
        let border = Style::default().fg(self.theme.faded);
        if let Some(overlay) = &mut self.overlay {
            overlay.set_border(border);
        }

        if let Some(Overlay::Settings { form, .. }) = &mut self.overlay {
            form.set_styles(
                Style::default().bg(self.theme.tint).fg(self.theme.text),
                Style::default().fg(self.theme.faded),
            );
        }
```

with

```rust
        let chrome = self.theme.chrome();
        if let Some(overlay) = &mut self.overlay {
            overlay.set_chrome(chrome);
        }
```

Then run `grep -rn "set_border\|set_styles" crates dispatch`: nothing should be left.

- [ ] **Step 8: Run the tests**

Run: `cargo test --workspace chrome` and then `cargo test --workspace`
Expected: PASS. A pre-existing test that asserted `Color::Cyan` on a border now fails. Change its expectation to the chrome's border colour (`Theme::fallback().faded` when nothing was set), since the change is the point of this task.

- [ ] **Step 9: Gate and commit**

```bash
git add -A crates/dispatch-tui dispatch/src
git commit -m "feat(tui): draw every dialog in one themed chrome"
```

---

### Task 3: The focus glide retargets from where it is

**Files:**
- Modify: `crates/dispatch-tui/src/sidebar.rs` (`Glide`, `render_glide`)
- Modify: `dispatch/src/app.rs` (`glide_from` field, new `glide_to` field, `notice_focus`, the `glide:` line in `draw`)
- Test: `crates/dispatch-tui/src/sidebar/tests.rs`, `dispatch/src/app.rs` tests

**Interfaces:**
- Produces: `pub enum sidebar::Origin { Row(Anchor), Between { from: Anchor, to: Anchor, t: f32 } }` with `pub fn nearest(self) -> Anchor`, and `pub struct Glide { pub from: Origin, pub t: f32 }`.

- [ ] **Step 1: Write the failing app test** (in `app.rs` tests, beside `focus_eases_the_border_from_faded_to_accent`)

```rust
    /// The sidebar row the focus tint is painted on, as last drawn.
    fn tinted_row(app: &App, terminal: &ratatui::Terminal<ratatui::backend::TestBackend>) -> Option<u16> {
        let buf = terminal.backend().buffer();
        // Past the frame, the twisty and the icon: every row's fill covers it.
        let x = app.sidebar_area.x + 8;
        (app.sidebar_area.y..app.sidebar_area.y + app.sidebar_area.height)
            .find(|&y| buf.cell((x, y)).is_some_and(|cell| cell.bg == app.theme.tint))
    }

    #[test]
    fn a_glide_replaced_mid_way_starts_where_the_tint_is() {
        let (mut app, project, daemon, _sent) = attached_app();
        let clock = hand_clock(&mut app);
        let panes = spawn_several(&mut app, &daemon, project, 8);
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30))
            .expect("a test backend can be created");
        app.focus_pane(panes[0]);
        drawn(&mut app, &mut terminal);
        advance(&clock, Duration::from_secs(1));
        drawn(&mut app, &mut terminal);
        let first = tinted_row(&app, &terminal).expect("the tint is on the first pane");

        // Off towards the last pane, seven rows down, and 40 ms in…
        app.focus_pane(panes[7]);
        drawn(&mut app, &mut terminal);
        advance(&clock, Duration::from_millis(40));
        drawn(&mut app, &mut terminal);
        let under_way = tinted_row(&app, &terminal).expect("the tint is moving");
        assert!(under_way > first, "it has left the first row");
        let last = first + 7;
        assert!(under_way < last, "and not reached the last");

        // …then back to the second. The new glide starts where the tint is,
        // not at the last pane's row it was heading to.
        app.focus_pane(panes[1]);
        drawn(&mut app, &mut terminal);
        let restarted = tinted_row(&app, &terminal).expect("the tint is moving");
        assert!(
            restarted.abs_diff(under_way) <= 1,
            "restarted at {restarted}, the tint was at {under_way}"
        );

        advance(&clock, Duration::from_millis(200));
        drawn(&mut app, &mut terminal);
        assert_eq!(tinted_row(&app, &terminal), Some(first + 1), "it lands on the second pane");
    }
```

If the pane rows turn out not to be consecutive (for example, branch lines between them), compute `last` and the second pane's row by settling focus on each once and reading `tinted_row`, not by adding offsets.

- [ ] **Step 2: Run it and see it fail**

Run: `cargo test -p dispatch a_glide_replaced_mid_way_starts_where_the_tint_is`
Expected: FAIL on the `restarted` assertion: the tint restarts at `last`.

- [ ] **Step 3: Add `Origin` to `sidebar.rs`**

Replace `pub struct Glide` with:

```rust
/// Where a glide of the focus tint starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Origin {
    /// At rest on a row.
    Row(Anchor),
    /// Part-way from one row to another: where a glide had got to when a new
    /// one replaced it, so rapid moves read as one continuous motion.
    Between {
        /// The row the replaced glide came from.
        from: Anchor,
        /// The row it was heading to.
        to: Anchor,
        /// How far it had got, eased, 0.0–1.0.
        t: f32,
    },
}

impl Origin {
    /// The row it is nearer, for a glide that starts from it and is replaced
    /// in turn: one level of `Between` is all there ever is.
    #[must_use]
    pub fn nearest(self) -> Anchor {
        match self {
            Origin::Row(anchor) => anchor,
            Origin::Between { from, to, t } => {
                if t < 0.5 {
                    from
                } else {
                    to
                }
            }
        }
    }
}

/// The focus tint moving to the focused row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glide {
    /// Where it started.
    pub from: Origin,
    /// How far it has got, eased, 0.0–1.0.
    pub t: f32,
}
```

- [ ] **Step 4: Resolve the origin in `render_glide`**

Replace the `match (find(glide.from), find(to)) { … }` block with:

```rust
        // Where the tint starts, in rows, and the section it starts in:
        // resolved against this frame's rows, so a scroll under a glide
        // moves its start with it.
        let start = match glide.from {
            Origin::Row(from) => find(from).map(|(index, y, _)| (index, f32::from(y))),
            Origin::Between { from, to: towards, t } => match (find(from), find(towards)) {
                (Some((a, from_y, _)), Some((b, to_y, _))) if a == b => {
                    let (from_y, to_y) = (f32::from(from_y), f32::from(to_y));
                    Some((a, from_y + (to_y - from_y) * t))
                }
                _ => find(glide.from.nearest()).map(|(index, y, _)| (index, f32::from(y))),
            },
        };

        match (start, find(to)) {
            (Some((a, from_y)), Some((b, to_y, body))) if a == b => {
                let y = (from_y + (f32::from(to_y) - from_y) * glide.t).round() as u16;
                fill(buf, body, body.x, y, self.tinted());
            }
            (_, to_at) => {
                // A row the tint has wholly left, or not yet reached, is left
                // unpainted rather than painted the palette's background.
                let from_at = find(glide.from.nearest());
                let row = |at: Option<(usize, u16, Rect)>, t: f32| {
                    at.zip(self.theme.from_background(Role::Tint, t))
                };
                if let Some(((_, y, body), colour)) = row(from_at, 1.0 - glide.t) {
                    fill(buf, body, body.x, y, Style::default().bg(colour).fg(text));
                }
                if let Some(((_, y, body), colour)) = row(to_at, glide.t) {
                    fill(buf, body, body.x, y, Style::default().bg(colour).fg(text));
                }
            }
        }
```

Then fix existing sidebar tests that build `Glide { from: anchor, t }` to use `Glide { from: Origin::Row(anchor), t }`.

- [ ] **Step 5: Add a sidebar unit test for `Between`** (in `sidebar/tests.rs`)

Use that file's existing helpers for a state with one project and several panes (read how `render_glide` is tested today). Render with `SidebarMotion { glide: Some(Glide { from: Origin::Between { from: Anchor::Pane(first), to: Anchor::Pane(fifth), t: 0.5 }, t: 0.0 }), ..Default::default() }` while `fifth` is focused. Assert that the tinted row is the third pane's (half way from row 1 to row 5). Name it `a_glide_from_between_two_rows_starts_half_way`.

- [ ] **Step 6: Retarget in `notice_focus`**

In `App`: change `glide_from: Option<sidebar::Anchor>` to `Option<sidebar::Origin>`, keeping its doc, and add below it:

```rust
    /// The row the running glide is heading to, so a glide that replaces it
    /// can start from where it has got to.
    glide_to: Option<sidebar::Anchor>,
```

Initialise it to `None` in `App::new`. In `notice_focus`, replace the `if anchor != self.last_anchor { … }` block with:

```rust
        if anchor != self.last_anchor {
            // A glide replaced mid-way starts from where its tint is on
            // screen: from the row it was heading to, a burst of presses
            // makes the tint jump ahead before it moves.
            let running = self
                .glide_from
                .zip(self.animations.value(Target::Glide, now))
                .zip(self.glide_to);
            self.glide_from = match running {
                Some(((origin, t), to)) => Some(sidebar::Origin::Between {
                    from: origin.nearest(),
                    to,
                    t,
                }),
                None => self.last_anchor.map(sidebar::Origin::Row),
            };
            self.glide_to = anchor;
            if self.glide_from.is_some() {
                self.animations.start(Target::Glide, now, EASE, 0.0);
            }
        }
```

The `glide:` expression in `draw` already builds `sidebar::Glide { from, t }` from `glide_from`, so it compiles unchanged once the types agree.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p dispatch-tui sidebar` and `cargo test -p dispatch glide`
Expected: PASS, including `with_motion_off_focus_changes_at_once`.

- [ ] **Step 8: Gate and commit**

```bash
git add crates/dispatch-tui/src/sidebar.rs crates/dispatch-tui/src/sidebar/tests.rs dispatch/src/app.rs
git commit -m "fix(tui): start a replaced focus glide from where the tint is"
```

---

### Task 4: Spin only for spinners on screen; time frames

**Files:**
- Modify: `crates/dispatch-tui/src/sidebar.rs` (new `spins`)
- Create: `dispatch/src/frame_stats.rs`; register `mod frame_stats;` in `dispatch/src/main.rs`, where the other modules are declared
- Modify: `dispatch/src/app.rs` (`spinner_drawn`, `tabs_spun`, `frame_stats` fields; `draw`, `draw_tabs`, `next_frame`)
- Test: `sidebar/tests.rs`, `frame_stats.rs` (inline `mod tests`), `app.rs` tests

**Interfaces:**
- Produces: `pub fn sidebar::spins(state: &AppState, area: Rect, scroll: &Scroll) -> bool`, `frame_stats::FrameStats::{new(now: Instant) -> Self, record(&mut self, took: Duration, now: Instant) -> Option<Summary>}` and `frame_stats::Summary { p50, p95, max: Duration, frames: usize }`.

- [ ] **Step 1: Write the failing tests**

In `sidebar/tests.rs`, using that file's state builders:

```rust
#[test]
fn a_running_row_on_screen_spins_and_one_hidden_does_not() {
    let (mut state, project, _) = state();
    let parent = spawn(&mut state, project, "shell");
    let mut child = Pane::new(project, HarnessId::new("shell"));
    child.parent = Some(parent);
    let child = state.adopt_pane(child).expect("the project exists");
    state.set_pane_status(parent, PaneStatus::Idle).expect("exists");
    state.set_pane_status(child, PaneStatus::Running).expect("exists");

    // Unfolded: the child's row is drawn, so it spins.
    assert!(spins(&state, Rect::new(0, 0, WIDTH, 20), &Scroll::new()));
    // Parent folded: the child's row is gone and the parent is idle.
    state.toggle_pane_collapsed(parent);
    assert!(!spins(&state, Rect::new(0, 0, WIDTH, 20), &Scroll::new()));
    // Project folded: its row carries the rollup, which spins.
    state.toggle_pane_collapsed(parent);
    state.toggle_project_collapsed(project);
    assert!(spins(&state, Rect::new(0, 0, WIDTH, 20), &Scroll::new()));

    // Unfolded, but scrolled out of a two-row sidebar: not on screen.
    state.toggle_project_collapsed(project);
    for _ in 0..6 {
        spawn(&mut state, project, "shell");
    }
    let tiny = Rect::new(0, 0, WIDTH, 4);
    let mut scroll = Scroll::new();
    settle(&state, tiny, &mut scroll, None);
    assert!(!spins(&state, tiny, &scroll), "the child's row is below the fold");
}
```

(`PaneStatus` may need adding to the file's `use dispatch_core::{…}` line. The scrolled case relies on the child being below the first two rows: the project row, then the parent, then the child, with the six new panes after. If `settle` with no anchor leaves the offset at 0, the child at row 3 is off a 2-row body.)

In `app.rs` tests, replace the body of `a_spinner_asks_for_a_frame_a_tenth_of_a_second` so that it draws once before asking (`drawn(&mut app, &mut terminal)` on a 100×30 terminal after setting `Running`), and add:

```rust
    #[test]
    fn a_spinner_nobody_can_see_asks_for_no_frames() {
        let (mut app, project, daemon, _sent) = attached_app();
        let clock = hand_clock(&mut app);
        let parent = spawn_several(&mut app, &daemon, project, 1)[0];
        let child = PaneId::new();
        daemon
            .send(spawned(child, project, "shell", Some(parent), false))
            .expect("the app is listening");
        app.poll_daemon();
        app.state.set_pane_status(parent, PaneStatus::Idle).expect("exists");
        app.state.set_pane_status(child, PaneStatus::Running).expect("exists");
        app.state.toggle_pane_collapsed(parent);
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30))
            .expect("a test backend can be created");
        advance(&clock, Duration::from_secs(10));
        drawn(&mut app, &mut terminal);
        advance(&clock, Duration::from_secs(10));
        app.animations.sweep(app.now());

        assert_eq!(app.next_frame(app.now()), None, "its row is folded away");

        app.state.toggle_pane_collapsed(parent);
        drawn(&mut app, &mut terminal);
        assert_eq!(app.next_frame(app.now()), Some(SPIN_FRAME));
    }
```

In `frame_stats.rs`, write the module (Step 3) with this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_comes_every_ten_seconds_and_starts_afresh() {
        let start = Instant::now();
        let mut stats = FrameStats::new(start);
        for ms in 1..=100 {
            assert_eq!(stats.record(Duration::from_millis(ms), start + Duration::from_millis(ms)), None);
        }
        let summary = stats
            .record(Duration::from_millis(1), start + Duration::from_secs(10))
            .expect("ten seconds have passed");
        assert_eq!(summary.frames, 101);
        assert_eq!(summary.max, Duration::from_millis(100));
        assert_eq!(summary.p50, Duration::from_millis(50));
        assert_eq!(summary.p95, Duration::from_millis(95));
        assert_eq!(stats.record(Duration::from_millis(1), start + Duration::from_secs(11)), None);
    }

    #[test]
    fn only_the_last_frames_are_kept() {
        let start = Instant::now();
        let mut stats = FrameStats::new(start);
        for _ in 0..1000 {
            stats.record(Duration::from_millis(1), start);
        }
        let summary = stats
            .record(Duration::from_millis(1), start + Duration::from_secs(10))
            .expect("due");
        assert_eq!(summary.frames, KEPT);
    }
}
```

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test --workspace spin` and `cargo test -p dispatch frame_stats`
Expected: FAIL to compile (`spins`, `FrameStats` not found).

- [ ] **Step 3: Implement**

`sidebar.rs`, beside `settle`:

```rust
/// Whether any row drawn at `area` carries a spinner: a live running pane,
/// or a folded project whose most urgent state is working.
///
/// Walks the same sections the render does, so a row scrolled away or folded
/// out of sight asks for no frames: a spinner nobody can see is only work.
#[must_use]
pub fn spins(state: &AppState, area: Rect, scroll: &Scroll) -> bool {
    sections(state, area, scroll).iter().any(|section| {
        section.visible().any(|(_, row)| match *row {
            Row::Pane(id, _) => state
                .pane(id)
                .is_some_and(|pane| !pane.closed && pane.status == PaneStatus::Running),
            Row::Project(id) => {
                state.is_project_collapsed(id)
                    && Rollup::of(state, state.panes_for(id)) == Some(Rollup::Working)
            }
            Row::Branch(..) => false,
        })
    })
}
```

`dispatch/src/frame_stats.rs`:

```rust
//! How long frames take to draw, logged now and then so a change of frame
//! rate can be decided from numbers rather than guessed.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How many of the latest frames a summary covers.
pub const KEPT: usize = 256;

/// How often a summary is due.
pub const EVERY: Duration = Duration::from_secs(10);

/// The latest frames' durations.
#[derive(Debug)]
pub struct FrameStats {
    took: VecDeque<Duration>,
    since: Instant,
}

/// One summary: the median, the 95th percentile, the slowest, and how many
/// frames it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub p50: Duration,
    pub p95: Duration,
    pub max: Duration,
    pub frames: usize,
}

impl FrameStats {
    /// Nothing recorded yet, the first summary due [`EVERY`] after `now`.
    #[must_use]
    pub fn new(now: Instant) -> Self {
        Self {
            took: VecDeque::with_capacity(KEPT),
            since: now,
        }
    }

    /// Records one frame, returning a summary when one is due, and starting
    /// afresh after it.
    pub fn record(&mut self, took: Duration, now: Instant) -> Option<Summary> {
        if self.took.len() == KEPT {
            self.took.pop_front();
        }
        self.took.push_back(took);

        if now.saturating_duration_since(self.since) < EVERY {
            return None;
        }

        let mut sorted: Vec<Duration> = self.took.drain(..).collect();
        sorted.sort();
        self.since = now;

        let at = |share: f32| {
            let last = sorted.len() - 1;
            sorted[((last as f32) * share).round() as usize]
        };
        Some(Summary {
            p50: at(0.50),
            p95: at(0.95),
            max: sorted[sorted.len() - 1],
            frames: sorted.len(),
        })
    }
}
```

(`sorted` is never empty there, because a frame was just pushed. If clippy flags the casts, add a precise `#[allow(clippy::cast_…)]` with a one-line reason, as the codebase does elsewhere. Check with `grep -rn "allow(clippy::cast" dispatch/src`.)

Check the test's expected p50 and p95 against this index formula. With 101 samples (1..=100 ms, then 1 ms), sorted gives `[1, 1, 2, …, 100]`. Then `at(0.5)` is index 50, which is 50 ms, and `at(0.95)` is index 95, which is 95 ms. Adjust the test's expected values if the arithmetic differs. The formula is the contract.

In `App`: add these fields with doc comments, initialised in `App::new`:

- `spinner_drawn: bool` (`false`): whether the last frame drew a spinner.
- `tabs_spun: bool` (`false`): whether the last tab row drew one.
- `frame_stats: crate::frame_stats::FrameStats` (`FrameStats::new(Instant::now())`).

In `draw`, first line: `let drawing = Instant::now();` (the real clock, because this measures real cost). After the sidebar is rendered: `self.spinner_drawn = self.motion && sidebar::spins(&self.state, sidebar_area, &self.sidebar_scroll);`. After `self.draw_tabs(…)`: `self.spinner_drawn |= self.tabs_spun;`. At the very end of `draw`:

```rust
        if let Some(summary) = self.frame_stats.record(drawing.elapsed(), now) {
            tracing::debug!(
                p50 = ?summary.p50,
                p95 = ?summary.p95,
                max = ?summary.max,
                frames = summary.frames,
                "frame timing"
            );
        }
```

In `draw_tabs`: set `self.tabs_spun = false;` right after `self.tab_row = area;`. Where a chip's `rollup` is found, add `self.tabs_spun |= rollup == sidebar::Rollup::Working && spinner.is_some();`.

In `next_frame`, replace the `spinning` computation and return with:

```rust
        // Only what the last frame drew: a running pane folded away or
        // scrolled out of the sidebar has no spinner to turn.
        (self.motion && self.spinner_drawn).then_some(SPIN_FRAME)
```

and update its doc comment's first paragraph to say "a spinner on screen". Rename `_now` if it becomes unused; keep the signature.

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace`
Expected: PASS. A test that asserted `next_frame` for a running pane without drawing first now needs one `drawn(…)` call. Add it; do not weaken the assertion.

- [ ] **Step 5: Gate and commit**

```bash
git add crates/dispatch-tui/src/sidebar.rs crates/dispatch-tui/src/sidebar/tests.rs dispatch/src/frame_stats.rs dispatch/src/main.rs dispatch/src/app.rs
git commit -m "perf(tui): spin only for spinners on screen, and log frame timing"
```

---

### Task 5: State words, and secondary text that stays readable

**Files:**
- Modify: `crates/dispatch-tui/src/sidebar.rs` (new `status_text`), `crates/dispatch-tui/src/theme.rs` (`contrast`, `READABLE`, `secondary`, `Theme::new`, `Theme::rgb`)
- Modify: `dispatch/src/app.rs` (`draw_panes`: the focused pane's border)
- Test: `sidebar/tests.rs`, `theme/tests.rs`, `app.rs` tests

**Interfaces:**
- Produces: `pub fn sidebar::status_text(status: PaneStatus, reason: Option<&str>) -> Cow<'_, str>`, `pub fn theme::contrast(a: Rgb, b: Rgb) -> f32`, `pub const theme::READABLE: f32 = 4.5`.

- [ ] **Step 1: Write the failing tests**

`sidebar/tests.rs`:

```rust
#[test]
fn every_state_has_words() {
    assert_eq!(status_text(PaneStatus::Starting, None), "Starting");
    assert_eq!(status_text(PaneStatus::Running, None), "Working");
    assert_eq!(status_text(PaneStatus::Blocked, None), "Needs approval");
    assert_eq!(status_text(PaneStatus::Blocked, Some("Permission prompt")), "Permission prompt");
    assert_eq!(status_text(PaneStatus::Idle, None), "Finished");
    assert_eq!(status_text(PaneStatus::Exited(0), None), "Exited");
    assert_eq!(status_text(PaneStatus::Exited(2), None), "Exited (2)");
}
```

`theme/tests.rs`:

```rust
fn hex(value: u32) -> Rgb {
    Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

fn faded_rgb(palette: Palette) -> Rgb {
    Theme::new(palette, Depth::TrueColor).rgb(Role::Faded)
}

#[test]
fn contrast_is_the_wcag_ratio() {
    assert!((contrast(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 0.01);
    assert!((contrast(Rgb(9, 9, 9), Rgb(9, 9, 9)) - 1.0).abs() < 0.001);
}

#[test]
fn secondary_text_is_readable_on_dark_light_and_low_contrast_themes() {
    for (background, foreground) in [
        (0x16161e, 0xc8c8d8), // the fallback
        (0xfafafa, 0x383a42), // a light theme
        (0x2e3440, 0x81a1c1), // low contrast
    ] {
        let palette = Palette { background: hex(background), foreground: hex(foreground), accent: hex(0xb4a0f0) };
        let faded = faded_rgb(palette);
        assert!(
            contrast(faded, palette.background) >= READABLE,
            "{faded:?} on {:?}", palette.background
        );
        assert!(
            contrast(faded, palette.background) <= contrast(palette.foreground, palette.background),
            "never louder than the text itself"
        );
    }
}

#[test]
fn a_foreground_that_is_itself_faint_is_used_as_it_is() {
    let palette = Palette { background: hex(0x303030), foreground: hex(0x707070), accent: hex(0xb4a0f0) };
    assert_eq!(faded_rgb(palette), palette.foreground);
}

#[test]
fn the_colour_drawn_and_the_colour_animated_agree() {
    let theme = Theme::fallback();
    let rgb = theme.rgb(Role::Faded);
    assert_eq!(theme.faded, Color::Rgb(rgb.0, rgb.1, rgb.2));
}
```

Also update `every_role_is_mixed_from_the_palette`: its `theme.faded` expectation becomes `rgb(Theme::fallback().rgb(Role::Faded))`, and it adds `assert!(contrast(theme.rgb(Role::Faded), palette.background) >= READABLE);`. The old 0.45 mix gives 4.13:1 on the fallback, below the bar, so the fallback's `faded` changes. That is intended.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p dispatch-tui every_state_has_words secondary_text contrast`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`sidebar.rs` (add `use std::borrow::Cow;`):

```rust
/// A pane's state in words, for where a glyph alone would have to be decoded:
/// the attention picker and the focused pane's border.
///
/// Blocked says why when its rule does, and "Needs approval" when it does not.
#[must_use]
pub fn status_text(status: PaneStatus, reason: Option<&str>) -> Cow<'_, str> {
    match status {
        PaneStatus::Starting => Cow::Borrowed("Starting"),
        PaneStatus::Running => Cow::Borrowed("Working"),
        PaneStatus::Blocked => Cow::Borrowed(reason.unwrap_or("Needs approval")),
        PaneStatus::Idle => Cow::Borrowed("Finished"),
        PaneStatus::Exited(0) => Cow::Borrowed("Exited"),
        PaneStatus::Exited(code) => Cow::Owned(format!("Exited ({code})")),
    }
}
```

`theme.rs`:

```rust
/// The contrast secondary text needs against the background: WCAG's AA
/// ratio for body text.
pub const READABLE: f32 = 4.5;

/// WCAG relative luminance, 0.0 for black to 1.0 for white.
fn luminance(colour: Rgb) -> f32 {
    let channel = |value: u8| {
        let value = f32::from(value) / 255.0;
        if value <= 0.039_28 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(colour.0) + 0.7152 * channel(colour.1) + 0.0722 * channel(colour.2)
}

/// The WCAG contrast ratio between two colours, from 1.0 to 21.0.
#[must_use]
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Secondary text: the foreground faded toward the background, but only as
/// far as it stays readable.
///
/// A fixed mix read well on the fallback's dark palette and fell below
/// readable on light and low-contrast themes. This starts at the same 0.45
/// and steps back by 0.05 until the text reaches [`READABLE`]; a foreground
/// that never does is used as it is, since fading it further only hurts.
fn secondary(palette: Palette) -> Rgb {
    (0..9u8)
        .map(|step| palette.foreground.mix(palette.background, 0.45 - 0.05 * f32::from(step)))
        .find(|colour| contrast(*colour, palette.background) >= READABLE)
        .unwrap_or(palette.foreground)
}
```

In `Theme::new`: `faded: colour(secondary(palette)),`. In `Theme::rgb`: `Role::Faded => secondary(p),`. Update the `faded` field's doc to add: "Faded only as far as it stays readable: see [`secondary`]."

`app.rs` `draw_panes`: build the block as a variable and, for the focused pane, add the words on the right of its top border:

```rust
            let mut block = pane_block(colour, is_focused).title(pane_title(&self.state, *id));
            // The focused pane says what it is doing in words, on the right
            // of its top border; the others leave it to their glyphs, or the
            // grid would fill with words.
            if is_focused && let Some(state) = self.state.pane(*id) {
                let words = sidebar::status_text(state.status, None);
                block = block.title_top(
                    Line::styled(format!(" {words} "), Style::default().fg(self.theme.faded))
                        .right_aligned(),
                );
            }
            frame.render_widget(block, *outer);
```

Task 6 replaces the `None` with the pane's reason.

Add an app test:

```rust
    #[test]
    fn the_focused_pane_says_what_it_is_doing() {
        let (mut app, project, daemon, _sent) = attached_app();
        let panes = spawn_several(&mut app, &daemon, project, 2);
        app.state.set_pane_status(panes[0], PaneStatus::Running).expect("exists");
        app.state.set_pane_status(panes[1], PaneStatus::Running).expect("exists");
        app.focus_pane(panes[0]);
        let mut terminal = a_wide_terminal();
        drawn(&mut app, &mut terminal);

        let top_of = |pane: PaneId| {
            let (_, rect) = app.frames.iter().find(|(id, _)| *id == pane).expect("tiled");
            let line = rendered_text(&terminal).lines().nth(rect.y as usize).unwrap_or("").to_string();
            line.chars().skip(rect.x as usize).take(rect.width as usize).collect::<String>()
        };
        assert!(top_of(panes[0]).contains(" Working "), "{}", top_of(panes[0]));
        assert!(!top_of(panes[1]).contains(" Working "), "only the focused one says");
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace`
Expected: PASS. Any test that hardcoded the old fallback `faded` RGB `(120, 120, 132)` should read it from `Theme::fallback().faded` instead.

- [ ] **Step 5: Gate and commit**

```bash
git add -A crates/dispatch-tui dispatch/src/app.rs
git commit -m "feat(tui): name each state in words and keep faded text readable"
```

---

### Task 6: Next attention, the attention picker, and rule reasons

**Files:**
- Modify: `crates/dispatch-config/src/status.rs`, `status/builtin.rs`, `status/tests.rs`, `crates/dispatch-config/src/lib.rs`
- Modify: `crates/dispatch-tui/src/activity.rs` (+tests), `keymap.rs` (+tests), `input.rs`
- Modify: `dispatch/src/app.rs`; `docs/usage.md`, `docs/configuration.md`

**Interfaces:**
- Consumes: `sidebar::status_text` (T5), `Chrome` (T2).
- Produces:
  - `RuleDef.reason: Option<String>`;
  - `pub struct RuleMatch<'a> { pub state: RuleState, pub reason: Option<&'a str> }` and `StatusRules::matching(&self, &StatusInput) -> Option<RuleMatch<'_>>`, with `evaluate` kept as a wrapper;
  - `Tracker::reason(&self) -> Option<&str>`;
  - `Command::{NextAttention, AttentionPicker}` and `Action::{NextAttention, AttentionPicker}`;
  - `App::{pane_order, is_waiting, go_to_pane, next_attention, open_attention_picker}`, `App.blocked_since: HashMap<PaneId, Instant>`, and `Overlay::Attention(Picker)` with `OverlayKind::Attention`.

- [ ] **Step 1: Failing tests for the rule reason** (`status/tests.rs`)

```rust
#[test]
fn a_blocked_rule_says_why() {
    let def: StatusDef = toml::from_str(
        r#"
        [[rules]]
        state = "blocked"
        region = "screen"
        contains = ["allow?"]
        reason = "Permission prompt"
        "#,
    )
    .expect("parses");
    let rules = StatusRules::compile("test", &def);
    let screen = vec!["Allow? [y/n]".to_string()];
    let input = StatusInput { title: "", progress: "", screen: &screen };

    assert_eq!(
        rules.matching(&input),
        Some(RuleMatch { state: RuleState::Blocked, reason: Some("Permission prompt") })
    );
    assert_eq!(rules.evaluate(&input), Some(RuleState::Blocked));
}

#[test]
fn every_built_in_blocked_rule_has_a_reason() {
    #[derive(serde::Deserialize)]
    struct File {
        status: StatusDef,
    }
    for id in ["claude", "codex", "opencode", "agy"] {
        let text = super::builtin::builtin(id).expect("built in");
        let file: File = toml::from_str(text).expect("parses");
        for rule in file.status.rules.iter().filter(|rule| rule.state == "blocked") {
            assert!(
                rule.reason.as_deref().is_some_and(|r| !r.trim().is_empty()),
                "{id}: {rule:?}"
            );
        }
    }
}
```

(Check the built-in ids against `builtin()`'s `match` and use exactly those.)

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p dispatch-config status`
Expected: FAIL to compile (`reason`, `matching` and `RuleMatch` not found).

- [ ] **Step 3: Implement the reason**

`status.rs`:
- `RuleDef` gains, after `priority`:

```rust
    /// What the pane is waiting on, in a few words, for a `blocked` rule:
    /// shown where the user picks which agent to answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
```

- `Rule` gains `reason: Option<String>`. `compile_rule` sets `reason: def.reason.as_deref().map(str::trim).filter(|r| !r.is_empty()).map(str::to_string),`.
- Add the type and the method, and make `evaluate` a wrapper:

```rust
/// What the first matching rule says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleMatch<'a> {
    /// The state it names.
    pub state: RuleState,
    /// Why, when it says: what a blocked pane is waiting on.
    pub reason: Option<&'a str>,
}

    /// The first matching rule's state and reason, trying the highest
    /// priority first; `None` when nothing matches.
    #[must_use]
    pub fn matching(&self, input: &StatusInput<'_>) -> Option<RuleMatch<'_>> {
        self.rules.iter().find(|rule| rule.matches(input)).map(|rule| RuleMatch {
            state: rule.state,
            reason: rule.reason.as_deref(),
        })
    }

    /// The state the first matching rule names, trying the highest priority
    /// first; `None` when nothing matches.
    #[must_use]
    pub fn evaluate(&self, input: &StatusInput<'_>) -> Option<RuleState> {
        self.matching(input).map(|found| found.state)
    }
```

- `lib.rs`: `pub use status::{RuleMatch, RuleState, StatusInput, StatusRules};`.
- `builtin.rs`: add a `reason = "…"` line to each blocked rule, after its `state` line:
  - Claude: permission prompt → `"Permission prompt"`; form → `"Choice form"`.
  - Codex: title `action required` → `"Action required"`; trust question → `"Trust this directory?"`; confirm/submit/allow → `"Confirm or answer"`; `[y/n]` → `"Yes/no question"`.
  - OpenCode: `△ permission required` → `"Permission prompt"`; `esc dismiss` form → `"Choice form"`.
  - Agy: `requesting permission for:` → `"Permission prompt"`.

- [ ] **Step 4: The tracker keeps the reason** (`activity.rs`)

Test first, in `activity/tests.rs`: build a tracker over rules with one blocked rule carrying `reason = "Permission prompt"` (the same TOML as Step 1). Evaluate with a matching screen: `reason()` is `Some("Permission prompt")`. Evaluate with a non-matching screen: `reason()` is `None`. Name it `the_tracker_keeps_the_blocked_reason`.

Then add to `Tracker` the field `reason: Option<String>` (doc: `/// Why the pane is blocked, from the rule that says so, while one does.`), initialised to `None`. In `evaluate`, replace `let rule = self.rules.evaluate(&StatusInput { … });` with:

```rust
        let found = self.rules.matching(&StatusInput {
            title: &self.title,
            progress: &self.progress,
            screen,
        });
        let rule = found.map(|found| found.state);
        self.reason = found
            .filter(|found| found.state == RuleState::Blocked)
            .and_then(|found| found.reason)
            .map(str::to_string);
```

and add:

```rust
    /// Why the pane is blocked, when a rule said, as of the last evaluation.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
```

- [ ] **Step 5: Commands and actions**

`input.rs` `Action`, before `Quit`:

```rust
    /// Focus the next pane waiting on the user, wherever it is.
    NextAttention,
    /// Open the list of every pane waiting on the user.
    AttentionPicker,
```

`keymap.rs`:
- `Command` gains `NextAttention` and `AttentionPicker` (before `Quit`).
- `NAMED` gains `(Command::NextAttention, "next_attention")` and `(Command::AttentionPicker, "attention_picker")`.
- `label`: `Command::NextAttention => "attention"`, `Command::AttentionPicker => "waiting"`.
- `action`: `Command::NextAttention => Action::NextAttention`, `Command::AttentionPicker => Action::AttentionPicker`.
- `defaults()`: in `normal`, append `(Chord::alt('a'), Command::NextAttention)`. In `prefix`, before the `digits(...)` line, add `(Chord::char('w'), Command::NextAttention)`. In `session`, after `(Chord::char('q'), Command::Quit)`, add `(Chord::char('w'), Command::AttentionPicker)`.
- `keymap/tests.rs`: the session `mode_help` string becomes `"SESSION  Esc/Enter done  p projects  o open  m machine  H harnesses  a approvals  f fold  q quit  w waiting"`. Add a test that `Keymap::defaults().lookup(KeyMode::Normal, &Chord::alt('a'))` is `Some(Command::NextAttention)`. Check the file for a test that lists every command name, or that checks `NAMED` covers every variant, and extend it.
- `docs/usage.md` keys table: add `Alt a` (next agent waiting on you), prefix `w`, and session `w` (list of waiting agents), in the style of the neighbouring rows.
- `docs/configuration.md`: where status rules are described, document `reason`: an optional short text on a `blocked` rule, shown in the waiting list, "Needs approval" when absent. Also add `next_attention` and `attention_picker` to the list of command names, if the file has one.

- [ ] **Step 6: Failing app tests**

In `app.rs` tests:

```rust
    fn press_alt(app: &mut App, c: char) {
        app.handle(
            &Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)),
            Size::new(100, 30),
        )
        .expect("a keystroke is handled");
    }

    fn block(app: &mut App, pane: PaneId) {
        app.state.set_pane_status(pane, PaneStatus::Blocked).expect("exists");
    }

    #[test]
    fn alt_a_walks_the_waiting_panes_in_sidebar_order_across_projects() {
        let (mut app, first, daemon, _sent) = attached_app();
        let project = Project::new("/tmp/second", ProjectSource::LocalDir);
        let second = project.id;
        daemon.send(ServerMessage::ProjectOpened { project }).expect("listening");
        app.poll_daemon();
        let ours = spawn_several(&mut app, &daemon, first, 3);
        app.select_project(second);
        let theirs = spawn_several(&mut app, &daemon, second, 2);
        block(&mut app, ours[2]);
        block(&mut app, theirs[0]);
        app.select_project(first);
        app.focus_pane(ours[0]);

        press_alt(&mut app, 'a');
        assert_eq!(app.state.focused_pane(), Some(ours[2]));
        press_alt(&mut app, 'a');
        assert_eq!(app.state.selected_project(), Some(second), "it crosses projects");
        assert_eq!(app.state.focused_pane(), Some(theirs[0]));
        press_alt(&mut app, 'a');
        assert_eq!(app.state.focused_pane(), Some(ours[2]), "and wraps");
    }

    #[test]
    fn alt_a_unfolds_what_hides_the_pane() {
        let (mut app, project, daemon, _sent) = attached_app();
        let panes = spawn_several(&mut app, &daemon, project, 2);
        block(&mut app, panes[0]);
        app.focus_pane(panes[1]);
        app.state.toggle_project_collapsed(project);

        press_alt(&mut app, 'a');

        assert_eq!(app.state.focused_pane(), Some(panes[0]));
        assert!(!app.state.is_project_collapsed(project));
    }

    #[test]
    fn with_nothing_waiting_alt_a_says_so() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        press_alt(&mut app, 'a');
        assert_eq!(app.status, "nothing waiting on you");
    }

    #[test]
    fn the_waiting_list_names_project_title_and_reason() {
        let (mut app, project, daemon, _sent) = attached_app();
        let panes = spawn_several(&mut app, &daemon, project, 2);
        app.rename(panes[1], "fixing tests");
        block(&mut app, panes[1]);

        app.open_attention_picker();

        let Some(Overlay::Attention(picker)) = &app.overlay else {
            panic!("the waiting list is open");
        };
        let item = &picker.items()[0];
        assert_eq!(item.label, "attached · fixing tests");
        assert!(item.detail.as_deref().is_some_and(|d| d.starts_with("Needs approval")), "{item:?}");
        assert_eq!(picker.items().len(), 1);
    }

    #[test]
    fn choosing_a_pane_that_stopped_waiting_does_nothing_harmful() {
        let (mut app, project, daemon, _sent) = attached_app();
        let panes = spawn_several(&mut app, &daemon, project, 2);
        block(&mut app, panes[0]);
        app.focus_pane(panes[1]);
        app.open_attention_picker();
        // It exits while the list is open.
        app.state.set_pane_status(panes[0], PaneStatus::Exited(0)).expect("exists");
        let _ = app.state.close_pane(panes[0]);

        press(&mut app, KeyCode::Enter);

        assert!(app.overlay.is_none());
        assert_eq!(app.state.focused_pane(), Some(panes[1]), "focus stays put");
    }

    #[test]
    fn the_waiting_reminder_names_its_key_and_survives_an_overlay() {
        let (mut app, project, daemon, _sent) = attached_app();
        let panes = spawn_several(&mut app, &daemon, project, 2);
        block(&mut app, panes[0]);
        let mut terminal = a_wide_terminal();

        drawn(&mut app, &mut terminal);
        assert!(bottom_row(&terminal).contains("1 waiting on you — Alt a"), "{}", bottom_row(&terminal));

        app.open_attention_picker();
        assert!(app.overlay.is_some());
        drawn(&mut app, &mut terminal);
        assert!(bottom_row(&terminal).contains("1 waiting on you"), "{}", bottom_row(&terminal));

        app.overlay = None;
        rebind(&mut app, &[("normal", "Alt a", "none"), ("prefix", "w", "none")]);
        drawn(&mut app, &mut terminal);
        let row = bottom_row(&terminal);
        assert!(row.contains("1 waiting on you") && !row.contains("waiting on you —"), "{row}");
    }
```

The project's name in the first expected label is whatever `attached_app` names `/tmp/attached`. Check `Project::new`'s naming and use it.

Run: `cargo test -p dispatch attention waiting alt_a`
Expected: FAIL to compile.

- [ ] **Step 7: Implement in `app.rs`**

Fields (doc each one, initialise in `App::new`): `blocked_since: HashMap<PaneId, Instant>`.

In the verdict loop of `poll_panes`, where `set_pane_status(id, status)` is called, add after it:

```rust
            if status == PaneStatus::Blocked {
                self.blocked_since.insert(id, now);
            } else {
                self.blocked_since.remove(&id);
            }
```

Constant near the other status strings: `const NOTHING_WAITING: &str = "nothing waiting on you";`.

Methods:

```rust
    /// Every pane in the order the sidebar lists them: machine by machine,
    /// project by project, each top-level pane followed by its subagents.
    fn pane_order(&self) -> Vec<PaneId> {
        let devices = self.state.devices();
        let projects: Vec<ProjectId> = if devices.len() <= 1 {
            self.state.projects().iter().map(|project| project.id).collect()
        } else {
            devices
                .iter()
                .flat_map(|device| {
                    self.state
                        .projects()
                        .iter()
                        .filter(move |project| project.device == device.id)
                        .map(|project| project.id)
                })
                .collect()
        };

        let mut order = Vec::new();
        for project in projects {
            for top in self.state.panes_for(project).into_iter().filter(|pane| pane.parent.is_none()) {
                order.push(top.id);
                order.extend(self.state.children_of(top.id).into_iter().map(|child| child.id));
            }
        }
        order
    }

    /// Whether `id` is a live pane waiting on the user.
    fn is_waiting(&self, id: PaneId) -> bool {
        self.state
            .pane(id)
            .is_some_and(|pane| !pane.closed && pane.status == PaneStatus::Blocked)
    }

    /// Why `id` is blocked, when its rule said.
    fn reason_of(&self, id: PaneId) -> Option<&str> {
        self.panes.get(&id).and_then(|pane| pane.activity.reason())
    }

    /// Shows `id` wherever it is: its project selected, anything folded over
    /// it unfolded, its tab on screen — the view follows the focus.
    fn go_to_pane(&mut self, id: PaneId) {
        if self.state.pane(id).is_none_or(|pane| pane.closed) {
            return;
        }
        let Some((project, parent)) = self.state.pane(id).map(|pane| (pane.project, pane.parent)) else {
            return;
        };
        if self.state.selected_project() != Some(project) {
            self.select_project(project);
        }
        if self.state.is_project_collapsed(project) {
            self.state.toggle_project_collapsed(project);
        }
        if let Some(parent) = parent
            && self.state.is_pane_collapsed(parent)
        {
            self.state.toggle_pane_collapsed(parent);
        }
        self.focus_pane(id);
    }

    /// Focuses the next pane waiting on the user after the focused one, in
    /// sidebar order, wrapping.
    fn next_attention(&mut self) {
        let order = self.pane_order();
        let start = self
            .state
            .focused_pane()
            .and_then(|focused| order.iter().position(|id| *id == focused))
            .map_or(0, |index| index + 1);
        let next = (0..order.len())
            .map(|step| order[(start + step) % order.len()])
            .find(|id| self.is_waiting(*id));

        match next {
            Some(id) => self.go_to_pane(id),
            None => self.status = NOTHING_WAITING.to_string(),
        }
    }

    /// Opens the list of every pane waiting on the user.
    fn open_attention_picker(&mut self) {
        let now = self.now();
        let items: Vec<Item> = self
            .pane_order()
            .into_iter()
            .filter(|id| self.is_waiting(*id))
            .filter_map(|id| {
                let pane = self.state.pane(id)?;
                let project = self
                    .state
                    .projects()
                    .iter()
                    .find(|project| project.id == pane.project)
                    .map_or("", |project| project.name.as_str());
                let why = sidebar::status_text(pane.status, self.reason_of(id));
                let waited = self.blocked_since.get(&id).map_or_else(String::new, |since| {
                    format!(" · {}", waited_for(now.saturating_duration_since(*since)))
                });
                Some(
                    Item::new(id.to_string(), format!("{project} · {}", pane.title))
                        .with_detail(format!("{why}{waited}")),
                )
            })
            .collect();

        if items.is_empty() {
            self.status = NOTHING_WAITING.to_string();
            return;
        }
        self.overlay = Some(Overlay::Attention(
            Picker::new("Waiting on you", items).with_hint("↑↓ choose  Enter go  Esc close"),
        ));
    }
```

A free function beside `pane_title`:

```rust
/// How long a pane has waited, as the waiting list says it: `12s`, `3m`, `1h`.
fn waited_for(waited: Duration) -> String {
    let seconds = waited.as_secs();
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        _ => format!("{}h", seconds / 3600),
    }
}
```

`Overlay`: add the variant `/// The panes waiting on the user, to go to one.  Attention(Picker),`. Add it to `picker()` and `picker_mut()` (with the other picker variants), to `kind()` as `Overlay::Attention(_) => Some(OverlayKind::Attention)`, and to `set_chrome`. `OverlayKind` gains `Attention`. In `choose`:

```rust
            OverlayKind::Attention => {
                // Chosen from a list drawn a moment ago: the pane may have
                // exited or been answered since, and then there is nowhere
                // to go.
                if let Some(pane) = self
                    .pane_order()
                    .into_iter()
                    .find(|pane| pane.to_string() == id)
                    .filter(|pane| self.state.pane(*pane).is_some_and(|p| !p.closed))
                {
                    self.go_to_pane(pane);
                }
            }
```

`act_on`'s action match: `Action::NextAttention => self.next_attention(),` and `Action::AttentionPicker => self.open_attention_picker(),`.

`draw_status`: replace the `blocked_reminder` lines with:

```rust
        // A blocked pane on another tab, or in a folded project, still needs
        // to be found; the status row is the one place always on screen, and
        // it stays said over an overlay, which may be what hides the pane.
        let blocked = self
            .state
            .projects()
            .iter()
            .flat_map(|project| self.state.panes_for(project.id))
            .filter(|pane| !pane.closed && pane.status == PaneStatus::Blocked)
            .count();
        let blocked_reminder = (blocked > 0).then(|| {
            let waiting = format!("{blocked} waiting on you");
            match self.router.keymap().path_to(Command::NextAttention) {
                Some(keys) => format!("{waiting} — {keys}"),
                None => waiting,
            }
        });
```

`draw_panes`: the `status_text(state.status, None)` from Task 5 becomes `sidebar::status_text(state.status, self.reason_of(*id))`. `self` is borrowed immutably there alongside `self.panes`. If the borrow checker objects inside the loop, compute `let reason = self.reason_of(*id).map(str::to_string);` first.

- [ ] **Step 8: Run the tests**

Run: `cargo test --workspace`
Expected: PASS. A pre-existing test asserting the reminder is hidden under an overlay contradicts the spec now. Update it to the new behaviour and say so in the commit body.

- [ ] **Step 9: Gate and commit**

```bash
git add -A crates dispatch docs
git commit -m "feat(tui): go to the next agent waiting on you, and list them with reasons"
```

---

### Task 7: Room for small windows

**Files:**
- Create: `crates/dispatch-config/src/ui_state.rs` and `crates/dispatch-config/src/ui_state/tests.rs`; `pub mod ui_state;` in `lib.rs`
- Modify: `picker.rs`, `prompt.rs`, `settings_form.rs` (`desired_width`); `keymap.rs`, `input.rs`; `dispatch/src/app.rs`; `dispatch/src/main.rs`; the docs

**Interfaces:**
- Produces:
  - `dispatch_config::ui_state::{UiState { sidebar_width: u16, sidebar_collapsed: bool }, load(dir) -> UiState, save(dir, UiState) -> Result<(), ConfigError>, FILE, MIN_SIDEBAR = 20, MAX_SIDEBAR = 60, DEFAULT_SIDEBAR = 34}`;
  - `Command::{ToggleSidebar, SidebarNarrower, SidebarWider}` and `Action::{ToggleSidebar, ResizeSidebar(i16)}`;
  - `App::keep_ui_in(dir)` and `App.{sidebar_width, sidebar_collapsed, drawer_open, dragging_sidebar, ui_dir, window}`;
  - `desired_width(&self) -> u16` on `Picker`, `Prompt` and `SettingsForm`.

- [ ] **Step 1: `ui_state` tests**

`crates/dispatch-config/src/ui_state/tests.rs`:

```rust
//! Tests for the saved interface state.

use super::*;

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary directory")
}

#[test]
fn nothing_saved_is_the_defaults() {
    let dir = dir();
    assert_eq!(load(dir.path()), UiState::default());
    assert_eq!(UiState::default().sidebar_width, DEFAULT_SIDEBAR);
    assert!(!UiState::default().sidebar_collapsed);
}

#[test]
fn what_is_saved_comes_back() {
    let dir = dir();
    let state = UiState { sidebar_width: 41, sidebar_collapsed: true };
    save(dir.path(), state).expect("saved");
    assert_eq!(load(dir.path()), state);
}

#[test]
fn a_hand_edited_width_is_brought_into_range_or_ignored() {
    let dir = dir();
    for (text, width) in [
        ("sidebar_width = 500", MAX_SIDEBAR),
        ("sidebar_width = 3", MIN_SIDEBAR),
        ("sidebar_width = \"wide\"", DEFAULT_SIDEBAR),
        ("sidebar_width = -1", DEFAULT_SIDEBAR),
        ("not toml at all [", DEFAULT_SIDEBAR),
    ] {
        std::fs::write(dir.path().join(FILE), text).expect("written");
        assert_eq!(load(dir.path()).sidebar_width, width, "{text}");
    }
}
```

(Check how the other `dispatch-config` tests make temporary directories, `grep -rn tempdir crates/dispatch-config`, and use the same crate or helper. If `testing.rs` has one, use it.)

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p dispatch-config ui_state`
Expected: FAIL to compile.

- [ ] **Step 3: Write `ui_state.rs`**

```rust
//! How the interface was left: the sidebar's width, and whether it was
//! folded away.
//!
//! A file of its own beside `projects.toml`, as `harness-settings.toml` is:
//! `config.toml` is written by hand and would lose its comments to a rewrite.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ConfigError;

/// The file, inside the configuration directory.
pub const FILE: &str = "ui.toml";

/// The narrowest the sidebar can be made: an icon, a short name and a state.
pub const MIN_SIDEBAR: u16 = 20;

/// The widest: beyond this it is taking the panes' room for blank.
pub const MAX_SIDEBAR: u16 = 60;

/// Its width until the user changes it.
pub const DEFAULT_SIDEBAR: u16 = 34;

/// What is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    /// The sidebar's width, in columns.
    pub sidebar_width: u16,
    /// Whether the sidebar is folded away.
    pub sidebar_collapsed: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            sidebar_width: DEFAULT_SIDEBAR,
            sidebar_collapsed: false,
        }
    }
}

/// What was kept in `dir`, its width brought into range. A file that cannot
/// be read is logged and taken as nothing kept: losing a width is not worth
/// stopping over.
#[must_use]
pub fn load(dir: &Path) -> UiState {
    match crate::store::read::<UiState>(dir, FILE) {
        Ok(state) => UiState {
            sidebar_width: state.sidebar_width.clamp(MIN_SIDEBAR, MAX_SIDEBAR),
            ..state
        },
        Err(error) => {
            tracing::warn!(%error, "ignoring a ui.toml that cannot be read");
            UiState::default()
        }
    }
}

/// Keeps `state` in `dir`.
pub fn save(dir: &Path, state: UiState) -> Result<(), ConfigError> {
    crate::store::update(dir, FILE, |kept: &mut UiState| {
        let changed = *kept != state;
        *kept = state;
        Ok(((), changed))
    })
}

#[cfg(test)]
mod tests;
```

`save` into a directory whose file is unparseable fails at `store::update`'s read. That is fine: the caller logs it, and the user's file is left alone.

- [ ] **Step 4: Commands, actions, keys**

`input.rs` `Action`, before `Quit`:

```rust
    /// Fold the sidebar away or bring it back; in a narrow window, open or
    /// close it as a drawer over the panes.
    ToggleSidebar,
    /// Make the sidebar this many columns wider; negative narrows it.
    ResizeSidebar(i16),
```

`keymap.rs`:
- `Command` gains `ToggleSidebar`, `SidebarNarrower` and `SidebarWider`.
- `NAMED` gains `toggle_sidebar`, `sidebar_narrower` and `sidebar_wider`.
- `label`: `"sidebar"`, `"narrower"`, `"wider"`.
- `stays`: add `Command::SidebarNarrower | Command::SidebarWider`.
- `action`: `ToggleSidebar => Action::ToggleSidebar`, `SidebarNarrower => Action::ResizeSidebar(-2)`, `SidebarWider => Action::ResizeSidebar(2)`.
- defaults: normal `(Chord::alt('s'), Command::ToggleSidebar)`; prefix `(Chord::char('b'), Command::ToggleSidebar)` next to `w`; session after `w`: `(Chord::char('b'), Command::ToggleSidebar)`, `(Chord::char('<'), Command::SidebarNarrower)`, `(Chord::char('>'), Command::SidebarWider)`.
- session `mode_help` test string gains `  b sidebar  < narrower  > wider` at its end.

- [ ] **Step 5: Failing app tests**

```rust
    #[test]
    fn the_sidebar_keeps_the_default_width_until_changed() {
        assert_eq!(sidebar::WIDTH, dispatch_config::ui_state::DEFAULT_SIDEBAR);
    }

    #[test]
    fn alt_s_folds_the_sidebar_away_and_back() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        let mut terminal = a_wide_terminal();
        drawn(&mut app, &mut terminal);
        let tiled = app.frames[0].1;
        assert_eq!(tiled.x, sidebar::WIDTH);

        press_alt(&mut app, 's');
        drawn(&mut app, &mut terminal);
        assert_eq!(app.frames[0].1.x, 0, "the panes take the whole width");
        assert!(app.sidebar_area.is_empty(), "and a click there is not the sidebar's");

        press_alt(&mut app, 's');
        drawn(&mut app, &mut terminal);
        assert_eq!(app.frames[0].1.x, sidebar::WIDTH);
    }

    #[test]
    fn session_keys_resize_the_sidebar_within_its_bounds() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        app.sidebar_width = dispatch_config::ui_state::MIN_SIDEBAR;
        app.handle(&Event::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)), Size::new(120, 30))
            .expect("handled");
        press(&mut app, KeyCode::Char('<'));
        assert_eq!(app.sidebar_width, dispatch_config::ui_state::MIN_SIDEBAR, "no narrower");
        press(&mut app, KeyCode::Char('>'));
        assert_eq!(app.sidebar_width, dispatch_config::ui_state::MIN_SIDEBAR + 2);
        assert!(app.router.key_mode().is_modal(), "resizing stays in the mode");
        app.sidebar_width = dispatch_config::ui_state::MAX_SIDEBAR;
        press(&mut app, KeyCode::Char('>'));
        assert_eq!(app.sidebar_width, dispatch_config::ui_state::MAX_SIDEBAR, "no wider");
    }

    #[test]
    fn dragging_the_sidebars_edge_resizes_it_and_keeps_the_width() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let (mut app, project, daemon, _sent) = attached_app();
        app.keep_ui_in(dir.path());
        spawn_several(&mut app, &daemon, project, 1);
        let mut terminal = a_wide_terminal();
        drawn(&mut app, &mut terminal);
        let edge = app.sidebar_area.x + app.sidebar_area.width - 1;
        let mouse = |kind, column| Event::Mouse(dispatch_tui::input::MouseEvent {
            kind, column, row: 5, modifiers: KeyModifiers::NONE,
        });
        use crossterm::event::MouseButton;
        app.handle(&mouse(MouseEventKind::Down(MouseButton::Left), edge), Size::new(120, 30)).expect("handled");
        app.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), edge + 6), Size::new(120, 30)).expect("handled");
        app.handle(&mouse(MouseEventKind::Up(MouseButton::Left), edge + 6), Size::new(120, 30)).expect("handled");

        assert_eq!(app.sidebar_width, sidebar::WIDTH + 6);
        assert_eq!(dispatch_config::ui_state::load(dir.path()).sidebar_width, sidebar::WIDTH + 6);
    }

    #[test]
    fn a_narrow_window_gives_the_panes_every_column_and_the_sidebar_is_a_drawer() {
        let (mut app, project, daemon, _sent) = attached_app();
        let panes = spawn_several(&mut app, &daemon, project, 2);
        app.rename(panes[0], "alpha-pane");
        app.focus_pane(panes[1]);
        let mut narrow = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 24))
            .expect("a test backend can be created");
        drawn(&mut app, &mut narrow);
        assert_eq!(app.frames.iter().map(|(_, r)| r.x).min(), Some(0));
        assert!(app.sidebar_area.is_empty());

        press_alt(&mut app, 's');
        drawn(&mut app, &mut narrow);
        assert!(!app.sidebar_area.is_empty(), "the drawer is open");
        assert_eq!(app.frames.iter().map(|(_, r)| r.x).min(), Some(0), "over the panes, not beside");

        // A click on a pane row picks it and closes the drawer.
        let screen = rendered_text(&narrow);
        let row = (app.sidebar_area.y..app.sidebar_area.y + app.sidebar_area.height)
            .find(|&y| {
                screen
                    .lines()
                    .nth(y as usize)
                    .is_some_and(|line| sidebar_column(line).contains("alpha-pane"))
            })
            .expect("the first pane's row is drawn in the drawer");
        click(&mut app, app.sidebar_area.x + 8, row);
        assert_eq!(app.state.focused_pane(), Some(panes[0]));
        assert!(!app.drawer_open);
    }

    #[test]
    fn widening_the_window_closes_the_drawer_and_brings_the_sidebar_back() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        let mut narrow = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 24))
            .expect("a test backend can be created");
        drawn(&mut app, &mut narrow);
        press_alt(&mut app, 's');
        drawn(&mut app, &mut narrow);

        let mut wide = a_wide_terminal();
        drawn(&mut app, &mut wide);

        assert!(!app.drawer_open);
        assert!(!app.sidebar_collapsed, "the drawer never touched the docked state");
        assert_eq!(app.frames[0].1.x, sidebar::WIDTH);
    }

    #[test]
    fn nothing_panics_in_a_window_narrower_than_the_sidebar() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 2);
        for width in [1, 5, 12, 19, 20, 21, 33, 34, 35, 79, 80] {
            let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 10))
                .expect("a test backend can be created");
            drawn(&mut app, &mut terminal);
            app.drawer_open = true;
            drawn(&mut app, &mut terminal);
            app.drawer_open = false;
            app.open_attention_picker();
            app.open_harness_picker();
            drawn(&mut app, &mut terminal);
            app.overlay = None;
        }
    }

    #[test]
    fn a_dialog_wider_than_the_panes_uses_the_whole_window() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        app.sidebar_width = dispatch_config::ui_state::MAX_SIDEBAR;
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24))
            .expect("a test backend can be created");
        let long = "a very long harness name that will not fit in thirty columns";
        app.overlay = Some(Overlay::Harness(Picker::new("New pane", vec![Item::new("x", long)])));
        drawn(&mut app, &mut terminal);
        assert!(rendered_text(&terminal).contains(long), "drawn whole, over the sidebar");
    }
```

The existing test `a_row_too_narrow_for_the_plus_draws_none_and_leaves_the_sidebar_alone` (35 columns) describes a layout that no longer exists: under 80 columns the sidebar takes no columns. Replace it with `a_narrow_window_gives_the_tab_row_the_whole_width`: at 35×30 the top row starts at column 0, holds the `+`, and a click on the `+` opens the picker. Check `the_plus_and_the_scroll_marks_stay_clickable_on_a_narrow_row` (56 columns) still passes. It computes positions from the drawn row, so it should.

Run: `cargo test -p dispatch sidebar drawer narrow dialog`
Expected: FAIL.

- [ ] **Step 6: Implement in `app.rs`**

Fields (doc each one; initial values in `App::new`):

- `sidebar_width: u16` (`sidebar::WIDTH`)
- `sidebar_collapsed: bool` (`false`)
- `drawer_open: bool` (`false`)
- `dragging_sidebar: bool` (`false`)
- `ui_dir: Option<PathBuf>` (`None`)
- `window: Rect` (`Rect::default()`): the whole window, as last drawn.

Constant: `/// Under this many columns the sidebar takes none, and opens as a drawer.  const NARROW: u16 = 80;`

```rust
    /// Keeps the sidebar's width and fold in `dir`, and takes up what was
    /// kept there.
    pub fn keep_ui_in(&mut self, dir: impl Into<PathBuf>) {
        let dir = dir.into();
        let kept = dispatch_config::ui_state::load(&dir);
        self.sidebar_width = kept.sidebar_width;
        self.sidebar_collapsed = kept.sidebar_collapsed;
        self.ui_dir = Some(dir);
    }

    /// Writes the sidebar's width and fold back, when this client keeps them.
    fn save_ui(&self) {
        let Some(dir) = &self.ui_dir else {
            return;
        };
        let state = dispatch_config::ui_state::UiState {
            sidebar_width: self.sidebar_width,
            sidebar_collapsed: self.sidebar_collapsed,
        };
        if let Err(error) = dispatch_config::ui_state::save(dir, state) {
            tracing::warn!(%error, "failed to keep the sidebar's width");
        }
    }

    fn toggle_sidebar(&mut self) {
        if self.window.width < NARROW {
            self.drawer_open = !self.drawer_open;
            return;
        }
        self.sidebar_collapsed = !self.sidebar_collapsed;
        self.save_ui();
    }

    fn resize_sidebar(&mut self, by: i16) {
        use dispatch_config::ui_state::{MAX_SIDEBAR, MIN_SIDEBAR};
        let width = self.sidebar_width.saturating_add_signed(by).clamp(MIN_SIDEBAR, MAX_SIDEBAR);
        if width != self.sidebar_width {
            self.sidebar_width = width;
            self.save_ui();
        }
    }
```

`main.rs`, after `app.keep_settings_in(&config_dir);`: `app.keep_ui_in(&config_dir);`.

`act_on`'s action match: `Action::ToggleSidebar => self.toggle_sidebar(),` and `Action::ResizeSidebar(by) => self.resize_sidebar(by),`.

**Layout in `draw`.** Replace the `sidebar_width` / `sidebar_area` / `panes_area` block with:

```rust
        self.window = area;
        let narrow = area.width < NARROW;
        // Widened past narrow, the drawer has nothing to be: the sidebar is
        // docked again, as it was left.
        if !narrow {
            self.drawer_open = false;
        }
        let docked = if narrow || self.sidebar_collapsed {
            0
        } else {
            self.sidebar_width.min(area.width)
        };
        let panes_area = Rect::new(
            body.x + docked,
            body.y,
            body.width.saturating_sub(docked),
            body.height,
        );
        // Empty when the sidebar is neither docked nor open as a drawer, so a
        // click there is never read as one on the sidebar.
        let sidebar_area = if docked > 0 {
            Rect::new(body.x, body.y, docked, body.height)
        } else if self.drawer_open {
            Rect::new(body.x, body.y, self.sidebar_width.min(body.width), body.height)
        } else {
            Rect::default()
        };
```

Move the code that settles the scroll, builds `sidebar_motion`, renders the `Sidebar` and computes `spinner_drawn` into `fn draw_sidebar(&mut self, frame: &mut Frame<'_>, area: Rect, now: Instant)`, unchanged except that it uses `area`. It returns early when `area.is_empty()`, after setting `self.sidebar_area = area` and `self.spinner_drawn = false`. In `draw`: if `docked > 0`, call it where the sidebar was drawn before. If the drawer is open, call it after `self.draw_panes(frame, now)`, preceded by `Clear.render(sidebar_area, frame.buffer_mut())`. Otherwise call it with `Rect::default()`. `draw_name` gets `Rect::new(top.x, top.y, docked, top.height)`: zero width draws nothing, which it already handles.

**Drawer input.** In `act_on`, right after the overlay early return:

```rust
        // The drawer lies over the panes: Esc or a click beside it puts it
        // away; a click on it is the sidebar's, below, and then puts it away.
        if self.drawer_open {
            match event {
                Event::Key(key) if key.kind == KeyEventKind::Press && key.code == KeyCode::Esc => {
                    self.drawer_open = false;
                    return Ok(());
                }
                Event::Mouse(mouse)
                    if matches!(mouse.kind, MouseEventKind::Down(_))
                        && !self.sidebar_area.contains(ratatui::layout::Position::new(mouse.column, mouse.row)) =>
                {
                    self.drawer_open = false;
                    return Ok(());
                }
                _ => {}
            }
        }
```

In the sidebar hit block, after `match hit { … }`: close the drawer for the hits that pick something, `if matches!(hit, sidebar::Hit::Project(_) | sidebar::Hit::Pane(_)) { self.drawer_open = false; }`. Write the match so `hit` stays readable afterwards: it is `Copy`.

**Edge drag.** Before the sidebar hit test in `act_on`:

```rust
        // The sidebar's right edge is a handle: pressed, dragged, let go.
        // Only a docked sidebar has one; the drawer's width is the docked
        // width, changed from a window wide enough to dock it.
        if let Event::Mouse(mouse) = event {
            let area = self.sidebar_area;
            let on_edge = !self.drawer_open
                && area.width > 0
                && mouse.column == area.x + area.width - 1
                && mouse.row >= area.y
                && mouse.row < area.y + area.height;
            match mouse.kind {
                MouseEventKind::Down(crossterm::event::MouseButton::Left) if on_edge => {
                    self.dragging_sidebar = true;
                    return Ok(());
                }
                MouseEventKind::Drag(crossterm::event::MouseButton::Left) if self.dragging_sidebar => {
                    use dispatch_config::ui_state::{MAX_SIDEBAR, MIN_SIDEBAR};
                    self.sidebar_width = (mouse.column + 1)
                        .saturating_sub(area.x)
                        .clamp(MIN_SIDEBAR, MAX_SIDEBAR);
                    return Ok(());
                }
                MouseEventKind::Up(_) if self.dragging_sidebar => {
                    self.dragging_sidebar = false;
                    self.save_ui();
                    return Ok(());
                }
                _ => {}
            }
        }
```

(If `crossterm` is not a direct dependency of the `dispatch` crate, use the re-export path the tests use. Check `dispatch/Cargo.toml`, or add `MouseButton` to `dispatch_tui::input`'s `pub use`.)

**Dialogs over the body.** Give `Picker`, `Prompt` and `SettingsForm` a `pub fn desired_width(&self) -> u16`: the width their `render` computes before clamping to the area. Move that computation into the method and call it from `render`, so there is one copy. For `Picker`, that is `(widest.max(title).max(hint) + 6).max(20)`. Add `Overlay::desired_width(&self) -> u16`:

```rust
    /// The width the overlay needs to be drawn whole.
    fn desired_width(&self) -> u16 {
        match self {
            Overlay::Harness(picker)
            | Overlay::Project(picker)
            | Overlay::Register(picker)
            | Overlay::Machine(picker)
            | Overlay::Attention(picker) => picker.desired_width(),
            Overlay::Settings { form, .. } => form.desired_width(),
            Overlay::OpenOn { prompt, .. }
            | Overlay::RenameTab { prompt, .. }
            | Overlay::CloseTab { prompt, .. } => prompt.desired_width(),
            Overlay::AddMachine(add) => add.prompt().desired_width(),
            // It shows as much of the directory as it has room for.
            Overlay::Browse(_) => 30,
            // Its fields and its task wrap to whatever width it is given.
            Overlay::Approval { .. } => 44,
        }
    }
```

`draw` passes `body` to `draw_overlay` too: `self.draw_overlay(frame, panes_area, body)`. At the top of `draw_overlay`, after setting the chrome:

```rust
        // Over the panes when they have the room, so the sidebar stays in
        // view; over the whole window when they do not.
        let panes_area = match &self.overlay {
            Some(overlay) if overlay.desired_width().saturating_add(2) > panes_area.width => body,
            _ => panes_area,
        };
```

- [ ] **Step 7: Docs**

`docs/usage.md`: under "The sidebar", say that `Alt s` folds it away and back, `Ctrl o <` / `Ctrl o >` or dragging its right edge resize it (20–60 columns), and under 80 columns it opens as a drawer over the panes. Add the keys to the keys table. `docs/configuration.md`: `ui.toml`, what it holds, that it is written by Dispatch, and that a bad value falls back to the default.

- [ ] **Step 8: Run the tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 9: Gate and commit**

```bash
git add -A crates dispatch docs
git commit -m "feat(tui): collapsible, resizable sidebar kept in ui.toml, a drawer in narrow windows"
```

---

### Task 8: Discoverability: empty-project card, command help, help chip

**Files:**
- Modify: `crates/dispatch-tui/src/picker.rs` (+tests) (filter), `keymap.rs` (+tests), `input.rs`
- Modify: `dispatch/src/app.rs`; the docs

**Interfaces:**
- Consumes: `Chrome` (T2), `Picker::desired_width` (T7).
- Produces:
  - `Picker::{with_filter, filter, push_filter, pop_filter}`;
  - `Command::Help`, `Command::describe(self) -> &'static str`, `Action::Help`;
  - `App::{perform, open_help, draw_empty}` and `Overlay::Help(Picker)`.

- [ ] **Step 1: Failing picker filter tests** (`picker/tests.rs`)

```rust
#[test]
fn a_filter_narrows_the_rows_and_the_selection_follows() {
    let mut picker = picker().with_filter();
    for c in "cod".chars() {
        picker.push_filter(c);
    }
    assert_eq!(picker.filter(), Some("cod"));
    assert_eq!(picker.selected().map(|item| item.id.as_str()), Some("codex"));
    picker.next();
    assert_eq!(picker.selected().map(|item| item.id.as_str()), Some("codex"), "one row left");

    picker.pop_filter();
    picker.pop_filter();
    picker.pop_filter();
    assert_eq!(picker.selected().map(|item| item.id.as_str()), Some("claude"));
}

#[test]
fn a_filter_matching_nothing_says_so_and_chooses_nothing() {
    let mut picker = picker().with_filter();
    picker.push_filter('z');
    assert!(picker.selected().is_none());
    assert!(text(&render(&picker, 40, 8)).contains("nothing to choose"));
}

#[test]
fn a_filter_is_drawn_above_the_rows() {
    let mut picker = picker().with_filter();
    picker.push_filter('a');
    let drawn = text(&render(&picker, 40, 8));
    let lines: Vec<&str> = drawn.lines().collect();
    let filter = lines.iter().position(|l| l.contains("> a")).expect("the filter line");
    let row = lines.iter().position(|l| l.contains("agy")).expect("a row");
    assert!(filter < row);
}
```

Run: `cargo test -p dispatch-tui filter`. Expected: FAIL to compile.

- [ ] **Step 2: Implement the filter**

`Picker` gains `filter: Option<String>` (doc: `/// What has been typed to narrow the rows, for a picker that takes typing.`), set to `None` in `new`. Add:

```rust
    /// The same picker, narrowed by what is typed into it.
    #[must_use]
    pub fn with_filter(mut self) -> Self {
        self.filter = Some(String::new());
        self
    }

    /// What has been typed, for a picker that takes typing.
    #[must_use]
    pub fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    /// Types `c` into the filter. The highlight goes back to the first row
    /// left, which is the one most likely meant.
    pub fn push_filter(&mut self, c: char) {
        if let Some(filter) = &mut self.filter {
            filter.push(c);
            self.selected = 0;
        }
    }

    /// Deletes the filter's last character.
    pub fn pop_filter(&mut self) {
        if let Some(filter) = &mut self.filter {
            filter.pop();
            self.selected = 0;
        }
    }

    /// The rows the filter leaves, in order: every row when there is none.
    /// Matched on the id, the label and the detail, ignoring case.
    fn shown(&self) -> Vec<&Item> {
        let needle = match &self.filter {
            Some(filter) if !filter.is_empty() => filter.to_lowercase(),
            _ => return self.items.iter().collect(),
        };
        self.items
            .iter()
            .filter(|item| {
                item.id.to_lowercase().contains(&needle)
                    || item.label.to_lowercase().contains(&needle)
                    || item
                        .detail
                        .as_ref()
                        .is_some_and(|detail| detail.to_lowercase().contains(&needle))
            })
            .collect()
    }
```

Change `selected()` to `self.shown().get(self.selected).copied()`, `next`/`previous` to wrap over `self.shown().len()` (returning early when it is zero), `select(id)` to search `self.shown()`, and `is_empty` to `self.shown().is_empty()`. `items()` still returns every row. In `render`:
- iterate `self.shown()`;
- with a filter present, the box is one row taller and its first inner row is `"> "` plus the filter plus `"▏"`, in `self.chrome.accent`, the rows starting one line lower;
- "nothing to choose" is shown when `shown()` is empty.

`desired_width` counts the filter line too, via `filter.chars().count() + 3`.

- [ ] **Step 3: `Command::Help` and `describe`**

`input.rs`: `/// Open the searchable list of every command.  Help,` before `Quit`.

`keymap.rs`:
- `Command::Help` goes in the enum and in `NAMED` as `(Command::Help, "help")`, with `label` `"help"` and `action` `Action::Help`.
- Defaults: normal `(Chord::alt('/'), Command::Help)`; prefix `(Chord::char('?'), Command::Help)`; session, after `>`: `(Chord::char('?'), Command::Help)`.
- Session `mode_help` string gains `  ? help`.
- Add `describe`:

```rust
    /// What it does, for the command help: a few words that stand alone,
    /// where [`Command::label`] leans on the mode row around it.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Command::PaneMode => "Pane mode",
            Command::TabMode => "Tab mode",
            Command::ScrollMode => "Scroll mode",
            Command::SessionMode => "Session mode",
            Command::Lock => "Lock the keyboard",
            Command::Unlock => "Unlock the keyboard",
            Command::Prefix => "Prefix",
            Command::LeaveMode => "Leave the mode",
            Command::None => "Nothing",
            Command::NewPane => "Start an agent or a shell",
            Command::ClosePane => "Close the focused pane",
            Command::Zoom => "Zoom the focused pane",
            Command::FocusLeft => "Focus the pane to the left",
            Command::FocusRight => "Focus the pane to the right",
            Command::FocusUp => "Focus the pane above",
            Command::FocusDown => "Focus the pane below",
            Command::FocusLeftOrTab => "Focus left, or the previous tab",
            Command::FocusRightOrTab => "Focus right, or the next tab",
            Command::FocusNext => "Focus the next pane",
            Command::ExpandChild => "Open a subagent's pane",
            Command::CollapseChild => "Close a subagent's pane",
            Command::NewTab => "New tab",
            Command::RenameTab => "Rename the tab",
            Command::CloseTab => "Close the tab",
            Command::PreviousTab => "Previous tab",
            Command::NextTab => "Next tab",
            Command::LastTab => "The tab before this one",
            Command::MovePaneLeft => "Move the pane to the previous tab",
            Command::MovePaneRight => "Move the pane to the next tab",
            Command::MoveTabLeft => "Move the tab left",
            Command::MoveTabRight => "Move the tab right",
            Command::GoToTab(_) => "Go to tab 1–9",
            Command::ScrollDown => "Scroll down a line",
            Command::ScrollUp => "Scroll up a line",
            Command::ScrollHalfDown => "Scroll down half a page",
            Command::ScrollHalfUp => "Scroll up half a page",
            Command::ScrollPageDown => "Scroll down a page",
            Command::ScrollPageUp => "Scroll up a page",
            Command::ScrollTop => "Scroll to the oldest output",
            Command::ScrollBottom | Command::LeaveScroll => "Back to live output",
            Command::ProjectPicker => "Switch project",
            Command::OpenProject => "Open a project",
            Command::AddMachine => "Add a machine",
            Command::HarnessManager => "Register a harness",
            Command::Approvals => "Answer delegation requests",
            Command::Fold => "Fold or unfold",
            Command::NextAttention => "Go to the next agent waiting on you",
            Command::AttentionPicker => "List the agents waiting on you",
            Command::ToggleSidebar => "Show or hide the sidebar",
            Command::SidebarNarrower => "Make the sidebar narrower",
            Command::SidebarWider => "Make the sidebar wider",
            Command::Help => "Command help",
            Command::Quit => "Quit",
        }
    }
```

Add a keymap test that every `NAMED` command's `describe()` is non-empty and that no two commands with an action share one, except the `ScrollBottom`/`LeaveScroll` pair.

- [ ] **Step 4: Failing app tests**

```rust
    #[test]
    fn an_empty_project_says_how_to_start() {
        let (mut app, _project, _daemon, _sent) = attached_app();
        let mut terminal = a_wide_terminal();
        drawn(&mut app, &mut terminal);
        let screen = rendered_text(&terminal);
        assert!(screen.contains("Start an agent") && screen.contains("Alt n"), "{screen}");
        assert!(screen.contains("Open a shell") && screen.contains("Alt n, then shell"), "{screen}");
        assert!(screen.contains("Command help") && screen.contains("Alt /"), "{screen}");

        rebind(&mut app, &[("normal", "Alt n", "none"), ("prefix", "n", "none"), ("pane", "n", "none")]);
        drawn(&mut app, &mut terminal);
        let screen = rendered_text(&terminal);
        assert!(!screen.contains("Start an agent"), "no key reaches it, so it is left out");
    }

    #[test]
    fn help_lists_commands_filters_them_and_runs_the_one_chosen() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        let mut terminal = a_wide_terminal();
        drawn(&mut app, &mut terminal);

        press_alt(&mut app, '/');
        let Some(Overlay::Help(picker)) = &app.overlay else {
            panic!("help is open");
        };
        assert!(picker.items().iter().any(|item| item.label == "Show or hide the sidebar"));

        for c in "sidebar".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);

        assert!(app.overlay.is_none());
        assert!(app.sidebar_collapsed, "the command ran");
    }

    #[test]
    fn help_with_no_match_keeps_the_list_open() {
        let (mut app, _project, _daemon, _sent) = attached_app();
        press_alt(&mut app, '/');
        for c in "zzzz".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.overlay, Some(Overlay::Help(_))), "nothing to run, so it stays");
        press(&mut app, KeyCode::Esc);
        assert!(app.overlay.is_none());
    }

    #[test]
    fn the_help_chip_survives_a_long_status_message() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        app.set_status("x".repeat(200));
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24))
            .expect("a test backend can be created");
        drawn(&mut app, &mut terminal);
        assert!(bottom_row(&terminal).trim_end().ends_with("Alt / help"), "{}", bottom_row(&terminal));
    }
```

Run: `cargo test -p dispatch help empty`. Expected: FAIL.

- [ ] **Step 5: Implement in `app.rs`**

**`perform`.** Move the body of `act_on`'s `match action { … }` into `fn perform(&mut self, action: Action)`, and have `act_on` call `self.perform(action)`. Add `Action::Help => self.open_help(),`.

**`Overlay::Help(Picker)`.** Add it to `picker()`, `picker_mut()`, `set_chrome` and `desired_width`, but *not* to `kind()`: help has its own key handling. Doc: `/// Every command, to find one and run it.`

```rust
    /// Opens the searchable list of every command, each with the keys that
    /// reach it.
    fn open_help(&mut self) {
        let keymap = self.router.keymap();
        let items: Vec<Item> = Command::NAMED
            .iter()
            .map(|(command, name)| (*command, (*name).to_string()))
            .chain(std::iter::once((Command::GoToTab(1), Command::GoToTab(1).name())))
            .filter(|(command, _)| command.action().is_some() && *command != Command::Help)
            .map(|(command, name)| {
                let keys = keymap.path_to(command).unwrap_or_else(|| "no key".to_string());
                Item::new(name, command.describe()).with_detail(keys)
            })
            .collect();
        self.overlay = Some(Overlay::Help(
            Picker::new("Commands", items)
                .with_filter()
                .with_hint("type to search  Enter run  Esc close"),
        ));
    }
```

In `handle_overlay`, before the generic `match key.code`, add:

```rust
        // Letters are the search, so only arrows, Enter, Backspace and Esc
        // act here.
        if let Some(Overlay::Help(picker)) = &mut self.overlay {
            match key.code {
                KeyCode::Esc => self.overlay = None,
                KeyCode::Down => picker.next(),
                KeyCode::Up => picker.previous(),
                KeyCode::Backspace => picker.pop_filter(),
                KeyCode::Enter => {
                    let action = picker
                        .selected()
                        .and_then(|item| Command::from_name(&item.id))
                        .and_then(Command::action);
                    if let Some(action) = action {
                        self.overlay = None;
                        self.perform(action);
                    }
                }
                KeyCode::Char(c)
                    if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    picker.push_filter(c);
                }
                _ => {}
            }
            if self.overlay.is_none() {
                self.open_next_approval();
            }
            return Ok(());
        }
```

(Borrow note: `picker` borrows `self.overlay`, so read the action out, let the borrow end, and only then call `self.perform`. The shape above does that, because `action` is owned. If the compiler still objects, restructure it as `let chosen = …; if let Some(action) = chosen { … }` outside the `if let`.)

**Empty-project card.** After `self.draw_panes(frame, now)` in `draw` (and before the drawer):

```rust
        if self.state.selected_project().is_some()
            && self.frames.is_empty()
            && self.closing.is_empty()
            && self.overlay.is_none()
        {
            self.draw_empty(frame, panes_area);
        }
```

```rust
    /// Says how to start, in a project with nothing open: the keys as bound,
    /// and a line left out when no key reaches it.
    fn draw_empty(&self, frame: &mut Frame<'_>, area: Rect) {
        let keymap = self.router.keymap();
        let new_pane = keymap.path_to(Command::NewPane);
        let lines: Vec<(&str, String)> = [
            ("Start an agent", new_pane.clone()),
            ("Open a shell", new_pane.map(|keys| format!("{keys}, then shell"))),
            ("Command help", keymap.path_to(Command::Help)),
        ]
        .into_iter()
        .filter_map(|(label, keys)| keys.map(|keys| (label, keys)))
        .collect();

        let label_width = lines.iter().map(|(label, _)| label.width()).max().unwrap_or(0);
        let width = lines
            .iter()
            .map(|(_, keys)| label_width + 4 + keys.width())
            .max()
            .unwrap_or(0);
        let width = u16::try_from(width).unwrap_or(u16::MAX).min(area.width);
        let height = u16::try_from(lines.len()).unwrap_or(0).min(area.height);
        if width == 0 || height == 0 {
            return;
        }
        let rect = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );

        let chrome = self.theme.chrome();
        let text: Vec<Line<'_>> = lines
            .into_iter()
            .map(|(label, keys)| {
                Line::from(vec![
                    Span::styled(format!("{label:<label_width$}    "), chrome.secondary),
                    Span::styled(keys, chrome.accent),
                ])
            })
            .collect();
        Paragraph::new(text).render(rect, frame.buffer_mut());
    }
```

(`width()` is `unicode_width::UnicodeWidthStr`. Check it is imported in `app.rs`, `grep -n UnicodeWidth dispatch/src/app.rs`, and import it if not.)

**Help chip.** At the end of `draw_status`, replace the final `Paragraph::new(text)…render(row, …)` with:

```rust
        // The way into the command help, pinned to the end of the row and
        // reserved first, so no message can push it off. A mode's row lists
        // its own keys instead, and locked, help is not reachable anyway.
        let chip = (mode == KeyMode::Normal)
            .then(|| keymap.path_to(Command::Help))
            .flatten()
            .map(|keys| format!("{keys} help"));
        let reserved = chip
            .as_ref()
            .map_or(0, |chip| u16::try_from(chip.width() + 2).unwrap_or(u16::MAX))
            .min(row.width);

        Paragraph::new(text)
            .style(style)
            .render(Rect::new(row.x, row.y, row.width - reserved, 1), frame.buffer_mut());
        if let Some(chip) = chip
            && reserved > 2
        {
            Paragraph::new(chip)
                .style(Style::default().fg(self.theme.faded))
                .alignment(ratatui::layout::Alignment::Right)
                .render(Rect::new(row.x + row.width - reserved, row.y, reserved, 1), frame.buffer_mut());
        }
```

- [ ] **Step 6: Docs**

`docs/usage.md`: `Alt /` opens command help (type to search, Enter runs), with prefix and session `?`. An empty project shows how to start. The status row always ends with the help key.

- [ ] **Step 7: Run the tests**

Run: `cargo test --workspace`
Expected: PASS. A test that asserted the status row's exact full text may now see `Alt / help` at the end. Change it to assert on the part it was about.

- [ ] **Step 8: Gate and commit**

```bash
git add -A crates dispatch docs
git commit -m "feat(tui): command help, an empty-project card, and a help key always on the status row"
```

---

## After the last task

- [ ] Run the whole gate once more on a clean tree: fmt, clippy, test.
- [ ] Run `cargo build --release` and try it by hand (the spec's hands-on checks):
  - Arrow-key bursts in pane mode: the tint moves continuously.
  - `Alt a` walks across blocked agents.
  - The sidebar resizes by drag and by keys, and the width comes back after a restart.
  - At 70 columns the sidebar is a drawer.
  - Dialogs match a custom terminal theme, and a light theme's faded text is readable.
  - `Alt /` finds and runs a command.
- [ ] Update the memory note `open-follow-ups.md` with anything parked.
