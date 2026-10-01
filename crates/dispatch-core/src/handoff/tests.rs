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
    let text = format!("# Handoff for the tests\n\n{}", complete()).replace(
        "The client is in src/http.rs.",
        "Intro.\n\n## Files\nsrc/http.rs",
    );
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
    let parent: PaneId = "00000000-0000-0000-0000-000000000001"
        .parse()
        .expect("an id");
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
