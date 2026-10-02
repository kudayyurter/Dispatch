//! Tests for the top-level configuration file.

use super::*;

use crate::testing::TempDir;

#[test]
fn a_missing_file_means_the_defaults() {
    // A fresh install has no config.toml, and must behave like a configured one
    // that changed nothing.
    let dir = TempDir::new("missing");
    let config =
        Config::load(&dir.path().join("config.toml")).expect("an absent file is not an error");

    assert_eq!(config, Config::default());
    assert_eq!(config.delegation.max_depth, 1);
    assert_eq!(config.delegation.max_live_per_parent, 4);
    assert_eq!(config.delegation.request_timeout_secs, 600);
}

#[test]
fn the_caps_can_be_raised_deliberately() {
    let dir = TempDir::new("raised");
    let path = dir.config(
        r#"
[delegation]
max_depth = 2
max_live_per_parent = 8
request_timeout_secs = 60
"#,
    );

    let config = Config::load(&path).expect("the file parses");
    assert_eq!(config.delegation.max_depth, 2);
    assert_eq!(config.delegation.max_live_per_parent, 8);
    assert_eq!(config.delegation.request_timeout_secs, 60);
}

#[test]
fn a_partly_written_section_keeps_the_other_defaults() {
    let dir = TempDir::new("partial");
    let path = dir.config("[delegation]\nmax_depth = 2\n");

    let config = Config::load(&path).expect("the file parses");
    assert_eq!(config.delegation.max_depth, 2);
    assert_eq!(
        config.delegation.max_live_per_parent, 4,
        "an unmentioned cap keeps its default"
    );
}

#[test]
fn a_key_this_build_does_not_know_is_kept_and_reported() {
    // A newer daemon's key must not stop an older one starting, and a typo must
    // not be silent.
    let dir = TempDir::new("unknown");
    let path = dir.config(
        r#"
[delegation]
max_depth = 1
max_liv_per_parent = 9
"#,
    );

    let loaded = Config::load_reporting(&path).expect("the file parses");
    assert_eq!(loaded.config.delegation.max_live_per_parent, 4);
    assert_eq!(
        loaded.unknown,
        vec!["delegation.max_liv_per_parent".to_string()],
        "the key is named so a typo can be found"
    );
}

#[test]
fn a_broken_file_names_itself() {
    let dir = TempDir::new("broken");
    let path = dir.config("[delegation\nmax_depth = 1\n");

    let error = Config::load(&path).expect_err("invalid TOML is an error");
    assert!(
        error.to_string().contains("config.toml"),
        "the user has to be told which file to fix, got {error}"
    );
}

#[test]
fn motion_is_on_unless_turned_off() {
    assert!(Config::default().interface.motion);

    let config: Config = toml::from_str("[interface]\nmotion = false\n").expect("parses");
    assert!(!config.interface.motion);
}

#[test]
fn hovering_claims_panes_only_when_turned_on() {
    assert!(!Config::default().interface.hover_claims_panes);

    let config: Config =
        toml::from_str("[interface]\nhover_claims_panes = true\n").expect("parses");
    assert!(config.interface.hover_claims_panes);

    let raw: toml::Table =
        toml::from_str("[interface]\nhover_claims_panes = true\n").expect("parses");
    assert!(unknown_keys(&raw).is_empty(), "a key this build knows");
}

#[test]
fn focus_follows_the_pointer_only_when_turned_on() {
    assert!(!Config::default().interface.focus_follows_pointer);

    let config: Config =
        toml::from_str("[interface]\nfocus_follows_pointer = true\n").expect("parses");
    assert!(config.interface.focus_follows_pointer);

    let raw: toml::Table =
        toml::from_str("[interface]\nfocus_follows_pointer = true\n").expect("parses");
    assert!(unknown_keys(&raw).is_empty(), "a key this build knows");
}

#[test]
fn the_interface_section_is_not_reported_unknown() {
    let raw: toml::Table = toml::from_str("[interface]\nmotion = false\n").expect("parses");
    assert!(unknown_keys(&raw).is_empty());

    let raw: toml::Table = toml::from_str("[interface]\nsparkles = true\n").expect("parses");
    assert_eq!(unknown_keys(&raw), vec!["interface.sparkles".to_string()]);
}

/// Loads `text` as a `config.toml`.
fn load(label: &str, text: &str) -> Config {
    let dir = TempDir::new(label);
    Config::load(&dir.config(text)).expect("the file parses")
}

#[test]
fn a_shell_section_is_read() {
    let config = load(
        "shell",
        "[shell]\ncommand = \"/usr/bin/fish\"\nargs = [\"--private\"]\nlogin = \"always\"\n",
    );

    assert_eq!(
        config.shell,
        ShellConfig {
            command: Some("/usr/bin/fish".into()),
            args: vec!["--private".into()],
            login: LoginShell::Always,
        }
    );
}

#[test]
fn without_a_shell_section_the_shell_is_found_and_login_decided_for_this_platform() {
    let config = load("no-shell", "");

    assert_eq!(config.shell, ShellConfig::default());
    assert_eq!(config.shell.login, LoginShell::Auto);
    assert_eq!(config.shell.command, None);
}

#[test]
fn a_login_value_that_is_not_one_of_the_three_is_an_error() {
    let dir = TempDir::new("bad-login");
    let path = dir.config("[shell]\nlogin = \"sometimes\"\n");

    assert!(Config::load(&path).is_err());
}

#[test]
fn an_unknown_shell_key_is_reported_by_name() {
    let dir = TempDir::new("shell-unknown");
    let path = dir.config("[shell]\ncommand = \"sh\"\nprompt = \"starship\"\n");

    let loaded = Config::load_reporting(&path).expect("the file loads");

    assert_eq!(loaded.unknown, vec!["shell.prompt".to_string()]);
}

#[test]
fn a_named_shell_is_run_as_named() {
    let shell = ShellConfig {
        command: Some("/opt/bin/nu".into()),
        args: vec!["--no-history".into()],
        login: LoginShell::Never,
    };

    let launch = shell.launch_with(|| unreachable!("not asked"), true, true);

    assert_eq!(launch.command, "/opt/bin/nu");
    assert_eq!(launch.args, vec!["--no-history".to_string()]);
}

#[test]
fn with_no_shell_named_the_machines_own_is_used() {
    let launch = ShellConfig::default().launch_with(|| "/usr/bin/zsh".into(), false, true);

    assert_eq!(launch.command, "/usr/bin/zsh");
    assert!(
        launch.args.is_empty(),
        "no -l where logins are not the habit"
    );
}

#[test]
fn auto_logs_in_where_that_is_the_platforms_habit_and_args_follow_the_flag() {
    let shell = ShellConfig {
        command: None,
        args: vec!["--extra".into()],
        login: LoginShell::Auto,
    };

    let launch = shell.launch_with(|| "/bin/zsh".into(), true, true);

    assert_eq!(launch.args, vec!["-l".to_string(), "--extra".to_string()]);
}

#[test]
fn always_and_never_override_the_platform() {
    let always = ShellConfig {
        login: LoginShell::Always,
        ..ShellConfig::default()
    };
    let never = ShellConfig {
        login: LoginShell::Never,
        ..ShellConfig::default()
    };

    assert_eq!(
        always.launch_with(|| "sh".into(), false, true).args,
        vec!["-l".to_string()]
    );
    assert!(
        never
            .launch_with(|| "sh".into(), true, true)
            .args
            .is_empty()
    );
}

#[test]
fn a_shell_that_takes_no_login_flag_is_never_given_one() {
    // PowerShell has no `-l`; passing it would stop the pane starting.
    let always = ShellConfig {
        login: LoginShell::Always,
        ..ShellConfig::default()
    };

    assert!(
        always
            .launch_with(|| "pwsh".into(), true, false)
            .args
            .is_empty()
    );
}

#[test]
fn a_blank_command_counts_as_none() {
    let blank = ShellConfig {
        command: Some("  ".into()),
        ..ShellConfig::default()
    };

    assert_eq!(
        blank.launch_with(|| "/bin/sh".into(), false, true).command,
        "/bin/sh"
    );
}

#[test]
fn a_keys_section_is_kept_as_written() {
    let config = load(
        "keys",
        "[keys.pane]\n\"w\" = \"close_pane\"\nclear = true\n\n[keys.normal]\n\"Ctrl q\" = \"quit\"\n",
    );

    let KeyTable::Table(pane) = &config.keys.modes["pane"] else {
        panic!("a mode's table is kept as one");
    };
    assert_eq!(pane["w"], KeyValue::Command("close_pane".into()));
    assert_eq!(pane["clear"], KeyValue::Flag(true));
    assert_eq!(
        config.keys.modes["normal"],
        KeyTable::Table([("Ctrl q".to_string(), KeyValue::Command("quit".into()))].into())
    );
    assert!(!config.keys.not_a_table);
}

#[test]
fn a_key_given_the_wrong_kind_of_value_is_kept_to_be_reported() {
    // A number where a command's name goes costs that one binding, not the
    // whole file: the interface says what it cannot use.
    let config = load("keys-type", "[keys.pane]\nx = 3\n");

    assert_eq!(
        config.keys.modes["pane"],
        KeyTable::Table([("x".to_string(), KeyValue::Other("integer".into()))].into())
    );
}

#[test]
fn without_a_keys_section_nothing_is_rebound() {
    assert_eq!(load("no-keys", "").keys, KeysConfig::default());
}

#[test]
fn the_keys_section_is_judged_by_the_interface_not_reported_unknown() {
    let dir = TempDir::new("keys-known");
    let path = dir.config("[keys.whatever]\nx = \"none\"\n");

    let loaded = Config::load_reporting(&path).expect("the file loads");

    assert!(loaded.unknown.is_empty(), "{:?}", loaded.unknown);
}

/// Loads `text` as a `config.toml` through the path both binaries use, which
/// must not fail on anything `[keys]` says.
fn load_keys(label: &str, text: &str) -> LoadedConfig {
    let dir = TempDir::new(label);
    Config::load_reporting(&dir.config(text))
        .unwrap_or_else(|error| panic!("a mistake in [keys] stops nothing: {error}"))
}

#[test]
fn a_binding_written_straight_under_keys_does_not_stop_the_file_loading() {
    let loaded = load_keys("keys-bare", "[keys]\n\"Ctrl q\" = \"quit\"\n");

    assert_eq!(
        loaded.config.keys.modes["Ctrl q"],
        KeyTable::Other("string".into()),
        "kept for the interface to name"
    );
    assert!(loaded.unknown.is_empty(), "{:?}", loaded.unknown);
}

#[test]
fn clear_written_straight_under_keys_does_not_stop_the_file_loading() {
    let loaded = load_keys("keys-clear", "[keys]\nclear = true\n");

    assert_eq!(
        loaded.config.keys.modes["clear"],
        KeyTable::Other("boolean".into())
    );
}

#[test]
fn keys_that_is_not_a_table_does_not_stop_the_file_loading() {
    let loaded = load_keys("keys-string", "keys = \"vim\"\n");

    assert!(loaded.config.keys.not_a_table, "noted, to be reported");
    assert!(loaded.config.keys.modes.is_empty(), "and binds nothing");
}

#[test]
fn a_mode_written_as_an_array_of_tables_does_not_stop_the_file_loading() {
    let loaded = load_keys("keys-array", "[[keys.normal]]\n\"Ctrl q\" = \"quit\"\n");

    assert_eq!(
        loaded.config.keys.modes["normal"],
        KeyTable::Other("array".into())
    );
}

#[test]
fn interface_present_is_true_only_for_keys_written() {
    let dir = TempDir::new("interface-present");
    let path = dir.config("[interface]\nmotion = true\nhover_claims_panes = false\n");

    let present = Config::load_reporting(&path)
        .expect("the file loads")
        .interface_present;

    assert!(present.motion);
    assert!(present.hover_claims_panes, "false is still written");
    assert!(!present.focus_follows_pointer);
}

#[test]
fn interface_present_is_all_false_without_a_file_or_a_section() {
    let dir = TempDir::new("interface-present-none");
    let missing = Config::load_reporting(&dir.path().join("config.toml")).expect("loads");
    assert_eq!(missing.interface_present, InterfaceKeysPresent::default());

    let path = dir.config("[shell]\nlogin = \"never\"\n");
    let loaded = Config::load_reporting(&path).expect("loads");
    assert_eq!(loaded.interface_present, InterfaceKeysPresent::default());
}

#[test]
fn interactive_subagents_close_on_report_by_default() {
    assert_eq!(
        DelegationLimits::default().interactive_on_done,
        OnDone::Close
    );
}

#[test]
fn interactive_on_done_can_ask_instead() {
    let config: Config =
        toml::from_str("[delegation]\ninteractive_on_done = \"ask\"\n").expect("valid");
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
    let raw: toml::Table =
        toml::from_str("[delegation]\ninteractive_on_done = \"ask\"\n").expect("valid");
    assert!(unknown_keys(&raw).is_empty());
}
