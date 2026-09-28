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

/// One saved value: a flag as a TOML boolean, anything else as a string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum Stored {
    Flag(bool),
    Text(String),
}

impl Stored {
    /// The value as [`Choices`] carries it.
    fn to_choice(&self) -> String {
        match self {
            Stored::Flag(on) => on.to_string(),
            Stored::Text(text) => text.clone(),
        }
    }
}

/// The file's shape: each harness's saved values, by harness id and then by
/// setting key.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
struct Saved(BTreeMap<String, BTreeMap<String, Stored>>);

/// What the user saved for `harness`. No file is nothing saved.
pub fn load(dir: &Path, harness: &str) -> Result<Choices, ConfigError> {
    let saved: Saved = crate::store::read(dir, FILE)?;
    Ok(saved
        .0
        .get(harness)
        .map(|values| {
            values
                .iter()
                .map(|(key, value)| (key.clone(), value.to_choice()))
                .collect()
        })
        .unwrap_or_default())
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
