//! `dispatch-tui`: rendering, input routing and keymap.

pub mod activity;
pub mod browser;
pub mod button;
pub mod glyphs;
pub mod input;
pub mod keymap;
pub mod menu;
pub mod motion;
pub mod pane;
pub mod picker;
pub mod prompt;
pub mod settings_form;
pub mod settings_view;
pub mod sidebar;
pub mod theme;

pub use input::{Action, Direction, InputRouter, KeyMode};
pub use keymap::{Chord, Command, Keymap};
pub use pane::PaneWidget;
pub use picker::{Item, Picker};
pub use prompt::{Note, Prompt};
pub use settings_form::{FormAction, SettingsForm};
pub use sidebar::{Sidebar, truncate};
pub use theme::{Chrome, Theme};
