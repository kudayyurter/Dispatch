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
        Chord::new(
            KeyCode::Char('x'),
            KeyModifiers::CONTROL | KeyModifiers::ALT
        )
    );
    assert_eq!(
        parsed("Super k"),
        Chord::new(KeyCode::Char('k'), KeyModifiers::SUPER)
    );
}

#[test]
fn shift_on_a_letter_is_its_capital() {
    assert_eq!(parsed("Shift h"), parsed("H"));
    assert_eq!(
        parsed("Ctrl Shift t"),
        Chord::new(KeyCode::Char('T'), KeyModifiers::CONTROL)
    );
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
    assert_eq!(
        parsed("Shift Tab"),
        Chord::new(KeyCode::Tab, KeyModifiers::SHIFT)
    );
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
