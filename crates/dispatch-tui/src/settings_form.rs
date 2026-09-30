//! The settings popup: a harness's settings, one row each, stepped through
//! their values before a pane is opened with them or they are saved as the
//! harness's defaults.
//!
//! It owns its keys, so whoever holds it acts only on what a key asks for:
//! open, save, or go back.

use dispatch_config::{
    Allowed, ChoiceOption, Choices, MAX_VALUE_CHARS, SAFE_CHARACTERS, SettingDef, SettingKind,
    allowed, fit_limits, is_safe_char,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Widget};

use crate::button::{self, Button, ButtonId, DialogLayout};
use crate::input::{KeyCode, KeyEvent, KeyModifiers};
use crate::picker::{centred, write};
use crate::theme::Chrome;

/// What a key in the popup asks of whoever holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormAction {
    /// Nothing beyond what the popup did to itself.
    None,
    /// Open a pane with [`SettingsForm::values`], saving nothing.
    Open,
    /// Save [`SettingsForm::values`] as the harness's defaults.
    Save,
    /// Back to the picker, with nothing saved.
    Back,
    /// A key was refused; the reason is for the status row.
    Refused(String),
}

/// What is shown for a value that leaves the choice to the agent.
pub const AGENT_DEFAULT: &str = "agent default";

/// What is shown for a setting the chosen value of another leaves nothing
/// to choose from.
pub const NOT_AVAILABLE: &str = "not available";

/// What the keys do.
const HINT: &str = "Enter open · s save as default · Esc back";

/// What the keys do while a value is being typed.
const EDITING_HINT: &str = "Enter confirm · Esc cancel";

/// How many characters of a typed value the box keeps room for, so typing
/// does not resize it.
const TYPED_WIDTH: usize = 24;

/// What a row holds.
#[derive(Debug, Clone)]
enum Shape {
    /// On or off.
    Flag,
    /// One of a list, and maybe a typed value after it.
    Choice {
        options: Vec<ChoiceOption>,
        custom: bool,
    },
    /// Typed.
    Text,
}

/// Where a value sits among its row's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// Agent default.
    Default,
    /// One of the options, by index.
    Option(usize),
    /// Typed.
    Typed,
}

/// One setting.
#[derive(Debug, Clone)]
struct Row {
    key: String,
    label: String,
    shape: Shape,
    /// `""` for agent default; `"true"` or `"false"` for a flag.
    value: String,
    /// The last value typed on this row, offered again when the row steps
    /// back onto its typed slot.
    typed: String,
    /// Whether another setting limits this one.
    limited: bool,
    /// What it may take while the others hold their values.
    allowed: Allowed,
}

impl Row {
    fn new(setting: &SettingDef, value: String) -> Row {
        let shape = match &setting.kind {
            SettingKind::Bool { .. } => Shape::Flag,
            SettingKind::Choice { options, .. } => Shape::Choice {
                options: options.clone(),
                custom: setting.custom,
            },
            SettingKind::Text { .. } => Shape::Text,
        };
        let mut row = Row {
            key: setting.key.clone(),
            label: setting.label.clone(),
            shape,
            value,
            typed: String::new(),
            limited: setting.limited_by.is_some(),
            allowed: Allowed::Free,
        };
        if row.slot() == Slot::Typed {
            row.typed = row.value.clone();
        }
        row
    }

    /// The row's values, in the order `←` and `→` step through them.
    fn slots(&self) -> Vec<Slot> {
        match &self.shape {
            Shape::Flag => Vec::new(),
            Shape::Choice { options, custom } => match &self.allowed {
                Allowed::Unavailable => Vec::new(),
                Allowed::Only(only) => options
                    .iter()
                    .enumerate()
                    .filter(|(_, option)| only.contains(&option.value))
                    .map(|(index, _)| Slot::Option(index))
                    .collect(),
                Allowed::Free => {
                    let mut slots = vec![Slot::Default];
                    slots.extend((0..options.len()).map(Slot::Option));
                    if *custom {
                        slots.push(Slot::Typed);
                    }
                    slots
                }
            },
            Shape::Text => vec![Slot::Default, Slot::Typed],
        }
    }

    /// Where the value sits.
    fn slot(&self) -> Slot {
        match &self.shape {
            Shape::Flag => Slot::Default,
            _ if self.value.is_empty() => Slot::Default,
            Shape::Choice { options, .. } => options
                .iter()
                .position(|option| option.value == self.value)
                .map_or(Slot::Typed, Slot::Option),
            Shape::Text => Slot::Typed,
        }
    }

    /// The value as the popup shows it: an option by its label.
    fn shown(&self) -> String {
        match &self.shape {
            Shape::Flag if self.value == "true" => "on".to_string(),
            Shape::Flag => "off".to_string(),
            _ if self.allowed == Allowed::Unavailable => NOT_AVAILABLE.to_string(),
            _ if self.value.is_empty() => AGENT_DEFAULT.to_string(),
            Shape::Choice { options, .. } => options
                .iter()
                .find(|option| option.value == self.value)
                .map_or_else(
                    || format!("Custom: {}", self.value),
                    |option| option.label().to_string(),
                ),
            Shape::Text => self.value.clone(),
        }
    }

    /// The widest this row's value can be drawn, so stepping through its
    /// values does not resize the box.
    fn widest(&self) -> usize {
        let typed = "Custom: ".len() + TYPED_WIDTH;
        let longest = match &self.shape {
            Shape::Flag => "off".len(),
            Shape::Choice { options, custom } => {
                let option = options
                    .iter()
                    .map(|option| option.label().chars().count())
                    .max()
                    .unwrap_or(0);
                let unavailable = if self.limited { NOT_AVAILABLE.len() } else { 0 };
                option
                    .max(AGENT_DEFAULT.len())
                    .max(unavailable)
                    .max(if *custom { typed } else { 0 })
            }
            Shape::Text => AGENT_DEFAULT.len().max(typed),
        };
        // A value already typed can be longer than the room kept for one;
        // one more for the cursor.
        longest.max(self.shown().chars().count() + 1)
    }
}

/// A value being typed on the selected row.
#[derive(Debug, Clone)]
struct Edit {
    /// The row's value before typing began, for `Esc`.
    before: String,
    text: String,
}

/// A harness's settings, one row each.
#[derive(Debug, Clone)]
pub struct SettingsForm {
    title: String,
    /// The settings the rows show, for working out what limits each one.
    settings: Vec<SettingDef>,
    rows: Vec<Row>,
    selected: usize,
    editing: Option<Edit>,
    /// The dialog colours, set by whoever draws the overlay.
    chrome: Chrome,
    /// What can be clicked along the foot, when the caller wants any.
    buttons: Vec<Button>,
    /// The button held down, drawn in the selection.
    pressed: Option<ButtonId>,
    /// The row the pointer is over.
    hovered: Option<usize>,
}

impl SettingsForm {
    /// A popup titled `title` over `settings`, each showing its value from
    /// `values`, or its file's default where `values` has none.
    #[must_use]
    pub fn new(title: impl Into<String>, settings: &[SettingDef], values: &Choices) -> Self {
        let rows = settings
            .iter()
            .map(|setting| {
                let value = values
                    .get(&setting.key)
                    .cloned()
                    .unwrap_or_else(|| setting.file_default());
                Row::new(setting, value)
            })
            .collect();

        let mut form = Self {
            title: title.into(),
            settings: settings.to_vec(),
            rows,
            selected: 0,
            editing: None,
            chrome: Chrome::default(),
            buttons: Vec::new(),
            pressed: None,
            hovered: None,
        };
        form.refit();
        if form
            .rows
            .first()
            .is_some_and(|row| row.allowed == Allowed::Unavailable)
        {
            form.move_row(true);
        }
        form
    }

    /// Draws in `chrome`.
    pub fn set_chrome(&mut self, chrome: Chrome) {
        self.chrome = chrome;
    }

    /// The same popup, with `buttons` along its foot when they fit.
    #[must_use]
    pub fn with_buttons(mut self, buttons: Vec<Button>) -> Self {
        self.buttons = buttons;
        self
    }

    /// Marks the button held down, or none.
    pub fn set_pressed(&mut self, pressed: Option<ButtonId>) {
        self.pressed = pressed;
    }

    /// Marks the row the pointer is over.
    pub fn set_hovered(&mut self, hovered: Option<usize>) {
        self.hovered = hovered;
    }

    /// Moves to row `index`, if it has something to choose: what a click on
    /// a row does. A value being typed on another row is kept, as moving
    /// with the arrows keeps it.
    pub fn select_row(&mut self, index: usize) {
        let available = self
            .rows
            .get(index)
            .is_some_and(|row| row.allowed != Allowed::Unavailable);
        if available && index != self.selected {
            self.confirm();
            self.selected = index;
        }
    }

    /// Steps row `index` to its next or previous value: what a click on its
    /// `◂` or `▸` does, through the code `←` and `→` run. A row with nothing
    /// to choose is left alone.
    pub fn step_row(&mut self, index: usize, forward: bool) -> FormAction {
        self.select_row(index);
        if self.selected == index {
            self.step_value(forward);
        }
        FormAction::None
    }

    /// Every row's value, by key: what opening or saving uses.
    #[must_use]
    pub fn values(&self) -> Choices {
        self.rows
            .iter()
            .map(|row| (row.key.clone(), row.value.clone()))
            .collect()
    }

    /// Whether a value is being typed.
    #[must_use]
    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    /// Index of the selected row.
    #[must_use]
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Acts on one key.
    pub fn key(&mut self, key: &KeyEvent) -> FormAction {
        if self.editing.is_some() {
            return self.edit_key(key);
        }
        // A chord is nobody's here: Ctrl s must not save by accident.
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return FormAction::None;
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.previous_row(),
            KeyCode::Down | KeyCode::Char('j') => self.next_row(),
            KeyCode::Left | KeyCode::Char('h') => self.step_value(false),
            KeyCode::Right | KeyCode::Char('l') => self.step_value(true),
            KeyCode::Char(' ') => self.toggle(),
            KeyCode::Enter => return FormAction::Open,
            KeyCode::Char('s') => return FormAction::Save,
            KeyCode::Esc => return FormAction::Back,
            _ => {}
        }
        FormAction::None
    }

    /// Types `text` into a value being typed, dropping line breaks: `Enter`
    /// is a decision, and the newline a copied name often ends with must not
    /// make it. What a value cannot hold is left out, and said.
    pub fn paste(&mut self, text: &str) -> FormAction {
        if self.editing.is_none() {
            return FormAction::None;
        }
        let mut outcome = FormAction::None;
        for c in text.chars().filter(|c| !matches!(c, '\r' | '\n')) {
            if let FormAction::Refused(why) = self.type_char(c) {
                outcome = FormAction::Refused(why);
            }
        }
        outcome
    }

    /// Acts on one key while a value is being typed: every printable key is
    /// text, and the arrows still move.
    fn edit_key(&mut self, key: &KeyEvent) -> FormAction {
        match key.code {
            KeyCode::Enter => self.confirm(),
            KeyCode::Esc => self.cancel(),
            KeyCode::Up => {
                self.confirm();
                self.previous_row();
            }
            KeyCode::Down => {
                self.confirm();
                self.next_row();
            }
            // The text is dropped: the arrows never trap the user on the
            // typed slot.
            KeyCode::Left => self.step_value(false),
            KeyCode::Right => self.step_value(true),
            KeyCode::Backspace => {
                if let Some(edit) = &mut self.editing {
                    edit.text.pop();
                }
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                return self.type_char(c);
            }
            _ => {}
        }
        FormAction::None
    }

    /// Types `c`, if a value may hold it there.
    fn type_char(&mut self, c: char) -> FormAction {
        let Some(edit) = &mut self.editing else {
            return FormAction::None;
        };
        let fits = edit.text.chars().count() < MAX_VALUE_CHARS;
        let leading_dash = edit.text.is_empty() && c == '-';

        if is_safe_char(c) && fits && !leading_dash {
            edit.text.push(c);
            FormAction::None
        } else {
            FormAction::Refused(format!(
                "a value takes only {SAFE_CHARACTERS}, up to {MAX_VALUE_CHARS}"
            ))
        }
    }

    fn previous_row(&mut self) {
        self.move_row(false);
    }

    fn next_row(&mut self) {
        self.move_row(true);
    }

    /// Moves to the next or previous row that has something to choose,
    /// wrapping.
    fn move_row(&mut self, forward: bool) {
        let count = self.rows.len();
        for _ in 0..count {
            self.selected = if forward {
                (self.selected + 1) % count
            } else {
                self.selected.checked_sub(1).unwrap_or(count - 1)
            };
            if self.rows[self.selected].allowed != Allowed::Unavailable {
                return;
            }
        }
    }

    /// Brings every limited row within what the others now allow, and
    /// notes what each may take.
    fn refit(&mut self) {
        let mut values = self.values();
        fit_limits(&self.settings, &mut values);
        for row in &mut self.rows {
            row.allowed = allowed(&self.settings, &row.key, &values);
            if let Some(value) = values.get(&row.key) {
                row.value.clone_from(value);
            }
        }
    }

    /// Flips the selected row, if it is on or off.
    fn toggle(&mut self) {
        if let Some(row) = self.rows.get_mut(self.selected)
            && matches!(row.shape, Shape::Flag)
        {
            row.value = if row.value == "true" { "false" } else { "true" }.to_string();
        }
    }

    /// Steps the selected row to its next or previous value, wrapping.
    ///
    /// Stepping onto the typed slot starts typing there; stepping while
    /// typing drops the text and moves to the typed slot's neighbour.
    fn step_value(&mut self, forward: bool) {
        let typing = self.editing.take().is_some();
        let Some(row) = self.rows.get_mut(self.selected) else {
            return;
        };
        if matches!(row.shape, Shape::Flag) {
            row.value = if row.value == "true" { "false" } else { "true" }.to_string();
            return;
        }

        let slots = row.slots();
        if slots.is_empty() {
            return;
        }
        let here = if typing {
            slots.len() - 1
        } else {
            slots
                .iter()
                .position(|slot| *slot == row.slot())
                .unwrap_or(0)
        };
        let next = if forward {
            (here + 1) % slots.len()
        } else {
            (here + slots.len() - 1) % slots.len()
        };

        match slots[next] {
            Slot::Default => row.value.clear(),
            Slot::Option(index) => {
                if let Shape::Choice { options, .. } = &row.shape {
                    row.value = options[index].value.clone();
                }
            }
            Slot::Typed => {
                self.editing = Some(Edit {
                    before: row.value.clone(),
                    text: row.typed.clone(),
                });
            }
        }
        self.refit();
    }

    /// Stops typing and keeps the text: nothing typed is agent default.
    fn confirm(&mut self) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        if let Some(row) = self.rows.get_mut(self.selected) {
            row.typed = edit.text.clone();
            row.value = edit.text;
        }
        self.refit();
    }

    /// Stops typing and puts back the value from before it began.
    fn cancel(&mut self) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        if let Some(row) = self.rows.get_mut(self.selected) {
            row.value = edit.before;
        }
        self.refit();
    }

    /// The width of what a value being typed shows right now (its prefix,
    /// the text and the cursor), so the box can grow to fit it instead of
    /// cutting it off at [`TYPED_WIDTH`]. Stepping alone leaves this at
    /// zero, so it never resizes the box.
    fn editing_width(&self) -> usize {
        let Some(edit) = &self.editing else {
            return 0;
        };
        let Some(row) = self.rows.get(self.selected) else {
            return 0;
        };
        let prefix = match row.shape {
            Shape::Text => 0,
            _ => "Custom: ".len(),
        };
        // One more for the cursor drawn after the text.
        prefix + edit.text.chars().count() + 1
    }
}

impl SettingsForm {
    /// The width it needs to be drawn whole.
    #[must_use]
    pub fn desired_width(&self) -> u16 {
        self.width_for(true)
    }

    /// The width it needs, counting its buttons or not: a box that drops
    /// them goes back to the width it had without.
    fn width_for(&self, buttons: bool) -> u16 {
        let label_width = self
            .rows
            .iter()
            .map(|row| row.label.chars().count())
            .max()
            .unwrap_or(0);
        let value_width = self
            .rows
            .iter()
            .map(Row::widest)
            .max()
            .unwrap_or(0)
            .max(self.editing_width());
        let hint = if self.editing.is_some() {
            EDITING_HINT
        } else {
            HINT
        };

        // A space either side, two between label and value, and the arrows
        // around the value.
        let row_width = 1 + label_width + 2 + 2 + value_width + 2 + 1;
        let widest = row_width
            .max(self.title.chars().count() + 4)
            .max(hint.chars().count() + 4)
            .max(if buttons {
                usize::from(button::total_width(&self.buttons))
            } else {
                0
            });
        u16::try_from(widest + 2).unwrap_or(u16::MAX)
    }
}

impl SettingsForm {
    /// What a row shows as its value, with a value being typed and its
    /// cursor where it is typed.
    fn value_text(&self, index: usize, row: &Row) -> String {
        match (&self.editing, index == self.selected, &row.shape) {
            (Some(edit), true, Shape::Text) => format!("{}▏", edit.text),
            (Some(edit), true, _) => format!("Custom: {}▏", edit.text),
            _ => row.shown(),
        }
    }

    /// Where the box and what can be clicked in it fall inside `area`.
    ///
    /// The one place this is worked out: drawing follows it, so what is
    /// clicked is what is seen. Empty when `area` is too small to draw in.
    #[must_use]
    pub fn layout(&self, area: Rect) -> DialogLayout {
        if area.width < 8 || area.height < 3 {
            return DialogLayout::default();
        }

        let label_width = self
            .rows
            .iter()
            .map(|row| row.label.chars().count())
            .max()
            .unwrap_or(0);
        let width = |buttons| self.width_for(buttons).min(area.width);
        let height = |extra: usize| {
            u16::try_from(self.rows.len() + 2 + extra)
                .unwrap_or(u16::MAX)
                .min(area.height)
        };

        // The buttons are one row more than the rows need, kept only when
        // the area has it and they all fit; otherwise the box keeps its old
        // shape and its hint.
        let frame = Block::default().borders(Borders::ALL);
        let grown = centred(area, width(true), height(1));
        let foot = frame.inner(grown);
        let (rect, buttons) = if usize::from(foot.height) > self.rows.len() {
            let row = Rect::new(foot.x, foot.y + foot.height - 1, foot.width, 1);
            let placed = button::lay_out(&self.buttons, row);
            if placed.is_empty() {
                (centred(area, width(false), height(0)), placed)
            } else {
                (grown, placed)
            }
        } else {
            (centred(area, width(false), height(0)), Vec::new())
        };
        let inner = frame.inner(rect);

        let value_x = inner
            .x
            .saturating_add(1)
            .saturating_add(u16::try_from(label_width + 2).unwrap_or(u16::MAX));

        let mut rows = Vec::new();
        let mut steps = Vec::new();
        for (index, row) in self.rows.iter().enumerate().take(usize::from(inner.height)) {
            let y = inner.y + u16::try_from(index).unwrap_or(0);
            rows.push((Rect::new(inner.x, y, inner.width, 1), index));
            if row.allowed == Allowed::Unavailable {
                continue;
            }

            // `◂ ` before the value and ` ▸` after it, each two cells, cut
            // to the box as drawing cuts them.
            let after = value_x.saturating_add(2).saturating_add(
                u16::try_from(self.value_text(index, row).chars().count()).unwrap_or(u16::MAX),
            );
            for (x, forward) in [(value_x, false), (after, true)] {
                let cell = Rect::new(x, y, 2, 1).intersection(inner);
                if !cell.is_empty() {
                    steps.push((cell, index, forward));
                }
            }
        }

        DialogLayout {
            rect,
            rows,
            steps,
            buttons,
        }
    }
}

impl Widget for &SettingsForm {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout = self.layout(area);
        let rect = layout.rect;
        if rect.is_empty() {
            return;
        }

        let label_width = self
            .rows
            .iter()
            .map(|row| row.label.chars().count())
            .max()
            .unwrap_or(0);
        let hint = if self.editing.is_some() {
            EDITING_HINT
        } else {
            HINT
        };

        // It floats over the grid, so whatever it covers is erased rather
        // than left showing through.
        Clear.render(rect, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.title))
            .title_bottom(Line::styled(format!(" {hint} "), self.chrome.secondary).right_aligned())
            .border_style(self.chrome.border);
        let inner = block.inner(rect);
        block.render(rect, buf);

        let value_x = inner
            .x
            .saturating_add(1)
            .saturating_add(u16::try_from(label_width + 2).unwrap_or(u16::MAX));

        for (line, index) in &layout.rows {
            let (y, index) = (line.y, *index);
            let row = &self.rows[index];
            let chosen = index == self.selected;
            let base = if chosen {
                self.chrome.selection
            } else {
                Style::default()
            };
            // Underlined rather than barred, so what the pointer is over
            // never reads as what is chosen.
            let label = if !chosen && self.hovered == Some(index) {
                button::hover(&self.chrome)
            } else {
                base
            };

            // The whole row, so the highlight is a bar rather than just
            // behind the text.
            if chosen {
                for x in inner.x..inner.x + inner.width {
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_symbol(" ");
                        cell.set_style(base);
                    }
                }
            }

            if row.allowed == Allowed::Unavailable {
                let faded = base.patch(self.chrome.secondary);
                write(buf, inner, inner.x + 1, y, &row.label, faded);
                write(buf, inner, value_x + 2, y, NOT_AVAILABLE, faded);
                continue;
            }
            write(buf, inner, inner.x + 1, y, &row.label, label);

            let shown = self.value_text(index, row);
            let arrows = base.patch(self.chrome.secondary);
            let x = write(buf, inner, value_x, y, "◂ ", arrows);
            let x = write(buf, inner, x, y, &shown, base);
            write(buf, inner, x, y, " ▸", arrows);
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

#[cfg(test)]
mod tests;
