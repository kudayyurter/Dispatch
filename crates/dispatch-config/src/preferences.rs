//! What the user chose in Settings.
//!
//! A file of its own, beside `projects.toml` and `ui.toml`: `config.toml` is
//! written by hand and would lose its comments to a rewrite, so Dispatch never
//! writes it. The precedence is built-in, then `config.toml`'s `[interface]`,
//! then this file.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{ConfigError, InterfaceConfig, InterfaceKeysPresent};

/// The file, inside the configuration directory.
pub const FILE: &str = "preferences.toml";

/// Which palette the interface draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    /// The terminal's own colours.
    #[default]
    Terminal,
    /// Dispatch's dark palette.
    Dark,
    /// Dispatch's light palette.
    Light,
}

/// An 8-bit colour, as `#rrggbb` writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb8(pub u8, pub u8, pub u8);

/// The colour that marks what is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Accent {
    /// The terminal's own accent.
    #[default]
    Terminal,
    /// `#b4a0f0`.
    Violet,
    /// `#61afef`.
    Blue,
    /// `#56b6c2`.
    Teal,
    /// `#98c379`.
    Green,
    /// `#e5c07b`.
    Amber,
    /// `#e06c75`.
    Red,
    /// `#ff79c6`.
    Pink,
    /// A colour the user typed.
    Custom(Rgb8),
}

impl Accent {
    /// The name a preset is written under.
    fn name(self) -> Option<&'static str> {
        Some(match self {
            Accent::Terminal => "terminal",
            Accent::Violet => "violet",
            Accent::Blue => "blue",
            Accent::Teal => "teal",
            Accent::Green => "green",
            Accent::Amber => "amber",
            Accent::Red => "red",
            Accent::Pink => "pink",
            Accent::Custom(_) => return None,
        })
    }

    /// Reads a preset name or `#rrggbb`, in either case. Anything else is
    /// `None`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if let Some(hex) = text.strip_prefix('#') {
            // `from_str_radix` accepts a leading `+`, so the digits are
            // checked first.
            if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return None;
            }
            let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
            return Some(Accent::Custom(Rgb8(byte(0)?, byte(2)?, byte(4)?)));
        }
        Some(match text.to_ascii_lowercase().as_str() {
            "terminal" => Accent::Terminal,
            "violet" => Accent::Violet,
            "blue" => Accent::Blue,
            "teal" => Accent::Teal,
            "green" => Accent::Green,
            "amber" => Accent::Amber,
            "red" => Accent::Red,
            "pink" => Accent::Pink,
            _ => return None,
        })
    }
}

impl Serialize for Accent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Accent::Custom(Rgb8(r, g, b)) => {
                serializer.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}"))
            }
            preset => serializer.serialize_str(preset.name().expect("a preset has a name")),
        }
    }
}

impl<'de> Deserialize<'de> for Accent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Accent::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("not an accent: {text:?}")))
    }
}

/// Which glyphs the interface draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IconSet {
    /// Nerd Font glyphs.
    #[default]
    Nerd,
    /// Characters every font has.
    Plain,
}

/// The interface keys Settings can set. `None` inherits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InterfacePrefs {
    /// Whether things move.
    pub motion: Option<bool>,
    /// Whether the pointer resting on a pane gives it the keyboard.
    pub focus_follows_pointer: Option<bool>,
    /// Whether the pointer over a window makes it the one in use.
    pub hover_claims_panes: Option<bool>,
}

/// How the interface looks. `None` inherits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppearancePrefs {
    /// The palette.
    pub theme: Option<ThemeChoice>,
    /// The selection colour.
    pub accent: Option<Accent>,
    /// The glyph set.
    pub icons: Option<IconSet>,
}

/// Everything `preferences.toml` can say.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Preferences {
    /// `[interface]`.
    pub interface: InterfacePrefs,
    /// `[appearance]`.
    pub appearance: AppearancePrefs,
}

/// Where an effective value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Dispatch's own default.
    BuiltIn,
    /// `config.toml`'s `[interface]`.
    ConfigFile,
    /// `preferences.toml`.
    Preferences,
}

/// A value, and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sourced<T> {
    /// The value in force.
    pub value: T,
    /// Which layer set it.
    pub source: Source,
}

/// Every preference as it applies, with its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effective {
    /// Whether things move.
    pub motion: Sourced<bool>,
    /// Whether the pointer resting on a pane gives it the keyboard.
    pub focus_follows_pointer: Sourced<bool>,
    /// Whether the pointer over a window makes it the one in use.
    pub hover_claims_panes: Sourced<bool>,
    /// The palette.
    pub theme: Sourced<ThemeChoice>,
    /// The selection colour.
    pub accent: Sourced<Accent>,
    /// The glyph set.
    pub icons: Sourced<IconSet>,
}

/// The part of Settings an apply covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// `focus_follows_pointer` and `hover_claims_panes`.
    Mouse,
    /// `theme`, `accent`, `icons`, and `motion`, which the spec files under
    /// Appearance.
    Appearance,
}

/// Why an apply wrote nothing.
#[derive(Debug)]
pub enum ApplyError {
    /// These fields changed on disk since Settings opened.
    Conflict(Vec<&'static str>),
    /// The file is not TOML. It is left as it is, for the user to mend.
    Unreadable(PathBuf),
    /// The file could not be read or written.
    Io(ConfigError),
}

/// `chosen`, or the built-in value when nothing was chosen. Appearance has no
/// `config.toml` layer.
fn layered<T>(chosen: Option<T>, built_in: T) -> Sourced<T> {
    match chosen {
        Some(value) => Sourced {
            value,
            source: Source::Preferences,
        },
        None => Sourced {
            value: built_in,
            source: Source::BuiltIn,
        },
    }
}

/// Loads `dir`'s preferences. No file is nothing chosen.
///
/// A value that cannot be read -- an unknown preset, a malformed hex, the
/// wrong type -- is logged and left unset, so it costs that one field rather
/// than the file. Only TOML that does not parse at all is an error.
pub fn load(dir: &Path) -> Result<Preferences, ConfigError> {
    let table: toml::Table = crate::store::read(dir, FILE)?;
    Ok(Preferences::from_table(&table))
}

/// [`load`], with a file that cannot be read taken as nothing chosen and
/// logged: starting must not stop over it.
#[must_use]
pub fn load_or_default(dir: &Path) -> Preferences {
    load(dir).unwrap_or_else(|error| {
        tracing::warn!(%error, "ignoring preferences that cannot be read");
        Preferences::default()
    })
}

/// The value in force for each preference, with its source.
#[must_use]
pub fn effective(
    config: &InterfaceConfig,
    interface_set: &InterfaceKeysPresent,
    prefs: &Preferences,
) -> Effective {
    let flag =
        |chosen: Option<bool>, written: bool, config_value: bool, built_in: bool| match chosen {
            Some(value) => Sourced {
                value,
                source: Source::Preferences,
            },
            None if written => Sourced {
                value: config_value,
                source: Source::ConfigFile,
            },
            None => Sourced {
                value: built_in,
                source: Source::BuiltIn,
            },
        };
    let built_in = InterfaceConfig::default();
    Effective {
        motion: flag(
            prefs.interface.motion,
            interface_set.motion,
            config.motion,
            built_in.motion,
        ),
        focus_follows_pointer: flag(
            prefs.interface.focus_follows_pointer,
            interface_set.focus_follows_pointer,
            config.focus_follows_pointer,
            built_in.focus_follows_pointer,
        ),
        hover_claims_panes: flag(
            prefs.interface.hover_claims_panes,
            interface_set.hover_claims_panes,
            config.hover_claims_panes,
            built_in.hover_claims_panes,
        ),
        theme: layered(prefs.appearance.theme, ThemeChoice::default()),
        accent: layered(prefs.appearance.accent, Accent::default()),
        icons: layered(prefs.appearance.icons, IconSet::default()),
    }
}

/// Writes the fields of `section` that `draft` changed from `base`.
///
/// Done under the file's lock, against the file as it is now. A field whose
/// value on disk is no longer `base`'s was changed by someone else since
/// Settings opened; any such field refuses the whole apply, so a half of it
/// is never written. A field set back to `None` is removed, and fields the
/// section did not change are left as they are, so unrelated edits merge.
pub fn apply(
    dir: &Path,
    section: Section,
    base: &Preferences,
    draft: &Preferences,
) -> Result<(), ApplyError> {
    let conflicts = crate::store::update(dir, FILE, |table: &mut toml::Table| {
        let mut conflicts = Vec::new();
        let mut edits: Vec<Edit> = Vec::new();

        let (interface, appearance) = (
            (&base.interface, &draft.interface),
            (&base.appearance, &draft.appearance),
        );
        let mut plan = Plan {
            table,
            conflicts: &mut conflicts,
            edits: &mut edits,
        };
        match section {
            Section::Mouse => {
                plan.field(
                    "interface",
                    "focus_follows_pointer",
                    &interface.0.focus_follows_pointer,
                    &interface.1.focus_follows_pointer,
                );
                plan.field(
                    "interface",
                    "hover_claims_panes",
                    &interface.0.hover_claims_panes,
                    &interface.1.hover_claims_panes,
                );
            }
            Section::Appearance => {
                plan.field(
                    "interface",
                    "motion",
                    &interface.0.motion,
                    &interface.1.motion,
                );
                plan.field(
                    "appearance",
                    "theme",
                    &appearance.0.theme,
                    &appearance.1.theme,
                );
                plan.field(
                    "appearance",
                    "accent",
                    &appearance.0.accent,
                    &appearance.1.accent,
                );
                plan.field(
                    "appearance",
                    "icons",
                    &appearance.0.icons,
                    &appearance.1.icons,
                );
            }
        }

        if !conflicts.is_empty() {
            return Ok((conflicts, false));
        }
        let changed = !edits.is_empty();
        for (group, key, value) in edits {
            match value {
                Some(value) => {
                    let entry = table
                        .entry(group)
                        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
                    // A hand edit may have left something else under the
                    // name; the table replaces it only because a field of
                    // ours is now written there.
                    if !entry.is_table() {
                        *entry = toml::Value::Table(toml::Table::new());
                    }
                    if let Some(inner) = entry.as_table_mut() {
                        inner.insert(key.to_string(), value);
                    }
                }
                None => {
                    if let Some(inner) = table.get_mut(group).and_then(toml::Value::as_table_mut) {
                        inner.remove(key);
                        if inner.is_empty() {
                            table.remove(group);
                        }
                    }
                }
            }
        }
        Ok((conflicts, changed))
    })
    .map_err(|error| match error {
        ConfigError::Toml { path, .. } => ApplyError::Unreadable(path),
        other => ApplyError::Io(other),
    })?;

    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(ApplyError::Conflict(conflicts))
    }
}

/// One change to the table: its group, its key, and the value or its removal.
type Edit = (&'static str, &'static str, Option<toml::Value>);

/// The fields of one apply, checked against the file before any is written.
struct Plan<'a> {
    table: &'a toml::Table,
    conflicts: &'a mut Vec<&'static str>,
    edits: &'a mut Vec<Edit>,
}

impl Plan<'_> {
    /// Plans `key` in `group` if the draft changed it, or notes a conflict if
    /// the file no longer holds what the draft started from.
    fn field<T>(
        &mut self,
        group: &'static str,
        key: &'static str,
        base: &Option<T>,
        draft: &Option<T>,
    ) where
        T: PartialEq + Serialize + DeserializeOwned,
    {
        if base == draft {
            return;
        }
        if read_field::<T>(self.table, group, key) != *base {
            self.conflicts.push(key);
            return;
        }
        let value = draft
            .as_ref()
            .map(|value| toml::Value::try_from(value).expect("a preference serialises"));
        self.edits.push((group, key, value));
    }
}

/// The value at `group.key`, or `None` when it is absent or cannot be read.
fn read_field<T: DeserializeOwned>(table: &toml::Table, group: &str, key: &str) -> Option<T> {
    let value = table.get(group)?.as_table()?.get(key)?;
    match value.clone().try_into() {
        Ok(value) => Some(value),
        Err(_) => {
            tracing::warn!(field = key, "ignoring a preference that cannot be read");
            None
        }
    }
}

impl Preferences {
    fn from_table(table: &toml::Table) -> Self {
        Self {
            interface: InterfacePrefs {
                motion: read_field(table, "interface", "motion"),
                focus_follows_pointer: read_field(table, "interface", "focus_follows_pointer"),
                hover_claims_panes: read_field(table, "interface", "hover_claims_panes"),
            },
            appearance: AppearancePrefs {
                theme: read_field(table, "appearance", "theme"),
                accent: read_field(table, "appearance", "accent"),
                icons: read_field(table, "appearance", "icons"),
            },
        }
    }
}

#[cfg(test)]
mod tests;
