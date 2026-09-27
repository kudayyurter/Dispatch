# D — Keybindings: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Dispatch's hand-written key matches with one keymap table per mode (normal, prefix, pane, tab, scroll, session, lock), zellij-style, rebindable from `[keys]` in `config.toml`.

**Architecture:** A new `dispatch-tui::keymap` module owns three things:
- `Chord`: a key as written, e.g. `"Ctrl t"`;
- `Command`: every named thing a key can do;
- `Keymap`: an ordered chord → command list per `KeyMode`, with the defaults, `[keys]` overrides, safety rails, and status-row help.

`InputRouter` becomes a lookup over the keymap. `dispatch-config` carries `[keys]` as raw text, and the interface decides what it means. The app gains scroll-mode actions, `focus_next`, and a status row generated from the keymap.

**Tech Stack:** Rust 2024, ratatui 0.29, crossterm 0.28 (`KeyCode`, `KeyModifiers`), serde + toml.

**Spec:** `docs/superpowers/specs/2026-09-27-keybindings-design.md`

## Global Constraints

- Platform `#[cfg]` attributes live only in `dispatch-os`. Tests may carry `#[cfg(...)]`.
- CI: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` on Linux, macOS and Windows-gnu. Windows is already red on `main` in `dispatch-os/src/host.rs`/`ipc.rs`; add nothing new to it.
- No new dependencies. The protocol does not change.
- Mode names, in `[keys]` and in code: `normal`, `prefix`, `pane`, `tab`, `scroll`, `session`, `lock`.
- Chord text:
  - Modifier words `Ctrl`, `Alt`, `Shift`, `Super`, case-insensitive.
  - Named keys, case-insensitive: `Esc`, `Enter`, `Tab`, `Space`, `Backspace`, `Left`, `Right`, `Up`, `Down`, `PageUp`, `PageDown`, `Home`, `End`, `Delete`, `Insert`, and `F1`–`F12`.
  - A single character is case-sensitive, and a capital means shift. `Shift` with a lowercase letter is its capital, and `Shift Tab` is back-tab.
- The status row shows the arrow keys as `←` `→` `↑` `↓`.
- Command names are snake_case, verbatim from the spec. `none` unbinds a key, and `clear = true` drops a mode's defaults.
- Safety rails:
  - lock always has an unlock (the default `Ctrl g` is kept if nothing unlocks);
  - `Esc` always leaves pane, tab, scroll and session mode;
  - a mode-entering command is accepted only in `normal` and `prefix`;
  - `unlock` is accepted only in `lock`, and `lock` accepts only `unlock`.
- Warning texts, verbatim, where `<where>` is `keys.<mode>."<chord>"`:
  - `keys.<mode>: not a mode`
  - `<where>: not a key`
  - `<where>: a command's name, in quotes`
  - `<where>: no command "<name>"`
  - `<where>: a mode is entered from normal mode or the prefix`
  - `<where>: lock mode only unlocks`
  - `<where>: only lock mode unlocks`
  - `keys.<mode>.clear: true or false`
  - `keys.lock: nothing unlocks, so Ctrl g still does`
  - `keys.<mode>: Esc always leaves the mode`
- Status row:
  - Prefix mode shows `PREFIX`.
  - Lock mode shows `LOCKED  <chord> unlock`.
  - A modal mode shows `<TITLE>  <keys>`, and puts a status message between the title and the keys.
  - The normal key help reads `Ctrl p pane  Ctrl t tabs  Ctrl s scroll  Ctrl o session  Ctrl g lock  Ctrl a prefix`, generated from the table.
  - Every mode but normal is drawn in the highlighted style.
- `Ctrl q` is not bound by default. `^a [` enters scroll mode.
- Comments explain why, in the surrounding voice. Commits are `type(scope): lowercase summary`, ending with the session's attribution trailer lines.

## Review Focus

1. **How the terminal reports a capital.** A binding written `H` must fire whether crossterm reports `Char('H')` with SHIFT or without. Pinned in Task 1 by `a_key_event_becomes_the_chord_it_is_written_as`.
2. **A config that would trap the user.** `Esc` unbound in a mode, or lock left with no unlock, is repaired and reported. Pinned in Task 3 by `lock_always_unlocks` and `esc_always_leaves_a_mode`.
3. **A mode key pressed twice reaches the pane.** For example, `Ctrl s Ctrl s` gives a shell its `Ctrl s`. Pinned in Task 4 by `a_mode_key_twice_goes_to_the_pane`.
4. **Old `^a` habits.** Every prefix command does what it did before, except `[`. Pinned in Task 4 by `every_prefix_command_is_bound_as_before`.
5. **Scroll mode when there is nothing to scroll, or the pane goes.** No scrollback does no harm, and a pane that closes ends the mode. Pinned in Task 4 by `scroll_mode_on_a_pane_with_no_scrollback_is_harmless` and in Task 5 by `scroll_mode_ends_when_its_pane_goes`.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/dispatch-tui/src/keymap.rs` (new) + `keymap/tests.rs` | `Chord`, `ChordError`, `KeyMode`, `Command`, `Keymap` (defaults, overrides, rails, help) |
| `crates/dispatch-tui/src/lib.rs` | `pub mod keymap;` re-exports |
| `crates/dispatch-tui/src/input.rs` + `input/tests.rs` | the router over the keymap; `Action` gains `FocusNext`, `ScrollHalfPages`, `ScrollPages`, `ScrollToTop`, `ScrollToBottom`, loses `Scrollback`; `Prefix`, `command_for`, `tab_key`, `direct` go |
| `crates/dispatch-config/src/config.rs` + `config/tests.rs` | `KeysConfig`, `KeyValue`, `Config.keys`, `unknown_keys` |
| `crates/dispatch-config/src/lib.rs` | re-exports |
| `dispatch/src/app.rs` | new actions, `focus_next`, `scroll_view`, `scroll_pages`, `set_keymap`, generated status row, clear-on-entry for every mode, scroll mode ends with its pane |
| `dispatch/src/main.rs` | keymap from `config.keys`, warnings logged |
| `dispatch/tests/end_to_end.rs` | scroll, lock and a rebinding end to end |
| `README.md` | the key sections |

---
### Task 1: Chords

**Files:**
- Create: `crates/dispatch-tui/src/keymap.rs`, `crates/dispatch-tui/src/keymap/tests.rs`
- Modify: `crates/dispatch-tui/src/lib.rs` (`pub mod keymap;`)

**Interfaces:**
- Produces (used by every later task):
  - `dispatch_tui::keymap::Chord { pub code: KeyCode, pub modifiers: KeyModifiers }`, which is `Copy + Eq + Hash + Display`.
  - Constructors: `Chord::new(KeyCode, KeyModifiers)`, `Chord::char(char)`, `Chord::ctrl(char)`, `Chord::alt(char)`, `Chord::key(KeyCode)`, `Chord::alt_key(KeyCode)`.
  - `Chord::from_event(&KeyEvent) -> Chord`.
  - `Chord::parse(&str) -> Result<Chord, ChordError>`.
  - `Chord::short(&self) -> String`.
  - `ChordError(pub String)`.

- [ ] **Step 1: Write the failing tests**

Create `crates/dispatch-tui/src/keymap/tests.rs`:

```rust
//! Tests for chords, commands and the keymap.

use super::*;

use crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};

fn parsed(text: &str) -> Chord {
    Chord::parse(text).unwrap_or_else(|error| panic!("{text:?} parses: {error}"))
}

fn event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

#[test]
fn a_plain_character_is_itself() {
    assert_eq!(parsed("x"), Chord::char('x'));
    assert_eq!(parsed("["), Chord::char('['));
    assert_eq!(parsed("H"), Chord::char('H'));
}

#[test]
fn modifier_words_are_read_in_any_case() {
    assert_eq!(parsed("Ctrl t"), Chord::ctrl('t'));
    assert_eq!(parsed("ctrl t"), Chord::ctrl('t'));
    assert_eq!(parsed("CTRL t"), Chord::ctrl('t'));
    assert_eq!(parsed("Alt n"), Chord::alt('n'));
    assert_eq!(
        parsed("Ctrl Alt x"),
        Chord::new(KeyCode::Char('x'), KeyModifiers::CONTROL | KeyModifiers::ALT)
    );
    assert_eq!(
        parsed("Super k"),
        Chord::new(KeyCode::Char('k'), KeyModifiers::SUPER)
    );
}

#[test]
fn shift_on_a_letter_is_its_capital() {
    assert_eq!(parsed("Shift h"), parsed("H"));
    assert_eq!(parsed("Ctrl Shift t"), Chord::new(KeyCode::Char('T'), KeyModifiers::CONTROL));
}

#[test]
fn named_keys_are_read_in_any_case() {
    let cases = [
        ("Esc", KeyCode::Esc),
        ("escape", KeyCode::Esc),
        ("Enter", KeyCode::Enter),
        ("Tab", KeyCode::Tab),
        ("Space", KeyCode::Char(' ')),
        ("Backspace", KeyCode::Backspace),
        ("Left", KeyCode::Left),
        ("right", KeyCode::Right),
        ("Up", KeyCode::Up),
        ("Down", KeyCode::Down),
        ("PageUp", KeyCode::PageUp),
        ("pagedown", KeyCode::PageDown),
        ("Home", KeyCode::Home),
        ("End", KeyCode::End),
        ("Delete", KeyCode::Delete),
        ("Insert", KeyCode::Insert),
        ("F1", KeyCode::F(1)),
        ("f12", KeyCode::F(12)),
    ];

    for (text, code) in cases {
        assert_eq!(parsed(text), Chord::key(code), "{text:?}");
    }
    assert_eq!(parsed("Shift Tab"), Chord::new(KeyCode::Tab, KeyModifiers::SHIFT));
    assert_eq!(parsed("Alt Left"), Chord::alt_key(KeyCode::Left));
}

#[test]
fn what_is_not_a_key_is_refused_by_its_text() {
    for text in ["", "Ctrl", "Hyper x", "F13", "F0", "xy", "Ctrl ?x"] {
        assert_eq!(
            Chord::parse(text),
            Err(ChordError(text.to_string())),
            "{text:?}"
        );
    }
    assert_eq!(
        ChordError("Ctrl ?x".into()).to_string(),
        "\"Ctrl ?x\": not a key"
    );
}

#[test]
fn a_chord_prints_as_it_is_written() {
    for text in [
        "Ctrl t",
        "Alt n",
        "x",
        "H",
        "[",
        "Esc",
        "Enter",
        "Shift Tab",
        "F5",
        "Space",
        "Ctrl Alt x",
        "PageUp",
        "Left",
        "Super k",
    ] {
        assert_eq!(parsed(text).to_string(), text);
    }
}

#[test]
fn the_short_form_draws_the_arrows() {
    assert_eq!(parsed("Left").short(), "←");
    assert_eq!(parsed("Right").short(), "→");
    assert_eq!(parsed("Up").short(), "↑");
    assert_eq!(parsed("Down").short(), "↓");
    assert_eq!(parsed("Alt Left").short(), "Alt ←");
    assert_eq!(parsed("Ctrl t").short(), "Ctrl t");
}

#[test]
fn a_key_event_becomes_the_chord_it_is_written_as() {
    // Terminals disagree on whether a capital arrives with SHIFT set; the
    // character already says it, so a binding written `H` fires either way.
    assert_eq!(
        Chord::from_event(&event(KeyCode::Char('H'), KeyModifiers::SHIFT)),
        parsed("H")
    );
    assert_eq!(
        Chord::from_event(&event(KeyCode::Char('H'), KeyModifiers::NONE)),
        parsed("H")
    );
    assert_eq!(
        Chord::from_event(&event(KeyCode::Char('t'), KeyModifiers::CONTROL)),
        parsed("Ctrl t")
    );
    assert_eq!(
        Chord::from_event(&event(KeyCode::BackTab, KeyModifiers::SHIFT)),
        parsed("Shift Tab")
    );
    assert_eq!(
        Chord::from_event(&event(KeyCode::Char('['), KeyModifiers::SHIFT)),
        parsed("[")
    );
}
```

Create `crates/dispatch-tui/src/keymap.rs` holding only its doc comment and `#[cfg(test)] mod tests;`. Add `pub mod keymap;` to `crates/dispatch-tui/src/lib.rs`, keeping the module list alphabetical.

Run: `cargo test -p dispatch-tui keymap`
Expected: FAIL to compile, because `Chord` and `ChordError` are not found.

- [ ] **Step 2: Implement**

Replace `crates/dispatch-tui/src/keymap.rs` with:

```rust
//! Keys as a user writes them, and what each mode binds them to.
//!
//! One table per mode rather than a match per mode: the built-in keys and
//! the user's `[keys]` are then the same kind of thing, and the status row
//! can say what a mode's keys do by reading the table that runs them.

use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The modifiers a chord can name. Anything else a terminal reports (hyper,
/// meta) is left off, so it cannot quietly stop a binding from matching.
const MODIFIERS: KeyModifiers = KeyModifiers::CONTROL
    .union(KeyModifiers::ALT)
    .union(KeyModifiers::SHIFT)
    .union(KeyModifiers::SUPER);

/// A key plus the modifiers held with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    /// The key.
    pub code: KeyCode,
    /// The modifiers held with it. Never `SHIFT` on a character: its case
    /// already says so.
    pub modifiers: KeyModifiers,
}

/// Text that does not name a key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?}: not a key")]
pub struct ChordError(pub String);

impl Chord {
    /// A key with modifiers.
    #[must_use]
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    /// A character on its own.
    #[must_use]
    pub const fn char(c: char) -> Self {
        Self::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// `Ctrl` and a character.
    #[must_use]
    pub const fn ctrl(c: char) -> Self {
        Self::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    /// `Alt` and a character.
    #[must_use]
    pub const fn alt(c: char) -> Self {
        Self::new(KeyCode::Char(c), KeyModifiers::ALT)
    }

    /// A named key on its own.
    #[must_use]
    pub const fn key(code: KeyCode) -> Self {
        Self::new(code, KeyModifiers::NONE)
    }

    /// `Alt` and a named key.
    #[must_use]
    pub const fn alt_key(code: KeyCode) -> Self {
        Self::new(code, KeyModifiers::ALT)
    }

    /// The chord a key press is.
    ///
    /// A character's case carries shift, and terminals disagree on whether
    /// they also set the modifier for it, so it is dropped: a binding
    /// written `H` fires however the capital arrived. Back-tab is `Shift
    /// Tab`, as it is written.
    #[must_use]
    pub fn from_event(event: &KeyEvent) -> Self {
        let mut modifiers = event.modifiers & MODIFIERS;
        let code = match event.code {
            KeyCode::BackTab => {
                modifiers |= KeyModifiers::SHIFT;
                KeyCode::Tab
            }
            code => code,
        };
        if matches!(code, KeyCode::Char(_)) {
            modifiers.remove(KeyModifiers::SHIFT);
        }
        Self { code, modifiers }
    }

    /// Reads a chord written as a user writes one: `"Ctrl t"`, `"Alt n"`,
    /// `"x"`, `"H"`, `"Shift Tab"`, `"F5"`.
    pub fn parse(text: &str) -> Result<Self, ChordError> {
        let refuse = || ChordError(text.to_string());
        let words: Vec<&str> = text.split_whitespace().collect();
        let (key, held) = words.split_last().ok_or_else(refuse)?;

        let mut modifiers = KeyModifiers::NONE;
        for word in held {
            modifiers |= match word.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => KeyModifiers::CONTROL,
                "alt" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                "super" => KeyModifiers::SUPER,
                _ => return Err(refuse()),
            };
        }

        let code = match named_key(key) {
            Some(code) => code,
            None => {
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => KeyCode::Char(c),
                    _ => return Err(refuse()),
                }
            }
        };

        // Shift on a letter is its capital, the same chord a terminal
        // reports for it.
        let code = match code {
            KeyCode::Char(c) if modifiers.contains(KeyModifiers::SHIFT) => {
                modifiers.remove(KeyModifiers::SHIFT);
                KeyCode::Char(c.to_ascii_uppercase())
            }
            code => code,
        };

        Ok(Self { code, modifiers })
    }

    /// How the status row writes this chord: as it is written, with the
    /// arrow keys drawn as arrows, which says the same in a quarter of the
    /// room.
    #[must_use]
    pub fn short(&self) -> String {
        let arrow = match self.code {
            KeyCode::Left => "←",
            KeyCode::Right => "→",
            KeyCode::Up => "↑",
            KeyCode::Down => "↓",
            _ => return self.to_string(),
        };
        let mut text = modifier_words(self.modifiers);
        text.push_str(arrow);
        text
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&modifier_words(self.modifiers))?;
        match self.code {
            KeyCode::Char(' ') => f.write_str("Space"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::Esc => f.write_str("Esc"),
            KeyCode::Enter => f.write_str("Enter"),
            KeyCode::Tab => f.write_str("Tab"),
            KeyCode::Backspace => f.write_str("Backspace"),
            KeyCode::Left => f.write_str("Left"),
            KeyCode::Right => f.write_str("Right"),
            KeyCode::Up => f.write_str("Up"),
            KeyCode::Down => f.write_str("Down"),
            KeyCode::PageUp => f.write_str("PageUp"),
            KeyCode::PageDown => f.write_str("PageDown"),
            KeyCode::Home => f.write_str("Home"),
            KeyCode::End => f.write_str("End"),
            KeyCode::Delete => f.write_str("Delete"),
            KeyCode::Insert => f.write_str("Insert"),
            KeyCode::F(n) => write!(f, "F{n}"),
            other => write!(f, "{other:?}"),
        }
    }
}

/// `"Ctrl Alt "` and so on, in the order they are written.
fn modifier_words(modifiers: KeyModifiers) -> String {
    let mut words = String::new();
    for (flag, word) in [
        (KeyModifiers::CONTROL, "Ctrl "),
        (KeyModifiers::ALT, "Alt "),
        (KeyModifiers::SHIFT, "Shift "),
        (KeyModifiers::SUPER, "Super "),
    ] {
        if modifiers.contains(flag) {
            words.push_str(word);
        }
    }
    words
}

/// The key a name stands for, in any case.
fn named_key(word: &str) -> Option<KeyCode> {
    let lower = word.to_ascii_lowercase();
    Some(match lower.as_str() {
        "esc" | "escape" => KeyCode::Esc,
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "space" => KeyCode::Char(' '),
        "backspace" => KeyCode::Backspace,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "delete" | "del" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        _ => {
            let number: u8 = lower.strip_prefix('f')?.parse().ok()?;
            if !(1..=12).contains(&number) {
                return None;
            }
            KeyCode::F(number)
        }
    })
}

#[cfg(test)]
mod tests;
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p dispatch-tui keymap`
Expected: PASS, 8 tests.

- [ ] **Step 4: Lint and commit**

Run: `cargo fmt --all --check && cargo clippy -p dispatch-tui --all-targets -- -D warnings`

```bash
git add crates/dispatch-tui/src/keymap.rs crates/dispatch-tui/src/keymap/tests.rs crates/dispatch-tui/src/lib.rs
git commit -m "feat(tui): chords written the way a user writes them"
```

---

### Task 2: Commands, modes and the default keymap

**Files:**
- Modify: `crates/dispatch-tui/src/keymap.rs` (add `KeyMode`, `Command`, `Keymap`)
- Modify: `crates/dispatch-tui/src/keymap/tests.rs`
- Modify: `crates/dispatch-tui/src/input.rs`: `Action` gains `FocusNext`, `ScrollHalfPages(isize)`, `ScrollPages(isize)`, `ScrollToTop` and `ScrollToBottom`. Only the enum changes here; the router is Task 4.
- Modify: `dispatch/src/app.rs`: temporary arms for the five new `Action`s, so `handle`'s match stays exhaustive (see Step 3).

**Interfaces:**
- Consumes: Task 1's `Chord`.
- Produces:
  - `KeyMode::{Normal, Prefix, Pane, Tab, Scroll, Session, Lock}` (`Default` = `Normal`), with:
    - `KeyMode::ALL`
    - `name(self) -> &'static str`, `from_name(&str) -> Option<KeyMode>`
    - `is_modal(self) -> bool` (Pane, Tab, Scroll, Session)
    - `title(self) -> &'static str` (`PANE`, `TAB`, `SCROLL`, `SESSION`, otherwise the name)
    - `entered_by(self) -> Option<Command>`, the command that enters it
  - `Command` (the full list is in the code below), with:
    - `name(self) -> String`, `from_name(&str) -> Option<Command>`
    - `label(self) -> &'static str`
    - `stays(self) -> bool`
    - `enters(self) -> Option<KeyMode>`
    - `action(self) -> Option<Action>`
  - `Keymap`, with:
    - `Keymap::defaults()` (also `Default`)
    - `bindings(&self, KeyMode) -> &[(Chord, Command)]`
    - `lookup(&self, KeyMode, &Chord) -> Option<Command>`
    - `chords_for(&self, KeyMode, Command) -> Vec<Chord>`
    - `mode_help(&self, KeyMode) -> String`, `normal_help(&self) -> String`, `lock_help(&self) -> String`
    - `pub(crate) bind`, `pub(crate) unbind`
  - `Action::{FocusNext, ScrollHalfPages(isize), ScrollPages(isize), ScrollToTop, ScrollToBottom}`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/dispatch-tui/src/keymap/tests.rs`. `Action` and `Direction` come through `use super::*`, once Step 3 imports them in `keymap.rs`.

```rust
#[test]
fn the_defaults_bind_what_the_spec_lists() {
    let keymap = Keymap::defaults();
    let cases = [
        (KeyMode::Normal, "Ctrl p", Command::PaneMode),
        (KeyMode::Normal, "Ctrl t", Command::TabMode),
        (KeyMode::Normal, "Ctrl s", Command::ScrollMode),
        (KeyMode::Normal, "Ctrl o", Command::SessionMode),
        (KeyMode::Normal, "Ctrl g", Command::Lock),
        (KeyMode::Normal, "Ctrl a", Command::Prefix),
        (KeyMode::Normal, "Alt n", Command::NewPane),
        (KeyMode::Normal, "Alt i", Command::MoveTabLeft),
        (KeyMode::Normal, "Alt o", Command::MoveTabRight),
        (KeyMode::Normal, "Alt Left", Command::FocusLeftOrTab),
        (KeyMode::Normal, "Alt l", Command::FocusRightOrTab),
        (KeyMode::Normal, "Alt k", Command::FocusUp),
        (KeyMode::Normal, "Alt Down", Command::FocusDown),
        (KeyMode::Prefix, "n", Command::NewPane),
        (KeyMode::Prefix, "x", Command::ClosePane),
        (KeyMode::Prefix, "z", Command::Zoom),
        (KeyMode::Prefix, "h", Command::FocusLeft),
        (KeyMode::Prefix, "p", Command::ProjectPicker),
        (KeyMode::Prefix, "H", Command::HarnessManager),
        (KeyMode::Prefix, "a", Command::Approvals),
        (KeyMode::Prefix, "s", Command::ExpandChild),
        (KeyMode::Prefix, "c", Command::CollapseChild),
        (KeyMode::Prefix, "f", Command::Fold),
        (KeyMode::Prefix, "o", Command::OpenProject),
        (KeyMode::Prefix, "m", Command::AddMachine),
        (KeyMode::Prefix, "[", Command::ScrollMode),
        (KeyMode::Prefix, "4", Command::GoToTab(4)),
        (KeyMode::Prefix, "Tab", Command::NextTab),
        (KeyMode::Prefix, "q", Command::Quit),
        (KeyMode::Pane, "n", Command::NewPane),
        (KeyMode::Pane, "f", Command::Zoom),
        (KeyMode::Pane, "z", Command::Zoom),
        (KeyMode::Pane, "Left", Command::FocusLeft),
        (KeyMode::Pane, "p", Command::FocusNext),
        (KeyMode::Pane, "Esc", Command::LeaveMode),
        (KeyMode::Tab, "n", Command::NewTab),
        (KeyMode::Tab, "r", Command::RenameTab),
        (KeyMode::Tab, "j", Command::NextTab),
        (KeyMode::Tab, "k", Command::PreviousTab),
        (KeyMode::Tab, "]", Command::MovePaneRight),
        (KeyMode::Tab, "o", Command::MoveTabRight),
        (KeyMode::Tab, "9", Command::GoToTab(9)),
        (KeyMode::Tab, "Tab", Command::LastTab),
        (KeyMode::Scroll, "j", Command::ScrollDown),
        (KeyMode::Scroll, "Up", Command::ScrollUp),
        (KeyMode::Scroll, "d", Command::ScrollHalfDown),
        (KeyMode::Scroll, "Ctrl b", Command::ScrollPageUp),
        (KeyMode::Scroll, "l", Command::ScrollPageDown),
        (KeyMode::Scroll, "g", Command::ScrollTop),
        (KeyMode::Scroll, "G", Command::ScrollBottom),
        (KeyMode::Scroll, "Esc", Command::LeaveScroll),
        (KeyMode::Scroll, "Ctrl c", Command::LeaveScroll),
        (KeyMode::Session, "p", Command::ProjectPicker),
        (KeyMode::Session, "H", Command::HarnessManager),
        (KeyMode::Session, "q", Command::Quit),
        (KeyMode::Session, "Enter", Command::LeaveMode),
        (KeyMode::Lock, "Ctrl g", Command::Unlock),
    ];

    for (mode, chord, command) in cases {
        assert_eq!(
            keymap.lookup(mode, &parsed(chord)),
            Some(command),
            "{mode:?} {chord}"
        );
    }
    assert_eq!(keymap.lookup(KeyMode::Normal, &parsed("Ctrl q")), None, "quit is not one keystroke away");
    assert_eq!(keymap.bindings(KeyMode::Lock).len(), 1, "lock looks up only its unlock");
}

#[test]
fn normal_mode_takes_no_plain_typing_from_a_pane() {
    for (chord, _) in Keymap::defaults().bindings(KeyMode::Normal) {
        assert!(
            chord.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT),
            "{chord} would steal typing"
        );
    }
}

#[test]
fn every_command_has_a_name_that_reads_back() {
    let mut commands: Vec<Command> = Command::NAMED.iter().map(|(command, _)| *command).collect();
    commands.extend((1..=9).map(Command::GoToTab));

    for command in commands {
        assert_eq!(Command::from_name(&command.name()), Some(command), "{command:?}");
    }
    assert_eq!(Command::GoToTab(3).name(), "go_to_tab_3");
    assert_eq!(Command::from_name("go_to_tab_0"), None);
    assert_eq!(Command::from_name("explode"), None);
}

#[test]
fn every_mode_has_a_name_that_reads_back() {
    for mode in KeyMode::ALL {
        assert_eq!(KeyMode::from_name(mode.name()), Some(mode));
    }
    assert_eq!(KeyMode::from_name("panes"), None);
}

#[test]
fn stepping_commands_keep_their_mode_on_and_the_rest_end_it() {
    for command in [
        Command::FocusLeft,
        Command::FocusNext,
        Command::ExpandChild,
        Command::PreviousTab,
        Command::MovePaneRight,
        Command::MoveTabLeft,
        Command::ScrollDown,
        Command::ScrollPageUp,
        Command::ScrollTop,
        Command::ScrollBottom,
    ] {
        assert!(command.stays(), "{command:?}");
    }
    for command in [
        Command::NewPane,
        Command::ClosePane,
        Command::Zoom,
        Command::NewTab,
        Command::GoToTab(2),
        Command::LastTab,
        Command::LeaveScroll,
        Command::ProjectPicker,
        Command::Quit,
    ] {
        assert!(!command.stays(), "{command:?}");
    }
}

#[test]
fn a_command_is_the_action_the_app_runs() {
    assert_eq!(Command::NewPane.action(), Some(Action::NewPane));
    assert_eq!(Command::Zoom.action(), Some(Action::ToggleZoom));
    assert_eq!(
        Command::FocusLeft.action(),
        Some(Action::FocusDirection(Direction::Left))
    );
    assert_eq!(
        Command::FocusRightOrTab.action(),
        Some(Action::FocusOrTab(Direction::Right))
    );
    assert_eq!(Command::GoToTab(3).action(), Some(Action::SelectTab(2)));
    assert_eq!(Command::ScrollUp.action(), Some(Action::Scroll(-1)));
    assert_eq!(Command::ScrollHalfDown.action(), Some(Action::ScrollHalfPages(1)));
    assert_eq!(Command::ScrollPageUp.action(), Some(Action::ScrollPages(-1)));
    assert_eq!(Command::LeaveScroll.action(), Some(Action::ScrollToBottom));
    assert_eq!(Command::Fold.action(), Some(Action::ToggleFold));
    assert_eq!(Command::PaneMode.action(), None, "entering a mode is the router's");
    assert_eq!(Command::PaneMode.enters(), Some(KeyMode::Pane));
    assert_eq!(Command::Prefix.enters(), Some(KeyMode::Prefix));
}

#[test]
fn each_mode_spells_out_its_keys() {
    let keymap = Keymap::defaults();

    assert_eq!(
        keymap.mode_help(KeyMode::Pane),
        "PANE  n new  x close  f/z zoom  h/← left  j/↓ down  k/↑ up  l/→ right  p next  s child  c collapse  Esc/Enter done"
    );
    assert_eq!(
        keymap.mode_help(KeyMode::Tab),
        "TAB  n new  r rename  x close  ←/h prev  →/l next  [ pane left  ] pane right  i tab left  o tab right  1-9 go  Tab last  Esc/Enter done"
    );
    assert_eq!(
        keymap.mode_help(KeyMode::Scroll),
        "SCROLL  j/↓ down  k/↑ up  d half down  u half up  PageDown/Ctrl f page down  PageUp/Ctrl b page up  g top  G bottom  Esc/Enter done"
    );
    assert_eq!(
        keymap.mode_help(KeyMode::Session),
        "SESSION  p projects  o open  m machine  H harnesses  a approvals  f fold  q quit  Esc/Enter done"
    );
    assert_eq!(
        keymap.normal_help(),
        "Ctrl p pane  Ctrl t tabs  Ctrl s scroll  Ctrl o session  Ctrl g lock  Ctrl a prefix"
    );
    assert_eq!(keymap.lock_help(), "LOCKED  Ctrl g unlock");
}

#[test]
fn a_chord_bound_twice_answers_with_its_first_binding() {
    let mut keymap = Keymap::defaults();
    keymap.bind(KeyMode::Pane, parsed("x"), Command::Zoom);

    assert_eq!(keymap.lookup(KeyMode::Pane, &parsed("x")), Some(Command::Zoom));
    assert_eq!(
        keymap.bindings(KeyMode::Pane).iter().filter(|(chord, _)| *chord == parsed("x")).count(),
        1,
        "binding a chord again replaces it rather than adding a second"
    );
}
```

Run: `cargo test -p dispatch-tui keymap`
Expected: FAIL to compile, because `KeyMode`, `Command`, `Keymap` and `Action::ScrollHalfPages` are not found.

- [ ] **Step 2: Add the actions**

In `crates/dispatch-tui/src/input.rs`, add these variants to `Action` after `FocusOrTab`:

```rust
    /// Focus the next pane on the tab on screen, wrapping.
    FocusNext,
    /// Scroll the focused pane by half its height this many times; negative
    /// towards older output.
    ScrollHalfPages(isize),
    /// Scroll the focused pane by its whole height this many times; negative
    /// towards older output.
    ScrollPages(isize),
    /// Show the oldest output the focused pane still holds.
    ScrollToTop,
    /// Return the focused pane to its newest output.
    ScrollToBottom,
```

In `dispatch/src/app.rs`, `App::handle`'s `match action` must stay exhaustive. Add one arm that Task 4 replaces with the real handlers:

```rust
            // Wired to the app's scrolling and focus in the next change.
            Action::FocusNext
            | Action::ScrollHalfPages(_)
            | Action::ScrollPages(_)
            | Action::ScrollToTop
            | Action::ScrollToBottom => {}
```

- [ ] **Step 3: Implement the keymap**

Append to `crates/dispatch-tui/src/keymap.rs`, above `#[cfg(test)] mod tests;`. Also add `use crate::input::{Action, Direction};` next to the crossterm import.

```rust
/// Which keys the router is reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KeyMode {
    /// Keys go to the focused pane, but for the few bound here.
    #[default]
    Normal,
    /// One command key after the prefix, then back to normal.
    Prefix,
    /// Pane commands, until one ends the mode.
    Pane,
    /// Tab commands, until one ends the mode.
    Tab,
    /// Reading the focused pane's scrollback, until one ends the mode.
    Scroll,
    /// Session commands, until one ends the mode.
    Session,
    /// Every key goes to the pane but the one that unlocks.
    Lock,
}

impl KeyMode {
    /// Every mode, in the order the table keeps them.
    pub const ALL: [KeyMode; 7] = [
        KeyMode::Normal,
        KeyMode::Prefix,
        KeyMode::Pane,
        KeyMode::Tab,
        KeyMode::Scroll,
        KeyMode::Session,
        KeyMode::Lock,
    ];

    /// The name `[keys]` uses for it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            KeyMode::Normal => "normal",
            KeyMode::Prefix => "prefix",
            KeyMode::Pane => "pane",
            KeyMode::Tab => "tab",
            KeyMode::Scroll => "scroll",
            KeyMode::Session => "session",
            KeyMode::Lock => "lock",
        }
    }

    /// The mode `[keys]` names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == name)
    }

    /// Whether it is one of the modes a key enters and stays in until a
    /// command, `Esc` or a click ends it.
    #[must_use]
    pub fn is_modal(self) -> bool {
        matches!(
            self,
            KeyMode::Pane | KeyMode::Tab | KeyMode::Scroll | KeyMode::Session
        )
    }

    /// What the status row calls it.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            KeyMode::Pane => "PANE",
            KeyMode::Tab => "TAB",
            KeyMode::Scroll => "SCROLL",
            KeyMode::Session => "SESSION",
            other => other.name(),
        }
    }

    /// The command that enters it from normal mode.
    #[must_use]
    pub fn entered_by(self) -> Option<Command> {
        match self {
            KeyMode::Normal => None,
            KeyMode::Prefix => Some(Command::Prefix),
            KeyMode::Pane => Some(Command::PaneMode),
            KeyMode::Tab => Some(Command::TabMode),
            KeyMode::Scroll => Some(Command::ScrollMode),
            KeyMode::Session => Some(Command::SessionMode),
            KeyMode::Lock => Some(Command::Lock),
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Everything a key can be bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    PaneMode,
    TabMode,
    ScrollMode,
    SessionMode,
    Lock,
    Unlock,
    Prefix,
    LeaveMode,
    None,
    NewPane,
    ClosePane,
    Zoom,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    FocusLeftOrTab,
    FocusRightOrTab,
    FocusNext,
    ExpandChild,
    CollapseChild,
    NewTab,
    RenameTab,
    CloseTab,
    PreviousTab,
    NextTab,
    LastTab,
    MovePaneLeft,
    MovePaneRight,
    MoveTabLeft,
    MoveTabRight,
    /// Go to the tab at this position, counting from one.
    GoToTab(u8),
    ScrollDown,
    ScrollUp,
    ScrollHalfDown,
    ScrollHalfUp,
    ScrollPageDown,
    ScrollPageUp,
    ScrollTop,
    ScrollBottom,
    LeaveScroll,
    ProjectPicker,
    OpenProject,
    AddMachine,
    HarnessManager,
    Approvals,
    Fold,
    Quit,
}

impl Command {
    /// Every command but `GoToTab`, with the name `[keys]` uses for it.
    pub const NAMED: &[(Command, &str)] = &[
        (Command::PaneMode, "pane_mode"),
        (Command::TabMode, "tab_mode"),
        (Command::ScrollMode, "scroll_mode"),
        (Command::SessionMode, "session_mode"),
        (Command::Lock, "lock"),
        (Command::Unlock, "unlock"),
        (Command::Prefix, "prefix"),
        (Command::LeaveMode, "leave_mode"),
        (Command::None, "none"),
        (Command::NewPane, "new_pane"),
        (Command::ClosePane, "close_pane"),
        (Command::Zoom, "zoom"),
        (Command::FocusLeft, "focus_left"),
        (Command::FocusRight, "focus_right"),
        (Command::FocusUp, "focus_up"),
        (Command::FocusDown, "focus_down"),
        (Command::FocusLeftOrTab, "focus_left_or_tab"),
        (Command::FocusRightOrTab, "focus_right_or_tab"),
        (Command::FocusNext, "focus_next"),
        (Command::ExpandChild, "expand_child"),
        (Command::CollapseChild, "collapse_child"),
        (Command::NewTab, "new_tab"),
        (Command::RenameTab, "rename_tab"),
        (Command::CloseTab, "close_tab"),
        (Command::PreviousTab, "previous_tab"),
        (Command::NextTab, "next_tab"),
        (Command::LastTab, "last_tab"),
        (Command::MovePaneLeft, "move_pane_left"),
        (Command::MovePaneRight, "move_pane_right"),
        (Command::MoveTabLeft, "move_tab_left"),
        (Command::MoveTabRight, "move_tab_right"),
        (Command::ScrollDown, "scroll_down"),
        (Command::ScrollUp, "scroll_up"),
        (Command::ScrollHalfDown, "scroll_half_down"),
        (Command::ScrollHalfUp, "scroll_half_up"),
        (Command::ScrollPageDown, "scroll_page_down"),
        (Command::ScrollPageUp, "scroll_page_up"),
        (Command::ScrollTop, "scroll_top"),
        (Command::ScrollBottom, "scroll_bottom"),
        (Command::LeaveScroll, "leave_scroll"),
        (Command::ProjectPicker, "project_picker"),
        (Command::OpenProject, "open_project"),
        (Command::AddMachine, "add_machine"),
        (Command::HarnessManager, "harness_manager"),
        (Command::Approvals, "approvals"),
        (Command::Fold, "fold"),
        (Command::Quit, "quit"),
    ];

    /// The name `[keys]` uses for it.
    #[must_use]
    pub fn name(self) -> String {
        if let Command::GoToTab(n) = self {
            return format!("go_to_tab_{n}");
        }
        Self::NAMED
            .iter()
            .find(|(command, _)| *command == self)
            .map(|(_, name)| (*name).to_string())
            .unwrap_or_default()
    }

    /// The command `[keys]` names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        if let Some(n) = name.strip_prefix("go_to_tab_") {
            return n
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=9).contains(n))
                .map(Command::GoToTab);
        }
        Self::NAMED
            .iter()
            .find(|(_, known)| *known == name)
            .map(|(command, _)| *command)
    }

    /// What the status row calls it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Command::PaneMode => "pane",
            Command::TabMode => "tabs",
            Command::ScrollMode => "scroll",
            Command::SessionMode => "session",
            Command::Lock => "lock",
            Command::Unlock => "unlock",
            Command::Prefix => "prefix",
            Command::LeaveMode | Command::LeaveScroll => "done",
            Command::None => "",
            Command::NewPane | Command::NewTab => "new",
            Command::ClosePane | Command::CloseTab => "close",
            Command::Zoom => "zoom",
            Command::FocusLeft | Command::FocusLeftOrTab => "left",
            Command::FocusRight | Command::FocusRightOrTab => "right",
            Command::FocusUp | Command::ScrollUp => "up",
            Command::FocusDown | Command::ScrollDown => "down",
            Command::FocusNext | Command::NextTab => "next",
            Command::ExpandChild => "child",
            Command::CollapseChild => "collapse",
            Command::RenameTab => "rename",
            Command::PreviousTab => "prev",
            Command::LastTab => "last",
            Command::MovePaneLeft => "pane left",
            Command::MovePaneRight => "pane right",
            Command::MoveTabLeft => "tab left",
            Command::MoveTabRight => "tab right",
            Command::GoToTab(_) => "go",
            Command::ScrollHalfDown => "half down",
            Command::ScrollHalfUp => "half up",
            Command::ScrollPageDown => "page down",
            Command::ScrollPageUp => "page up",
            Command::ScrollTop => "top",
            Command::ScrollBottom => "bottom",
            Command::ProjectPicker => "projects",
            Command::OpenProject => "open",
            Command::AddMachine => "machine",
            Command::HarnessManager => "harnesses",
            Command::Approvals => "approvals",
            Command::Fold => "fold",
            Command::Quit => "quit",
        }
    }

    /// Whether a mode stays on after it: stepping does, so focus, a tab or
    /// the scrollback can be walked several places along; anything that
    /// opens, closes or jumps ends the mode.
    #[must_use]
    pub fn stays(self) -> bool {
        matches!(
            self,
            Command::FocusLeft
                | Command::FocusRight
                | Command::FocusUp
                | Command::FocusDown
                | Command::FocusLeftOrTab
                | Command::FocusRightOrTab
                | Command::FocusNext
                | Command::ExpandChild
                | Command::CollapseChild
                | Command::PreviousTab
                | Command::NextTab
                | Command::MovePaneLeft
                | Command::MovePaneRight
                | Command::MoveTabLeft
                | Command::MoveTabRight
                | Command::ScrollDown
                | Command::ScrollUp
                | Command::ScrollHalfDown
                | Command::ScrollHalfUp
                | Command::ScrollPageDown
                | Command::ScrollPageUp
                | Command::ScrollTop
                | Command::ScrollBottom
        )
    }

    /// The mode it enters, for the commands that enter one.
    #[must_use]
    pub fn enters(self) -> Option<KeyMode> {
        Some(match self {
            Command::PaneMode => KeyMode::Pane,
            Command::TabMode => KeyMode::Tab,
            Command::ScrollMode => KeyMode::Scroll,
            Command::SessionMode => KeyMode::Session,
            Command::Lock => KeyMode::Lock,
            Command::Prefix => KeyMode::Prefix,
            _ => return None,
        })
    }

    /// What the app does for it. `None` for what the router does itself:
    /// entering, leaving and unlocking a mode, and nothing at all.
    #[must_use]
    pub fn action(self) -> Option<Action> {
        Some(match self {
            Command::PaneMode
            | Command::TabMode
            | Command::ScrollMode
            | Command::SessionMode
            | Command::Lock
            | Command::Unlock
            | Command::Prefix
            | Command::LeaveMode
            | Command::None => return None,
            Command::NewPane => Action::NewPane,
            Command::ClosePane => Action::ClosePane,
            Command::Zoom => Action::ToggleZoom,
            Command::FocusLeft => Action::FocusDirection(Direction::Left),
            Command::FocusRight => Action::FocusDirection(Direction::Right),
            Command::FocusUp => Action::FocusDirection(Direction::Up),
            Command::FocusDown => Action::FocusDirection(Direction::Down),
            Command::FocusLeftOrTab => Action::FocusOrTab(Direction::Left),
            Command::FocusRightOrTab => Action::FocusOrTab(Direction::Right),
            Command::FocusNext => Action::FocusNext,
            Command::ExpandChild => Action::ExpandChild,
            Command::CollapseChild => Action::CollapseChild,
            Command::NewTab => Action::NewTab,
            Command::RenameTab => Action::RenameTab,
            Command::CloseTab => Action::CloseTab,
            Command::PreviousTab => Action::PreviousTab,
            Command::NextTab => Action::NextTab,
            Command::LastTab => Action::LastTab,
            Command::MovePaneLeft => Action::MovePaneLeft,
            Command::MovePaneRight => Action::MovePaneRight,
            Command::MoveTabLeft => Action::MoveTabLeft,
            Command::MoveTabRight => Action::MoveTabRight,
            Command::GoToTab(n) => Action::SelectTab(usize::from(n.saturating_sub(1))),
            Command::ScrollDown => Action::Scroll(1),
            Command::ScrollUp => Action::Scroll(-1),
            Command::ScrollHalfDown => Action::ScrollHalfPages(1),
            Command::ScrollHalfUp => Action::ScrollHalfPages(-1),
            Command::ScrollPageDown => Action::ScrollPages(1),
            Command::ScrollPageUp => Action::ScrollPages(-1),
            Command::ScrollTop => Action::ScrollToTop,
            Command::ScrollBottom | Command::LeaveScroll => Action::ScrollToBottom,
            Command::ProjectPicker => Action::ProjectPicker,
            Command::OpenProject => Action::OpenProject,
            Command::AddMachine => Action::AddMachine,
            Command::HarnessManager => Action::HarnessManager,
            Command::Approvals => Action::Approvals,
            Command::Fold => Action::ToggleFold,
            Command::Quit => Action::Quit,
        })
    }
}

/// Each mode's keys, in the order the status row lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    modes: [Vec<(Chord, Command)>; 7],
}

impl Default for Keymap {
    fn default() -> Self {
        Self::defaults()
    }
}

impl Keymap {
    /// The keys Dispatch ships with: zellij's modes, and `Ctrl a` as a
    /// tmux-style prefix holding every command it always has.
    #[must_use]
    pub fn defaults() -> Self {
        use KeyCode::{Down, Enter, Esc, Left, PageDown, PageUp, Right, Tab, Up};

        let digits = |then: fn(u8) -> Command| {
            (1..=9u8).map(move |n| (Chord::char(char::from(b'0' + n)), then(n)))
        };

        let normal = vec![
            (Chord::ctrl('p'), Command::PaneMode),
            (Chord::ctrl('t'), Command::TabMode),
            (Chord::ctrl('s'), Command::ScrollMode),
            (Chord::ctrl('o'), Command::SessionMode),
            (Chord::ctrl('g'), Command::Lock),
            (Chord::ctrl('a'), Command::Prefix),
            (Chord::alt('n'), Command::NewPane),
            (Chord::alt('i'), Command::MoveTabLeft),
            (Chord::alt('o'), Command::MoveTabRight),
            (Chord::alt_key(Left), Command::FocusLeftOrTab),
            (Chord::alt('h'), Command::FocusLeftOrTab),
            (Chord::alt_key(Right), Command::FocusRightOrTab),
            (Chord::alt('l'), Command::FocusRightOrTab),
            (Chord::alt_key(Up), Command::FocusUp),
            (Chord::alt('k'), Command::FocusUp),
            (Chord::alt_key(Down), Command::FocusDown),
            (Chord::alt('j'), Command::FocusDown),
        ];

        let mut prefix = vec![
            (Chord::char('n'), Command::NewPane),
            (Chord::char('x'), Command::ClosePane),
            (Chord::char('z'), Command::Zoom),
            (Chord::char('h'), Command::FocusLeft),
            (Chord::char('j'), Command::FocusDown),
            (Chord::char('k'), Command::FocusUp),
            (Chord::char('l'), Command::FocusRight),
            (Chord::char('p'), Command::ProjectPicker),
            (Chord::char('H'), Command::HarnessManager),
            (Chord::char('a'), Command::Approvals),
            (Chord::char('s'), Command::ExpandChild),
            (Chord::char('c'), Command::CollapseChild),
            (Chord::char('f'), Command::Fold),
            (Chord::char('o'), Command::OpenProject),
            (Chord::char('m'), Command::AddMachine),
            (Chord::char('['), Command::ScrollMode),
        ];
        prefix.extend(digits(Command::GoToTab));
        prefix.push((Chord::key(Tab), Command::NextTab));
        prefix.push((Chord::char('q'), Command::Quit));

        let pane = vec![
            (Chord::char('n'), Command::NewPane),
            (Chord::char('x'), Command::ClosePane),
            (Chord::char('f'), Command::Zoom),
            (Chord::char('z'), Command::Zoom),
            (Chord::char('h'), Command::FocusLeft),
            (Chord::key(Left), Command::FocusLeft),
            (Chord::char('j'), Command::FocusDown),
            (Chord::key(Down), Command::FocusDown),
            (Chord::char('k'), Command::FocusUp),
            (Chord::key(Up), Command::FocusUp),
            (Chord::char('l'), Command::FocusRight),
            (Chord::key(Right), Command::FocusRight),
            (Chord::char('p'), Command::FocusNext),
            (Chord::char('s'), Command::ExpandChild),
            (Chord::char('c'), Command::CollapseChild),
            (Chord::key(Esc), Command::LeaveMode),
            (Chord::key(Enter), Command::LeaveMode),
        ];

        let mut tab = vec![
            (Chord::char('n'), Command::NewTab),
            (Chord::char('r'), Command::RenameTab),
            (Chord::char('x'), Command::CloseTab),
            (Chord::key(Left), Command::PreviousTab),
            (Chord::char('h'), Command::PreviousTab),
            (Chord::char('k'), Command::PreviousTab),
            (Chord::key(Right), Command::NextTab),
            (Chord::char('l'), Command::NextTab),
            (Chord::char('j'), Command::NextTab),
            (Chord::char('['), Command::MovePaneLeft),
            (Chord::char(']'), Command::MovePaneRight),
            (Chord::char('i'), Command::MoveTabLeft),
            (Chord::char('o'), Command::MoveTabRight),
        ];
        tab.extend(digits(Command::GoToTab));
        tab.extend([
            (Chord::key(Tab), Command::LastTab),
            (Chord::key(Esc), Command::LeaveMode),
            (Chord::key(Enter), Command::LeaveMode),
        ]);

        let scroll = vec![
            (Chord::char('j'), Command::ScrollDown),
            (Chord::key(Down), Command::ScrollDown),
            (Chord::char('k'), Command::ScrollUp),
            (Chord::key(Up), Command::ScrollUp),
            (Chord::char('d'), Command::ScrollHalfDown),
            (Chord::char('u'), Command::ScrollHalfUp),
            (Chord::key(PageDown), Command::ScrollPageDown),
            (Chord::ctrl('f'), Command::ScrollPageDown),
            (Chord::char('l'), Command::ScrollPageDown),
            (Chord::key(Right), Command::ScrollPageDown),
            (Chord::key(PageUp), Command::ScrollPageUp),
            (Chord::ctrl('b'), Command::ScrollPageUp),
            (Chord::char('h'), Command::ScrollPageUp),
            (Chord::key(Left), Command::ScrollPageUp),
            (Chord::char('g'), Command::ScrollTop),
            (Chord::char('G'), Command::ScrollBottom),
            (Chord::key(Esc), Command::LeaveScroll),
            (Chord::key(Enter), Command::LeaveScroll),
            (Chord::ctrl('c'), Command::LeaveScroll),
        ];

        let session = vec![
            (Chord::char('p'), Command::ProjectPicker),
            (Chord::char('o'), Command::OpenProject),
            (Chord::char('m'), Command::AddMachine),
            (Chord::char('H'), Command::HarnessManager),
            (Chord::char('a'), Command::Approvals),
            (Chord::char('f'), Command::Fold),
            (Chord::char('q'), Command::Quit),
            (Chord::key(Esc), Command::LeaveMode),
            (Chord::key(Enter), Command::LeaveMode),
        ];

        let lock = vec![(Chord::ctrl('g'), Command::Unlock)];

        Self {
            modes: [normal, prefix, pane, tab, scroll, session, lock],
        }
    }

    /// A mode's keys, in order.
    #[must_use]
    pub fn bindings(&self, mode: KeyMode) -> &[(Chord, Command)] {
        &self.modes[mode.index()]
    }

    /// What `chord` does in `mode`.
    #[must_use]
    pub fn lookup(&self, mode: KeyMode, chord: &Chord) -> Option<Command> {
        self.bindings(mode)
            .iter()
            .find(|(bound, _)| bound == chord)
            .map(|(_, command)| *command)
    }

    /// Every chord bound to `command` in `mode`, in order.
    #[must_use]
    pub fn chords_for(&self, mode: KeyMode, command: Command) -> Vec<Chord> {
        self.bindings(mode)
            .iter()
            .filter(|(_, bound)| *bound == command)
            .map(|(chord, _)| *chord)
            .collect()
    }

    /// Binds `chord` in `mode`: in place of what it was bound to, or at the
    /// end.
    pub(crate) fn bind(&mut self, mode: KeyMode, chord: Chord, command: Command) {
        let keys = &mut self.modes[mode.index()];
        match keys.iter_mut().find(|(bound, _)| *bound == chord) {
            Some(existing) => existing.1 = command,
            None => keys.push((chord, command)),
        }
    }

    /// Unbinds `chord` in `mode`.
    pub(crate) fn unbind(&mut self, mode: KeyMode, chord: &Chord) {
        self.modes[mode.index()].retain(|(bound, _)| bound != chord);
    }

    /// Drops every key in `mode`.
    pub(crate) fn clear(&mut self, mode: KeyMode) {
        self.modes[mode.index()].clear();
    }

    /// The status row for a mode: its title, then each command with its
    /// first one or two keys, in the order the keys are bound. The go-to-tab
    /// keys read as one range, `1-9 go`.
    #[must_use]
    pub fn mode_help(&self, mode: KeyMode) -> String {
        let mut groups: Vec<(Command, Vec<Chord>)> = Vec::new();
        for (chord, command) in self.bindings(mode) {
            let key = match command {
                Command::GoToTab(_) => Command::GoToTab(0),
                other => *other,
            };
            match groups.iter_mut().find(|(grouped, _)| *grouped == key) {
                Some((_, chords)) => chords.push(*chord),
                None => groups.push((key, vec![*chord])),
            }
        }

        let mut parts = vec![mode.title().to_string()];
        for (command, chords) in groups {
            let keys = if matches!(command, Command::GoToTab(_)) && chords.len() > 2 {
                format!(
                    "{}-{}",
                    chords[0].short(),
                    chords[chords.len() - 1].short()
                )
            } else {
                chords
                    .iter()
                    .take(2)
                    .map(Chord::short)
                    .collect::<Vec<_>>()
                    .join("/")
            };
            parts.push(format!("{keys} {}", command.label()));
        }
        parts.join("  ")
    }

    /// The keys normal mode offers, for the status row: the first key of
    /// each mode, and the prefix.
    #[must_use]
    pub fn normal_help(&self) -> String {
        [
            Command::PaneMode,
            Command::TabMode,
            Command::ScrollMode,
            Command::SessionMode,
            Command::Lock,
            Command::Prefix,
        ]
        .into_iter()
        .filter_map(|command| {
            self.chords_for(KeyMode::Normal, command)
                .first()
                .map(|chord| format!("{} {}", chord.short(), command.label()))
        })
        .collect::<Vec<_>>()
        .join("  ")
    }

    /// The status row while locked: the one key that gets out.
    #[must_use]
    pub fn lock_help(&self) -> String {
        match self.chords_for(KeyMode::Lock, Command::Unlock).first() {
            Some(chord) => format!("LOCKED  {} unlock", chord.short()),
            None => "LOCKED".to_string(),
        }
    }
}
```

Add `#[allow(dead_code)] // used by [keys] overrides in the next change` on `bind`, `unbind` and `clear`. Only this task's tests call them until Task 3 uses them and removes the attributes.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p dispatch-tui keymap && cargo build -p dispatch`
Expected: PASS, 16 tests in `keymap`, and `dispatch` builds.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/dispatch-tui/src/keymap.rs crates/dispatch-tui/src/keymap/tests.rs crates/dispatch-tui/src/input.rs dispatch/src/app.rs
git commit -m "feat(tui): a keymap per mode, with the keys dispatch ships with"
```

---
### Task 3: `[keys]` and overrides

**Files:**
- Modify: `crates/dispatch-config/src/config.rs`: `KeyValue`, `KeysConfig`, `Config.keys`, and a `"keys"` arm in `unknown_keys`.
- Modify: `crates/dispatch-config/src/config/tests.rs`
- Modify: `crates/dispatch-config/src/lib.rs`: re-export `KeysConfig`, `KeyValue`.
- Modify: `crates/dispatch-tui/src/keymap.rs`: `Keymap::with_overrides`, `keep_a_way_out`, `allowed`, and removing Task 2's three `#[allow(dead_code)]`.
- Modify: `crates/dispatch-tui/src/keymap/tests.rs`

**Interfaces:**
- Consumes: Task 2's `Keymap::{defaults, bind, unbind, clear, lookup, chords_for}`, `Command::{from_name, enters}` and `KeyMode::{from_name, is_modal}`.
- Produces (used by Tasks 4–6):
  - `dispatch_config::KeysConfig(pub BTreeMap<String, BTreeMap<String, KeyValue>>)`, with `Default`.
  - `dispatch_config::KeyValue::{Command(String), Flag(bool), Other(String)}`.
  - `Config.keys: KeysConfig`.
  - `Keymap::with_overrides(&KeysConfig) -> (Keymap, Vec<String>)`, where the warnings use the Global Constraints texts.

- [ ] **Step 1: Write the failing config tests**

Append to `crates/dispatch-config/src/config/tests.rs`. The `load(label, text)` helper already exists.

```rust
#[test]
fn a_keys_section_is_kept_as_written() {
    let config = load(
        "keys",
        "[keys.pane]\n\"w\" = \"close_pane\"\nclear = true\n\n[keys.normal]\n\"Ctrl q\" = \"quit\"\n",
    );

    let pane = &config.keys.0["pane"];
    assert_eq!(pane["w"], KeyValue::Command("close_pane".into()));
    assert_eq!(pane["clear"], KeyValue::Flag(true));
    assert_eq!(
        config.keys.0["normal"]["Ctrl q"],
        KeyValue::Command("quit".into())
    );
}

#[test]
fn a_key_given_the_wrong_kind_of_value_is_kept_to_be_reported() {
    // A number where a command's name goes costs that one binding, not the
    // whole file: the interface says what it cannot use.
    let config = load("keys-type", "[keys.pane]\nx = 3\n");

    assert_eq!(config.keys.0["pane"]["x"], KeyValue::Other("integer".into()));
}

#[test]
fn without_a_keys_section_nothing_is_rebound() {
    assert!(load("no-keys", "").keys.0.is_empty());
}

#[test]
fn the_keys_section_is_judged_by_the_interface_not_reported_unknown() {
    let dir = TempDir::new("keys-known");
    let path = dir.config("[keys.whatever]\nx = \"none\"\n");

    let loaded = Config::load_reporting(&path).expect("the file loads");

    assert!(loaded.unknown.is_empty(), "{:?}", loaded.unknown);
}
```

Run: `cargo test -p dispatch-config`
Expected: FAIL to compile, because there is no field `keys` and no `KeyValue`.

- [ ] **Step 2: Implement the config**

In `crates/dispatch-config/src/config.rs`, add `Deserializer` to the serde import (`use serde::{Deserialize, Deserializer, Serialize};`). `BTreeMap` is already imported. Add before `Config`:

```rust
/// One value in a `[keys.<mode>]` table, as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum KeyValue {
    /// A command's name, or `none`.
    Command(String),
    /// A flag, as `clear = true` is.
    Flag(bool),
    /// Anything else, named by its TOML type, so it is reported rather than
    /// failing the file.
    Other(String),
}

impl<'de> Deserialize<'de> for KeyValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match toml::Value::deserialize(deserializer)? {
            toml::Value::String(name) => KeyValue::Command(name),
            toml::Value::Boolean(flag) => KeyValue::Flag(flag),
            other => KeyValue::Other(other.type_str().to_string()),
        })
    }
}

/// What `[keys]` says: per mode, each key's text and what it is bound to,
/// as written.
///
/// Kept as text: which modes, keys and commands exist is the interface's
/// business, and it reports what it cannot use, so a mistake costs one
/// binding rather than the whole file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeysConfig(pub BTreeMap<String, BTreeMap<String, KeyValue>>);
```

Add a field to `Config`, after `shell`:

```rust
    /// Keys the interface binds, over its defaults. The daemon ignores it.
    pub keys: KeysConfig,
```

In `unknown_keys`, add an arm before the final `_ =>`:

```rust
            // The interface judges `[keys]` itself: it alone knows which
            // modes, keys and commands exist, and it says what it skips.
            ("keys", _) => {}
```

In `crates/dispatch-config/src/lib.rs`, add `KeyValue` and `KeysConfig` to the `pub use config::{…}` line.

Run: `cargo test -p dispatch-config`
Expected: PASS.

- [ ] **Step 3: Write the failing keymap tests**

Append to `crates/dispatch-tui/src/keymap/tests.rs`:

```rust
use std::collections::BTreeMap;

use dispatch_config::{KeyValue, KeysConfig};

/// A `[keys]` holding `entries`, each (mode, key, value).
fn keys(entries: &[(&str, &str, KeyValue)]) -> KeysConfig {
    let mut keys = KeysConfig::default();
    for (mode, chord, value) in entries {
        keys.0
            .entry((*mode).to_string())
            .or_insert_with(BTreeMap::new)
            .insert((*chord).to_string(), value.clone());
    }
    keys
}

fn named(name: &str) -> KeyValue {
    KeyValue::Command(name.to_string())
}

#[test]
fn no_keys_is_the_defaults() {
    assert_eq!(
        Keymap::with_overrides(&KeysConfig::default()),
        (Keymap::defaults(), Vec::new())
    );
}

#[test]
fn a_binding_replaces_adds_and_unbinds() {
    let (keymap, warnings) = Keymap::with_overrides(&keys(&[
        ("pane", "x", named("zoom")),
        ("pane", "w", named("close_pane")),
        ("pane", "n", named("none")),
        ("normal", "Ctrl q", named("quit")),
    ]));

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(keymap.lookup(KeyMode::Pane, &parsed("x")), Some(Command::Zoom));
    assert_eq!(keymap.lookup(KeyMode::Pane, &parsed("w")), Some(Command::ClosePane));
    assert_eq!(keymap.lookup(KeyMode::Pane, &parsed("n")), None);
    assert_eq!(keymap.lookup(KeyMode::Normal, &parsed("Ctrl q")), Some(Command::Quit));
    assert_eq!(
        keymap.bindings(KeyMode::Pane).last(),
        Some(&(parsed("w"), Command::ClosePane)),
        "a new key goes after the defaults"
    );
}

#[test]
fn the_prefix_can_move_to_another_key() {
    let (keymap, warnings) = Keymap::with_overrides(&keys(&[
        ("normal", "Ctrl a", named("none")),
        ("normal", "Ctrl b", named("prefix")),
    ]));

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(keymap.lookup(KeyMode::Normal, &parsed("Ctrl a")), None);
    assert_eq!(keymap.lookup(KeyMode::Normal, &parsed("Ctrl b")), Some(Command::Prefix));
}

#[test]
fn clearing_a_mode_drops_its_defaults() {
    let (keymap, warnings) = Keymap::with_overrides(&keys(&[
        ("session", "clear", KeyValue::Flag(true)),
        ("session", "q", named("quit")),
    ]));

    assert_eq!(
        keymap.bindings(KeyMode::Session),
        &[
            (parsed("q"), Command::Quit),
            (parsed("Esc"), Command::LeaveMode)
        ]
    );
    assert_eq!(warnings, vec!["keys.session: Esc always leaves the mode".to_string()]);
}

#[test]
fn mistakes_are_reported_by_name_and_the_rest_still_applies() {
    let (keymap, warnings) = Keymap::with_overrides(&keys(&[
        ("panes", "x", named("zoom")),
        ("pane", "Ctrl ?x", named("zoom")),
        ("pane", "y", named("explode")),
        ("pane", "u", KeyValue::Other("integer".into())),
        ("pane", "v", KeyValue::Flag(true)),
        ("tab", "clear", named("yes")),
        ("pane", "w", named("close_pane")),
    ]));

    assert_eq!(
        keymap.lookup(KeyMode::Pane, &parsed("w")),
        Some(Command::ClosePane),
        "the rest still applies"
    );
    assert_eq!(
        warnings,
        vec![
            "keys.pane.\"Ctrl ?x\": not a key".to_string(),
            "keys.pane.\"u\": a command's name, in quotes".to_string(),
            "keys.pane.\"v\": a command's name, in quotes".to_string(),
            "keys.pane.\"y\": no command \"explode\"".to_string(),
            "keys.panes: not a mode".to_string(),
            "keys.tab.clear: true or false".to_string(),
        ]
    );
}

#[test]
fn lock_always_unlocks() {
    let (keymap, warnings) =
        Keymap::with_overrides(&keys(&[("lock", "Ctrl g", named("none"))]));

    assert_eq!(keymap.chords_for(KeyMode::Lock, Command::Unlock), vec![parsed("Ctrl g")]);
    assert_eq!(
        warnings,
        vec!["keys.lock: nothing unlocks, so Ctrl g still does".to_string()]
    );
}

#[test]
fn lock_can_unlock_on_another_key() {
    let (keymap, warnings) = Keymap::with_overrides(&keys(&[
        ("lock", "Ctrl g", named("none")),
        ("lock", "Ctrl l", named("unlock")),
    ]));

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(keymap.chords_for(KeyMode::Lock, Command::Unlock), vec![parsed("Ctrl l")]);
}

#[test]
fn esc_always_leaves_a_mode() {
    let (keymap, warnings) = Keymap::with_overrides(&keys(&[
        ("pane", "Esc", named("none")),
        ("scroll", "Esc", named("scroll_down")),
    ]));

    assert_eq!(keymap.lookup(KeyMode::Pane, &parsed("Esc")), Some(Command::LeaveMode));
    assert_eq!(keymap.lookup(KeyMode::Scroll, &parsed("Esc")), Some(Command::LeaveScroll));
    assert_eq!(
        warnings,
        vec![
            "keys.pane: Esc always leaves the mode".to_string(),
            "keys.scroll: Esc always leaves the mode".to_string(),
        ]
    );
}

#[test]
fn a_mode_is_entered_only_from_normal_mode_or_the_prefix() {
    let (keymap, warnings) = Keymap::with_overrides(&keys(&[
        ("pane", "t", named("tab_mode")),
        ("prefix", "t", named("tab_mode")),
    ]));

    assert_eq!(keymap.lookup(KeyMode::Pane, &parsed("t")), None);
    assert_eq!(keymap.lookup(KeyMode::Prefix, &parsed("t")), Some(Command::TabMode));
    assert_eq!(
        warnings,
        vec!["keys.pane.\"t\": a mode is entered from normal mode or the prefix".to_string()]
    );
}

#[test]
fn only_lock_mode_unlocks_and_lock_mode_only_unlocks() {
    let (keymap, warnings) = Keymap::with_overrides(&keys(&[
        ("lock", "x", named("new_pane")),
        ("normal", "Ctrl u", named("unlock")),
    ]));

    assert_eq!(keymap.lookup(KeyMode::Lock, &parsed("x")), None);
    assert_eq!(keymap.lookup(KeyMode::Normal, &parsed("Ctrl u")), None);
    assert_eq!(
        warnings,
        vec![
            "keys.lock.\"x\": lock mode only unlocks".to_string(),
            "keys.normal.\"Ctrl u\": only lock mode unlocks".to_string(),
        ]
    );
}
```

Run: `cargo test -p dispatch-tui keymap`
Expected: FAIL to compile, because there is no function `with_overrides`.

- [ ] **Step 4: Implement the overrides**

In `crates/dispatch-tui/src/keymap.rs`, add `use dispatch_config::{KeyValue, KeysConfig};`. Remove the three `#[allow(dead_code)]` from Task 2. Add to `impl Keymap`:

```rust
    /// The defaults with the user's `[keys]` laid over them, and what could
    /// not be used, one line each, for the log.
    ///
    /// Nothing in `[keys]` stops Dispatch: a mistake costs its own binding,
    /// and whatever would leave the user stuck in a mode is put back.
    #[must_use]
    pub fn with_overrides(keys: &KeysConfig) -> (Self, Vec<String>) {
        let mut keymap = Self::defaults();
        let mut warnings = Vec::new();

        for (mode_name, table) in &keys.0 {
            let Some(mode) = KeyMode::from_name(mode_name) else {
                warnings.push(format!("keys.{mode_name}: not a mode"));
                continue;
            };

            // First, so the mode's own keys in the same table are added to
            // an empty mode rather than cleared with the defaults.
            match table.get("clear") {
                Some(KeyValue::Flag(true)) => keymap.clear(mode),
                Some(KeyValue::Flag(false)) | None => {}
                Some(_) => warnings.push(format!("keys.{mode_name}.clear: true or false")),
            }

            for (text, value) in table {
                if text == "clear" {
                    continue;
                }
                let place = format!("keys.{mode_name}.{text:?}");

                let Ok(chord) = Chord::parse(text) else {
                    warnings.push(format!("{place}: not a key"));
                    continue;
                };
                let KeyValue::Command(name) = value else {
                    warnings.push(format!("{place}: a command's name, in quotes"));
                    continue;
                };
                let Some(command) = Command::from_name(name) else {
                    warnings.push(format!("{place}: no command {name:?}"));
                    continue;
                };

                if command == Command::None {
                    keymap.unbind(mode, &chord);
                    continue;
                }
                if let Err(reason) = allowed(mode, command) {
                    warnings.push(format!("{place}: {reason}"));
                    continue;
                }
                keymap.bind(mode, chord, command);
            }
        }

        keymap.keep_a_way_out(&mut warnings);
        (keymap, warnings)
    }

    /// Puts back what a config took away that would leave the user stuck:
    /// lock's unlock, and `Esc` out of every mode that stays on.
    fn keep_a_way_out(&mut self, warnings: &mut Vec<String>) {
        if self.chords_for(KeyMode::Lock, Command::Unlock).is_empty() {
            self.bind(KeyMode::Lock, Chord::ctrl('g'), Command::Unlock);
            warnings.push("keys.lock: nothing unlocks, so Ctrl g still does".to_string());
        }

        let esc = Chord::key(KeyCode::Esc);
        for mode in KeyMode::ALL.into_iter().filter(|mode| mode.is_modal()) {
            let leave = if mode == KeyMode::Scroll {
                Command::LeaveScroll
            } else {
                Command::LeaveMode
            };
            if self.lookup(mode, &esc) != Some(leave) {
                self.bind(mode, esc, leave);
                warnings.push(format!("keys.{}: Esc always leaves the mode", mode.name()));
            }
        }
    }
```

And a free function after `impl Keymap`:

```rust
/// Why `command` cannot be bound in `mode`, when it cannot.
///
/// Modes are entered from normal mode, or through the prefix as `^a [`
/// enters scroll mode; lock looks up nothing but its unlock.
fn allowed(mode: KeyMode, command: Command) -> Result<(), &'static str> {
    if command.enters().is_some() && !matches!(mode, KeyMode::Normal | KeyMode::Prefix) {
        return Err("a mode is entered from normal mode or the prefix");
    }
    if mode == KeyMode::Lock && command != Command::Unlock {
        return Err("lock mode only unlocks");
    }
    if command == Command::Unlock && mode != KeyMode::Lock {
        return Err("only lock mode unlocks");
    }
    Ok(())
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p dispatch-config && cargo test -p dispatch-tui keymap && cargo build --workspace`
Expected: PASS, with 4 new config tests and 10 new keymap tests.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/dispatch-config crates/dispatch-tui/src/keymap.rs crates/dispatch-tui/src/keymap/tests.rs
git commit -m "feat(config): a [keys] section laid over the keys dispatch ships with"
```

---

### Task 4: The router reads the keymap

**Files:**
- Modify: `crates/dispatch-tui/src/input.rs`:
  - remove the `KeyMode` enum and put `pub use crate::keymap::KeyMode;` in its place;
  - remove `Prefix`, `with_prefix`, `command_for`, `is_tab_mode_key`, `tab_key`, `direct` and `Action::Scrollback`;
  - the router over `Keymap`, plus `send`.
- Modify: `crates/dispatch-tui/src/input/tests.rs`
- Modify: `crates/dispatch-tui/src/lib.rs`: `pub use input::{Action, Direction, InputRouter, KeyMode};` and `pub use keymap::{Chord, Command, Keymap};`, with `Prefix` gone.
- Modify: `dispatch/src/app.rs`:
  - rename `KeyMode::Tabs` to `KeyMode::Tab`;
  - replace Task 2's temporary arm and the `Scrollback` arm with real handlers;
  - add `focus_next`, `scroll_view` and `scroll_pages`;
  - the `Action::Scroll` arm uses `scroll_view`;
  - add tests.

**Interfaces:**
- Consumes: Tasks 1–3 (`Chord::from_event`, `Keymap::{defaults, lookup, chords_for, with_overrides}`, `Command::{action, enters, stays}`, `KeyMode::{entered_by, is_modal}`).
- Produces (used by Task 5):
  - `InputRouter::with_keymap(Keymap) -> Self`, `InputRouter::keymap(&self) -> &Keymap`.
  - `is_armed()` is true in prefix mode.
  - `leave_mode()` leaves any mode but lock.
  - `App::{focus_next, scroll_view(ScrollTo), scroll_pages(isize, bool)}`.

- [ ] **Step 1: Write the failing router tests**

In `crates/dispatch-tui/src/input/tests.rs`:
- Rename `KeyMode::Tabs` to `KeyMode::Tab` throughout.
- Delete `every_command_is_bound` and `a_custom_prefix_is_honoured`; they are replaced below.
- Add these imports:

```rust
use std::collections::BTreeMap;

use dispatch_config::{KeyValue, KeysConfig};

use crate::keymap::Keymap;
```

Then append:

```rust
fn ctrl(c: char) -> Event {
    press_with(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// A router in the mode `entering` puts it in.
fn in_mode(entering: Event) -> InputRouter {
    let mut router = router();
    router.handle(&entering, &[]);
    router
}

/// A router whose keys have `entries` laid over the defaults.
fn router_with(entries: &[(&str, &str, &str)]) -> InputRouter {
    let mut keys = KeysConfig::default();
    for (mode, chord, name) in entries {
        keys.0
            .entry((*mode).to_string())
            .or_insert_with(BTreeMap::new)
            .insert((*chord).to_string(), KeyValue::Command((*name).to_string()));
    }
    let (keymap, warnings) = Keymap::with_overrides(&keys);
    assert!(warnings.is_empty(), "{warnings:?}");
    InputRouter::with_keymap(keymap)
}

#[test]
fn every_prefix_command_is_bound_as_before() {
    let cases = [
        ('n', Action::NewPane),
        ('x', Action::ClosePane),
        ('z', Action::ToggleZoom),
        ('h', Action::FocusDirection(Direction::Left)),
        ('j', Action::FocusDirection(Direction::Down)),
        ('k', Action::FocusDirection(Direction::Up)),
        ('l', Action::FocusDirection(Direction::Right)),
        ('p', Action::ProjectPicker),
        ('H', Action::HarnessManager),
        ('a', Action::Approvals),
        ('s', Action::ExpandChild),
        ('c', Action::CollapseChild),
        ('f', Action::ToggleFold),
        ('o', Action::OpenProject),
        ('m', Action::AddMachine),
        ('q', Action::Quit),
        ('3', Action::SelectTab(2)),
    ];

    for (key, expected) in cases {
        let mut router = router();
        router.handle(&ctrl_a(), &[]);
        assert_eq!(
            router.handle(&press(KeyCode::Char(key)), &[]),
            expected,
            "prefix then {key:?}"
        );
        assert_eq!(router.key_mode(), KeyMode::Normal, "one key, then back");
    }

    let mut router = router();
    router.handle(&ctrl_a(), &[]);
    assert_eq!(router.handle(&press(KeyCode::Tab), &[]), Action::NextTab);
}

#[test]
fn the_prefix_then_a_bracket_opens_scroll_mode() {
    let mut router = router();
    router.handle(&ctrl_a(), &[]);

    assert_eq!(router.handle(&press(KeyCode::Char('[')), &[]), Action::None);
    assert_eq!(router.key_mode(), KeyMode::Scroll);
}

#[test]
fn each_mode_key_enters_its_mode() {
    for (c, mode) in [
        ('p', KeyMode::Pane),
        ('t', KeyMode::Tab),
        ('s', KeyMode::Scroll),
        ('o', KeyMode::Session),
        ('g', KeyMode::Lock),
    ] {
        let mut router = router();
        assert_eq!(router.handle(&ctrl(c), &[]), Action::None);
        assert_eq!(router.key_mode(), mode, "Ctrl {c}");
    }
}

#[test]
fn a_mode_key_twice_goes_to_the_pane() {
    // Every Ctrl key a mode takes is one some program uses: a shell's
    // history, XON/XOFF, Claude Code's task list. Twice is how it still gets
    // there.
    for c in ['p', 't', 's', 'o'] {
        let mut router = in_mode(ctrl(c));

        assert_eq!(
            router.handle(&ctrl(c), &[]),
            Action::SendKey(
                Key::Char(c),
                Modifiers {
                    ctrl: true,
                    ..Modifiers::NONE
                }
            ),
            "Ctrl {c} twice"
        );
        assert_eq!(router.key_mode(), KeyMode::Normal);
    }
}

/// Runs `cases` of (key, action, mode after) against a router freshly put
/// in the mode `entering` enters.
fn check_mode(entering: char, cases: &[(Event, Action, KeyMode)]) {
    for (event, action, after) in cases {
        let mut router = in_mode(ctrl(entering));
        assert_eq!(&router.handle(event, &[]), action, "Ctrl {entering} then {event:?}");
        assert_eq!(router.key_mode(), *after, "mode after Ctrl {entering} then {event:?}");
    }
}

#[test]
fn pane_mode_keys() {
    use KeyMode::{Normal, Pane};

    check_mode(
        'p',
        &[
            (press(KeyCode::Char('n')), Action::NewPane, Normal),
            (press(KeyCode::Char('x')), Action::ClosePane, Normal),
            (press(KeyCode::Char('f')), Action::ToggleZoom, Normal),
            (press(KeyCode::Char('z')), Action::ToggleZoom, Normal),
            (press(KeyCode::Char('h')), Action::FocusDirection(Direction::Left), Pane),
            (press(KeyCode::Down), Action::FocusDirection(Direction::Down), Pane),
            (press(KeyCode::Char('p')), Action::FocusNext, Pane),
            (press(KeyCode::Char('s')), Action::ExpandChild, Pane),
            (press(KeyCode::Char('c')), Action::CollapseChild, Pane),
            (press(KeyCode::Esc), Action::None, Normal),
            (press(KeyCode::Enter), Action::None, Normal),
            (press(KeyCode::Char('q')), Action::None, Pane),
        ],
    );
}

#[test]
fn tab_mode_steps_with_j_and_k_as_well() {
    use KeyMode::Tab;

    check_mode(
        't',
        &[
            (press(KeyCode::Char('j')), Action::NextTab, Tab),
            (press(KeyCode::Char('k')), Action::PreviousTab, Tab),
        ],
    );
}

#[test]
fn scroll_mode_keys() {
    use KeyMode::{Normal, Scroll};

    check_mode(
        's',
        &[
            (press(KeyCode::Char('j')), Action::Scroll(1), Scroll),
            (press(KeyCode::Up), Action::Scroll(-1), Scroll),
            (press(KeyCode::Char('d')), Action::ScrollHalfPages(1), Scroll),
            (press(KeyCode::Char('u')), Action::ScrollHalfPages(-1), Scroll),
            (press(KeyCode::PageDown), Action::ScrollPages(1), Scroll),
            (ctrl('b'), Action::ScrollPages(-1), Scroll),
            (press(KeyCode::Char('g')), Action::ScrollToTop, Scroll),
            (press(KeyCode::Char('G')), Action::ScrollToBottom, Scroll),
            (press(KeyCode::Esc), Action::ScrollToBottom, Normal),
            (ctrl('c'), Action::ScrollToBottom, Normal),
        ],
    );
}

#[test]
fn session_mode_keys() {
    use KeyMode::Normal;

    check_mode(
        'o',
        &[
            (press(KeyCode::Char('p')), Action::ProjectPicker, Normal),
            (press(KeyCode::Char('o')), Action::OpenProject, Normal),
            (press(KeyCode::Char('m')), Action::AddMachine, Normal),
            (press(KeyCode::Char('H')), Action::HarnessManager, Normal),
            (press(KeyCode::Char('a')), Action::Approvals, Normal),
            (press(KeyCode::Char('f')), Action::ToggleFold, Normal),
            (press(KeyCode::Char('q')), Action::Quit, Normal),
        ],
    );
}

#[test]
fn lock_sends_everything_but_its_unlock_to_the_pane() {
    let mut router = in_mode(ctrl('g'));

    for event in [
        ctrl('t'),
        ctrl('p'),
        ctrl_a(),
        press(KeyCode::Char('x')),
        press(KeyCode::Esc),
        alt(KeyCode::Char('n')),
    ] {
        assert!(
            matches!(router.handle(&event, &[]), Action::SendKey(..)),
            "{event:?} reaches the pane"
        );
        assert_eq!(router.key_mode(), KeyMode::Lock);
    }

    assert_eq!(router.handle(&ctrl('g'), &[]), Action::None);
    assert_eq!(router.key_mode(), KeyMode::Normal);
}

#[test]
fn a_click_or_a_paste_does_not_unlock() {
    let mut router = in_mode(ctrl('g'));

    router.handle(&mouse_down(), &[]);
    assert_eq!(router.key_mode(), KeyMode::Lock);
    assert_eq!(
        router.handle(&Event::Paste("hi".into()), &[]),
        Action::Paste("hi".into())
    );
    assert_eq!(router.key_mode(), KeyMode::Lock);
}

#[test]
fn a_click_ends_every_other_mode() {
    for c in ['p', 't', 's', 'o'] {
        let mut router = in_mode(ctrl(c));
        router.handle(&mouse_down(), &[]);
        assert_eq!(router.key_mode(), KeyMode::Normal, "Ctrl {c}");
    }
}

#[test]
fn a_key_rebound_in_keys_is_the_one_read() {
    let mut router = router_with(&[("pane", "w", "close_pane")]);
    router.handle(&ctrl('p'), &[]);

    assert_eq!(router.handle(&press(KeyCode::Char('w')), &[]), Action::ClosePane);
}

#[test]
fn a_prefix_moved_in_keys_is_honoured_and_the_old_key_goes_to_the_pane() {
    let mut router = router_with(&[
        ("normal", "Ctrl a", "none"),
        ("normal", "Ctrl b", "prefix"),
    ]);

    assert_eq!(
        router.handle(&ctrl_a(), &[]),
        Action::SendKey(
            Key::Char('a'),
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
        ),
        "Ctrl a is the shell's again"
    );
    router.handle(&ctrl('b'), &[]);
    assert!(router.is_armed());
    assert_eq!(router.handle(&press(KeyCode::Char('n')), &[]), Action::NewPane);
}
```

Run: `cargo test -p dispatch-tui input`
Expected: FAIL to compile, because there is no function `with_keymap`.

- [ ] **Step 2: Rewrite the router**

In `crates/dispatch-tui/src/input.rs`:
- Delete the `KeyMode` enum and put `pub use crate::keymap::KeyMode;` in its place.
- Add `use crate::keymap::{Chord, Command, Keymap};`.
- Delete `Action::Scrollback` and its doc comment.
- Delete `Prefix` and its impls, and `command_for`, `is_tab_mode_key` and `direct`.
- Update the module doc's last sentence to say that keys reach Dispatch through the keymap's modes and its prefix.

Replace `InputRouter` and its whole `impl` block, up to `handle_mouse` (keep `handle_mouse` and everything after it), with:

```rust
/// Routes input to panes or to Dispatch, reading keys through a keymap.
#[derive(Debug, Default)]
pub struct InputRouter {
    keymap: Keymap,
    /// Which keys it is reading.
    mode: KeyMode,
}

impl InputRouter {
    /// A router reading the keys Dispatch ships with.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A router reading `keymap`.
    #[must_use]
    pub fn with_keymap(keymap: Keymap) -> Self {
        Self {
            keymap,
            mode: KeyMode::Normal,
        }
    }

    /// The keys it reads, for the status row to spell out.
    #[must_use]
    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// Whether the next key will be read as a command after the prefix.
    ///
    /// Shown in the status bar: a prefix that armed invisibly is how a
    /// keystroke goes missing with no explanation.
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.mode == KeyMode::Prefix
    }

    /// Which keys the router is reading, for the status row to say.
    #[must_use]
    pub fn key_mode(&self) -> KeyMode {
        self.mode
    }

    /// Leaves whatever mode it is in, for a click the router never sees.
    ///
    /// Not lock: that is left only by its own key, or a click meant for a
    /// program that wants the mouse would unlock the keys it wants too.
    pub fn leave_mode(&mut self) {
        if self.mode != KeyMode::Lock {
            self.mode = KeyMode::Normal;
        }
    }

    /// Decides what an event means.
    ///
    /// `panes` gives the on-screen rectangle of each pane, used to resolve
    /// which one the pointer is over.
    pub fn handle(&mut self, event: &Event, panes: &[(PaneId, Rect)]) -> Action {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => {
                // A click ends a mode, then does what it would have anyway.
                if matches!(mouse.kind, MouseEventKind::Down(_)) {
                    self.leave_mode();
                }
                self.handle_mouse(mouse, panes)
            }
            Event::Paste(text) => {
                // Text for the pane, not a command. A mode ends too, or the
                // next key would be read as one the user never meant.
                self.leave_mode();
                Action::Paste(text.clone())
            }
            _ => Action::None,
        }
    }

    fn handle_key(&mut self, event: &KeyEvent) -> Action {
        // Key releases and repeats reach us on some platforms. Only presses
        // should act, or every keystroke would fire twice.
        if event.kind != KeyEventKind::Press {
            return Action::None;
        }
        let chord = Chord::from_event(event);

        match self.mode {
            KeyMode::Normal => match self.keymap.lookup(KeyMode::Normal, &chord) {
                Some(command) => self.run(command),
                None => send(event),
            },
            // Every key goes to the pane but the one that unlocks.
            KeyMode::Lock => {
                if self.keymap.lookup(KeyMode::Lock, &chord) == Some(Command::Unlock) {
                    self.mode = KeyMode::Normal;
                    Action::None
                } else {
                    send(event)
                }
            }
            mode => {
                // The key that entered the mode, pressed again, is for the
                // pane: the only way to type it into a program that wants it,
                // as `^a ^a` always was.
                let entering = mode
                    .entered_by()
                    .map(|command| self.keymap.chords_for(KeyMode::Normal, command))
                    .unwrap_or_default();
                if entering.contains(&chord) {
                    self.mode = KeyMode::Normal;
                    return send(event);
                }

                // The prefix reads one key, whatever it is.
                if mode == KeyMode::Prefix {
                    self.mode = KeyMode::Normal;
                }

                // An unbound key does nothing rather than reaching the pane,
                // so a stray key cannot run something in an agent, and a mode
                // stays on through it.
                let Some(command) = self.keymap.lookup(mode, &chord) else {
                    return Action::None;
                };
                let action = self.run(command);
                if mode.is_modal() && self.mode == mode && !command.stays() {
                    self.mode = KeyMode::Normal;
                }
                action
            }
        }
    }

    /// Carries out a command: the router's own — entering, leaving and
    /// unlocking a mode — here, and the rest as the action the app runs.
    fn run(&mut self, command: Command) -> Action {
        if let Some(mode) = command.enters() {
            self.mode = mode;
            return Action::None;
        }
        if matches!(command, Command::LeaveMode | Command::Unlock) {
            self.mode = KeyMode::Normal;
        }
        command.action().unwrap_or(Action::None)
    }
```

Close the `impl` block after `handle_mouse` as before. Then add, after `contains`:

```rust
/// The key, for the focused pane.
fn send(event: &KeyEvent) -> Action {
    match translate(event) {
        Some((key, mods)) => Action::SendKey(key, mods),
        None => Action::None,
    }
}
```

In `crates/dispatch-tui/src/lib.rs`, change the re-exports to:

```rust
pub use input::{Action, Direction, InputRouter, KeyMode};
pub use keymap::{Chord, Command, Keymap};
```

- [ ] **Step 3: Write the failing app tests**

In `dispatch/src/app.rs`'s test module, rename `KeyMode::Tabs` to `KeyMode::Tab` throughout, then append:

```rust
    /// The rows a pane shows now, as text.
    fn shown(app: &App, pane: PaneId) -> Vec<String> {
        app.panes[&pane].screen.text_lines()
    }

    /// A pane that has printed `line-001` to `line-100`, so it has
    /// scrollback.
    fn pane_with_history(app: &mut App, daemon: &Sender<ServerMessage>, project: ProjectId) -> PaneId {
        let pane = spawn_several(app, daemon, project, 1)[0];
        let output: String = (1..=100).map(|n| format!("line-{n:03}\r\n")).collect();
        print(app, daemon, pane, output.as_bytes());
        pane
    }

    #[test]
    fn scroll_mode_reads_back_through_a_panes_output_and_esc_returns() {
        let (mut app, project, daemon, _sent) = attached_app();
        let pane = pane_with_history(&mut app, &daemon, project);

        key_with(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(shown(&app, pane)[0].trim_end(), "line-001", "the oldest output");
        assert!(app.status.is_empty(), "the mode's own row says where the user is");

        // Half of a 24-row pane.
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(shown(&app, pane)[0].trim_end(), "line-013");

        press(&mut app, KeyCode::Esc);
        assert_eq!(app.router.key_mode(), KeyMode::Normal);
        assert!(
            shown(&app, pane).iter().any(|line| line.starts_with("line-100")),
            "back to live output"
        );
        assert!(!app.panes[&pane].scrolled_back);
    }

    #[test]
    fn scroll_mode_on_a_pane_with_no_scrollback_is_harmless() {
        let (mut app, project, daemon, _sent) = attached_app();
        let pane = spawn_several(&mut app, &daemon, project, 1)[0];
        print(&mut app, &daemon, pane, b"only-line\r\n");

        key_with(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
        for code in [KeyCode::Char('g'), KeyCode::Char('u'), KeyCode::PageUp, KeyCode::Char('k')] {
            press(&mut app, code);
        }

        assert!(shown(&app, pane).iter().any(|line| line.starts_with("only-line")));
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.router.key_mode(), KeyMode::Normal);
    }

    #[test]
    fn pane_mode_p_walks_the_panes_on_this_tab() {
        let (mut app, project, daemon, _sent) = attached_app();
        let panes = spawn_several(&mut app, &daemon, project, 3);
        app.focus_pane(panes[0]);

        key_with(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
        for expected in [panes[1], panes[2], panes[0]] {
            press(&mut app, KeyCode::Char('p'));
            assert_eq!(app.state.focused_pane(), Some(expected));
        }
        assert_eq!(app.router.key_mode(), KeyMode::Pane, "still walking");
    }

    #[test]
    fn the_prefix_then_a_bracket_opens_scroll_mode_in_the_app() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);

        command(&mut app, '[');

        assert_eq!(app.router.key_mode(), KeyMode::Scroll);
    }
```

Run: `cargo test -p dispatch`
Expected: FAIL. It won't compile: `KeyMode::Tab` is only there after Step 2, the `Scrollback` arm refers to a removed variant, and nothing handles the new actions.

- [ ] **Step 4: Wire the app**

In `dispatch/src/app.rs`, `App::handle`:
- Delete the `Action::Scrollback => …` arm.
- Replace Task 2's temporary arm with:

```rust
            Action::FocusNext => self.focus_next(),
            Action::ScrollHalfPages(pages) => self.scroll_pages(pages, true),
            Action::ScrollPages(pages) => self.scroll_pages(pages, false),
            Action::ScrollToTop => self.scroll_view(ScrollTo::Top),
            Action::ScrollToBottom => {
                if let Some(id) = self.state.focused_pane() {
                    self.scroll_to_bottom(id);
                }
            }
```

- Change the `Action::Scroll(rows)` arm to `Action::Scroll(rows) => self.scroll_view(ScrollTo::Delta(rows)),`. Only scroll mode sends it now, and its row already says where the user is.
- If `scroll_focused` is then unused, delete it. The mouse wheel keeps using `scroll_pane`, and its "scrolled back" message.
- In the clear-on-entry block, change `mode != KeyMode::Tabs && self.router.key_mode() == KeyMode::Tabs` to `KeyMode::Tab`. Task 5 widens it to every mode.

Add these methods to `impl App`, next to `scroll_pane`:

```rust
    /// Focuses the next pane on the tab on screen, wrapping to the first.
    fn focus_next(&mut self) {
        let panes = self.panes_on_tab();
        if panes.is_empty() {
            return;
        }
        let next = self
            .state
            .focused_pane()
            .and_then(|focused| panes.iter().position(|pane| *pane == focused))
            .map_or(0, |at| (at + 1) % panes.len());
        self.focus_pane(panes[next]);
    }

    /// Moves the focused pane's view over its scrollback, for scroll mode.
    ///
    /// Says nothing on the status row, unlike a wheel scroll: the mode's own
    /// row already says where the user is and how to get back.
    fn scroll_view(&mut self, to: ScrollTo) {
        let Some(id) = self.state.focused_pane() else {
            return;
        };
        let Some(pane) = self.panes.get_mut(&id) else {
            return;
        };

        pane.backend.terminal_mut().scroll(to);
        pane.scrolled_back = !matches!(to, ScrollTo::Bottom);

        if let Ok(screen) = pane.reader.read(pane.backend.terminal()) {
            pane.screen = screen;
        }
    }

    /// Scrolls the focused pane by `pages` of its own height, or of half its
    /// height; negative towards older output.
    fn scroll_pages(&mut self, pages: isize, half: bool) {
        let Some(rows) = self
            .state
            .focused_pane()
            .and_then(|id| self.panes.get(&id))
            .map(|pane| pane.backend.size().rows)
        else {
            return;
        };
        let step = if half { (rows / 2).max(1) } else { rows.max(1) };
        self.scroll_view(ScrollTo::Delta(pages * isize::from(step)));
    }
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p dispatch-tui && cargo test -p dispatch`
Expected: PASS. That includes 14 new router tests, 4 new app tests, and the renamed tab-mode tests from slice C.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/dispatch-tui dispatch/src/app.rs
git commit -m "feat(tui): read every key through the keymap, with pane, scroll, session and lock modes"
```

---

### Task 5: The status row, and keys from `config.toml`

**Files:**
- Modify: `dispatch/src/app.rs`:
  - `draw_status` builds each mode's row from the keymap;
  - remove `TAB_MODE_HELP`;
  - clear on entry for every modal mode;
  - scroll mode ends with its pane;
  - `set_keymap`;
  - tests.
- Modify: `dispatch/src/main.rs`: the keymap comes from `config.keys`.

**Interfaces:**
- Consumes: Task 4's `InputRouter::{with_keymap, keymap, key_mode, leave_mode}` and Task 2's `Keymap::{mode_help, normal_help, lock_help}`.
- Produces: `App::set_keymap(&mut self, Keymap)`.

- [ ] **Step 1: Write the failing tests**

Append to `dispatch/src/app.rs`'s test module:

```rust
    fn a_wide_terminal() -> ratatui::Terminal<ratatui::backend::TestBackend> {
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(220, 30))
            .expect("a test backend can be created")
    }

    #[test]
    fn each_mode_spells_out_its_keys_on_the_status_row() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        let mut terminal = a_wide_terminal();

        for (key, mode) in [
            ('p', KeyMode::Pane),
            ('t', KeyMode::Tab),
            ('s', KeyMode::Scroll),
            ('o', KeyMode::Session),
        ] {
            key_with(&mut app, KeyCode::Char(key), KeyModifiers::CONTROL);
            drawn(&mut app, &mut terminal);
            assert_eq!(
                bottom_row(&terminal).trim_end(),
                app.router.keymap().mode_help(mode),
                "{mode:?}"
            );
            press(&mut app, KeyCode::Esc);
        }
    }

    #[test]
    fn lock_says_how_to_unlock() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        let mut terminal = a_terminal();

        key_with(&mut app, KeyCode::Char('g'), KeyModifiers::CONTROL);
        drawn(&mut app, &mut terminal);

        assert_eq!(bottom_row(&terminal).trim_end(), "LOCKED  Ctrl g unlock");
    }

    #[test]
    fn the_normal_row_lists_the_mode_keys() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        let mut terminal = a_wide_terminal();

        drawn(&mut app, &mut terminal);

        assert!(
            bottom_row(&terminal).contains(
                "Ctrl p pane  Ctrl t tabs  Ctrl s scroll  Ctrl o session  Ctrl g lock  Ctrl a prefix"
            ),
            "{}",
            bottom_row(&terminal)
        );
    }

    #[test]
    fn a_rebound_key_shows_on_the_status_row() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        let mut keys = dispatch_config::KeysConfig::default();
        keys.0.insert(
            "pane".into(),
            [("w".to_string(), dispatch_config::KeyValue::Command("close_pane".into()))]
                .into_iter()
                .collect(),
        );
        app.set_keymap(dispatch_tui::Keymap::with_overrides(&keys).0);
        let mut terminal = a_wide_terminal();

        key_with(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
        drawn(&mut app, &mut terminal);

        assert!(bottom_row(&terminal).contains("x/w close"), "{}", bottom_row(&terminal));
    }

    #[test]
    fn entering_any_mode_clears_a_stale_message() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);

        app.status = "old".into();
        key_with(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert!(app.status.is_empty());

        press(&mut app, KeyCode::Esc);
        app.status = "old".into();
        command(&mut app, '[');
        assert_eq!(app.router.key_mode(), KeyMode::Scroll);
        assert!(app.status.is_empty(), "through the prefix too");
    }

    #[test]
    fn scroll_mode_ends_when_its_pane_goes() {
        let (mut app, project, daemon, _sent) = attached_app();
        let pane = spawn_several(&mut app, &daemon, project, 1)[0];
        let mut terminal = a_terminal();
        key_with(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);

        daemon
            .send(ServerMessage::PaneClosed { pane })
            .expect("the app is listening");
        app.poll_daemon();
        drawn(&mut app, &mut terminal);

        assert_eq!(app.router.key_mode(), KeyMode::Normal);
    }

    #[test]
    fn every_mode_but_normal_is_drawn_highlighted() {
        let (mut app, project, daemon, _sent) = attached_app();
        spawn_several(&mut app, &daemon, project, 1);
        let mut terminal = a_terminal();

        for key in ['p', 's', 'o', 'g'] {
            key_with(&mut app, KeyCode::Char(key), KeyModifiers::CONTROL);
            drawn(&mut app, &mut terminal);
            let row = terminal.backend().buffer().area.height - 1;
            let cell = terminal
                .backend()
                .buffer()
                .cell((0, row))
                .expect("the status row is drawn");
            assert_eq!(cell.bg, app.theme.tab, "Ctrl {key}");
            key_with(&mut app, KeyCode::Char(key), KeyModifiers::CONTROL);
        }
    }
```

In the last test, `Ctrl <key>` again leaves each mode: for `p`, `s` and `o` it is sent through to the pane, and for `g` it unlocks.

Slice C's `tab_mode_is_spelled_out_on_the_status_row` asserts `starts_with(TAB_MODE_HELP)`. The generated tab row is longer than the 100-column test terminal, so change that assertion to

```rust
app.router.keymap().mode_help(KeyMode::Tab).starts_with(bottom_row(&terminal).trim_end())
```

and list the change.

Run: `cargo test -p dispatch`
Expected: FAIL to compile, because there is no method `set_keymap`.

- [ ] **Step 2: Implement**

In `dispatch/src/app.rs`:
- Delete `TAB_MODE_HELP` and its doc comment.
- Add `Keymap` to the `dispatch_tui` import.
- Add this method:

```rust
    /// Reads keys through `keymap`: the defaults with the user's `[keys]`
    /// laid over them.
    pub fn set_keymap(&mut self, keymap: Keymap) {
        self.router = InputRouter::with_keymap(keymap);
    }
```

In `App::handle`, widen the clear-on-entry test to every modal mode:

```rust
        // Entering a mode wipes a stale message, so what the mode's row shows
        // between its name and its keys is about this mode.
        if !mode.is_modal() && self.router.key_mode().is_modal() {
            self.status.clear();
        }
```

In `draw`, straight after `self.notice_focus(now);`, add:

```rust
        // Scroll mode reads the focused pane's scrollback; with that pane gone
        // there is nothing left to read, and the mode would hold keys for
        // nothing.
        if self.router.key_mode() == KeyMode::Scroll
            && self
                .state
                .focused_pane()
                .is_none_or(|id| !self.panes.contains_key(&id))
        {
            self.router.leave_mode();
        }
```

In `draw_status`, replace the `let text = if self.router.is_armed() { … } else if self.router.key_mode() == KeyMode::Tabs { … } else {` opening with:

```rust
        let mode = self.router.key_mode();
        let keymap = self.router.keymap();
        let text = if mode == KeyMode::Prefix {
            // A prefix that armed invisibly is how a keystroke goes missing
            // with no explanation.
            "PREFIX".to_string()
        } else if mode == KeyMode::Lock {
            keymap.lock_help()
        } else if mode.is_modal() {
            // A mode spells out its own keys, from the same table that runs
            // them. A message goes between its name and its keys: several
            // keys keep the mode on, and a refusal hidden behind the key list
            // would make the key look dead.
            let help = keymap.mode_help(mode);
            match help.split_once("  ") {
                Some((title, keys)) if !self.status.is_empty() => {
                    format!("{title}  {}  {keys}", self.status)
                }
                _ => help,
            }
        } else {
```

Keep the `else` body. Change its key help to:

```rust
                let help = format!("{panes} pane(s){where_}{tabs}  {}", keymap.normal_help());
```

Change the style test to `let style = if mode != KeyMode::Normal {`.

In `dispatch/src/main.rs`, after the `App` is built and `set_motion` is called, add:

```rust
    // Keys are the interface's alone; the daemon never reads `[keys]`. What
    // could not be used is logged by name, and the rest still applies.
    let (keymap, warnings) = dispatch_tui::Keymap::with_overrides(&loaded.config.keys);
    for warning in &warnings {
        tracing::warn!(%warning, path = %config_path.display(), "ignoring a key binding");
    }
    app.set_keymap(keymap);
```

If `loaded` or `config_path` has moved out of scope by that point, bind the keymap from `loaded.config.keys` where `loaded` is read, and hand it to the app once the app exists.

- [ ] **Step 3: Run the tests**

Run: `cargo test -p dispatch`
Expected: PASS, including 7 new app tests.

- [ ] **Step 4: Lint and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add dispatch/src/app.rs dispatch/src/main.rs
git commit -m "feat(dispatch): a status row written from the keymap, and keys from config.toml"
```

---

### Task 6: End to end, and the README

**Files:**
- Modify: `dispatch/tests/end_to_end.rs`
- Modify: `README.md`

**Interfaces:**
- Consumes: the whole feature.

- [ ] **Step 1: Rewrite the scrollback test for scroll mode**

`a_pane_can_be_scrolled_back_and_typing_returns_to_the_newest_output` in `dispatch/tests/end_to_end.rs` sends `^a [` and waits for "scrolled back". `^a [` now opens scroll mode, where typed letters are keys rather than text. Replace the part of its body from `app.send(b"\x01[");` to the end with:

```rust
    // The prefix route into scroll mode, which is what `^a [` became.
    app.send(b"\x01[");
    assert!(
        app.wait_for(|lines| contains(lines, "SCROLL")),
        "the prefix then [ opens scroll mode"
    );

    // Half a page back: the newest row is no longer on screen.
    app.send(b"u");
    assert!(
        app.wait_for(|lines| !contains(lines, "row80")),
        "the view moved back through the output"
    );

    app.send(b"\x1b");
    assert!(
        app.wait_for(|lines| contains(lines, "row80") && !contains(lines, "SCROLL")),
        "Esc returns to the newest output"
    );

    app.send(b"echo back-at-the-bottom\r");
    assert!(
        app.wait_for(|lines| contains(lines, "back-at-the-bottom")),
        "typing reaches the shell again"
    );
```

Rename it `the_prefix_then_a_bracket_scrolls_back_and_esc_returns_to_the_newest_output`, and change its opening comment to:

```rust
    // Scrollback is only useful if new output does not yank the view away
    // and there is a plain way back, which in scroll mode is Esc.
```

The wheel keeps its old behaviour, with "scrolled back" and typing to return. The app's unit tests cover that.

- [ ] **Step 2: Write the new end-to-end tests**

Append to `dispatch/tests/end_to_end.rs`:

```rust
#[test]
#[cfg_attr(windows, ignore = "the test harness spawns a POSIX shell")]
fn scroll_mode_reads_a_shells_output_back_and_esc_returns() {
    let mut app = Harness::start(Size::new(100, 30));
    assert!(app.wait_for(|lines| contains(lines, "pane(s)")));
    app.spawn_shell();

    app.send(b"seq -f 'line-%03g' 1 200\r");
    assert!(app.wait_for(|lines| contains(lines, "line-200")));

    // Ctrl s, then k thirty times: a line at a time, past a whole pane.
    app.send(b"\x13");
    app.send(&[b'k'; 30]);
    assert!(
        app.wait_for(|lines| contains(lines, "SCROLL") && !contains(lines, "line-200")),
        "k scrolls the output back"
    );

    // g: the oldest output.
    app.send(b"g");
    assert!(
        app.wait_for(|lines| contains(lines, "line-001")),
        "g shows where the output began"
    );

    app.send(b"\x1b");
    assert!(
        app.wait_for(|lines| contains(lines, "line-200") && !contains(lines, "SCROLL")),
        "Esc returns to live output"
    );
}

#[test]
#[cfg_attr(windows, ignore = "the test harness spawns a POSIX shell")]
fn lock_mode_gives_the_shell_the_keys_dispatch_takes() {
    let mut app = Harness::start(Size::new(100, 30));
    assert!(app.wait_for(|lines| contains(lines, "pane(s)")));
    app.spawn_shell();
    // Type-ahead into a shell that has not started yet can be lost.
    assert!(
        app.wait_for(|lines| contains(lines, "$")),
        "the shell should print a prompt"
    );

    // `cat -v` shows a control key it is given as `^P`.
    app.send(b"cat -v\r");

    // Ctrl g locks.
    app.send(b"\x07");
    assert!(app.wait_for(|lines| contains(lines, "LOCKED")));

    // Ctrl p, pane mode's key, is the shell's while locked. Ctrl p rather than
    // Ctrl t: on macOS the terminal driver takes Ctrl t itself (it prints the
    // load), so it would never reach `cat` and the test would prove nothing.
    app.send(b"\x10\r");
    assert!(
        app.wait_for(|lines| contains(lines, "^P")),
        "the shell was given Ctrl p"
    );
    let lines = app.lines();
    assert!(contains(&lines, "LOCKED"), "still locked");
    assert!(!contains(&lines, "PANE  n new"), "no pane mode while locked");

    // Ctrl d ends `cat`; Ctrl g unlocks.
    app.send(b"\x04\x07");
    assert!(
        app.wait_for(|lines| !contains(lines, "LOCKED")),
        "Ctrl g unlocks"
    );
}

#[test]
#[cfg_attr(windows, ignore = "the test harness spawns a POSIX shell")]
fn a_key_rebound_in_config_toml_is_the_one_dispatch_reads() {
    let fixture = Fixture::new("rebind");
    let config = fixture.config.path().join("config.toml");
    let mut text = std::fs::read_to_string(&config).expect("the fixture wrote its config");
    text.push_str("\n[keys.normal]\n\"Ctrl a\" = \"none\"\n\"Ctrl b\" = \"prefix\"\n");
    std::fs::write(&config, text).expect("temp dir is writable");

    let mut app = Harness::spawn(&fixture, Size::new(100, 30), &[]);
    assert!(app.wait_for(|lines| contains(lines, "pane(s)")));

    // Ctrl b, then n: the picker, through the moved prefix.
    app.send(b"\x02");
    assert!(app.wait_for(|lines| contains(lines, "PREFIX")));
    app.send(b"n");
    assert!(
        app.wait_for(|lines| contains(lines, "New pane")),
        "the new prefix reaches its commands"
    );
    app.send(b"\x1b");
}
```

`Harness::{start, spawn, spawn_shell, send, lines, wait_for}`, `Fixture` and `contains` are the file's existing helpers. The fixture already writes a `config.toml` with `[shell]`, and the last test adds `[keys]` to it. `spawn_shell` does not wait for a prompt, which is why the lock test does.

Run: `cargo test -p dispatch --test end_to_end`
Expected: PASS, including the 3 new tests and the rewritten one. Every other end-to-end test still passes.

- [ ] **Step 3: The README**

In `README.md`:

1. **The grid.** In its last paragraph, change `` `^a x` is what removes it for good `` to `` `Ctrl p x` (or `^a x`) is what removes it for good ``.
2. **Tabs.**
   - In the table, change the row `` | `←` `→` / `h` `l` | previous / next tab | `` to `` | `←` `→` / `h` `l` / `k` `j` | previous / next tab | ``.
   - Replace the paragraph that begins "Some keys work without a mode:" with:

     ```
     Some keys work without a mode: see Keys below.
     ```

3. **Keys.** Add this section straight after the Tabs section (before `## Shell panes`):

~~~markdown
## Keys

Keys work the way zellij's do: a `Ctrl` key enters a mode for one kind of
thing, the status row lists that mode's keys, and `Esc` leaves it. Every
other key goes to the pane.

| Key | Mode |
|---|---|
| `Ctrl p` | pane: `n` new, `x` close, `f`/`z` zoom, `h` `j` `k` `l` or arrows to move focus, `p` next pane, `s` open a subagent, `c` collapse it |
| `Ctrl t` | tab: see Tabs above |
| `Ctrl s` | scroll: `j` `k` a line, `d` `u` half a page, `PageDown` `PageUp` (or `Ctrl f` `Ctrl b`, `l` `h`) a page, `g` `G` the oldest and newest output; `Esc` returns to live output |
| `Ctrl o` | session: `p` projects, `o` open a project, `m` add a machine, `H` harnesses, `a` approvals, `f` fold, `q` quit |
| `Ctrl g` | lock: every key goes to the pane until `Ctrl g` again |
| `Ctrl a` | the prefix: one command key, as in tmux — every `^a` command still works, and `^a [` opens scroll mode |

Some keys work without a mode: `Alt n` opens a new pane on this tab,
`Alt i` / `Alt o` move the tab, and `Alt` with an arrow or `h` `j` `k` `l`
moves focus, going on to the next tab at the grid's edge. As in zellij, a
quick `Esc` followed by a letter (as in vim) can reach Dispatch as `Alt` and
that letter.

A mode's key pressed twice goes to the pane (`Ctrl s Ctrl s` gives a shell
its `Ctrl s`), and lock mode gives a program every key until you unlock.
`Ctrl q` does not quit on its own — standalone, quitting ends every agent —
so quit is `Ctrl o q` or `^a q`.

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
`quit`, `pane_mode`, `lock`, `prefix`, and so on. A mistake is logged by
name and skipped, and the rest still applies. `Esc` always leaves a mode,
and lock always has a key that unlocks. Keys are read when Dispatch starts.
~~~

4. **Everywhere else.** In the rest of the README, mentions like `` `^a f` ``, `` `^a o` ``, `` `^a p` ``, `` `^a m` ``, `` `^a a` ``, `` `^a s` `` and `` `^a c` `` stay true. Leave them.

- [ ] **Step 4: Commit**

Run: `cargo fmt --all --check`

```bash
git add dispatch/tests/end_to_end.rs README.md
git commit -m "docs: document the key modes and [keys], and drive them end to end"
```

---

### Task 7: Verify the whole branch

**Files:** none, unless a check fails. Then fix it, and commit as `fix(dispatch): what verifying keybindings turned up`.

- [ ] **Step 1: The whole suite.** Run `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --no-fail-fast`. Report the total.
- [ ] **Step 2: The other platforms.**
  - Run `cargo clippy -p dispatch-tui -p dispatch-config -p dispatch-core -p dispatch-proto -p dispatch-pty --all-targets --target aarch64-apple-darwin -- -D warnings`.
  - Run the Windows-gnu workspace clippy. Confirm nothing new comes from a file this branch changed (`git diff --name-only ui/tabs-shells...HEAD`).
- [ ] **Step 3: Run it.**
  - Build `dispatch`. Drive it under `script` in a scratchpad copy with `[shell] command = "sh"`, `login = "never"`, and send this sequence:
    1. `\x01n`, then `\r` (a shell);
    2. `seq 1 300\r`;
    3. `\x13g` (scroll to the top), then `\x1b`;
    4. `\x07` (lock), then `\x07`;
    5. `\x10` (pane mode), then `\x1b`;
    6. `\x01q`.
  - Expected in the extracted text: `SCROLL`, `LOCKED`, `PANE  n new`, and no `target/debug/dispatch` process afterwards. Check with `ps -eo pid,args | grep target/debug/dispatch | grep -v grep`.
- [ ] **Step 4: The spec against the code.** Read `docs/superpowers/specs/2026-09-27-keybindings-design.md` against the code. Fix whichever is wrong. Update the spec's status line to "implemented on branch `ui/keybindings`".
- [ ] **Step 5: Commit, if anything changed.** Run `git status --short`, then stage by name.
