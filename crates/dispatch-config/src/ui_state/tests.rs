//! Tests for the saved interface state.

use super::*;
use crate::testing::TempDir;

#[test]
fn nothing_saved_is_the_defaults() {
    let dir = TempDir::new("ui-none");
    assert_eq!(load(dir.path()), UiState::default());
    assert_eq!(UiState::default().sidebar_width, DEFAULT_SIDEBAR);
    assert!(!UiState::default().sidebar_collapsed);
}

#[test]
fn what_is_saved_comes_back() {
    let dir = TempDir::new("ui-saved");
    let state = UiState {
        sidebar_width: 41,
        sidebar_collapsed: true,
    };
    save(dir.path(), state).expect("saved");
    assert_eq!(load(dir.path()), state);
}

#[test]
fn a_hand_edited_width_is_brought_into_range_or_ignored() {
    let dir = TempDir::new("ui-edited");
    for (text, width) in [
        ("sidebar_width = 500", MAX_SIDEBAR),
        ("sidebar_width = 3", MIN_SIDEBAR),
        ("sidebar_width = \"wide\"", DEFAULT_SIDEBAR),
        ("sidebar_width = -1", DEFAULT_SIDEBAR),
        ("not toml at all [", DEFAULT_SIDEBAR),
    ] {
        dir.write(FILE, text);
        assert_eq!(load(dir.path()).sidebar_width, width, "{text}");
    }
}
