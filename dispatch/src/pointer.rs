//! What the pointer is over, and what a press started.
//!
//! Each frame records where it drew every clickable thing, in the order
//! drawn, so a pointer event is resolved against exactly what is on screen:
//! the last rectangle covering a cell is the one on top.

use std::time::{Duration, Instant};

use crossterm::event::MouseButton;
use dispatch_core::PaneId;
use ratatui::layout::Rect;

use crate::tabs::TabHit;

/// Two presses on one target this close together are a double-click.
pub const DOUBLE_CLICK: Duration = Duration::from_millis(400);

pub use dispatch_tui::button::ButtonId;

/// The buttons each kind of dialog carries, left to right. The default, which
/// is the one Enter presses, is drawn in the accent. A dialog where Enter does
/// nothing, such as the delegation request, has none.
pub mod buttons {
    use dispatch_tui::button::{Button, ButtonId};

    fn button(id: ButtonId, label: &'static str, default: bool) -> Button {
        Button { id, label, default }
    }

    /// A picker or the directory browser.
    #[must_use]
    pub fn open_cancel() -> Vec<Button> {
        vec![
            button(ButtonId::Cancel, "Cancel", false),
            button(ButtonId::Open, "Open", true),
        ]
    }

    /// The command list, where Enter runs what is chosen.
    #[must_use]
    pub fn run_cancel() -> Vec<Button> {
        vec![
            button(ButtonId::Cancel, "Cancel", false),
            button(ButtonId::Run, "Run", true),
        ]
    }

    /// A harness's settings before a pane is opened with them.
    #[must_use]
    pub fn settings() -> Vec<Button> {
        vec![
            button(ButtonId::Cancel, "Cancel", false),
            button(ButtonId::SaveDefault, "Save as default", false),
            button(ButtonId::OpenPane, "Open pane", true),
        ]
    }

    /// A delegation request.
    #[must_use]
    pub fn approval() -> Vec<Button> {
        vec![
            button(ButtonId::Later, "Later", false),
            button(ButtonId::Deny, "Deny", false),
            button(ButtonId::Always, "Always", false),
            button(ButtonId::Approve, "Approve", false),
        ]
    }

    /// A prompt that takes typing.
    #[must_use]
    pub fn ok_cancel() -> Vec<Button> {
        vec![
            button(ButtonId::Cancel, "Cancel", false),
            button(ButtonId::Ok, "OK", true),
        ]
    }

    /// The question before a tab is closed.
    #[must_use]
    pub fn close_cancel() -> Vec<Button> {
        vec![
            button(ButtonId::Cancel, "Cancel", false),
            button(ButtonId::Close, "Close", false),
        ]
    }
}

/// A row or button of the open dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogHit {
    /// A row of its list, by its index among the rows shown.
    Row(usize),
    /// One of its buttons.
    Button(ButtonId),
    /// A value's `‹` or `›` in the settings form's row: `false` for back.
    Step(usize, bool),
    /// Anywhere else inside its box.
    Area,
}

/// An item of the open menu, or its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuHit {
    /// An enabled item, by index.
    Item(usize),
    /// Anywhere else inside its box.
    Area,
}

/// What a cell of the frame belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The sidebar, docked or as the drawer; `sidebar::hit_test` says which
    /// row.
    Sidebar,
    /// The docked sidebar's right border, which drags to resize.
    SidebarEdge,
    /// A stretch of the tab row.
    Tab(TabHit),
    /// A tile's top border, but its `…`.
    PaneHeader(PaneId),
    /// The `…` at a tile's top right.
    PaneMenu(PaneId),
    /// A tile's interior: the child's.
    PaneContent(PaneId),
    /// The open menu.
    Menu(MenuHit),
    /// The open dialog.
    Dialog(DialogHit),
}

impl Target {
    /// Whether it belongs to an overlay rather than the frame beneath one.
    #[must_use]
    pub fn is_overlay(self) -> bool {
        matches!(self, Target::Menu(_) | Target::Dialog(_))
    }
}

/// Every clickable rectangle of one frame, in the order drawn.
#[derive(Debug, Default)]
pub struct HitMap {
    entries: Vec<(Rect, Target)>,
    /// The overlay open when the frame was drawn, by kind.
    overlay: Option<&'static str>,
}

impl HitMap {
    /// An empty map for a frame drawn with `overlay` open.
    #[must_use]
    pub fn new(overlay: Option<&'static str>) -> Self {
        Self {
            entries: Vec::new(),
            overlay,
        }
    }

    /// Records `target` as drawn over `rect`, above everything before it.
    pub fn push(&mut self, rect: Rect, target: Target) {
        if !rect.is_empty() {
            self.entries.push((rect, target));
        }
    }

    /// What is on top at `(x, y)`.
    #[must_use]
    pub fn at(&self, x: u16, y: u16) -> Option<Target> {
        let position = ratatui::layout::Position::new(x, y);
        self.entries
            .iter()
            .rev()
            .find(|(rect, _)| rect.contains(position))
            .map(|(_, target)| *target)
    }

    /// Whether this frame was drawn with `overlay_now` open, so that what it
    /// recorded is what is on screen.
    #[must_use]
    pub fn is_for(&self, overlay_now: Option<&'static str>) -> bool {
        overlay_now == self.overlay
    }

    /// What a press at `(x, y)` may act on, with `overlay_now` open.
    ///
    /// When the overlay has changed since this frame was drawn, nothing
    /// resolves: a stale map holds none of the current overlay's targets, and
    /// a rectangle drawn under an overlay that has since opened, or over one
    /// that has since closed, must not act.
    #[must_use]
    pub fn resolve(&self, x: u16, y: u16, overlay_now: Option<&'static str>) -> Option<Target> {
        let target = self.at(x, y)?;
        if overlay_now != self.overlay {
            return None;
        }
        Some(target)
    }
}

/// The last press, for telling a double-click.
#[derive(Debug, Default)]
pub struct Clicks {
    last: Option<(Target, Instant)>,
}

impl Clicks {
    /// Records a left press on `target` at `now`, returning whether it is
    /// the second of a double-click. A double-click is not the first of the
    /// next: three presses are one double and one single.
    pub fn press(&mut self, target: Target, now: Instant) -> bool {
        let double = self.last.is_some_and(|(last, at)| {
            last == target && now.saturating_duration_since(at) <= DOUBLE_CLICK
        });
        self.last = if double { None } else { Some((target, now)) };
        double
    }
}

/// A press still held, and who it belongs to until it is let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gesture {
    /// What was pressed.
    pub owner: Target,
    /// Which button holds it.
    pub button: MouseButton,
    /// Where the pointer last was, for a release sent on its behalf.
    pub last: (u16, u16),
    /// The owner pane's interior when it was pressed, so a release can still
    /// be addressed to a pane that has since left the grid.
    pub rect: Option<Rect>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane() -> PaneId {
        PaneId::new()
    }

    #[test]
    fn the_last_rectangle_drawn_is_the_one_on_top() {
        let a = pane();
        let mut map = HitMap::new(None);
        map.push(Rect::new(0, 0, 10, 10), Target::PaneContent(a));
        map.push(Rect::new(2, 2, 3, 3), Target::Menu(MenuHit::Item(0)));

        assert_eq!(map.at(3, 3), Some(Target::Menu(MenuHit::Item(0))));
        assert_eq!(map.at(0, 0), Some(Target::PaneContent(a)));
        assert_eq!(map.at(20, 20), None);
    }

    #[test]
    fn an_empty_rectangle_is_never_hit() {
        let mut map = HitMap::new(None);
        map.push(Rect::new(5, 5, 0, 3), Target::SidebarEdge);
        assert_eq!(map.at(5, 5), None);
    }

    #[test]
    fn a_map_drawn_under_another_overlay_resolves_nothing() {
        let a = pane();
        let mut map = HitMap::new(None);
        map.push(Rect::new(0, 0, 10, 10), Target::PaneContent(a));

        assert_eq!(map.resolve(1, 1, None), Some(Target::PaneContent(a)));
        assert_eq!(map.resolve(1, 1, Some("menu")), None, "a menu opened since");

        let mut map = HitMap::new(Some("menu"));
        map.push(Rect::new(0, 0, 10, 10), Target::Menu(MenuHit::Area));
        assert_eq!(map.resolve(1, 1, None), None, "the menu has closed since");
    }

    #[test]
    fn two_quick_presses_on_one_target_are_a_double_click() {
        let a = pane();
        let start = Instant::now();
        let mut clicks = Clicks::default();

        assert!(!clicks.press(Target::PaneHeader(a), start));
        assert!(clicks.press(Target::PaneHeader(a), start + Duration::from_millis(300)));
        assert!(
            !clicks.press(Target::PaneHeader(a), start + Duration::from_millis(350)),
            "a third press starts again"
        );
    }

    #[test]
    fn a_slow_second_press_or_another_target_is_not_a_double_click() {
        let (a, b) = (pane(), pane());
        let start = Instant::now();
        let mut clicks = Clicks::default();

        clicks.press(Target::PaneHeader(a), start);
        assert!(!clicks.press(Target::PaneHeader(a), start + Duration::from_millis(401)));
        assert!(!clicks.press(Target::PaneHeader(b), start + Duration::from_millis(450)));
    }

    #[test]
    fn a_dialog_where_enter_does_nothing_accents_no_button() {
        for buttons in [buttons::approval(), buttons::close_cancel()] {
            assert!(buttons.iter().all(|button| !button.default));
        }
        assert!(buttons::open_cancel().iter().any(|button| button.default));
    }
}
