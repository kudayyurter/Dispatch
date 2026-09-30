//! Deciding what a keystroke or mouse event means.
//!
//! A focused pane receives every key verbatim, so an agent's own full-screen
//! interface works unchanged. A prefix key escapes to Dispatch's commands,
//! which is the only way to have both without stealing bindings the agents
//! already use. Keys reach Dispatch through the keymap's modes and its
//! prefix.

use dispatch_core::PaneId;
use dispatch_pty::{Key, Modifiers, MouseAction, MouseButton, MouseInput};
use ratatui::layout::Rect;

pub use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};

// Renamed to keep it distinct from the encoder's own button type, which this
// module converts into.
use crossterm::event::MouseButton as MouseButton_;

pub use crate::keymap::KeyMode;
use crate::keymap::{Chord, Command, Keymap};

/// Which way to move focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Left.
    Left,
    /// Down.
    Down,
    /// Up.
    Up,
    /// Right.
    Right,
}

/// What an input event should cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Nothing.
    None,
    /// Send a key to the focused pane.
    ///
    /// Carries the key rather than bytes: encoding depends on the receiving
    /// pane's current modes, which only the caller knows.
    SendKey(Key, Modifiers),
    /// Paste text into the focused pane.
    Paste(String),
    /// Focus a pane, from the mouse moving over it.
    FocusPane(PaneId),
    /// Forward a pointer event to a pane, in coordinates relative to it.
    SendMouse(PaneId, MouseInput),
    /// Scroll the focused pane's viewport by a signed number of rows.
    Scroll(isize),
    /// Move focus in a direction.
    FocusDirection(Direction),
    /// Open the pane picker.
    NewPane,
    /// Close the focused pane.
    ClosePane,
    /// Zoom the focused pane, or restore the grid.
    ToggleZoom,
    /// Open the project picker.
    ProjectPicker,
    /// Open the harness manager.
    HarnessManager,
    /// Show the tab holding this many panes in, counting from zero.
    SelectTab(usize),
    /// Move to the next tab, wrapping.
    NextTab,
    /// Show the tab to the left, wrapping to the last.
    PreviousTab,
    /// Show the tab this client was on before this one.
    LastTab,
    /// Open the picker for a pane on a new tab, straight after this one.
    NewTab,
    /// Rename the tab on screen.
    RenameTab,
    /// Close every pane on the tab on screen, once the user says yes.
    CloseTab,
    /// Move the focused pane to the previous tab.
    MovePaneLeft,
    /// Move the focused pane to the next tab, or a new one past the last.
    MovePaneRight,
    /// Move the tab on screen one place left.
    MoveTabLeft,
    /// Move the tab on screen one place right.
    MoveTabRight,
    /// Move focus left or right, going on to the neighbouring tab at the
    /// grid's edge.
    FocusOrTab(Direction),
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
    /// Reopen the approval prompt for whatever delegation requests are queued.
    Approvals,
    /// Focus the focused pane's next child, opening it into the tiled grid.
    ///
    /// Cycles through several children one at a time — the mouse is not the
    /// only way to reach a subagent's pane.
    ExpandChild,
    /// If the focused pane is a subagent, remove it from the tiled grid and
    /// return focus to its parent.
    CollapseChild,
    /// Fold or unfold whatever the focus is in: the focused pane's subagents,
    /// or its project when the pane has none.
    ///
    /// The sidebar's twistys answer a click, and a click needs mouse reporting
    /// — which is exactly what an SSH session without it does not have.
    ToggleFold,
    /// Open the directory browser, to add a project without restarting.
    OpenProject,
    /// Open the overlay that registers a machine, to add one without
    /// restarting.
    AddMachine,
    /// Focus the next pane waiting on the user, wherever it is.
    NextAttention,
    /// Open the list of every pane waiting on the user.
    AttentionPicker,
    /// Fold the sidebar away or bring it back; in a narrow window, open or
    /// close it as a drawer over the panes.
    ToggleSidebar,
    /// Make the sidebar this many columns wider; negative narrows it.
    ResizeSidebar(i16),
    /// Quit.
    Quit,
}

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

                // After the prefix, a key typed with Ctrl still held is read
                // as the key alone when that is all that is bound: `^a ^x`
                // for `^a x`, as screen's users type it, and as the prefix
                // read keys before the keymap.
                let command = self.keymap.lookup(mode, &chord).or_else(|| {
                    (mode == KeyMode::Prefix && chord.modifiers.contains(KeyModifiers::CONTROL))
                        .then(|| Chord::new(chord.code, chord.modifiers - KeyModifiers::CONTROL))
                        .and_then(|bare| self.keymap.lookup(mode, &bare))
                });

                // An unbound key does nothing rather than reaching the pane,
                // so a stray key cannot run something in an agent, and a mode
                // stays on through it.
                let Some(command) = command else {
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

    fn handle_mouse(&mut self, event: &MouseEvent, panes: &[(PaneId, Rect)]) -> Action {
        let Some((id, rect)) = panes
            .iter()
            .find(|(_, rect)| contains(*rect, event.column, event.row))
        else {
            // The sidebar and status row are not panes. Moving across them
            // must not drop focus or send anything anywhere.
            return Action::None;
        };

        // Coordinates are relative to the pane, because that is the only frame
        // of reference the child has.
        let col = event.column - rect.x;
        let row = event.row - rect.y;
        let modifiers = modifiers_of(event.modifiers);

        let (action, button) = match event.kind {
            // Focus follows the pointer, so moving the mouse over a pane is
            // enough to type into it. Not in scroll mode: its keys read the
            // pane it was entered on, and a nudge of the mouse would hand
            // them to another.
            MouseEventKind::Moved if self.mode == KeyMode::Scroll => return Action::None,
            MouseEventKind::Moved => return Action::FocusPane(*id),
            MouseEventKind::Down(button) => (MouseAction::Press, translate_button(button)),
            MouseEventKind::Up(button) => (MouseAction::Release, translate_button(button)),
            MouseEventKind::Drag(button) => (MouseAction::Motion, translate_button(button)),
            // Scrolling is handled by Dispatch when the pane is not tracking
            // the mouse, which the caller decides; sending it as a wheel
            // button lets a pane that does track it receive it instead.
            MouseEventKind::ScrollUp => (MouseAction::Press, MouseButton::WheelUp),
            MouseEventKind::ScrollDown => (MouseAction::Press, MouseButton::WheelDown),
            MouseEventKind::ScrollLeft => (MouseAction::Press, MouseButton::WheelLeft),
            MouseEventKind::ScrollRight => (MouseAction::Press, MouseButton::WheelRight),
        };

        Action::SendMouse(
            *id,
            MouseInput {
                action,
                button,
                col,
                row,
                modifiers,
            },
        )
    }
}

/// Whether `rect` covers the cell at `(x, y)`.
fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

/// The key, for the focused pane.
fn send(event: &KeyEvent) -> Action {
    match translate(event) {
        Some((key, mods)) => Action::SendKey(key, mods),
        None => Action::None,
    }
}

/// Converts a crossterm button into the encoder's.
fn translate_button(button: MouseButton_) -> MouseButton {
    match button {
        MouseButton_::Left => MouseButton::Left,
        MouseButton_::Middle => MouseButton::Middle,
        MouseButton_::Right => MouseButton::Right,
    }
}

/// Converts crossterm modifiers into the encoder's.
fn modifiers_of(m: KeyModifiers) -> Modifiers {
    Modifiers {
        shift: m.contains(KeyModifiers::SHIFT),
        ctrl: m.contains(KeyModifiers::CONTROL),
        alt: m.contains(KeyModifiers::ALT),
        super_: m.contains(KeyModifiers::SUPER),
    }
}

/// Converts a crossterm key into the encoder's.
///
/// Returns nothing for keys with no meaning to a child, such as a bare
/// modifier press.
fn translate(event: &KeyEvent) -> Option<(Key, Modifiers)> {
    let key = match event.code {
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Esc => Key::Escape,
        KeyCode::Delete => Key::Delete,
        KeyCode::Insert => Key::Insert,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::F(n) => Key::Function(n),
        _ => return None,
    };

    let mut mods = modifiers_of(event.modifiers);

    // Shift-Tab arrives as its own code with the modifier already folded in.
    if event.code == KeyCode::BackTab {
        mods.shift = true;
    }

    Some((key, mods))
}

#[cfg(test)]
mod tests;
