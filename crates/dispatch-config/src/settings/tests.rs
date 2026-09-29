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

    assert_eq!(
        setting(&def, "model").options(),
        [ChoiceOption::new("small")]
    );
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

/// A model whose options are tables with labels, mixed with a plain one.
const LABELLED: &str = r#"
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = [{ value = "small", label = "Small" }, "large"]
custom = true
args = ["--model", "{value}"]
"#;

#[test]
fn an_option_can_be_a_table_with_a_label_mixed_with_plain_ones() {
    let def = demo_harness(LABELLED);
    let model = setting(&def, "model");

    let values: Vec<&str> = model.options().iter().map(|o| o.value.as_str()).collect();
    assert_eq!(values, ["small", "large"]);
    assert_eq!(model.options()[0].label(), "Small");
    assert_eq!(
        model.options()[1].label(),
        "large",
        "a label defaults to the value"
    );
}

#[test]
fn a_table_option_with_no_value_is_dropped_and_the_rest_kept() {
    let def = demo_harness(
        r#"
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["small", { label = "Nameless" }]
args = ["--model", "{value}"]
"#,
    );

    let model = setting(&def, "model");
    let values: Vec<&str> = model.options().iter().map(|o| o.value.as_str()).collect();
    assert_eq!(values, ["small"]);
}

#[test]
fn a_label_the_popup_cannot_draw_falls_back_to_the_value() {
    // An escape sequence in a label would be written to the terminal as is.
    let def = demo_harness(
        r#"
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = [{ value = "small", label = "\u001b[2JSmall" }, { value = "large", label = "  " }]
args = ["--model", "{value}"]
"#,
    );
    let model = setting(&def, "model");

    assert_eq!(model.options()[0].label(), "small");
    assert_eq!(model.options()[1].label(), "large");
}

#[test]
fn a_chosen_option_is_described_by_its_label_and_a_typed_one_as_itself() {
    let def = demo_harness(LABELLED);

    assert_eq!(
        def.describe_changes(&values(&[("model", "small")])),
        vec!["Small"]
    );
    assert_eq!(
        def.describe_changes(&values(&[("model", "large")])),
        vec!["large"]
    );
    assert_eq!(
        def.describe_changes(&values(&[("model", "big-1")])),
        vec!["big-1"]
    );
}

#[test]
fn an_option_written_back_reads_the_same() {
    let model = setting(&demo_harness(LABELLED), "model");

    let written = toml::Value::try_from(&model).expect("a setting serialises");
    let back: SettingDef = written.try_into().expect("and reads back");

    assert_eq!(back, model);
}

/// A model that decides which efforts may be chosen: two list theirs, one
/// lists none, and a model can be typed.
const LIMITED: &str = r#"
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = [
  { value = "flash", label = "Flash", effort = ["low", "medium", "high"] },
  { value = "pro", label = "Pro", effort = ["low", "high"] },
  { value = "plain", label = "Plain" },
]
custom = true
args = ["--model", "{value}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
limited_by = "model"
options = [
  { value = "low", label = "Low" },
  { value = "medium", label = "Medium" },
  { value = "high", label = "High" },
]
args = ["--effort", "{value}"]
"#;

/// What `chosen` resolves to for [`LIMITED`], nothing saved.
fn fitted(chosen: &[(&str, &str)]) -> Choices {
    demo_harness(LIMITED)
        .resolve(&values(chosen), &Choices::new())
        .expect("nothing here is refused")
}

#[test]
fn an_effort_the_model_offers_is_kept() {
    assert_eq!(
        fitted(&[("model", "pro"), ("effort", "low")])["effort"],
        "low"
    );
}

#[test]
fn an_effort_the_model_does_not_offer_becomes_its_highest() {
    assert_eq!(
        fitted(&[("model", "pro"), ("effort", "medium")])["effort"],
        "high"
    );
    assert_eq!(
        fitted(&[("model", "flash"), ("effort", "")])["effort"],
        "high",
        "a model that lists efforts takes one: agent default is not offered"
    );
}

#[test]
fn a_model_that_lists_no_effort_leaves_it_unset_and_passes_none() {
    let def = demo_harness(LIMITED);
    let values = def
        .resolve(
            &values(&[("model", "plain"), ("effort", "high")]),
            &Choices::new(),
        )
        .expect("fitted, not refused");

    assert_eq!(values["effort"], "");
    assert_eq!(
        def.launch_with("linux", &values).args,
        ["--tui", "--model", "plain"]
    );
}

#[test]
fn a_typed_or_unset_model_limits_nothing() {
    assert_eq!(
        fitted(&[("model", "big-1"), ("effort", "medium")])["effort"],
        "medium"
    );
    assert_eq!(fitted(&[("model", "big-1"), ("effort", "")])["effort"], "");
    assert_eq!(
        fitted(&[("model", ""), ("effort", "medium")])["effort"],
        "medium"
    );
}

#[test]
fn a_chosen_effort_that_is_no_option_at_all_is_still_refused() {
    let refused = demo_harness(LIMITED).resolve(
        &values(&[("model", "pro"), ("effort", "ultra")]),
        &Choices::new(),
    );

    assert!(refused.is_err(), "{refused:?}");
}

#[test]
fn a_saved_pair_is_fitted_too() {
    let values = demo_harness(LIMITED)
        .resolve(
            &Choices::new(),
            &values(&[("model", "pro"), ("effort", "medium")]),
        )
        .expect("nothing chosen");

    assert_eq!(values["effort"], "high");
}

#[test]
fn a_launch_fits_the_values_it_is_handed_however_they_arrived() {
    let def = demo_harness(LIMITED);

    let args = def
        .launch_with("linux", &values(&[("model", "pro"), ("effort", "medium")]))
        .args;

    assert_eq!(args, ["--tui", "--model", "pro", "--effort", "high"]);
}

#[test]
fn allowed_says_what_a_limited_setting_may_take() {
    let def = demo_harness(LIMITED);
    let allowed_for = |model: &str| allowed(&def.settings, "effort", &values(&[("model", model)]));

    assert_eq!(
        allowed_for("pro"),
        Allowed::Only(vec!["low".into(), "high".into()])
    );
    assert_eq!(allowed_for("plain"), Allowed::Unavailable);
    assert_eq!(allowed_for("big-1"), Allowed::Free);
    assert_eq!(allowed_for(""), Allowed::Free);
    assert_eq!(
        allowed(&def.settings, "model", &values(&[("model", "pro")])),
        Allowed::Free,
        "the model itself is limited by nothing"
    );
}

#[test]
fn a_limited_setting_declared_first_is_fitted_all_the_same() {
    // The effort row above the model row in the file.
    let mut parts: Vec<&str> = LIMITED
        .split("[[settings]]")
        .filter(|part| !part.trim().is_empty())
        .collect();
    parts.reverse();
    let reordered: String = parts
        .iter()
        .map(|part| format!("[[settings]]{part}"))
        .collect();
    let def = demo_harness(&reordered);
    assert_eq!(def.settings[0].key, "effort");

    let values = def
        .resolve(
            &values(&[("model", "pro"), ("effort", "medium")]),
            &Choices::new(),
        )
        .expect("fitted");

    assert_eq!(values["effort"], "high");
}

#[test]
fn a_limit_that_cannot_work_is_dropped_and_the_setting_kept() {
    for limited_by in ["nothing", "bypass", "effort"] {
        let def = demo_harness(&format!(
            r#"
[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
limited_by = "{limited_by}"
options = ["low", "high"]
args = ["--effort", "{{value}}"]

[[settings]]
key = "bypass"
label = "Skip prompts"
kind = "bool"
args = ["--yolo"]
"#
        ));

        assert_eq!(
            setting(&def, "effort").limited_by,
            None,
            "limited_by = {limited_by:?}"
        );
    }
}

#[test]
fn a_limit_on_a_flag_is_dropped() {
    let def = demo_harness(
        r#"
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["small"]
args = ["--model", "{value}"]

[[settings]]
key = "bypass"
label = "Skip prompts"
kind = "bool"
limited_by = "model"
args = ["--yolo"]
"#,
    );

    assert_eq!(setting(&def, "bypass").limited_by, None);
}

#[test]
fn a_chain_of_limits_is_cut_where_it_would_chain() {
    let def = demo_harness(
        r#"
[[settings]]
key = "a"
label = "A"
kind = "choice"
options = ["x"]
args = ["--a", "{value}"]

[[settings]]
key = "b"
label = "B"
kind = "choice"
limited_by = "a"
options = ["y"]
args = ["--b", "{value}"]

[[settings]]
key = "c"
label = "C"
kind = "choice"
limited_by = "b"
options = ["z"]
args = ["--c", "{value}"]
"#,
    );

    assert_eq!(setting(&def, "b").limited_by.as_deref(), Some("a"));
    assert_eq!(setting(&def, "c").limited_by, None);
}

#[test]
fn an_options_list_keeps_only_what_it_may_name() {
    // `ultra` is no effort option, and nothing called colour is limited by
    // the model.
    let def = demo_harness(&LIMITED.replace(
        r#"effort = ["low", "high"] }"#,
        r#"effort = ["low", "ultra"], colour = ["red"] }"#,
    ));
    let pro = setting(&def, "model")
        .option("pro")
        .cloned()
        .expect("pro is offered");

    assert_eq!(pro.allows.get("effort"), Some(&vec!["low".to_string()]));
    assert!(!pro.allows.contains_key("colour"));
}

#[test]
fn a_malformed_option_costs_that_option_or_list_not_the_harness() {
    // A string where a list belongs is taken as a list of one; anything else
    // that is not a list of strings, a label that is not a string, and an
    // option that is neither a string nor a table are dropped.
    let def = demo_harness(
        r#"
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = [
  { value = "one", label = "One", effort = "medium" },
  { value = "typo", lable = "Typo" },
  { value = "flagged", hidden = true, effort = [1, 2] },
  { value = "numbered", label = 3 },
  { value = 4 },
  5,
  "plain",
]
args = ["--model", "{value}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
limited_by = "model"
options = ["low", "medium"]
args = ["--effort", "{value}"]
"#,
    );
    let model = setting(&def, "model");

    let kept: Vec<&str> = model.options().iter().map(|o| o.value.as_str()).collect();
    assert_eq!(kept, ["one", "typo", "flagged", "numbered", "plain"]);
    let one = model.option("one").expect("kept");
    assert_eq!(one.label(), "One");
    assert_eq!(one.allows.get("effort"), Some(&vec!["medium".to_string()]));
    assert!(model.option("typo").expect("kept").allows.is_empty());
    assert!(model.option("flagged").expect("kept").allows.is_empty());
    assert_eq!(model.option("numbered").expect("kept").label(), "numbered");
}
