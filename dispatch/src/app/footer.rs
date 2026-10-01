//! The last row: what is going on, and the two buttons that open the rest.
//!
//! It says state rather than shortcuts. Keys are discoverable in the command
//! help, in menus and in Settings, so the row stays quiet enough to be read at
//! a glance and to be trusted when it does speak.

use dispatch_pty::Scrollbar;

use super::*;
use crate::pointer::FooterHit;

/// What the Settings button says; the narrow fit reserves its width first.
const SETTINGS: &str = "[ Settings ]";
const ACTIVITY: &str = "[ Activity ]";
/// Between two readings on the left of the row.
const SEPARATOR: &str = " · ";
/// Before `[ Activity ]`, which is a control rather than a reading.
const GAP: &str = "  ";

/// One run of text on the row, and what a click on it does.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Part {
    pub(super) text: String,
    pub(super) style: Style,
    pub(super) hit: Option<FooterHit>,
}

impl Part {
    fn new(text: impl Into<String>, style: Style, hit: Option<FooterHit>) -> Self {
        Self {
            text: text.into(),
            style,
            hit,
        }
    }

    fn width(&self) -> u16 {
        u16::try_from(cols(&self.text)).unwrap_or(u16::MAX)
    }

    /// Whether it sits at the right edge rather than after what came before.
    fn is_right(&self) -> bool {
        matches!(
            self.hit,
            Some(FooterHit::Settings | FooterHit::ReturnToLive | FooterHit::LeaveMode)
        )
    }
}

/// A reading on the left of the normal row, before it is fitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Lock,
    Connection,
    Message,
    Working,
    Attention,
    Delegations,
    Activity,
}

struct Item {
    slot: Slot,
    text: String,
    style: Style,
    hit: Option<FooterHit>,
}

/// The row as parts: the readings joined by separators, then `[ Activity ]`
/// after a gap. The right-hand button is not among them.
fn lay_out(items: &[Item], separator_style: Style) -> Vec<Part> {
    let mut parts = Vec::new();
    for item in items {
        if !parts.is_empty() {
            let (between, style) = if item.slot == Slot::Activity {
                (GAP, Style::default())
            } else {
                (SEPARATOR, separator_style)
            };
            parts.push(Part::new(between, style, None));
        }
        parts.push(Part::new(item.text.clone(), item.style, item.hit));
    }
    parts
}

/// How many columns `text` takes, which is not its length.
fn cols(text: &str) -> usize {
    Span::raw(text).width()
}

fn width_of(parts: &[Part]) -> u16 {
    parts.iter().map(Part::width).sum()
}

/// `text` cut to `width` columns, but never below its first word.
fn cut_to(text: &str, width: u16) -> String {
    let first = text.split(' ').next().map_or(0, cols);
    sidebar::truncate(text, usize::from(width).max(first + 1))
}

impl App {
    /// Draws the footer into the last row of `area` and records what answers
    /// a click.
    pub(super) fn draw_footer(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        hits: &mut pointer::HitMap,
    ) {
        if area.height == 0 {
            return;
        }
        let row = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
        let mode = self.router.key_mode();
        let base = if mode == KeyMode::Normal {
            self.theme.chrome().secondary
        } else {
            self.mode_style()
        };
        Paragraph::new("")
            .style(base)
            .render(row, frame.buffer_mut());

        let chrome = self.theme.chrome();
        let held = match self.gesture.map(|gesture| gesture.owner) {
            Some(pointer::Target::Footer(hit)) => Some(hit),
            _ => None,
        };

        let mut x = row.x;
        let end = row.x + row.width;
        for part in self.footer_parts(row.width) {
            let width = part.width().min(end.saturating_sub(x));
            let at = if part.is_right() {
                end.saturating_sub(part.width()).max(row.x)
            } else {
                x
            };
            let rect = Rect::new(at, row.y, part.width().min(end - at), 1);
            let style = match part.hit {
                Some(hit) if Some(hit) == held => chrome.selection,
                _ => part.style,
            };
            Paragraph::new(part.text.as_str())
                .style(style)
                .render(rect, frame.buffer_mut());
            if let Some(hit) = part.hit {
                hits.push(rect, pointer::Target::Footer(hit));
            }
            if !part.is_right() {
                x += width;
            }
        }
    }

    /// The tint a key mode's row is drawn on, like the active tab and in the
    /// same text colour for the same reason: a mode is never missed.
    fn mode_style(&self) -> Style {
        Style::default()
            .bg(self.theme.tab)
            .fg(self.theme.text)
            .add_modifier(Modifier::BOLD)
    }

    /// What the row says at `width`, in the order it is drawn; a right-hand
    /// button, if any, is last.
    pub(super) fn footer_parts(&self, width: u16) -> Vec<Part> {
        let mode = self.router.key_mode();
        match mode {
            KeyMode::Normal => self.readings(width, mode),
            // A prefix that armed invisibly is how a keystroke goes missing
            // with no explanation.
            KeyMode::Prefix => vec![Part::new("PREFIX", self.mode_style(), None)],
            // Left only by its own key, as its contract says, so no button.
            // A lock can last all afternoon, and a refusal or a waiting agent
            // hidden behind it that long is as good as dropped, so the row
            // still says them.
            KeyMode::Lock => self.readings(width, mode),
            KeyMode::Scroll => {
                let text = match self.scrollback_lines() {
                    Some(1) => "Scrollback · 1 line above live".to_string(),
                    Some(lines) => format!("Scrollback · {lines} lines above live"),
                    None => "Scrollback".to_string(),
                };
                self.mode_parts(
                    width,
                    text,
                    "[ Return to live ]",
                    FooterHit::ReturnToLive,
                    false,
                )
            }
            KeyMode::Pane | KeyMode::Tab | KeyMode::Session => self.mode_parts(
                width,
                mode.title().to_string(),
                "[ Done ]",
                FooterHit::LeaveMode,
                true,
            ),
        }
    }

    /// How far above live output the scrolled pane's viewport is, or `None`
    /// where there is no scrollback to speak of: the alternate screen has
    /// none, and a terminal that cannot say has nothing to report.
    fn scrollback_lines(&self) -> Option<u64> {
        let pane = self.panes.get(&self.scrolling?)?;
        let bar: Scrollbar = pane.backend.terminal().scrollbar().ok()?;
        (bar.total != bar.len).then(|| bar.above_live())
    }

    /// A key mode's row: its title, a message if it carries one, and the
    /// button that leaves it at the right.
    fn mode_parts(
        &self,
        width: u16,
        title: String,
        button: &str,
        hit: FooterHit,
        with_message: bool,
    ) -> Vec<Part> {
        let style = self.mode_style();
        let button = Part::new(button, style, Some(hit));
        let room = width.saturating_sub(button.width() + 1);
        let mut parts = vec![Part::new(title, style, None)];
        if with_message && !self.status.text.is_empty() {
            let used = width_of(&parts) + u16::try_from(cols(SEPARATOR)).unwrap_or(0);
            if room > used + 1 {
                parts.push(Part::new(SEPARATOR, style, None));
                parts.push(Part::new(
                    sidebar::truncate(&self.status.text, usize::from(room - used)),
                    style,
                    Some(FooterHit::Message),
                ));
            }
        }
        parts.push(button);
        parts
    }

    /// The normal row, or the locked one, which is the same readings under
    /// the lock's own words and without the buttons.
    fn readings(&self, width: u16, mode: KeyMode) -> Vec<Part> {
        let locked = mode == KeyMode::Lock;
        let mut chrome = self.theme.chrome();
        if locked {
            // On the mode's tint, so it is never missed.
            chrome.secondary = self.mode_style();
            chrome.accent = self.mode_style();
        }
        let red = Style::default().fg(Color::Red);
        let mut items: Vec<Item> = Vec::new();
        let mut push = |slot: Slot, text: String, style: Style, hit: Option<FooterHit>| {
            items.push(Item {
                slot,
                text,
                style,
                hit,
            });
        };

        // Read from the `Device.reachable` `sync_attachment` stamps each poll,
        // not asked live: this runs every frame, and the sidebar and the
        // refusal path read the same field, so disagreeing with them is worse
        // than being a tick stale. Named, because on a fleet "the daemon" says
        // nothing about which machine went.
        let unreachable: Vec<String> = self
            .state
            .devices()
            .iter()
            .filter(|device| !device.reachable)
            .map(|device| device.name.clone())
            .collect();
        let showing = !self.status.text.is_empty();

        if locked {
            let unlock = self
                .router
                .keymap()
                .chords_for(KeyMode::Lock, Command::Unlock)
                .first()
                .map(|chord| format!("LOCKED · {} unlocks", chord.short()));
            push(
                Slot::Lock,
                unlock.unwrap_or_else(|| "LOCKED".to_string()),
                chrome.secondary,
                None,
            );
        }
        if !unreachable.is_empty() {
            push(
                Slot::Connection,
                format!(
                    "{} unreachable — its agents are still running",
                    unreachable.join(", ")
                ),
                chrome.secondary,
                None,
            );
        } else if self.device().is_some() && !showing && !locked {
            // Attached is worth saying: it is the difference between closing
            // Dispatch and killing the agents. A message takes its place.
            push(Slot::Connection, "Connected".into(), chrome.secondary, None);
        }
        if showing {
            let style = if self.status.is_error() {
                red
            } else {
                chrome.secondary
            };
            push(
                Slot::Message,
                self.status.text.clone(),
                style,
                Some(FooterHit::Message),
            );
        }

        // Counted from the walk `Alt a` takes, so the footer, that key and the
        // waiting list always agree on the number.
        let order = self.pane_order();
        let working = order
            .iter()
            .filter(|id| {
                self.state
                    .pane(**id)
                    .is_some_and(|pane| !pane.closed && pane.status == PaneStatus::Running)
            })
            .count();
        let waiting = order.iter().filter(|id| self.is_waiting(**id)).count();
        if working > 0 && !showing && !locked {
            push(
                Slot::Working,
                format!("{working} working"),
                chrome.secondary,
                None,
            );
        }
        if waiting > 0 {
            push(
                Slot::Attention,
                format!("{waiting} need attention"),
                chrome.accent,
                Some(FooterHit::Attention),
            );
        }
        // Derived live from the queue rather than stamped once: a stamped
        // string can be clobbered by a later write, and a deadline the daemon
        // enforces means a deferral whose reminder went missing is decided by
        // inaction. Silent while the prompt is on screen, since it would only
        // repeat what is in front of the user.
        let queued = self.pending.len();
        if queued > 0 && !matches!(self.overlay, Some(Overlay::Approval { .. })) {
            let noun = if queued == 1 {
                "delegation"
            } else {
                "delegations"
            };
            push(
                Slot::Delegations,
                format!("{queued} {noun} waiting"),
                chrome.accent,
                Some(FooterHit::Delegations),
            );
        }
        if !locked {
            push(
                Slot::Activity,
                ACTIVITY.into(),
                chrome.text,
                Some(FooterHit::Activity),
            );
        }

        let settings = Part::new(SETTINGS, chrome.text, Some(FooterHit::Settings));
        let room = if locked {
            width
        } else {
            width.saturating_sub(settings.width() + 1)
        };
        let fits = |items: &[Item]| width_of(&lay_out(items, chrome.secondary)) <= room;

        // Routine counts go first, then the connection, then the button that
        // opens what the row cannot hold.
        for slot in [Slot::Working, Slot::Connection, Slot::Activity] {
            if !fits(&items) {
                items.retain(|item| item.slot != slot);
            }
        }
        // Then the message is cut, and given up when nothing readable is left.
        if !fits(&items)
            && let Some(at) = items.iter().position(|item| item.slot == Slot::Message)
        {
            let over = width_of(&lay_out(&items, chrome.secondary)) - room;
            let current = u16::try_from(cols(&items[at].text)).unwrap_or(u16::MAX);
            match current.checked_sub(over).filter(|keep| *keep >= 2) {
                Some(keep) => items[at].text = sidebar::truncate(&items[at].text, keep.into()),
                None => {
                    items.remove(at);
                }
            }
        }
        // Last, the counts that need the user, never below their first word.
        for slot in [Slot::Delegations, Slot::Attention] {
            if !fits(&items)
                && let Some(at) = items.iter().position(|item| item.slot == slot)
            {
                let over = width_of(&lay_out(&items, chrome.secondary)) - room;
                let current = u16::try_from(cols(&items[at].text)).unwrap_or(u16::MAX);
                items[at].text = cut_to(&items[at].text, current.saturating_sub(over));
            }
        }

        let mut parts = lay_out(&items, chrome.secondary);
        if !locked {
            parts.push(settings);
        }
        parts
    }

    /// Does what a click on a footer part does.
    pub(super) fn activate_footer(&mut self, hit: FooterHit) {
        match hit {
            FooterHit::Attention => self.open_attention_picker(),
            FooterHit::Delegations => self.open_next_approval(),
            FooterHit::Message => {
                if self.status.is_error() {
                    self.clear_status();
                }
            }
            FooterHit::ReturnToLive => {
                self.router.leave_mode();
                self.settle_scroll_mode();
            }
            FooterHit::LeaveMode => self.router.leave_mode(),
            FooterHit::Activity => self.open_activity(),
            // Opened by Task 9, which adds the Settings shell.
            FooterHit::Settings => {}
        }
    }
}
