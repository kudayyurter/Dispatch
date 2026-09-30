//! Tests for the dialog buttons.

use super::*;
use ratatui::layout::Rect;

fn pair() -> Vec<Button> {
    vec![
        Button {
            id: ButtonId::Cancel,
            label: "Cancel",
            default: false,
        },
        Button {
            id: ButtonId::Open,
            label: "Open",
            default: true,
        },
    ]
}

#[test]
fn buttons_sit_at_the_right_end_padded_with_a_gap_between() {
    let placed = lay_out(&pair(), Rect::new(0, 10, 40, 1));
    // "[ Cancel ]" is 10 wide, "[ Open ]" 8, one blank between, one before the edge.
    assert_eq!(
        placed,
        vec![
            (Rect::new(20, 10, 10, 1), ButtonId::Cancel),
            (Rect::new(31, 10, 8, 1), ButtonId::Open),
        ]
    );
}

#[test]
fn buttons_that_do_not_fit_are_all_dropped() {
    assert!(lay_out(&pair(), Rect::new(0, 0, 18, 1)).is_empty());
}

#[test]
fn buttons_that_just_fit_are_kept() {
    assert_eq!(lay_out(&pair(), Rect::new(0, 0, 20, 1)).len(), 2);
}

#[test]
fn no_buttons_and_no_row_place_nothing() {
    assert!(lay_out(&[], Rect::new(0, 0, 40, 1)).is_empty());
    assert!(lay_out(&pair(), Rect::new(0, 0, 40, 0)).is_empty());
}

#[test]
fn the_default_is_drawn_in_the_accent_and_a_held_one_in_the_selection() {
    let chrome = crate::theme::loud_chrome();
    let area = Rect::new(0, 0, 40, 1);
    let placed = lay_out(&pair(), area);

    let mut buf = Buffer::empty(area);
    render(&mut buf, &pair(), &placed, &chrome, None);
    assert_eq!(buf.cell((22, 0)).map(|c| c.symbol()), Some("C"));
    assert_eq!(buf.cell((22, 0)).map(|c| c.fg), chrome.text.fg);
    assert_eq!(buf.cell((33, 0)).map(|c| c.fg), chrome.accent.fg);

    let mut held = Buffer::empty(area);
    render(&mut held, &pair(), &placed, &chrome, Some(ButtonId::Cancel));
    assert_eq!(held.cell((22, 0)).map(|c| c.bg), chrome.selection.bg);
    assert_eq!(held.cell((33, 0)).map(|c| c.fg), chrome.accent.fg);
}
