//! A short list of actions, opened on something: a pane, a project, a tab.
//!
//! It is drawn at an anchor — the pointer, or a `…` — and kept inside the
//! window. Generic over what choosing an item does, so the widget knows
//! nothing of the app it serves.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Clear, Widget};

use crate::picker::write;
use crate::theme::Chrome;

/// One row of a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem<A> {
    /// What the user reads.
    pub label: String,
    /// The keys that do the same, shown dimmed on the right.
    pub keys: Option<String>,
    /// What choosing it does.
    pub action: A,
    /// Whether it can be chosen now.
    pub enabled: bool,
}

/// Where a menu was drawn, for a click to be matched against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuLayout {
    /// The whole box, frame included.
    pub rect: Rect,
    /// Each enabled item's row, with its index.
    pub items: Vec<(Rect, usize)>,
}

/// A menu with a selection.
#[derive(Debug, Clone)]
pub struct Menu<A> {
    items: Vec<MenuItem<A>>,
    anchor: (u16, u16),
    selected: usize,
    hovered: Option<usize>,
    chrome: Chrome,
}

impl<A> Menu<A> {
    /// A menu of `items` opened at `anchor`, its first enabled item selected.
    #[must_use]
    pub fn new(items: Vec<MenuItem<A>>, anchor: (u16, u16)) -> Self {
        let selected = items.iter().position(|item| item.enabled).unwrap_or(0);
        Self {
            items,
            anchor,
            selected,
            hovered: None,
            chrome: Chrome::default(),
        }
    }

    /// Draws in `chrome`.
    pub fn set_chrome(&mut self, chrome: Chrome) {
        self.chrome = chrome;
    }

    /// The selected item, when it can be chosen.
    #[must_use]
    pub fn selected(&self) -> Option<&MenuItem<A>> {
        self.items.get(self.selected).filter(|item| item.enabled)
    }

    /// Selects the item at `index`, when it can be chosen.
    pub fn select(&mut self, index: usize) {
        if self.items.get(index).is_some_and(|item| item.enabled) {
            self.selected = index;
        }
    }

    /// Marks the item under the pointer, or none. It never moves the
    /// selection: hover shows, it does not choose.
    pub fn hover(&mut self, index: Option<usize>) {
        self.hovered =
            index.filter(|index| self.items.get(*index).is_some_and(|item| item.enabled));
    }

    /// Moves to the next enabled item, wrapping.
    pub fn next(&mut self) {
        self.step(1);
    }

    /// Moves to the previous enabled item, wrapping.
    pub fn previous(&mut self) {
        self.step(self.items.len().saturating_sub(1));
    }

    fn step(&mut self, by: usize) {
        let count = self.items.len();
        if count == 0 {
            return;
        }
        let mut index = self.selected;
        for _ in 0..count {
            index = (index + by) % count;
            if self.items[index].enabled {
                self.selected = index;
                return;
            }
        }
    }

    /// The items in this menu.
    #[must_use]
    pub fn items(&self) -> &[MenuItem<A>] {
        &self.items
    }

    /// Where it draws inside `area`: at its anchor, moved left or up as far
    /// as it must to stay inside, and never larger than `area`.
    #[must_use]
    pub fn layout(&self, area: Rect) -> MenuLayout {
        let widest = self
            .items
            .iter()
            .map(|item| {
                item.label.chars().count()
                    + item
                        .keys
                        .as_ref()
                        .map_or(0, |keys| keys.chars().count() + 3)
            })
            .max()
            .unwrap_or(0);
        let width = u16::try_from(widest + 4)
            .unwrap_or(u16::MAX)
            .min(area.width);
        let height = u16::try_from(self.items.len() + 2)
            .unwrap_or(u16::MAX)
            .min(area.height);
        let x = self
            .anchor
            .0
            .clamp(area.x, (area.x + area.width).saturating_sub(width));
        let y = self
            .anchor
            .1
            .clamp(area.y, (area.y + area.height).saturating_sub(height));
        let rect = Rect::new(x, y, width, height);

        let inner = Block::default().borders(Borders::ALL).inner(rect);
        let items = self
            .items
            .iter()
            .enumerate()
            .take(usize::from(inner.height))
            .filter(|(_, item)| item.enabled)
            .map(|(index, _)| {
                (
                    Rect::new(
                        inner.x,
                        inner.y + u16::try_from(index).unwrap_or(0),
                        inner.width,
                        1,
                    ),
                    index,
                )
            })
            .collect();
        MenuLayout { rect, items }
    }
}

impl<A> Widget for &Menu<A> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout = self.layout(area);
        if layout.rect.width < 3 || layout.rect.height < 3 {
            return;
        }
        Clear.render(layout.rect, buf);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(self.chrome.border);
        let inner = block.inner(layout.rect);
        block.render(layout.rect, buf);

        for (index, item) in self
            .items
            .iter()
            .enumerate()
            .take(usize::from(inner.height))
        {
            let y = inner.y + u16::try_from(index).unwrap_or(0);
            let lit = item.enabled && (index == self.selected || self.hovered == Some(index));
            let style = if !item.enabled {
                self.chrome.secondary
            } else if lit {
                self.chrome.selection
            } else {
                self.chrome.text
            };
            for x in inner.x..inner.x + inner.width {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_symbol(" ");
                    cell.set_style(style);
                }
            }
            write(buf, inner, inner.x + 1, y, &item.label, style);
            if let Some(keys) = &item.keys {
                let keys_width = u16::try_from(keys.chars().count()).unwrap_or(0);
                let x = (inner.x + inner.width).saturating_sub(keys_width + 1);
                let keys_style = if lit { style } else { self.chrome.secondary };
                write(buf, inner, x, y, keys, keys_style);
            }
        }
    }
}

#[cfg(test)]
mod tests;
