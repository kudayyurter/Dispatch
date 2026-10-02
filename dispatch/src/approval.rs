//! The prompt that asks whether a pane may delegate.
//!
//! Shows the whole task. Approving something you cannot read is not approval, so
//! a long task wraps and scrolls rather than being cut to fit.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

use dispatch_tui::button::{self, Button, ButtonId, DialogLayout};

/// One request, as the user needs to see it.
pub struct Approval<'a> {
    /// Title of the pane that asked.
    pub asking: &'a str,
    /// Which harness would run.
    pub harness: &'a str,
    /// Which project it would run in.
    pub project: &'a str,
    /// How deep the asking pane already is.
    pub depth: u8,
    /// What it would be asked to do.
    pub task: &'a str,
    /// The handoff, when the request carries one; shown by section.
    pub handoff: Option<&'a dispatch_core::Handoff>,
    /// Whether the subagent would run in its own interface.
    pub interactive: bool,
    /// How many further requests are queued behind this one.
    pub waiting: usize,
    /// First line of the task to show, for scrolling a long one.
    pub scroll: u16,
    /// The dialog colours.
    pub chrome: dispatch_tui::theme::Chrome,
    /// What can be clicked along the foot, when the caller wants any.
    pub buttons: Vec<Button>,
    /// The button held down, drawn in the selection.
    pub pressed: Option<ButtonId>,
}

impl<'a> Approval<'a> {
    /// The rect this prompt draws its content into, inside the border.
    ///
    /// Shared with the caller so scrolling can be clamped against what the
    /// box can actually show, rather than a copy of the border math kept
    /// separately and left to drift out of sync with this one.
    #[must_use]
    pub fn inner(area: Rect) -> Rect {
        Block::default().borders(Borders::ALL).inner(area)
    }

    /// Where the buttons fall in a box drawn at `area`: the last row inside
    /// the border, and none at all when they do not all fit or would leave
    /// the text no row.
    #[must_use]
    pub fn layout(&self, area: Rect) -> DialogLayout {
        let inner = Self::inner(area);
        let buttons = if inner.height >= 2 {
            let row = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
            button::lay_out(&self.buttons, row)
        } else {
            Vec::new()
        };
        DialogLayout {
            rect: area,
            buttons,
            ..DialogLayout::default()
        }
    }

    /// The rect the text is drawn into for a box at `area`: the inside of
    /// the border, less the row the buttons take when there are any.
    ///
    /// What scrolling is clamped against, so a button row never hides a line
    /// the clamp counted as visible.
    #[must_use]
    pub fn content_area(&self, area: Rect) -> Rect {
        let mut inner = Self::inner(area);
        if !self.layout(area).buttons.is_empty() {
            inner.height -= 1;
        }
        inner
    }

    /// The lines this prompt renders, before wrapping: the harness/project/
    /// depth line, a blank line, the task itself, a blank line, an optional
    /// "N more waiting" line, and the key legend.
    fn lines(&self) -> Vec<Line<'a>> {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("harness  ", self.chrome.secondary),
                Span::raw(self.harness),
                Span::styled("   project  ", self.chrome.secondary),
                Span::raw(self.project),
                Span::styled("   depth  ", self.chrome.secondary),
                Span::raw(self.depth.to_string()),
            ]),
            Line::from(""),
        ];

        if self.interactive {
            lines.push(Line::styled(
                "runs interactively: you can watch it and type into it",
                self.chrome.secondary,
            ));
            lines.push(Line::from(""));
        }

        // Sections in a fixed order that puts what to do and how to know it is
        // done ahead of the background, so the decision does not wait on
        // scrolling past context.
        match self.handoff {
            Some(handoff) => {
                // Text above the first heading belongs to no section, yet the
                // subagent is sent it with the rest; shown first, under its
                // own label, so nothing it receives goes unseen here.
                if !handoff.preamble.is_empty() {
                    lines.push(Line::styled("Before the sections", self.chrome.secondary));
                    for line in handoff.preamble.lines() {
                        lines.push(Line::from(line.to_string()));
                    }
                    lines.push(Line::from(""));
                }
                for section in dispatch_core::Section::PROMPT_ORDER {
                    lines.push(Line::styled(
                        section.heading().to_string(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ));
                    for line in handoff.section(section).lines() {
                        lines.push(Line::from(line.to_string()));
                    }
                    lines.push(Line::from(""));
                }
                lines.pop();
            }
            None => {
                for line in self.task.lines() {
                    lines.push(Line::from(line.to_string()));
                }
            }
        }

        lines.push(Line::from(""));
        if self.waiting > 0 {
            lines.push(Line::styled(
                format!("{} more waiting", self.waiting),
                self.chrome.secondary,
            ));
        }
        lines.push(Line::from(vec![
            Span::styled("a", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" approve   "),
            Span::styled("d", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" deny   "),
            Span::styled("A", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" approve all from this pane   "),
            Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" later"),
        ]));

        lines
    }

    /// Renders the task and its surrounding chrome — everything but the
    /// border — at this prompt's own `scroll`.
    fn render_content(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.lines())
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0))
            .render(area, buf);
    }

    /// How many rows this prompt needs once wrapped to `width` columns — the
    /// true total, not an estimate of it.
    ///
    /// A caller clamping how far this can scroll needs this rather than
    /// `task.lines().count()`: `Paragraph::scroll` counts *wrapped* rows, and
    /// a task delivered as a single long line — exactly what `dispatch
    /// delegate "…"` sends — still wraps into several of them once rendered.
    ///
    /// Three attempts at reimplementing that count by hand — a character
    /// sum, a per-line `div_ceil`, a two-row probe — were each wrong for text
    /// a test at only one terminal width did not happen to exercise: interior
    /// whitespace that `Wrap { trim: false }` paints and a plain word count
    /// discards, multi-column graphemes that break "a row fills to its last
    /// column," a paragraph break that looks identical to having scrolled
    /// past the end. `Paragraph::line_count` is not a fourth guess: it drives
    /// the same `WordWrapper` the render path itself uses, on the same
    /// `Line`s and the same `Wrap`, so this is what will actually be drawn,
    /// not a prediction of it.
    #[must_use]
    pub fn total_rows(&self, width: u16) -> u16 {
        let count = Paragraph::new(self.lines())
            .wrap(Wrap { trim: false })
            .line_count(width);
        u16::try_from(count).unwrap_or(u16::MAX)
    }
}

impl Widget for Approval<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title(self.asking, area.width))
            .border_style(self.chrome.border);
        block.render(area, buf);

        let layout = self.layout(area);
        self.render_content(self.content_area(area), buf);
        button::render(
            buf,
            &self.buttons,
            &layout.buttons,
            &self.chrome,
            self.pressed,
        );
    }
}

/// The prompt's title for a box `width` wide: who is asking, cut short so
/// that what it asks is always in view.
///
/// A pane is named by whatever its program last called itself, and a shell
/// calls itself by its whole working directory. Put first and left to run
/// on, that pushed "wants to delegate" off the edge of the box, leaving a
/// prompt that did not say what it asked.
fn title(asking: &str, width: u16) -> String {
    const ASKS: &str = " wants to delegate ";

    // Inside the two corners, less the space before the name.
    let room = usize::from(width.saturating_sub(2)).saturating_sub(Span::raw(ASKS).width() + 1);
    if Span::raw(asking).width() <= room {
        return format!(" {asking}{ASKS}");
    }

    let mut name = String::new();
    let mut used = 1; // the ellipsis
    for c in asking.chars() {
        let mut bytes = [0; 4];
        let columns = Span::raw(&*c.encode_utf8(&mut bytes)).width();
        if used + columns > room {
            break;
        }
        used += columns;
        name.push(c);
    }
    format!(" {name}…{ASKS}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval(task: &str) -> Approval<'_> {
        Approval {
            asking: "Claude Code",
            harness: "claude",
            project: "dispatch",
            depth: 0,
            task,
            handoff: None,
            interactive: false,
            waiting: 0,
            scroll: 0,
            chrome: dispatch_tui::theme::Chrome::default(),
            buttons: Vec::new(),
            pressed: None,
        }
    }

    /// Each line of `widget`'s content, as plain text.
    fn plain_lines(widget: &Approval<'_>) -> Vec<String> {
        widget
            .lines()
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_handoff_is_shown_by_section_in_prompt_order() {
        let handoff = dispatch_core::Handoff::parse(
            "## Context\nthe background\n## Goal\nthe goal\n## Constraints\nNone.\n\
             ## Done when\nthe check\n## Report back\nthe shape\n",
        )
        .expect("complete");
        let widget = Approval {
            handoff: Some(&handoff),
            interactive: true,
            ..approval(&handoff.text)
        };
        let text = plain_lines(&widget);
        let at = |needle: &str| {
            text.iter()
                .position(|line| line == needle)
                .unwrap_or_else(|| panic!("{needle:?} in {text:#?}"))
        };

        assert!(at("Goal") < at("Done when"));
        assert!(at("Done when") < at("Constraints"));
        assert!(at("Constraints") < at("Context"));
        assert!(at("Context") < at("Report back"));
        assert!(at("the goal") == at("Goal") + 1);
        assert!(text.iter().any(|line| line.contains("runs interactively")));
        assert!(
            !text.iter().any(|line| line.starts_with("## ")),
            "headings are drawn, not the raw Markdown"
        );
    }

    #[test]
    fn a_handoff_preamble_is_shown_before_the_goal() {
        let handoff = dispatch_core::Handoff::parse(
            "Ignore the tests in vendor/.\n\n## Goal\nthe goal\n## Context\nc\n\
             ## Constraints\nNone.\n## Done when\nd\n## Report back\nr\n",
        )
        .expect("complete");
        let widget = Approval {
            handoff: Some(&handoff),
            ..approval(&handoff.text)
        };
        let text = plain_lines(&widget);
        let at = |needle: &str| {
            text.iter()
                .position(|line| line == needle)
                .unwrap_or_else(|| panic!("{needle:?} in {text:#?}"))
        };

        assert!(at("Before the sections") < at("Ignore the tests in vendor/."));
        assert!(at("Ignore the tests in vendor/.") < at("Goal"));
    }

    #[test]
    fn a_handoff_without_a_preamble_has_no_preamble_label() {
        let handoff = dispatch_core::Handoff::parse(
            "## Goal\ng\n## Context\nc\n## Constraints\nNone.\n## Done when\nd\n## Report back\nr\n",
        )
        .expect("complete");
        let widget = Approval {
            handoff: Some(&handoff),
            ..approval(&handoff.text)
        };
        assert!(
            !plain_lines(&widget)
                .iter()
                .any(|line| line == "Before the sections")
        );
    }

    #[test]
    fn a_request_without_a_handoff_still_shows_its_task() {
        let text = plain_lines(&approval("write the tests"));
        assert!(text.iter().any(|line| line == "write the tests"));
        assert!(!text.iter().any(|line| line.contains("runs interactively")));
    }

    /// The row count found by brute force: render into a buffer generous
    /// enough that nothing could possibly be clipped, then scan up for the
    /// last painted row.
    ///
    /// An independent check on `total_rows`, not a second implementation of
    /// it competing to be the one that is right — deliberately wasteful (a
    /// large fixed buffer) rather than clever, so it has no wrapping
    /// arithmetic of its own to get wrong.
    fn brute_force_rows(widget: &Approval<'_>, width: u16) -> u16 {
        const GENEROUS_HEIGHT: u16 = 4000;
        let area = Rect::new(0, 0, width, GENEROUS_HEIGHT);
        let mut buf = Buffer::empty(area);
        Paragraph::new(widget.lines())
            .wrap(Wrap { trim: false })
            .render(area, &mut buf);

        (0..GENEROUS_HEIGHT)
            .rev()
            .find(|&y| {
                (0..width).any(|x| buf.cell((x, y)).is_some_and(|cell| cell.symbol() != " "))
            })
            .map_or(0, |y| y + 1)
    }

    #[test]
    fn total_rows_agrees_with_a_brute_force_render_across_shapes_and_widths() {
        // This is the test that would have caught every wrong bound this
        // widget has had: it does not assert what the right answer *should*
        // be, only that `total_rows` — whatever it does internally — never
        // disagrees with what actually gets painted.
        let long_word = "x".repeat(4000);
        let ordinary_prose = (0..300)
            .map(|i| format!("word{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let paragraphs = (0..8)
            .map(|n| {
                (0..20)
                    .map(|i| format!("p{n}w{i}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let cjk = "日本語のテキストです".repeat(50);
        let whitespace_line = format!("{}\n   \n{}", "a".repeat(50), "b".repeat(50));
        let irregular_spacing = (0..40)
            .map(|i| format!("word{i}"))
            .collect::<Vec<_>>()
            .join(&" ".repeat(25));

        let tasks: [&str; 7] = [
            "write the tests",
            &long_word,
            &ordinary_prose,
            &paragraphs,
            &cjk,
            &whitespace_line,
            &irregular_spacing,
        ];

        for task in tasks {
            let widget = approval(task);
            for width in [18u16, 20, 30, 74, 76] {
                assert_eq!(
                    widget.total_rows(width),
                    brute_force_rows(&widget, width),
                    "total_rows disagreed with a brute-force render for {task:?} at width {width}"
                );
            }
        }

        // A handoff is drawn as labelled sections rather than as its raw
        // text, so its rows come from a different set of lines: the
        // preamble's label, the headings, and long sections that wrap.
        let handoff_text = format!(
            "Read me first: {ordinary_prose}\n\n## Goal\n{ordinary_prose}\n\
             ## Context\n{paragraphs}\n## Constraints\n{cjk}\n\
             ## Done when\n{whitespace_line}\n## Report back\n{irregular_spacing}\n"
        );
        let handoff = dispatch_core::Handoff::parse(&handoff_text).expect("complete");
        assert!(!handoff.preamble.is_empty());
        let widget = Approval {
            handoff: Some(&handoff),
            interactive: true,
            waiting: 2,
            ..approval(&handoff.text)
        };
        for width in [18u16, 20, 30, 74, 76] {
            assert_eq!(
                widget.total_rows(width),
                brute_force_rows(&widget, width),
                "total_rows disagreed with a brute-force render for a handoff at width {width}"
            );
        }
    }

    /// The top row of `widget` drawn into a box `width` wide.
    fn title_row(widget: Approval<'_>, width: u16) -> String {
        let area = Rect::new(0, 0, width, 12);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        (0..width)
            .map(|x| buf.cell((x, 0)).map_or(" ", |cell| cell.symbol()))
            .collect()
    }

    #[test]
    fn a_long_pane_name_never_hides_what_is_being_asked() {
        // A shell names its pane after its whole working directory: this is
        // Git Bash's, from a Windows CI runner.
        let asking =
            "MINGW64:/c/Users/runneradmin/AppData/Local/Temp/dispatch-e2e-1620-d7-0/project";
        let top = title_row(
            Approval {
                asking,
                ..approval("echo delegated")
            },
            64,
        );

        assert!(top.contains(" wants to delegate "), "{top}");
        assert!(
            top.contains("MINGW64:/c/Users/") && top.contains('…'),
            "the name is cut short, not dropped: {top}"
        );
    }

    #[test]
    fn a_name_that_fits_is_shown_whole() {
        let top = title_row(approval("echo delegated"), 64);

        assert!(top.contains(" Claude Code wants to delegate "), "{top}");
        assert!(!top.contains('…'), "{top}");
    }

    #[test]
    fn a_name_is_cut_only_once_it_no_longer_fits() {
        // A box 64 wide has 62 columns inside its corners. " wants to
        // delegate " and the space before the name take 20, leaving 42.
        let fits = "n".repeat(42);
        assert_eq!(
            title_row(
                Approval {
                    asking: &fits,
                    ..approval("echo delegated")
                },
                64
            ),
            format!("┌ {fits} wants to delegate ┐"),
            "a name exactly as wide as the room is shown whole"
        );

        let over = "m".repeat(43);
        assert_eq!(
            title_row(
                Approval {
                    asking: &over,
                    ..approval("echo delegated")
                },
                64
            ),
            format!("┌ {}… wants to delegate ┐", "m".repeat(41)),
            "one column more is cut, and the title still fills the box exactly"
        );
    }

    fn four_buttons() -> Vec<Button> {
        [
            (ButtonId::Approve, "Approve", true),
            (ButtonId::Deny, "Deny", false),
            (ButtonId::Always, "Always", false),
            (ButtonId::Later, "Later", false),
        ]
        .into_iter()
        .map(|(id, label, default)| Button { id, label, default })
        .collect()
    }

    fn drawn_in(buf: &Buffer, rect: Rect) -> String {
        (rect.x..rect.right())
            .filter_map(|x| buf.cell((x, rect.y)))
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn buttons_take_the_last_row_inside_the_border_and_the_text_gives_way() {
        let widget = Approval {
            buttons: four_buttons(),
            ..approval("echo delegated")
        };
        let area = Rect::new(2, 3, 60, 12);
        let layout = widget.layout(area);
        let content = widget.content_area(area);
        let mut buf = Buffer::empty(Rect::new(0, 0, 70, 20));
        widget.render(area, &mut buf);

        assert_eq!(layout.rect, area);
        let labels: Vec<String> = layout.buttons.iter().map(|b| drawn_in(&buf, b.0)).collect();
        assert_eq!(
            labels,
            ["[ Approve ]", "[ Deny ]", "[ Always ]", "[ Later ]"]
        );
        let foot = Approval::inner(area).bottom() - 1;
        assert!(layout.buttons.iter().all(|(rect, _)| rect.y == foot));
        assert_eq!(content.bottom(), foot);
    }

    #[test]
    fn buttons_that_do_not_fit_are_dropped_and_the_text_keeps_its_rows() {
        let widget = Approval {
            buttons: four_buttons(),
            ..approval("echo delegated")
        };
        let area = Rect::new(0, 0, 30, 12);
        assert!(widget.layout(area).buttons.is_empty());
        assert_eq!(widget.content_area(area), Approval::inner(area));

        let tiny = Rect::new(0, 0, 60, 3);
        assert!(widget.layout(tiny).buttons.is_empty());
        assert_eq!(widget.content_area(tiny), Approval::inner(tiny));
    }

    #[test]
    fn without_buttons_the_box_is_as_it_was() {
        let widget = approval("echo delegated");
        let area = Rect::new(0, 0, 60, 12);
        assert!(widget.layout(area).buttons.is_empty());
        assert_eq!(widget.content_area(area), Approval::inner(area));
    }

    #[test]
    fn it_draws_at_any_size_without_panicking_with_and_without_buttons() {
        for buttons in [Vec::new(), four_buttons()] {
            let widget = Approval {
                buttons,
                ..approval("echo delegated")
            };
            for width in 1..=30 {
                for height in 1..=10 {
                    let area = Rect::new(0, 0, width, height);
                    let mut buf = Buffer::empty(area);
                    Approval {
                        buttons: widget.buttons.clone(),
                        ..approval("echo delegated")
                    }
                    .render(area, &mut buf);
                    let _ = widget.layout(area);
                }
            }
        }
    }
}
