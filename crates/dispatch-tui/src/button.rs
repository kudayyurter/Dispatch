//! A row of buttons at the foot of a dialog, each a padded label a click
//! anywhere on lands on.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::picker::write;
use crate::theme::Chrome;

/// Which button, for the dialog's owner to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonId {
    Open,
    /// Go to the chosen pane; pressed exactly as `Open` is.
    Go,
    Run,
    Cancel,
    OpenPane,
    SaveDefault,
    Approve,
    Deny,
    Always,
    Later,
    Ok,
    Close,
}

/// One button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Button {
    /// What pressing it means.
    pub id: ButtonId,
    /// What it says.
    pub label: &'static str,
    /// Whether it is what Enter does, drawn in the accent.
    pub default: bool,
}

/// Where a dialog drew what can be clicked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogLayout {
    /// The whole box.
    pub rect: Rect,
    /// Each list row drawn, with its index among the rows shown.
    pub rows: Vec<(Rect, usize)>,
    /// A settings row's `‹` (false) or `›` (true), with the row's index.
    pub steps: Vec<(Rect, usize, bool)>,
    /// Each button drawn.
    pub buttons: Vec<(Rect, ButtonId)>,
}

/// How wide a button is drawn: its label with a blank and a bracket either side.
fn width(button: &Button) -> u16 {
    u16::try_from(button.label.chars().count() + 4).unwrap_or(u16::MAX)
}

/// How much room a set of buttons takes: each one's width, a blank after
/// every one of them, which puts one between each pair and one before the
/// edge.
#[must_use]
pub fn total_width(buttons: &[Button]) -> u16 {
    buttons
        .iter()
        .map(|button| width(button).saturating_add(1))
        .fold(0, u16::saturating_add)
}

/// Places `buttons` at the right end of `row`, one blank apart and one
/// from the edge; none at all when they do not all fit, since half a set of
/// choices is worse than the keys alone.
#[must_use]
pub fn lay_out(buttons: &[Button], row: Rect) -> Vec<(Rect, ButtonId)> {
    let total = total_width(buttons);
    if buttons.is_empty() || row.height == 0 || total > row.width {
        return Vec::new();
    }
    let mut x = row.x + row.width - total;
    buttons
        .iter()
        .map(|button| {
            let rect = Rect::new(x, row.y, width(button), 1);
            x += width(button) + 1;
            (rect, button.id)
        })
        .collect()
}

/// How a row the pointer is over is drawn: underlined text, which reads as
/// "this is live" without the bar that means "this is chosen".
#[must_use]
pub(crate) fn hover(chrome: &Chrome) -> Style {
    chrome.text.add_modifier(Modifier::UNDERLINED)
}

/// Draws the placed buttons: the default in the accent, a held one in the
/// selection, the rest in the dialog's text.
pub fn render(
    buf: &mut Buffer,
    buttons: &[Button],
    placed: &[(Rect, ButtonId)],
    chrome: &Chrome,
    pressed: Option<ButtonId>,
) {
    for (rect, id) in placed {
        let Some(button) = buttons.iter().find(|button| button.id == *id) else {
            continue;
        };
        let style = if pressed == Some(*id) {
            chrome.selection
        } else if button.default {
            chrome.accent
        } else {
            chrome.text
        };
        write(
            buf,
            *rect,
            rect.x,
            rect.y,
            &format!("[ {} ]", button.label),
            style,
        );
    }
}

#[cfg(test)]
mod tests;
