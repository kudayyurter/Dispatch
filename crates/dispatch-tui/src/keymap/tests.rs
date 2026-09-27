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
    assert_eq!(
        keymap.lookup(KeyMode::Normal, &parsed("Ctrl q")),
        None,
        "quit is not one keystroke away"
    );
    assert_eq!(
        keymap.bindings(KeyMode::Lock).len(),
        1,
        "lock looks up only its unlock"
    );
}

#[test]
fn normal_mode_takes_no_plain_typing_from_a_pane() {
    for (chord, _) in Keymap::defaults().bindings(KeyMode::Normal) {
        assert!(
            chord
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT),
            "{chord} would steal typing"
        );
    }
}

#[test]
fn every_command_has_a_name_that_reads_back() {
    let mut commands: Vec<Command> = Command::NAMED.iter().map(|(command, _)| *command).collect();
    commands.extend((1..=9).map(Command::GoToTab));

    for command in commands {
        assert_eq!(
            Command::from_name(&command.name()),
            Some(command),
            "{command:?}"
        );
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
    assert_eq!(
        Command::ScrollHalfDown.action(),
        Some(Action::ScrollHalfPages(1))
    );
    assert_eq!(
        Command::ScrollPageUp.action(),
        Some(Action::ScrollPages(-1))
    );
    assert_eq!(Command::LeaveScroll.action(), Some(Action::ScrollToBottom));
    assert_eq!(Command::Fold.action(), Some(Action::ToggleFold));
    assert_eq!(
        Command::PaneMode.action(),
        None,
        "entering a mode is the router's"
    );
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

    assert_eq!(
        keymap.lookup(KeyMode::Pane, &parsed("x")),
        Some(Command::Zoom)
    );
    assert_eq!(
        keymap
            .bindings(KeyMode::Pane)
            .iter()
            .filter(|(chord, _)| *chord == parsed("x"))
            .count(),
        1,
        "binding a chord again replaces it rather than adding a second"
    );
}
