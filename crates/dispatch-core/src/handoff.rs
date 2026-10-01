//! What a parent agent hands a subagent.
//!
//! A one-line task loses everything the parent knew: the files that matter,
//! what was ruled out, how to tell the work is finished. A handoff is a
//! Markdown file with five required sections, so none of that can be
//! forgotten by accident. It is checked twice, by `dispatch delegate`
//! before it connects and by the daemon when the request arrives, with this
//! one parser.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::PaneId;

/// The largest handoff accepted, in bytes.
///
/// The wrapped handoff becomes one command-line argument, and Linux limits
/// one argument to 128 KiB. Half that leaves room for the preamble and for
/// every other argument.
pub const MAX_HANDOFF_BYTES: usize = 64 * 1024;

/// An empty handoff, printed by `dispatch delegate --template` and after
/// every refusal, so an agent can learn the format from the error alone.
pub const TEMPLATE: &str = "## Goal\n\
<What the subagent is to achieve, in a sentence or two.>\n\
\n\
## Context\n\
<What you already know: the files that matter, decisions made, what was tried and ruled out.>\n\
\n\
## Constraints\n\
<What must not change; tools or approaches to avoid. Write None. if there are none.>\n\
\n\
## Done when\n\
<How the subagent knows it has finished: tests that pass, a file that exists.>\n\
\n\
## Report back\n\
<What the report should contain, and in what shape.>\n";

/// One required section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    /// What to achieve.
    Goal,
    /// What the parent already knows.
    Context,
    /// What must not change.
    Constraints,
    /// How the subagent knows it has finished.
    DoneWhen,
    /// What the report should contain.
    ReportBack,
}

impl Section {
    /// Every section, in the order the template writes them.
    pub const ALL: [Section; 5] = [
        Section::Goal,
        Section::Context,
        Section::Constraints,
        Section::DoneWhen,
        Section::ReportBack,
    ];

    /// The order the approval prompt shows them in: what the user needs to
    /// judge a request by comes first, the long background last but one.
    pub const PROMPT_ORDER: [Section; 5] = [
        Section::Goal,
        Section::DoneWhen,
        Section::Constraints,
        Section::Context,
        Section::ReportBack,
    ];

    /// Its heading, as the template writes it.
    #[must_use]
    pub fn heading(self) -> &'static str {
        match self {
            Section::Goal => "Goal",
            Section::Context => "Context",
            Section::Constraints => "Constraints",
            Section::DoneWhen => "Done when",
            Section::ReportBack => "Report back",
        }
    }

    fn from_heading(title: &str) -> Option<Section> {
        Section::ALL
            .into_iter()
            .find(|section| section.heading().eq_ignore_ascii_case(title.trim()))
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// One thing wrong with a handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The heading is not there.
    Missing(Section),
    /// The heading is there with nothing under it.
    Empty(Section),
    /// The section still holds the template's placeholder.
    Unfilled(Section),
    /// The heading appears more than once.
    Repeated(Section),
    /// The whole file is over [`MAX_HANDOFF_BYTES`].
    TooLarge {
        /// How large it is.
        bytes: usize,
    },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Problem::Missing(section) => write!(f, "{} is missing", section.heading()),
            Problem::Empty(section) => write!(f, "{} is empty", section.heading()),
            Problem::Unfilled(section) => {
                write!(
                    f,
                    "{} still holds the template's placeholder",
                    section.heading()
                )
            }
            Problem::Repeated(section) => {
                write!(f, "{} appears more than once", section.heading())
            }
            Problem::TooLarge { bytes } => write!(
                f,
                "the handoff is {bytes} bytes, over the {MAX_HANDOFF_BYTES}-byte limit"
            ),
        }
    }
}

/// Why a handoff was refused: every problem found, not only the first, so
/// an agent fixes them in one go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffError {
    /// What is wrong, in section order.
    pub problems: Vec<Problem>,
}

impl fmt::Display for HandoffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let problems: Vec<String> = self.problems.iter().map(ToString::to_string).collect();
        write!(f, "the handoff is not ready: {}", problems.join("; "))
    }
}

impl std::error::Error for HandoffError {}

/// A checked handoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handoff {
    /// What to achieve.
    pub goal: String,
    /// What the parent already knows.
    pub context: String,
    /// What must not change.
    pub constraints: String,
    /// How the subagent knows it has finished.
    pub done_when: String,
    /// What the report should contain.
    pub report_back: String,
    /// The whole file, exactly as written: the subagent gets this, not a
    /// reassembly of the sections.
    pub text: String,
}

impl Handoff {
    /// Checks `text` and splits it into its sections.
    ///
    /// A section runs from its heading to the next required heading, so a
    /// parent may structure a long section with headings of its own. Text
    /// before the first required heading belongs to no section and is kept
    /// in [`Handoff::text`]. Headings inside fenced code blocks are content:
    /// a parent pasting an example handoff must not trip over its `## Goal`.
    ///
    /// # Errors
    ///
    /// Every [`Problem`] found, in section order.
    pub fn parse(text: &str) -> Result<Handoff, HandoffError> {
        if text.len() > MAX_HANDOFF_BYTES {
            return Err(HandoffError {
                problems: vec![Problem::TooLarge { bytes: text.len() }],
            });
        }

        let mut bodies: [Option<String>; 5] = Default::default();
        let mut problems = Vec::new();
        let mut current: Option<Section> = None;
        let mut fenced = false;

        // `lines` strips a trailing `\r` as well as the `\n`, so a file saved
        // with Windows line endings parses the same.
        for line in text.lines() {
            let trimmed = line.trim_start();
            let fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");

            if !fenced
                && !fence
                && let Some(section) = line.strip_prefix("## ").and_then(Section::from_heading)
            {
                if bodies[section.index()].is_some() {
                    if !problems.contains(&Problem::Repeated(section)) {
                        problems.push(Problem::Repeated(section));
                    }
                } else {
                    bodies[section.index()] = Some(String::new());
                }
                current = Some(section);
                continue;
            }

            if fence {
                fenced = !fenced;
            }
            if let Some(section) = current
                && let Some(body) = bodies[section.index()].as_mut()
            {
                body.push_str(line);
                body.push('\n');
            }
        }

        let mut sections: [String; 5] = Default::default();
        for section in Section::ALL {
            match bodies[section.index()].as_deref().map(str::trim) {
                None => problems.push(Problem::Missing(section)),
                Some("") => problems.push(Problem::Empty(section)),
                Some(body) if is_placeholder(body) => problems.push(Problem::Unfilled(section)),
                Some(body) => sections[section.index()] = body.to_string(),
            }
        }

        if !problems.is_empty() {
            // Repeated headings were found mid-file; the rest in section
            // order. Sorting keeps the whole list in section order.
            problems.sort_by_key(|problem| match problem {
                Problem::Missing(s)
                | Problem::Empty(s)
                | Problem::Unfilled(s)
                | Problem::Repeated(s) => s.index(),
                Problem::TooLarge { .. } => 0,
            });
            return Err(HandoffError { problems });
        }

        let [goal, context, constraints, done_when, report_back] = sections;
        Ok(Handoff {
            goal,
            context,
            constraints,
            done_when,
            report_back,
            text: text.to_string(),
        })
    }

    /// One section's text, trimmed.
    #[must_use]
    pub fn section(&self, section: Section) -> &str {
        match section {
            Section::Goal => &self.goal,
            Section::Context => &self.context,
            Section::Constraints => &self.constraints,
            Section::DoneWhen => &self.done_when,
            Section::ReportBack => &self.report_back,
        }
    }

    /// What the subagent is given: one fixed preamble, the same for every
    /// harness, then the handoff verbatim.
    ///
    /// The preamble is what makes a report arrive: it is the only place the
    /// subagent learns that `dispatch report` exists.
    #[must_use]
    pub fn brief(&self, parent: PaneId) -> String {
        format!(
            "You are a subagent started by Dispatch for the agent in pane {parent}.\n\
             Nobody can answer questions while you work: where something is unclear,\n\
             choose, and say what you assumed in your report.\n\
             \n\
             When you have finished, run `dispatch report` with your report, either as a\n\
             file (`dispatch report report.md`) or on standard input\n\
             (`dispatch report -`). Your work is not delivered until you do.\n\
             \n\
             {}",
            self.text
        )
    }
}

/// Whether `body` is one of the template's `<…>` placeholders, left as it was.
fn is_placeholder(body: &str) -> bool {
    !body.contains('\n') && body.starts_with('<') && body.ends_with('>')
}

#[cfg(test)]
mod tests;
