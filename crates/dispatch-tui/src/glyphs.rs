//! The glyphs the sidebar and the tab chips draw, as one table.
//!
//! Two sets: the Nerd Font one the interface was drawn with, and a plain one
//! of single ASCII characters for a terminal whose font has no private-use
//! glyphs. Keeping both in one struct means a drawing routine takes a
//! reference and never has to know which it was given.

use dispatch_config::HarnessDef;
use dispatch_config::harness::DEFAULT_ICON;

use crate::sidebar;

/// Every glyph drawn for a pane, a project or a row's twisty.
///
/// The braille spinner is not here: it is in the Unicode block every
/// monospace font draws, so both sets share it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyphs {
    /// A pane whose process is starting.
    pub starting: &'static str,
    /// A pane that is working, drawn when motion is off.
    pub running: &'static str,
    /// A pane waiting for its next prompt.
    pub idle: &'static str,
    /// A pane waiting on the user.
    pub blocked: &'static str,
    /// A pane that exited cleanly.
    pub done: &'static str,
    /// A pane that exited with an error.
    pub failed: &'static str,
    /// A closed pane's tombstone.
    pub closed: &'static str,
    /// A pane that finished while the user was elsewhere.
    pub unseen: &'static str,
    /// The twisty of an expanded row.
    pub open: &'static str,
    /// The twisty of a collapsed row.
    pub shut: &'static str,
    /// The twisty column of a row with nothing under it.
    pub leaf: &'static str,
    /// A git repository.
    pub repository: &'static str,
    /// A plain directory.
    pub folder: &'static str,
    /// A harness that names no icon of its own.
    pub default_icon: &'static str,
    /// Whether this is the Nerd Font set, whose harness icons come from the
    /// harness definitions.
    pub nerd: bool,
}

impl Glyphs {
    /// The set drawn before there was a choice, taken from the sidebar's own
    /// constants so the two cannot drift apart.
    pub const NERD: Glyphs = Glyphs {
        starting: sidebar::STARTING,
        running: sidebar::RUNNING,
        idle: sidebar::IDLE,
        blocked: sidebar::BLOCKED,
        done: sidebar::DONE,
        failed: sidebar::FAILED,
        closed: sidebar::CLOSED,
        unseen: sidebar::UNSEEN,
        open: sidebar::OPEN,
        shut: sidebar::SHUT,
        leaf: sidebar::LEAF,
        repository: sidebar::REPOSITORY,
        folder: sidebar::SHUT_FOLDER,
        default_icon: DEFAULT_ICON,
        nerd: true,
    };

    /// One ASCII character each, for a font without Nerd Font glyphs.
    pub const PLAIN: Glyphs = Glyphs {
        starting: "~",
        running: ">",
        idle: "-",
        blocked: "!",
        done: "+",
        failed: "x",
        closed: "#",
        unseen: "*",
        open: "v",
        shut: ">",
        leaf: " ",
        repository: "@",
        folder: "/",
        default_icon: "*",
        nerd: false,
    };

    /// The mark drawn beside a harness's panes.
    ///
    /// The Nerd set uses the harness's own icon. The plain set cannot draw
    /// those, so it uses the first letter of the display name, which tells
    /// harnesses apart well enough in one cell.
    #[must_use]
    pub fn harness_icon(&self, def: Option<&HarnessDef>, display: &str) -> String {
        if self.nerd {
            return def.map_or(DEFAULT_ICON, HarnessDef::icon).to_owned();
        }

        display.chars().next().map_or_else(
            || self.default_icon.to_owned(),
            |first| first.to_uppercase().collect(),
        )
    }
}

impl Default for Glyphs {
    fn default() -> Self {
        Self::NERD
    }
}

#[cfg(test)]
mod tests;
