//! The settings popup: a harness's settings, one row each, stepped through
//! their values before a pane is opened with them or they are saved as the
//! harness's defaults.
//!
//! It owns its keys, so whoever holds it acts only on what a key asks for:
//! open, save, or go back.

use dispatch_config::{
    Choices, MAX_VALUE_CHARS, SAFE_CHARACTERS, SettingDef, SettingKind, is_safe_char,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Widget};

use crate::input::{KeyCode, KeyEvent, KeyModifiers};
use crate::picker::{centred, write};

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
    Choice { options: Vec<String>, custom: bool },
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
            Shape::Choice { options, custom } => {
                let mut slots = vec![Slot::Default];
                slots.extend((0..options.len()).map(Slot::Option));
                if *custom {
                    slots.push(Slot::Typed);
                }
                slots
            }
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
                .position(|option| *option == self.value)
                .map_or(Slot::Typed, Slot::Option),
            Shape::Text => Slot::Typed,
        }
    }

    /// The value as the popup shows it.
    fn shown(&self) -> String {
        match &self.shape {
            Shape::Flag if self.value == "true" => "on".to_string(),
            Shape::Flag => "off".to_string(),
            _ if self.value.is_empty() => AGENT_DEFAULT.to_string(),
            Shape::Choice { .. } if self.slot() == Slot::Typed => {
                format!("Custom: {}", self.value)
            }
            Shape::Choice { .. } | Shape::Text => self.value.clone(),
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
                    .map(|option| option.chars().count())
                    .max()
                    .unwrap_or(0);
                option
                    .max(AGENT_DEFAULT.len())
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
    rows: Vec<Row>,
    selected: usize,
    editing: Option<Edit>,
    /// The frame's colour, set by whoever draws it so it matches the rest
    /// of the interface.
    border: Style,
    /// The selected row.
    highlight: Style,
    /// The arrows and the hint.
    faded: Style,
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

        Self {
            title: title.into(),
            rows,
            selected: 0,
            editing: None,
            border: Style::default().fg(Color::Cyan),
            highlight: Style::default().bg(Color::DarkGray),
            faded: Style::default().fg(Color::DarkGray),
        }
    }

    /// Draws the frame in `style`.
    pub fn set_border(&mut self, style: Style) {
        self.border = style;
    }

    /// Draws the selected row in `highlight`, and the arrows and hint in
    /// `faded`.
    pub fn set_styles(&mut self, highlight: Style, faded: Style) {
        self.highlight = highlight;
        self.faded = faded;
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
            KeyCode::Left | KeyCode::Char('h') => self.step(false),
            KeyCode::Right | KeyCode::Char('l') => self.step(true),
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
            KeyCode::Left => self.step(false),
            KeyCode::Right => self.step(true),
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
        if !self.rows.is_empty() {
            self.selected = self.selected.checked_sub(1).unwrap_or(self.rows.len() - 1);
        }
    }

    fn next_row(&mut self) {
        if !self.rows.is_empty() {
            self.selected = (self.selected + 1) % self.rows.len();
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
    fn step(&mut self, forward: bool) {
        let typing = self.editing.take().is_some();
        let Some(row) = self.rows.get_mut(self.selected) else {
            return;
        };
        if matches!(row.shape, Shape::Flag) {
            row.value = if row.value == "true" { "false" } else { "true" }.to_string();
            return;
        }

        let slots = row.slots();
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
                    row.value = options[index].clone();
                }
            }
            Slot::Typed => {
                self.editing = Some(Edit {
                    before: row.value.clone(),
                    text: row.typed.clone(),
                });
            }
        }
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
    }

    /// Stops typing and puts back the value from before it began.
    fn cancel(&mut self) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        if let Some(row) = self.rows.get_mut(self.selected) {
            row.value = edit.before;
        }
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

impl Widget for &SettingsForm {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 8 || area.height < 3 {
            return;
        }

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
            .max(hint.chars().count() + 4);
        let width = u16::try_from(widest + 2)
            .unwrap_or(u16::MAX)
            .min(area.width);
        let height = u16::try_from(self.rows.len() + 2)
            .unwrap_or(u16::MAX)
            .min(area.height);
        let rect = centred(area, width, height);

        // It floats over the grid, so whatever it covers is erased rather
        // than left showing through.
        Clear.render(rect, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.title))
            .title_bottom(Line::styled(format!(" {hint} "), self.faded).right_aligned())
            .border_style(self.border);
        let inner = block.inner(rect);
        block.render(rect, buf);

        let value_x = inner
            .x
            .saturating_add(1)
            .saturating_add(u16::try_from(label_width + 2).unwrap_or(u16::MAX));

        for (index, row) in self.rows.iter().enumerate().take(usize::from(inner.height)) {
            let y = inner.y + u16::try_from(index).unwrap_or(0);
            let chosen = index == self.selected;
            let base = if chosen {
                self.highlight
            } else {
                Style::default()
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

            write(buf, inner, inner.x + 1, y, &row.label, base);

            let shown = match (&self.editing, chosen, &row.shape) {
                (Some(edit), true, Shape::Text) => format!("{}▏", edit.text),
                (Some(edit), true, _) => format!("Custom: {}▏", edit.text),
                _ => row.shown(),
            };
            let arrows = base.patch(self.faded);
            let x = write(buf, inner, value_x, y, "◂ ", arrows);
            let x = write(buf, inner, x, y, &shown, base);
            write(buf, inner, x, y, " ▸", arrows);
        }
    }
}

#[cfg(test)]
mod tests;
