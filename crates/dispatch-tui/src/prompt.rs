//! One line of typed input, for the questions a list cannot answer.
//!
//! A machine's ssh target and a path on another machine have nothing to pick
//! from: this client cannot see the other machine's filesystem, and the hosts
//! a user can reach live in their ssh configuration, not in Dispatch's.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear, Widget};

use crate::button::{self, Button, ButtonId, DialogLayout};
use crate::picker::{centred, write};
use crate::theme::Chrome;

/// A line under the input saying what is happening, or what went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// Work in progress, such as a machine being dialled.
    Busy(String),
    /// Why the last answer was not accepted.
    Error(String),
}

/// A question and what has been typed in answer.
#[derive(Debug, Clone)]
pub struct Prompt {
    title: String,
    hint: String,
    input: String,
    note: Option<Note>,
    /// The dialog colours, set by whoever draws the overlay.
    chrome: Chrome,
    /// What can be clicked under the hint, when the caller wants any.
    buttons: Vec<Button>,
    /// The button held down, drawn in the selection.
    pressed: Option<ButtonId>,
}

/// The narrowest a prompt is drawn, so a short title still leaves room to
/// type a path.
const MIN_WIDTH: u16 = 44;

impl Prompt {
    /// An empty prompt.
    #[must_use]
    pub fn new(title: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            hint: hint.into(),
            input: String::new(),
            note: None,
            chrome: Chrome::default(),
            buttons: Vec::new(),
            pressed: None,
        }
    }

    /// The same prompt, with `buttons` under its hint when they fit.
    #[must_use]
    pub fn with_buttons(mut self, buttons: Vec<Button>) -> Self {
        self.buttons = buttons;
        self
    }

    /// Marks the button held down, or none.
    pub fn set_pressed(&mut self, pressed: Option<ButtonId>) {
        self.pressed = pressed;
    }

    /// Draws in `chrome`.
    pub fn set_chrome(&mut self, chrome: Chrome) {
        self.chrome = chrome;
    }

    /// The same prompt with something already typed, for a default the user
    /// can accept or edit.
    #[must_use]
    pub fn with_input(mut self, input: impl Into<String>) -> Self {
        self.input = input.into();
        self
    }

    /// The question.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Everything typed, as typed.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }

    /// What was typed without surrounding whitespace, or `None` when that
    /// leaves nothing.
    ///
    /// Enter on nothing must do nothing: an empty path or an empty machine
    /// name is never what the user meant.
    #[must_use]
    pub fn answer(&self) -> Option<&str> {
        let trimmed = self.input.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    /// Types a character.
    ///
    /// Clears an error, which was about what was typed before, but not a
    /// busy note, which is about work still going on.
    pub fn push(&mut self, c: char) {
        self.input.push(c);
        if matches!(self.note, Some(Note::Error(_))) {
            self.note = None;
        }
    }

    /// Removes the last character, if there is one.
    pub fn backspace(&mut self) {
        self.input.pop();
    }

    /// The line under the input, if there is one.
    #[must_use]
    pub fn note(&self) -> Option<&Note> {
        self.note.as_ref()
    }

    /// Replaces the line under the input.
    pub fn set_note(&mut self, note: Option<Note>) {
        self.note = note;
    }
}

impl Prompt {
    /// The width it needs to be drawn whole.
    ///
    /// The note counts: it is usually the reason something was refused, and a
    /// reason cut at the box's edge loses the part that says why.
    #[must_use]
    pub fn desired_width(&self) -> u16 {
        self.width_for(true)
    }

    /// The width it needs, counting its buttons or not: a box that drops
    /// them goes back to the width it had without.
    fn width_for(&self, buttons: bool) -> u16 {
        let note = self.note.as_ref().map_or(0, |note| match note {
            Note::Busy(text) | Note::Error(text) => text.chars().count(),
        });
        let widest = [
            self.title.chars().count(),
            self.hint.chars().count(),
            self.input.chars().count(),
            note,
            if buttons {
                usize::from(button::total_width(&self.buttons))
            } else {
                0
            },
        ]
        .into_iter()
        .max()
        .unwrap_or(0);
        u16::try_from(widest + 6).unwrap_or(u16::MAX).max(MIN_WIDTH)
    }
}

impl Prompt {
    /// The note as it is drawn in a box `width` wide, with its colour: one
    /// line, or two when it is wider than the box.
    fn note_lines(&self, width: u16) -> Option<((&str, Option<&str>), Color)> {
        let (text, colour) = match self.note.as_ref()? {
            Note::Busy(text) => (text.as_str(), Color::Yellow),
            Note::Error(text) => (text.as_str(), Color::Red),
        };
        Some((wrap(text, usize::from(width.saturating_sub(2))), colour))
    }

    /// Where the box and what can be clicked in it fall inside `area`.
    ///
    /// The one place this is worked out: drawing follows it, so what is
    /// clicked is what is seen. Empty when `area` is too small to draw in.
    #[must_use]
    pub fn layout(&self, area: Rect) -> DialogLayout {
        if area.width < 8 || area.height < 5 {
            return DialogLayout::default();
        }

        // Border, input, hint, note — and the note's second line when it
        // needs one, for as many rows as the area has. A note still wider
        // than the box, because the area is, takes a second line rather than
        // being cut.
        let shape = |buttons: bool| {
            let width = self
                .width_for(buttons)
                .clamp(MIN_WIDTH.min(area.width), area.width);
            let wraps = self
                .note_lines(width)
                .is_some_and(|((_, second), _)| second.is_some());
            (width, if wraps { 6 } else { 5 })
        };

        // The buttons are one row more, kept only when the area has it and
        // they all fit; otherwise the box keeps its old shape.
        let (width, height) = shape(true);
        let rect = centred(area, width, height + 1);
        if rect.height == height + 1 {
            let inner = Block::default().borders(Borders::ALL).inner(rect);
            let row = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
            let buttons = button::lay_out(&self.buttons, row);
            if !buttons.is_empty() {
                return DialogLayout {
                    rect,
                    buttons,
                    ..DialogLayout::default()
                };
            }
        }

        let (width, height) = shape(false);
        DialogLayout {
            rect: centred(area, width, height),
            ..DialogLayout::default()
        }
    }
}

impl Widget for &Prompt {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout = self.layout(area);
        let rect = layout.rect;
        if rect.is_empty() {
            return;
        }

        // Floats over the grid, so whatever it covers is erased rather than
        // left showing through.
        Clear.render(rect, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.title))
            .border_style(self.chrome.border);
        let inner = block.inner(rect);
        block.render(rect, buf);

        // The end of a long input rather than its start: the end is where the
        // user is typing.
        let room = usize::from(inner.width.saturating_sub(3));
        let count = self.input.chars().count();
        let shown: String = self
            .input
            .chars()
            .skip(count.saturating_sub(room))
            .collect();
        let x = write(buf, inner, inner.x, inner.y, "> ", self.chrome.accent);
        let x = write(buf, inner, x, inner.y, &shown, Style::default());
        write(buf, inner, x, inner.y, "▏", self.chrome.accent);

        write(
            buf,
            inner,
            inner.x,
            inner.y + 1,
            &self.hint,
            self.chrome.secondary,
        );

        if let Some(((first, second), colour)) = self.note_lines(rect.width) {
            let style = Style::default().fg(colour);
            write(buf, inner, inner.x, inner.y + 2, first, style);
            if let Some(second) = second {
                write(buf, inner, inner.x, inner.y + 3, second, style);
            }
        }

        button::render(
            buf,
            &self.buttons,
            &layout.buttons,
            &self.chrome,
            self.pressed,
        );
    }
}

/// Splits `text` into what fits in `width` columns and the rest, if any.
///
/// At the last space that fits, so a word is not broken across the two
/// lines; mid-word only when a single word is wider than the line. The rest
/// is not split again: two lines are as much as a one-line prompt spares, and
/// `write` cuts whatever is left at the box's edge.
fn wrap(text: &str, width: usize) -> (&str, Option<&str>) {
    if text.chars().count() <= width {
        return (text, None);
    }

    // The first character that does not fit, and where it starts. There is
    // one: the text is longer than the line.
    let Some((cut, next)) = text.char_indices().nth(width) else {
        return (text, None);
    };

    // A space right at the edge fits the whole of the word before it.
    let space = if next == ' ' {
        Some(cut)
    } else {
        text[..cut].rfind(' ')
    };

    match space {
        Some(space) if space > 0 => (&text[..space], Some(&text[space + 1..])),
        _ => (&text[..cut], Some(&text[cut..])),
    }
}

#[cfg(test)]
mod tests;
