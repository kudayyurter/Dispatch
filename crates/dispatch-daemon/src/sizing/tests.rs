//! Tests for which window's size a pane takes.

use super::*;

fn size(cols: u16, rows: u16) -> Size {
    Size::new(cols, rows)
}

#[test]
fn a_pane_nobody_asked_about_has_no_wanted_size() {
    assert_eq!(Sizes::default().wanted(PaneId::new()), None);
}

#[test]
fn the_most_recently_used_window_decides() {
    let pane = PaneId::new();
    let mut sizes = Sizes::default();
    sizes.touch(1);
    sizes.ask(1, pane, size(100, 30));
    sizes.touch(2);
    sizes.ask(2, pane, size(60, 20));

    assert_eq!(sizes.wanted(pane), Some(size(60, 20)));

    sizes.touch(1);
    assert_eq!(sizes.wanted(pane), Some(size(100, 30)));
}

#[test]
fn touching_a_window_names_the_panes_it_sizes_unless_it_was_already_in_use() {
    let (first, second) = (PaneId::new(), PaneId::new());
    let mut sizes = Sizes::default();
    sizes.touch(1);
    sizes.ask(1, first, size(100, 30));
    sizes.ask(1, second, size(100, 30));
    sizes.touch(2);

    let mut moved = sizes.touch(1);
    moved.sort_by_key(|pane| pane.to_string());
    let mut expected = vec![first, second];
    expected.sort_by_key(|pane| pane.to_string());
    assert_eq!(moved, expected);

    assert!(
        sizes.touch(1).is_empty(),
        "already the window in use: nothing moves"
    );
}

#[test]
fn a_window_that_hides_a_pane_stops_sizing_it() {
    let pane = PaneId::new();
    let mut sizes = Sizes::default();
    sizes.touch(1);
    sizes.ask(1, pane, size(100, 30));
    sizes.touch(2);
    sizes.ask(2, pane, size(60, 20));

    sizes.hide(2, pane);

    assert_eq!(sizes.wanted(pane), Some(size(100, 30)));
}

#[test]
fn a_window_that_leaves_hands_its_panes_on() {
    let pane = PaneId::new();
    let mut sizes = Sizes::default();
    sizes.touch(1);
    sizes.ask(1, pane, size(100, 30));
    sizes.touch(2);
    sizes.ask(2, pane, size(60, 20));

    assert_eq!(sizes.forget_client(2), vec![pane]);
    assert_eq!(sizes.wanted(pane), Some(size(100, 30)));
    assert!(sizes.forget_client(2).is_empty(), "forgotten once");
}

#[test]
fn a_closed_pane_is_forgotten() {
    let pane = PaneId::new();
    let mut sizes = Sizes::default();
    sizes.ask(1, pane, size(100, 30));

    sizes.forget_pane(pane);

    assert_eq!(sizes.wanted(pane), None);
}

#[test]
fn a_window_never_used_loses_to_one_that_was() {
    // A delegate connection is never ranked; its ask, if it sent one, must
    // not beat a window in use.
    let pane = PaneId::new();
    let mut sizes = Sizes::default();
    sizes.touch(1);
    sizes.ask(1, pane, size(100, 30));
    sizes.ask(9, pane, size(40, 10));

    assert_eq!(sizes.wanted(pane), Some(size(100, 30)));
}

#[test]
fn between_windows_never_used_the_later_one_decides() {
    let pane = PaneId::new();
    let mut sizes = Sizes::default();
    sizes.ask(1, pane, size(100, 30));
    sizes.ask(2, pane, size(60, 20));

    assert_eq!(sizes.wanted(pane), Some(size(60, 20)));
}
