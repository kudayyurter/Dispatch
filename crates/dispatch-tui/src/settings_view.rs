//! The Settings workspace: a search row, categories, fields and a footer, in
//! one box over the grid.
//!
//! The widget owns only what it shows (focus, selection, scroll, search); the
//! app owns the values and rebuilds the fields after every edit.

use std::cell::Cell;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, Widget};
use unicode_width::UnicodeWidthStr;

use crate::button::{self, Button, ButtonId};
use crate::picker::{centred, scrolled, write};
use crate::sidebar::truncate;
use crate::theme::Chrome;

/// The widest and tallest the box grows.
const MAX_SIZE: (u16, u16) = (110, 34);
/// Below this width the categories become a single row of arrows.
const COMPACT_WIDTH: u16 = 72;
/// Below this the box is only a message and the close control.
const MIN_SIZE: (u16, u16) = (40, 10);
/// The message a window too small to use Settings shows.
const TOO_SMALL: &str = "Make the window larger to use Settings · Esc closes and discards changes";

/// What a field edits, and so how its value is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Toggle,
    Choice(Vec<String>),
    Number {
        min: i64,
        max: i64,
    },
    Text,
    /// A button in the row, which presses this id.
    Action(ButtonId),
    ReadOnly,
}

impl FieldKind {
    /// Whether the value is drawn between `‹` and `›`.
    fn steps(&self) -> bool {
        matches!(self, Self::Toggle | Self::Choice(_) | Self::Number { .. })
    }
}

/// One setting, as the app has it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldView {
    pub id: &'static str,
    pub label: String,
    pub description: String,
    /// Index into the categories.
    pub category: usize,
    pub kind: FieldKind,
    pub value: String,
    /// Where the value comes from: "Built-in", "config.toml", "Settings".
    pub source: String,
    /// When a change takes effect.
    pub applies: &'static str,
    /// Edited and not yet applied.
    pub changed: bool,
    /// Changed on disk by something else since it was loaded.
    pub conflict: bool,
    /// Set in Settings, so it can be put back to what it would inherit.
    pub resettable: bool,
}

/// Which part of the workspace has the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Search,
    Categories,
    Fields,
    Buttons,
}

/// Where the workspace drew what can be clicked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsLayout {
    /// The whole box.
    pub rect: Rect,
    /// The window is too small for anything but the message and `[×]`.
    pub too_small: bool,
    /// The categories are one row of arrows instead of a column.
    pub compact: bool,
    pub close: Rect,
    pub search: Rect,
    /// Each category drawn, with its index.
    pub categories: Vec<(Rect, usize)>,
    /// Each field row drawn, with its index among the rows shown.
    pub fields: Vec<(Rect, usize)>,
    /// A `‹` (false) or `›` (true) with its row's index among the rows shown.
    /// A compact category arrow has index `usize::MAX`.
    pub steps: Vec<(Rect, usize, bool)>,
    /// The source of a field Settings set, which puts it back to what it
    /// inherits, with its row's index among the rows shown.
    pub resets: Vec<(Rect, usize)>,
    /// The footer's buttons.
    pub buttons: Vec<(Rect, ButtonId)>,
    /// The `[ … ]` of each Action row drawn, with the button it presses.
    pub actions: Vec<(Rect, ButtonId)>,
}

/// The layout and the few extra measurements drawing needs, worked out once
/// so that what is drawn is what can be clicked.
struct Plan {
    layout: SettingsLayout,
    inner: Rect,
    /// The column between categories and fields, when there is one.
    divider: Option<u16>,
    /// The rules under the search row and above the footer.
    rules: [u16; 2],
    header: Rect,
    footer: Rect,
    /// The line under the selected field.
    description: Option<Rect>,
    label_width: usize,
    source_width: usize,
    /// Indices into the fields of the rows shown.
    shown: Vec<usize>,
    buttons: Vec<Button>,
}

/// The Settings workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsView {
    categories: Vec<String>,
    fields: Vec<FieldView>,
    category: usize,
    /// Position among the rows shown.
    selected: usize,
    focus: Focus,
    /// Focus before a prompt took it, to give back.
    before_prompt: Option<Focus>,
    search: String,
    editing: bool,
    pending: Option<String>,
    prompt: Option<Vec<ButtonId>>,
    /// Position among the footer buttons while they have focus.
    button: usize,
    chrome: Chrome,
    hovered: Option<usize>,
    pressed: Option<ButtonId>,
    /// The first field row drawn. Kept between frames so a click on a row
    /// does not make the list jump from under the pointer; `layout` moves it
    /// only when the selection has left the rows shown.
    first: Cell<usize>,
    /// The first category drawn, kept the same way.
    first_category: Cell<usize>,
    /// Whether `first` follows the selection. The wheel lets go of it, and
    /// moving the selection takes it back.
    follow: Cell<bool>,
}

/// What a button says.
fn label(id: ButtonId) -> &'static str {
    match id {
        ButtonId::Open => "Open",
        ButtonId::Go => "Go",
        ButtonId::Run => "Run",
        ButtonId::Cancel => "Cancel",
        ButtonId::OpenPane => "Open pane",
        ButtonId::SaveDefault => "Save default",
        ButtonId::Approve => "Approve",
        ButtonId::Deny => "Deny",
        ButtonId::Always => "Always",
        ButtonId::Later => "Later",
        ButtonId::Ok => "OK",
        ButtonId::Close => "Close",
        ButtonId::Keep => "Keep",
        ButtonId::Apply => "Apply",
        ButtonId::Discard => "Discard",
        ButtonId::KeepEditing => "Keep editing",
        ButtonId::ClearSearch => "Clear search",
        ButtonId::ResetSidebar => "Reset sidebar",
    }
}

/// Cuts `text` to `width` columns and writes it at `x`.
fn put(
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    text: &str,
    width: usize,
    style: Style,
) -> u16 {
    write(buf, area, x, y, &truncate(text, width), style)
}

fn cols(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

impl SettingsView {
    #[must_use]
    pub fn new(categories: Vec<String>, fields: Vec<FieldView>) -> Self {
        Self {
            categories,
            fields,
            category: 0,
            selected: 0,
            focus: Focus::Categories,
            before_prompt: None,
            search: String::new(),
            editing: false,
            pending: None,
            prompt: None,
            button: 0,
            chrome: Chrome::default(),
            hovered: None,
            pressed: None,
            first: Cell::new(0),
            first_category: Cell::new(0),
            follow: Cell::new(true),
        }
    }

    pub fn set_chrome(&mut self, chrome: Chrome) {
        self.chrome = chrome;
    }

    /// Replaces the fields, which the app rebuilds after each edit, keeping
    /// the selection where it was as far as the new rows allow.
    pub fn set_fields(&mut self, fields: Vec<FieldView>) {
        self.fields = fields;
        self.selected = self.selected.min(self.shown().len().saturating_sub(1));
    }

    /// The footer's text, such as "2 changes in Appearance"; none when
    /// nothing is pending, which also takes the Apply and Discard buttons.
    pub fn set_pending(&mut self, pending: Option<String>) {
        self.pending = pending;
    }

    /// Opens the leave-with-edits prompt, whose buttons replace the footer's
    /// and take focus; closing it gives focus back.
    pub fn set_prompt(&mut self, prompt: Option<Vec<ButtonId>>) {
        match (&self.prompt, &prompt) {
            (None, Some(_)) => self.before_prompt = Some(self.focus),
            (Some(_), None) => {
                if let Some(focus) = self.before_prompt.take() {
                    self.focus = focus;
                }
            }
            _ => {}
        }
        let opened = prompt.is_some();
        self.prompt = prompt;
        if opened {
            self.focus = Focus::Buttons;
            self.button = self.default_button();
        }
    }

    /// Whether a Text field is being typed into, drawn with a caret.
    pub fn set_editing(&mut self, editing: bool) {
        self.editing = editing;
    }

    #[must_use]
    pub fn category(&self) -> usize {
        self.category
    }

    pub fn select_category(&mut self, category: usize) {
        if category < self.categories.len() {
            self.category = category;
            self.selected = 0;
            self.first.set(0);
            self.follow.set(true);
        }
    }

    /// The field with the highlight, in the current category or among the
    /// search results.
    #[must_use]
    pub fn selected_field(&self) -> Option<&FieldView> {
        self.fields.get(*self.shown().get(self.selected)?)
    }

    /// Moves the highlight to the row at `index` among the rows shown.
    pub fn select_field(&mut self, index: usize) {
        if index < self.shown().len() {
            self.selected = index;
            self.follow.set(true);
        }
    }

    /// Leaves the search for the highlighted result's own category, with the
    /// field highlighted there: what Enter or a click on a result does.
    pub fn jump_to_selected(&mut self) {
        let Some(field) = self.selected_field() else {
            return;
        };
        let (id, category) = (field.id, field.category);
        self.search.clear();
        self.select_category(category);
        let position = self
            .shown()
            .iter()
            .position(|index| self.fields[*index].id == id);
        self.select_field(position.unwrap_or(0));
    }

    #[must_use]
    pub fn focus(&self) -> Focus {
        self.focus
    }

    fn focus_on(&mut self, focus: Focus) {
        self.focus = focus;
        if focus == Focus::Buttons {
            self.button = self.default_button();
        }
    }

    /// Gives `focus` to a part at once, as a click on it does.
    pub fn set_focus(&mut self, focus: Focus) {
        self.focus_on(focus);
    }

    /// Moves among the footer buttons and round again, which is how Tab goes
    /// while a prompt holds the focus on them.
    pub fn cycle_button(&mut self, forward: bool) {
        let count = self.footer_buttons().len();
        if count == 0 {
            return;
        }
        self.button = if forward {
            (self.button + 1) % count
        } else {
            (self.button + count - 1) % count
        };
    }

    /// Search, categories, fields, buttons, and round again. The footer is
    /// passed over while it has no buttons, since focus there would show
    /// nowhere and Enter would do nothing.
    pub fn focus_next(&mut self) {
        let buttons = !self.footer_buttons().is_empty();
        self.focus_on(match self.focus {
            Focus::Search => Focus::Categories,
            Focus::Categories => Focus::Fields,
            Focus::Fields if buttons => Focus::Buttons,
            Focus::Fields | Focus::Buttons => Focus::Search,
        });
    }

    pub fn focus_previous(&mut self) {
        let buttons = !self.footer_buttons().is_empty();
        self.focus_on(match self.focus {
            Focus::Search if buttons => Focus::Buttons,
            Focus::Search | Focus::Buttons => Focus::Fields,
            Focus::Categories => Focus::Search,
            Focus::Fields => Focus::Categories,
        });
    }

    pub fn move_up(&mut self) {
        self.move_by(-1);
    }

    pub fn move_down(&mut self) {
        self.move_by(1);
    }

    fn move_by(&mut self, delta: isize) {
        match self.focus {
            Focus::Categories => {
                let next = self.category.saturating_add_signed(delta);
                self.select_category(next);
            }
            Focus::Fields => {
                let last = self.shown().len().saturating_sub(1);
                self.selected = self.selected.saturating_add_signed(delta).min(last);
                self.follow.set(true);
            }
            Focus::Search | Focus::Buttons => {}
        }
    }

    /// Moves among the footer buttons, without wrapping.
    pub fn move_button(&mut self, forward: bool) {
        let last = self.footer_buttons().len().saturating_sub(1);
        self.button = if forward {
            (self.button + 1).min(last)
        } else {
            self.button.saturating_sub(1)
        };
    }

    /// The button Enter presses, while the footer has focus.
    #[must_use]
    pub fn focused_button(&self) -> Option<ButtonId> {
        if self.focus != Focus::Buttons {
            return None;
        }
        let buttons = self.footer_buttons();
        buttons
            .get(self.button.min(buttons.len().saturating_sub(1)))
            .map(|b| b.id)
    }

    #[must_use]
    pub fn search(&self) -> &str {
        &self.search
    }

    pub fn push_search(&mut self, c: char) {
        self.search.push(c);
        self.reset_selection();
    }

    pub fn pop_search(&mut self) {
        self.search.pop();
        self.reset_selection();
    }

    pub fn clear_search(&mut self) {
        self.search.clear();
        self.reset_selection();
    }

    fn reset_selection(&mut self) {
        self.selected = 0;
        self.first.set(0);
        self.follow.set(true);
    }

    /// Scrolls the field list by `rows`, leaving the selection alone: the
    /// wheel never changes a value.
    pub fn scroll_by(&mut self, rows: i32) {
        let first = i64::try_from(self.first.get()).unwrap_or(0) + i64::from(rows);
        self.first.set(usize::try_from(first.max(0)).unwrap_or(0));
        self.follow.set(false);
    }

    pub fn set_hovered(&mut self, hovered: Option<usize>) {
        self.hovered = hovered;
    }

    pub fn set_pressed(&mut self, pressed: Option<ButtonId>) {
        self.pressed = pressed;
    }

    /// Indices of the fields shown: the search's matches, or the current
    /// category's fields.
    fn shown(&self) -> Vec<usize> {
        let needle = self.search.to_lowercase();
        self.fields
            .iter()
            .enumerate()
            .filter(|(_, field)| {
                if needle.is_empty() {
                    return field.category == self.category;
                }
                let category = self
                    .categories
                    .get(field.category)
                    .map_or("", String::as_str);
                [field.label.as_str(), field.description.as_str(), category]
                    .iter()
                    .any(|text| text.to_lowercase().contains(&needle))
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn searching(&self) -> bool {
        !self.search.is_empty()
    }

    /// The footer's buttons: the prompt's, else Clear search over an empty
    /// result, else Discard and Apply while edits are pending.
    fn footer_buttons(&self) -> Vec<Button> {
        let ids: Vec<ButtonId> = if let Some(prompt) = &self.prompt {
            prompt.clone()
        } else if self.searching() && self.shown().is_empty() {
            vec![ButtonId::ClearSearch]
        } else if self.pending.is_some() {
            vec![ButtonId::Discard, ButtonId::Apply]
        } else {
            Vec::new()
        };
        let default = ids
            .iter()
            .find(|id| **id == ButtonId::Apply)
            .or_else(|| ids.first())
            .copied();
        ids.into_iter()
            .map(|id| Button {
                id,
                label: label(id),
                default: Some(id) == default,
            })
            .collect()
    }

    /// Apply while there is something to apply, otherwise the first button.
    fn default_button(&self) -> usize {
        self.footer_buttons()
            .iter()
            .position(|button| button.default)
            .unwrap_or(0)
    }

    /// What the source column says. A field Settings set carries `↺`, the
    /// control that resets it, so the click target is visible.
    fn source_text(field: &FieldView) -> String {
        if field.conflict {
            "changed elsewhere".to_string()
        } else if field.resettable {
            format!("{} ↺", field.source)
        } else {
            field.source.clone()
        }
    }

    /// Where a row's source is drawn, and what it says, cut to its column.
    fn source_span(field: &FieldView, row: Rect, plan: &Plan) -> Option<(u16, String)> {
        if plan.source_width == 0 {
            return None;
        }
        let source = truncate(&Self::source_text(field), plan.source_width);
        let x = row.right().saturating_sub(1 + cols(source.width()));
        Some((x, source))
    }

    /// What is drawn for a field's value, and where it starts.
    fn value_span(
        &self,
        field: &FieldView,
        selected: bool,
        row: Rect,
        plan: &Plan,
    ) -> Option<(u16, String)> {
        let x = row.x + 1 + 2 + cols(plan.label_width) + 2;
        let right = row.right().saturating_sub(1 + cols(plan.source_width));
        let room = usize::from(right.saturating_sub(x + 1));
        if room == 0 {
            return None;
        }
        let text = match &field.kind {
            FieldKind::Toggle | FieldKind::Choice(_) | FieldKind::Number { .. } => {
                format!("‹ {} ›", field.value)
            }
            FieldKind::Text if self.editing && selected => format!("{}▏", field.value),
            FieldKind::Text | FieldKind::ReadOnly => field.value.clone(),
            FieldKind::Action(_) => {
                let name = if field.value.is_empty() {
                    &field.label
                } else {
                    &field.value
                };
                format!("[ {name} ]")
            }
        };
        Some((x, truncate(&text, room)))
    }

    /// Where the box and everything clickable in it fall inside `area`: the
    /// one place this is worked out, which drawing follows.
    #[must_use]
    pub fn layout(&self, area: Rect) -> SettingsLayout {
        self.plan(area).layout
    }

    #[allow(clippy::too_many_lines)]
    fn plan(&self, area: Rect) -> Plan {
        let blank = Rect::default();
        let mut plan = Plan {
            layout: SettingsLayout::default(),
            inner: blank,
            divider: None,
            rules: [0, 0],
            header: blank,
            footer: blank,
            description: None,
            label_width: 0,
            source_width: 0,
            shown: Vec::new(),
            buttons: Vec::new(),
        };
        if area.is_empty() {
            return plan;
        }

        if area.width < MIN_SIZE.0 || area.height < MIN_SIZE.1 {
            plan.layout.rect = area;
            plan.layout.too_small = true;
            if area.width >= 3 {
                plan.layout.close = Rect::new(area.right() - 3, area.y, 3, 1);
            }
            return plan;
        }

        let rect = if area.width >= 100 && area.height >= 28 {
            centred(
                area,
                MAX_SIZE.0.min(area.width - 2),
                MAX_SIZE.1.min(area.height - 2),
            )
        } else {
            area
        };
        let compact = rect.width < COMPACT_WIDTH;
        let inner = Block::default().borders(Borders::ALL).inner(rect);
        plan.layout.rect = rect;
        plan.layout.compact = compact;
        plan.layout.close = Rect::new(rect.right() - 5, rect.y, 3, 1);
        plan.layout.search = Rect::new(inner.x, inner.y, inner.width, 1);
        plan.inner = inner;

        // Search row, a rule, the body, a rule, then the footer.
        plan.rules = [inner.y + 1, inner.bottom() - 2];
        plan.footer = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
        let body = Rect::new(inner.x, inner.y + 2, inner.width, inner.height - 4);

        let mut field_area = body;
        if compact {
            let row = Rect::new(body.x, body.y, body.width, 1);
            plan.layout
                .steps
                .push((Rect::new(row.x, row.y, 3, 1), usize::MAX, false));
            plan.layout
                .steps
                .push((Rect::new(row.right() - 3, row.y, 3, 1), usize::MAX, true));
            plan.layout.categories.push((
                Rect::new(row.x + 3, row.y, row.width.saturating_sub(6), 1),
                self.category,
            ));
        } else {
            let widest = self
                .categories
                .iter()
                .map(|name| name.width())
                .max()
                .unwrap_or(0);
            let width = cols(widest + 4).min(inner.width / 3);
            let divider = body.x + width;
            plan.divider = Some(divider);
            field_area = Rect::new(divider + 1, body.y, body.right() - divider - 1, body.height);

            let rows = usize::from(body.height);
            let first = scrolled(
                self.first_category.get(),
                self.category,
                rows,
                self.categories.len(),
            );
            self.first_category.set(first);
            plan.layout.categories = (first..self.categories.len())
                .take(rows)
                .enumerate()
                .map(|(offset, index)| (Rect::new(body.x, body.y + cols(offset), width, 1), index))
                .collect();
        }
        plan.header = Rect::new(field_area.x, field_area.y, field_area.width, 1);

        plan.shown = self.shown();
        plan.buttons = self.footer_buttons();
        plan.layout.buttons = button::lay_out(&plan.buttons, plan.footer);

        let list = Rect::new(
            field_area.x,
            field_area.y + 1,
            field_area.width,
            field_area.height.saturating_sub(1),
        );
        let total = plan.shown.len();
        let width = usize::from(field_area.width);
        let rows = usize::from(list.height);
        plan.label_width = plan
            .shown
            .iter()
            .map(|index| self.fields[*index].label.width())
            .max()
            .unwrap_or(0)
            .min(width / 2);
        if !self.searching() {
            plan.source_width = plan
                .shown
                .iter()
                .map(|index| Self::source_text(&self.fields[*index]).width())
                .max()
                .unwrap_or(0)
                .min(width / 4);
        }

        // The line under the selected field takes a row of its own, so one
        // fewer field than rows is on screen while it is.
        let visible = if rows > 1 { rows - 1 } else { rows };
        let first = if self.follow.get() {
            scrolled(self.first.get(), self.selected, visible, total)
        } else {
            self.first.get().min(total.saturating_sub(visible))
        };
        self.first.set(first);

        let mut y = list.y;
        for position in first..total {
            if y >= list.bottom() {
                break;
            }
            let row = Rect::new(list.x, y, list.width, 1);
            plan.layout.fields.push((row, position));
            y += 1;
            if position == self.selected && y < list.bottom() {
                plan.description = Some(Rect::new(list.x, y, list.width, 1));
                y += 1;
            }
        }
        if !self.searching() {
            let rows: Vec<(Rect, usize)> = plan.layout.fields.clone();
            for (row, position) in rows {
                let field = &self.fields[plan.shown[position]];
                if field.resettable
                    && !field.conflict
                    && let Some((x, text)) = Self::source_span(field, row, &plan)
                {
                    plan.layout
                        .resets
                        .push((Rect::new(x, row.y, cols(text.width()), 1), position));
                }
                if let FieldKind::Action(id) = field.kind
                    && let Some((x, text)) = self.value_span(field, false, row, &plan)
                {
                    plan.layout
                        .actions
                        .push((Rect::new(x, row.y, cols(text.width()), 1), id));
                }
                if !field.kind.steps() {
                    continue;
                }
                let selected = position == self.selected;
                if let Some((x, text)) = self.value_span(field, selected, row, &plan) {
                    let end = x + cols(text.width());
                    plan.layout
                        .steps
                        .push((Rect::new(x, row.y, 2, 1), position, false));
                    plan.layout.steps.push((
                        Rect::new(end.saturating_sub(2), row.y, 2, 1),
                        position,
                        true,
                    ));
                }
            }
        }
        plan
    }
}

impl Widget for &SettingsView {
    #[allow(clippy::too_many_lines)]
    fn render(self, area: Rect, buf: &mut Buffer) {
        let plan = self.plan(area);
        let layout = &plan.layout;
        let rect = layout.rect;
        if rect.is_empty() {
            return;
        }
        let chrome = &self.chrome;

        Clear.render(rect, buf);
        if layout.too_small {
            let mut y = area.y + 1;
            let mut line = String::new();
            let room = usize::from(area.width.saturating_sub(2));
            for word in TOO_SMALL.split(' ') {
                if !line.is_empty() && line.width() + 1 + word.width() > room {
                    write(
                        buf,
                        area,
                        area.x + 1,
                        y,
                        &truncate(&line, room),
                        chrome.secondary,
                    );
                    y += 1;
                    line.clear();
                }
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
            }
            write(
                buf,
                area,
                area.x + 1,
                y,
                &truncate(&line, room),
                chrome.secondary,
            );
            if !layout.close.is_empty() {
                write(
                    buf,
                    area,
                    layout.close.x,
                    layout.close.y,
                    "[×]",
                    chrome.text,
                );
            }
            return;
        }

        Block::default()
            .borders(Borders::ALL)
            .title(" Settings ")
            .border_style(chrome.border)
            .render(rect, buf);
        write(
            buf,
            rect,
            layout.close.x,
            layout.close.y,
            "[×]",
            chrome.text,
        );

        // Search row.
        let inner = plan.inner;
        let search_width = usize::from(inner.width.saturating_sub(2));
        if self.focus == Focus::Search || self.searching() {
            let style = if self.focus == Focus::Search {
                chrome.accent
            } else {
                chrome.text
            };
            put(
                buf,
                inner,
                inner.x + 1,
                inner.y,
                &format!("> {}▏", self.search),
                search_width,
                style,
            );
        } else {
            put(
                buf,
                inner,
                inner.x + 1,
                inner.y,
                "Search settings…",
                search_width,
                chrome.secondary,
            );
        }

        // Rules, joined to the frame and to the divider.
        for (n, y) in plan.rules.iter().enumerate() {
            for x in rect.x..rect.right() {
                let symbol = if x == rect.x {
                    "├"
                } else if x == rect.right() - 1 {
                    "┤"
                } else if Some(x) == plan.divider {
                    if n == 0 { "┬" } else { "┴" }
                } else {
                    "─"
                };
                if let Some(cell) = buf.cell_mut((x, *y)) {
                    cell.set_symbol(symbol);
                    cell.set_style(chrome.border);
                }
            }
        }
        if let Some(divider) = plan.divider {
            for y in plan.rules[0] + 1..plan.rules[1] {
                if let Some(cell) = buf.cell_mut((divider, y)) {
                    cell.set_symbol("│");
                    cell.set_style(chrome.border);
                }
            }
        }

        // Categories.
        for (row, index) in &layout.categories {
            let Some(name) = self.categories.get(*index) else {
                continue;
            };
            let chosen = *index == self.category;
            if layout.compact {
                // The one row stands for the whole list, so it carries the
                // focus bar as the list's selected row would.
                let style = if self.focus == Focus::Categories {
                    chrome.selection
                } else {
                    chrome.text
                };
                fill(
                    buf,
                    *row,
                    if self.focus == Focus::Categories {
                        style
                    } else {
                        Style::default()
                    },
                );
                put(buf, *row, row.x, row.y, name, usize::from(row.width), style);
                continue;
            }
            let style = if chosen && self.focus == Focus::Categories {
                chrome.selection
            } else if chosen {
                chrome.text.add_modifier(Modifier::UNDERLINED)
            } else {
                chrome.text
            };
            fill(
                buf,
                *row,
                if chosen && self.focus == Focus::Categories {
                    style
                } else {
                    Style::default()
                },
            );
            put(
                buf,
                *row,
                row.x + 1,
                row.y,
                name,
                usize::from(row.width.saturating_sub(2)),
                style,
            );
        }
        if layout.compact {
            for (arrow, _, forward) in layout
                .steps
                .iter()
                .filter(|(_, index, _)| *index == usize::MAX)
            {
                write(
                    buf,
                    *arrow,
                    arrow.x,
                    arrow.y,
                    if *forward { "›" } else { "‹" },
                    chrome.accent,
                );
            }
        }

        // The field column's heading.
        let head_width = usize::from(plan.header.width.saturating_sub(2));
        if !layout.compact {
            let heading = if self.searching() {
                "Search results".to_string()
            } else {
                self.categories
                    .get(self.category)
                    .cloned()
                    .unwrap_or_default()
            };
            put(
                buf,
                plan.header,
                plan.header.x + 1,
                plan.header.y,
                &heading,
                head_width,
                chrome.secondary,
            );
        }

        if plan.shown.is_empty() {
            let text = if self.searching() {
                "No settings match"
            } else {
                "Nothing in this category"
            };
            let below = Rect::new(plan.header.x, plan.header.y + 1, plan.header.width, 1);
            put(
                buf,
                below,
                below.x + 1,
                below.y,
                text,
                head_width,
                chrome.secondary,
            );
        }

        // Fields.
        for (row, position) in &layout.fields {
            let field = &self.fields[plan.shown[*position]];
            let chosen = *position == self.selected;
            let barred = chosen && self.focus == Focus::Fields;
            let base = if barred {
                chrome.selection
            } else {
                Style::default()
            };
            let text = if barred {
                chrome.selection
            } else {
                chrome.text
            };
            let underlined = (chosen && !barred) || (!chosen && self.hovered == Some(*position));
            let label_style = if underlined {
                text.add_modifier(Modifier::UNDERLINED)
            } else {
                text
            };
            fill(buf, *row, base);

            let x = row.x + 1;
            if field.conflict {
                write(buf, *row, x, row.y, "!", base.fg(Color::Red));
            } else if field.changed {
                write(
                    buf,
                    *row,
                    x,
                    row.y,
                    "•",
                    if barred {
                        chrome.selection
                    } else {
                        chrome.accent
                    },
                );
            }

            if self.searching() {
                let category = self
                    .categories
                    .get(field.category)
                    .map_or("", String::as_str);
                let after = put(
                    buf,
                    *row,
                    x + 2,
                    row.y,
                    &field.label,
                    usize::from(row.width.saturating_sub(3)),
                    label_style,
                );
                let used = after.saturating_sub(row.x);
                put(
                    buf,
                    *row,
                    after + 2,
                    row.y,
                    &format!("({category})"),
                    usize::from(row.width.saturating_sub(used + 2)),
                    if barred {
                        chrome.selection
                    } else {
                        chrome.secondary
                    },
                );
                continue;
            }

            put(
                buf,
                *row,
                x + 2,
                row.y,
                &field.label,
                plan.label_width,
                label_style,
            );
            if let Some((vx, value)) = self.value_span(field, chosen, *row, &plan) {
                let style = match field.kind {
                    FieldKind::ReadOnly => base.patch(chrome.secondary),
                    // Held under the pointer, it shows pressed as a footer
                    // button does.
                    FieldKind::Action(id) => {
                        if barred || self.pressed == Some(id) {
                            chrome.selection
                        } else {
                            chrome.accent
                        }
                    }
                    _ => text,
                };
                write(buf, *row, vx, row.y, &value, style);
            }
            if let Some((sx, source)) = SettingsView::source_span(field, *row, &plan) {
                write(
                    buf,
                    *row,
                    sx,
                    row.y,
                    &source,
                    if barred {
                        chrome.selection
                    } else {
                        chrome.secondary
                    },
                );
            }
        }

        if let (Some(row), Some(field)) = (plan.description, self.selected_field()) {
            let mut text = format!("{} · {}", field.description, field.applies);
            if field.resettable {
                text.push_str(" · Backspace resets");
            }
            put(
                buf,
                row,
                row.x + 3,
                row.y,
                &text,
                usize::from(row.width.saturating_sub(4)),
                chrome.secondary,
            );
        }

        // Footer.
        let reserved = layout
            .buttons
            .first()
            .map_or(0, |(first, _)| usize::from(plan.footer.right() - first.x));
        let note = self.pending.clone().or_else(|| {
            self.prompt
                .as_ref()
                .map(|_| "Unapplied changes".to_string())
        });
        if let Some(note) = note {
            let room = usize::from(plan.footer.width.saturating_sub(2)).saturating_sub(reserved);
            put(
                buf,
                plan.footer,
                plan.footer.x + 1,
                plan.footer.y,
                &note,
                room,
                chrome.secondary,
            );
        }
        let held = self.pressed.or_else(|| self.focused_button());
        button::render(buf, &plan.buttons, &layout.buttons, chrome, held);
    }
}

/// Paints `style` behind the whole of `row`, so a highlight is a bar.
fn fill(buf: &mut Buffer, row: Rect, style: Style) {
    for x in row.x..row.right() {
        if let Some(cell) = buf.cell_mut((x, row.y)) {
            cell.set_symbol(" ");
            cell.set_style(style);
        }
    }
}

#[cfg(test)]
mod tests;
