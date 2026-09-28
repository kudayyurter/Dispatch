//! Tests for settings: what a value may be, and which settings a harness
//! file can use.

use super::*;

use crate::testing::{DEMO_SETTINGS, demo_harness};

/// The setting `key` of `def`.
fn setting(def: &crate::HarnessDef, key: &str) -> SettingDef {
    def.settings
        .iter()
        .find(|setting| setting.key == key)
        .unwrap_or_else(|| panic!("{key} is offered"))
        .clone()
}

#[test]
fn a_real_model_name_is_a_safe_value() {
    for value in [
        "opus",
        "gpt-5.6-sol",
        "claude-opus-4-6-thinking",
        "amazon-bedrock/au.anthropic.claude-haiku-4-5-20251001-v1:0",
        "openai/gpt-6-astra#high",
        "a+b@c_d",
    ] {
        assert!(is_safe_value(value), "{value:?} should be safe");
    }
}

#[test]
fn anything_a_shell_would_read_is_refused() {
    // cmd.exe reads its whole command line as shell syntax on Windows, and a
    // leading dash would be read as a flag of the value's own.
    for value in [
        "",
        "-x",
        "--dangerously-skip-permissions",
        "a b",
        "a&b",
        "a|b",
        "%PATH%",
        "a^b",
        "a\"b",
        "a'b",
        "a\\b",
        "a\nb",
        "a;b",
        "a<b",
        "a>b",
        "a(b)",
        "a!b",
        "a`b",
        "a$b",
        "é",
    ] {
        assert!(!is_safe_value(value), "{value:?} should be refused");
    }
}

#[test]
fn a_value_may_be_two_hundred_characters_and_no_more() {
    assert!(is_safe_value(&"a".repeat(MAX_VALUE_CHARS)));
    assert!(!is_safe_value(&"a".repeat(MAX_VALUE_CHARS + 1)));
}

#[test]
fn a_setting_reads_its_args_env_and_custom() {
    let def = demo_harness(
        r#"
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["small"]
custom = true
args = ["--model", "{value}"]
env = { MODEL = "{value}" }
"#,
    );

    let model = setting(&def, "model");
    assert!(model.custom);
    assert_eq!(model.args, vec!["--model", "{value}"]);
    assert_eq!(model.env.get("MODEL").map(String::as_str), Some("{value}"));
}

#[test]
fn a_setting_with_no_args_or_env_is_not_offered() {
    // What every harness file edited before settings had `args` carries.
    let def = demo_harness(
        r#"
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["default", "opus"]
default = "default"
"#,
    );

    assert!(def.settings.is_empty(), "{:?}", def.settings);
}

#[test]
fn a_second_setting_with_a_used_key_is_dropped() {
    let def = demo_harness(
        r#"
[[settings]]
key = "model"
label = "Model"
kind = "text"
args = ["--first", "{value}"]

[[settings]]
key = "model"
label = "Model again"
kind = "text"
args = ["--second", "{value}"]
"#,
    );

    assert_eq!(def.settings.len(), 1);
    assert_eq!(def.settings[0].args[0], "--first");
}

#[test]
fn an_option_a_command_line_cannot_carry_is_dropped_and_the_rest_kept() {
    let def = demo_harness(
        r#"
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["small", "a&b", "-x"]
args = ["--model", "{value}"]
"#,
    );

    assert!(matches!(
        &setting(&def, "model").kind,
        SettingKind::Choice { options, .. } if options == &vec!["small".to_string()]
    ));
}

#[test]
fn a_choice_with_no_usable_option_is_dropped() {
    let def = demo_harness(
        r#"
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["a&b"]
args = ["--model", "{value}"]
"#,
    );

    assert!(def.settings.is_empty());
}

#[test]
fn a_default_that_is_no_option_is_dropped_unless_it_can_be_typed() {
    let def = demo_harness(
        r#"
[[settings]]
key = "fixed"
label = "Fixed"
kind = "choice"
options = ["small"]
default = "huge"
args = ["--fixed", "{value}"]

[[settings]]
key = "typed"
label = "Typed"
kind = "choice"
options = ["small"]
custom = true
default = "huge"
args = ["--typed", "{value}"]

[[settings]]
key = "unsafe"
label = "Unsafe"
kind = "choice"
options = ["small"]
custom = true
default = "a&b"
args = ["--unsafe", "{value}"]
"#,
    );

    assert_eq!(setting(&def, "fixed").file_default(), "");
    assert_eq!(setting(&def, "typed").file_default(), "huge");
    assert_eq!(setting(&def, "unsafe").file_default(), "");
}

#[test]
fn custom_on_a_flag_or_text_drops_the_setting() {
    let def = demo_harness(
        r#"
[[settings]]
key = "flag"
label = "Flag"
kind = "bool"
custom = true
args = ["--flag"]

[[settings]]
key = "note"
label = "Note"
kind = "text"
custom = true
args = ["--note", "{value}"]
"#,
    );

    assert!(def.settings.is_empty(), "{:?}", def.settings);
}

#[test]
fn a_flag_that_names_a_value_is_dropped() {
    let def = demo_harness(
        r#"
[[settings]]
key = "flag"
label = "Flag"
kind = "bool"
args = ["--flag={value}"]
"#,
    );

    assert!(def.settings.is_empty());
}

#[test]
fn a_file_default_is_the_files_own_or_agent_default() {
    let def = demo_harness(DEMO_SETTINGS);

    assert_eq!(setting(&def, "model").file_default(), "");
    assert_eq!(setting(&def, "effort").file_default(), "");
    assert_eq!(setting(&def, "bypass").file_default(), "true");
}

#[test]
fn a_value_is_checked_against_its_setting() {
    let def = demo_harness(DEMO_SETTINGS);
    let model = setting(&def, "model");
    let effort = setting(&def, "effort");
    let bypass = setting(&def, "bypass");

    assert!(model.check("").is_ok(), "agent default");
    assert!(model.check("small").is_ok());
    assert!(
        model.check("typed-1").is_ok(),
        "a custom choice takes a typed value"
    );
    assert!(
        model
            .check("a&b")
            .expect_err("refused")
            .contains(SAFE_CHARACTERS)
    );

    assert!(effort.check("high").is_ok());
    assert!(
        effort
            .check("extreme")
            .expect_err("refused")
            .contains("not one of")
    );

    assert!(bypass.check("false").is_ok());
    assert!(bypass.check("yes").expect_err("refused").contains("true"));
    assert!(
        bypass.check("").is_err(),
        "a flag is on or off, never unset"
    );
}

#[test]
fn a_text_setting_takes_any_safe_value() {
    let def = demo_harness(
        r#"
[[settings]]
key = "model"
label = "Model"
kind = "text"
args = ["--model", "{value}"]
"#,
    );
    let model = setting(&def, "model");

    assert!(model.check("").is_ok());
    assert!(model.check("provider/model#high").is_ok());
    assert!(model.check("-x").is_err());
}

/// A permission setting with a default of its own, as Claude's has.
const PERMISSIONS: &str = r#"
[[settings]]
key = "permissions"
label = "Permissions"
kind = "choice"
options = ["ask", "never"]
default = "never"
args = ["--permissions", "{value}"]
"#;

/// `pairs` as [`Choices`].
fn values(pairs: &[(&str, &str)]) -> Choices {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

#[test]
fn what_was_chosen_beats_what_was_saved_which_beats_the_files_default() {
    let def = demo_harness(DEMO_SETTINGS);

    let resolved = def
        .resolve(
            &values(&[("model", "large")]),
            &values(&[("model", "small"), ("effort", "high")]),
        )
        .expect("every value is usable");

    assert_eq!(
        resolved,
        values(&[("model", "large"), ("effort", "high"), ("bypass", "true")])
    );
}

#[test]
fn agent_default_saved_over_a_files_default_stays_agent_default() {
    let def = demo_harness(PERMISSIONS);

    let resolved = def
        .resolve(&Choices::new(), &values(&[("permissions", "")]))
        .expect("agent default is usable");

    assert_eq!(resolved, values(&[("permissions", "")]));
    assert_eq!(
        def.launch_with("linux", &resolved).args,
        vec!["--tui"],
        "agent default passes nothing"
    );
}

#[test]
fn a_chosen_setting_the_harness_does_not_have_is_refused() {
    let def = demo_harness(DEMO_SETTINGS);

    let refused = def
        .resolve(&values(&[("colour", "red")]), &Choices::new())
        .expect_err("refused");

    assert_eq!(refused, "demo has no setting \"colour\"");
}

#[test]
fn a_chosen_value_the_setting_cannot_take_is_refused_by_name() {
    let def = demo_harness(DEMO_SETTINGS);

    for (key, value) in [
        ("effort", "extreme"),
        ("model", "a&b"),
        ("model", "--yolo"),
        ("bypass", "yes"),
    ] {
        let refused = def
            .resolve(&values(&[(key, value)]), &Choices::new())
            .expect_err("refused");
        assert!(
            refused.starts_with("demo: ") && refused.contains(key),
            "{key} = {value:?}: {refused}"
        );
    }
}

#[test]
fn a_saved_value_the_harness_cannot_take_is_ignored() {
    // The saved file is the user's: a stale entry in it must not stop the
    // harness opening.
    let def = demo_harness(DEMO_SETTINGS);
    let saved = values(&[("model", "large"), ("effort", "extreme"), ("colour", "red")]);

    assert_eq!(def.usable_saved(&saved), values(&[("model", "large")]));
    assert_eq!(
        def.resolve(&Choices::new(), &saved)
            .expect("nothing chosen"),
        values(&[("model", "large"), ("effort", ""), ("bypass", "true")])
    );
}

#[test]
fn values_become_flags_after_the_launchs_own_arguments() {
    let def = demo_harness(DEMO_SETTINGS);
    let resolved = values(&[("model", "large"), ("effort", ""), ("bypass", "true")]);

    assert_eq!(
        def.launch_with("linux", &resolved).args,
        vec!["--tui", "--model", "large", "--yolo"]
    );
}

#[test]
fn unset_values_and_flags_turned_off_add_nothing() {
    let def = demo_harness(DEMO_SETTINGS);
    let resolved = values(&[("model", ""), ("effort", ""), ("bypass", "false")]);

    assert_eq!(def.launch_with("linux", &resolved).args, vec!["--tui"]);
}

#[test]
fn a_windows_launch_gets_its_flags_after_the_wrapper() {
    let def = demo_harness(DEMO_SETTINGS);
    let resolved = values(&[("model", "large"), ("effort", ""), ("bypass", "true")]);

    let launch = def.launch_with("windows", &resolved);

    assert_eq!(launch.command, "cmd.exe");
    assert_eq!(
        launch.args,
        vec!["/c", "demo", "--model", "large", "--yolo"]
    );
}

#[test]
fn a_one_shot_run_gets_the_flags_after_its_task() {
    let def = demo_harness(DEMO_SETTINGS);
    let resolved = values(&[("model", "large"), ("effort", ""), ("bypass", "true")]);

    let run = def
        .task_launch_with("linux", "do it", &resolved)
        .expect("it has a task form");

    assert_eq!(
        run.launch.args,
        vec!["-p", "do it", "--model", "large", "--yolo"]
    );
}

#[test]
fn a_windows_file_form_keeps_its_redirect_and_gets_the_flags() {
    // cmd.exe reads a redirect wherever it stands, so flags after it still
    // reach the agent, and the task still never touches the command line.
    let def = demo_harness(DEMO_SETTINGS);
    let resolved = values(&[("model", "large"), ("effort", ""), ("bypass", "true")]);

    let run = def
        .task_launch_with("windows", "x & y", &resolved)
        .expect("it has a Windows task form");

    assert_eq!(run.input, crate::TaskInput::File);
    assert_eq!(
        run.launch.args,
        vec![
            "/d",
            "/c",
            "demo",
            "-p",
            "<%DISPATCH_TASK_FILE%",
            "--model",
            "large",
            "--yolo"
        ]
    );
}

#[test]
fn a_settings_variable_carries_the_value_and_wins_over_the_harnesss_own() {
    let def = demo_harness(
        r#"
[env]
MODEL_JSON = "stale"

[[settings]]
key = "model"
label = "Model"
kind = "text"
env = { MODEL_JSON = '{"model":"{value}"}' }
"#,
    );

    let launch = def.launch_with("linux", &values(&[("model", "p/m#high")]));

    assert_eq!(
        launch.env.get("MODEL_JSON").map(String::as_str),
        Some(r#"{"model":"p/m#high"}"#)
    );
}

#[test]
fn a_launch_leaves_out_a_value_that_is_not_safe_however_it_arrived() {
    // `launch_with` is handed resolved values, but what it adds lands on a
    // command line cmd.exe parses, so it checks again.
    let def = demo_harness(DEMO_SETTINGS);

    let launch = def.launch_with(
        "linux",
        &values(&[("model", "a&b"), ("effort", ""), ("bypass", "true")]),
    );

    assert_eq!(launch.args, vec!["--tui", "--yolo"]);
}

#[test]
fn only_what_differs_from_the_files_defaults_is_described() {
    let def = demo_harness(DEMO_SETTINGS);

    assert_eq!(
        def.describe_changes(&values(&[
            ("model", "large"),
            ("effort", ""),
            ("bypass", "false")
        ])),
        vec!["large", "Skip prompts off"]
    );
    assert!(
        def.describe_changes(&values(&[
            ("model", ""),
            ("effort", ""),
            ("bypass", "true")
        ]))
        .is_empty()
    );
}

#[test]
fn agent_default_over_a_files_default_is_described_by_name() {
    let def = demo_harness(PERMISSIONS);

    assert_eq!(
        def.describe_changes(&values(&[("permissions", "")])),
        vec!["Permissions agent default"]
    );
}
