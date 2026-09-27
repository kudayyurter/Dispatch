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
