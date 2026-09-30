//! A centred list picker.
//!
//! Used for choosing a harness, a project, or anything else that is a list of
//! named things. Selection state lives here so the caller keeps only the list.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Widget};

use crate::theme::Chrome;

/// One row of a picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Stable value returned when the row is chosen.
    pub id: String,
    /// What the user reads.
    pub label: String,
    /// Optional detail, shown dimmed after the label.
    pub detail: Option<String>,
}

impl Item {
    /// An item with no detail.
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            detail: None,
        }
    }

    /// Adds detail.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// A list with a selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    /// Heading.
    pub title: String,
    items: Vec<Item>,
    selected: usize,
    /// The dialog colours, set by whoever draws the overlay.
    chrome: Chrome,
    /// What the keys do, drawn on the bottom border.
    hint: Option<String>,
}

impl Picker {
    /// Creates a picker over `items`.
    #[must_use]
    pub fn new(title: impl Into<String>, items: Vec<Item>) -> Self {
        Self {
            title: title.into(),
            items,
            selected: 0,
            chrome: Chrome::default(),
            hint: None,
        }
    }

    /// Draws in `chrome`.
    pub fn set_chrome(&mut self, chrome: Chrome) {
        self.chrome = chrome;
    }

    /// The same picker, saying on its bottom border what its keys do.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// What the bottom border says, if anything.
    #[must_use]
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// Moves the highlight to the row whose id is `id`, if there is one.
    pub fn select(&mut self, id: &str) {
        if let Some(index) = self.items.iter().position(|item| item.id == id) {
            self.selected = index;
        }
    }

    /// The rows.
    #[must_use]
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Whether there is nothing to choose.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Index of the highlighted row.
    #[must_use]
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// The highlighted row.
    #[must_use]
    pub fn selected(&self) -> Option<&Item> {
        self.items.get(self.selected)
    }

    /// Moves the highlight down, wrapping at the end.
    ///
    /// Wrapping matters because these lists are short; walking off the bottom
    /// of a four-item list and stopping is more annoying than useful.
    pub fn next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.items.len();
    }

    /// Moves the highlight up, wrapping at the start.
    pub fn previous(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = self.selected.checked_sub(1).unwrap_or(self.items.len() - 1);
    }
}

/// Centres a box of at most `width` by `height` inside `area`.
pub(crate) fn centred(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);

    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

impl Widget for &Picker {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 4 || area.height < 3 {
            return;
        }

        let widest = self
            .items
            .iter()
            .map(|i| {
                i.label.chars().count() + i.detail.as_ref().map_or(0, |d| d.chars().count() + 3)
            })
            .max()
            .unwrap_or(0);

        let hint_width = self.hint.as_ref().map_or(0, |hint| hint.chars().count());
        let width = u16::try_from(widest.max(self.title.chars().count()).max(hint_width) + 6)
            .unwrap_or(u16::MAX)
            // The floor gives way to a narrower area, as the prompt's and the
            // browser's do: `clamp` panics when its minimum is above its maximum.
            .clamp(20.min(area.width), area.width);
        let height = u16::try_from(self.items.len() + 2)
            .unwrap_or(u16::MAX)
            .clamp(3, area.height);

        let rect = centred(area, width, height);

        // The picker floats over the grid, so whatever it covers is erased
        // rather than left showing through.
        Clear.render(rect, buf);

        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.title))
            .border_style(self.chrome.border);
        if let Some(hint) = &self.hint {
            block = block.title_bottom(
                Line::styled(format!(" {hint} "), self.chrome.border).right_aligned(),
            );
        }
        let inner = block.inner(rect);
        block.render(rect, buf);

        if self.items.is_empty() {
            write(
                buf,
                inner,
                inner.x,
                inner.y,
                "nothing to choose",
                self.chrome.secondary,
            );
            return;
        }

        // Scroll so the selection stays visible in a list taller than the box.
        let rows = inner.height as usize;
        let first = self.selected.saturating_sub(rows.saturating_sub(1));

        for (offset, item) in self.items.iter().skip(first).take(rows).enumerate() {
            let index = first + offset;
            let y = inner.y + u16::try_from(offset).unwrap_or(0);
            let chosen = index == self.selected;

            let style = if chosen {
                self.chrome.selection
            } else {
                Style::default()
            };

            // Paint the whole row so the highlight is a bar rather than just
            // behind the text.
            for x in inner.x..inner.x + inner.width {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_symbol(" ");
                    cell.set_style(style);
                }
            }

            let x = write(buf, inner, inner.x + 1, y, &item.label, style);

            if let Some(detail) = &item.detail {
                let detail_style = if chosen { style } else { self.chrome.secondary };
                write(buf, inner, x + 2, y, detail, detail_style);
            }
        }
    }
}

/// Writes `text` at `(x, y)` clipped to `area`, returning the next column.
pub(crate) fn write(buf: &mut Buffer, area: Rect, x: u16, y: u16, text: &str, style: Style) -> u16 {
    let mut cursor = x;

    for c in text.chars() {
        if cursor >= area.x + area.width || y >= area.y + area.height {
            break;
        }
        if let Some(cell) = buf.cell_mut((cursor, y)) {
            cell.set_symbol(&c.to_string());
            cell.set_style(style);
        }
        cursor += 1;
    }

    cursor
}

#[cfg(test)]
mod tests;
