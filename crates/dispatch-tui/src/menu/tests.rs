use super::*;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

fn menu(anchor: (u16, u16)) -> Menu<u8> {
    Menu::new(
        vec![
            MenuItem {
                label: "Zoom".into(),
                keys: Some("Ctrl p z".into()),
                action: 1,
                enabled: true,
            },
            MenuItem {
                label: "Move to previous tab".into(),
                keys: None,
                action: 2,
                enabled: false,
            },
            MenuItem {
                label: "Close pane and stop agent".into(),
                keys: Some("Ctrl p x".into()),
                action: 3,
                enabled: true,
            },
        ],
        anchor,
    )
}

#[test]
fn arrows_skip_disabled_items() {
    let mut menu = menu((0, 0));
    assert_eq!(menu.selected().map(|item| item.action), Some(1));
    menu.next();
    assert_eq!(
        menu.selected().map(|item| item.action),
        Some(3),
        "the disabled item is skipped"
    );
    menu.next();
    assert_eq!(
        menu.selected().map(|item| item.action),
        Some(1),
        "and it wraps"
    );
    menu.previous();
    assert_eq!(menu.selected().map(|item| item.action), Some(3));
}

#[test]
fn only_enabled_items_are_clickable() {
    let menu = menu((2, 2));
    let layout = menu.layout(Rect::new(0, 0, 80, 24));
    let indices: Vec<usize> = layout.items.iter().map(|(_, index)| *index).collect();
    assert_eq!(indices, vec![0, 2]);
}

#[test]
fn a_menu_stays_inside_the_window_whatever_its_anchor() {
    for (w, h) in [(80, 24), (30, 10), (12, 5), (1, 1)] {
        let area = Rect::new(0, 0, w, h);
        for anchor in [
            (0, 0),
            (w.saturating_sub(1), 0),
            (0, h.saturating_sub(1)),
            (w.saturating_sub(1), h.saturating_sub(1)),
        ] {
            let menu = menu(anchor);
            let rect = menu.layout(area).rect;
            assert!(
                rect.x + rect.width <= w && rect.y + rect.height <= h,
                "{rect:?} in {area:?} at {anchor:?}"
            );
            let mut buf = Buffer::empty(area);
            (&menu).render(area, &mut buf);
        }
    }
}

#[test]
fn a_menu_draws_its_keys_and_leaves_disabled_items_unhighlighted() {
    let mut menu = menu((0, 0));
    let chrome = crate::theme::loud_chrome();
    menu.set_chrome(chrome);
    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);
    (&menu).render(area, &mut buf);
    let text: String = buf.content().iter().map(|cell| cell.symbol()).collect();
    assert!(text.contains("Ctrl p z"));
    crate::theme::assert_no_fixed_colours(&buf);
}

#[test]
fn items_getter_returns_the_menu_items() {
    let menu = menu((0, 0));
    assert_eq!(menu.items().len(), 3);
}
