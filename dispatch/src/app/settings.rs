//! The Settings workspace as the app runs it: what it holds, how a key or the
//! pointer edits it, and what Apply and Discard do.
//!
//! The widget in `dispatch_tui::settings_view` draws and keeps its own focus,
//! selection and scroll. This is where the values live: a draft copy of
//! `preferences.toml`'s contents is edited, previewed in this window at once,
//! and written, one section at a time, only when the user applies it.

use dispatch_config::preferences::{
    self, ApplyError, Effective, Preferences, Rgb8, Section, Source, Sourced,
};
use dispatch_tui::settings_view::{FieldKind, FieldView, Focus, SettingsLayout, SettingsView};

use super::*;
use pointer::ButtonId;

/// Index of each category, in the order they are listed.
const MOUSE: usize = 0;
const APPEARANCE: usize = 1;
const ADVANCED: usize = 2;
const CATEGORIES: [&str; 3] = ["Mouse & layout", "Appearance", "Advanced"];

const THEMES: [(ThemeChoice, &str); 3] = [
    (ThemeChoice::Terminal, "Follow terminal"),
    (ThemeChoice::Dark, "Dark"),
    (ThemeChoice::Light, "Light"),
];

/// The accents that are a plain choice; `Custom…` follows them.
///
/// `Accent::Terminal` is stored as "terminal" but shown as the theme's own:
/// following the terminal it is the terminal's accent, and under Dark or
/// Light it is the preset's, so "Terminal" would be wrong half the time.
const PRESETS: [(Accent, &str); 8] = [
    (Accent::Terminal, "Theme default"),
    (Accent::Violet, "Violet"),
    (Accent::Blue, "Blue"),
    (Accent::Teal, "Teal"),
    (Accent::Green, "Green"),
    (Accent::Amber, "Amber"),
    (Accent::Red, "Red"),
    (Accent::Pink, "Pink"),
];
const CUSTOM: &str = "Custom…";

const ICONS: [(IconSet, &str); 2] = [(IconSet::Nerd, "Nerd Font"), (IconSet::Plain, "Plain")];

/// What the workspace was leaving when it stopped to ask about unapplied
/// edits, to carry on with once they are dealt with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Leave {
    /// Close Settings.
    Close,
    /// Show another category.
    Category(usize),
    /// Follow the highlighted search result to its own category.
    Jump,
}

/// Whether the workspace is still there after something was done to it.
enum Flow {
    Stay,
    Close,
}

/// The open Settings workspace.
pub(super) struct SettingsWorkspace {
    /// What is drawn and where the keyboard is.
    pub(super) view: SettingsView,
    /// What `preferences.toml` held when Settings opened, or when it last
    /// applied: what an apply is checked against, and what Discard returns to.
    pub(super) base: Preferences,
    /// What is being edited.
    pub(super) draft: Preferences,
    /// The keys an apply found changed on disk since `base`.
    pub(super) conflicts: Vec<&'static str>,
    /// A custom accent being typed, until Enter or Esc.
    pub(super) text: Option<String>,
    /// What the leave-with-edits prompt will go on to do.
    leaving: Option<Leave>,
}

/// How many preferences of `section` the draft has changed.
fn changes_in(section: Section, base: &Preferences, draft: &Preferences) -> usize {
    let (b, d) = (base, draft);
    let flags = match section {
        Section::Mouse => [
            b.interface.focus_follows_pointer != d.interface.focus_follows_pointer,
            b.interface.hover_claims_panes != d.interface.hover_claims_panes,
            false,
            false,
        ],
        Section::Appearance => [
            b.interface.motion != d.interface.motion,
            b.appearance.theme != d.appearance.theme,
            b.appearance.accent != d.appearance.accent,
            b.appearance.icons != d.appearance.icons,
        ],
    };
    flags.into_iter().filter(|changed| *changed).count()
}

/// The sections the draft has edited.
fn dirty_sections(base: &Preferences, draft: &Preferences) -> Vec<(Section, usize)> {
    [Section::Mouse, Section::Appearance]
        .into_iter()
        .map(|section| (section, changes_in(section, base, draft)))
        .filter(|(_, n)| *n > 0)
        .collect()
}

/// Copies `section`'s preferences from `from` into `into`.
fn adopt(section: Section, into: &mut Preferences, from: &Preferences) {
    match section {
        Section::Mouse => {
            into.interface.focus_follows_pointer = from.interface.focus_follows_pointer;
            into.interface.hover_claims_panes = from.interface.hover_claims_panes;
        }
        Section::Appearance => {
            into.interface.motion = from.interface.motion;
            into.appearance = from.appearance.clone();
        }
    }
}

/// Takes `fresh`'s values of `section` as what is committed. A field the draft
/// had not edited follows it, so only the user's own edits stay pending.
fn rebase(section: Section, base: &mut Preferences, draft: &mut Preferences, fresh: &Preferences) {
    macro_rules! follow {
        ($($path:ident).+) => {
            if draft.$($path).+ == base.$($path).+ {
                draft.$($path).+ = fresh.$($path).+.clone();
            }
            base.$($path).+ = fresh.$($path).+.clone();
        };
    }
    match section {
        Section::Mouse => {
            follow!(interface.focus_follows_pointer);
            follow!(interface.hover_claims_panes);
        }
        Section::Appearance => {
            follow!(interface.motion);
            follow!(appearance.theme);
            follow!(appearance.accent);
            follow!(appearance.icons);
        }
    }
}

fn source_label(source: Source) -> &'static str {
    match source {
        Source::BuiltIn => "Built-in",
        Source::ConfigFile => "config.toml",
        Source::Preferences => "Settings",
    }
}

/// A row outside Appearance, in `category`.
#[allow(clippy::too_many_arguments)]
fn plain_row(
    category: usize,
    id: &'static str,
    label: &str,
    description: &str,
    kind: FieldKind,
    value: String,
    source: String,
    applies: &'static str,
) -> FieldView {
    FieldView {
        id,
        label: label.to_string(),
        description: description.to_string(),
        category,
        kind,
        value,
        source,
        applies,
        changed: false,
        conflict: false,
        resettable: false,
    }
}

/// Whether the draft sets the preference a row edits, rather than leaving it
/// to `config.toml` and the default.
fn is_set(draft: &Preferences, id: &str) -> bool {
    match id {
        "focus_follows_pointer" => draft.interface.focus_follows_pointer.is_some(),
        "hover_claims_panes" => draft.interface.hover_claims_panes.is_some(),
        "motion" => draft.interface.motion.is_some(),
        "theme" => draft.appearance.theme.is_some(),
        "accent" | "accent_custom" => draft.appearance.accent.is_some(),
        "icons" => draft.appearance.icons.is_some(),
        _ => false,
    }
}

/// Puts the preference a row edits back to "inherit". Returns whether the
/// row edits a preference at all.
fn unset(draft: &mut Preferences, id: &str) -> bool {
    match id {
        "focus_follows_pointer" => draft.interface.focus_follows_pointer = None,
        "hover_claims_panes" => draft.interface.hover_claims_panes = None,
        "motion" => draft.interface.motion = None,
        "theme" => draft.appearance.theme = None,
        "accent" | "accent_custom" => draft.appearance.accent = None,
        "icons" => draft.appearance.icons = None,
        _ => return false,
    }
    true
}

/// What a step stores: nothing, when it lands on what the field would
/// inherit and nothing was committed for it, so stepping there and back is
/// no change and never pins the default into the file.
fn settled<T: PartialEq>(value: T, inherited: T, committed: Option<&T>) -> Option<T> {
    (committed.is_some() || value != inherited).then_some(value)
}

/// A path as text, or `unknown` when it could not be worked out.
fn shown_path<E>(path: Result<std::path::PathBuf, E>) -> String {
    path.map_or_else(|_| "unknown".to_string(), |path| path.display().to_string())
}

fn on_off(on: bool) -> String {
    if on { "On" } else { "Off" }.to_string()
}

fn accent_index(accent: &Accent) -> usize {
    PRESETS
        .iter()
        .position(|(preset, _)| preset == accent)
        .unwrap_or(PRESETS.len())
}

fn hex(Rgb8(r, g, b): Rgb8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Adds `c` to a colour being typed, when it can be part of `#rrggbb`.
fn push_hex(text: &mut String, c: char) {
    if (c == '#' || c.is_ascii_hexdigit()) && text.len() < 7 {
        text.push(c);
    }
}

/// `index` stepped one place and wrapped within `len`.
fn wrapped(index: usize, len: usize, forward: bool) -> usize {
    if forward {
        (index + 1) % len
    } else {
        (index + len - 1) % len
    }
}

impl App {
    /// Opens Settings, on the category and field it was last left on.
    pub(super) fn open_settings_workspace(&mut self) {
        let base = self
            .preferences_dir
            .as_deref()
            .map(preferences::load_or_default)
            .unwrap_or_default();
        let mut workspace = SettingsWorkspace {
            view: SettingsView::new(CATEGORIES.map(String::from).to_vec(), Vec::new()),
            draft: base.clone(),
            base,
            conflicts: Vec::new(),
            text: None,
            leaving: None,
        };
        workspace.view.set_chrome(self.theme.chrome());
        self.refresh_settings(&mut workspace);
        let (category, field) = self.settings_memory;
        workspace.view.select_category(category);
        workspace.view.select_field(field);
        self.overlay = Some(Overlay::Preferences(workspace));
    }

    /// Where the open workspace drew what can be clicked, as the last frame
    /// left it.
    pub(crate) fn settings_layout(&self) -> Option<SettingsLayout> {
        match self.overlay.as_ref()? {
            Overlay::Preferences(workspace) => Some(workspace.view.layout(self.overlay_area)),
            _ => None,
        }
    }

    /// Records what the workspace drew that answers a click, above the frame.
    pub(super) fn push_settings_hits(&self, hits: &mut pointer::HitMap) {
        use pointer::{DialogHit, Target};

        let Some(layout) = self.settings_layout() else {
            return;
        };
        let at = |hit| Target::Dialog(hit);
        hits.push(layout.rect, at(DialogHit::Area));
        hits.push(layout.search, at(DialogHit::Search));
        for (rect, index) in layout.categories {
            hits.push(rect, at(DialogHit::Category(index)));
        }
        for (rect, index) in layout.fields {
            hits.push(rect, at(DialogHit::Field(index)));
        }
        for (rect, index) in layout.resets {
            hits.push(rect, at(DialogHit::FieldReset(index)));
        }
        for (rect, index, forward) in layout.steps {
            hits.push(rect, at(DialogHit::FieldStep(index, forward)));
        }
        for (rect, id) in layout.buttons.into_iter().chain(layout.actions) {
            hits.push(rect, at(DialogHit::Button(id)));
        }
        hits.push(layout.close, at(DialogHit::Close));
    }

    /// Runs `act` on the open workspace, taken out of the overlay so that it
    /// and the rest of the app can both be changed.
    fn in_settings(&mut self, act: impl FnOnce(&mut Self, &mut SettingsWorkspace) -> Flow) {
        let mut workspace = match self.overlay.take() {
            Some(Overlay::Preferences(workspace)) => workspace,
            other => {
                self.overlay = other;
                return;
            }
        };
        match act(self, &mut workspace) {
            Flow::Stay => self.overlay = Some(Overlay::Preferences(workspace)),
            Flow::Close => {
                self.remember_settings(&workspace);
                // A request that arrived while Settings was open was queued.
                self.open_next_approval();
            }
        }
    }

    /// Keeps where the workspace was, to reopen there.
    fn remember_settings(&mut self, workspace: &SettingsWorkspace) {
        let Some(field) = workspace.view.selected_field() else {
            self.settings_memory = (workspace.view.category(), 0);
            return;
        };
        let (id, category) = (field.id, field.category);
        // A place among the rows shown now: the custom accent's row is
        // there only while that accent is chosen, so a fixed list of ids
        // would put a later row on the wrong one.
        let position = self
            .settings_fields(workspace)
            .iter()
            .filter(|own| own.category == category)
            .position(|own| own.id == id);
        self.settings_memory = (category, position.unwrap_or(0));
    }

    /// What the draft makes of each setting.
    fn draft_effective(&self, workspace: &SettingsWorkspace) -> Effective {
        preferences::effective(
            &self.interface_config,
            &self.interface_present,
            &workspace.draft,
        )
    }

    /// The rows of every category, from the draft.
    fn settings_fields(&self, workspace: &SettingsWorkspace) -> Vec<FieldView> {
        let effective = self.draft_effective(workspace);
        let (base, draft) = (&workspace.base, &workspace.draft);
        let conflict = |key: &str| workspace.conflicts.contains(&key);
        let applies = "Applies now";
        let row =
            |id, label: &str, description: &str, kind, value: String, source, changed, key| {
                FieldView {
                    id,
                    label: label.to_string(),
                    description: description.to_string(),
                    category: APPEARANCE,
                    kind,
                    value,
                    source: source_label(source).to_string(),
                    applies,
                    changed,
                    conflict: conflict(key),
                    resettable: false,
                }
            };
        let choices =
            |names: Vec<&str>| FieldKind::Choice(names.into_iter().map(String::from).collect());

        let theme = THEMES
            .iter()
            .find(|(choice, _)| *choice == effective.theme.value)
            .map_or("", |(_, name)| *name);
        let accent = accent_index(&effective.accent.value);
        let accent_name = PRESETS.get(accent).map_or(CUSTOM, |(_, name)| *name);
        let mut fields = self.mouse_fields(workspace, &effective);
        fields.extend([
            row(
                "theme",
                "Theme",
                "Which palette Dispatch draws with.",
                choices(THEMES.iter().map(|(_, name)| *name).collect()),
                theme.to_string(),
                effective.theme.source,
                base.appearance.theme != draft.appearance.theme,
                "theme",
            ),
            row(
                "accent",
                "Accent",
                "The colour that marks what is selected; Theme default is the terminal's own, or the preset's.",
                choices(
                    PRESETS
                        .iter()
                        .map(|(_, name)| *name)
                        .chain([CUSTOM])
                        .collect(),
                ),
                accent_name.to_string(),
                effective.accent.source,
                base.appearance.accent != draft.appearance.accent,
                "accent",
            ),
        ]);
        if let Accent::Custom(rgb) = effective.accent.value {
            fields.push(row(
                "accent_custom",
                "Custom accent",
                "A colour written #rrggbb.",
                FieldKind::Text,
                workspace.text.clone().unwrap_or_else(|| hex(rgb)),
                effective.accent.source,
                base.appearance.accent != draft.appearance.accent,
                "accent",
            ));
        }
        fields.push(row(
            "motion",
            "Motion",
            "Spinners, pulses and transitions.",
            FieldKind::Toggle,
            on_off(effective.motion.value),
            effective.motion.source,
            base.interface.motion != draft.interface.motion,
            "motion",
        ));
        fields.push(row(
            "icons",
            "Icons",
            "Nerd Font glyphs, or plain characters every font has.",
            choices(ICONS.iter().map(|(_, name)| *name).collect()),
            ICONS
                .iter()
                .find(|(set, _)| *set == effective.icons.value)
                .map_or("", |(_, name)| *name)
                .to_string(),
            effective.icons.source,
            base.appearance.icons != draft.appearance.icons,
            "icons",
        ));
        fields.extend(self.advanced_fields());
        for field in &mut fields {
            field.resettable = is_set(draft, field.id);
        }
        fields
    }

    /// The Mouse & layout rows. The two pointer toggles are the draft's, to be
    /// applied; the sidebar's rows are read from the app and act at once, so
    /// they are never changed against a base.
    fn mouse_fields(&self, workspace: &SettingsWorkspace, effective: &Effective) -> Vec<FieldView> {
        use dispatch_config::ui_state::{MAX_SIDEBAR, MIN_SIDEBAR};

        let (base, draft) = (&workspace.base, &workspace.draft);
        let toggle = |id, label: &str, description: &str, value: &Sourced<bool>, changed| {
            let mut field = plain_row(
                MOUSE,
                id,
                label,
                description,
                FieldKind::Toggle,
                on_off(value.value),
                source_label(value.source).to_string(),
                "Applies now",
            );
            field.changed = changed;
            field.conflict = workspace.conflicts.contains(&id);
            field
        };
        let sidebar = |id, label: &str, description: &str, kind, value: String| {
            plain_row(
                MOUSE,
                id,
                label,
                description,
                kind,
                value,
                "ui.toml".to_string(),
                "Applies now",
            )
        };
        vec![
            toggle(
                "focus_follows_pointer",
                "Focus follows pointer",
                "Resting the pointer on a pane gives it the keyboard.",
                &effective.focus_follows_pointer,
                base.interface.focus_follows_pointer != draft.interface.focus_follows_pointer,
            ),
            toggle(
                "hover_claims_panes",
                "Hover claims shared panes",
                "The window under the pointer sets the size of panes shared between windows.",
                &effective.hover_claims_panes,
                base.interface.hover_claims_panes != draft.interface.hover_claims_panes,
            ),
            sidebar(
                "sidebar_width",
                "Sidebar width",
                "Columns the project sidebar takes; saved at once.",
                FieldKind::Number {
                    min: i64::from(MIN_SIDEBAR),
                    max: i64::from(MAX_SIDEBAR),
                },
                self.sidebar_width.to_string(),
            ),
            sidebar(
                "show_sidebar",
                "Show sidebar",
                "Fold the sidebar away and back; saved at once.",
                FieldKind::Toggle,
                on_off(!self.sidebar_collapsed),
            ),
            plain_row(
                MOUSE,
                "reset_sidebar",
                "Reset sidebar width",
                "Back to the default width.",
                FieldKind::Action(ButtonId::ResetSidebar),
                String::new(),
                "ui.toml".to_string(),
                "Applies now",
            ),
        ]
    }

    /// The Advanced rows: where things are, and what is running. None can be
    /// edited here.
    fn advanced_fields(&self) -> Vec<FieldView> {
        use dispatch_os::paths;

        let config_dir = paths::config_dir();
        let beside = |name: &str| {
            shown_path(
                config_dir
                    .as_ref()
                    .map(|dir| dir.join(name))
                    .map_err(|_| ()),
            )
        };
        let mode = match self.attachments().len() {
            0 => "Standalone".to_string(),
            n => format!("Attached to {n} machine(s)"),
        };
        [
            (
                "config_file",
                "Configuration file",
                "Where config.toml is read from.",
                shown_path(paths::config_file()),
            ),
            (
                "preferences_file",
                "Preferences",
                "What Settings writes; config.toml is never rewritten.",
                beside(preferences::FILE),
            ),
            (
                "ui_file",
                "ui.toml",
                "The sidebar's width and fold.",
                beside("ui.toml"),
            ),
            (
                "harnesses_dir",
                "Harnesses",
                "One file for each agent.",
                shown_path(paths::harnesses_dir()),
            ),
            (
                "log_file",
                "Log file",
                "Where diagnostics go, since the screen is Dispatch's.",
                shown_path(paths::log_file()),
            ),
            (
                "version",
                "Version",
                "This build of Dispatch.",
                env!("CARGO_PKG_VERSION").to_string(),
            ),
            (
                "mode",
                "Mode",
                "Whether this window runs its own agents or is attached to daemons.",
                mode,
            ),
        ]
        .into_iter()
        .map(|(id, label, description, value)| {
            plain_row(
                ADVANCED,
                id,
                label,
                description,
                FieldKind::ReadOnly,
                value,
                String::new(),
                "Read only",
            )
        })
        .collect()
    }

    /// The footer's account of what is unapplied, or none.
    fn settings_pending(workspace: &SettingsWorkspace) -> Option<String> {
        let dirty = dirty_sections(&workspace.base, &workspace.draft);
        let total: usize = dirty.iter().map(|(_, n)| n).sum();
        if total == 0 {
            return None;
        }
        let noun = if total == 1 { "change" } else { "changes" };
        Some(match dirty.as_slice() {
            [(section, _)] => {
                let category = match section {
                    Section::Mouse => MOUSE,
                    Section::Appearance => APPEARANCE,
                };
                format!("{total} {noun} in {}", CATEGORIES[category])
            }
            _ => format!("{total} {noun}"),
        })
    }

    /// Rebuilds the rows and the footer's count from the draft.
    fn refresh_settings(&self, workspace: &mut SettingsWorkspace) {
        let fields = self.settings_fields(workspace);
        workspace.view.set_fields(fields);
        workspace
            .view
            .set_pending(Self::settings_pending(workspace));
    }

    /// Draws this window from the draft, as a preview, and refreshes the rows.
    fn preview(&mut self, workspace: &mut SettingsWorkspace) {
        let effective = self.draft_effective(workspace);
        self.apply_appearance(
            effective.theme.value,
            &effective.accent.value,
            effective.icons.value,
        );
        self.set_motion(effective.motion.value);
        self.refresh_settings(workspace);
    }

    /// Writes what the draft changed, section by section.
    ///
    /// Returns whether everything was written. Nothing is written when the
    /// file was changed by something else since Settings opened; the fields
    /// that were are marked, and say so on the footer.
    pub(super) fn apply_settings(&mut self, workspace: &mut SettingsWorkspace) -> bool {
        let dirty = dirty_sections(&workspace.base, &workspace.draft);
        for (section, _) in dirty {
            if let Some(dir) = self.preferences_dir.clone() {
                match preferences::apply(&dir, section, &workspace.base, &workspace.draft) {
                    Ok(()) => {}
                    Err(ApplyError::Conflict(fields)) => {
                        // What is on disk is now the committed value, with
                        // the user's edit still pending over it, so applying
                        // again writes their choice knowingly.
                        let fresh = preferences::load_or_default(&dir);
                        rebase(section, &mut workspace.base, &mut workspace.draft, &fresh);
                        // The user's edit is what stays in the draft, so
                        // the message says what each button now does with it.
                        let it = if fields.len() == 1 { "it" } else { "them" };
                        self.warn(format!(
                            "{} changed elsewhere since you opened Settings; \
                             Apply again to replace {it} with yours, or Discard to take {it}",
                            fields.join(", ")
                        ));
                        workspace.conflicts = fields;
                        self.preview(workspace);
                        return false;
                    }
                    Err(ApplyError::Unreadable(path)) => {
                        self.warn(format!(
                            "{} is not valid TOML; mend or remove it",
                            path.display()
                        ));
                        return false;
                    }
                    Err(ApplyError::Io(error)) => {
                        self.warn(format!("could not save settings: {error}"));
                        return false;
                    }
                }
            }
            // This section is now what is on disk, so a later section that
            // fails does not make it a conflict on the next try.
            adopt(section, &mut workspace.base, &workspace.draft);
            if section == Section::Mouse {
                self.commit_mouse(workspace);
            }
        }
        workspace.conflicts.clear();
        self.preferences = workspace.base.clone();
        self.say("Settings applied");
        self.refresh_settings(workspace);
        self.settle_button_focus(workspace);
        true
    }

    /// Makes the pointer settings of what is committed the ones in force. They
    /// are not previewed, so only Apply and Discard touch them.
    fn commit_mouse(&mut self, workspace: &SettingsWorkspace) {
        let effective = preferences::effective(
            &self.interface_config,
            &self.interface_present,
            &workspace.base,
        );
        self.set_focus_follows_pointer(effective.focus_follows_pointer.value);
        self.set_hover_claims_panes(effective.hover_claims_panes.value);
    }

    /// Puts the draft back to what was committed, and this window with it.
    pub(super) fn discard_settings(&mut self, workspace: &mut SettingsWorkspace) {
        // The file as it is now, so a change made elsewhere is not a
        // conflict on the next edit.
        if let Some(dir) = self.preferences_dir.clone() {
            // A file that cannot be read still holds the user's settings, so
            // reading it as the defaults would throw them away here.
            match preferences::load(&dir) {
                Ok(fresh) => {
                    workspace.base = fresh;
                    self.preferences = workspace.base.clone();
                    self.commit_mouse(workspace);
                }
                Err(error) => {
                    tracing::warn!(%error, "ignoring preferences that cannot be read");
                    self.warn(format!(
                        "{} cannot be read; keeping what Settings had",
                        dir.join(preferences::FILE).display()
                    ));
                }
            }
        }
        workspace.draft = workspace.base.clone();
        workspace.conflicts.clear();
        workspace.text = None;
        workspace.view.set_editing(false);
        self.preview(workspace);
        self.settle_button_focus(workspace);
    }

    /// The footer's buttons go once nothing is pending, so focus on them goes
    /// back to the rows.
    fn settle_button_focus(&self, workspace: &mut SettingsWorkspace) {
        if workspace.view.focus() == Focus::Buttons && workspace.view.focused_button().is_none() {
            workspace.view.set_focus(Focus::Fields);
        }
    }

    /// Whether there is anything unapplied.
    fn settings_dirty(workspace: &SettingsWorkspace) -> bool {
        !dirty_sections(&workspace.base, &workspace.draft).is_empty()
    }

    /// Asks what to do with unapplied edits before `leave`.
    fn ask_to_leave(workspace: &mut SettingsWorkspace, leave: Leave) {
        workspace.leaving = Some(leave);
        workspace.view.set_prompt(Some(vec![
            ButtonId::KeepEditing,
            ButtonId::Discard,
            ButtonId::Apply,
        ]));
    }

    /// Does what was being left once the edits are dealt with.
    fn finish_leave(workspace: &mut SettingsWorkspace, leave: Leave) -> Flow {
        workspace.view.set_prompt(None);
        match leave {
            Leave::Close => return Flow::Close,
            Leave::Category(category) => workspace.view.select_category(category),
            Leave::Jump => {
                workspace.view.jump_to_selected();
                workspace.view.set_focus(Focus::Fields);
            }
        }
        Flow::Stay
    }

    /// Shows `category`, asking first when there are edits to lose track of.
    fn request_category(workspace: &mut SettingsWorkspace, category: usize) {
        if category >= CATEGORIES.len() || category == workspace.view.category() {
            return;
        }
        if Self::settings_dirty(workspace) {
            Self::ask_to_leave(workspace, Leave::Category(category));
        } else {
            workspace.view.select_category(category);
        }
    }

    /// Follows the highlighted search result to its category.
    fn request_jump(workspace: &mut SettingsWorkspace) {
        let Some(category) = workspace.view.selected_field().map(|field| field.category) else {
            return;
        };
        if category != workspace.view.category() && Self::settings_dirty(workspace) {
            Self::ask_to_leave(workspace, Leave::Jump);
        } else {
            workspace.view.jump_to_selected();
            workspace.view.set_focus(Focus::Fields);
        }
    }

    /// Closes Settings, asking first when there are unapplied edits.
    fn request_close(workspace: &mut SettingsWorkspace) -> Flow {
        if Self::settings_dirty(workspace) {
            Self::ask_to_leave(workspace, Leave::Close);
            Flow::Stay
        } else {
            Flow::Close
        }
    }

    /// What a footer button, or a prompt's, does.
    fn press_settings_button(&mut self, workspace: &mut SettingsWorkspace, id: ButtonId) -> Flow {
        // While the prompt asks, only its own buttons answer: an Action row
        // under it is still drawn, and still clickable, but must wait.
        let answers = matches!(
            id,
            ButtonId::Apply | ButtonId::Discard | ButtonId::KeepEditing
        );
        if workspace.leaving.is_some() && !answers {
            return Flow::Stay;
        }
        match id {
            ButtonId::Apply => {
                let applied = self.apply_settings(workspace);
                match workspace.leaving.take() {
                    Some(leave) if applied => return Self::finish_leave(workspace, leave),
                    // Not written, so there is nothing to carry on past.
                    Some(_) => workspace.view.set_prompt(None),
                    None => {}
                }
            }
            ButtonId::Discard => {
                self.discard_settings(workspace);
                if let Some(leave) = workspace.leaving.take() {
                    return Self::finish_leave(workspace, leave);
                }
            }
            ButtonId::KeepEditing => {
                workspace.leaving = None;
                workspace.view.set_prompt(None);
            }
            ButtonId::ClearSearch => workspace.view.clear_search(),
            ButtonId::ResetSidebar => {
                self.sidebar_width = dispatch_config::ui_state::DEFAULT_SIDEBAR;
                self.save_ui();
                self.refresh_settings(workspace);
            }
            _ => {}
        }
        Flow::Stay
    }

    /// A button of the workspace was pressed, by key or by release.
    pub(super) fn settings_button(&mut self, id: ButtonId) {
        self.in_settings(|app, workspace| app.press_settings_button(workspace, id));
    }

    /// Whether the window is too small to show anything of the workspace
    /// but its resize message and `[×]`.
    fn settings_too_small(&self, workspace: &SettingsWorkspace) -> bool {
        workspace.view.layout(self.overlay_area).too_small
    }

    /// Closes Settings without asking, throwing pending edits away: what a
    /// window too small to show the prompt does, as its message says.
    fn close_discarding(&mut self, workspace: &mut SettingsWorkspace) -> Flow {
        workspace.leaving = None;
        workspace.view.set_prompt(None);
        self.discard_settings(workspace);
        Flow::Close
    }

    /// `[×]` was pressed.
    pub(super) fn settings_close(&mut self) {
        self.in_settings(|app, workspace| {
            if app.settings_too_small(workspace) {
                return app.close_discarding(workspace);
            }
            if workspace.leaving.is_some() {
                return Flow::Stay;
            }
            Self::request_close(workspace)
        });
    }

    /// A field's `‹` or `›` was pressed, by its row among those shown. A
    /// compact category arrow carries `usize::MAX`.
    pub(super) fn settings_step_at(&mut self, row: usize, forward: bool) {
        self.in_settings(|app, workspace| {
            if workspace.leaving.is_some() {
                return Flow::Stay;
            }
            if row == usize::MAX {
                let category = workspace.view.category();
                let next = if forward {
                    category + 1
                } else {
                    category.saturating_sub(1)
                };
                Self::request_category(workspace, next);
                return Flow::Stay;
            }
            workspace.view.select_field(row);
            workspace.view.set_focus(Focus::Fields);
            app.step_selected(workspace, forward);
            Flow::Stay
        });
    }

    /// Steps the highlighted field's value, or flips it.
    fn step_selected(&mut self, workspace: &mut SettingsWorkspace, forward: bool) {
        // A search result belongs to its own category: go there first, so
        // edits never span sections without the leave prompt having a say.
        if !workspace.view.search().is_empty() {
            Self::request_jump(workspace);
            if workspace.leaving.is_some() {
                return;
            }
        }
        let Some(field) = workspace.view.selected_field() else {
            return;
        };
        let (id, kind) = (field.id, field.kind.clone());
        if !matches!(
            kind,
            FieldKind::Toggle | FieldKind::Choice(_) | FieldKind::Number { .. }
        ) {
            return;
        }
        let effective = self.draft_effective(workspace);
        // What each field would be with nothing set in Settings: every field
        // is layered on its own, so the empty preferences give all of them.
        let inherited = preferences::effective(
            &self.interface_config,
            &self.interface_present,
            &Preferences::default(),
        );
        let (base, draft) = (&workspace.base, &mut workspace.draft);
        match id {
            "theme" => {
                let at = THEMES
                    .iter()
                    .position(|(choice, _)| *choice == effective.theme.value)
                    .unwrap_or(0);
                draft.appearance.theme = settled(
                    THEMES[wrapped(at, THEMES.len(), forward)].0,
                    inherited.theme.value,
                    base.appearance.theme.as_ref(),
                );
            }
            "accent" => {
                let next = wrapped(
                    accent_index(&effective.accent.value),
                    PRESETS.len() + 1,
                    forward,
                );
                let accent = match PRESETS.get(next) {
                    Some((preset, _)) => *preset,
                    // Starts from the colour in force, so choosing Custom
                    // changes nothing until it is edited.
                    None => {
                        let dispatch_tui::theme::Rgb(r, g, b) = self.theme.palette().accent;
                        Accent::Custom(Rgb8(r, g, b))
                    }
                };
                draft.appearance.accent = settled(
                    accent,
                    inherited.accent.value,
                    base.appearance.accent.as_ref(),
                );
            }
            "focus_follows_pointer" => {
                draft.interface.focus_follows_pointer = settled(
                    !effective.focus_follows_pointer.value,
                    inherited.focus_follows_pointer.value,
                    base.interface.focus_follows_pointer.as_ref(),
                );
            }
            "hover_claims_panes" => {
                draft.interface.hover_claims_panes = settled(
                    !effective.hover_claims_panes.value,
                    inherited.hover_claims_panes.value,
                    base.interface.hover_claims_panes.as_ref(),
                );
            }
            // The sidebar acts at once, as dragging and `Alt s` do, so it
            // never touches the draft.
            "sidebar_width" => {
                self.resize_sidebar(if forward { 2 } else { -2 });
                self.refresh_settings(workspace);
                return;
            }
            "show_sidebar" => {
                self.toggle_sidebar();
                self.refresh_settings(workspace);
                return;
            }
            "motion" => {
                draft.interface.motion = settled(
                    !effective.motion.value,
                    inherited.motion.value,
                    base.interface.motion.as_ref(),
                );
            }
            "icons" => {
                let at = ICONS
                    .iter()
                    .position(|(set, _)| *set == effective.icons.value)
                    .unwrap_or(0);
                draft.appearance.icons = settled(
                    ICONS[wrapped(at, ICONS.len(), forward)].0,
                    inherited.icons.value,
                    base.appearance.icons.as_ref(),
                );
            }
            _ => return,
        }
        self.preview(workspace);
    }

    /// Puts the highlighted field back to what it inherits: Reset to default.
    /// Applying then removes it from the file.
    fn reset_selected(&mut self, workspace: &mut SettingsWorkspace) {
        if !workspace.view.search().is_empty() {
            Self::request_jump(workspace);
            if workspace.leaving.is_some() {
                return;
            }
        }
        let Some(id) = workspace.view.selected_field().map(|field| field.id) else {
            return;
        };
        if unset(&mut workspace.draft, id) {
            workspace.text = None;
            workspace.view.set_editing(false);
            self.preview(workspace);
        }
    }

    /// A field's reset control was pressed, by its row among those shown.
    pub(super) fn settings_reset_at(&mut self, row: usize) {
        self.in_settings(|app, workspace| {
            if workspace.leaving.is_some() {
                return Flow::Stay;
            }
            workspace.view.select_field(row);
            workspace.view.set_focus(Focus::Fields);
            app.reset_selected(workspace);
            Flow::Stay
        });
    }

    /// What Enter does on the highlighted field.
    fn activate_selected(&mut self, workspace: &mut SettingsWorkspace) -> Flow {
        if !workspace.view.search().is_empty() {
            Self::request_jump(workspace);
            return Flow::Stay;
        }
        let Some(field) = workspace.view.selected_field() else {
            return Flow::Stay;
        };
        match field.kind.clone() {
            FieldKind::Toggle | FieldKind::Choice(_) | FieldKind::Number { .. } => {
                self.step_selected(workspace, true);
            }
            FieldKind::Text => {
                workspace.text = Some(field.value.clone());
                workspace.view.set_editing(true);
                self.refresh_settings(workspace);
            }
            FieldKind::Action(id) => return self.press_settings_button(workspace, id),
            FieldKind::ReadOnly => {}
        }
        Flow::Stay
    }

    /// Acts on one key while the workspace has the keyboard. Nothing is sent
    /// on to a pane.
    pub(super) fn settings_key(&mut self, key: &KeyEvent) {
        self.in_settings(|app, workspace| app.settings_key_in(workspace, key));
    }

    fn settings_key_in(&mut self, workspace: &mut SettingsWorkspace, key: &KeyEvent) -> Flow {
        // Chords belong to the keymap, which is not listening here.
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return Flow::Stay;
        }

        // Too small to draw anything but the message, which says Esc closes
        // and discards: every other key would edit what cannot be seen.
        if self.settings_too_small(workspace) {
            if key.code == KeyCode::Esc {
                return self.close_discarding(workspace);
            }
            return Flow::Stay;
        }

        // The prompt holds the keyboard on its buttons until it is answered.
        if workspace.leaving.is_some() {
            match key.code {
                KeyCode::Esc => {
                    return self.press_settings_button(workspace, ButtonId::KeepEditing);
                }
                KeyCode::Tab => workspace.view.cycle_button(true),
                KeyCode::BackTab => workspace.view.cycle_button(false),
                KeyCode::Left => workspace.view.move_button(false),
                KeyCode::Right => workspace.view.move_button(true),
                KeyCode::Enter => {
                    if let Some(id) = workspace.view.focused_button() {
                        return self.press_settings_button(workspace, id);
                    }
                }
                _ => {}
            }
            return Flow::Stay;
        }

        if workspace.text.is_some() {
            self.edit_text(workspace, key);
            return Flow::Stay;
        }

        match key.code {
            KeyCode::Tab => {
                workspace.view.focus_next();
                return Flow::Stay;
            }
            KeyCode::BackTab => {
                workspace.view.focus_previous();
                return Flow::Stay;
            }
            KeyCode::Esc => {
                // The innermost thing first: the search, then the workspace.
                if workspace.view.focus() == Focus::Search || !workspace.view.search().is_empty() {
                    workspace.view.clear_search();
                    if workspace.view.focus() == Focus::Search {
                        workspace.view.focus_next();
                    }
                    return Flow::Stay;
                }
                return Self::request_close(workspace);
            }
            _ => {}
        }

        let compact = workspace.view.layout(self.overlay_area).compact;
        let category = workspace.view.category();
        match (workspace.view.focus(), key.code) {
            (Focus::Search, KeyCode::Char(c)) => workspace.view.push_search(c),
            (Focus::Search, KeyCode::Backspace) => workspace.view.pop_search(),
            (Focus::Search, KeyCode::Down) => {
                workspace.view.set_focus(Focus::Fields);
            }
            (Focus::Search, KeyCode::Enter) => Self::request_jump(workspace),

            (Focus::Categories, KeyCode::Up) => {
                Self::request_category(workspace, category.saturating_sub(1));
            }
            (Focus::Categories, KeyCode::Down) => Self::request_category(workspace, category + 1),
            (Focus::Categories, KeyCode::Left) if compact => {
                Self::request_category(workspace, category.saturating_sub(1));
            }
            (Focus::Categories, KeyCode::Right) if compact => {
                Self::request_category(workspace, category + 1);
            }
            (Focus::Categories, KeyCode::Enter | KeyCode::Right) => {
                workspace.view.set_focus(Focus::Fields);
            }

            (Focus::Fields, KeyCode::Up) => workspace.view.move_up(),
            (Focus::Fields, KeyCode::Down) => workspace.view.move_down(),
            (Focus::Fields, KeyCode::Left) => self.step_selected(workspace, false),
            (Focus::Fields, KeyCode::Right) => self.step_selected(workspace, true),
            (Focus::Fields, KeyCode::Enter) => return self.activate_selected(workspace),
            (Focus::Fields, KeyCode::Backspace | KeyCode::Delete) => {
                self.reset_selected(workspace);
            }

            (Focus::Buttons, KeyCode::Left) => workspace.view.move_button(false),
            (Focus::Buttons, KeyCode::Right) => workspace.view.move_button(true),
            (Focus::Buttons, KeyCode::Enter) => {
                if let Some(id) = workspace.view.focused_button() {
                    return self.press_settings_button(workspace, id);
                }
            }
            _ => {}
        }
        Flow::Stay
    }

    /// Takes a paste into whatever is being typed: the custom accent, or the
    /// search while it has the keyboard. Line breaks are dropped, as in every
    /// prompt: Enter is a decision the paste must not make.
    pub(super) fn settings_paste(&mut self, pasted: &str) {
        self.in_settings(|app, workspace| {
            if workspace.leaving.is_some() || app.settings_too_small(workspace) {
                return Flow::Stay;
            }
            let typed = pasted.chars().filter(|c| !c.is_control());
            if let Some(text) = workspace.text.as_mut() {
                typed.for_each(|c| push_hex(text, c));
                app.refresh_settings(workspace);
            } else if workspace.view.focus() == Focus::Search {
                typed.for_each(|c| workspace.view.push_search(c));
            }
            Flow::Stay
        });
    }

    /// Types into the custom accent.
    fn edit_text(&mut self, workspace: &mut SettingsWorkspace, key: &KeyEvent) {
        let Some(text) = workspace.text.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Char(c) => push_hex(text, c),
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Esc => {
                workspace.text = None;
                workspace.view.set_editing(false);
            }
            KeyCode::Enter => {
                let typed = if text.starts_with('#') {
                    text.clone()
                } else {
                    format!("#{text}")
                };
                match Accent::parse(&typed) {
                    Some(accent @ Accent::Custom(_)) => {
                        workspace.draft.appearance.accent = Some(accent);
                        workspace.text = None;
                        workspace.view.set_editing(false);
                        self.preview(workspace);
                        return;
                    }
                    _ => self.warn("a colour is written #rrggbb"),
                }
            }
            _ => {}
        }
        self.refresh_settings(workspace);
    }

    /// Acts on a pointer event while the workspace is open. Presses on rows,
    /// categories and the search take effect at once; steps, buttons and `[×]`
    /// are held and act on release, through the gesture. A press outside the
    /// box does nothing: it is a dialog, and never throws edits away.
    pub(super) fn settings_pointer(&mut self, mouse: &MouseEvent, target: Option<pointer::Target>) {
        use crossterm::event::MouseButton;
        use pointer::DialogHit;

        let hit = match target {
            Some(pointer::Target::Dialog(hit)) => Some(hit),
            _ => None,
        };
        // A press anywhere ends an inline edit, so keys never go on to one
        // that can no longer be seen.
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.in_settings(|app, workspace| {
                if workspace.text.take().is_some() {
                    workspace.view.set_editing(false);
                    app.refresh_settings(workspace);
                }
                Flow::Stay
            });
        }
        match mouse.kind {
            MouseEventKind::Moved => {
                let row = match hit {
                    Some(DialogHit::Field(index)) => Some(index),
                    _ => None,
                };
                if let Some(Overlay::Preferences(workspace)) = &mut self.overlay {
                    workspace.view.set_hovered(row);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let asking = matches!(
                    &self.overlay,
                    Some(Overlay::Preferences(workspace)) if workspace.leaving.is_some()
                );
                match (hit, target) {
                    // The prompt is answered before anything else is touched.
                    (Some(DialogHit::Button(_)), Some(owner)) => {
                        self.hold_settings_press(owner, mouse)
                    }
                    _ if asking => {}
                    (
                        Some(
                            DialogHit::FieldStep(..) | DialogHit::FieldReset(_) | DialogHit::Close,
                        ),
                        Some(owner),
                    ) => self.hold_settings_press(owner, mouse),
                    (Some(DialogHit::Category(index)), _) => self.in_settings(|_, workspace| {
                        workspace.view.set_focus(Focus::Categories);
                        Self::request_category(workspace, index);
                        Flow::Stay
                    }),
                    (Some(DialogHit::Field(index)), _) => self.in_settings(|_, workspace| {
                        workspace.view.select_field(index);
                        workspace.view.set_focus(Focus::Fields);
                        if !workspace.view.search().is_empty() {
                            Self::request_jump(workspace);
                        }
                        Flow::Stay
                    }),
                    (Some(DialogHit::Search), _) => self.in_settings(|_, workspace| {
                        workspace.view.set_focus(Focus::Search);
                        Flow::Stay
                    }),
                    _ => {}
                }
            }
            // One notch is one row of the list, inside the box.
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if hit.is_some() => {
                let rows = if mouse.kind == MouseEventKind::ScrollDown {
                    1
                } else {
                    -1
                };
                if let Some(Overlay::Preferences(workspace)) = &mut self.overlay
                    && workspace.leaving.is_none()
                {
                    workspace.view.scroll_by(rows);
                }
            }
            _ => {}
        }
    }

    /// Starts a press that acts when it is let go over the same control.
    fn hold_settings_press(&mut self, owner: pointer::Target, mouse: &MouseEvent) {
        self.gesture = Some(pointer::Gesture {
            owner,
            button: crossterm::event::MouseButton::Left,
            last: (mouse.column, mouse.row),
            rect: None,
        });
        self.pressed_double = false;
        if let pointer::Target::Dialog(pointer::DialogHit::Button(id)) = owner {
            self.set_dialog_pressed(Some(id));
        }
    }
}
