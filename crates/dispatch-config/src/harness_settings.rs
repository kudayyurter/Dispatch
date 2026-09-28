//! The defaults a user saved from the settings popup.
//!
//! A file of its own, beside `projects.toml`: not `config.toml`, which is
//! written by hand and would lose its comments to a rewrite, and not the
//! harness files, which an edit would cut off from upgrades.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Choices, ConfigError, HarnessDef};

/// The file, inside the configuration directory.
pub const FILE: &str = "harness-settings.toml";

/// One saved value: a flag as a TOML boolean, anything else as a string, or
/// -- a hand edit's mistake, such as `effort = 3` -- whatever TOML holds
/// there instead, so it costs only that value rather than the whole file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum Stored {
    Flag(bool),
    Text(String),
    Other(toml::Value),
}

impl Stored {
    /// The value as [`Choices`] carries it, or `None` for a value that is
    /// neither a flag nor text.
    fn to_choice(&self) -> Option<String> {
        match self {
            Stored::Flag(on) => Some(on.to_string()),
            Stored::Text(text) => Some(text.clone()),
            Stored::Other(_) => None,
        }
    }
}

/// The file's shape: each harness's saved values, by harness id and then by
/// setting key.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
struct Saved(BTreeMap<String, BTreeMap<String, Stored>>);

/// What the user saved for `harness`. No file is nothing saved.
///
/// A key whose value is neither a flag nor text -- a hand edit's mistake,
/// such as `effort = 3` -- is left out and logged, rather than failing the
/// whole file: the rest of this harness's values, and every other harness's,
/// still load.
pub fn load(dir: &Path, harness: &str) -> Result<Choices, ConfigError> {
    let saved: Saved = crate::store::read(dir, FILE)?;
    Ok(saved
        .0
        .get(harness)
        .map(|values| {
            values
                .iter()
                .filter_map(|(key, value)| match value.to_choice() {
                    Some(choice) => Some((key.clone(), choice)),
                    None => {
                        tracing::warn!(
                            harness,
                            key = key.as_str(),
                            "ignoring a saved harness setting of the wrong type"
                        );
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Whether `dir`'s saved-settings file can be read at all. No file is fine;
/// a file that does not parse as TOML is not, and `s` will fail the same
/// way, but silently until then -- this is what makes it visible sooner.
pub fn check(dir: &Path) -> Result<(), ConfigError> {
    crate::store::read::<Saved>(dir, FILE)?;
    Ok(())
}

/// [`load`], with a file that cannot be read taken as nothing saved and
/// logged: opening a pane must not stop over it.
#[must_use]
pub fn load_or_empty(dir: &Path, harness: &str) -> Choices {
    load(dir, harness).unwrap_or_else(|error| {
        tracing::warn!(%error, "ignoring saved harness settings that cannot be read");
        Choices::new()
    })
}

/// Saves `values` as `def`'s defaults.
///
/// Only what differs from the harness file's own default is written, and a
/// value equal to it removes its key: a default Dispatch later ships then
/// reaches every setting the user never changed. A key `def` has no setting
/// for, or a value that setting cannot take, is not written. A file that
/// cannot be read is refused, not overwritten: the user may want it back.
pub fn save(dir: &Path, def: &HarnessDef, values: &Choices) -> Result<(), ConfigError> {
    crate::store::update(dir, FILE, |saved: &mut Saved| {
        let before = saved.0.get(&def.id).cloned();
        let table = saved.0.entry(def.id.clone()).or_default();

        for setting in &def.settings {
            let Some(value) = values.get(&setting.key) else {
                continue;
            };
            if setting.check(value).is_err() {
                continue;
            }

            if *value == setting.file_default() {
                table.remove(&setting.key);
            } else if setting.is_flag() {
                table.insert(setting.key.clone(), Stored::Flag(value == "true"));
            } else {
                table.insert(setting.key.clone(), Stored::Text(value.clone()));
            }
        }

        if table.is_empty() {
            saved.0.remove(&def.id);
        }
        let changed = saved.0.get(&def.id).cloned() != before;
        Ok(((), changed))
    })
}

#[cfg(test)]
mod tests;
