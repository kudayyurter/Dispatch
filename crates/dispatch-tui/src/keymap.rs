//! Keys as a user writes them, and what each mode binds them to.
//!
//! One table per mode rather than a match per mode: the built-in keys and
//! the user's `[keys]` are then the same kind of thing, and the status row
//! can say what a mode's keys do by reading the table that runs them.

use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use dispatch_config::{KeyValue, KeysConfig};

use crate::input::{Action, Direction};

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
                format!("{}-{}", chords[0].short(), chords[chords.len() - 1].short())
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
}

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

#[cfg(test)]
mod tests;
