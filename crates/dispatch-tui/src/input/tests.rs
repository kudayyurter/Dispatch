//! Tests for input routing.

use super::*;

use std::collections::BTreeMap;

use dispatch_core::PaneId;

use dispatch_config::{KeyValue, KeysConfig};

use crate::keymap::Keymap;

fn press(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn press_with(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn ctrl_a() -> Event {
    press_with(KeyCode::Char('a'), KeyModifiers::CONTROL)
}

fn moved(column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn router() -> InputRouter {
    InputRouter::new()
}

#[test]
fn an_ordinary_key_goes_to_the_pane() {
    let mut router = router();

    assert_eq!(
        router.handle(&press(KeyCode::Char('x')), &[]),
        Action::SendKey(Key::Char('x'), Modifiers::NONE)
    );
}

#[test]
fn control_keys_reach_the_pane_unchanged() {
    // Ctrl-C must reach the agent; intercepting it would make agents
    // uninterruptible.
    let mut router = router();

    assert_eq!(
        router.handle(&press_with(KeyCode::Char('c'), KeyModifiers::CONTROL), &[]),
        Action::SendKey(
            Key::Char('c'),
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
        )
    );
}

#[test]
fn special_keys_reach_the_pane() {
    let mut router = router();

    assert_eq!(
        router.handle(&press(KeyCode::Enter), &[]),
        Action::SendKey(Key::Enter, Modifiers::NONE)
    );
    assert_eq!(
        router.handle(&press(KeyCode::Up), &[]),
        Action::SendKey(Key::Up, Modifiers::NONE)
    );
    assert_eq!(
        router.handle(&press(KeyCode::F(5)), &[]),
        Action::SendKey(Key::Function(5), Modifiers::NONE)
    );
}

#[test]
fn the_prefix_alone_produces_nothing_and_arms() {
    let mut router = router();

    assert_eq!(router.handle(&ctrl_a(), &[]), Action::None);
    assert!(router.is_armed(), "the prefix should arm the next key");
}

#[test]
fn a_command_runs_after_the_prefix() {
    let mut router = router();
    router.handle(&ctrl_a(), &[]);

    assert_eq!(
        router.handle(&press(KeyCode::Char('z')), &[]),
        Action::ToggleZoom
    );
    assert!(!router.is_armed(), "the prefix should disarm after one key");
}

#[test]
fn the_prefix_twice_sends_the_prefix_itself() {
    // Otherwise there is no way to type Ctrl-a into an agent that wants it.
    let mut router = router();
    router.handle(&ctrl_a(), &[]);

    assert_eq!(
        router.handle(&ctrl_a(), &[]),
        Action::SendKey(
            Key::Char('a'),
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
        )
    );
    assert!(!router.is_armed());
}

#[test]
fn an_unbound_key_after_the_prefix_does_nothing() {
    // It must not fall through to the pane: a mistyped command would
    // otherwise run something inside an agent.
    let mut router = router();
    router.handle(&ctrl_a(), &[]);

    assert_eq!(router.handle(&press(KeyCode::Char('Q')), &[]), Action::None);
    assert!(!router.is_armed());
}

#[test]
fn a_command_key_without_the_prefix_reaches_the_pane() {
    let mut router = router();

    assert_eq!(
        router.handle(&press(KeyCode::Char('z')), &[]),
        Action::SendKey(Key::Char('z'), Modifiers::NONE),
        "z is only a command after the prefix"
    );
}

#[test]
fn key_releases_are_ignored() {
    // Windows and the kitty protocol report releases; acting on them would
    // send every keystroke twice.
    let mut router = router();
    let mut event = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    event.kind = KeyEventKind::Release;

    assert_eq!(router.handle(&Event::Key(event), &[]), Action::None);
}

#[test]
fn a_release_does_not_consume_an_armed_prefix() {
    let mut router = router();
    router.handle(&ctrl_a(), &[]);

    let mut release = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE);
    release.kind = KeyEventKind::Release;
    router.handle(&Event::Key(release), &[]);

    assert!(router.is_armed(), "a release should not disarm the prefix");
    assert_eq!(
        router.handle(&press(KeyCode::Char('z')), &[]),
        Action::ToggleZoom
    );
}

#[test]
fn moving_the_pointer_focuses_the_pane_under_it() {
    let left = PaneId::new();
    let right = PaneId::new();
    let panes = [
        (left, Rect::new(0, 0, 10, 10)),
        (right, Rect::new(10, 0, 10, 10)),
    ];

    let mut router = router();

    assert_eq!(router.handle(&moved(5, 5), &panes), Action::FocusPane(left));
    assert_eq!(
        router.handle(&moved(15, 5), &panes),
        Action::FocusPane(right)
    );
}

#[test]
fn the_pane_boundary_is_exact() {
    // Column 10 belongs to the right pane, not the left.
    let left = PaneId::new();
    let right = PaneId::new();
    let panes = [
        (left, Rect::new(0, 0, 10, 10)),
        (right, Rect::new(10, 0, 10, 10)),
    ];

    let mut router = router();

    assert_eq!(router.handle(&moved(9, 0), &panes), Action::FocusPane(left));
    assert_eq!(
        router.handle(&moved(10, 0), &panes),
        Action::FocusPane(right)
    );
}

#[test]
fn moving_outside_every_pane_changes_nothing() {
    // The sidebar is not a pane, and crossing it must not drop focus.
    let pane = PaneId::new();
    let panes = [(pane, Rect::new(10, 0, 10, 10))];

    let mut router = router();

    assert_eq!(router.handle(&moved(2, 2), &panes), Action::None);
}

#[test]
fn a_paste_is_delivered_as_text() {
    // Bracketed paste arrives whole; sending it key by key would let an agent
    // act on a half-pasted line.
    let mut router = router();

    assert_eq!(
        router.handle(&Event::Paste("two words".into()), &[]),
        Action::Paste("two words".into())
    );
}

fn ctrl_t() -> Event {
    press_with(KeyCode::Char('t'), KeyModifiers::CONTROL)
}

fn alt(code: KeyCode) -> Event {
    press_with(code, KeyModifiers::ALT)
}

fn mouse_down() -> Event {
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton_::Left),
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn ctrl_t_enters_tab_mode_and_sends_nothing() {
    let mut router = router();

    assert_eq!(router.handle(&ctrl_t(), &[]), Action::None);
    assert_eq!(router.key_mode(), KeyMode::Tab);
}

#[test]
fn every_tab_mode_key_is_bound_and_says_whether_the_mode_stays() {
    let cases = [
        (KeyCode::Char('n'), Action::NewTab, KeyMode::Normal),
        (KeyCode::Char('r'), Action::RenameTab, KeyMode::Normal),
        (KeyCode::Char('x'), Action::CloseTab, KeyMode::Normal),
        (KeyCode::Left, Action::PreviousTab, KeyMode::Tab),
        (KeyCode::Char('h'), Action::PreviousTab, KeyMode::Tab),
        (KeyCode::Right, Action::NextTab, KeyMode::Tab),
        (KeyCode::Char('l'), Action::NextTab, KeyMode::Tab),
        (KeyCode::Char('['), Action::MovePaneLeft, KeyMode::Tab),
        (KeyCode::Char(']'), Action::MovePaneRight, KeyMode::Tab),
        (KeyCode::Char('i'), Action::MoveTabLeft, KeyMode::Tab),
        (KeyCode::Char('o'), Action::MoveTabRight, KeyMode::Tab),
        (KeyCode::Char('3'), Action::SelectTab(2), KeyMode::Normal),
        (KeyCode::Tab, Action::LastTab, KeyMode::Normal),
        (KeyCode::Esc, Action::None, KeyMode::Normal),
        (KeyCode::Enter, Action::None, KeyMode::Normal),
    ];

    for (code, action, after) in cases {
        let mut router = router();
        router.handle(&ctrl_t(), &[]);

        assert_eq!(
            router.handle(&press(code), &[]),
            action,
            "tab mode then {code:?}"
        );
        assert_eq!(router.key_mode(), after, "the mode after {code:?}");
    }
}

#[test]
fn any_other_key_in_tab_mode_is_ignored_and_the_mode_stays() {
    // A stray key must neither reach a pane nor drop the user out of what
    // they were doing.
    let mut router = router();
    router.handle(&ctrl_t(), &[]);

    for event in [
        press(KeyCode::Char('q')),
        press(KeyCode::Char('z')),
        ctrl_a(),
        alt(KeyCode::Char('n')),
    ] {
        assert_eq!(router.handle(&event, &[]), Action::None);
    }
    assert_eq!(router.key_mode(), KeyMode::Tab);
}

#[test]
fn a_paste_in_tab_mode_ends_it_and_goes_to_the_pane() {
    // A paste is text for the pane; left in tab mode, the user's next key
    // would be read as a tab command they never meant.
    let mut router = router();
    router.handle(&ctrl_t(), &[]);

    assert_eq!(
        router.handle(&Event::Paste("hi".into()), &[]),
        Action::Paste("hi".into())
    );
    assert_eq!(router.key_mode(), KeyMode::Normal);
}

#[test]
fn ctrl_t_twice_sends_ctrl_t_to_the_pane() {
    // Claude Code's task list and a shell's fzf both use it.
    let mut router = router();
    router.handle(&ctrl_t(), &[]);

    assert_eq!(
        router.handle(&ctrl_t(), &[]),
        Action::SendKey(
            Key::Char('t'),
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
        )
    );
    assert_eq!(router.key_mode(), KeyMode::Normal);
}

#[test]
fn a_click_ends_tab_mode() {
    let mut router = router();
    router.handle(&ctrl_t(), &[]);

    router.handle(&mouse_down(), &[]);

    assert_eq!(router.key_mode(), KeyMode::Normal);
}

#[test]
fn tab_mode_does_not_start_while_the_prefix_is_armed() {
    let mut router = router();
    router.handle(&ctrl_a(), &[]);

    assert_eq!(router.handle(&ctrl_t(), &[]), Action::None);
    assert_eq!(router.key_mode(), KeyMode::Normal);
}

#[test]
fn the_direct_alt_keys_reach_dispatch() {
    let cases = [
        (KeyCode::Char('n'), Action::NewPane),
        (KeyCode::Char('i'), Action::MoveTabLeft),
        (KeyCode::Char('o'), Action::MoveTabRight),
        (KeyCode::Left, Action::FocusOrTab(Direction::Left)),
        (KeyCode::Char('h'), Action::FocusOrTab(Direction::Left)),
        (KeyCode::Right, Action::FocusOrTab(Direction::Right)),
        (KeyCode::Char('l'), Action::FocusOrTab(Direction::Right)),
        (KeyCode::Up, Action::FocusDirection(Direction::Up)),
        (KeyCode::Char('k'), Action::FocusDirection(Direction::Up)),
        (KeyCode::Down, Action::FocusDirection(Direction::Down)),
        (KeyCode::Char('j'), Action::FocusDirection(Direction::Down)),
    ];

    for (code, expected) in cases {
        let mut router = router();
        assert_eq!(router.handle(&alt(code), &[]), expected, "Alt {code:?}");
    }
}

#[test]
fn an_alt_key_dispatch_does_not_bind_still_reaches_the_pane() {
    let mut router = router();

    assert_eq!(
        router.handle(&alt(KeyCode::Char('b')), &[]),
        Action::SendKey(
            Key::Char('b'),
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            }
        )
    );
    assert!(matches!(
        router.handle(
            &press_with(KeyCode::Char('N'), KeyModifiers::ALT | KeyModifiers::SHIFT),
            &[]
        ),
        Action::SendKey(..)
    ));
}

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
        assert_eq!(
            &router.handle(event, &[]),
            action,
            "Ctrl {entering} then {event:?}"
        );
        assert_eq!(
            router.key_mode(),
            *after,
            "mode after Ctrl {entering} then {event:?}"
        );
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
            (
                press(KeyCode::Char('h')),
                Action::FocusDirection(Direction::Left),
                Pane,
            ),
            (
                press(KeyCode::Down),
                Action::FocusDirection(Direction::Down),
                Pane,
            ),
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
            (
                press(KeyCode::Char('d')),
                Action::ScrollHalfPages(1),
                Scroll,
            ),
            (
                press(KeyCode::Char('u')),
                Action::ScrollHalfPages(-1),
                Scroll,
            ),
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

    assert_eq!(
        router.handle(&press(KeyCode::Char('w')), &[]),
        Action::ClosePane
    );
}

#[test]
fn a_prefix_moved_in_keys_is_honoured_and_the_old_key_goes_to_the_pane() {
    let mut router = router_with(&[("normal", "Ctrl a", "none"), ("normal", "Ctrl b", "prefix")]);

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
    assert_eq!(
        router.handle(&press(KeyCode::Char('n')), &[]),
        Action::NewPane
    );
}
