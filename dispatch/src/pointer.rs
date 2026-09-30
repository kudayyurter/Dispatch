//! What the pointer is over, and what a press started.
//!
//! Each frame records where it drew every clickable thing, in the order
//! drawn, so a pointer event is resolved against exactly what is on screen:
//! the last rectangle covering a cell is the one on top.
//!
//! Task 2 wires these types in and will remove the `allow(dead_code)` below.
#![allow(dead_code)]

use std::time::{Duration, Instant};

use crossterm::event::MouseButton;
use dispatch_core::PaneId;
use ratatui::layout::Rect;

use crate::tabs::TabHit;

/// Two presses on one target this close together are a double-click.
pub const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Stand-in until Task 8 moves the dialog buttons into `dispatch-tui`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonId {}

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

    /// What a press at `(x, y)` may act on, with `overlay_now` open.
    ///
    /// When the overlay has changed since this frame was drawn, only the
    /// overlay's own targets count: a rectangle drawn under an overlay that
    /// has since opened, or over one that has since closed, must not act.
    #[must_use]
    pub fn resolve(&self, x: u16, y: u16, overlay_now: Option<&'static str>) -> Option<Target> {
        let target = self.at(x, y)?;
        if overlay_now != self.overlay {
            return None;
        }
        Some(target)
    }

    /// The overlay open when it was drawn.
    #[must_use]
    pub fn overlay(&self) -> Option<&'static str> {
        self.overlay
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
}
