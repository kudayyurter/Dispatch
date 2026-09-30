//! Tests for the list picker.

use super::*;

fn picker() -> Picker {
    Picker::new(
        "Harness",
        vec![
            Item::new("claude", "Claude Code"),
            Item::new("codex", "Codex").with_detail("codex"),
            Item::new("agy", "agy"),
        ],
    )
}

fn render(picker: &Picker, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    picker.render(area, &mut buf);
    buf
}

/// The bar a chosen row is drawn on when nothing set the chrome.
fn selection_bg() -> ratatui::style::Color {
    crate::theme::Chrome::default()
        .selection
        .bg
        .expect("a background")
}

fn text(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .filter_map(|x| buf.cell((x, y)))
                .map(|c| c.symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_first_item_starts_selected() {
    let picker = picker();
    assert_eq!(picker.selected_index(), 0);
    assert_eq!(picker.selected().expect("an item").id, "claude");
}

#[test]
fn moving_down_advances_the_selection() {
    let mut picker = picker();
    picker.next();
    assert_eq!(picker.selected().expect("an item").id, "codex");
}

#[test]
fn moving_past_the_end_wraps_to_the_start() {
    // These lists are short; stopping at the bottom is more annoying than
    // useful.
    let mut picker = picker();
    picker.next();
    picker.next();
    picker.next();
    assert_eq!(picker.selected().expect("an item").id, "claude");
}

#[test]
fn moving_up_from_the_start_wraps_to_the_end() {
    let mut picker = picker();
    picker.previous();
    assert_eq!(picker.selected().expect("an item").id, "agy");
}

#[test]
fn an_empty_picker_selects_nothing_and_does_not_panic() {
    let mut picker = Picker::new("Empty", Vec::new());

    assert!(picker.is_empty());
    assert_eq!(picker.selected(), None);

    picker.next();
    picker.previous();

    assert_eq!(picker.selected(), None);
}

#[test]
fn the_title_and_every_label_are_drawn() {
    let buf = render(&picker(), 40, 10);
    let text = text(&buf);

    assert!(text.contains("Harness"), "{text}");
    assert!(text.contains("Claude Code"), "{text}");
    assert!(text.contains("Codex"), "{text}");
    assert!(text.contains("agy"), "{text}");
}

#[test]
fn the_selected_row_is_highlighted_across_its_width() {
    // A highlight only behind the text reads as an artefact rather than a
    // selection.
    let buf = render(&picker(), 40, 10);

    // Find the row holding the first label.
    let row = (0..buf.area.height)
        .find(|y| {
            (0..buf.area.width)
                .filter_map(|x| buf.cell((x, *y)))
                .map(|c| c.symbol())
                .collect::<String>()
                .contains("Claude Code")
        })
        .expect("the first label is drawn");

    let highlighted = (1..buf.area.width - 1)
        .filter(|x| buf.cell((*x, row)).expect("cell exists").bg == selection_bg())
        .count();

    assert!(
        highlighted > "Claude Code".len(),
        "the highlight should span the row, covered {highlighted} cells"
    );
}

#[test]
fn an_unselected_row_is_not_highlighted() {
    let buf = render(&picker(), 40, 10);
    let text_rows: Vec<String> = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .filter_map(|x| buf.cell((x, y)))
                .map(|c| c.symbol())
                .collect()
        })
        .collect();

    let row = text_rows
        .iter()
        .position(|r| r.contains("Codex"))
        .expect("the second label is drawn") as u16;

    assert_ne!(
        buf.cell((2, row)).expect("cell exists").bg,
        selection_bg(),
        "only the selected row should be highlighted"
    );
}

#[test]
fn detail_is_drawn_after_the_label() {
    let buf = render(&picker(), 40, 10);
    let text = text(&buf);
    assert!(text.contains("Codex"), "{text}");
    assert!(text.contains("codex"), "{text}");
}

#[test]
fn the_picker_is_centred() {
    let buf = render(&picker(), 60, 20);

    // The border should not touch the edges of a much larger area.
    let top_row: String = (0..buf.area.width)
        .filter_map(|x| buf.cell((x, 0)))
        .map(|c| c.symbol())
        .collect();

    assert_eq!(
        top_row.trim(),
        "",
        "the first row should be clear of the picker"
    );
}

#[test]
fn an_empty_picker_says_so() {
    let buf = render(&Picker::new("Nothing", Vec::new()), 40, 10);
    assert!(text(&buf).contains("nothing to choose"));
}

#[test]
fn a_tiny_area_paints_nothing_rather_than_panicking() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
    picker().render(Rect::new(0, 0, 3, 2), &mut buf);
    assert_eq!(text(&buf).trim(), "");
}

#[test]
fn a_list_taller_than_the_box_scrolls_to_keep_the_selection_visible() {
    let items: Vec<Item> = (0..20)
        .map(|i| Item::new(format!("id{i}"), format!("item-{i}")))
        .collect();
    let mut picker = Picker::new("Long", items);

    for _ in 0..15 {
        picker.next();
    }

    let buf = render(&picker, 40, 8);
    assert!(
        text(&buf).contains("item-15"),
        "the selected row must stay visible: {}",
        text(&buf)
    );
}

#[test]
fn a_hint_is_drawn_on_the_bottom_border() {
    let picker = picker().with_hint("Enter open · e settings");
    let text = text(&render(&picker, 60, 10));

    let line = text
        .lines()
        .find(|line| line.contains("Enter open · e settings"))
        .expect("the hint is drawn");
    assert!(line.contains('└'), "on the bottom border: {line:?}");
    assert_eq!(picker.hint(), Some("Enter open · e settings"));
}

#[test]
fn a_hint_widens_a_picker_to_fit() {
    let picker = Picker::new("P", vec![Item::new("a", "a")])
        .with_hint("a hint much wider than anything in the list");

    assert!(text(&render(&picker, 80, 10)).contains("a hint much wider than anything in the list"));
}

#[test]
fn a_picker_has_no_hint_unless_given_one() {
    assert_eq!(picker().hint(), None);
}

#[test]
fn selecting_by_id_moves_the_highlight() {
    let mut picker = picker();

    picker.select("agy");
    assert_eq!(picker.selected().expect("an item").id, "agy");

    picker.select("nothing-by-that-name");
    assert_eq!(
        picker.selected().expect("an item").id,
        "agy",
        "an unknown id leaves it where it was"
    );
}

#[test]
fn a_picker_draws_at_any_size_without_panicking() {
    // `clamp(20, width)` panics when the width is under 20, and the early
    // return only covered widths under 4.
    let picker = picker().with_hint("↑↓ choose  Enter open");
    for width in 1..=25 {
        for height in 1..=6 {
            render(&picker, width, height);
        }
    }
}

#[test]
fn the_picker_draws_only_in_its_chrome() {
    let mut picker = picker();
    let chrome = crate::theme::loud_chrome();
    picker.set_chrome(chrome);
    let buf = render(&picker, 40, 8);

    crate::theme::assert_no_fixed_colours(&buf);
    // The first row is selected: its bar is the chrome's selection.
    let y = (0..buf.area.height)
        .find(|&y| {
            text(&buf)
                .lines()
                .nth(y as usize)
                .is_some_and(|l| l.contains("Claude Code"))
        })
        .expect("the first row is drawn");
    let cell = buf.cell((buf.area.width / 2, y)).expect("on screen");
    assert_eq!(cell.bg, chrome.selection.bg.expect("a background"));
}

#[test]
fn a_filter_narrows_the_rows_and_the_selection_follows() {
    let mut picker = picker().with_filter();
    for c in "codex".chars() {
        picker.push_filter(c);
    }
    assert_eq!(picker.filter(), Some("codex"));
    assert_eq!(
        picker.selected().map(|item| item.id.as_str()),
        Some("codex")
    );
    picker.next();
    assert_eq!(
        picker.selected().map(|item| item.id.as_str()),
        Some("codex"),
        "one row left"
    );

    picker.pop_filter();
    picker.pop_filter();
    picker.pop_filter();
    assert_eq!(
        picker.selected().map(|item| item.id.as_str()),
        Some("claude")
    );
}

#[test]
fn a_filter_matching_nothing_says_so_and_chooses_nothing() {
    let mut picker = picker().with_filter();
    picker.push_filter('z');
    assert!(picker.selected().is_none());
    assert!(text(&render(&picker, 40, 8)).contains("nothing to choose"));
}

#[test]
fn a_filter_is_drawn_above_the_rows() {
    let mut picker = picker().with_filter();
    picker.push_filter('a');
    let drawn = text(&render(&picker, 40, 8));
    let lines: Vec<&str> = drawn.lines().collect();
    let filter = lines
        .iter()
        .position(|l| l.contains("> a"))
        .expect("the filter line");
    let row = lines.iter().position(|l| l.contains("agy")).expect("a row");
    assert!(filter < row);
}

fn open_cancel() -> Vec<crate::button::Button> {
    use crate::button::{Button, ButtonId};
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

/// What is drawn inside `rect`.
fn drawn_in(buf: &Buffer, rect: Rect) -> String {
    (rect.x..rect.right())
        .filter_map(|x| buf.cell((x, rect.y)))
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn buttons_are_laid_out_and_drawn_at_a_comfortable_size() {
    let picker = picker().with_buttons(open_cancel());
    let area = Rect::new(0, 0, 60, 12);
    let layout = picker.layout(area);
    let mut buf = Buffer::empty(area);
    (&picker).render(area, &mut buf);

    assert_eq!(layout.buttons.len(), 2);
    for (rect, id) in &layout.buttons {
        let label = if *id == crate::button::ButtonId::Open {
            "[ Open ]"
        } else {
            "[ Cancel ]"
        };
        assert_eq!(drawn_in(&buf, *rect), label);
        assert!(rect.bottom() < layout.rect.bottom(), "inside the border");
    }
}

#[test]
fn buttons_never_overlap_the_list() {
    let picker = picker().with_buttons(open_cancel());
    let layout = picker.layout(Rect::new(0, 0, 60, 12));
    let foot = layout.buttons[0].0.y;

    assert_eq!(layout.rows.len(), 3);
    assert!(layout.rows.iter().all(|(row, _)| row.y < foot));
}

#[test]
fn buttons_that_do_not_fit_are_dropped_and_the_box_keeps_its_old_shape() {
    let bare = picker();
    let with = picker().with_buttons(open_cancel());
    // Too short for the list and a row of buttons.
    let area = Rect::new(0, 0, 60, 3);
    assert!(with.layout(area).buttons.is_empty());
    assert_eq!(with.layout(area).rect, bare.layout(area).rect);
}

#[test]
fn a_picker_without_buttons_keeps_the_shape_it_had() {
    let area = Rect::new(0, 0, 60, 12);
    let bare = picker().layout(area);
    let with = picker().with_buttons(open_cancel()).layout(area);

    assert!(bare.buttons.is_empty());
    assert_eq!(with.rect.height, bare.rect.height + 1);
}

#[test]
fn rows_are_where_each_shown_row_is_drawn() {
    let picker = picker().with_buttons(open_cancel());
    let area = Rect::new(0, 0, 60, 12);
    let layout = picker.layout(area);
    let mut buf = Buffer::empty(area);
    (&picker).render(area, &mut buf);

    for ((rect, index), label) in layout.rows.iter().zip(["Claude Code", "Codex", "agy"]) {
        assert_eq!(rect.height, 1);
        assert_eq!(
            *index,
            layout.rows.iter().position(|r| r.0 == *rect).unwrap()
        );
        assert!(drawn_in(&buf, *rect).contains(label), "{label}");
    }
}

#[test]
fn rows_index_the_filtered_list() {
    let mut picker = picker().with_filter().with_buttons(open_cancel());
    picker.push_filter('x');
    let layout = picker.layout(Rect::new(0, 0, 60, 12));
    assert_eq!(layout.rows.len(), 1, "only Codex has an x");
    assert_eq!(layout.rows[0].1, 0);
}

#[test]
fn selecting_by_shown_index_moves_the_highlight_and_ignores_the_out_of_range() {
    let mut picker = picker();
    picker.select_shown(2);
    assert_eq!(picker.selected_index(), 2);
    picker.select_shown(9);
    assert_eq!(picker.selected_index(), 2);
}

#[test]
fn hover_is_underlined_and_not_a_bar_and_never_on_the_chosen_row() {
    let mut picker = picker();
    picker.set_hovered(Some(1));
    let area = Rect::new(0, 0, 40, 8);
    let layout = picker.layout(area);
    let mut buf = Buffer::empty(area);
    (&picker).render(area, &mut buf);

    let (hovered, _) = layout.rows[1];
    let cell = buf.cell((hovered.x + 1, hovered.y)).unwrap();
    assert!(cell.modifier.contains(ratatui::style::Modifier::UNDERLINED));
    assert_ne!(cell.bg, selection_bg(), "not the selection's bar");

    picker.set_hovered(Some(0));
    let mut buf = Buffer::empty(area);
    (&picker).render(area, &mut buf);
    let (chosen, _) = layout.rows[0];
    let cell = buf.cell((chosen.x + 1, chosen.y)).unwrap();
    assert!(!cell.modifier.contains(ratatui::style::Modifier::UNDERLINED));
}

#[test]
fn a_pressed_button_is_drawn_in_the_selection() {
    let mut picker = picker().with_buttons(open_cancel());
    picker.set_pressed(Some(crate::button::ButtonId::Open));
    let area = Rect::new(0, 0, 60, 12);
    let layout = picker.layout(area);
    let mut buf = Buffer::empty(area);
    (&picker).render(area, &mut buf);

    let open = layout
        .buttons
        .iter()
        .find(|b| b.1 == crate::button::ButtonId::Open)
        .unwrap()
        .0;
    assert_eq!(buf.cell((open.x + 2, open.y)).unwrap().bg, selection_bg());
}

#[test]
fn a_picker_with_buttons_draws_at_any_size_without_panicking() {
    let picker = picker()
        .with_hint("↑↓ choose  Enter open")
        .with_filter()
        .with_buttons(open_cancel());
    for width in 1..=30 {
        for height in 1..=10 {
            render(&picker, width, height);
            let _ = picker.layout(Rect::new(0, 0, width, height));
        }
    }
}

#[test]
fn clicking_a_visible_row_of_a_scrolled_list_does_not_move_the_list() {
    let items: Vec<Item> = (0..20)
        .map(|i| Item::new(format!("id{i}"), format!("item-{i}")))
        .collect();
    let mut picker = Picker::new("Long", items);
    let area = Rect::new(0, 0, 40, 8);
    for _ in 0..15 {
        picker.next();
    }
    let before = picker.layout(area).rows;
    let (rect, index) = before[1];

    picker.select_shown(index);

    let after = picker.layout(area).rows;
    assert_eq!(after, before, "the same rows are in the same places");
    assert_eq!(picker.selected_index(), index);
    let buf = render(&picker, 40, 8);
    assert!(
        drawn_in(&buf, rect).contains(&format!("item-{index}")),
        "the row clicked still holds its label"
    );
}

#[test]
fn moving_up_inside_the_visible_rows_keeps_the_list_still_until_it_leaves_them() {
    let items: Vec<Item> = (0..20)
        .map(|i| Item::new(format!("id{i}"), format!("item-{i}")))
        .collect();
    let mut picker = Picker::new("Long", items);
    let area = Rect::new(0, 0, 40, 8);
    for _ in 0..15 {
        picker.next();
    }
    let first = picker.layout(area).rows[0].1;

    picker.previous();
    assert_eq!(picker.layout(area).rows[0].1, first);
    for _ in 0..14 {
        picker.previous();
    }
    assert_eq!(picker.layout(area).rows[0].1, 0, "it follows the selection");
}
