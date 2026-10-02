# Delegation handoff, reports, and interactive subagents Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `dispatch delegate` takes a structured handoff file instead of a bare string, the subagent finishes by running `dispatch report`, and `--interactive` runs the subagent in the agent's own interface, closing it once it has reported.

**Architecture:** A new `dispatch_core::handoff` module parses and checks the handoff and builds the brief the subagent receives. The protocol gains optional fields and three messages (`DelegateReport`, `ReportAnswered`, `SubagentReported`). The daemon requires a handoff, wraps it, runs either the harness's `[task]` form or its new `[task.interactive]` form, and answers the caller the moment a report arrives. The `dispatch` CLI gains `--handoff`/`--template`/`--interactive` on `delegate` and a new `report` subcommand. The interface shows the handoff's sections in the approval prompt and marks reported panes.

**Tech Stack:** Rust 1.89 workspace; serde + rmp-serde (MessagePack maps) on the wire; clap for the CLI; ratatui for the interface; `cargo test --workspace`.

**Spec:** `docs/superpowers/specs/2026-09-30-delegation-handoff-design.md`. Read it before starting any task.

## Global Constraints

- Every field added to a wire message carries `#[serde(default)]`; every new tagged enum has a `#[serde(other)] Unknown` variant (`crates/dispatch-proto/src/message.rs` header).
- The protocol `VERSION` stays at `1.1`. `crates/dispatch-proto/src/lib.rs` says it is "bumped only for a change an older peer cannot safely ignore. Adding a message variant or an optional field is not such a change." The spec's "the protocol's minor version goes up" is corrected in Task 2.
- Required headings, exactly: `Goal`, `Context`, `Constraints`, `Done when`, `Report back` (level two, `## `, case-insensitive, once each).
- Handoff limit: 64 KiB (`64 * 1024` bytes). Report limit: 1 MiB (`1024 * 1024` bytes).
- `[delegation] interactive_on_done` accepts `"close"` (default) or `"ask"`.
- `dispatch delegate` exit codes: 0–125 subagent's own (0 when it reported), 64 bad or missing handoff, 66 unreadable handoff, 69 no daemon, 75 temporary failure (now also "interactive subagent finished without a report"), 77 denied, 78 refused.
- `dispatch report` exit codes: 0 delivered, 64 empty report, 66 unreadable file, 69 no daemon, 75 nobody waiting or unknown pane, 78 not a subagent or already reported.
- Shipped harness files change only through the superseded mechanism: before editing `crates/dispatch-config/harnesses/<id>.toml`, copy it byte for byte to `harnesses/superseded/<id>-<next>.toml` and add that file to `superseded` in `crates/dispatch-config/src/defaults.rs`.
- Comments and docs follow the repo's voice: full sentences explaining *why*, no marketing words.
- Every commit message ends with these two lines (the `git commit -m` examples below show only the subject; add a body and these lines):
  ```
  Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_013GkF7AMxpukR9qX1dQFbFK
  ```
- Run `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` before each commit. Run `cargo test --workspace` (not `-p`) for anything touching `dispatch/tests/` or `dispatchd/tests/`: those tests run the sibling binaries the last workspace build left.

## Review Focus

- **A handoff saved with Windows line endings (`\r\n`)** must parse exactly like one with `\n`. Pinned in Task 1.
- **A `## Goal` line inside a fenced code block** in Context (a parent pasting an example handoff) must not count as a second Goal. Pinned in Task 1.
- **A report over 1 MiB containing multi-byte characters** must be cut on a character boundary, never panic. Pinned in Task 5.
- **The user closing an interactive subagent's pane before it reports** must answer the caller (exit 75, "stopped before it finished"), not leave it waiting 24 hours. Pinned in Task 6.
- **A report piped in with bytes that are not UTF-8** must still be delivered (lossily), not refused as unreadable. Pinned in Task 7.

## Conflict warning

Another session is working on the interface on branch `tui-settings` (pointer support, including approval-prompt mouse handling in `dispatch/src/app.rs` and `dispatch/src/approval.rs`). Tasks 8 and 9 touch those files. Keep their edits small and local, and expect to rebase them.

## File map

| File | Change |
|---|---|
| `crates/dispatch-core/src/handoff.rs` | **new**: `Handoff`, `Section`, `Problem`, `HandoffError`, `TEMPLATE`, `MAX_HANDOFF_BYTES`, `brief` |
| `crates/dispatch-core/src/handoff/tests.rs` | **new**: its tests |
| `crates/dispatch-core/src/lib.rs` | export the module |
| `crates/dispatch-core/src/pane.rs` | `Pane::reported` (Task 9) |
| `crates/dispatch-core/src/state.rs` | `AppState::set_reported` (Task 9) |
| `crates/dispatch-proto/src/message.rs`, `lib.rs`, `message/tests.rs` | new fields, `ReportOutcome`, three messages |
| `crates/dispatch-config/src/config.rs`, `config/tests.rs` | `OnDone`, `DelegationLimits::interactive_on_done`, known keys |
| `crates/dispatch-config/src/harness.rs`, `settings.rs`, `tests.rs` | `[task.interactive]`, `interactive_launch_with`, `interactive_refusal_as` |
| `crates/dispatch-config/harnesses/*.toml`, `superseded/`, `src/defaults.rs` | shipped interactive forms |
| `crates/dispatch-daemon/src/delegation.rs`, `delegation/tests.rs` | `Pending` carries the handoff; `refusal` knows interactive |
| `crates/dispatch-daemon/src/pane.rs` | `DaemonPane::interactive`, `DaemonPane::reported` |
| `crates/dispatch-daemon/src/session.rs`, `session/tests.rs` | require handoff, reports, interactive lifecycle |
| `crates/dispatch-daemon/src/outbox.rs` | cost of the new payloads |
| `dispatch/src/delegate.rs` | `--handoff`, `--template`, `--interactive`, report printing |
| `dispatch/src/report.rs` | **new**: `dispatch report` |
| `dispatch/src/main.rs` | CLI wiring |
| `dispatch/tests/delegate_shim.rs`, `dispatchd/tests/serves_clients.rs` | fixtures and end-to-end tests |
| `dispatch/src/approval.rs`, `dispatch/src/app.rs` | sections in the prompt; reported mark and prompt |
| `crates/dispatch-tui/src/sidebar.rs` | reported glyph |
| `docs/delegation.md`, `docs/configuration.md`, `README.md` | user docs |

---

### Task 1: The handoff type

**Files:**
- Create: `crates/dispatch-core/src/handoff.rs`
- Create: `crates/dispatch-core/src/handoff/tests.rs`
- Modify: `crates/dispatch-core/src/lib.rs`

**Interfaces:**
- Consumes: `dispatch_core::PaneId` (has `Display`).
- Produces:
  - `pub const MAX_HANDOFF_BYTES: usize`
  - `pub const TEMPLATE: &str`
  - `pub enum Section { Goal, Context, Constraints, DoneWhen, ReportBack }` with `Section::ALL`, `Section::PROMPT_ORDER`, `fn heading(self) -> &'static str`
  - `pub enum Problem { Missing(Section), Empty(Section), Unfilled(Section), Repeated(Section), TooLarge { bytes: usize } }` with `Display`
  - `pub struct HandoffError { pub problems: Vec<Problem> }` with `Display` and `std::error::Error`
  - `pub struct Handoff { pub goal, pub context, pub constraints, pub done_when, pub report_back, pub text: String }`, `Serialize`/`Deserialize`/`Clone`/`PartialEq`/`Eq`/`Debug`
  - `Handoff::parse(text: &str) -> Result<Handoff, HandoffError>`
  - `Handoff::section(&self, Section) -> &str`
  - `Handoff::brief(&self, parent: PaneId) -> String`
  - re-exported from `dispatch_core`: `Handoff`, `HandoffError`, `Problem`, `Section`

- [ ] **Step 1: Write the failing tests**

`crates/dispatch-core/src/handoff/tests.rs`:

```rust
use super::*;

fn complete() -> String {
    "## Goal\nWrite the tests.\n\n## Context\nThe client is in src/http.rs.\n\n\
     ## Constraints\nNone.\n\n## Done when\ncargo test passes.\n\n\
     ## Report back\nWhich tests were added.\n"
        .to_string()
}

#[test]
fn a_complete_handoff_parses_into_its_sections() {
    let handoff = Handoff::parse(&complete()).expect("complete");
    assert_eq!(handoff.goal, "Write the tests.");
    assert_eq!(handoff.context, "The client is in src/http.rs.");
    assert_eq!(handoff.constraints, "None.");
    assert_eq!(handoff.done_when, "cargo test passes.");
    assert_eq!(handoff.report_back, "Which tests were added.");
    assert_eq!(handoff.text, complete(), "the file travels verbatim");
}

#[test]
fn headings_match_whatever_their_case_and_order() {
    let text = "## report BACK\nr\n## done when\nd\n## CONSTRAINTS\nc\n## context\nx\n## goal\ng\n";
    let handoff = Handoff::parse(text).expect("complete");
    assert_eq!(handoff.goal, "g");
    assert_eq!(handoff.report_back, "r");
}

#[test]
fn every_missing_section_is_named() {
    let error = Handoff::parse("## Goal\ng\n").expect_err("incomplete");
    assert_eq!(
        error.problems,
        vec![
            Problem::Missing(Section::Context),
            Problem::Missing(Section::Constraints),
            Problem::Missing(Section::DoneWhen),
            Problem::Missing(Section::ReportBack),
        ]
    );
}

#[test]
fn an_empty_section_is_refused() {
    let text = complete().replace("None.", "  \n");
    let error = Handoff::parse(&text).expect_err("empty constraints");
    assert_eq!(error.problems, vec![Problem::Empty(Section::Constraints)]);
}

#[test]
fn a_repeated_heading_is_refused() {
    let text = format!("{}## Goal\nagain\n", complete());
    let error = Handoff::parse(&text).expect_err("two goals");
    assert_eq!(error.problems, vec![Problem::Repeated(Section::Goal)]);
}

#[test]
fn the_unfilled_template_is_refused_section_by_section() {
    let error = Handoff::parse(TEMPLATE).expect_err("placeholders only");
    assert_eq!(
        error.problems,
        Section::ALL.map(Problem::Unfilled).to_vec(),
        "an agent that sends the template back unchanged is told so"
    );
}

#[test]
fn text_before_the_first_heading_and_other_headings_are_kept() {
    let text = format!("# Handoff for the tests\n\n{}", complete())
        .replace("The client is in src/http.rs.", "Intro.\n\n## Files\nsrc/http.rs");
    let handoff = Handoff::parse(&text).expect("complete");
    assert_eq!(handoff.context, "Intro.\n\n## Files\nsrc/http.rs");
    assert!(handoff.text.starts_with("# Handoff for the tests"));
}

#[test]
fn windows_line_endings_parse_the_same() {
    let crlf = complete().replace('\n', "\r\n");
    let handoff = Handoff::parse(&crlf).expect("complete");
    assert_eq!(handoff.goal, "Write the tests.");
    assert_eq!(handoff.report_back, "Which tests were added.");
}

#[test]
fn a_heading_inside_a_fenced_block_is_content() {
    let text = complete().replace(
        "The client is in src/http.rs.",
        "An example:\n```markdown\n## Goal\nnot this one\n```",
    );
    let handoff = Handoff::parse(&text).expect("one goal, outside the fence");
    assert_eq!(handoff.goal, "Write the tests.");
    assert!(handoff.context.contains("not this one"));
}

#[test]
fn a_handoff_over_the_limit_is_refused_before_anything_else() {
    let text = format!("{}{}", complete(), "x".repeat(MAX_HANDOFF_BYTES));
    let error = Handoff::parse(&text).expect_err("too large");
    assert_eq!(
        error.problems,
        vec![Problem::TooLarge { bytes: text.len() }]
    );
}

#[test]
fn the_brief_wraps_the_handoff_in_the_fixed_preamble() {
    let handoff = Handoff::parse(&complete()).expect("complete");
    let parent: PaneId = "00000000-0000-0000-0000-000000000001".parse().expect("an id");
    let brief = handoff.brief(parent);

    assert!(brief.starts_with(
        "You are a subagent started by Dispatch for the agent in pane \
         00000000-0000-0000-0000-000000000001.\n"
    ));
    assert!(brief.contains("run `dispatch report` with your report"));
    assert!(brief.ends_with(&complete()), "the handoff follows verbatim");
}

#[test]
fn an_error_lists_every_problem_in_words() {
    let error = Handoff::parse("## Goal\n\n").expect_err("incomplete");
    let words = error.to_string();
    assert!(words.contains("Goal is empty"), "{words}");
    assert!(words.contains("Report back is missing"), "{words}");
}
```

If `PaneId` does not parse from a string, build it the way `crates/dispatch-core/src/id.rs` allows (look for `FromStr` or a `from_u128`-style constructor) and assert on `format!("{parent}")` instead of the literal.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dispatch-core handoff`
Expected: compile error, `handoff` module not found.

- [ ] **Step 3: Write the module**

`crates/dispatch-core/src/handoff.rs`:

```rust
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
                write!(f, "{} still holds the template's placeholder", section.heading())
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
```

Note the `Repeated` check: a repeated section's later body is appended to the first, which does not matter because the handoff is refused. The sort is stable, so `Repeated(Goal)` and `Missing(Goal)` cannot both occur.

In `crates/dispatch-core/src/lib.rs`, add `pub mod handoff;` with the other modules and `pub use handoff::{Handoff, HandoffError, Problem, Section};` with the other re-exports.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dispatch-core handoff`
Expected: all 12 pass.

- [ ] **Step 5: Commit**

```bash
git add crates/dispatch-core/src/handoff.rs crates/dispatch-core/src/handoff/tests.rs crates/dispatch-core/src/lib.rs
git commit -m "feat(core): a handoff with five required sections, and the brief built from it"
```

---

### Task 2: Protocol additions

**Files:**
- Modify: `crates/dispatch-proto/src/message.rs`
- Modify: `crates/dispatch-proto/src/lib.rs`
- Modify: `crates/dispatch-proto/src/message/tests.rs`
- Modify (compile fixes only): `crates/dispatch-daemon/src/session.rs`, `crates/dispatch-daemon/src/session/tests.rs`, `crates/dispatch-daemon/src/outbox.rs`, `dispatchd/tests/serves_clients.rs`, `dispatch/src/delegate.rs`, `dispatch/src/app.rs`, `dispatch/tests/delegate_shim.rs`
- Modify: `docs/superpowers/specs/2026-09-30-delegation-handoff-design.md` (one line)

**Interfaces:**
- Consumes: `dispatch_core::Handoff` (Task 1).
- Produces:
  - `ClientMessage::DelegateRequest { parent, harness, task, size, handoff: Option<Handoff>, interactive: bool }`
  - `ClientMessage::DelegateReport { pane: PaneId, report: String }`
  - `ServerMessage::DelegatePending { …, handoff: Option<Handoff>, interactive: bool }`
  - `ServerMessage::DelegateFinished { request, exit, tail, report: Option<String> }`
  - `ServerMessage::ReportAnswered { outcome: ReportOutcome }`
  - `ServerMessage::SubagentReported { pane: PaneId }`
  - `pub enum ReportOutcome { Delivered, NotWaiting { reason: String }, Refused { reason: String }, Unknown }`, re-exported from `dispatch_proto`

- [ ] **Step 1: Write the failing tests**

Append to `crates/dispatch-proto/src/message/tests.rs` (it already has `round_trip`):

```rust
fn sample_handoff() -> dispatch_core::Handoff {
    dispatch_core::Handoff::parse(
        "## Goal\ng\n## Context\nc\n## Constraints\nNone.\n## Done when\nd\n## Report back\nr\n",
    )
    .expect("complete")
}

#[test]
fn the_handoff_messages_round_trip() {
    let pane = PaneId::new();
    let messages = vec![
        ClientMessage::DelegateRequest {
            parent: pane,
            harness: "claude".into(),
            task: "g".into(),
            size: (80, 24),
            handoff: Some(sample_handoff()),
            interactive: true,
        },
        ClientMessage::DelegateReport {
            pane,
            report: "done".into(),
        },
    ];
    for message in &messages {
        assert_eq!(&round_trip(message), message);
    }

    let server = vec![
        ServerMessage::DelegateFinished {
            request: RequestId::new(),
            exit: 0,
            tail: Vec::new(),
            report: Some("done".into()),
        },
        ServerMessage::ReportAnswered {
            outcome: ReportOutcome::NotWaiting {
                reason: "gone".into(),
            },
        },
        ServerMessage::SubagentReported { pane },
    ];
    for message in &server {
        assert_eq!(&round_trip(message), message);
    }
}

#[test]
fn a_request_from_an_older_client_has_no_handoff() {
    #[derive(serde::Serialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum Old {
        DelegateRequest {
            parent: PaneId,
            harness: String,
            task: String,
            size: (u16, u16),
        },
    }

    let bytes = rmp_serde::to_vec_named(&Old::DelegateRequest {
        parent: PaneId::new(),
        harness: "claude".into(),
        task: "do it".into(),
        size: (80, 24),
    })
    .expect("encodes");
    let decoded: ClientMessage = rmp_serde::from_slice(&bytes).expect("decodes");
    assert!(matches!(
        decoded,
        ClientMessage::DelegateRequest {
            handoff: None,
            interactive: false,
            ..
        }
    ));
}

#[test]
fn an_older_client_skips_the_report_messages() {
    #[derive(serde::Deserialize, Debug)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum Old {
        Pong {},
        #[serde(other)]
        Unknown,
    }

    for message in [
        ServerMessage::SubagentReported {
            pane: PaneId::new(),
        },
        ServerMessage::ReportAnswered {
            outcome: ReportOutcome::Delivered,
        },
    ] {
        let bytes = rmp_serde::to_vec_named(&message).expect("encodes");
        let decoded: Old = rmp_serde::from_slice(&bytes).expect("an older peer decodes it");
        assert!(matches!(decoded, Old::Unknown));
    }
}
```

Match the file's existing imports (`PaneId`, `RequestId`, `rmp_serde`); check how `a_message_from_a_newer_peer_is_skipped_rather_than_fatal` encodes and follow it if `to_vec_named` is not what the file uses.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dispatch-proto`
Expected: compile errors on the missing fields and variants.

- [ ] **Step 3: Add the fields and messages**

In `ClientMessage::DelegateRequest`, after `size`:

```rust
        /// The handoff, checked by the caller and checked again here. An
        /// older client sends none and is refused: a bare task is no longer
        /// enough to delegate with.
        #[serde(default)]
        handoff: Option<dispatch_core::Handoff>,
        /// Whether to run the subagent in its harness's own interface rather
        /// than its one-shot form.
        #[serde(default)]
        interactive: bool,
```

Change the doc on `task` to: `/// The handoff's text, for an older daemon that knows nothing of handoffs.`

Add to `ClientMessage`, after `DelegateDecision`:

```rust
    /// A subagent's report, sent by `dispatch report` from inside its pane.
    ///
    /// The report is what the parent asked for: delivering it answers the
    /// waiting `dispatch delegate` at once, without waiting for the process
    /// to exit.
    DelegateReport {
        /// The reporting pane, from `DISPATCH_PANE` in its environment.
        pane: PaneId,
        /// The report.
        report: String,
    },
```

In `ServerMessage::DelegatePending`, after `depth`:

```rust
        /// The handoff, for showing its sections. `task` carries its text
        /// for an older client.
        #[serde(default)]
        handoff: Option<dispatch_core::Handoff>,
        /// Whether the subagent would run in its own interface.
        #[serde(default)]
        interactive: bool,
```

In `ServerMessage::DelegateFinished`, after `tail`:

```rust
        /// The subagent's report, when it sent one. The caller prints this
        /// rather than the tail.
        #[serde(default)]
        report: Option<String>,
```

Add to `ServerMessage`, after `DelegateFinished`:

```rust
    /// Answers a [`ClientMessage::DelegateReport`].
    ReportAnswered {
        /// What became of it.
        outcome: ReportOutcome,
    },

    /// An interactive subagent has reported and waits for the user to close
    /// it, under `interactive_on_done = "ask"`.
    ///
    /// Its own message rather than a new [`PaneStatus`]: an older client
    /// skips an unknown message, and loses only the mark.
    SubagentReported {
        /// Which pane.
        pane: PaneId,
    },
```

Add, after `DelegateOutcome`:

```rust
/// What became of a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReportOutcome {
    /// The caller has it.
    Delivered,
    /// Nobody is waiting for it any more. Asking again will not help, but the
    /// work is not wrong: the caller went away.
    NotWaiting {
        /// Why, in words.
        reason: String,
    },
    /// This pane cannot report: it is not a subagent, or it already has.
    Refused {
        /// Why, in words.
        reason: String,
    },
    /// An outcome this build does not know.
    #[serde(other)]
    Unknown,
}
```

In `crates/dispatch-proto/src/lib.rs`, add `ReportOutcome` to the `pub use message::{…}` list. Leave `VERSION` alone.

- [ ] **Step 4: Fix every construction site so the workspace builds**

Run `cargo build --workspace --all-targets` and fix each error mechanically, behaviour unchanged:
- every `ClientMessage::DelegateRequest { … }` literal gains `handoff: None, interactive: false,` (`dispatch/src/delegate.rs`, nine in `session/tests.rs`, one in `serves_clients.rs`, the proto tests);
- every `ServerMessage::DelegatePending { … }` literal gains `handoff: None, interactive: false,` (`session.rs` `delegate_request`);
- every `ServerMessage::DelegateFinished { … }` literal gains `report: None,` (`session.rs` twice);
- `delegate_request` in `session.rs` receives the new fields in the `ClientMessage::DelegateRequest` match arm as `handoff: _, interactive: _` for now;
- add `ClientMessage::DelegateReport { .. } => {}` to the daemon's `handle_request` match with a `// Task 5.` comment, and handle the new `ServerMessage` variants wherever a match is exhaustive (`dispatch/src/app.rs`, `dispatch/src/delegate.rs`, `crates/dispatch-client`) with `=> {}` or `=> false`, matching what that match does for `Unknown`;
- in `crates/dispatch-daemon/src/outbox.rs`, the cost function gains `ServerMessage::DelegateFinished { tail, report, .. } => tail.len() + report.as_ref().map_or(0, String::len)`.

- [ ] **Step 5: Correct the spec**

In `docs/superpowers/specs/2026-09-30-delegation-handoff-design.md`, replace the line `- The protocol's minor version goes up.` with:

```markdown
- The protocol version stays 1.1: `dispatch-proto` bumps it only for a change
  an older peer cannot safely ignore, and optional fields and new messages are
  not such a change.
```

- [ ] **Step 6: Run the tests**

Run: `cargo test --workspace`
Expected: all pass, including the three new proto tests.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(proto): carry a handoff, a report, and the reported mark on the wire"
```

---

### Task 3: Configuration and harness forms

**Files:**
- Modify: `crates/dispatch-config/src/config.rs`, `crates/dispatch-config/src/config/tests.rs`
- Modify: `crates/dispatch-config/src/harness.rs`, `crates/dispatch-config/src/settings.rs`, `crates/dispatch-config/src/tests.rs`
- Modify: `crates/dispatch-config/harnesses/{claude,codex,agy,opencode}.toml`
- Create: `crates/dispatch-config/harnesses/superseded/{claude-8,codex-8,agy-4,opencode-2}.toml`
- Modify: `crates/dispatch-config/src/defaults.rs`
- Modify: `crates/dispatch-config/src/lib.rs` (re-export `OnDone`)

**Interfaces:**
- Produces:
  - `pub enum OnDone { Close, Ask }` (`#[serde(rename_all = "lowercase")]`, `Default = Close`), re-exported from `dispatch_config`
  - `DelegationLimits::interactive_on_done: OnDone`
  - `pub struct InteractiveLaunch { pub args: Vec<String>, pub platform: BTreeMap<String, InteractiveArgs> }`, `pub struct InteractiveArgs { pub args: Vec<String> }`
  - `TaskLaunch::interactive: Option<InteractiveLaunch>` (TOML `[task.interactive]`)
  - `HarnessDef::interactive_launch_for(&self, os: &str, task: &str) -> Option<Launch>`
  - `HarnessDef::interactive_launch_with(&self, os: &str, task: &str, values: &Choices) -> Option<Launch>` (in `settings.rs`, beside `task_launch_with`)
  - `HarnessDef::interactive_refusal_as(&self, os: &str, launch: &Launch) -> Option<String>`

Before editing, check the next free superseded numbers: `ls crates/dispatch-config/harnesses/superseded/`. The numbers above assume claude and codex end at 7, agy at 3, opencode at 1. Use whatever is next.

- [ ] **Step 1: Write the failing config tests**

Append to `crates/dispatch-config/src/config/tests.rs`, following how the file loads a `config.toml` from a `TempDir` (look at an existing `[delegation]` test and copy its loading call):

```rust
#[test]
fn interactive_subagents_close_on_report_by_default() {
    assert_eq!(
        DelegationLimits::default().interactive_on_done,
        OnDone::Close
    );
}

#[test]
fn interactive_on_done_can_ask_instead() {
    let config: Config = toml::from_str("[delegation]\ninteractive_on_done = \"ask\"\n")
        .expect("valid");
    assert_eq!(config.delegation.interactive_on_done, OnDone::Ask);
}

#[test]
fn an_unknown_on_done_value_is_an_error_naming_both_choices() {
    let error = toml::from_str::<Config>("[delegation]\ninteractive_on_done = \"keep\"\n")
        .expect_err("not a choice");
    let words = error.to_string();
    assert!(words.contains("close") && words.contains("ask"), "{words}");
}

#[test]
fn interactive_on_done_is_a_known_key() {
    let raw: toml::Table = toml::from_str("[delegation]\ninteractive_on_done = \"ask\"\n")
        .expect("valid");
    assert!(unknown_keys(&raw).is_empty());
}
```

If `Config` is not deserialised directly with `toml::from_str` in this file's tests, use the loader the other tests use; the assertions stay the same.

- [ ] **Step 2: Write the failing harness tests**

Append to `crates/dispatch-config/src/tests.rs`, following its existing harness-parsing tests (they build a `HarnessDef` from a TOML string):

```rust
fn interactive_harness() -> HarnessDef {
    toml::from_str(
        "id = \"agent\"\ndisplay_name = \"Agent\"\ncommand = \"agent\"\n\n\
         [platform.windows]\ncommand = \"cmd.exe\"\nargs = [\"/c\", \"agent\"]\n\n\
         [task]\nargs = [\"-p\", \"{task}\"]\n\n\
         [task.interactive]\nargs = [\"-i\", \"{task}\"]\n\n\
         [task.interactive.platform.windows]\nargs = [\"/c\", \"agent\", \"-i\", \"{task}\"]\n",
    )
    .expect("valid")
}

#[test]
fn an_interactive_form_puts_the_task_in_its_arguments() {
    let launch = interactive_harness()
        .interactive_launch_for("linux", "the brief")
        .expect("has a form");
    assert_eq!(launch.command, "agent");
    assert_eq!(launch.args, vec!["-i".to_string(), "the brief".to_string()]);
}

#[test]
fn a_harness_without_an_interactive_form_has_none() {
    let mut def = interactive_harness();
    def.task.as_mut().expect("has [task]").interactive = None;
    assert!(def.interactive_launch_for("linux", "x").is_none());
}

#[test]
fn an_interactive_form_that_never_names_the_task_is_no_form() {
    let mut def = interactive_harness();
    def.task.as_mut().expect("has [task]").interactive = Some(InteractiveLaunch {
        args: vec!["-i".into()],
        platform: Default::default(),
    });
    assert!(
        def.interactive_launch_for("linux", "x").is_none(),
        "the handoff would never reach the agent"
    );
}

#[test]
fn an_interactive_form_through_cmd_is_refused_on_windows() {
    let def = interactive_harness();
    let launch = def
        .interactive_launch_for("windows", "x & y")
        .expect("has a windows form");
    let reason = def
        .interactive_refusal_as("windows", &launch)
        .expect("refused");
    assert!(reason.contains("cmd.exe"), "{reason}");
    assert!(def.interactive_refusal_as("linux", &launch).is_none());
}

#[test]
fn every_shipped_harness_has_an_interactive_form() {
    for built_in in crate::defaults::BUILT_INS {
        let def: HarnessDef = toml::from_str(built_in.toml).expect("ships valid");
        assert!(
            def.interactive_launch_for("linux", "the brief").is_some(),
            "{} ships no [task.interactive]",
            built_in.id
        );
    }
}
```

And in `crates/dispatch-config/src/settings/tests.rs`, beside the `task_launch_with` tests (around line 437), one test that settings are appended:

```rust
#[test]
fn an_interactive_launch_gets_the_saved_settings_too() {
    let def: HarnessDef = toml::from_str(
        "id = \"agent\"\ndisplay_name = \"Agent\"\ncommand = \"agent\"\n\n\
         [task]\nargs = [\"-p\", \"{task}\"]\n\n\
         [task.interactive]\nargs = [\"{task}\"]\n\n\
         [[settings]]\nkey = \"model\"\nlabel = \"Model\"\nkind = \"text\"\nargs = [\"--model\", \"{value}\"]\n",
    )
    .expect("valid");
    let mut values = Choices::new();
    values.insert("model".into(), "opus".into());

    let launch = def
        .interactive_launch_with("linux", "the brief", &values)
        .expect("has a form");
    assert_eq!(launch.args, vec!["the brief", "--model", "opus"]);
}
```

Use whatever `kind` and fields the neighbouring settings tests use for a text setting.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p dispatch-config`
Expected: compile errors (`OnDone`, `InteractiveLaunch`, methods missing).

- [ ] **Step 4: Implement `OnDone`**

In `crates/dispatch-config/src/config.rs`:

```rust
/// What happens to an interactive subagent's pane once it has reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnDone {
    /// It closes itself, as a one-shot subagent ends on its own.
    #[default]
    Close,
    /// It stays open, marked as reported, and the user is asked before it
    /// closes.
    Ask,
}
```

Add to `DelegationLimits`:

```rust
    /// What happens to an interactive subagent's pane once it has reported.
    pub interactive_on_done: OnDone,
```

and `interactive_on_done: OnDone::Close,` to its `Default`. Add `"interactive_on_done"` to the `DELEGATION` known-keys array (its length becomes 4). Re-export `OnDone` from `crates/dispatch-config/src/lib.rs` next to `DelegationLimits`.

Serde's error for an unknown variant already names the expected ones (`expected one of \`close\`, \`ask\``), which is what the third test checks.

- [ ] **Step 5: Implement the interactive form**

In `crates/dispatch-config/src/harness.rs`, after `TaskLaunch`:

```rust
/// How to start a harness's own interface on a delegated task.
///
/// The handoff becomes the agent's first prompt, and the agent stays open:
/// the user can watch it and type into it. It never exits by itself, so the
/// daemon knows it is finished when it runs `dispatch report`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InteractiveLaunch {
    /// Arguments, with `{task}` standing for the brief.
    #[serde(default)]
    pub args: Vec<String>,
    /// Per-platform overrides, keyed by `std::env::consts::OS`.
    #[serde(default)]
    pub platform: BTreeMap<String, InteractiveArgs>,
}

/// One platform's interactive form.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InteractiveArgs {
    /// Arguments, with `{task}` standing for the brief.
    #[serde(default)]
    pub args: Vec<String>,
}
```

Add to `TaskLaunch`:

```rust
    /// The interactive form, `[task.interactive]`, for `dispatch delegate
    /// --interactive`.
    #[serde(default)]
    pub interactive: Option<InteractiveLaunch>,
```

Fix any `TaskLaunch { … }` literals the compiler finds with `interactive: None`.

Add to `impl HarnessDef`, after `task_form_for`:

```rust
    /// The launch for running this harness's own interface on `task`, on a
    /// named platform.
    ///
    /// `None` when it has no interactive form there, or one that never names
    /// `{task}`: the brief has no other way in, since the agent's standard
    /// input is its terminal.
    #[must_use]
    pub fn interactive_launch_for(&self, os: &str, task: &str) -> Option<Launch> {
        let form = self.task.as_ref()?.interactive.as_ref()?;
        let args = form.platform.get(os).map_or(&form.args, |over| &over.args);
        if !args.iter().any(|arg| arg.contains("{task}")) {
            return None;
        }

        let mut launch = self.launch_for(os);
        launch.args = args.iter().map(|arg| arg.replace("{task}", task)).collect();
        Some(launch)
    }

    /// Why this harness's interactive form must not start as `launch` on
    /// `os`, if it must not.
    ///
    /// The one-shot form escapes `cmd.exe` by reading its task from a file
    /// on standard input. An interactive agent cannot: its standard input is
    /// the terminal. So on Windows any interactive form reached through
    /// `cmd.exe` is refused outright, rather than letting a handoff's `&`
    /// and `%VAR%` run as commands.
    #[must_use]
    pub fn interactive_refusal_as(&self, os: &str, launch: &Launch) -> Option<String> {
        if os != "windows" {
            return None;
        }
        if !runs_through_cmd(&launch.command)
            && !runs_through_cmd(&found_as(launch).to_string_lossy())
        {
            return None;
        }

        Some(format!(
            "harness {id:?} would put the handoff on cmd.exe's command line, where characters \
             like & and % run as commands, and an interactive agent cannot read it from a file \
             instead. Interactive delegation is not available for {id} on Windows; delegate \
             without --interactive",
            id = self.id
        ))
    }
```

In `crates/dispatch-config/src/settings.rs`, after `task_launch_with`:

```rust
    /// The interactive launch of `task` on `os`, with the flags and variables
    /// `values` turn on added after its own, as for any pane.
    #[must_use]
    pub fn interactive_launch_with(&self, os: &str, task: &str, values: &Choices) -> Option<Launch> {
        let mut launch = self.interactive_launch_for(os, task)?;
        self.apply_settings(&mut launch, values);
        Some(launch)
    }
```

- [ ] **Step 6: Ship the forms**

For each of `claude`, `codex`, `agy`, `opencode`:
1. `cp crates/dispatch-config/harnesses/<id>.toml crates/dispatch-config/harnesses/superseded/<id>-<next>.toml`
2. Add `include_str!("../harnesses/superseded/<id>-<next>.toml"),` as the last entry of that harness's `superseded` list in `crates/dispatch-config/src/defaults.rs`.
3. Add the form to `<id>.toml`, directly after its `[task]` block (or, for agy and opencode, which have no `[task]`, before the first `[[settings]]`):

claude.toml and codex.toml:

```toml
# `dispatch delegate --interactive` starts the agent's own interface with
# the handoff as its first prompt, so it can be watched and typed into. It
# says it is finished by running `dispatch report`. There is no Windows form:
# the prompt would sit on cmd.exe's command line, and an interactive agent
# cannot read it from a file instead.
[task.interactive]
args = ["{task}"]
```

agy.toml (same comment):

```toml
[task.interactive]
args = ["-i", "{task}"]
```

opencode.toml (same comment):

```toml
[task.interactive]
args = ["--prompt", "{task}"]
```

agy and opencode gain a `[task]` table with no `args` this way, which `task_launch_for` already treats as no one-shot form, so one-shot delegation to them is still refused. Check `crates/dispatch-config/src/tests.rs` for a test asserting agy or opencode have `task: None`; if one exists, change it to assert `task_launch_for("linux", "x").is_none()` instead.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p dispatch-config`
Expected: all pass, including the upgrade tests that check each superseded body is replaced.

- [ ] **Step 8: Commit**

```bash
git add -A crates/dispatch-config
git commit -m "feat(config): an interactive form per harness, and what happens once it reports"
```

---

### Task 4: The daemon requires a handoff and hands over the brief

**Files:**
- Modify: `crates/dispatch-daemon/src/delegation.rs`, `crates/dispatch-daemon/src/delegation/tests.rs`
- Modify: `crates/dispatch-daemon/src/session.rs`
- Modify: `crates/dispatch-daemon/src/session/tests.rs`
- Modify: `dispatchd/tests/serves_clients.rs`

**Interfaces:**
- Consumes: `Handoff::parse`, `Handoff::brief` (Task 1); the Task 2 fields.
- Produces:
  - `delegation::Pending { …, handoff: Handoff, interactive: bool }` (field `task` removed)
  - `delegation::refusal(depth, live, limits, has_form, harness, interactive) -> Option<String>`
  - `delegation::NO_HANDOFF: &str`
  - `Daemon::delegate_request(caller, parent, harness, handoff: Option<Handoff>, interactive: bool, size)`
  - `Daemon::task_run(&self, harness: &str, brief: &str, pane: PaneId, interactive: bool) -> Option<TaskRun>`
  - test helper `handoff_for(goal: &str) -> Handoff` in `session/tests.rs`

**The test fixture problem.** Today's test harness is `sh -c {task}`: the task *is* the script. After this task the subagent receives a brief (preamble plus handoff), which is not a script. So the fixture harness becomes a small script that pulls the Goal section out of the brief and runs it. Tests keep writing shell commands; they now travel as the Goal.

- [ ] **Step 1: Replace the session-test fixture harness**

In `crates/dispatch-daemon/src/session/tests.rs`, replace the `body` written to `shell.toml` in `harnesses()` with one that runs a goal-runner script written beside it:

```rust
    // The subagent is handed a brief: a preamble, then the handoff. These
    // runners pull the Goal section out of it and run that as a script, so
    // a test still says what the subagent does as a shell command.
    let runner = if cfg!(windows) {
        let path = dir.join("run-goal.ps1");
        std::fs::write(
            &path,
            "$brief = Get-Content -Raw -LiteralPath $env:DISPATCH_TASK_FILE.Trim('\"')\n\
             $m = [regex]::Match($brief, '(?ms)^## Goal\\s*\\r?\\n(.*?)(?=^## |\\z)')\n\
             Invoke-Expression $m.Groups[1].Value.Trim()\n",
        )
        .expect("temp dir is writable");
        path
    } else {
        let path = dir.join("run-goal.sh");
        std::fs::write(
            &path,
            "goal=$(printf '%s\\n' \"$1\" | awk '/^## Goal[[:space:]]*$/{f=1;next} /^## /{f=0} f')\n\
             eval \"$goal\"\n",
        )
        .expect("temp dir is writable");
        path
    };
    let body = if cfg!(windows) {
        format!(
            "id = \"shell\"\ndisplay_name = \"Shell\"\ncommand = \"cmd.exe\"\n\n[task]\n\
             args = [\"/d\", \"/v:off\", \"/c\", \"powershell.exe\", \"-NoProfile\", \"-NonInteractive\", \
             \"-ExecutionPolicy\", \"Bypass\", \"-File\", '{}']\ninput = \"file\"\n",
            runner.display()
        )
    } else {
        format!(
            "id = \"shell\"\ndisplay_name = \"Shell\"\ncommand = \"sh\"\n\n[task]\n\
             args = ['{}', \"{{task}}\"]\n\n[task.interactive]\nargs = ['{}', \"{{task}}\"]\n",
            runner.display(),
            runner.display()
        )
    };
```

(`create_dir_all(dir)` must run before these writes; move it up if needed.) TOML literal strings (`'…'`) keep Windows backslashes intact. The Unix harness also gets a `[task.interactive]` form running the same script, for Task 6. The interactive Unix form passes the brief as an argument, which the runner reads as `$1`.

The `no-task-args` harness is unchanged.

- [ ] **Step 2: Send handoffs from the test helpers**

In the same file, add:

```rust
/// A complete handoff whose Goal is `goal`, which the test harness runs as a
/// script.
fn handoff_for(goal: &str) -> dispatch_core::Handoff {
    dispatch_core::Handoff::parse(&format!(
        "## Goal\n{goal}\n\n## Context\nA test.\n\n## Constraints\nNone.\n\n\
         ## Done when\nIt has run.\n\n## Report back\nNothing.\n"
    ))
    .expect("a complete handoff")
}
```

Change `ask_as` to send it:

```rust
    daemon.request_for_test(
        id,
        ClientMessage::DelegateRequest {
            parent,
            harness: "shell".into(),
            task: task.into(),
            size: (80, 24),
            handoff: Some(handoff_for(task)),
            interactive: false,
        },
    );
```

Change the other eight `DelegateRequest` literals in the file the same way: `handoff: Some(handoff_for(<their task string>))`.

In `a_delegation_request_is_put_to_the_user`, the assertion on `DelegatePending { task, .. } if task == "echo delegated"` becomes:

```rust
            Some(ServerMessage::DelegatePending { handoff: Some(handoff), .. })
                if handoff.goal == "echo delegated"
```

Do the same fixture change (goal-runner script and `handoff: Some(…)`) in `dispatchd/tests/serves_clients.rs`: its `shell.toml` and its one `DelegateRequest` (goal `echo delegated-$((6*7))`). Copy the two runner scripts and the `handoff_for` helper there; it is a separate crate and cannot share them.

- [ ] **Step 3: Write the failing tests**

Append to `session/tests.rs`:

```rust
#[test]
fn a_request_without_a_handoff_is_refused_without_asking() {
    let (mut daemon, project, _dir) = daemon("delegate-bare");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "old delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: "shell".into(),
            task: "echo bare".into(),
            size: (80, 24),
            handoff: None,
            interactive: false,
        },
    );

    let reason = refusal(&drain(&caller)).expect("refused");
    assert!(reason.contains("--handoff"), "{reason}");
    assert!(pending(&drain(&ui)).is_none(), "nobody is asked");
}

#[test]
fn a_handoff_with_problems_is_refused_even_when_the_caller_skipped_the_check() {
    let (mut daemon, project, _dir) = daemon("delegate-broken");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let mut broken = handoff_for("echo x");
    broken.text = "## Goal\necho x\n".into();

    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "careless".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: "shell".into(),
            task: String::new(),
            size: (80, 24),
            handoff: Some(broken),
            interactive: false,
        },
    );

    let reason = refusal(&drain(&caller)).expect("refused");
    assert!(reason.contains("Context is missing"), "{reason}");
}

#[test]
fn the_subagent_is_handed_the_brief_not_the_bare_goal() {
    let (mut daemon, project, _dir) = daemon("delegate-brief");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    // The Goal prints the subagent's own first argument (Unix) or task file
    // (Windows): the brief exactly as the harness received it.
    let goal = if cfg!(windows) {
        "Get-Content -Raw -LiteralPath $env:DISPATCH_TASK_FILE.Trim('\"')"
    } else {
        "printf '%s\\n' \"$1\""
    };
    let caller = ask(&mut daemon, parent, goal);
    let request = pending(&drain(&ui)).expect("asked");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );

    let seen = wait_for(&mut daemon, &caller, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. }))
    });
    let tail = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::DelegateFinished { tail, .. } => Some(String::from_utf8_lossy(tail).into_owned()),
            _ => None,
        })
        .expect("finished");
    assert!(tail.contains("You are a subagent started by Dispatch"), "{tail}");
    assert!(tail.contains("## Report back"), "{tail}");
}

#[test]
fn the_prompt_carries_the_handoff() {
    let (mut daemon, project, _dir) = daemon("delegate-prompt-handoff");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let _caller = ask(&mut daemon, parent, "echo shown");

    let seen = drain(&ui);
    let shown = seen.iter().find_map(|m| match m {
        ServerMessage::DelegatePending { handoff, task, interactive, .. } => {
            Some((handoff.clone(), task.clone(), *interactive))
        }
        _ => None,
    });
    let (handoff, task, interactive) = shown.expect("asked");
    let handoff = handoff.expect("the handoff travels");
    assert_eq!(handoff.goal, "echo shown");
    assert_eq!(task, handoff.text, "an older client is shown the whole handoff");
    assert!(!interactive);
}
```

`refusal()` (line ~1798) is an existing helper returning the first `Refused` reason.

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p dispatch-daemon delegate`
Expected: the four new tests fail; most existing delegation tests also fail, since the daemon still runs `task` (now a goal string) instead of the brief, and the runner finds no `## Goal`. That is the point: Step 5 makes them pass again.

- [ ] **Step 5: Implement**

In `crates/dispatch-daemon/src/delegation.rs`:
- `Pending`: replace `pub task: String` with

  ```rust
      /// What it would be asked to do.
      pub handoff: dispatch_core::Handoff,
      /// Whether it would run in its harness's own interface.
      pub interactive: bool,
  ```
- add

  ```rust
  /// Why a request with no handoff is refused.
  pub const NO_HANDOFF: &str = "a bare task is not enough to delegate with: write a handoff \
      (`dispatch delegate --template` prints one) and pass it with `--handoff FILE`, or \
      `--handoff -` for standard input";
  ```
- `refusal` gains a last parameter `interactive: bool`, and its first branch becomes:

  ```rust
      if !has_task_form {
          return Some(if interactive {
              format!(
                  "harness {harness:?} has no [task.interactive] form, so it cannot run as an \
                   interactive subagent; add one or delegate without --interactive"
              )
          } else {
              format!(
                  "harness {harness:?} has no [task] form, so it cannot be run on one task; \
                   add one or delegate to a harness that has one"
              )
          });
      }
  ```
  Update `delegation/tests.rs` call sites with `false`, and add one test that `refusal(0, 0, DelegationLimits::default(), false, "agy", true)` mentions `[task.interactive]`.

In `crates/dispatch-daemon/src/session.rs`:

- The `DelegateRequest` match arm passes everything through:

  ```rust
              ClientMessage::DelegateRequest {
                  parent,
                  harness,
                  task: _,
                  size,
                  handoff,
                  interactive,
              } => self.delegate_request(id, parent, harness, handoff, interactive, size),
  ```

- `task_run` takes the brief and the mode:

  ```rust
      fn task_run(
          &self,
          harness: &str,
          brief: &str,
          pane: PaneId,
          interactive: bool,
      ) -> Option<TaskRun> {
          let def = self.harnesses.get(harness)?;
          let values = def
              .resolve(&Choices::new(), &self.saved_settings(harness))
              .unwrap_or_default();
          let mut run = if interactive {
              // An interactive agent takes its brief as an argument: its
              // standard input is its terminal.
              TaskRun {
                  launch: def.interactive_launch_with(std::env::consts::OS, brief, &values)?,
                  input: TaskInput::Argument,
              }
          } else {
              def.task_launch_with(std::env::consts::OS, brief, &values)?
          };
          // … the rest of the existing body unchanged (pane_env, TASK_FILE_ENV) …
          Some(run)
      }
  ```

- `unsafe_task_form` takes `interactive: bool` and calls `interactive_refusal_as` when it is set, `task_refusal_as` otherwise.

- `delegate_request` takes `handoff: Option<Handoff>, interactive: bool` instead of `task: String`. Right after the `NoSuchPane` check and the empty-harness resolution:

  ```rust
          // Checked again here, whatever the caller checked: anything that can
          // reach the socket can send a request, and the parse is what the
          // brief is built from.
          let handoff = match handoff.map(|sent| Handoff::parse(&sent.text)) {
              None => {
                  self.resolve(request, caller, DelegateOutcome::Refused {
                      reason: crate::delegation::NO_HANDOFF.into(),
                  });
                  return;
              }
              Some(Err(error)) => {
                  self.resolve(request, caller, DelegateOutcome::Refused {
                      reason: error.to_string(),
                  });
                  return;
              }
              Some(Ok(handoff)) => handoff,
          };
          let brief = handoff.brief(parent);
  ```

  Then `self.task_run(&harness, &brief, PaneId::new(), interactive)`, `self.unsafe_task_form(&harness, run, interactive)`, `refusal(…, &harness, interactive)`, the blanket path `self.approve(request, parent, &harness, &handoff, interactive, size, caller, true)`, and:

  ```rust
          let announcement = ServerMessage::DelegatePending {
              request,
              parent,
              project,
              harness: harness.clone(),
              task: handoff.text.clone(),
              depth,
              handoff: Some(handoff.clone()),
              interactive,
          };
  ```

  and `Pending { …, handoff, interactive, … }`.

- `approve` takes `handoff: &Handoff, interactive: bool` instead of `task: &str`, builds `let brief = handoff.brief(parent);` and uses `self.task_run(harness, &brief, id, interactive)`, `self.unsafe_task_form(harness, run, interactive)` and `refusal(…, harness, interactive)`. The `TaskFile::write` call writes `&brief`. Nothing else in `approve` changes in this task.

- The `DelegateDecision` arm passes `&waiting.handoff, waiting.interactive`.

Add `use dispatch_core::Handoff;` at the top of `session.rs`.

- [ ] **Step 6: Run the tests**

Run: `cargo test --workspace`
Expected: all pass. If a Windows-only path cannot be run locally, say so in the commit body; CI covers it.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(daemon): refuse a bare task, and hand the subagent its handoff in a fixed brief"
```

---

### Task 5: Reports

**Files:**
- Modify: `crates/dispatch-daemon/src/pane.rs`
- Modify: `crates/dispatch-daemon/src/session.rs`
- Modify: `crates/dispatch-daemon/src/session/tests.rs`

**Interfaces:**
- Consumes: `ClientMessage::DelegateReport`, `ServerMessage::{ReportAnswered, DelegateFinished { report }}`, `ReportOutcome` (Task 2).
- Produces:
  - `DaemonPane::interactive: bool`, `DaemonPane::reported: bool`
  - `Daemon::report(&mut self, client: ClientId, pane: PaneId, report: String)`
  - `const MAX_REPORT_BYTES: usize = 1024 * 1024` and `fn cut_report(report: String) -> String` in `session.rs`
  - `live_children` counts unfinished requests

- [ ] **Step 1: Write the failing tests**

Append to `session/tests.rs`:

```rust
/// Approves the one request pending on `ui` and returns the subagent's pane.
fn approve_and_spawn(daemon: &mut Daemon, ui: &Inbox) -> PaneId {
    let request = pending(&drain(ui)).expect("asked");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );
    let seen = wait_for(daemon, ui, |m| m.iter().any(m_is_child));
    seen.iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned { pane, parent: Some(_), .. } => Some(*pane),
            _ => None,
        })
        .expect("checked by wait_for")
}

/// Sends a report from `pane` over a fresh delegate connection, and returns
/// that connection's inbox.
fn report_from(daemon: &mut Daemon, id: u64, pane: PaneId, report: &str) -> Inbox {
    let reporter = daemon.attach_for_test(id);
    daemon.request_for_test(
        id,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "report".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        id,
        ClientMessage::DelegateReport {
            pane,
            report: report.into(),
        },
    );
    reporter
}

fn report_outcome(messages: &[ServerMessage]) -> Option<dispatch_proto::ReportOutcome> {
    messages.iter().find_map(|m| match m {
        ServerMessage::ReportAnswered { outcome } => Some(outcome.clone()),
        _ => None,
    })
}

#[test]
fn a_report_answers_the_caller_at_once() {
    let (mut daemon, project, _dir) = daemon("report-delivered");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask(&mut daemon, parent, long_task());
    let child = approve_and_spawn(&mut daemon, &ui);

    let reporter = report_from(&mut daemon, 20, child, "the findings");

    assert_eq!(
        report_outcome(&drain(&reporter)),
        Some(dispatch_proto::ReportOutcome::Delivered)
    );
    let finished = drain(&caller).into_iter().find_map(|m| match m {
        ServerMessage::DelegateFinished { exit, report, .. } => Some((exit, report)),
        _ => None,
    });
    assert_eq!(
        finished,
        Some((0, Some("the findings".to_string()))),
        "answered before the still-running subagent exits"
    );
}

#[test]
fn a_one_shot_subagent_that_reported_is_not_killed_when_its_caller_leaves() {
    let (mut daemon, project, _dir) = daemon("report-keeps-pane");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask(&mut daemon, parent, long_task());
    let child = approve_and_spawn(&mut daemon, &ui);
    let _ = report_from(&mut daemon, 20, child, "done");

    // `dispatch delegate` exits the moment it has its answer.
    daemon.detach_for_test(9);
    daemon.tick();

    assert_eq!(daemon.pane_count(), 2, "its pane stays for a person to read");
}

#[test]
fn a_second_report_is_refused() {
    let (mut daemon, project, _dir) = daemon("report-twice");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask(&mut daemon, parent, long_task());
    let child = approve_and_spawn(&mut daemon, &ui);
    let _ = report_from(&mut daemon, 20, child, "first");

    let second = report_from(&mut daemon, 21, child, "second");
    assert!(matches!(
        report_outcome(&drain(&second)),
        Some(dispatch_proto::ReportOutcome::Refused { reason }) if reason.contains("already reported")
    ));
}

#[test]
fn a_pane_that_is_not_a_subagent_cannot_report() {
    let (mut daemon, project, _dir) = daemon("report-not-subagent");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let top = spawn_pane_for_test(&mut daemon, &ui, project);

    let reporter = report_from(&mut daemon, 20, top, "unasked");
    assert!(matches!(
        report_outcome(&drain(&reporter)),
        Some(dispatch_proto::ReportOutcome::Refused { reason }) if reason.contains("not a subagent")
    ));
}

#[test]
fn a_report_nobody_is_waiting_for_says_so() {
    let (mut daemon, project, _dir) = daemon("report-no-caller");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask(&mut daemon, parent, "echo quick");
    let child = approve_and_spawn(&mut daemon, &ui);
    // The one-shot exits and its caller is answered with the tail.
    wait_for(&mut daemon, &ui, |m| {
        m.iter().any(|m| matches!(m, ServerMessage::PaneChanged { pane, update: PaneUpdate::Status { .. } } if *pane == child))
    });
    for _ in 0..50 {
        daemon.tick();
        std::thread::sleep(Duration::from_millis(10));
    }

    let late = report_from(&mut daemon, 20, child, "too late");
    assert!(matches!(
        report_outcome(&drain(&late)),
        Some(dispatch_proto::ReportOutcome::NotWaiting { .. })
    ));
}

#[test]
fn a_report_from_an_unknown_pane_is_an_error() {
    let (mut daemon, _project, _dir) = daemon("report-unknown");
    let reporter = report_from(&mut daemon, 20, PaneId::new(), "who");
    assert!(drain(&reporter).iter().any(|m| matches!(
        m,
        ServerMessage::Error { error: ProtocolError::NoSuchPane(_) }
    )));
}

#[test]
fn a_report_over_the_limit_is_cut_on_a_character_boundary() {
    // Three-byte characters, so the limit falls inside one.
    let report = "€".repeat(MAX_REPORT_BYTES / 3 + 10);
    let cut = cut_report(report);
    assert!(cut.len() <= MAX_REPORT_BYTES + 200);
    assert!(cut.ends_with("[dispatch] the report was cut at 1 MiB\n"));
    assert!(cut.starts_with('€'));
}

#[test]
fn the_cap_counts_requests_still_open_not_processes_still_running() {
    let limits = DelegationLimits {
        max_live_per_parent: 1,
        ..DelegationLimits::default()
    };
    let (mut daemon, project, _dir) = daemon_with_limits("report-cap", limits);
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _first = ask_as(&mut daemon, 9, parent, long_task());
    let child = approve_and_spawn(&mut daemon, &ui);
    let _ = report_from(&mut daemon, 20, child, "done");

    // The first is still running, but its request is finished.
    let _second = ask_as(&mut daemon, 10, parent, "echo second");
    assert!(
        pending(&drain(&ui)).is_some(),
        "a reported subagent no longer holds its parent's slot"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dispatch-daemon report`
Expected: compile errors (`MAX_REPORT_BYTES`, `cut_report`), then failures.

- [ ] **Step 3: Implement**

`crates/dispatch-daemon/src/pane.rs`, add to `DaemonPane`:

```rust
    /// Whether it runs its harness's own interface rather than its one-shot
    /// form. Such a pane never exits by itself, and is never ended because
    /// its caller went away: the user may be watching it.
    pub interactive: bool,
    /// Whether it has sent its report. A second one is refused.
    pub reported: bool,
```

Set both to `false` at every `DaemonPane { … }` literal in `session.rs` (`spawn_pane` and `approve`). In `approve`, set `interactive` from its parameter.

`crates/dispatch-daemon/src/session.rs`:

```rust
/// The largest report passed on, in bytes.
///
/// Far past anything an agent should write for another to read, and far
/// short of the protocol's 64 MiB frame limit.
const MAX_REPORT_BYTES: usize = 1024 * 1024;

/// `report`, cut at [`MAX_REPORT_BYTES`] on a character boundary, with a
/// line saying so.
fn cut_report(mut report: String) -> String {
    if report.len() <= MAX_REPORT_BYTES {
        return report;
    }
    let mut end = MAX_REPORT_BYTES;
    while !report.is_char_boundary(end) {
        end -= 1;
    }
    report.truncate(end);
    report.push_str("\n[dispatch] the report was cut at 1 MiB\n");
    report
}
```

In `handle_request`, replace the Task 2 placeholder arm with:

```rust
            ClientMessage::DelegateReport { pane, report } => self.report(id, pane, report),
```

Add the method next to `approve`:

```rust
    /// Delivers a subagent's report to the caller waiting on it.
    ///
    /// The caller is answered at once: the report is the work, and the
    /// process may go on for a moment after sending it. The pane's request
    /// and caller are both cleared, so neither its exit nor its caller
    /// leaving does anything more to it: a one-shot pane stays to be read,
    /// as one that exited does.
    fn report(&mut self, client: ClientId, pane: PaneId, report: String) {
        use dispatch_proto::ReportOutcome;

        let answer = |outcome| ServerMessage::ReportAnswered { outcome };

        let Some(target) = self.panes.get_mut(&pane) else {
            self.send(
                client,
                ServerMessage::Error {
                    error: ProtocolError::NoSuchPane(pane),
                },
            );
            return;
        };

        if target.parent.is_none() {
            let reason = format!(
                "pane {pane} is not a subagent: only a pane started by `dispatch delegate` \
                 has anyone to report to"
            );
            self.send(client, answer(ReportOutcome::Refused { reason }));
            return;
        }
        if target.reported {
            let reason = format!("pane {pane} has already reported");
            self.send(client, answer(ReportOutcome::Refused { reason }));
            return;
        }
        let (Some(request), Some(caller)) = (target.request, target.caller) else {
            let reason = "the agent that asked is no longer waiting".to_string();
            self.send(client, answer(ReportOutcome::NotWaiting { reason }));
            return;
        };

        target.request = None;
        target.caller = None;
        target.reported = true;
        let interactive = target.interactive;

        self.send(
            caller,
            ServerMessage::DelegateFinished {
                request,
                exit: 0,
                tail: Vec::new(),
                report: Some(cut_report(report)),
            },
        );
        self.send(client, answer(ReportOutcome::Delivered));

        // What an interactive pane does next is Task 6.
        let _ = interactive;
    }
```

`live_children`:

```rust
    /// How many of a pane's subagents are still working for it: their
    /// request is open. Counted by request rather than by process, since an
    /// interactive pane that has reported may stay open, at the user's
    /// choice, for as long as they like.
    fn live_children(&self, parent: PaneId) -> usize {
        self.panes
            .values()
            .filter(|p| p.parent == Some(parent) && p.request.is_some())
            .count()
    }
```

Update its doc in `delegation.rs`'s `refusal` comment if it says "running".

- [ ] **Step 4: Run the tests**

Run: `cargo test -p dispatch-daemon`
Expected: all pass, including the existing cap tests (`approving_more_requests_than_the_cap_allows_starts_only_what_fits`, `approvals_from_two_interfaces_do_not_share_one_slot`).

- [ ] **Step 5: Commit**

```bash
git add -A crates/dispatch-daemon
git commit -m "feat(daemon): a subagent's report answers its caller at once"
```

---

### Task 6: Interactive subagents

**Files:**
- Modify: `crates/dispatch-daemon/src/session.rs`
- Modify: `crates/dispatch-daemon/src/session/tests.rs`

**Interfaces:**
- Consumes: `DaemonPane::interactive`/`reported` and `report` (Task 5); `OnDone` (Task 3); the fixture's `[task.interactive]` (Task 4).
- Produces: interactive panes are `durable`; `abandon` detaches rather than kills them; on report, `Close` ends the pane after answering and `Ask` broadcasts `SubagentReported`; a late subscriber is told about reported panes; an interactive exit without a report sends `DelegateFinished` with an empty tail.

- [ ] **Step 1: Write the failing tests**

Append to `session/tests.rs`:

```rust
/// Attaches a delegate caller and asks for an interactive subagent.
fn ask_interactive(daemon: &mut Daemon, parent: PaneId, goal: &str) -> Inbox {
    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: "shell".into(),
            task: String::new(),
            size: (80, 24),
            handoff: Some(handoff_for(goal)),
            interactive: true,
        },
    );
    caller
}

#[cfg(unix)]
#[test]
fn an_interactive_subagent_closes_once_it_has_reported() {
    let (mut daemon, project, _dir) = daemon("interactive-close");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask_interactive(&mut daemon, parent, "sleep 30");
    let child = approve_and_spawn(&mut daemon, &ui);

    let _ = report_from(&mut daemon, 20, child, "interactive findings");

    let answer = drain(&caller).into_iter().find_map(|m| match m {
        ServerMessage::DelegateFinished { report, .. } => report,
        _ => None,
    });
    assert_eq!(answer.as_deref(), Some("interactive findings"));
    assert!(
        drain(&ui)
            .iter()
            .any(|m| matches!(m, ServerMessage::PaneClosed { pane } if *pane == child)),
        "closed after the caller had its answer"
    );
}

#[cfg(unix)]
#[test]
fn under_ask_a_reported_interactive_subagent_stays_and_is_marked() {
    let limits = DelegationLimits {
        interactive_on_done: dispatch_config::OnDone::Ask,
        ..DelegationLimits::default()
    };
    let (mut daemon, project, _dir) = daemon_with_limits("interactive-ask", limits);
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask_interactive(&mut daemon, parent, "sleep 30");
    let child = approve_and_spawn(&mut daemon, &ui);

    let _ = report_from(&mut daemon, 20, child, "done");

    assert!(drain(&ui).iter().any(
        |m| matches!(m, ServerMessage::SubagentReported { pane } if *pane == child)
    ));
    assert_eq!(daemon.pane_count(), 2, "kept for the user to close");

    // A window attaching later is told too.
    let late = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);
    assert!(drain(&late).iter().any(
        |m| matches!(m, ServerMessage::SubagentReported { pane } if *pane == child)
    ));
}

#[cfg(unix)]
#[test]
fn an_interactive_subagent_outlives_a_caller_that_left_before_it_reported() {
    let (mut daemon, project, _dir) = daemon("interactive-orphan");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask_interactive(&mut daemon, parent, "sleep 30");
    let child = approve_and_spawn(&mut daemon, &ui);

    daemon.detach_for_test(9);
    daemon.tick();
    assert_eq!(daemon.pane_count(), 2, "never ended under the user");

    let late = report_from(&mut daemon, 20, child, "nobody");
    assert!(matches!(
        report_outcome(&drain(&late)),
        Some(dispatch_proto::ReportOutcome::NotWaiting { .. })
    ));
    assert_eq!(daemon.pane_count(), 2, "and not closed by a report nobody wanted");
}

#[cfg(unix)]
#[test]
fn an_interactive_subagent_that_exits_without_reporting_answers_with_no_tail() {
    let (mut daemon, project, _dir) = daemon("interactive-no-report");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask_interactive(&mut daemon, parent, "echo screen-noise");
    let _child = approve_and_spawn(&mut daemon, &ui);

    let seen = wait_for(&mut daemon, &caller, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. }))
    });
    let finished = seen.iter().find_map(|m| match m {
        ServerMessage::DelegateFinished { tail, report, .. } => Some((tail.clone(), report.clone())),
        _ => None,
    });
    assert_eq!(finished, Some((Vec::new(), None)), "a full-screen tail is not output");
}

#[cfg(unix)]
#[test]
fn closing_an_interactive_subagent_before_it_reports_answers_its_caller() {
    let (mut daemon, project, _dir) = daemon("interactive-closed");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask_interactive(&mut daemon, parent, "sleep 30");
    let child = approve_and_spawn(&mut daemon, &ui);

    daemon.request_for_test(1, ClientMessage::ClosePane { pane: child });

    assert!(drain(&caller).iter().any(
        |m| matches!(m, ServerMessage::DelegateFinished { exit: -1, .. })
    ));
}
```

These are `#[cfg(unix)]` because the shipped and fixture Windows harnesses have no interactive form. Add one test that runs everywhere:

```rust
#[test]
fn a_harness_without_an_interactive_form_refuses_interactive() {
    let (mut daemon, project, _dir) = daemon("interactive-none");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: "no-task-args".into(),
            task: String::new(),
            size: (80, 24),
            handoff: Some(handoff_for("echo x")),
            interactive: true,
        },
    );
    let reason = refusal(&drain(&caller)).expect("refused");
    assert!(reason.contains("[task.interactive]"), "{reason}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dispatch-daemon interactive`
Expected: failures (pane not closed, no `SubagentReported`, interactive pane killed on detach, tail not empty).

- [ ] **Step 3: Implement**

In `approve`, the inserted pane and its announcement use `durable || interactive`:

```rust
        // An interactive subagent is one the user may be watching or typing
        // into: like a blanket-approved one, it is never ended because the
        // call that asked for it went away.
        let durable = durable || interactive;
```

placed before the `DaemonPane { … }` literal, so both the pane and `PaneSpawned { durable, … }` carry it.

At the end of `report`, replace the `let _ = interactive;` placeholder:

```rust
        if interactive {
            match self.limits.interactive_on_done {
                // After the answer, never before: closing first would answer
                // the caller as if the pane had been killed.
                dispatch_config::OnDone::Close => self.end_pane(pane),
                dispatch_config::OnDone::Ask => {
                    self.broadcast(ServerMessage::SubagentReported { pane });
                }
            }
        }
```

In `abandon`, inside the loop over panes and before the `pane.durable` check:

```rust
            // An interactive subagent stays, as a pane the user can go on
            // using. It is detached from the caller that left, so a report
            // it sends later is told nobody is waiting and nothing closes it.
            if pane.interactive && pane.caller == Some(caller) {
                pane.caller = None;
                pane.request = None;
                continue;
            }
```

In `pump_panes`, where `DelegateFinished` is built from the tail, send an empty tail for an interactive pane:

```rust
                let tail = if pane.interactive {
                    // A full-screen interface's bytes are drawing commands,
                    // not output: nothing a caller could read.
                    Vec::new()
                } else {
                    let start = pane.history.len().saturating_sub(TAIL_BYTES);
                    pane.history[start..].to_vec()
                };
```

In the `Subscribe` catch-up, after the pane loop and before the tabs:

```rust
                // A reported pane waiting on the user's decision is marked in
                // every window, a late one included.
                if self.limits.interactive_on_done == dispatch_config::OnDone::Ask {
                    for pane in self.panes.values().filter(|p| p.interactive && p.reported) {
                        existing.push(ServerMessage::SubagentReported { pane: pane.id });
                    }
                }
```

`answer_for_a_closed_subagent` already sends `exit: -1`; for an interactive pane make its tail empty the same way as in `pump_panes`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p dispatch-daemon`
Expected: all pass, including the existing `a_subagent_dies_with_the_caller_that_asked_for_it`.

- [ ] **Step 5: Commit**

```bash
git add -A crates/dispatch-daemon
git commit -m "feat(daemon): interactive subagents that close once reported, or wait for the user"
```

---

### Task 7: The `dispatch delegate` and `dispatch report` commands

**Files:**
- Modify: `dispatch/src/delegate.rs`
- Create: `dispatch/src/report.rs`
- Modify: `dispatch/src/main.rs`
- Modify: `dispatch/tests/delegate_shim.rs`

**Interfaces:**
- Consumes: `Handoff`, `TEMPLATE` (Task 1); protocol (Task 2); daemon behaviour (Tasks 4–6).
- Produces:
  - `delegate::run(args: delegate::Args) -> Result<ExitCode>` with `pub struct Args { pub harness: Option<String>, pub size: (u16, u16), pub task: Option<String>, pub handoff: Option<String>, pub template: bool, pub interactive: bool }`
  - `report::run(file: &str) -> Result<ExitCode>`
  - `delegate::exit::{USAGE = 64, NOINPUT = 66}` added beside the existing codes, and `pub(crate)` so `report` can use the module

- [ ] **Step 1: Update the end-to-end fixture**

In `dispatch/tests/delegate_shim.rs`, replace the `shell` harness in `Config::new` with the goal-runner from Task 4 Step 1 (same two scripts, written into `harnesses`, same `[task]` and Unix `[task.interactive]` forms). Replace `run_delegate_shim` so it pipes a handoff:

```rust
/// A complete handoff whose Goal the test harness runs as a script.
fn handoff_for(goal: &str) -> String {
    format!(
        "## Goal\n{goal}\n\n## Context\nA test.\n\n## Constraints\nNone.\n\n\
         ## Done when\nIt has run.\n\n## Report back\nNothing.\n"
    )
}

/// Runs `dispatch delegate --handoff -` with `handoff` on its standard input,
/// plus `extra` arguments, and waits for it to exit.
fn run_delegate_shim(
    config: &Config,
    parent: dispatch_core::PaneId,
    harness: &str,
    handoff: &str,
    extra: &[&str],
) -> std::process::Output {
    let child = start_delegate_shim(config, parent, harness, handoff, extra);
    finish(child)
}

/// Starts `dispatch delegate --handoff -` without waiting for it.
fn start_delegate_shim(
    config: &Config,
    parent: dispatch_core::PaneId,
    harness: &str,
    handoff: &str,
    extra: &[&str],
) -> std::process::Child {
    use std::io::Write;

    let (key, value) = config.env();
    let mut command = Command::new(env!("CARGO_BIN_EXE_dispatch"));
    command
        .arg("delegate")
        .arg("--handoff")
        .arg("-")
        .args(extra)
        .env("DISPATCH_PANE", parent.to_string())
        .env(key, value)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if !harness.is_empty() {
        command.arg("--harness").arg(harness);
    }
    let mut child = command.spawn().expect("the dispatch binary can be started");
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(handoff.as_bytes())
        .expect("the shim reads its handoff");
    child
}
```

Move the existing bounded-wait body of the old `run_delegate_shim` (the part after `spawn`) into `fn finish(child: std::process::Child) -> std::process::Output`. Update its existing callers to pass `&handoff_for(<old task>)` and `&[]`.

- [ ] **Step 2: Write the failing end-to-end tests**

Append to `dispatch/tests/delegate_shim.rs`:

```rust
/// Runs `dispatch` with `args` and no daemon involved, with `stdin`.
fn run_plain(args: &[&str], stdin: &[u8], pane: Option<&str>) -> std::process::Output {
    use std::io::Write;

    let mut command = Command::new(env!("CARGO_BIN_EXE_dispatch"));
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(pane) = pane {
        command.env("DISPATCH_PANE", pane);
    }
    let mut child = command.spawn().expect("starts");
    child.stdin.take().expect("piped").write_all(stdin).expect("writes");
    child.wait_with_output().expect("exits")
}

#[test]
fn the_template_is_printed_without_a_daemon() {
    let output = run_plain(&["delegate", "--template"], b"", None);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.starts_with("## Goal"), "{text}");
}

#[test]
fn a_bare_task_is_refused_with_the_template() {
    let output = run_plain(&["delegate", "do the thing"], b"", Some(&dispatch_core::PaneId::new().to_string()));
    assert_eq!(output.status.code(), Some(64));
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("--handoff") && errors.contains("## Report back"), "{errors}");
}

#[test]
fn an_incomplete_handoff_is_refused_naming_what_is_missing() {
    let output = run_plain(
        &["delegate", "--handoff", "-"],
        b"## Goal\nsomething\n",
        Some(&dispatch_core::PaneId::new().to_string()),
    );
    assert_eq!(output.status.code(), Some(64));
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("Context is missing"), "{errors}");
}

#[test]
fn an_unreadable_handoff_file_is_66() {
    let output = run_plain(
        &["delegate", "--handoff", "/nonexistent/dispatch/handoff.md"],
        b"",
        Some(&dispatch_core::PaneId::new().to_string()),
    );
    assert_eq!(output.status.code(), Some(66));
}

#[test]
fn an_empty_report_is_64() {
    let output = run_plain(&["report", "-"], b"  \n", Some(&dispatch_core::PaneId::new().to_string()));
    assert_eq!(output.status.code(), Some(64));
}

#[test]
fn the_subagents_report_reaches_the_callers_stdout() {
    let config = Config::new("report-e2e");
    let project = std::env::temp_dir();
    let _daemon = Daemon::start(&config, &project);
    let (ui, mut ui_writer) = attach(&config);
    let parent = spawn_parent_pane(&ui, &mut ui_writer);

    // The report holds a byte that is not UTF-8: it is still delivered.
    let goal = if cfg!(windows) {
        "[byte[]](0x66,0x6f,0x75,0x6e,0x64,0xff) | dispatch report -"
    } else {
        "printf 'found\\377' | dispatch report -"
    };
    let child = start_delegate_shim(&config, parent, "", &handoff_for(goal), &[]);

    let asked = wait_for(&ui, "the prompt", |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegatePending { .. }))
    });
    let request = asked
        .iter()
        .find_map(|m| match m {
            ServerMessage::DelegatePending { request, .. } => Some(*request),
            _ => None,
        })
        .expect("checked");
    Frame::write(
        &mut ui_writer,
        &ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    )
    .expect("writing succeeds");

    let output = finish(child);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("found"), "the report, not the tail: {stdout:?}");
}
```

If the Windows PowerShell pipe of raw bytes into a native command does not deliver `0xff` intact, keep the Windows goal as `'found' | dispatch report -` and leave the non-UTF-8 check Unix-only with a comment saying why.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo build --workspace && cargo test -p dispatch --test delegate_shim`
Expected: failures (no `--handoff`, `--template`, or `report`).

- [ ] **Step 4: Wire the CLI**

In `dispatch/src/main.rs`, replace the `Delegate` variant and add `Report`:

```rust
    /// Ask Dispatch to run one task in a second agent, and wait for it.
    ///
    /// Runs inside a Dispatch pane. Write a handoff first: `dispatch
    /// delegate --template` prints one with its five sections. Prints the
    /// subagent's report on stdout (or, if it sent none, the tail of its
    /// output) and progress on stderr. Exits 0 when it reported, otherwise
    /// with the subagent's own status; 64 when the handoff is missing or
    /// incomplete, 66 when it cannot be read, 69 when no daemon is
    /// listening, 75 when the request timed out, the connection dropped, the
    /// daemon does not know the asking pane, the subagent was stopped, or an
    /// interactive subagent finished without a report, 77 when it was
    /// denied, 78 when it was refused.
    Delegate {
        /// Which harness to run. Defaults to this pane's own.
        #[arg(long)]
        harness: Option<String>,

        /// Size to start the subagent at, as COLSxROWS.
        #[arg(long, default_value = "80x24", value_parser = parse_size)]
        size: (u16, u16),

        /// The handoff file, or `-` to read it from standard input.
        #[arg(long, value_name = "FILE")]
        handoff: Option<String>,

        /// Print an empty handoff and exit.
        #[arg(long, conflicts_with_all = ["handoff", "task"])]
        template: bool,

        /// Run the subagent in its harness's own interface, where it can be
        /// watched and typed into, rather than invisibly.
        #[arg(long)]
        interactive: bool,

        /// No longer accepted: write a handoff instead. Kept so a bare task
        /// is answered with the template rather than a usage error.
        #[arg(hide = true)]
        task: Option<String>,
    },

    /// Send this subagent's report to the agent that delegated to it.
    ///
    /// Runs inside a subagent's pane. Exits 0 when delivered, 64 when the
    /// report is empty, 66 when the file cannot be read, 69 when no daemon is
    /// listening, 75 when nobody is waiting for it any more, 78 when this pane
    /// is not a subagent or has already reported.
    Report {
        /// The report file, or `-` to read it from standard input.
        file: String,
    },
```

And in `main`:

```rust
        Some(Command::Delegate {
            harness,
            size,
            handoff,
            template,
            interactive,
            task,
        }) => {
            return delegate::run(delegate::Args {
                harness,
                size,
                task,
                handoff,
                template,
                interactive,
            });
        }
        Some(Command::Report { file }) => return report::run(&file),
```

Add `mod report;`.

- [ ] **Step 5: Rewrite `delegate::run`**

In `dispatch/src/delegate.rs`, make `mod exit` `pub(crate) mod exit`, add:

```rust
    /// The handoff is missing, incomplete, or over the limit.
    pub const USAGE: u8 = 64;
    /// The handoff, or a report, cannot be read.
    pub const NOINPUT: u8 = 66;
```

and replace `run`'s signature and opening with:

```rust
/// What `dispatch delegate` was asked to do.
pub struct Args {
    pub harness: Option<String>,
    pub size: (u16, u16),
    pub task: Option<String>,
    pub handoff: Option<String>,
    pub template: bool,
    pub interactive: bool,
}

/// Reads `source`, a path or `-` for standard input, as text.
///
/// Read as bytes and converted lossily: one stray byte in what an agent
/// piped in should not cost it the whole message.
pub(crate) fn read_source(source: &str) -> std::io::Result<String> {
    use std::io::Read;

    let bytes = if source == "-" {
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes)?;
        bytes
    } else {
        std::fs::read(source)?
    };
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Says why a handoff was not accepted, then prints the template, so an
/// agent can retry in the right shape from the error alone.
fn refuse_handoff(why: &str) -> ExitCode {
    eprintln!("[dispatch] {why}");
    eprintln!("[dispatch] the template (`dispatch delegate --template` prints it again):\n");
    eprint!("{}", dispatch_core::handoff::TEMPLATE);
    ExitCode::from(exit::USAGE)
}

/// Runs one delegation to completion.
pub fn run(args: Args) -> Result<ExitCode> {
    if args.template {
        print!("{}", dispatch_core::handoff::TEMPLATE);
        return Ok(ExitCode::SUCCESS);
    }
    if args.task.is_some() {
        return Ok(refuse_handoff(
            "a bare task is no longer accepted: write a handoff and pass it with \
             --handoff FILE, or --handoff - for standard input",
        ));
    }
    let Some(source) = args.handoff else {
        return Ok(refuse_handoff(
            "no handoff: pass one with --handoff FILE, or --handoff - for standard input",
        ));
    };
    let text = match read_source(&source) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("[dispatch] cannot read the handoff {source}: {error}");
            return Ok(ExitCode::from(exit::NOINPUT));
        }
    };
    let handoff = match dispatch_core::Handoff::parse(&text) {
        Ok(handoff) => handoff,
        Err(error) => {
            let problems: Vec<String> =
                error.problems.iter().map(|p| format!("  - {p}")).collect();
            return Ok(refuse_handoff(&format!(
                "the handoff is not ready:\n{}",
                problems.join("\n")
            )));
        }
    };

    let parent: PaneId = std::env::var("DISPATCH_PANE")
        // … unchanged from here: attach, then send …
```

The `send` becomes:

```rust
    client.send(ClientMessage::DelegateRequest {
        parent,
        harness,
        task: handoff.text.clone(),
        size: args.size,
        handoff: Some(handoff),
        interactive: args.interactive,
    });
```

using `args.harness.unwrap_or_default()` for `harness`. The `DelegateFinished` arm becomes:

```rust
                ServerMessage::DelegateFinished {
                    exit: code,
                    tail,
                    report,
                    ..
                } => {
                    use std::io::Write;

                    if let Some(report) = report {
                        std::io::stdout()
                            .write_all(report.as_bytes())
                            .context("failed to write the subagent's report")?;
                        std::io::stdout().flush().ok();
                        eprintln!("[dispatch] the subagent reported");
                        return Ok(ExitCode::SUCCESS);
                    }

                    if code == -1 {
                        eprintln!("[dispatch] the subagent was stopped before it finished");
                        return Ok(ExitCode::from(exit::TEMPFAIL));
                    }

                    if args.interactive {
                        eprintln!(
                            "[dispatch] the subagent finished without a report (exit {code}); \
                             its pane has what it did"
                        );
                        return Ok(ExitCode::from(exit::TEMPFAIL));
                    }

                    std::io::stdout()
                        .write_all(&tail)
                        .context("failed to write the subagent's output")?;
                    std::io::stdout().flush().ok();
                    eprintln!("[dispatch] no report was sent; returning the output tail");
                    eprintln!("[dispatch] subagent exited {code}");
                    return Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)));
                }
```

Keep the existing comments on the `-1` sentinel; move them with the code. `args.interactive` must be read before `args` is partly moved; copy it into a local `let interactive = args.interactive;` near the top.

- [ ] **Step 6: Write `dispatch report`**

`dispatch/src/report.rs`:

```rust
//! `dispatch report`: a subagent hands its work back.
//!
//! Runs inside the subagent's pane, as the last thing it does. The daemon
//! passes the report to the `dispatch delegate` waiting on this pane, which
//! prints it for the agent that asked.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use dispatch_client::{Client, ClientError};
use dispatch_core::PaneId;
use dispatch_proto::{ClientMessage, ProtocolError, ReportOutcome, Role, ServerMessage};

use crate::delegate::{exit, read_source};

/// How long to wait for the daemon's answer. It answers at once; this only
/// covers a daemon that died without closing its socket.
const PATIENCE: Duration = Duration::from_secs(60);

/// Sends one report.
pub fn run(file: &str) -> Result<ExitCode> {
    let report = match read_source(file) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("[dispatch] cannot read the report {file}: {error}");
            return Ok(ExitCode::from(exit::NOINPUT));
        }
    };
    if report.trim().is_empty() {
        eprintln!("[dispatch] the report is empty; write what the handoff's Report back asked for");
        return Ok(ExitCode::from(exit::USAGE));
    }

    let pane: PaneId = std::env::var("DISPATCH_PANE")
        .context("DISPATCH_PANE is not set: `dispatch report` runs inside a subagent's pane")?
        .parse()
        .context("DISPATCH_PANE does not name a pane")?;

    let client = match Client::attach_as(Role::Delegate, "dispatch report") {
        Ok(client) => client,
        Err(ClientError::NotRunning(endpoint)) => {
            eprintln!("[dispatch] no daemon is listening on {endpoint}");
            return Ok(ExitCode::from(exit::UNAVAILABLE));
        }
        Err(error) => return Err(error).context("failed to reach the daemon"),
    };

    client.send(ClientMessage::DelegateReport { pane, report });

    let deadline = Instant::now() + PATIENCE;
    loop {
        for message in client.poll() {
            match message {
                ServerMessage::ReportAnswered { outcome } => {
                    return Ok(match outcome {
                        ReportOutcome::Delivered => {
                            eprintln!("[dispatch] report delivered");
                            ExitCode::SUCCESS
                        }
                        ReportOutcome::NotWaiting { reason } => {
                            eprintln!("[dispatch] {reason}");
                            ExitCode::from(exit::TEMPFAIL)
                        }
                        ReportOutcome::Refused { reason } => {
                            eprintln!("[dispatch] refused: {reason}");
                            ExitCode::from(exit::CONFIG)
                        }
                        ReportOutcome::Unknown => {
                            eprintln!("[dispatch] unexpected answer from the daemon");
                            ExitCode::from(exit::CONFIG)
                        }
                    });
                }
                ServerMessage::Error {
                    error: ProtocolError::NoSuchPane(pane),
                } => {
                    eprintln!("[dispatch] the daemon does not know pane {pane}");
                    return Ok(ExitCode::from(exit::TEMPFAIL));
                }
                ServerMessage::Error { error } => {
                    eprintln!("[dispatch] {error}");
                    return Ok(ExitCode::from(exit::CONFIG));
                }
                _ => {}
            }
        }

        if !client.is_connected() || Instant::now() >= deadline {
            eprintln!("[dispatch] the daemon did not answer; the report may not have arrived");
            return Ok(ExitCode::from(exit::TEMPFAIL));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
```

- [ ] **Step 7: Run the tests**

Run: `cargo build --workspace && cargo test --workspace`
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add -A dispatch
git commit -m "feat(cli): delegate with a handoff, and report back with dispatch report"
```

---

### Task 8: The approval prompt shows the handoff

**Files:**
- Modify: `dispatch/src/approval.rs`
- Modify: `dispatch/src/app.rs`

**Interfaces:**
- Consumes: `ServerMessage::DelegatePending { handoff, interactive, … }` (Task 2); `Section::PROMPT_ORDER`, `Handoff::section` (Task 1).
- Produces: `Approval { …, handoff: Option<&'a Handoff>, interactive: bool }`; `PendingRequest { …, handoff: Option<Handoff>, interactive: bool }`.

- [ ] **Step 1: Write the failing tests**

In `dispatch/src/approval.rs`'s `mod tests`, the existing `approval(task)` helper gains `handoff: None, interactive: false,`. Add:

```rust
    /// Each line of `widget`'s content, as plain text.
    fn plain_lines(widget: &Approval<'_>) -> Vec<String> {
        widget
            .lines()
            .iter()
            .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
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
    fn a_request_without_a_handoff_still_shows_its_task() {
        let text = plain_lines(&approval("write the tests"));
        assert!(text.iter().any(|line| line == "write the tests"));
        assert!(!text.iter().any(|line| line.contains("runs interactively")));
    }
```

In `dispatch/src/app.rs`'s test module, beside `a_task_title_keeps_whole_words_and_says_when_it_cut`:

```rust
    #[test]
    fn a_subagent_is_titled_by_its_handoffs_goal() {
        let (mut app, project, daemon, _sent) = attached_app();
        let parent = spawn_several(&mut app, &daemon, project, 1)[0];
        let handoff = dispatch_core::Handoff::parse(
            "## Context\nlong background first\n## Goal\nWrite the http client tests\n\
             ## Constraints\nNone.\n## Done when\nthey pass\n## Report back\nwhich tests\n",
        )
        .expect("complete");
        let request = RequestId::new();
        let child = PaneId::new();

        for message in [
            ServerMessage::DelegatePending {
                request,
                parent,
                project,
                harness: "claude".into(),
                task: handoff.text.clone(),
                depth: 0,
                handoff: Some(handoff.clone()),
                interactive: false,
            },
            ServerMessage::DelegateResolved {
                request,
                outcome: DelegateOutcome::Approved { pane: child },
            },
            spawned(child, project, "claude", Some(parent), false),
        ] {
            daemon.send(message).expect("the app is listening");
        }
        app.poll_daemon();

        let title = &app.state.pane(child).expect("adopted").title;
        assert!(title.starts_with("Write the http"), "{title:?}");
    }
```

If the approval prompt opening on `DelegatePending` swallows the `DelegateResolved` in a way this test trips over, poll between the messages (`app.poll_daemon()` after each send), as the neighbouring delegation tests do.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dispatch approval`
Expected: compile errors on the new fields.

- [ ] **Step 3: Implement**

`dispatch/src/approval.rs`:
- add fields

  ```rust
      /// The handoff, when the request carries one; shown by section.
      pub handoff: Option<&'a dispatch_core::Handoff>,
      /// Whether the subagent would run in its own interface.
      pub interactive: bool,
  ```
- in `lines()`, after the harness/project/depth line, when `interactive`, push `Line::styled("runs interactively: you can watch it and type into it", Style::default().fg(Color::DarkGray))`;
- replace the `for line in self.task.lines()` loop with:

  ```rust
          match self.handoff {
              Some(handoff) => {
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
  ```

`dispatch/src/app.rs`:
- `PendingRequest` gains `handoff: Option<Handoff>` and `interactive: bool`; the `PendingRequest { … }` literals in its tests (`app_with_one_pending`, `the_reminders_fit_an_eighty_column_status_row`, and any others the compiler finds) gain `handoff: None, interactive: false,`;
- the `DelegatePending` arm destructures and stores them;
- `approval_widget` passes `handoff: request.handoff.as_ref(), interactive: request.interactive`;
- `task_of` returns the goal when there is a handoff, so the child title comes from it: `.map(|waiting| waiting.handoff.as_ref().map_or_else(|| waiting.task.clone(), |h| h.goal.clone()))`. Do the same wherever `answered` is filled (search `answered.insert`).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p dispatch`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add dispatch/src/approval.rs dispatch/src/app.rs
git commit -m "feat(tui): the approval prompt shows a handoff by section"
```

---

### Task 9: The reported mark and its prompt

**Files:**
- Modify: `crates/dispatch-core/src/pane.rs`, `crates/dispatch-core/src/state.rs`
- Modify: `crates/dispatch-tui/src/sidebar.rs`, `crates/dispatch-tui/src/sidebar/tests.rs`
- Modify: `dispatch/src/app.rs`

**Interfaces:**
- Consumes: `ServerMessage::SubagentReported { pane }` (Task 2).
- Produces: `Pane::reported: bool`; `AppState::set_reported(&mut self, id: PaneId, reported: bool)`; `sidebar::REPORTED`; `Overlay::Reported { pane: PaneId, prompt: Prompt }`.

- [ ] **Step 1: Write the failing tests**

In `crates/dispatch-tui/src/sidebar/tests.rs`, add `REPORTED` to the glyph import list (line ~713) and, beside `a_blocked_pane_says_so_in_yellow`:

```rust
#[test]
fn a_reported_subagent_is_marked_in_bold() {
    let (mut state, alpha, _) = state();
    let pane = spawn(&mut state, alpha, "claude");
    state.set_reported(pane, true);

    let buf = render(&state, WIDTH, 6);
    let cell = buf.cell((WIDTH - 3, TOP + 1)).expect("cell exists");

    assert_eq!(cell.symbol(), REPORTED);
    assert!(cell.modifier.contains(Modifier::BOLD));
}
```

In `dispatch/src/app.rs`'s test module, beside `closing_a_tab_asks_first`:

```rust
    /// An attached app with one parent and one interactive child that has
    /// reported, with whatever the app sent so far drained.
    fn app_with_a_reported_child() -> (
        App,
        PaneId,
        Sender<ServerMessage>,
        Receiver<ClientMessage>,
    ) {
        let (mut app, project, daemon, sent) = attached_app();
        let parent = spawn_several(&mut app, &daemon, project, 1)[0];
        let child = delegated(&mut app, &daemon, project, parent, "claude");
        daemon
            .send(ServerMessage::SubagentReported { pane: child })
            .expect("the app is listening");
        app.poll_daemon();
        while sent.try_recv().is_ok() {}
        (app, child, daemon, sent)
    }

    #[test]
    fn a_reported_subagent_is_counted_and_asks_before_it_closes() {
        let (mut app, child, _daemon, sent) = app_with_a_reported_child();
        assert!(app.state.pane(child).expect("listed").reported);

        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 24))
            .expect("a test backend can be created");
        drawn(&mut app, &mut terminal);
        let row = bottom_row(&terminal);
        assert!(row.contains("1 reported"), "{row:?}");

        app.open_next_reported();
        let Some(Overlay::Reported { pane, prompt }) = &app.overlay else {
            panic!("the question is open");
        };
        assert_eq!(*pane, child);
        assert_eq!(
            prompt.title(),
            "Subagent finished and sent its report. Close this pane? y/n"
        );

        press(&mut app, KeyCode::Char('y'));
        assert!(app.overlay.is_none());
        let closed: Vec<ClientMessage> = sent.try_iter().collect();
        assert!(
            closed.contains(&ClientMessage::ClosePane { pane: child }),
            "{closed:#?}"
        );
    }

    #[test]
    fn keeping_a_reported_subagent_clears_its_mark() {
        let (mut app, child, _daemon, sent) = app_with_a_reported_child();

        app.open_next_reported();
        press(&mut app, KeyCode::Char('n'));

        assert!(app.overlay.is_none());
        assert!(!app.state.pane(child).expect("still listed").reported);
        assert!(
            !sent
                .try_iter()
                .any(|m| matches!(m, ClientMessage::ClosePane { .. })),
            "nothing is closed"
        );
    }
```

`App::close_pane` reaches the daemon through the pane's backend (`dispatch/src/backend.rs` sends `ClientMessage::ClosePane`). If `delegated` panes in tests have no backend and nothing is sent, assert instead that `app.state.pane(child)` is closed, and note it in the commit body.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dispatch-tui sidebar && cargo test -p dispatch reported`
Expected: compile errors.

- [ ] **Step 3: Implement**

`crates/dispatch-core/src/pane.rs`, add to `Pane` and to `Pane::new` (`false`):

```rust
    /// Whether this interactive subagent has sent its report and waits for
    /// the user to close it or keep it.
    #[serde(default)]
    pub reported: bool,
```

`crates/dispatch-core/src/state.rs`, next to `set_pane_status`:

```rust
    /// Marks an interactive subagent as having reported, or clears the mark.
    pub fn set_reported(&mut self, id: PaneId, reported: bool) {
        if let Some(pane) = self.panes.iter_mut().find(|p| p.id == id) {
            pane.reported = reported;
        }
    }
```

`crates/dispatch-tui/src/sidebar.rs`:

```rust
/// An interactive subagent that has reported and waits on the user.
pub const REPORTED: &str = "\u{f0e0}";
```

In `state_glyph`, right after the `closed` check:

```rust
    if pane.reported {
        return (REPORTED, Style::default().fg(theme.accent).add_modifier(Modifier::BOLD));
    }
```

In `Rollup::of`, map a reported pane to `Some(Rollup::Done)` before the status match: `.filter_map(|pane| if pane.reported { return Some(Rollup::Done) } …)`.

`dispatch/src/app.rs`:
- `Overlay` gains

  ```rust
      /// An interactive subagent has reported: close it, or keep it.
      Reported {
          /// The subagent's pane.
          pane: PaneId,
          /// The question.
          prompt: Prompt,
      },
  ```
  and joins `CloseTab` in every match that lists `CloseTab` (the `picker`/`picker_mut`/… helpers returning `None`, `set_border`, and the `draw_overlay` branch that renders a `Prompt`).
- handle `ServerMessage::SubagentReported { pane }` in the message match: `self.state.set_reported(pane, true); true`.
- add

  ```rust
      /// Asks about the first reported subagent, if there is one.
      fn open_next_reported(&mut self) {
          let Some(pane) = self
              .state
              .projects()
              .iter()
              .flat_map(|project| self.state.panes_for(project.id))
              .find(|pane| pane.reported && !pane.closed)
              .map(|pane| pane.id)
          else {
              return;
          };
          self.overlay = Some(Overlay::Reported {
              pane,
              prompt: Prompt::new(
                  "Subagent finished and sent its report. Close this pane? y/n",
                  "y closes it, n keeps it as an ordinary pane",
              ),
          });
      }

      /// Acts on one key while a reported subagent waits on an answer.
      fn handle_reported_key(&mut self, key: &KeyEvent) {
          let Some(Overlay::Reported { pane, .. }) = &self.overlay else {
              return;
          };
          let pane = *pane;

          match key.code {
              KeyCode::Char('y') => {
                  self.overlay = None;
                  self.close_pane(pane);
              }
              KeyCode::Char('n') | KeyCode::Esc => {
                  self.overlay = None;
                  self.state.set_reported(pane, false);
              }
              _ => {}
          }
      }
  ```
- in the key dispatch, beside `if matches!(self.overlay, Some(Overlay::CloseTab { .. }))`, add the same for `Overlay::Reported` calling `handle_reported_key`;
- `Action::Approvals => { if self.pending.is_empty() { self.open_next_reported() } else { self.open_next_approval() } }`;
- in `focus_pane`, after focusing: if the pane is reported and `self.overlay.is_none()`, open the prompt for that pane (build the same `Overlay::Reported` directly with `pane: id`);
- in `draw_status`, beside `blocked_reminder`:

  ```rust
          let reported = self
              .state
              .projects()
              .iter()
              .flat_map(|project| self.state.panes_for(project.id))
              .filter(|pane| !pane.closed && pane.reported)
              .count();
          let reported_reminder = (reported > 0 && self.overlay.is_none()).then(|| {
              let text = format!("{reported} reported");
              match self.router.keymap().path_to(Command::Approvals) {
                  Some(keys) if self.pending.is_empty() => format!("{text} — {keys}"),
                  _ => text,
              }
          });
  ```
  and chain `reported_reminder` after `blocked_reminder` everywhere `blocked_reminder` is chained.

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A crates/dispatch-core crates/dispatch-tui dispatch/src/app.rs
git commit -m "feat(tui): mark a reported subagent and ask before closing it"
```

---

### Task 10: Docs and hands-on checks

**Files:**
- Modify: `docs/delegation.md`
- Modify: `docs/configuration.md`
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-09-30-delegation-handoff-design.md` (status line)

- [ ] **Step 1: Rewrite `docs/delegation.md`**

Keep its structure and voice. Change:
- The opening example becomes a handoff written to a file and `dispatch delegate --handoff h.md`, with the five sections shown, and a line that `dispatch delegate --template` prints an empty one.
- A section "Reporting back": the subagent is told to run `dispatch report`, the caller prints the report, and the tail is the fallback when none is sent.
- A section "Watching a subagent": `--interactive`, the `[task.interactive]` form, `interactive_on_done`, and that there is no Windows interactive form.
- The `[task]` example gains the `[task.interactive]` lines from claude.toml.
- The exit-code table becomes the two tables in the plan's Global Constraints.
- The fan-out example uses `--handoff tests.md` and `--handoff docs.md`.

- [ ] **Step 2: Update `docs/configuration.md` and `README.md`**

`docs/configuration.md`: add `interactive_on_done = "close"   # or "ask"` to the `[delegation]` example with one sentence on each value, and `[task.interactive]` to the harness-file reference.

`README.md`: the commands table row becomes ``| `dispatch delegate --handoff h.md` | an agent hands a task, with its context, to a second one ([more](docs/delegation.md)) |``.

- [ ] **Step 3: Check each shipped interactive form by hand**

For each installed harness, from a Dispatch pane (`./target/release/dispatch --attach .` after `cargo build --release`), write a handoff whose Goal is "Reply with the word ready, then report it" and run `dispatch delegate --handoff h.md --harness <id> --interactive`. Approve it, bring the pane into view with `Ctrl p s`, and confirm:
1. the agent's own interface starts with the brief as its first prompt;
2. it runs `dispatch report` (it may need its permission prompt approved, if auto-approve is off);
3. the caller prints the report and exits 0, and the pane closes.

Record the result for claude, codex, agy and opencode in the spec's "The harness form" table (a "checked 2026-MM-DD" column). If a form does not work (for example a CLI that rejects a positional prompt), fix that harness file through the superseded mechanism and note it.

- [ ] **Step 4: Mark the spec implemented**

Change the spec's `Status:` line to `Status: implemented on branch \`daemon\`.`

- [ ] **Step 5: Commit**

```bash
git add README.md docs
git commit -m "docs: delegation with a handoff, reports, and interactive subagents"
```
