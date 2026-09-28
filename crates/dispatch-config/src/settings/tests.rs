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
