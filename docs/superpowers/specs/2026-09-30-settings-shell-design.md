# Quiet footer and Settings shell

Status: designed, not yet implemented.
Date: 2026-09-30.
Branch: `tui-settings`, cut from `TUI` at 4ca3dcd. `TUI` holds the polish pass and
the pointer foundation, and is not merged into `main`.
Source: sub-project B of
[the mouse and Settings handoff](2026-09-30-mouse-settings-handoff.md). Sub-project A
(the pointer foundation) is done. C (keyboard editor), D (agent defaults,
profiles, Machines) and E (tab drag, dividers, text selection) come later.

## Decisions taken with the user

- **Settings categories in B:** Mouse & layout, Appearance and Advanced. The
  Keyboard, Agents & models and Machines categories appear when C and D build
  them. Nothing is shown that cannot be edited yet, except Advanced, which is
  read-only by nature.
- **Icon fallback:** in B, as an Appearance setting.
- **Settings key:** `Alt ,` in normal mode, and `,` in session mode and after the
  prefix.
- **Message lifetime:** a transient message expires after 5 seconds. Errors and
  pending attention stay.
- **Approval:** the design below was approved in conversation.

## What it replaces

The handoff supersedes three parts of the polish pass:

- the `— Alt a` (and `— Ctrl a a`) key suffix on the footer's reminders;
- the per-mode key lists on the footer;
- the `Alt / help` chip.

The actions and bindings all stay. Keys remain discoverable in command help
(`Alt /`), in menus, and in Settings once C adds the Keyboard category.

## 1. Messages

`App.status: String` becomes:

```rust
struct Status {
    text: String,
    kind: StatusKind,   // Info | Error
    at: Instant,
}
```

- **Helpers.** `fn say(&mut self, text)` sets an Info message and
  `fn warn(&mut self, text)` sets an Error. `set_status`, the public setter
  `main.rs` uses, becomes `warn`, because every caller reports a failure at
  start-up.
- **Call sites.** Each existing `self.status = …` picks one. A refusal, a
  failure or an unreachable machine is an Error. A confirmation ("registered
  claude", "kept the sidebar") is Info. `self.status.clear()` becomes
  `self.clear_status()`.
- **Expiry.** Info expires `MESSAGE_FOR` (5 s) after `at`, read against
  `App::now()`. `next_frame` asks for a frame at the expiry instant when an Info
  message is showing and nothing else asks sooner, so an idle Dispatch still
  draws only when something changes.
- **Errors** stay until replaced, or until clicked (§2).
- **Mode text.** A key mode's entry still clears any message, as now.

## 2. The footer

The last row. It shows state, not shortcuts.

**Normal mode, left to right:**

| Part | When | Text | Click |
|---|---|---|---|
| Connection | attached and every machine reachable | `Connected` | — |
| Unreachable | any machine down | `laptop unreachable — its agents are still running` (names joined) | — |
| Message | a message is showing | its text, Errors in `Color::Red` | Error: dismisses it |
| Working | at least one `Running` pane | `4 working` | — |
| Attention | at least one waiting pane (`App::is_waiting`) | `2 need attention`, in the accent | opens the attention picker |
| Delegations | `pending` non-empty and no approval open | `1 delegation waiting`, in the accent | opens the approval |
| `[Activity]` | always | button | opens Activity (§3) |
| `[Settings]` | always | button, right-aligned | opens Settings (§5) |

- Parts are joined with ` · `, in that order. Standalone mode never says
  `Connected`.
- While a message shows, it takes the place of Connection and Working.
- Counts use `pane_order()` and `is_waiting`, as the attention picker does, so
  the footer, `Alt a` and the waiting list always agree.
- **Narrow windows.** When the row is too narrow, parts go in this order:
  Working, Connection, `[Activity]`, then the message is cut with `…`.
  Attention, Delegations and `[Settings]` are never dropped while there is room
  for them.
- **Buttons.** `[Activity]` and `[Settings]` are drawn like dialog buttons
  (`chrome.text`, `chrome.selection` while pressed) and use the pointer
  foundation: activate on release inside.

**Scroll mode:** `Scrollback · 120 lines above live` and a right-aligned
`[Return to live]`, which does what `Esc` does in scroll mode. The count comes
from a new `dispatch_pty::Terminal::scrollbar() -> Scrollbar { total, offset,
len }`, which wraps `ghostty_terminal_get(GHOSTTY_TERMINAL_DATA_SCROLLBAR = 9)`.
Lines above live is `total - offset - len`, saturating. On the alternate screen,
which has no scrollback, it reads `Scrollback`.

**Pane, tab and session modes:** the mode's title (`PANE`, `TAB`, `SESSION`),
then any message, and a right-aligned `[Done]` that leaves the mode
(`InputRouter::leave_mode`). There is no key list.

**Prefix:** `PREFIX`.

**Lock:** `LOCKED · Ctrl g unlocks`, with the key from `Keymap::lock_help` as
bound. There is no exit button: lock is left only by its key, as its contract
says.

**Style:** the normal mode text is `chrome.secondary`. A key mode keeps today's
`tab` background with bold text, so a mode is never missed.

**Hits:** `Target::Footer(FooterHit)`, where `FooterHit` is one of `Attention`,
`Delegations`, `Message`, `Activity`, `Settings`, `ReturnToLive` or `LeaveMode`.
The footer records its parts' rectangles in the hit map as it draws them.

## 3. Activity

`Overlay::Activity(Picker)`:
- one row per live pane, in `pane_order()`;
- label `"<project> · <title>"`;
- detail `status_text(status, reason)`, plus ` · 3m` while waiting (`blocked_since`);
- title `Activity`, buttons `[Cancel] [Go]`.

Enter, double-click or `[Go]` goes to the pane through `go_to_pane`. It is not a
log: it lists only what is true now.

`Command::Activity` (`activity`, label `activity`, describe "Show every pane's
state") joins the command catalog with no default key.

## 4. `preferences.toml`

A new `crates/dispatch-config/src/preferences.rs`. The file sits beside
`projects.toml` and `ui.toml`, and every read and write goes through `store`.

```rust
#[derive(Default, Serialize, Deserialize)] #[serde(default)]
pub struct Preferences {
    pub interface: InterfacePrefs,
    pub appearance: AppearancePrefs,
}
#[derive(Default, Serialize, Deserialize)] #[serde(default)]
pub struct InterfacePrefs {
    pub motion: Option<bool>,
    pub focus_follows_pointer: Option<bool>,
    pub hover_claims_panes: Option<bool>,
}
#[derive(Default, Serialize, Deserialize)] #[serde(default)]
pub struct AppearancePrefs {
    pub theme: Option<ThemeChoice>,   // "terminal" | "dark" | "light"
    pub accent: Option<Accent>,       // "terminal" | a preset name | "#rrggbb"
    pub icons: Option<IconSet>,       // "nerd" | "plain"
}
```

- **Missing keys.** A `None` field is not written, because `Option` fields carry
  `skip_serializing_if`.
- **Bad values.** An unknown preset or a malformed hex makes that one field fail
  to load. It is logged and treated as absent, as `harness_settings` treats a
  bad value. The rest of the file still loads.

**Effective values.** `pub fn effective(config: &Config, prefs: &Preferences) ->
Effective`. Each value is paired with its `Source` (`BuiltIn`, `ConfigFile`,
`Preferences`), following built-in < `config.toml` `[interface]` <
`preferences.toml`. Appearance keys exist only in preferences, so their source
is `BuiltIn` or `Preferences`.

**Applying a section.**
`pub fn apply(dir, section: Section, base: &Preferences, draft: &Preferences)
-> Result<Applied, ApplyError>`. `Section` is `Mouse` (the interface keys) or
`Appearance` (the appearance keys, plus `motion`).

1. Inside `store::update`, read the file as it is now.
2. Compare each field of the section that differs between `base` and `draft`
   against the file's current value.
3. If the file's value is not `base`'s, someone changed it since Settings
   opened. The field is a conflict: write nothing and return
   `ApplyError::Conflict(fields)`.
4. Otherwise write exactly the draft's edited fields. A field set back to
   "inherit" is removed. Fields the section did not edit are left as they are,
   so unrelated changes merge.
5. An unparseable file returns `ApplyError::Unreadable(path)` and is never
   replaced.

**Reset** a field: set it to `None` in the draft. Applying then removes it from
the file. In Settings, `Backspace` or `Delete` on a field, or a click on its
source (drawn `Settings ↺` while Settings sets it), resets it. A step that
lands on the value the field would inherit, while nothing is committed for it,
also stores `None`, so stepping there and back is no change.

`config.toml` is never written. Sidebar width and fold stay in `ui.toml`.

**Start-up.** `main.rs` loads preferences after `config.toml` and applies the
effective interface values (`set_motion`, `set_focus_follows_pointer`,
`set_hover_claims_panes`) and appearance (§7). It replaces the direct
`loaded.config.interface.*` calls.

## 5. The Settings workspace

**Opening.**
- `Command::Settings` (`settings`, label `settings`, describe "Open Settings")
  is bound to `Alt ,` in normal mode, and to `,` in session mode and after the
  prefix.
- It opens `Overlay::Preferences(SettingsView)`.
- Its view state is in a new `crates/dispatch-tui/src/settings_view.rs`. Its
  routing and Apply logic are in a new `dispatch/src/app/settings.rs`, with its
  own `impl App` block. `app.rs` gains only the overlay variant and its hooks.

**Sizing.**
- ≥100×28: centred, at most 110×34, with a one-cell margin.
- Smaller: the whole body.
- Under 72 columns: the category list becomes a one-row category switcher
  (`‹ Appearance ›`) at the top of the field column, so categories and fields
  stay on one page and no Back control is needed.
- Under 40×10: a message "Make the window larger to use Settings · Esc closes
  and discards changes" and the `[×]` only. Every other key is ignored, and
  `Esc` or `[×]` closes without the prompt, discarding pending edits.

**Layout:**

```text
┌ Settings ───────────────────────────────────────────── [×] ┐
│ Search settings…                                           │
├────────────────┬───────────────────────────────────────────┤
│ Mouse & layout │ Appearance                                │
│ Appearance     │ Theme        ‹ Follow terminal ›   Settings│
│ Advanced       │ Accent       ‹ Theme default ›     Built-in│
│                │ Motion       ‹ On ›               config   │
│                │ Icons        ‹ Nerd Font ›         Built-in│
│                │   Applies now · this window previews       │
├────────────────┴───────────────────────────────────────────┤
│ 2 changes in Appearance                 [Discard] [Apply]  │
└────────────────────────────────────────────────────────────┘
```

**Fields.** `Field { id, label, description, category, kind, value, base,
source, applies }`:
- `kind`: `Toggle`, `Choice(options)`, `Number { min, max }`, `Text` (the hex
  accent), `Action(ButtonId)` or `ReadOnly`;
- `applies`: one of "Applies now", "Applies to new panes", "Applies at next
  start", "Read only".

**Keys:**
- Tab and Shift-Tab move focus between the search field, categories, fields and
  footer buttons.
- `↑`/`↓` move within the focused list. `←`/`→` change a choice, toggle or
  number. Enter activates or toggles.
- Typing in the search field filters.
- Esc closes the innermost thing first (an open text edit, then the search
  focus), then the workspace.
- Leaving a category or closing with unapplied edits asks **Apply**,
  **Discard** or **Keep editing**, as a small prompt inside the workspace.

**Mouse.** Every row, control and button is clickable through the pointer
foundation. `DialogHit` gains `Category(usize)`, `Field(usize)`,
`FieldStep(usize, bool)`, `FieldReset(usize)`, `Search` and `Close`. An Action
row's `[ … ]` is a `DialogHit::Button` of its own id. Labels and padded control areas
are clickable, not single glyphs. The wheel scrolls the field list and never
changes a value. A click outside the workspace does nothing: it is a dialog,
and it never discards edits.

**Search** matches field labels, descriptions and category names, ignoring
case. Results replace the field list, each showing its category, and Enter or a
click jumps to the field in its category. With no results the query stays,
"No settings match" shows, and a `[Clear search]` button appears.

**Memory.** The selected category and field scroll are kept on `App` for the
session, so Settings reopens where it was.

**Focus and hover** have different styles: focus is `chrome.selection`, and
hover is underlined `chrome.text`, as the dialogs already do.

## 6. Categories

**Mouse & layout** (section `Mouse`, file `preferences.toml`):

| Field | Kind | Applies |
|---|---|---|
| Focus follows pointer | Toggle | now |
| Hover claims shared panes | Toggle | now |
| Sidebar width | Number 20–60 | now; written to `ui.toml` at once, not by Apply |
| Show sidebar | Toggle | now; written to `ui.toml` at once |
| Reset sidebar width | Action | now; back to 34 in `ui.toml` |

The sidebar fields act immediately, as dragging and `Alt s` do, and are never
part of the section's pending changes.

**Appearance** (section `Appearance`, file `preferences.toml`):

| Field | Kind | Applies |
|---|---|---|
| Theme | Choice: Follow terminal, Dark, Light | now (preview) |
| Accent | Choice: Theme default, Violet, Blue, Teal, Green, Amber, Red, Pink, Custom… | now (preview) |
| Custom accent | Text `#rrggbb`, shown when Accent is Custom | now (preview) |
| Motion | Toggle | now |
| Icons | Choice: Nerd Font, Plain | now (preview) |

**Advanced** is read-only:
- Paths: configuration file, preferences, `ui.toml`, harness directory, log file.
- Version: `CARGO_PKG_VERSION`.
- Mode: Standalone, or Attached to N machine(s).

## 7. Appearance

**Preview.** Editing an Appearance field rebuilds `App.theme` and the glyph set
from the draft at once, in this window only. **Discard** restores them from the
committed values, and so does closing Settings with the changes discarded.
**Apply** commits them.

**Theme:**

- **Follow terminal:** today's behaviour. It uses the palette the terminal
  reported at start-up, kept on `App` as `terminal_palette`.
- **Dark:** `Palette::FALLBACK`.
- **Light:** a new `Palette::LIGHT { background: #fafafa, foreground: #383a42,
  accent: #a626a4 }`.

All three pass through `Theme::new`, so the 4.5:1 secondary-text rule applies.

**Accent.** It replaces the palette's accent before `Theme::new`. Presets:

| Preset | RGB |
|---|---|
| Violet | `#b4a0f0` |
| Blue | `#61afef` |
| Teal | `#56b6c2` |
| Green | `#98c379` |
| Amber | `#e5c07b` |
| Red | `#e06c75` |
| Pink | `#ff79c6` |

**Theme default** (stored as `accent = "terminal"`) keeps the theme's own
accent: under Follow terminal it is the reported accent, palette slot 5; under
Dark or Light it is the preset's. In 256 colours, an accent that is not the
terminal's slot 5 is mapped through `at_depth` like every other mixed colour.

**Surface.** A Dark or Light preset's text is mixed for its own background, so
Dispatch paints that background behind its own surfaces: the sidebar, the tab
row, the footer, every dialog and menu, and the Settings workspace. Ordinary
text there takes the palette's foreground. Pane contents are never repainted,
and Follow terminal paints no background.

**Icons.** A new `crates/dispatch-tui/src/glyphs.rs` defines `pub struct Glyphs`.
It holds every glyph the chrome draws: the state glyphs (starting, running,
idle, blocked, done, failed, closed, unseen), the twisty open, shut and leaf,
repository and folder, and the default harness icon. It has two consts:

- `Glyphs::NERD`: today's Nerd Font values, unchanged.
- `Glyphs::PLAIN`: ASCII only, so nothing can be double-width or missing.

| Glyph | Plain |
|---|---|
| starting | `~` |
| running (still) | `>` |
| idle | `-` |
| blocked | `!` |
| done | `+` |
| failed | `x` |
| closed | `#` |
| unseen | `*` |
| twisty open | `v` |
| twisty shut | `>` |
| repository | `@` |
| folder | `/` |

The braille spinner stays in both sets, because braille is single-width.

- **Harness icons.** A harness's own icon is used only with Nerd Font. With
  Plain, a harness is drawn as the first letter of its display name, upper
  case.
- **Threading.** `Sidebar::with_glyphs(&Glyphs)`, and `App` passes the same set
  to the tab chips and `status_text` callers that draw glyphs. The existing
  `pub const` glyphs in `sidebar.rs` stay, as `Glyphs::NERD`'s source, so
  nothing outside changes meaning.

## 8. What this does not change

- The daemon and the protocol.
- `config.toml`, which is never written. `[keys]` stays as is; C adds key
  overrides to `preferences.toml`.
- Keyboard behaviour, except the new Settings key and the removal of the help
  chip. The footer's keys move to help and menus.
- The harness settings form (`Overlay::Settings`), which is D's to fold in.

## 9. Tests

- **Messages:** Info expires at 5 s with the hand clock, and `next_frame`
  reports the expiry. An Error stays and a click dismisses it. A key mode
  clears the message.
- **Footer:**
  - every mode's text;
  - Connected only when attached;
  - unreachable names shown;
  - counts agree with `pane_order` and `is_waiting`;
  - the narrow-width drop order at 120, 80, 60 and 40 columns;
  - a click on each part does its action;
  - `[Return to live]` leaves scroll mode;
  - `[Done]` leaves pane mode;
  - lock has no button and shows its key as bound.
- **Scrollbar:** `Terminal::scrollbar()` on a terminal fed more lines than its
  height, at the bottom and scrolled up three lines.
- **Activity:** rows, order, and going to a pane in another project.
- **Preferences:**
  - load, effective values and sources for each precedence case;
  - reset;
  - applying one section leaves the other section's fields in the file
    untouched;
  - a same-field change on disk gives a conflict and no write;
  - a corrupt file is unreadable and not replaced;
  - a bad accent value is skipped and the rest loads;
  - a round trip.
- **Settings view:**
  - renders at 120×40, 100×28, 80×24, 60×18, 40×10 and 20×6 without panic;
  - `[×]` always reachable;
  - under 72 columns the category picker shows;
  - search results, the no-result state and Clear search;
  - Tab order.
- **Acceptance**, as event sequences:
  - **Mouse only:** click `[Settings]`, click Appearance, click Theme's `›` to
    Light, click `[Apply]`, click `[×]`. `preferences.toml` says
    `theme = "light"` and `App.theme` is the light one.
  - **Keyboard only:** `Alt ,`, then Tab and arrows to the same change, Enter
    on Apply, Esc. Same result.
  - **Discard:** preview Dark, then `[Discard]`. The theme is back.
- **Glyphs:** `Glyphs::PLAIN` contains only ASCII. A sidebar rendered with
  Plain contains no codepoint in U+E000–U+F8FF.

## 10. Order of work

1. Messages: `Status`, `say` and `warn`, expiry, and `next_frame`.
2. `Terminal::scrollbar()` in `dispatch-pty`.
3. The new footer: parts, narrow drop order, clicks, modes, and removal of the
   chip, suffixes and key lists.
4. Activity overlay and command.
5. `preferences.toml`: types, load, effective with sources, apply with
   conflicts, reset, and wiring at start-up.
6. `Glyphs`: NERD and PLAIN, and threading through the sidebar and tab chips.
7. Theme presets and accent: `Palette::LIGHT`, the presets, `terminal_palette`,
   and the theme rebuilt from effective appearance.
8. `SettingsView` widget: layout, sizing rules, search, focus, and its layout
   for hits.
9. `app/settings.rs`: overlay, keys and mouse, preview, Apply, Discard and
   conflict display, the leave-with-edits prompt, and `Command::Settings`.
10. Categories wired: Mouse & layout, Appearance, Advanced.
11. Docs: `docs/usage.md` (footer, Activity, Settings),
    `docs/configuration.md` (`preferences.toml` and precedence).

Each step leaves `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace --no-fail-fast` green.
