//! Tests for the saved harness settings.

use super::*;

use crate::testing::{DEMO_SETTINGS, TempDir, demo_harness};

/// `pairs` as [`Choices`].
fn values(pairs: &[(&str, &str)]) -> Choices {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// The saved file, parsed.
fn table(dir: &TempDir) -> toml::Table {
    std::fs::read_to_string(dir.path().join(FILE))
        .expect("the file exists")
        .parse()
        .expect("it is valid TOML")
}

#[test]
fn nothing_saved_is_nothing_loaded() {
    let dir = TempDir::new("settings-none");

    assert_eq!(
        load(dir.path(), "demo").expect("no file is fine"),
        Choices::new()
    );
}

#[test]
fn a_saved_value_comes_back() {
    let dir = TempDir::new("settings-round-trip");
    let def = demo_harness(DEMO_SETTINGS);

    save(dir.path(), &def, &values(&[("model", "large")])).expect("it saves");

    assert_eq!(
        load(dir.path(), "demo").expect("it loads"),
        values(&[("model", "large")])
    );
    assert_eq!(table(&dir)["demo"]["model"].as_str(), Some("large"));
}

#[test]
fn a_flag_is_saved_as_a_toml_boolean() {
    let dir = TempDir::new("settings-flag");
    let def = demo_harness(DEMO_SETTINGS);

    save(dir.path(), &def, &values(&[("bypass", "false")])).expect("it saves");

    assert_eq!(table(&dir)["demo"]["bypass"].as_bool(), Some(false));
    assert_eq!(
        load(dir.path(), "demo").expect("it loads"),
        values(&[("bypass", "false")])
    );
}

#[test]
fn a_value_set_back_to_the_files_default_is_removed() {
    // So a default Dispatch later ships still reaches every setting the
    // user never changed.
    let dir = TempDir::new("settings-back-to-default");
    let def = demo_harness(DEMO_SETTINGS);

    save(dir.path(), &def, &values(&[("bypass", "false")])).expect("it saves");
    save(
        dir.path(),
        &def,
        &values(&[("model", ""), ("effort", ""), ("bypass", "true")]),
    )
    .expect("it saves");

    assert_eq!(load(dir.path(), "demo").expect("it loads"), Choices::new());
    assert!(
        !table(&dir).contains_key("demo"),
        "an emptied harness leaves no table behind"
    );
}

#[test]
fn agent_default_over_a_files_default_is_saved_as_empty() {
    let dir = TempDir::new("settings-agent-default");
    let def = demo_harness(
        r#"
[[settings]]
key = "permissions"
label = "Permissions"
kind = "choice"
options = ["ask", "never"]
default = "never"
args = ["--permissions", "{value}"]
"#,
    );

    save(dir.path(), &def, &values(&[("permissions", "")])).expect("it saves");

    assert_eq!(table(&dir)["demo"]["permissions"].as_str(), Some(""));
    assert_eq!(
        load(dir.path(), "demo").expect("it loads"),
        values(&[("permissions", "")])
    );
}

#[test]
fn a_value_the_setting_cannot_take_is_not_written() {
    let dir = TempDir::new("settings-refused");
    let def = demo_harness(DEMO_SETTINGS);

    save(
        dir.path(),
        &def,
        &values(&[("model", "a&b"), ("effort", "extreme"), ("colour", "red")]),
    )
    .expect("saving what can be saved succeeds");

    assert_eq!(load(dir.path(), "demo").expect("it loads"), Choices::new());
}

#[test]
fn another_harnesss_values_are_left_alone() {
    let dir = TempDir::new("settings-others");
    dir.write(FILE, "[other]\nmodel = \"kept\"\n");
    let def = demo_harness(DEMO_SETTINGS);

    save(dir.path(), &def, &values(&[("model", "large")])).expect("it saves");

    assert_eq!(
        load(dir.path(), "other").expect("it loads"),
        values(&[("model", "kept")])
    );
}

#[test]
fn an_unreadable_file_loads_as_empty_and_refuses_to_save() {
    let dir = TempDir::new("settings-unreadable");
    dir.write(FILE, "not = [valid");
    let def = demo_harness(DEMO_SETTINGS);

    assert!(load(dir.path(), "demo").is_err(), "load reports it");
    assert_eq!(load_or_empty(dir.path(), "demo"), Choices::new());
    assert!(
        save(dir.path(), &def, &values(&[("model", "large")])).is_err(),
        "saving refuses rather than overwrites"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join(FILE)).expect("it is still there"),
        "not = [valid"
    );
}
