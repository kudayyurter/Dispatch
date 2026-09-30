//! How the interface was left: the sidebar's width, and whether it was
//! folded away.
//!
//! A file of its own beside `projects.toml`, as `harness-settings.toml` is:
//! `config.toml` is written by hand and would lose its comments to a rewrite.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ConfigError;

/// The file, inside the configuration directory.
pub const FILE: &str = "ui.toml";

/// The narrowest the sidebar can be made: an icon, a short name and a state.
pub const MIN_SIDEBAR: u16 = 20;

/// The widest: beyond this it is taking the panes' room for blank.
pub const MAX_SIDEBAR: u16 = 60;

/// Its width until the user changes it.
pub const DEFAULT_SIDEBAR: u16 = 34;

/// What is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    /// The sidebar's width, in columns.
    pub sidebar_width: u16,
    /// Whether the sidebar is folded away.
    pub sidebar_collapsed: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            sidebar_width: DEFAULT_SIDEBAR,
            sidebar_collapsed: false,
        }
    }
}

/// What was kept in `dir`, its width brought into range. A file that cannot
/// be read is logged and taken as nothing kept: losing a width is not worth
/// stopping over.
#[must_use]
pub fn load(dir: &Path) -> UiState {
    match crate::store::read::<UiState>(dir, FILE) {
        Ok(state) => UiState {
            sidebar_width: state.sidebar_width.clamp(MIN_SIDEBAR, MAX_SIDEBAR),
            ..state
        },
        Err(error) => {
            tracing::warn!(%error, "ignoring a ui.toml that cannot be read");
            UiState::default()
        }
    }
}

/// Keeps `state` in `dir`.
pub fn save(dir: &Path, state: UiState) -> Result<(), ConfigError> {
    crate::store::update(dir, FILE, |kept: &mut UiState| {
        let changed = *kept != state;
        *kept = state;
        Ok(((), changed))
    })
}

#[cfg(test)]
mod tests;
