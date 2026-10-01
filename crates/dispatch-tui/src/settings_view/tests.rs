use super::*;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

fn view() -> SettingsView {
    let field = |id, label: &str, category| FieldView {
        id,
        label: label.into(),
        description: format!("about {label}"),
        category,
        kind: FieldKind::Choice(vec!["One".into(), "Two".into()]),
        value: "One".into(),
        source: "Built-in".into(),
        applies: "Applies now",
        changed: false,
        conflict: false,
        resettable: false,
    };
    SettingsView::new(
        vec![
            "Mouse & layout".into(),
            "Appearance".into(),
            "Advanced".into(),
        ],
        vec![
            field("focus", "Focus follows pointer", 0),
            field("theme", "Theme", 1),
            field("icons", "Icons", 1),
        ],
    )
}

fn draw(view: &SettingsView, w: u16, h: u16) -> (Buffer, SettingsLayout) {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    (buf, view.layout(area))
}

#[test]
fn a_large_window_centres_a_capped_box() {
    let (_, layout) = draw(&view(), 160, 50);
    assert_eq!((layout.rect.width, layout.rect.height), (110, 34));
    assert_eq!(layout.rect.x, (160 - 110) / 2);
    assert!(!layout.compact && !layout.too_small);
}

#[test]
fn a_smaller_window_uses_all_of_it_and_a_narrow_one_turns_compact() {
    let (_, layout) = draw(&view(), 90, 24);
    assert_eq!(layout.rect, Rect::new(0, 0, 90, 24));
    let (_, layout) = draw(&view(), 60, 18);
    assert!(
        layout.compact,
        "under 72 columns the categories become a picker"
    );
}

#[test]
fn a_tiny_window_shows_only_how_to_close() {
    let (buf, layout) = draw(&view(), 30, 8);
    assert!(layout.too_small);
    assert!(
        !layout.close.is_empty(),
        "the close control is always there"
    );
    let text: String = buf.content().iter().map(|c| c.symbol()).collect();
    assert!(text.contains("larger"), "{text}");
}

#[test]
fn it_never_panics_and_close_is_always_reachable() {
    for (w, h) in [
        (120, 40),
        (100, 28),
        (80, 24),
        (60, 18),
        (40, 10),
        (20, 6),
        (1, 1),
    ] {
        let (_, layout) = draw(&view(), w, h);
        if w >= 4 && h >= 3 {
            assert!(!layout.close.is_empty(), "{w}x{h}");
        }
    }
}

#[test]
fn search_finds_across_categories_and_offers_clear_when_empty() {
    let mut view = view();
    for c in "icon".chars() {
        view.push_search(c);
    }
    assert_eq!(view.selected_field().map(|f| f.id), Some("icons"));
    for c in "zzz".chars() {
        view.push_search(c);
    }
    let (buf, layout) = draw(&view, 120, 40);
    let text: String = buf.content().iter().map(|c| c.symbol()).collect();
    assert!(text.contains("No settings match"));
    assert!(
        layout
            .buttons
            .iter()
            .any(|(_, id)| *id == ButtonId::ClearSearch)
    );
}

#[test]
fn tab_walks_search_categories_fields_buttons_and_back() {
    let mut view = view();
    assert_eq!(view.focus(), Focus::Categories);
    view.focus_next();
    assert_eq!(view.focus(), Focus::Fields);
    view.focus_next();
    assert_eq!(view.focus(), Focus::Buttons);
    view.focus_next();
    assert_eq!(view.focus(), Focus::Search);
    view.focus_previous();
    assert_eq!(view.focus(), Focus::Buttons);
}

#[test]
fn the_layout_matches_what_is_drawn() {
    let view = view();
    let (buf, layout) = draw(&view, 120, 40);
    let row_text = |r: Rect| {
        (r.x..r.x + r.width)
            .map(|x| buf.cell((x, r.y)).map_or("", |c| c.symbol()))
            .collect::<String>()
    };
    let (rect, index) = layout.categories[1];
    assert!(row_text(rect).contains("Appearance"), "category {index}");
    let (rect, _) = layout.fields[0];
    assert!(row_text(rect).contains("Focus follows pointer"));
    assert!(
        layout
            .buttons
            .iter()
            .all(|(r, _)| row_text(*r).contains('['))
    );
}

#[test]
fn the_default_button_has_focus_when_changes_are_pending() {
    let mut view = view();
    view.set_pending(Some("1 change in Appearance".into()));
    let (_, layout) = draw(&view, 120, 40);
    let ids: Vec<ButtonId> = layout.buttons.iter().map(|(_, id)| *id).collect();
    assert_eq!(ids, [ButtonId::Discard, ButtonId::Apply]);
    view.focus_next();
    view.focus_next();
    assert_eq!(view.focus(), Focus::Buttons);
    assert_eq!(view.focused_button(), Some(ButtonId::Apply));
    view.move_button(false);
    assert_eq!(view.focused_button(), Some(ButtonId::Discard));
    view.move_button(false);
    assert_eq!(
        view.focused_button(),
        Some(ButtonId::Discard),
        "no wrapping"
    );
}

#[test]
fn without_changes_the_first_button_is_the_default() {
    let mut view = view();
    view.set_prompt(Some(vec![ButtonId::KeepEditing, ButtonId::Discard]));
    assert_eq!(view.focused_button(), Some(ButtonId::KeepEditing));
    view.set_prompt(None);
    assert_eq!(
        view.focus(),
        Focus::Categories,
        "the prompt gives focus back"
    );
    assert_eq!(view.focused_button(), None);
}

#[test]
fn compact_category_arrows_are_marked_apart_from_field_steps() {
    let (_, layout) = draw(&view(), 60, 18);
    assert_eq!(layout.categories.len(), 1);
    let arrows: Vec<_> = layout
        .steps
        .iter()
        .filter(|(_, i, _)| *i == usize::MAX)
        .collect();
    assert_eq!(arrows.len(), 2);
    assert!(
        layout.steps.iter().any(|(_, i, _)| *i == 0),
        "field steps keep their index"
    );
}

#[test]
fn steps_sit_on_the_arrows_that_are_drawn() {
    let (buf, layout) = draw(&view(), 120, 40);
    let (rect, _, forward) = layout.steps[0];
    assert!(!forward);
    assert_eq!(buf.cell((rect.x, rect.y)).map(|c| c.symbol()), Some("‹"));
    let (rect, _, forward) = layout.steps[1];
    assert!(forward);
    assert_eq!(
        buf.cell((rect.right() - 1, rect.y)).map(|c| c.symbol()),
        Some("›")
    );
}

#[test]
fn the_list_keeps_its_place_until_the_selection_leaves_it() {
    let fields = (0..30)
        .map(|n| FieldView {
            id: "x",
            label: format!("Field {n}"),
            description: String::new(),
            category: 0,
            kind: FieldKind::ReadOnly,
            value: String::new(),
            source: String::new(),
            applies: "Read only",
            changed: false,
            conflict: false,
            resettable: false,
        })
        .collect();
    let mut view = SettingsView::new(vec!["All".into()], fields);
    let area = Rect::new(0, 0, 90, 24);
    let first = view.layout(area).fields[0].1;
    view.select_field(3);
    assert_eq!(
        view.layout(area).fields[0].1,
        first,
        "a click inside the rows moves nothing"
    );
    view.scroll_by(5);
    assert_eq!(
        view.layout(area).fields[0].1,
        5,
        "the wheel scrolls without selecting"
    );
    assert_eq!(
        view.selected_field().map(|f| f.label.clone()),
        Some("Field 3".into())
    );
    view.select_field(29);
    let layout = view.layout(area);
    assert!(layout.fields.iter().any(|(_, i)| *i == 29));
}
