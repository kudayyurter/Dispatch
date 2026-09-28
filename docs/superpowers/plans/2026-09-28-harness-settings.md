# Harness Settings: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every shipped harness starts its agent without permission prompts. A popup, opened with `e` in the new-pane picker, lets the user pick a harness's model, effort and permissions for one pane, or save them as that harness's defaults.

**Architecture:**
- Each `[[settings]]` entry in a harness file carries its own `args` and `env` templates, with `{value}` standing for the chosen value (approach A in the spec). `dispatch-config` checks settings when a harness loads, resolves values (chosen, then saved, then the file's default), and builds launches from them. It also reads and writes `harness-settings.toml` through the existing locked `store::update`.
- `SpawnPane` gains a `settings` map. The client sends what the user chose or saved. The daemon checks every value, fills in the rest from its own saved file and the harness file, and refuses anything it cannot take. Delegated subagents get the saved defaults.
- `dispatch-tui` gains a self-contained `SettingsForm` widget. `app.rs` only wires the `e` key, the overlay, spawning and saving.

**Tech Stack:** Rust 2024 (rust-version 1.89), serde + toml 0.9, ratatui 0.30, crossterm 0.29 (through `dispatch_tui::input`), tracing. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-28-harness-settings-design.md`

## Global Constraints

- No new dependencies. Protocol `VERSION` stays `1.1`; the new `SpawnPane.settings` field is `#[serde(default)]`.
- Platform `#[cfg]` attributes live only in `dispatch-os`. Tests may carry `#[cfg(unix)]`.
- `rust-version` stays 1.89: no std API newer than 1.89 (`Option::is_none_or` and let-chains are fine).
- CI must stay green: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` (also with `--target x86_64-pc-windows-gnu` when a task touches Windows-relevant code), and `cargo test --workspace --no-fail-fast`.
- **The character rule**, exactly: a value is 1 to 200 characters, each an ASCII letter, an ASCII digit, or one of `. _ : / @ # + -`, and it does not start with `-`. The empty string is not a value: it means "agent default" and passes nothing.
- A value's wire and saved form: a choice or text as itself, `""` for agent default, a flag as `"true"` or `"false"` (a TOML boolean in the saved file).
- Resolution order: the value chosen, then the saved default, then the harness file's `default`, then unset. A saved `""` stops at step 2 as unset.
- A setting's `args` go after the launch's own arguments, in interactive and one-shot runs, on every platform. A setting's `env` wins over the harness file's `env`.
- The saved file is `harness-settings.toml` in `dispatch_os::paths::config_dir()`, written only through `store::update`. `s` writes only values that differ from the file's default, removes keys set back to it, and writes `""` for agent default over a file default.
- The daemon refuses (never drops) a `SpawnPane` whose settings name an unknown key, a non-option value of a non-custom choice, a flag value other than `true`/`false`, or a value breaking the character rule. The error names the setting.
- Exact user-visible text:
  - picker hint `Enter open · e settings · Esc close`
  - popup hint `Enter open · s save as default · Esc back`
  - popup hint while typing `Enter confirm · Esc cancel`
  - `agent default`, `on`, `off`, `Custom: <text>`
  - status `<Display name> has no settings`, `saved as <Display name>'s default`, `couldn't save: <error>`
  - picker detail separator ` · `
- Comments explain why, in the surrounding voice. Commits are `type(scope): lowercase summary`, ending with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
  ```

## Review Focus

1. **A saved value the harness no longer accepts**, because an option was removed or the file was edited by hand. The harness must still open on its other values and the file's defaults, and the stale value must not be sent. Pinned in Task 2 by `a_saved_value_the_harness_cannot_take_is_ignored`, and in Task 9 by `a_stale_saved_value_is_not_sent_and_the_pane_still_opens`.
2. **A plain `Enter` with nothing saved** must send no settings, so a machine whose harness file differs from this one's still starts the pane. Pinned in Task 9 by `enter_in_the_picker_with_nothing_saved_sends_no_settings`.
3. **A long custom value, or a terminal smaller than the popup**, must clip, not panic or overflow `u16`. Pinned in Task 8 by `a_popup_bigger_than_the_screen_is_clipped_not_a_panic`.
4. **A paste into a value being typed** that carries a newline or shell characters must keep only what a value may hold, and say why. Pinned in Task 8 by `a_paste_keeps_only_what_a_value_may_hold`, and in Task 9 by `a_paste_reaches_a_value_being_typed`.
5. **"Agent default" saved over a file default** (Claude's permissions) must survive the whole trip: saved as `""`, sent as `""`, launched with no permission flag. Pinned in Task 2 by `agent_default_saved_over_a_files_default_stays_agent_default`, and in Task 9 by `agent_default_saved_over_a_file_default_reaches_the_daemon_as_empty`.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/dispatch-config/src/harness.rs` | Task 1: `SettingDef` gains `custom`, `args`, `env`. |
| `crates/dispatch-config/src/settings.rs` + `settings/tests.rs` (new) | Task 1: the character rule, `SettingDef::{check, file_default, is_flag}`, load-time `usable`. Task 2: `Choices`, `HarnessDef::{usable_saved, resolve, launch_with, task_launch_with, describe_changes}`. |
| `crates/dispatch-config/src/harness_settings.rs` + `harness_settings/tests.rs` (new) | Task 3: `harness-settings.toml`: `load`, `load_or_empty`, `save`. |
| `crates/dispatch-config/src/lib.rs` | Tasks 1–3: modules, re-exports; `HarnessRegistry::with` runs `settings::usable`. |
| `crates/dispatch-config/src/testing.rs` | Task 1: `DEMO_SETTINGS`, `demo_harness` test fixtures. |
| `crates/dispatch-config/harnesses/*.toml`, `harnesses/superseded/*`, `src/defaults.rs`, `src/tests.rs` | Task 4: the shipped settings, the superseded bodies, upgrade tests. |
| `crates/dispatch-proto/src/message.rs` + `message/tests.rs` | Task 5: `SpawnPane.settings`. |
| Every `ClientMessage::SpawnPane { .. }` construction | Task 5: `settings: Default::default()`. |
| `crates/dispatch-daemon/src/session.rs` + `session/tests.rs` | Task 6: `settings_dir`, resolving and refusing on spawn, saved settings for delegation. |
| `dispatchd/src/main.rs` | Task 6: give the daemon its settings directory. |
| `crates/dispatch-tui/src/picker.rs` + `picker/tests.rs` | Task 7: `with_hint`, `hint`, `select`. |
| `crates/dispatch-tui/src/settings_form.rs` + `settings_form/tests.rs` (new), `lib.rs` | Task 8: the popup widget. |
| `dispatch/src/app.rs`, `dispatch/src/main.rs` | Task 9: `e`, `Overlay::Settings`, `spawn_pane_with`, saving, picker detail and hint. |
| `README.md`, `docs/security-model.md` | Task 10. |

---

### Task 1: What a setting may be

**Files:**
- Modify: `crates/dispatch-config/src/harness.rs` (the `SettingDef` struct, and the doc on `HarnessDef::settings`)
- Create: `crates/dispatch-config/src/settings.rs`
- Create: `crates/dispatch-config/src/settings/tests.rs`
- Modify: `crates/dispatch-config/src/lib.rs` (module, re-exports, `HarnessRegistry::with`)
- Modify: `crates/dispatch-config/src/testing.rs` (fixtures)

**Interfaces:**
- Consumes: `SettingDef`, `SettingKind`, `HarnessDef`, `HarnessRegistry` as they are today.
- Produces:
  - `pub const SAFE_CHARACTERS: &str`
  - `pub const MAX_VALUE_CHARS: usize = 200`
  - `pub fn is_safe_char(c: char) -> bool`
  - `pub fn is_safe_value(value: &str) -> bool`
  - `SettingDef` fields `pub custom: bool`, `pub args: Vec<String>`, `pub env: BTreeMap<String, String>`
  - `SettingDef::is_flag(&self) -> bool`
  - `SettingDef::file_default(&self) -> String`
  - `SettingDef::check(&self, value: &str) -> Result<(), String>`
  - `pub(crate) fn settings::usable(id: &str, settings: Vec<SettingDef>) -> Vec<SettingDef>`, run by `HarnessRegistry::with`
  - `pub(crate) const settings::VALUE: &str = "{value}"`
  - Test-only: `crate::testing::{DEMO_SETTINGS, demo_harness}`

- [ ] **Step 1: Add the new `SettingDef` fields**

In `crates/dispatch-config/src/harness.rs`, replace the whole `SettingDef` struct (the one with `key`, `label`, `kind`) with:

```rust
/// One configurable setting a harness exposes.
///
/// It says how it becomes a flag: its own `args` and `env`, with `{value}`
/// standing for the value. A harness Dispatch has never seen gets a settings
/// popup from its file alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingDef {
    /// Key the setting is saved and sent under.
    pub key: String,
    /// Human-readable label shown in the settings popup.
    pub label: String,
    /// What the setting accepts.
    #[serde(flatten)]
    pub kind: SettingKind,
    /// For a choice, whether the popup also offers a value the user types.
    #[serde(default)]
    pub custom: bool,
    /// Arguments added after the launch's own. A choice or text puts its
    /// value where `{value}` is and adds nothing when unset; a flag adds
    /// them as written when it is on.
    #[serde(default)]
    pub args: Vec<String>,
    /// Variables set for the child, filled in as `args` is.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}
```

Change the doc on `HarnessDef::settings` from `/// Settings the harness manager offers for this harness.` to `/// Settings the settings popup offers for this harness.`

- [ ] **Step 2: Add the test fixtures**

Append to `crates/dispatch-config/src/testing.rs`:

```rust
/// Settings as a shipped harness has them: a model to choose or type, an
/// effort to choose, and a flag that is on unless turned off.
pub const DEMO_SETTINGS: &str = r#"
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["small", "large"]
custom = true
args = ["--model", "{value}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
options = ["low", "high"]
args = ["--effort", "{value}"]

[[settings]]
key = "bypass"
label = "Skip prompts"
kind = "bool"
default = true
args = ["--yolo"]
"#;

/// A harness `demo` carrying `settings`, a TOML fragment of `[[settings]]`
/// tables (and any other tables), loaded through a registry so it is
/// checked as a harness file is.
///
/// It has a Windows wrapper and a Windows one-shot form that reads its task
/// from a file, as the shipped harnesses do.
pub fn demo_harness(settings: &str) -> crate::HarnessDef {
    let text = format!(
        "id = \"demo\"\ndisplay_name = \"Demo\"\ncommand = \"demo\"\nargs = [\"--tui\"]\n\n\
         [platform.windows]\ncommand = \"cmd.exe\"\nargs = [\"/c\", \"demo\"]\n\n\
         [task]\nargs = [\"-p\", \"{{task}}\"]\n\n\
         [task.platform.windows]\nargs = [\"/d\", \"/c\", \"demo\", \"-p\", \"<%DISPATCH_TASK_FILE%\"]\n\
         input = \"file\"\n\n\
         {settings}"
    );
    let def: crate::HarnessDef = toml::from_str(&text).expect("a valid harness");
    let registry: crate::HarnessRegistry = [def].into_iter().collect();
    registry.get("demo").expect("it is registered").clone()
}
```

- [ ] **Step 3: Write the failing tests**

Create `crates/dispatch-config/src/settings/tests.rs`:

```rust
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
    assert!(model.check("typed-1").is_ok(), "a custom choice takes a typed value");
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
    assert!(bypass.check("").is_err(), "a flag is on or off, never unset");
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
```

- [ ] **Step 4: Run the tests to see them fail**

Run: `cargo test -p dispatch-config settings::tests`
Expected: FAIL to compile. `settings` is not a module yet (`is_safe_value`, `check` and the others do not exist).

- [ ] **Step 5: Write `settings.rs`**

Create `crates/dispatch-config/src/settings.rs`:

```rust
//! A harness's settings: what a value may be, and which of a harness file's
//! settings can be used.
//!
//! A setting says how it becomes a flag -- its own `args` and `env`, with
//! `{value}` standing for the value -- so a harness Dispatch has never seen
//! gets a settings popup from its file alone.

use crate::harness::{SettingDef, SettingKind};

/// What a value may be made of, for telling the user.
pub const SAFE_CHARACTERS: &str = "letters, digits and . _ : / @ # + -, not starting with -";

/// The longest value a setting takes, in characters.
pub const MAX_VALUE_CHARS: usize = 200;

/// Where a setting's `args` and `env` put its value.
pub(crate) const VALUE: &str = "{value}";

/// Whether `c` may appear in a value.
///
/// On Windows every harness runs through `cmd.exe`, which reads its whole
/// command line as shell syntax: a value holding `&`, `|`, `%`, `^` or a
/// quote could run commands. None of these means anything to it, and none
/// can break out of a JSON string either.
#[must_use]
pub fn is_safe_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '@' | '#' | '+' | '-')
}

/// Whether `value` may reach a command line.
///
/// Not empty -- empty is agent default, and passes nothing -- not longer
/// than [`MAX_VALUE_CHARS`], made of [`is_safe_char`] only, and not starting
/// with `-`: a "model" of `--dangerously-skip-permissions` would otherwise
/// add a flag of its own.
#[must_use]
pub fn is_safe_value(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_VALUE_CHARS
        && !value.starts_with('-')
        && value.chars().all(is_safe_char)
}

impl SettingDef {
    /// Whether this is on or off, rather than a value.
    #[must_use]
    pub fn is_flag(&self) -> bool {
        matches!(self.kind, SettingKind::Bool { .. })
    }

    /// The value this setting has when nothing was chosen or saved: the
    /// file's `default`, or agent default. A flag with no default is off.
    #[must_use]
    pub fn file_default(&self) -> String {
        match &self.kind {
            SettingKind::Bool { default } => default.unwrap_or(false).to_string(),
            SettingKind::Choice { default, .. } | SettingKind::Text { default } => {
                default.clone().unwrap_or_default()
            }
        }
    }

    /// Why `value` cannot be this setting's, if it cannot.
    ///
    /// `""` is agent default for a choice or text, and never a flag's: a flag
    /// is on or off.
    pub fn check(&self, value: &str) -> Result<(), String> {
        let key = &self.key;
        let unsafe_value = || format!("{key} takes only {SAFE_CHARACTERS}; {value:?} is refused");

        match &self.kind {
            SettingKind::Bool { .. } => match value {
                "true" | "false" => Ok(()),
                _ => Err(format!(
                    "{key} is on or off, \"true\" or \"false\", not {value:?}"
                )),
            },
            SettingKind::Choice { options, .. } => {
                if value.is_empty() || options.iter().any(|option| option == value) {
                    Ok(())
                } else if !self.custom {
                    Err(format!("{value:?} is not one of {key}'s options"))
                } else if is_safe_value(value) {
                    Ok(())
                } else {
                    Err(unsafe_value())
                }
            }
            SettingKind::Text { .. } => {
                if value.is_empty() || is_safe_value(value) {
                    Ok(())
                } else {
                    Err(unsafe_value())
                }
            }
        }
    }
}

/// The settings of harness `id` that can be used, in file order, each with
/// only its usable options and default.
///
/// Whatever cannot be used is logged and left out, and the harness still
/// loads with the rest, as a bad `[status]` rule costs that rule and not the
/// agent. A setting with no `args` and no `env` passes nothing, so it is not
/// offered: every harness file edited before settings had `args` has some.
pub(crate) fn usable(id: &str, settings: Vec<SettingDef>) -> Vec<SettingDef> {
    let mut kept: Vec<SettingDef> = Vec::new();

    for mut setting in settings {
        let key = setting.key.clone();

        if kept.iter().any(|earlier| earlier.key == key) {
            skip(id, &key, "an earlier setting has the same key");
            continue;
        }

        if setting.args.is_empty() && setting.env.is_empty() {
            tracing::info!(
                harness = id,
                setting = %key,
                "a setting with no args or env does nothing; it is not offered"
            );
            continue;
        }

        let names_value = setting
            .args
            .iter()
            .chain(setting.env.values())
            .any(|template| template.contains(VALUE));

        match &mut setting.kind {
            SettingKind::Bool { .. } => {
                if setting.custom {
                    skip(id, &key, "custom is for a choice");
                    continue;
                }
                if names_value {
                    skip(id, &key, "a flag has no value to fill {value} with");
                    continue;
                }
            }
            SettingKind::Text { default } => {
                if setting.custom {
                    skip(id, &key, "custom is for a choice; text is typed already");
                    continue;
                }
                if default.as_deref().is_some_and(|value| !is_safe_value(value)) {
                    tracing::warn!(
                        harness = id,
                        setting = %key,
                        "dropping a default a command line cannot safely carry"
                    );
                    *default = None;
                }
            }
            SettingKind::Choice { options, default } => {
                options.retain(|option| {
                    let safe = is_safe_value(option);
                    if !safe {
                        tracing::warn!(
                            harness = id,
                            setting = %key,
                            %option,
                            "dropping an option a command line cannot safely carry"
                        );
                    }
                    safe
                });
                if options.is_empty() {
                    skip(id, &key, "a choice with no usable option");
                    continue;
                }

                let custom = setting.custom;
                let fits = default.as_deref().is_none_or(|value| {
                    options.iter().any(|option| option == value)
                        || (custom && is_safe_value(value))
                });
                if !fits {
                    tracing::warn!(
                        harness = id,
                        setting = %key,
                        "dropping a default that is not one of the options"
                    );
                    *default = None;
                }
            }
        }

        kept.push(setting);
    }

    kept
}

/// Logs a setting left out of harness `id`, and why.
fn skip(id: &str, key: &str, reason: &str) {
    tracing::warn!(harness = id, setting = key, reason, "skipping a setting");
}

#[cfg(test)]
mod tests;
```

If the borrow checker objects to reading `setting.custom` inside `match &mut setting.kind` in the `Bool` or `Text` arms, copy it first with `let custom = setting.custom;` above the `match`, as the `Choice` arm already does.

- [ ] **Step 6: Register the module and run `usable` at load**

In `crates/dispatch-config/src/lib.rs`:

1. Add `pub mod settings;` after `pub mod projects;`.
2. Add this re-export after the `pub use harness::{...};` block:

   ```rust
   pub use settings::{MAX_VALUE_CHARS, SAFE_CHARACTERS, is_safe_char, is_safe_value};
   ```

3. Replace the start of `HarnessRegistry::with`:

   ```rust
       /// A registry over `harnesses`, with each one's status rules compiled.
       fn with(harnesses: BTreeMap<String, HarnessDef>) -> Self {
   ```

   with:

   ```rust
       /// A registry over `harnesses`, with each one's settings checked and
       /// its status rules compiled.
       ///
       /// Settings are checked here, once, for files and for definitions
       /// built in memory alike: whatever a launch or the popup reads has
       /// already passed.
       fn with(mut harnesses: BTreeMap<String, HarnessDef>) -> Self {
           for def in harnesses.values_mut() {
               def.settings = settings::usable(&def.id, std::mem::take(&mut def.settings));
           }

   ```

   Leave the rest of the function body (the `let rules = ...` onwards) as it is.

- [ ] **Step 7: Run the tests to see them pass**

Run: `cargo test -p dispatch-config`
Expected: PASS, including the 14 new `settings::tests`. The existing `settings_parse_into_their_kinds` still passes, because it parses with `toml::from_str` and never goes through the registry.

- [ ] **Step 8: Lint and commit**

Run: `cargo fmt --all && cargo clippy -p dispatch-config --all-targets -- -D warnings`
Expected: no warnings.

```bash
git add crates/dispatch-config/src/harness.rs crates/dispatch-config/src/settings.rs crates/dispatch-config/src/settings/tests.rs crates/dispatch-config/src/lib.rs crates/dispatch-config/src/testing.rs
git commit -F - <<'EOF'
feat(config): let a harness setting say how it becomes a flag

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 2: From values to a launch

**Files:**
- Modify: `crates/dispatch-config/src/settings.rs` (append)
- Modify: `crates/dispatch-config/src/settings/tests.rs` (append)
- Modify: `crates/dispatch-config/src/lib.rs` (re-export `Choices`)

**Interfaces:**
- Consumes: Task 1's `SettingDef::{check, file_default, is_flag}`, `VALUE`; `HarnessDef::{launch_for, task_launch_for}`; `Launch`, `TaskRun`.
- Produces:
  - `pub type Choices = BTreeMap<String, String>`, re-exported as `dispatch_config::Choices`
  - `HarnessDef::usable_saved(&self, saved: &Choices) -> Choices`
  - `HarnessDef::resolve(&self, chosen: &Choices, saved: &Choices) -> Result<Choices, String>`. The result has one entry per setting.
  - `HarnessDef::launch_with(&self, os: &str, values: &Choices) -> Launch`
  - `HarnessDef::task_launch_with(&self, os: &str, task: &str, values: &Choices) -> Option<TaskRun>`
  - `HarnessDef::describe_changes(&self, values: &Choices) -> Vec<String>`

- [ ] **Step 1: Write the failing tests**

Append to `crates/dispatch-config/src/settings/tests.rs`:

```rust
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
        def.resolve(&Choices::new(), &saved).expect("nothing chosen"),
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
    assert_eq!(launch.args, vec!["/c", "demo", "--model", "large", "--yolo"]);
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
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch-config settings::tests`
Expected: FAIL to compile: `Choices`, `resolve`, `launch_with` and the rest do not exist yet.

- [ ] **Step 3: Implement**

In `crates/dispatch-config/src/settings.rs`:

1. Replace the `use crate::harness::{SettingDef, SettingKind};` line with:

   ```rust
   use std::collections::BTreeMap;

   use crate::harness::{HarnessDef, Launch, SettingDef, SettingKind, TaskRun};

   /// Each setting's value, by key, as the popup, the saved file and the
   /// protocol carry it: a choice or text as itself, `""` for agent default,
   /// and a flag as `"true"` or `"false"`.
   pub type Choices = BTreeMap<String, String>;
   ```

2. Insert this block just above `#[cfg(test)] mod tests;` at the bottom of the file:

```rust
impl HarnessDef {
    /// The values in `saved` this harness can take.
    ///
    /// A key it has no setting for, or a value that setting cannot take, is
    /// logged and left out: the saved file is the user's, and a stale entry
    /// in it must not stop the harness opening.
    #[must_use]
    pub fn usable_saved(&self, saved: &Choices) -> Choices {
        saved
            .iter()
            .filter(|(key, value)| {
                let Some(setting) = self.settings.iter().find(|setting| &setting.key == *key)
                else {
                    tracing::info!(
                        harness = %self.id,
                        setting = %key,
                        "ignoring a saved value for a setting this harness does not have"
                    );
                    return false;
                };
                match setting.check(value) {
                    Ok(()) => true,
                    Err(reason) => {
                        tracing::warn!(harness = %self.id, %reason, "ignoring a saved value");
                        false
                    }
                }
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    /// Each setting's value for one launch: what was `chosen` for it, else
    /// what was `saved`, else the file's default.
    ///
    /// A `chosen` value this harness cannot take is refused, naming the
    /// setting. It comes from whoever asked for the pane, and dropping it
    /// quietly could start an agent with its prompts off when the user
    /// turned them on. A `saved` one is only left out, as
    /// [`HarnessDef::usable_saved`] says.
    pub fn resolve(&self, chosen: &Choices, saved: &Choices) -> Result<Choices, String> {
        if let Some(key) = chosen
            .keys()
            .find(|key| !self.settings.iter().any(|setting| &setting.key == *key))
        {
            return Err(format!("{} has no setting {key:?}", self.id));
        }

        let saved = self.usable_saved(saved);
        let mut values = Choices::new();
        for setting in &self.settings {
            let value = match chosen.get(&setting.key) {
                Some(value) => {
                    setting
                        .check(value)
                        .map_err(|reason| format!("{}: {reason}", self.id))?;
                    value.clone()
                }
                None => saved
                    .get(&setting.key)
                    .cloned()
                    .unwrap_or_else(|| setting.file_default()),
            };
            values.insert(setting.key.clone(), value);
        }

        Ok(values)
    }

    /// The launch for `os`, with the flags and variables `values` turn on
    /// added after its own.
    #[must_use]
    pub fn launch_with(&self, os: &str, values: &Choices) -> Launch {
        let mut launch = self.launch_for(os);
        self.apply_settings(&mut launch, values);
        launch
    }

    /// The one-shot run of `task` on `os`, with the flags and variables
    /// `values` turn on added after its own.
    ///
    /// A subagent gets them as an interactive pane does: without them, a
    /// one-shot agent that must ask before using a tool cannot ask anyone.
    #[must_use]
    pub fn task_launch_with(&self, os: &str, task: &str, values: &Choices) -> Option<TaskRun> {
        let mut run = self.task_launch_for(os, task)?;
        self.apply_settings(&mut run.launch, values);
        Some(run)
    }

    /// Each value in `values` that differs from the file's default, as the
    /// new-pane picker names it beside the harness: a choice or text as
    /// itself, agent default and flags by their label.
    #[must_use]
    pub fn describe_changes(&self, values: &Choices) -> Vec<String> {
        self.settings
            .iter()
            .filter_map(|setting| {
                let value = values.get(&setting.key)?;
                if *value == setting.file_default() {
                    return None;
                }
                Some(if setting.is_flag() {
                    let state = if value == "true" { "on" } else { "off" };
                    format!("{} {state}", setting.label)
                } else if value.is_empty() {
                    format!("{} agent default", setting.label)
                } else {
                    value.clone()
                })
            })
            .collect()
    }

    /// Adds to `launch` what each setting's value in `values` turns on.
    fn apply_settings(&self, launch: &mut Launch, values: &Choices) {
        for setting in &self.settings {
            let value = values.get(&setting.key).map_or("", String::as_str);

            // Checked again, however `values` was come by: what is added
            // lands on a command line cmd.exe parses.
            if let Err(reason) = setting.check(value) {
                tracing::warn!(harness = %self.id, %reason, "leaving out a setting");
                continue;
            }

            let on = if setting.is_flag() {
                value == "true"
            } else {
                !value.is_empty()
            };
            if !on {
                continue;
            }

            launch
                .args
                .extend(setting.args.iter().map(|arg| arg.replace(VALUE, value)));
            for (name, template) in &setting.env {
                launch
                    .env
                    .insert(name.clone(), template.replace(VALUE, value));
            }
        }
    }
}
```

3. In `crates/dispatch-config/src/lib.rs`, change the Task 1 re-export to:

   ```rust
   pub use settings::{Choices, MAX_VALUE_CHARS, SAFE_CHARACTERS, is_safe_char, is_safe_value};
   ```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p dispatch-config`
Expected: PASS, with 14 more `settings::tests`.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy -p dispatch-config --all-targets -- -D warnings`

```bash
git add crates/dispatch-config/src/settings.rs crates/dispatch-config/src/settings/tests.rs crates/dispatch-config/src/lib.rs
git commit -F - <<'EOF'
feat(config): resolve a harness's settings and build its launch from them

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 3: The saved defaults file

**Files:**
- Create: `crates/dispatch-config/src/harness_settings.rs`
- Create: `crates/dispatch-config/src/harness_settings/tests.rs`
- Modify: `crates/dispatch-config/src/lib.rs` (`pub mod harness_settings;`)

**Interfaces:**
- Consumes: `crate::store::{read, update}` (both `pub(crate)`), Task 2's `Choices` and `SettingDef::{check, file_default, is_flag}`, `ConfigError`.
- Produces:
  - `dispatch_config::harness_settings::load(dir: &Path, harness: &str) -> Result<Choices, ConfigError>`
  - `dispatch_config::harness_settings::load_or_empty(dir: &Path, harness: &str) -> Choices`
  - `dispatch_config::harness_settings::save(dir: &Path, def: &HarnessDef, values: &Choices) -> Result<(), ConfigError>`
  - `pub const FILE: &str = "harness-settings.toml"`

- [ ] **Step 1: Write the failing tests**

Create `crates/dispatch-config/src/harness_settings/tests.rs`:

```rust
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

    assert_eq!(load(dir.path(), "demo").expect("no file is fine"), Choices::new());
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
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch-config harness_settings`
Expected: FAIL to compile: there is no `harness_settings` module.

- [ ] **Step 3: Implement**

Create `crates/dispatch-config/src/harness_settings.rs`:

```rust
//! The defaults a user saved from the settings popup.
//!
//! A file of its own, beside `projects.toml`: not `config.toml`, which is
//! written by hand and would lose its comments to a rewrite, and not the
//! harness files, which an edit would cut off from upgrades.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Choices, ConfigError, HarnessDef};

/// The file, inside the configuration directory.
pub const FILE: &str = "harness-settings.toml";

/// One saved value: a flag as a TOML boolean, anything else as a string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum Stored {
    Flag(bool),
    Text(String),
}

impl Stored {
    /// The value as [`Choices`] carries it.
    fn to_choice(&self) -> String {
        match self {
            Stored::Flag(on) => on.to_string(),
            Stored::Text(text) => text.clone(),
        }
    }
}

/// The file's shape: each harness's saved values, by harness id and then by
/// setting key.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
struct Saved(BTreeMap<String, BTreeMap<String, Stored>>);

/// What the user saved for `harness`. No file is nothing saved.
pub fn load(dir: &Path, harness: &str) -> Result<Choices, ConfigError> {
    let saved: Saved = crate::store::read(dir, FILE)?;
    Ok(saved
        .0
        .get(harness)
        .map(|values| {
            values
                .iter()
                .map(|(key, value)| (key.clone(), value.to_choice()))
                .collect()
        })
        .unwrap_or_default())
}

/// [`load`], with a file that cannot be read taken as nothing saved and
/// logged: opening a pane must not stop over it.
#[must_use]
pub fn load_or_empty(dir: &Path, harness: &str) -> Choices {
    load(dir, harness).unwrap_or_else(|error| {
        tracing::warn!(%error, "ignoring saved harness settings that cannot be read");
        Choices::new()
    })
}

/// Saves `values` as `def`'s defaults.
///
/// Only what differs from the harness file's own default is written, and a
/// value equal to it removes its key: a default Dispatch later ships then
/// reaches every setting the user never changed. A key `def` has no setting
/// for, or a value that setting cannot take, is not written. A file that
/// cannot be read is refused, not overwritten: the user may want it back.
pub fn save(dir: &Path, def: &HarnessDef, values: &Choices) -> Result<(), ConfigError> {
    crate::store::update(dir, FILE, |saved: &mut Saved| {
        let before = saved.0.get(&def.id).cloned();
        let table = saved.0.entry(def.id.clone()).or_default();

        for setting in &def.settings {
            let Some(value) = values.get(&setting.key) else {
                continue;
            };
            if setting.check(value).is_err() {
                continue;
            }

            if *value == setting.file_default() {
                table.remove(&setting.key);
            } else if setting.is_flag() {
                table.insert(setting.key.clone(), Stored::Flag(value == "true"));
            } else {
                table.insert(setting.key.clone(), Stored::Text(value.clone()));
            }
        }

        if table.is_empty() {
            saved.0.remove(&def.id);
        }
        let changed = saved.0.get(&def.id).cloned() != before;
        Ok(((), changed))
    })
}

#[cfg(test)]
mod tests;
```

In `crates/dispatch-config/src/lib.rs`, add `pub mod harness_settings;` after `pub mod harness;`.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p dispatch-config harness_settings`
Expected: PASS (8 tests).

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy -p dispatch-config --all-targets -- -D warnings`

```bash
git add crates/dispatch-config/src/harness_settings.rs crates/dispatch-config/src/harness_settings/tests.rs crates/dispatch-config/src/lib.rs
git commit -F - <<'EOF'
feat(config): keep the settings a user saves in harness-settings.toml

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 4: The shipped harness files

**Files:**
- Create (by copying): `crates/dispatch-config/harnesses/superseded/claude-6.toml`, `codex-6.toml`, `agy-1.toml`, `opencode-1.toml`
- Modify: `crates/dispatch-config/harnesses/claude.toml`, `codex.toml`, `agy.toml`, `opencode.toml`
- Modify: `crates/dispatch-config/src/defaults.rs`
- Modify: `crates/dispatch-config/src/tests.rs`

**Interfaces:**
- Consumes: Task 2's `resolve`, `launch_with`, `task_launch_with`, `Choices`.
- Produces: the shipped settings. Keys are `model`, `effort` and `permissions` for claude; `model`, `effort` and `bypass` for codex and agy; `model` and `bypass` for opencode. Later tasks and the docs rely on these keys and labels.

- [ ] **Step 1: Keep today's bodies as superseded**

The shipped files hold icon glyphs from a private-use area, so they are copied byte for byte, never retyped:

```bash
cd crates/dispatch-config/harnesses
cp claude.toml superseded/claude-6.toml
cp codex.toml superseded/codex-6.toml
cp agy.toml superseded/agy-1.toml
cp opencode.toml superseded/opencode-1.toml
cd -
```

- [ ] **Step 2: Write the failing tests**

In `crates/dispatch-config/src/tests.rs`, replace the whole `every_body_an_earlier_dispatch_wrote_is_upgraded` test with:

```rust
#[test]
fn every_body_an_earlier_dispatch_wrote_is_upgraded() {
    // Byte for byte what each release wrote, oldest first: an installation
    // made at any of them and never edited has one of these, and the unsafe
    // Windows form in all but the first.
    let written_before: Vec<(&str, Vec<&str>)> = vec![
        (
            "claude",
            vec![
                include_str!("../harnesses/superseded/claude-1.toml"),
                include_str!("../harnesses/superseded/claude-2.toml"),
                include_str!("../harnesses/superseded/claude-3.toml"),
                include_str!("../harnesses/superseded/claude-4.toml"),
                include_str!("../harnesses/superseded/claude-5.toml"),
                include_str!("../harnesses/superseded/claude-6.toml"),
            ],
        ),
        (
            "codex",
            vec![
                include_str!("../harnesses/superseded/codex-1.toml"),
                include_str!("../harnesses/superseded/codex-2.toml"),
                include_str!("../harnesses/superseded/codex-3.toml"),
                include_str!("../harnesses/superseded/codex-4.toml"),
                include_str!("../harnesses/superseded/codex-5.toml"),
                include_str!("../harnesses/superseded/codex-6.toml"),
            ],
        ),
        (
            "agy",
            vec![include_str!("../harnesses/superseded/agy-1.toml")],
        ),
        (
            "opencode",
            vec![include_str!("../harnesses/superseded/opencode-1.toml")],
        ),
    ];

    for (id, bodies) in written_before {
        let current = defaults::BUILT_INS
            .iter()
            .find(|b| b.id == id)
            .expect("it ships")
            .toml;
        for (release, body) in bodies.iter().enumerate() {
            let dir = TempDir::new("upgrade-every-body");
            dir.write(&format!("{id}.toml"), body);

            let written = write_missing_built_ins(dir.path()).expect("writing succeeds");

            assert!(
                written.contains(&id),
                "{id}-{} was not recognised: {written:?}",
                release + 1
            );
            assert_eq!(
                std::fs::read_to_string(dir.path().join(format!("{id}.toml"))).expect("it reads"),
                current,
                "{id}-{}",
                release + 1
            );
        }
    }
}
```

Append these tests to the same file:

```rust
/// The built-ins, written and loaded as a first run does.
fn shipped() -> (TempDir, HarnessRegistry) {
    let dir = TempDir::new("shipped-settings");
    write_missing_built_ins(dir.path()).expect("the built-ins are written");
    let registry = HarnessRegistry::load_from_dir(dir.path()).expect("they load");
    (dir, registry)
}

#[test]
fn every_built_in_skips_permission_prompts_by_default() {
    let (_dir, registry) = shipped();

    for (id, flags) in [
        ("claude", vec!["--permission-mode", "bypassPermissions"]),
        ("codex", vec!["--dangerously-bypass-approvals-and-sandbox"]),
        ("agy", vec!["--dangerously-skip-permissions"]),
        ("opencode", vec!["--auto"]),
    ] {
        let def = registry.get(id).expect("it ships");
        let values = def
            .resolve(&Choices::new(), &Choices::new())
            .expect("nothing chosen is nothing to refuse");

        assert_eq!(def.launch_with("linux", &values).args, flags, "{id}");

        let windows = def.launch_with("windows", &values).args;
        assert_eq!(windows[..2], ["/c", id], "{id}");
        assert_eq!(windows[2..], flags[..], "{id}");
    }
}

#[test]
fn built_in_models_and_efforts_ship_unset() {
    // Each agent keeps its own default, and whatever its own configuration
    // says, until the user saves one.
    let (_dir, registry) = shipped();

    for def in registry.all().filter(|def| def.id != SHELL) {
        let values = def
            .resolve(&Choices::new(), &Choices::new())
            .expect("nothing chosen");
        for key in ["model", "effort"] {
            if let Some(value) = values.get(key) {
                assert_eq!(value, "", "{}'s {key}", def.id);
            }
        }
        assert!(values.contains_key("model"), "{} offers a model", def.id);
    }
}

#[test]
fn no_built_in_setting_or_option_is_dropped_on_load() {
    // A shipped option the character rule refused would vanish quietly from
    // the popup.
    let (_dir, registry) = shipped();

    for built_in in defaults::BUILT_INS {
        let raw: HarnessDef = toml::from_str(built_in.toml).expect("it parses");
        assert!(!raw.settings.is_empty(), "{} ships settings", built_in.id);
        assert_eq!(
            registry.get(built_in.id).expect("it loads").settings,
            raw.settings,
            "{}",
            built_in.id
        );
    }
}

#[test]
fn a_delegated_built_in_skips_its_prompts_too() {
    // A one-shot agent that must ask before using a tool has nobody to ask.
    let (_dir, registry) = shipped();

    for (id, expected) in [
        (
            "claude",
            vec!["-p", "x", "--permission-mode", "bypassPermissions"],
        ),
        (
            "codex",
            vec!["exec", "x", "--dangerously-bypass-approvals-and-sandbox"],
        ),
    ] {
        let def = registry.get(id).expect("it ships");
        let values = def
            .resolve(&Choices::new(), &Choices::new())
            .expect("nothing chosen");
        let run = def
            .task_launch_with("linux", "x", &values)
            .expect("it can be delegated to");
        assert_eq!(run.launch.args, expected, "{id}");
    }

    let claude = registry.get("claude").expect("it ships");
    let values = claude
        .resolve(&Choices::new(), &Choices::new())
        .expect("nothing chosen");
    let windows = claude
        .task_launch_with("windows", "x", &values)
        .expect("it can be delegated to on Windows");
    assert_eq!(
        windows.launch.args[windows.launch.args.len() - 3..],
        [
            "<%DISPATCH_TASK_FILE%",
            "--permission-mode",
            "bypassPermissions"
        ]
    );
}
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -p dispatch-config`
Expected: FAIL. `every_body_an_earlier_dispatch_wrote_is_upgraded` fails because `claude-6` is not in the superseded list yet. The four new tests fail because the shipped files have no `args` on their settings.

- [ ] **Step 4: Write the new shipped settings**

In `crates/dispatch-config/harnesses/claude.toml`, delete everything from the first `[[settings]]` line to the end of the file, and put this in its place:

```toml
# Each setting's args go after the launch's own, in a one-shot run too, with
# {value} replaced by what was chosen. Unset passes nothing and leaves the
# choice to Claude. `e` on a harness in the new-pane picker changes them.
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["opus", "sonnet", "fable", "haiku"]
custom = true
args = ["--model", "{value}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
options = ["low", "medium", "high", "xhigh", "max"]
args = ["--effort", "{value}"]

[[settings]]
key = "permissions"
label = "Permissions"
kind = "choice"
options = ["bypassPermissions", "auto", "acceptEdits", "plan", "manual"]
default = "bypassPermissions"
args = ["--permission-mode", "{value}"]
```

In `crates/dispatch-config/harnesses/codex.toml`, delete everything from the first `[[settings]]` line to the end of the file, and put this in its place:

```toml
# See claude.toml for how settings become flags.
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["gpt-6-astra", "gpt-6-sol", "gpt-6-luna", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna", "gpt-5.5"]
custom = true
args = ["-m", "{value}"]

# -c parses its value as TOML and falls back to the raw string, so the level
# needs no quotes, which would sit on cmd.exe's command line on Windows.
[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
options = ["low", "medium", "high", "xhigh", "max", "ultra"]
args = ["-c", "model_reasoning_effort={value}"]

# Codex's only way to skip every prompt also drops its sandbox.
[[settings]]
key = "bypass"
label = "Skip prompts"
kind = "bool"
default = true
args = ["--dangerously-bypass-approvals-and-sandbox"]
```

Append to `crates/dispatch-config/harnesses/agy.toml` (after a blank line):

```toml

# Each setting's args go after the launch's own, with {value} replaced by
# what was chosen. Unset passes nothing and leaves the choice to agy.
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = [
  "gemini-3.8-flash-high", "gemini-3.8-flash-medium", "gemini-3.8-flash-low",
  "gemini-3.7-flash-high", "gemini-3.7-flash-medium", "gemini-3.7-flash-low",
  "gemini-3.6-flash-high", "gemini-3.6-flash-medium", "gemini-3.6-flash-low",
  "gemini-3.1-pro-high", "gemini-3.1-pro-low",
  "claude-sonnet-4-6", "claude-opus-4-6-thinking", "gpt-oss-120b-medium",
]
custom = true
args = ["--model", "{value}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
options = ["low", "medium", "high"]
args = ["--effort", "{value}"]

[[settings]]
key = "bypass"
label = "Skip prompts"
kind = "bool"
default = true
args = ["--dangerously-skip-permissions"]
```

Append to `crates/dispatch-config/harnesses/opencode.toml`, and delete its existing `[[settings]]` block first (the one with `key = "model"` and `kind = "text"` and no args):

```toml
# opencode's interactive mode has no model flag; its inline configuration
# variable carries one instead. It knows about two hundred models, too many
# to ship, so the model is typed, as provider/model with an optional
# #variant.
[[settings]]
key = "model"
label = "Model"
kind = "text"
env = { OPENCODE_CONFIG_CONTENT = '{"model":"{value}"}' }

[[settings]]
key = "bypass"
label = "Auto-approve"
kind = "bool"
default = true
args = ["--auto"]
```

- [ ] **Step 5: List the superseded bodies**

In `crates/dispatch-config/src/defaults.rs`:
- add `include_str!("../harnesses/superseded/claude-6.toml"),` after the `claude-5` line;
- add `include_str!("../harnesses/superseded/codex-6.toml"),` after the `codex-5` line;
- change `agy`'s `superseded: &[],` to `superseded: &[include_str!("../harnesses/superseded/agy-1.toml")],`;
- change `opencode`'s `superseded: &[],` to `superseded: &[include_str!("../harnesses/superseded/opencode-1.toml")],`.

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test -p dispatch-config`
Expected: PASS. The existing Windows task-form tests (`a_windows_task_reaches_the_agent_on_standard_input` and the others) are unchanged: they call `task_launch_for`, which adds no settings.

- [ ] **Step 7: Lint and commit**

Run: `cargo fmt --all && cargo clippy -p dispatch-config --all-targets -- -D warnings`

```bash
git add crates/dispatch-config/harnesses crates/dispatch-config/src/defaults.rs crates/dispatch-config/src/tests.rs
git commit -F - <<'EOF'
feat(config): ship every harness without permission prompts, with model and effort settings

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 5: Settings travel with a spawn

**Files:**
- Modify: `crates/dispatch-proto/src/message.rs`
- Modify: `crates/dispatch-proto/src/message/tests.rs`
- Modify (a field added to each construction): `crates/dispatch-daemon/src/session.rs`, `crates/dispatch-daemon/src/session/tests.rs`, `dispatchd/tests/serves_clients.rs`, `dispatch/src/app.rs`, `dispatch/tests/delegate_shim.rs`

**Interfaces:**
- Produces: `ClientMessage::SpawnPane { project, harness, size, place, settings: BTreeMap<String, String> }`, with `settings` as `#[serde(default)]`. Task 6 reads it in the daemon; Task 9 fills it in the app.

- [ ] **Step 1: Write the failing test**

Append to `crates/dispatch-proto/src/message/tests.rs`:

```rust
#[test]
fn a_spawns_settings_survive_the_wire() {
    let message = ClientMessage::SpawnPane {
        project: ProjectId::new(),
        harness: "claude".into(),
        size: (80, 24),
        place: Placement::Auto,
        settings: [
            ("model".to_string(), "opus".to_string()),
            ("permissions".to_string(), String::new()),
            ("bypass".to_string(), "false".to_string()),
        ]
        .into_iter()
        .collect(),
    };

    let mut buf = Vec::new();
    Frame::write(&mut buf, &message).expect("writing succeeds");
    let read: ClientMessage = Frame::read(&mut buf.as_slice()).expect("reading succeeds");

    assert_eq!(read, message);
}
```

In the same file, in the test that writes `Older::SpawnPane` (the one expecting `place: Placement::Auto`), add `settings: Default::default(),` to the expected `ClientMessage::SpawnPane { .. }` after `place: Placement::Auto,`. That now also pins "an older client sends none". Leave the test-local `Older` and `Newer` enums alone: they stand for other builds' messages.

In the long list of messages near the top of the file (the `ClientMessage::SpawnPane` with `place: Placement::Into { tab: TabId::new() }`), add:

```rust
            settings: [("model".to_string(), "opus".to_string())]
                .into_iter()
                .collect(),
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch-proto`
Expected: FAIL to compile: `SpawnPane` has no field named `settings`.

- [ ] **Step 3: Add the field**

In `crates/dispatch-proto/src/message.rs`, change `use std::path::PathBuf;` to:

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;
```

and add a field to `ClientMessage::SpawnPane` after `place`:

```rust
        /// Each setting's value the user chose or saved, by key: a choice
        /// or text as itself, `""` for agent default, a flag as `"true"` or
        /// `"false"`. A setting left out is the daemon's to decide, from its
        /// own saved defaults and then the harness file. An older client
        /// sends none.
        #[serde(default)]
        settings: BTreeMap<String, String>,
```

- [ ] **Step 4: Give every other construction the field**

Run: `cargo build --workspace --all-targets 2>&1 | grep -B2 -A8 "missing field .settings."`

For every `ClientMessage::SpawnPane { ... }` *construction* the compiler reports (error E0063, missing `settings`), add `settings: Default::default(),` after `place: ...,`. The reports are in `crates/dispatch-daemon/src/session/tests.rs` (about 29, including the `spawn_pane_for_test` helper), `dispatchd/tests/serves_clients.rs`, `dispatch/tests/delegate_shim.rs`, and the one in `dispatch/src/app.rs`'s `spawn_pane`.

The daemon's own *pattern* in `crates/dispatch-daemon/src/session.rs`, in `ClientMessage::SpawnPane { project, harness, size, place } => ...`, is error E0027. Change it to:

```rust
            ClientMessage::SpawnPane {
                project,
                harness,
                size,
                place,
                settings: _,
            } => self.spawn_pane(id, project, &harness, Size::new(size.0, size.1), place),
```

Task 6 puts the settings to use. Patterns that already end in `..` (such as `placed` in the app's tests) need nothing. Repeat the build until it is clean.

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test --workspace --no-fail-fast`
Expected: PASS everywhere, with 1 new test in `dispatch-proto`.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add -A crates dispatch dispatchd
git commit -F - <<'EOF'
feat(proto): carry the settings a user chose with a spawn

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 6: The daemon starts panes and subagents with settings

**Files:**
- Modify: `crates/dispatch-daemon/src/session.rs`
- Modify: `crates/dispatch-daemon/src/session/tests.rs`
- Modify: `dispatchd/src/main.rs`

**Interfaces:**
- Consumes: Task 2's `HarnessDef::{resolve, launch_with, task_launch_with}` and `Choices`; Task 3's `harness_settings::load_or_empty`; Task 5's `SpawnPane.settings`.
- Produces: `Daemon::set_settings_dir(&mut self, dir: PathBuf)`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/dispatch-daemon/src/session/tests.rs`:

```rust
/// Writes `record`: a harness that writes the arguments it was started
/// with to `out`, one to a line, and then waits. `$0` is the file, so what
/// it writes is exactly what its settings added. Its one-shot form writes
/// them and exits.
#[cfg(unix)]
fn write_recording_harness(dir: &std::path::Path, out: &std::path::Path) {
    let body = format!(
        r#"id = "record"
display_name = "Record"
command = "sh"
args = ["-c", "printf '%s\n' \"$@\" > \"$0\"; sleep 30", "{out}"]

[task]
args = ["-c", "printf '%s\n' \"$@\" > \"$0\"", "{out}"]

[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["small", "large"]
custom = true
args = ["--model", "{{value}}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
options = ["low", "high"]
args = ["--effort", "{{value}}"]

[[settings]]
key = "bypass"
label = "Skip prompts"
kind = "bool"
default = true
args = ["--yolo"]
"#,
        out = out.display()
    );
    std::fs::create_dir_all(dir).expect("temp dir is writable");
    std::fs::write(dir.join("record.toml"), body).expect("temp dir is writable");
}

/// A daemon serving `record` beside the usual harnesses, reading saved
/// settings from `dir/config`, plus the file `record` writes to.
#[cfg(unix)]
fn recording_daemon(label: &str) -> (Daemon, ProjectId, TempDir, PathBuf) {
    let dir = TempDir::new(label);
    let harness_dir = dir.0.join("harnesses");
    let out = dir.0.join("record.out");
    write_recording_harness(&harness_dir, &out);
    let registry = harnesses(&harness_dir);

    let mut daemon = Daemon::new(registry, "test-device");
    daemon.set_task_dir(dir.0.join("tasks"));
    daemon.set_settings_dir(dir.0.join("config"));
    let root = dispatch_os::paths::resolve(&dir.0).expect("the temp dir resolves");
    let project = daemon.open_project(root);

    (daemon, project, dir, out)
}

/// Saves `text` as the daemon's `harness-settings.toml`.
#[cfg(unix)]
fn save_settings(dir: &TempDir, text: &str) {
    let config = dir.0.join("config");
    std::fs::create_dir_all(&config).expect("temp dir is writable");
    std::fs::write(config.join("harness-settings.toml"), text).expect("temp dir is writable");
}

/// What `record` wrote, once it has: one argument to a line.
#[cfg(unix)]
fn recorded(daemon: &mut Daemon, out: &std::path::Path) -> Vec<String> {
    let deadline = Instant::now() + WAIT_FOR_DEADLINE;
    loop {
        daemon.tick();
        if let Ok(text) = std::fs::read_to_string(out)
            && text.ends_with('\n')
        {
            return text.lines().map(str::to_string).collect();
        }
        assert!(
            Instant::now() < deadline,
            "record never wrote its arguments"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Asks for a pane of `harness` with `settings`, as a client does.
fn spawn_with(daemon: &mut Daemon, project: ProjectId, harness: &str, settings: &[(&str, &str)]) {
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: harness.into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: settings
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        },
    );
}

/// The text of the first `Other` error among `messages`.
fn error_text(messages: &[ServerMessage]) -> Option<String> {
    messages.iter().find_map(|m| match m {
        ServerMessage::Error {
            error: ProtocolError::Other(text),
        } => Some(text.clone()),
        _ => None,
    })
}

#[cfg(unix)]
#[test]
fn a_spawn_starts_with_the_flags_the_client_chose() {
    let (mut daemon, project, _dir, out) = recording_daemon("settings-chosen");
    let _inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());

    spawn_with(
        &mut daemon,
        project,
        "record",
        &[("model", "large"), ("bypass", "false")],
    );

    assert_eq!(recorded(&mut daemon, &out), vec!["--model", "large"]);
}

#[cfg(unix)]
#[test]
fn a_spawn_with_nothing_chosen_uses_the_saved_settings_then_the_files() {
    let (mut daemon, project, dir, out) = recording_daemon("settings-saved");
    save_settings(&dir, "[record]\nmodel = \"small\"\n");
    let _inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());

    spawn_with(&mut daemon, project, "record", &[]);

    assert_eq!(
        recorded(&mut daemon, &out),
        vec!["--model", "small", "--yolo"]
    );
}

#[cfg(unix)]
#[test]
fn a_chosen_value_beats_a_saved_one() {
    let (mut daemon, project, dir, out) = recording_daemon("settings-chosen-over-saved");
    save_settings(&dir, "[record]\nmodel = \"small\"\n");
    let _inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());

    spawn_with(&mut daemon, project, "record", &[("model", "large")]);

    assert_eq!(
        recorded(&mut daemon, &out),
        vec!["--model", "large", "--yolo"]
    );
}

#[test]
fn a_setting_the_harness_does_not_have_is_refused_and_nothing_starts() {
    let (mut daemon, project, _dir) = daemon("settings-unknown");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    let _ = drain(&inbox);

    spawn_with(&mut daemon, project, "shell", &[("colour", "red")]);

    let text = error_text(&drain(&inbox)).expect("the spawn is refused");
    assert!(text.contains("colour"), "{text}");
    assert_eq!(daemon.pane_count(), 0);
}

#[cfg(unix)]
#[test]
fn a_value_the_harness_cannot_take_is_refused_and_nothing_starts() {
    // Refused, not dropped: a dropped `bypass = false` would start an agent
    // with its prompts off.
    for (key, value) in [
        ("model", "x&calc"),
        ("model", "--yolo"),
        ("effort", "extreme"),
        ("bypass", "yes"),
    ] {
        let (mut daemon, project, _dir, _out) = recording_daemon("settings-refused");
        let inbox = daemon.attach_for_test(1);
        daemon.request_for_test(1, hello());
        let _ = drain(&inbox);

        spawn_with(&mut daemon, project, "record", &[(key, value)]);

        let text = error_text(&drain(&inbox))
            .unwrap_or_else(|| panic!("{key} = {value:?} was not refused"));
        assert!(text.contains(key), "{key} = {value:?}: {text}");
        assert_eq!(daemon.pane_count(), 0, "{key} = {value:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_subagent_starts_with_the_saved_settings() {
    let (mut daemon, project, dir, out) = recording_daemon("settings-delegated");
    save_settings(&dir, "[record]\nmodel = \"large\"\n");
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
            harness: "record".into(),
            task: "anything".into(),
            size: (80, 24),
        },
    );
    let request = pending(&drain(&ui)).expect("the interface is asked");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );
    wait_for(&mut daemon, &caller, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. }))
    });

    assert_eq!(
        recorded(&mut daemon, &out),
        vec!["--model", "large", "--yolo"]
    );
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch-daemon settings`
Expected: FAIL to compile: `set_settings_dir` does not exist.

- [ ] **Step 3: Implement**

In `crates/dispatch-daemon/src/session.rs`:

1. Import `Choices` where `dispatch_config` is imported (add it to the existing `use dispatch_config::{...}` list, or add `use dispatch_config::Choices;`).

2. Add a field to `pub struct Daemon`, after `task_dir_lock`:

   ```rust
       /// Where the user's saved harness settings live, when this daemon
       /// reads them: `None` until it is told, so no test reads the
       /// developer's own.
       settings_dir: Option<PathBuf>,
   ```

   and `settings_dir: None,` in `with_limits`'s `Self { .. }`, after `task_dir_lock: None,`.

3. Add after `set_task_dir`:

   ```rust
       /// Reads the user's saved harness settings from `dir`.
       ///
       /// Before `serve`, which consumes the daemon.
       pub fn set_settings_dir(&mut self, dir: PathBuf) {
           self.settings_dir = Some(dir);
       }

       /// What the user saved for `harness`, read fresh so a save in any
       /// client takes effect at once. Nothing when this daemon was told of
       /// no directory, or the file cannot be read.
       fn saved_settings(&self, harness: &str) -> Choices {
           self.settings_dir
               .as_deref()
               .map(|dir| dispatch_config::harness_settings::load_or_empty(dir, harness))
               .unwrap_or_default()
       }
   ```

4. Replace the Task 5 match arm with:

   ```rust
               ClientMessage::SpawnPane {
                   project,
                   harness,
                   size,
                   place,
                   settings,
               } => self.spawn_pane(
                   id,
                   project,
                   &harness,
                   &settings,
                   Size::new(size.0, size.1),
                   place,
               ),
   ```

5. Change `fn spawn_pane`'s parameters to take `settings: &Choices` after `harness: &str`:

   ```rust
       fn spawn_pane(
           &mut self,
           client: ClientId,
           project: ProjectId,
           harness: &str,
           settings: &Choices,
           size: Size,
           place: Placement,
       ) {
   ```

   Then replace `let mut launch = def.launch_for_current_platform();` with:

   ```rust
           // The client sends only what the user chose or saved; the rest
           // is this machine's to decide. A value this harness cannot take
           // is refused, never dropped: a dropped `bypass = false` would
           // start an agent with its prompts off.
           let values = match def.resolve(settings, &self.saved_settings(harness)) {
               Ok(values) => values,
               Err(reason) => {
                   self.send(
                       client,
                       ServerMessage::Error {
                           error: ProtocolError::Other(format!("not starting {harness}: {reason}")),
                       },
                   );
                   return;
               }
           };
           let mut launch = def.launch_with(std::env::consts::OS, &values);
   ```

   If the borrow checker objects because `def` borrows `self.harnesses` across `self.send`, clone first: `let def = def.clone();` straight after the `let Some(def) = ... else { ... };` lookup.

6. In `fn task_run`, replace its first line, `let mut run = self.harnesses.get(harness)?.task_launch(task)?;`, with:

   ```rust
           let def = self.harnesses.get(harness)?;
           // No client chose anything for a subagent: it gets what the user
           // saved, then the file's defaults. Nothing chosen is nothing to
           // refuse, so this cannot fail.
           let values = def
               .resolve(&Choices::new(), &self.saved_settings(harness))
               .unwrap_or_default();
           let mut run = def.task_launch_with(std::env::consts::OS, task, &values)?;
   ```

In `dispatchd/src/main.rs`, straight after `let mut daemon = Daemon::with_limits(...);`, add:

```rust
    // Saved from the settings popup, and read at every spawn and delegation
    // so a save in any client takes effect at once.
    daemon.set_settings_dir(
        dispatch_os::paths::config_dir().context("failed to locate the configuration directory")?,
    );
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p dispatch-daemon && cargo test -p dispatchd`
Expected: PASS, including the 6 new tests (1 of them on every platform, 5 on Unix).

- [ ] **Step 5: Lint, including for Windows, and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy -p dispatch-daemon --all-targets --target x86_64-pc-windows-gnu -- -D warnings`
Expected: no warnings. If the Windows target is not installed locally, say so in the report rather than skip silently.

```bash
git add crates/dispatch-daemon/src/session.rs crates/dispatch-daemon/src/session/tests.rs dispatchd/src/main.rs
git commit -F - <<'EOF'
feat(daemon): start panes and subagents with the settings chosen or saved

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 7: The picker's hint and select-by-id

**Files:**
- Modify: `crates/dispatch-tui/src/picker.rs`
- Modify: `crates/dispatch-tui/src/picker/tests.rs`

**Interfaces:**
- Produces:
  - `Picker::with_hint(self, hint: impl Into<String>) -> Self`
  - `Picker::hint(&self) -> Option<&str>`
  - `Picker::select(&mut self, id: &str)`

- [ ] **Step 1: Write the failing tests**

Append to `crates/dispatch-tui/src/picker/tests.rs`:

```rust
#[test]
fn a_hint_is_drawn_on_the_bottom_border() {
    let picker = picker().with_hint("Enter open · e settings");
    let text = text(&render(&picker, 60, 10));

    let line = text
        .lines()
        .find(|line| line.contains("Enter open · e settings"))
        .expect("the hint is drawn");
    assert!(line.contains('└'), "on the bottom border: {line:?}");
    assert_eq!(picker.hint(), Some("Enter open · e settings"));
}

#[test]
fn a_hint_widens_a_picker_to_fit() {
    let picker = Picker::new("P", vec![Item::new("a", "a")])
        .with_hint("a hint much wider than anything in the list");

    assert!(
        text(&render(&picker, 80, 10)).contains("a hint much wider than anything in the list")
    );
}

#[test]
fn a_picker_has_no_hint_unless_given_one() {
    assert_eq!(picker().hint(), None);
}

#[test]
fn selecting_by_id_moves_the_highlight() {
    let mut picker = picker();

    picker.select("agy");
    assert_eq!(picker.selected().expect("an item").id, "agy");

    picker.select("nothing-by-that-name");
    assert_eq!(
        picker.selected().expect("an item").id,
        "agy",
        "an unknown id leaves it where it was"
    );
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch-tui picker`
Expected: FAIL to compile: `with_hint`, `hint` and `select` do not exist.

- [ ] **Step 3: Implement**

In `crates/dispatch-tui/src/picker.rs`:

1. Add `use ratatui::text::Line;` to the imports.
2. Add a field to `Picker` after `border`:

   ```rust
       /// What the keys do, drawn on the bottom border.
       hint: Option<String>,
   ```

   and `hint: None,` in `Picker::new`.
3. Add after `set_border`:

   ```rust
       /// The same picker, saying on its bottom border what its keys do.
       #[must_use]
       pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
           self.hint = Some(hint.into());
           self
       }

       /// What the bottom border says, if anything.
       #[must_use]
       pub fn hint(&self) -> Option<&str> {
           self.hint.as_deref()
       }

       /// Moves the highlight to the row whose id is `id`, if there is one.
       pub fn select(&mut self, id: &str) {
           if let Some(index) = self.items.iter().position(|item| item.id == id) {
               self.selected = index;
           }
       }
   ```

4. In `impl Widget for &Picker`, change the width to count the hint:

   ```rust
           let hint_width = self.hint.as_ref().map_or(0, |hint| hint.chars().count());
           let width = u16::try_from(widest.max(self.title.chars().count()).max(hint_width) + 6)
               .unwrap_or(u16::MAX)
               .clamp(20, area.width);
   ```

   and build the block with the hint:

   ```rust
           let mut block = Block::default()
               .borders(Borders::ALL)
               .title(format!(" {} ", self.title))
               .border_style(self.border);
           if let Some(hint) = &self.hint {
               block = block.title_bottom(Line::styled(format!(" {hint} "), self.border).right_aligned());
           }
   ```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p dispatch-tui picker`
Expected: PASS.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy -p dispatch-tui --all-targets -- -D warnings`

```bash
git add crates/dispatch-tui/src/picker.rs crates/dispatch-tui/src/picker/tests.rs
git commit -F - <<'EOF'
feat(tui): let a picker say what its keys do and select a row by id

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 8: The settings popup widget

**Files:**
- Create: `crates/dispatch-tui/src/settings_form.rs`
- Create: `crates/dispatch-tui/src/settings_form/tests.rs`
- Modify: `crates/dispatch-tui/src/lib.rs`

**Interfaces:**
- Consumes: `dispatch_config::{Choices, MAX_VALUE_CHARS, SAFE_CHARACTERS, SettingDef, SettingKind, is_safe_char}`; `crate::picker::{centred, write}` (both `pub(crate)`); `crate::input::{KeyCode, KeyEvent, KeyModifiers}`.
- Produces:
  - `pub enum FormAction { None, Open, Save, Back, Refused(String) }`
  - `pub const AGENT_DEFAULT: &str = "agent default"`
  - `SettingsForm::new(title: impl Into<String>, settings: &[SettingDef], values: &Choices) -> SettingsForm`
  - `SettingsForm::key(&mut self, key: &KeyEvent) -> FormAction`
  - `SettingsForm::paste(&mut self, text: &str) -> FormAction`
  - `SettingsForm::values(&self) -> Choices`
  - `SettingsForm::is_editing(&self) -> bool`
  - `SettingsForm::selected_index(&self) -> usize`
  - `SettingsForm::set_border(&mut self, style: Style)`
  - `SettingsForm::set_styles(&mut self, highlight: Style, faded: Style)`
  - `impl Widget for &SettingsForm`
  - Re-exported as `dispatch_tui::{FormAction, SettingsForm}`

- [ ] **Step 1: Write the failing tests**

Create `crates/dispatch-tui/src/settings_form/tests.rs`:

```rust
//! Tests for the settings popup.

use super::*;

use dispatch_config::HarnessDef;

/// A model to choose or type, an effort to choose, and a flag on by default.
const SETTINGS: &str = r#"
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["small", "large"]
custom = true
args = ["--model", "{value}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
options = ["low", "high"]
args = ["--effort", "{value}"]

[[settings]]
key = "bypass"
label = "Skip prompts"
kind = "bool"
default = true
args = ["--yolo"]
"#;

fn settings() -> Vec<SettingDef> {
    let def: HarnessDef = toml::from_str(&format!(
        "id = \"demo\"\ndisplay_name = \"Demo\"\ncommand = \"demo\"\n{SETTINGS}"
    ))
    .expect("a valid harness");
    def.settings
}

fn form_with(values: &[(&str, &str)]) -> SettingsForm {
    let values: Choices = values
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    SettingsForm::new("New Demo pane", &settings(), &values)
}

fn form() -> SettingsForm {
    form_with(&[])
}

fn press(form: &mut SettingsForm, code: KeyCode) -> FormAction {
    form.key(&KeyEvent::new(code, KeyModifiers::NONE))
}

fn value(form: &SettingsForm, key: &str) -> String {
    form.values()[key].clone()
}

fn render(form: &SettingsForm, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    form.render(area, &mut buf);
    (0..height)
        .map(|y| {
            (0..width)
                .filter_map(|x| buf.cell((x, y)))
                .map(|cell| cell.symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn it_opens_on_the_values_given_and_the_files_defaults_for_the_rest() {
    let form = form_with(&[("model", "large")]);

    assert_eq!(value(&form, "model"), "large");
    assert_eq!(value(&form, "effort"), "");
    assert_eq!(value(&form, "bypass"), "true");
}

#[test]
fn right_steps_through_agent_default_the_options_then_a_typed_value() {
    let mut form = form();

    press(&mut form, KeyCode::Right);
    assert_eq!(value(&form, "model"), "small");
    press(&mut form, KeyCode::Right);
    assert_eq!(value(&form, "model"), "large");
    press(&mut form, KeyCode::Right);
    assert!(form.is_editing(), "the typed slot starts typing");

    press(&mut form, KeyCode::Right);
    assert!(!form.is_editing());
    assert_eq!(value(&form, "model"), "", "and wraps to agent default");
}

#[test]
fn left_from_agent_default_wraps_to_typing_on_a_choice_that_takes_one() {
    let mut form = form();

    press(&mut form, KeyCode::Left);

    assert!(form.is_editing());
}

#[test]
fn a_choice_that_takes_no_typed_value_wraps_to_its_last_option() {
    let mut form = form();
    press(&mut form, KeyCode::Down);

    press(&mut form, KeyCode::Left);

    assert!(!form.is_editing());
    assert_eq!(value(&form, "effort"), "high");
}

#[test]
fn a_flag_steps_and_space_toggles_it() {
    let mut form = form();
    press(&mut form, KeyCode::Down);
    press(&mut form, KeyCode::Down);

    press(&mut form, KeyCode::Right);
    assert_eq!(value(&form, "bypass"), "false");
    press(&mut form, KeyCode::Char(' '));
    assert_eq!(value(&form, "bypass"), "true");
}

#[test]
fn rows_wrap_at_either_end() {
    let mut form = form();

    press(&mut form, KeyCode::Up);
    assert_eq!(form.selected_index(), 2);
    press(&mut form, KeyCode::Char('j'));
    assert_eq!(form.selected_index(), 0);
}

#[test]
fn while_typing_letters_are_text_not_keys() {
    let mut form = form();
    press(&mut form, KeyCode::Left);

    for c in ['j', 's', 'h', 'l', 'q'] {
        assert_eq!(press(&mut form, KeyCode::Char(c)), FormAction::None, "{c}");
    }
    assert_eq!(
        press(&mut form, KeyCode::Enter),
        FormAction::None,
        "Enter confirms the text and opens nothing"
    );

    assert!(!form.is_editing());
    assert_eq!(value(&form, "model"), "jshlq");
}

#[test]
fn a_character_a_value_cannot_hold_is_refused_and_said() {
    let mut form = form();
    press(&mut form, KeyCode::Left);
    press(&mut form, KeyCode::Char('a'));

    let action = press(&mut form, KeyCode::Char('&'));

    let FormAction::Refused(why) = action else {
        panic!("& was accepted");
    };
    assert!(why.contains(SAFE_CHARACTERS), "{why}");
    press(&mut form, KeyCode::Enter);
    assert_eq!(value(&form, "model"), "a");
}

#[test]
fn a_value_cannot_start_with_a_dash() {
    let mut form = form();
    press(&mut form, KeyCode::Left);

    assert!(matches!(
        press(&mut form, KeyCode::Char('-')),
        FormAction::Refused(_)
    ));
    press(&mut form, KeyCode::Char('a'));
    assert_eq!(press(&mut form, KeyCode::Char('-')), FormAction::None);
    press(&mut form, KeyCode::Enter);
    assert_eq!(value(&form, "model"), "a-");
}

#[test]
fn down_while_typing_keeps_the_text_and_moves_on() {
    let mut form = form();
    press(&mut form, KeyCode::Left);
    press(&mut form, KeyCode::Char('x'));
    press(&mut form, KeyCode::Char('1'));

    press(&mut form, KeyCode::Down);

    assert!(!form.is_editing());
    assert_eq!(value(&form, "model"), "x1");
    assert_eq!(form.selected_index(), 1);
}

#[test]
fn esc_while_typing_puts_back_the_value_from_before() {
    let mut form = form_with(&[("model", "large")]);
    press(&mut form, KeyCode::Right);
    assert!(form.is_editing());
    press(&mut form, KeyCode::Char('z'));

    assert_eq!(press(&mut form, KeyCode::Esc), FormAction::None);
    assert!(!form.is_editing());
    assert_eq!(value(&form, "model"), "large");

    assert_eq!(
        press(&mut form, KeyCode::Esc),
        FormAction::Back,
        "a second Esc leaves"
    );
}

#[test]
fn stepping_back_onto_the_typed_slot_offers_what_was_typed() {
    let mut form = form();
    press(&mut form, KeyCode::Left);
    for c in "abc".chars() {
        press(&mut form, KeyCode::Char(c));
    }
    press(&mut form, KeyCode::Enter);
    press(&mut form, KeyCode::Right);
    assert_eq!(value(&form, "model"), "");

    press(&mut form, KeyCode::Left);

    assert!(form.is_editing());
    assert!(render(&form, 80, 10).contains("Custom: abc▏"));
}

#[test]
fn nothing_typed_is_agent_default() {
    let mut form = form_with(&[("model", "large")]);
    press(&mut form, KeyCode::Right);

    press(&mut form, KeyCode::Enter);

    assert_eq!(value(&form, "model"), "");
}

#[test]
fn enter_opens_s_saves_and_esc_goes_back() {
    let mut form = form();

    assert_eq!(press(&mut form, KeyCode::Enter), FormAction::Open);
    assert_eq!(press(&mut form, KeyCode::Char('s')), FormAction::Save);
    assert_eq!(press(&mut form, KeyCode::Esc), FormAction::Back);
}

#[test]
fn a_chord_is_not_a_command() {
    let mut form = form();

    assert_eq!(
        form.key(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        FormAction::None
    );
}

#[test]
fn a_paste_keeps_only_what_a_value_may_hold() {
    let mut form = form();
    press(&mut form, KeyCode::Left);

    let action = form.paste("big-1\n&x");

    assert!(matches!(action, FormAction::Refused(_)), "the & is said");
    press(&mut form, KeyCode::Enter);
    assert_eq!(value(&form, "model"), "big-1x");
}

#[test]
fn a_paste_with_nothing_being_typed_does_nothing() {
    let mut form = form();

    assert_eq!(form.paste("large"), FormAction::None);
    assert_eq!(value(&form, "model"), "");
}

#[test]
fn it_draws_its_title_rows_and_hint() {
    let text = render(&form(), 80, 10);

    for expected in [
        "New Demo pane",
        "Model",
        "Effort",
        "Skip prompts",
        AGENT_DEFAULT,
        "on",
        "Enter open · s save as default · Esc back",
    ] {
        assert!(text.contains(expected), "{expected:?} missing from\n{text}");
    }
}

#[test]
fn while_typing_it_shows_the_text_and_its_own_hint() {
    let mut form = form();
    press(&mut form, KeyCode::Left);
    press(&mut form, KeyCode::Char('a'));
    press(&mut form, KeyCode::Char('b'));

    let text = render(&form, 80, 10);

    assert!(text.contains("Custom: ab▏"), "{text}");
    assert!(text.contains("Enter confirm · Esc cancel"), "{text}");
}

#[test]
fn a_typed_value_shows_as_custom() {
    assert!(render(&form_with(&[("model", "typed-1")]), 80, 10).contains("Custom: typed-1"));
}

#[test]
fn a_popup_bigger_than_the_screen_is_clipped_not_a_panic() {
    let long = "a".repeat(dispatch_config::MAX_VALUE_CHARS);
    let form = form_with(&[("model", long.as_str())]);

    for (width, height) in [(80, 10), (30, 4), (20, 3), (8, 3), (5, 2), (1, 1)] {
        let _ = render(&form, width, height);
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch-tui settings_form`
Expected: FAIL to compile: there is no module `settings_form`.

- [ ] **Step 3: Implement**

Create `crates/dispatch-tui/src/settings_form.rs`:

```rust
//! The settings popup: a harness's settings, one row each, stepped through
//! their values before a pane is opened with them or they are saved as the
//! harness's defaults.
//!
//! It owns its keys, so whoever holds it acts only on what a key asks for:
//! open, save, or go back.

use dispatch_config::{
    Choices, MAX_VALUE_CHARS, SAFE_CHARACTERS, SettingDef, SettingKind, is_safe_char,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Widget};

use crate::input::{KeyCode, KeyEvent, KeyModifiers};
use crate::picker::{centred, write};

/// What a key in the popup asks of whoever holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormAction {
    /// Nothing beyond what the popup did to itself.
    None,
    /// Open a pane with [`SettingsForm::values`], saving nothing.
    Open,
    /// Save [`SettingsForm::values`] as the harness's defaults.
    Save,
    /// Back to the picker, with nothing saved.
    Back,
    /// A key was refused; the reason is for the status row.
    Refused(String),
}

/// What is shown for a value that leaves the choice to the agent.
pub const AGENT_DEFAULT: &str = "agent default";

/// What the keys do.
const HINT: &str = "Enter open · s save as default · Esc back";

/// What the keys do while a value is being typed.
const EDITING_HINT: &str = "Enter confirm · Esc cancel";

/// How many characters of a typed value the box keeps room for, so typing
/// does not resize it.
const TYPED_WIDTH: usize = 24;

/// What a row holds.
#[derive(Debug, Clone)]
enum Shape {
    /// On or off.
    Flag,
    /// One of a list, and maybe a typed value after it.
    Choice { options: Vec<String>, custom: bool },
    /// Typed.
    Text,
}

/// Where a value sits among its row's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// Agent default.
    Default,
    /// One of the options, by index.
    Option(usize),
    /// Typed.
    Typed,
}

/// One setting.
#[derive(Debug, Clone)]
struct Row {
    key: String,
    label: String,
    shape: Shape,
    /// `""` for agent default; `"true"` or `"false"` for a flag.
    value: String,
    /// The last value typed on this row, offered again when the row steps
    /// back onto its typed slot.
    typed: String,
}

impl Row {
    fn new(setting: &SettingDef, value: String) -> Row {
        let shape = match &setting.kind {
            SettingKind::Bool { .. } => Shape::Flag,
            SettingKind::Choice { options, .. } => Shape::Choice {
                options: options.clone(),
                custom: setting.custom,
            },
            SettingKind::Text { .. } => Shape::Text,
        };
        let mut row = Row {
            key: setting.key.clone(),
            label: setting.label.clone(),
            shape,
            value,
            typed: String::new(),
        };
        if row.slot() == Slot::Typed {
            row.typed = row.value.clone();
        }
        row
    }

    /// The row's values, in the order `←` and `→` step through them.
    fn slots(&self) -> Vec<Slot> {
        match &self.shape {
            Shape::Flag => Vec::new(),
            Shape::Choice { options, custom } => {
                let mut slots = vec![Slot::Default];
                slots.extend((0..options.len()).map(Slot::Option));
                if *custom {
                    slots.push(Slot::Typed);
                }
                slots
            }
            Shape::Text => vec![Slot::Default, Slot::Typed],
        }
    }

    /// Where the value sits.
    fn slot(&self) -> Slot {
        match &self.shape {
            Shape::Flag => Slot::Default,
            _ if self.value.is_empty() => Slot::Default,
            Shape::Choice { options, .. } => options
                .iter()
                .position(|option| *option == self.value)
                .map_or(Slot::Typed, Slot::Option),
            Shape::Text => Slot::Typed,
        }
    }

    /// The value as the popup shows it.
    fn shown(&self) -> String {
        match &self.shape {
            Shape::Flag if self.value == "true" => "on".to_string(),
            Shape::Flag => "off".to_string(),
            _ if self.value.is_empty() => AGENT_DEFAULT.to_string(),
            Shape::Choice { .. } if self.slot() == Slot::Typed => {
                format!("Custom: {}", self.value)
            }
            Shape::Choice { .. } | Shape::Text => self.value.clone(),
        }
    }

    /// The widest this row's value can be drawn, so stepping through its
    /// values does not resize the box.
    fn widest(&self) -> usize {
        let typed = "Custom: ".len() + TYPED_WIDTH;
        let longest = match &self.shape {
            Shape::Flag => "off".len(),
            Shape::Choice { options, custom } => {
                let option = options
                    .iter()
                    .map(|option| option.chars().count())
                    .max()
                    .unwrap_or(0);
                option
                    .max(AGENT_DEFAULT.len())
                    .max(if *custom { typed } else { 0 })
            }
            Shape::Text => AGENT_DEFAULT.len().max(typed),
        };
        // A value already typed can be longer than the room kept for one;
        // one more for the cursor.
        longest.max(self.shown().chars().count() + 1)
    }
}

/// A value being typed on the selected row.
#[derive(Debug, Clone)]
struct Edit {
    /// The row's value before typing began, for `Esc`.
    before: String,
    text: String,
}

/// A harness's settings, one row each.
#[derive(Debug, Clone)]
pub struct SettingsForm {
    title: String,
    rows: Vec<Row>,
    selected: usize,
    editing: Option<Edit>,
    /// The frame's colour, set by whoever draws it so it matches the rest
    /// of the interface.
    border: Style,
    /// The selected row.
    highlight: Style,
    /// The arrows and the hint.
    faded: Style,
}

impl SettingsForm {
    /// A popup titled `title` over `settings`, each showing its value from
    /// `values`, or its file's default where `values` has none.
    #[must_use]
    pub fn new(title: impl Into<String>, settings: &[SettingDef], values: &Choices) -> Self {
        let rows = settings
            .iter()
            .map(|setting| {
                let value = values
                    .get(&setting.key)
                    .cloned()
                    .unwrap_or_else(|| setting.file_default());
                Row::new(setting, value)
            })
            .collect();

        Self {
            title: title.into(),
            rows,
            selected: 0,
            editing: None,
            border: Style::default().fg(Color::Cyan),
            highlight: Style::default().bg(Color::DarkGray),
            faded: Style::default().fg(Color::DarkGray),
        }
    }

    /// Draws the frame in `style`.
    pub fn set_border(&mut self, style: Style) {
        self.border = style;
    }

    /// Draws the selected row in `highlight`, and the arrows and hint in
    /// `faded`.
    pub fn set_styles(&mut self, highlight: Style, faded: Style) {
        self.highlight = highlight;
        self.faded = faded;
    }

    /// Every row's value, by key: what opening or saving uses.
    #[must_use]
    pub fn values(&self) -> Choices {
        self.rows
            .iter()
            .map(|row| (row.key.clone(), row.value.clone()))
            .collect()
    }

    /// Whether a value is being typed.
    #[must_use]
    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    /// Index of the selected row.
    #[must_use]
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Acts on one key.
    pub fn key(&mut self, key: &KeyEvent) -> FormAction {
        if self.editing.is_some() {
            return self.edit_key(key);
        }
        // A chord is nobody's here: Ctrl s must not save by accident.
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return FormAction::None;
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.previous_row(),
            KeyCode::Down | KeyCode::Char('j') => self.next_row(),
            KeyCode::Left | KeyCode::Char('h') => self.step(false),
            KeyCode::Right | KeyCode::Char('l') => self.step(true),
            KeyCode::Char(' ') => self.toggle(),
            KeyCode::Enter => return FormAction::Open,
            KeyCode::Char('s') => return FormAction::Save,
            KeyCode::Esc => return FormAction::Back,
            _ => {}
        }
        FormAction::None
    }

    /// Types `text` into a value being typed, dropping line breaks: `Enter`
    /// is a decision, and the newline a copied name often ends with must not
    /// make it. What a value cannot hold is left out, and said.
    pub fn paste(&mut self, text: &str) -> FormAction {
        if self.editing.is_none() {
            return FormAction::None;
        }
        let mut outcome = FormAction::None;
        for c in text.chars().filter(|c| !matches!(c, '\r' | '\n')) {
            if let FormAction::Refused(why) = self.type_char(c) {
                outcome = FormAction::Refused(why);
            }
        }
        outcome
    }

    /// Acts on one key while a value is being typed: every printable key is
    /// text, and the arrows still move.
    fn edit_key(&mut self, key: &KeyEvent) -> FormAction {
        match key.code {
            KeyCode::Enter => self.confirm(),
            KeyCode::Esc => self.cancel(),
            KeyCode::Up => {
                self.confirm();
                self.previous_row();
            }
            KeyCode::Down => {
                self.confirm();
                self.next_row();
            }
            // The text is dropped: the arrows never trap the user on the
            // typed slot.
            KeyCode::Left => self.step(false),
            KeyCode::Right => self.step(true),
            KeyCode::Backspace => {
                if let Some(edit) = &mut self.editing {
                    edit.text.pop();
                }
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                return self.type_char(c);
            }
            _ => {}
        }
        FormAction::None
    }

    /// Types `c`, if a value may hold it there.
    fn type_char(&mut self, c: char) -> FormAction {
        let Some(edit) = &mut self.editing else {
            return FormAction::None;
        };
        let fits = edit.text.chars().count() < MAX_VALUE_CHARS;
        let leading_dash = edit.text.is_empty() && c == '-';

        if is_safe_char(c) && fits && !leading_dash {
            edit.text.push(c);
            FormAction::None
        } else {
            FormAction::Refused(format!(
                "a value takes only {SAFE_CHARACTERS}, up to {MAX_VALUE_CHARS}"
            ))
        }
    }

    fn previous_row(&mut self) {
        if !self.rows.is_empty() {
            self.selected = self.selected.checked_sub(1).unwrap_or(self.rows.len() - 1);
        }
    }

    fn next_row(&mut self) {
        if !self.rows.is_empty() {
            self.selected = (self.selected + 1) % self.rows.len();
        }
    }

    /// Flips the selected row, if it is on or off.
    fn toggle(&mut self) {
        if let Some(row) = self.rows.get_mut(self.selected)
            && matches!(row.shape, Shape::Flag)
        {
            row.value = if row.value == "true" { "false" } else { "true" }.to_string();
        }
    }

    /// Steps the selected row to its next or previous value, wrapping.
    ///
    /// Stepping onto the typed slot starts typing there; stepping while
    /// typing drops the text and moves to the typed slot's neighbour.
    fn step(&mut self, forward: bool) {
        let typing = self.editing.take().is_some();
        let Some(row) = self.rows.get_mut(self.selected) else {
            return;
        };
        if matches!(row.shape, Shape::Flag) {
            row.value = if row.value == "true" { "false" } else { "true" }.to_string();
            return;
        }

        let slots = row.slots();
        let here = if typing {
            slots.len() - 1
        } else {
            slots
                .iter()
                .position(|slot| *slot == row.slot())
                .unwrap_or(0)
        };
        let next = if forward {
            (here + 1) % slots.len()
        } else {
            (here + slots.len() - 1) % slots.len()
        };

        match slots[next] {
            Slot::Default => row.value.clear(),
            Slot::Option(index) => {
                if let Shape::Choice { options, .. } = &row.shape {
                    row.value = options[index].clone();
                }
            }
            Slot::Typed => {
                self.editing = Some(Edit {
                    before: row.value.clone(),
                    text: row.typed.clone(),
                });
            }
        }
    }

    /// Stops typing and keeps the text: nothing typed is agent default.
    fn confirm(&mut self) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        if let Some(row) = self.rows.get_mut(self.selected) {
            row.typed = edit.text.clone();
            row.value = edit.text;
        }
    }

    /// Stops typing and puts back the value from before it began.
    fn cancel(&mut self) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        if let Some(row) = self.rows.get_mut(self.selected) {
            row.value = edit.before;
        }
    }
}

impl Widget for &SettingsForm {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 8 || area.height < 3 {
            return;
        }

        let label_width = self
            .rows
            .iter()
            .map(|row| row.label.chars().count())
            .max()
            .unwrap_or(0);
        let value_width = self.rows.iter().map(Row::widest).max().unwrap_or(0);
        let hint = if self.editing.is_some() {
            EDITING_HINT
        } else {
            HINT
        };

        // A space either side, two between label and value, and the arrows
        // around the value.
        let row_width = 1 + label_width + 2 + 2 + value_width + 2 + 1;
        let widest = row_width
            .max(self.title.chars().count() + 4)
            .max(hint.chars().count() + 4);
        let width = u16::try_from(widest + 2)
            .unwrap_or(u16::MAX)
            .min(area.width);
        let height = u16::try_from(self.rows.len() + 2)
            .unwrap_or(u16::MAX)
            .min(area.height);
        let rect = centred(area, width, height);

        // It floats over the grid, so whatever it covers is erased rather
        // than left showing through.
        Clear.render(rect, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.title))
            .title_bottom(Line::styled(format!(" {hint} "), self.faded).right_aligned())
            .border_style(self.border);
        let inner = block.inner(rect);
        block.render(rect, buf);

        let value_x = inner
            .x
            .saturating_add(1)
            .saturating_add(u16::try_from(label_width + 2).unwrap_or(u16::MAX));

        for (index, row) in self
            .rows
            .iter()
            .enumerate()
            .take(usize::from(inner.height))
        {
            let y = inner.y + u16::try_from(index).unwrap_or(0);
            let chosen = index == self.selected;
            let base = if chosen {
                self.highlight
            } else {
                Style::default()
            };

            // The whole row, so the highlight is a bar rather than just
            // behind the text.
            if chosen {
                for x in inner.x..inner.x + inner.width {
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_symbol(" ");
                        cell.set_style(base);
                    }
                }
            }

            write(buf, inner, inner.x + 1, y, &row.label, base);

            let shown = match (&self.editing, chosen, &row.shape) {
                (Some(edit), true, Shape::Text) => format!("{}▏", edit.text),
                (Some(edit), true, _) => format!("Custom: {}▏", edit.text),
                _ => row.shown(),
            };
            let arrows = base.patch(self.faded);
            let x = write(buf, inner, value_x, y, "◂ ", arrows);
            let x = write(buf, inner, x, y, &shown, base);
            write(buf, inner, x, y, " ▸", arrows);
        }
    }
}

#[cfg(test)]
mod tests;
```

In `crates/dispatch-tui/src/lib.rs`, add `pub mod settings_form;` after `pub mod prompt;`, and `pub use settings_form::{FormAction, SettingsForm};` after `pub use prompt::{Note, Prompt};`.

`write` in `picker.rs` takes `x: u16` and stops at the area's right edge; `value_x` is built with saturating adds, so a label too wide for the screen clips and cannot overflow.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p dispatch-tui settings_form`
Expected: PASS (21 tests).

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy -p dispatch-tui --all-targets -- -D warnings`

```bash
git add crates/dispatch-tui/src/settings_form.rs crates/dispatch-tui/src/settings_form/tests.rs crates/dispatch-tui/src/lib.rs
git commit -F - <<'EOF'
feat(tui): add the settings popup, stepping and typing a harness's values

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 9: The app: `e`, the popup, spawning and saving

**Files:**
- Modify: `dispatch/src/app.rs`
- Modify: `dispatch/src/main.rs`

**Interfaces:**
- Consumes:
  - Task 2's `HarnessDef::{usable_saved, resolve, launch_with, describe_changes}` and `Choices`
  - Task 3's `harness_settings::{load_or_empty, save}`
  - Task 5's `SpawnPane.settings`
  - Task 7's `Picker::{with_hint, hint, select}`
  - Task 8's `SettingsForm`, `FormAction`
- Produces: `App::keep_settings_in(&mut self, dir: impl Into<PathBuf>)` and `Overlay::Settings { harness: String, form: SettingsForm }`.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` in `dispatch/src/app.rs`, near `registry_with_shell`:

```rust
    /// Settings as a shipped harness has them: a model to choose or type,
    /// an effort, a permission choice with a default of its own, and a flag
    /// on by default.
    const DEMO_SETTINGS: &str = r#"
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["small", "large"]
custom = true
args = ["--model", "{value}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
options = ["low", "high"]
args = ["--effort", "{value}"]

[[settings]]
key = "permissions"
label = "Permissions"
kind = "choice"
options = ["ask", "never"]
default = "never"
args = ["--permissions", "{value}"]

[[settings]]
key = "bypass"
label = "Skip prompts"
kind = "bool"
default = true
args = ["--yolo"]
"#;

    /// A harness `id` running `command`, carrying [`DEMO_SETTINGS`].
    fn with_settings(id: &str, display: &str, command: &str) -> dispatch_config::HarnessDef {
        toml::from_str(&format!(
            "id = \"{id}\"\ndisplay_name = \"{display}\"\ncommand = \"{command}\"\n{DEMO_SETTINGS}"
        ))
        .expect("a valid harness")
    }

    /// `demo`, carrying [`DEMO_SETTINGS`], and the user's shell.
    fn registry_with_settings() -> HarnessRegistry {
        [
            with_settings("demo", "Demo", "demo"),
            dispatch_config::HarnessDef {
                id: SHELL.to_string(),
                display_name: "Shell".to_string(),
                ..dispatch_config::HarnessDef::default()
            },
        ]
        .into_iter()
        .collect()
    }

    /// `pairs` as [`Choices`].
    fn choices(pairs: &[(&str, &str)]) -> Choices {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    /// An attached app holding [`registry_with_settings`], its settings
    /// kept in `dir` when one is given, with the new-pane picker open on
    /// `demo`.
    fn app_on_demo(
        dir: Option<&std::path::Path>,
    ) -> (App, Sender<ServerMessage>, Receiver<ClientMessage>) {
        let (client, daemon, sent) = Client::for_test();
        let mut app = App::attached(registry_with_settings(), client);
        if let Some(dir) = dir {
            app.keep_settings_in(dir);
        }
        daemon
            .send(ServerMessage::ProjectOpened {
                project: Project::new("/tmp/attached", ProjectSource::LocalDir),
            })
            .expect("the app is listening");
        app.poll_daemon();

        app.open_harness_picker();
        let Some(Overlay::Harness(picker)) = &mut app.overlay else {
            panic!("the picker is open");
        };
        picker.select("demo");

        (app, daemon, sent)
    }

    /// The settings the last pane this app asked its daemon for was sent
    /// with.
    fn sent_settings(sent: &Receiver<ClientMessage>) -> Option<Choices> {
        sent.try_iter()
            .filter_map(|message| match message {
                ClientMessage::SpawnPane { settings, .. } => Some(settings),
                _ => None,
            })
            .last()
    }

    /// The popup's values, while it is open.
    fn form_values(app: &App) -> Choices {
        let Some(Overlay::Settings { form, .. }) = &app.overlay else {
            panic!("the settings are open");
        };
        form.values()
    }

    #[test]
    fn e_opens_the_settings_of_the_harness_the_picker_is_on() {
        let (mut app, _daemon, _sent) = app_on_demo(None);

        press(&mut app, KeyCode::Char('e'));

        assert!(
            matches!(&app.overlay, Some(Overlay::Settings { harness, .. }) if harness == "demo"),
            "the settings are open on demo"
        );
    }

    #[test]
    fn e_on_a_harness_with_no_settings_says_so() {
        let mut app = App::new(registry_with_settings());
        app.state
            .add_project(Project::new("/tmp/one", ProjectSource::LocalDir));
        app.open_harness_picker();

        press(&mut app, KeyCode::Char('e'));

        assert_eq!(app.status, "Shell has no settings");
        assert!(matches!(app.overlay, Some(Overlay::Harness(_))));
    }

    #[test]
    fn the_new_pane_picker_says_e_opens_the_settings() {
        let (app, _daemon, _sent) = app_on_demo(None);

        let Some(Overlay::Harness(picker)) = &app.overlay else {
            panic!("the picker is open");
        };
        assert_eq!(picker.hint(), Some("Enter open · e settings · Esc close"));
    }

    #[test]
    fn enter_in_the_settings_opens_a_pane_with_what_they_show() {
        let (mut app, _daemon, sent) = app_on_demo(None);
        press(&mut app, KeyCode::Char('e'));

        press(&mut app, KeyCode::Right); // model: small
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Right); // bypass: off
        press(&mut app, KeyCode::Enter);

        assert_eq!(
            sent_settings(&sent),
            Some(choices(&[
                ("model", "small"),
                ("effort", ""),
                ("permissions", "never"),
                ("bypass", "false"),
            ]))
        );
        assert!(app.overlay.is_none());
    }

    #[test]
    fn the_settings_open_showing_what_is_saved() {
        let dir = scratch("settings-open-saved");
        std::fs::write(dir.join("harness-settings.toml"), "[demo]\nmodel = \"large\"\n")
            .expect("temp dir is writable");
        let (mut app, _daemon, _sent) = app_on_demo(Some(&dir));

        press(&mut app, KeyCode::Char('e'));

        assert_eq!(form_values(&app)["model"], "large");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn s_saves_the_settings_and_keeps_them_open() {
        let dir = scratch("settings-save");
        let (mut app, _daemon, sent) = app_on_demo(Some(&dir));
        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Right); // model: small

        press(&mut app, KeyCode::Char('s'));

        assert_eq!(app.status, "saved as Demo's default");
        assert!(matches!(app.overlay, Some(Overlay::Settings { .. })));
        assert_eq!(sent_settings(&sent), None, "nothing was opened");
        let saved: toml::Table = std::fs::read_to_string(dir.join("harness-settings.toml"))
            .expect("the file was written")
            .parse()
            .expect("it is valid TOML");
        assert_eq!(saved["demo"]["model"].as_str(), Some("small"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn s_with_nowhere_to_keep_settings_says_so() {
        let (mut app, _daemon, _sent) = app_on_demo(None);
        press(&mut app, KeyCode::Char('e'));

        press(&mut app, KeyCode::Char('s'));

        assert!(app.status.starts_with("couldn't save"), "{}", app.status);
    }

    #[test]
    fn esc_in_the_settings_goes_back_to_the_picker_on_the_same_harness() {
        let mut app = App::new(registry_with_settings());
        app.state
            .add_project(Project::new("/tmp/one", ProjectSource::LocalDir));
        app.open_harness_picker();
        let Some(Overlay::Harness(picker)) = &mut app.overlay else {
            panic!("the picker is open");
        };
        picker.select("demo");
        press(&mut app, KeyCode::Char('e'));

        press(&mut app, KeyCode::Esc);

        let Some(Overlay::Harness(picker)) = &app.overlay else {
            panic!("back in the picker");
        };
        assert_eq!(
            picker.selected().map(|item| item.id.as_str()),
            Some("demo"),
            "on demo, not back at the shell"
        );
    }

    #[test]
    fn enter_in_the_picker_sends_the_saved_settings() {
        let dir = scratch("settings-picker-enter");
        std::fs::write(dir.join("harness-settings.toml"), "[demo]\nmodel = \"large\"\n")
            .expect("temp dir is writable");
        let (mut app, _daemon, sent) = app_on_demo(Some(&dir));

        press(&mut app, KeyCode::Enter);

        assert_eq!(sent_settings(&sent), Some(choices(&[("model", "large")])));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn enter_in_the_picker_with_nothing_saved_sends_no_settings() {
        // So a machine whose harness file differs from this one's still
        // starts the pane: what the user never chose is that machine's to
        // decide.
        let (mut app, _daemon, sent) = app_on_demo(None);

        press(&mut app, KeyCode::Enter);

        assert_eq!(sent_settings(&sent), Some(Choices::new()));
    }

    #[test]
    fn a_stale_saved_value_is_not_sent_and_the_pane_still_opens() {
        let dir = scratch("settings-stale");
        std::fs::write(
            dir.join("harness-settings.toml"),
            "[demo]\nmodel = \"large\"\neffort = \"extreme\"\ncolour = \"red\"\n",
        )
        .expect("temp dir is writable");
        let (mut app, _daemon, sent) = app_on_demo(Some(&dir));

        press(&mut app, KeyCode::Enter);

        assert_eq!(sent_settings(&sent), Some(choices(&[("model", "large")])));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn agent_default_saved_over_a_file_default_reaches_the_daemon_as_empty() {
        let dir = scratch("settings-agent-default");
        let (mut app, _daemon, sent) = app_on_demo(Some(&dir));
        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down); // permissions: never
        press(&mut app, KeyCode::Left); // ask
        press(&mut app, KeyCode::Left); // agent default
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Enter);

        assert_eq!(sent_settings(&sent), Some(choices(&[("permissions", "")])));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_picker_shows_what_differs_from_the_harnesss_defaults() {
        let dir = scratch("settings-detail");
        std::fs::write(
            dir.join("harness-settings.toml"),
            "[demo]\nmodel = \"large\"\nbypass = false\n",
        )
        .expect("temp dir is writable");
        let (app, _daemon, _sent) = app_on_demo(Some(&dir));

        let Some(Overlay::Harness(picker)) = &app.overlay else {
            panic!("the picker is open");
        };
        let demo = picker
            .items()
            .iter()
            .find(|item| item.id == "demo")
            .expect("demo is offered");
        assert_eq!(demo.detail.as_deref(), Some("demo · large · Skip prompts off"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_paste_reaches_a_value_being_typed() {
        let (mut app, _daemon, sent) = app_on_demo(None);
        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Left); // model: typed

        app.handle(&Event::Paste("big-1\n".to_string()), Size::new(100, 30))
            .expect("a paste is handled");
        press(&mut app, KeyCode::Enter); // confirm
        press(&mut app, KeyCode::Enter); // open

        assert_eq!(
            sent_settings(&sent).and_then(|settings| settings.get("model").cloned()),
            Some("big-1".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_standalone_pane_from_the_settings_starts_with_their_flags() {
        let dir = scratch("settings-standalone");
        let out = dir.join("args.out");
        let mut record = with_settings("record", "Record", "sh");
        record.launch.args = vec![
            "-c".to_string(),
            "printf '%s\\n' \"$@\" > \"$0\"; sleep 5".to_string(),
            out.display().to_string(),
        ];
        let mut app = App::new([record].into_iter().collect());
        app.add_project(dir.clone());
        let project = app.state.projects()[0].id;
        let _ = app.state.select_project(project);
        app.open_harness_picker();

        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Right); // model: small
        press(&mut app, KeyCode::Enter);

        let deadline = Instant::now() + Duration::from_secs(10);
        let written = loop {
            if let Ok(text) = std::fs::read_to_string(&out)
                && text.ends_with('\n')
            {
                break text;
            }
            assert!(Instant::now() < deadline, "the pane never wrote its arguments");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(
            written.lines().collect::<Vec<_>>(),
            vec!["--model", "small", "--permissions", "never", "--yolo"]
        );

        // Real processes: stopped here rather than left to outlive the test.
        for (_, mut pane) in app.panes.drain() {
            pane.backend.terminate();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
```

If `Event::Paste` is not reachable by that name in the tests module, use whatever path `handle_overlay`'s own `if let Event::Paste(text) = event` resolves through (`dispatch_tui::input::Event`). If `Duration` or `Instant` is not already in scope there, add `use std::time::{Duration, Instant};` to the tests module.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch settings`
Expected: FAIL to compile: `keep_settings_in` and `Overlay::Settings` do not exist.

- [ ] **Step 3: Implement**

All in `dispatch/src/app.rs` unless said otherwise.

1. **Imports.** Change `use dispatch_config::{HarnessRegistry, Launch, SHELL};` to `use dispatch_config::{Choices, HarnessRegistry, Launch, SHELL};`, and add `FormAction` and `SettingsForm` to the `use dispatch_tui::{ ... }` list.

2. **The overlay.** Add a variant to `enum Overlay`, after `Harness(Picker)`:

   ```rust
       /// A harness's settings, before a pane is opened with them.
       Settings {
           /// The harness they are for.
           harness: String,
           /// The rows.
           form: SettingsForm,
       },
   ```

   Add `| Overlay::Settings { .. }` to the list of variants that return `None` in `picker()`, `picker_mut()` and `kind()`. Add this arm to `set_border`:

   ```rust
               Overlay::Settings { form, .. } => form.set_border(style),
   ```

3. **Where settings are kept.** Add a field to `App`, after `kept`:

   ```rust
       /// Where the user's saved harness settings live, when this client
       /// keeps them. `None` in a test that has not said, so no test reads
       /// the developer's own.
       settings_dir: Option<PathBuf>,
   ```

   Add `settings_dir: None,` after `kept: None,` in `App::new`. Then add, after `keep_projects_in`:

   ```rust
       /// Reads and saves harness settings in `dir`.
       pub fn keep_settings_in(&mut self, dir: impl Into<PathBuf>) {
           self.settings_dir = Some(dir.into());
       }

       /// What the user saved for `harness`, or nothing when this client
       /// keeps no settings. A file that cannot be read is nothing saved.
       fn saved_settings(&self, harness: &str) -> Choices {
           self.settings_dir
               .as_deref()
               .map(|dir| dispatch_config::harness_settings::load_or_empty(dir, harness))
               .unwrap_or_default()
       }
   ```

   In `dispatch/src/main.rs`, after `app.keep_projects_in(&config_dir);`, add:

   ```rust
       app.keep_settings_in(&config_dir);
   ```

4. **Spawning with settings.** Replace the signature and head of `pub fn spawn_pane` so it becomes two functions:

   ```rust
       /// Starts a pane running `harness` in the selected project, with the
       /// user's saved settings.
       ///
       /// Attached, this asks and returns: the pane appears when the daemon says it
       /// has started one, which is also how the other clients hear about it.
       pub fn spawn_pane(&mut self, harness: &str, area: Size) -> Result<()> {
           // What Enter in the picker opens with: only what the user saved,
           // so a machine whose harness file differs from this one's still
           // starts the pane unless the user asked it for something it
           // cannot do.
           let chosen = self
               .harnesses
               .get(harness)
               .map(|def| def.usable_saved(&self.saved_settings(harness)))
               .unwrap_or_default();
           self.spawn_pane_with(harness, area, chosen)
       }

       /// Starts a pane running `harness` in the selected project, with
       /// `chosen` for its settings and the harness file's defaults for the
       /// rest.
       fn spawn_pane_with(&mut self, harness: &str, area: Size, chosen: Choices) -> Result<()> {
   ```

   Keep the body that follows (the `place` take, the project lookup, and so on), with two changes:
   - Replace the `let Some((display_name, launch)): Option<(String, Launch)> = ... else { ... };` statement with:

     ```rust
             // Resolved here as the daemon will resolve it: a value this
             // harness cannot take is refused before anything is asked of
             // anyone.
             let resolved: Option<Result<(String, Launch), String>> =
                 self.harnesses.get(harness).map(|def| {
                     def.resolve(&chosen, &Choices::new()).map(|values| {
                         (
                             def.display_name.clone(),
                             def.launch_with(std::env::consts::OS, &values),
                         )
                     })
                 });
             let (display_name, launch) = match resolved {
                 Some(Ok(found)) => found,
                 Some(Err(reason)) => {
                     self.status = reason;
                     return Ok(());
                 }
                 None => {
                     self.status = format!("unknown harness {harness:?}");
                     return Ok(());
                 }
             };
     ```

     Keep the comment above the old statement ("Cloned out of the registry…"), adjusted to the new one if it no longer fits.
   - In the attached branch's `ClientMessage::SpawnPane { .. }`, change `settings: Default::default(),` (from Task 5) to `settings: chosen,`.

5. **The `e` key.** In the picker key handling (the `match key.code` with `KeyCode::Char('d') if matches!(self.overlay, Some(Overlay::Project(_)))`), add before the `KeyCode::Enter` arm:

   ```rust
               // Only the new-pane picker: settings belong to a harness.
               KeyCode::Char('e') if matches!(self.overlay, Some(Overlay::Harness(_))) => {
                   self.open_settings();
               }
   ```

6. **Routing keys and pastes to the popup.** In `handle_overlay`, add a paste arm to the `match &mut self.overlay` in the `Event::Paste` branch:

   ```rust
                   Some(Overlay::Settings { form, .. }) => {
                       if let FormAction::Refused(why) = form.paste(text) {
                           self.status = why;
                       }
                   }
   ```

   After the `if matches!(self.overlay, Some(Overlay::Approval { .. })) { ... }` block, add:

   ```rust
           if matches!(self.overlay, Some(Overlay::Settings { .. })) {
               return self.handle_settings_key(key, area);
           }
   ```

7. **The popup's handlers.** Add these methods next to `open_harness_manager`:

   ```rust
       /// Opens the settings popup for the harness the picker is on.
       fn open_settings(&mut self) {
           let Some(id) = self
               .overlay
               .as_ref()
               .and_then(Overlay::picker)
               .and_then(Picker::selected)
               .map(|item| item.id.clone())
           else {
               return;
           };
           let Some(def) = self.harnesses.get(&id) else {
               return;
           };
           if def.settings.is_empty() {
               self.status = format!("{} has no settings", def.display_name);
               return;
           }

           let values = def
               .resolve(&Choices::new(), &self.saved_settings(&id))
               .unwrap_or_default();
           let form = SettingsForm::new(
               format!("New {} pane", def.display_name),
               &def.settings,
               &values,
           );
           self.overlay = Some(Overlay::Settings { harness: id, form });
       }

       /// Acts on one key while a harness's settings are open.
       fn handle_settings_key(&mut self, key: &KeyEvent, area: Size) -> Result<()> {
           let Some(Overlay::Settings { form, .. }) = &mut self.overlay else {
               return Ok(());
           };

           match form.key(key) {
               FormAction::None => {}
               FormAction::Refused(why) => self.status = why,
               FormAction::Back => self.back_to_picker(),
               FormAction::Save => self.save_settings(),
               FormAction::Open => {
                   if let Some(Overlay::Settings { harness, form }) = self.overlay.take() {
                       self.spawn_pane_with(&harness, area, form.values())?;
                   }
                   // A request that arrived while the popup had the keyboard
                   // was queued rather than shown.
                   if self.overlay.is_none() {
                       self.open_next_approval();
                   }
               }
           }
           Ok(())
       }

       /// Leaves the settings for the picker they came from, on the same
       /// harness, its detail brought up to date with anything just saved.
       fn back_to_picker(&mut self) {
           let Some(Overlay::Settings { harness, .. }) = self.overlay.take() else {
               return;
           };
           self.open_picker_placing(self.placing);
           if let Some(Overlay::Harness(picker)) = &mut self.overlay {
               picker.select(&harness);
           }
       }

       /// Saves what the popup shows as its harness's defaults, and says so.
       fn save_settings(&mut self) {
           let Some(Overlay::Settings { harness, form }) = &self.overlay else {
               return;
           };
           let Some(def) = self.harnesses.get(harness) else {
               return;
           };

           self.status = match &self.settings_dir {
               None => "couldn't save: there is nowhere to keep settings".to_string(),
               Some(dir) => {
                   match dispatch_config::harness_settings::save(dir, def, &form.values()) {
                       Ok(()) => format!("saved as {}'s default", def.display_name),
                       Err(error) => format!("couldn't save: {error}"),
                   }
               }
           };
       }
   ```

   `Placement` is `Copy`, so `self.open_picker_placing(self.placing)` compiles. If the borrow checker rejects that line, write `let place = self.placing;` first.

8. **The picker's detail and hint.** In `open_picker_placing`, replace the last line of the `.map(|h| { ... })` closure, `Item::new(&h.id, label).with_detail(&h.launch.command)`, with:

   ```rust
                   // What Enter would change from the harness file, so the
                   // choice is in view before it is made.
                   let mut detail = h.launch.command.clone();
                   let chosen = h.usable_saved(&self.saved_settings(&h.id));
                   if let Ok(values) = h.resolve(&chosen, &Choices::new()) {
                       for change in h.describe_changes(&values) {
                           detail.push_str(" · ");
                           detail.push_str(&change);
                       }
                   }
                   Item::new(&h.id, label).with_detail(detail)
   ```

   Then change `self.overlay = Some(Overlay::Harness(Picker::new("New pane", items)));` to:

   ```rust
           self.overlay = Some(Overlay::Harness(
               Picker::new("New pane", items).with_hint("Enter open · e settings · Esc close"),
           ));
   ```

9. **Drawing.** In `draw_overlay`, right after the `if let Some(overlay) = &mut self.overlay { overlay.set_border(border); }` block, add:

   ```rust
           if let Some(Overlay::Settings { form, .. }) = &mut self.overlay {
               form.set_styles(
                   Style::default().bg(self.theme.tint).fg(self.theme.text),
                   Style::default().fg(self.theme.faded),
               );
           }
   ```

   Then add, after the `if let Some(picker) = overlay.picker() { ... }` block:

   ```rust
           if let Overlay::Settings { form, .. } = overlay {
               frame.render_widget(form, panes_area);
               return;
           }
   ```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p dispatch`
Expected: PASS, including the 15 new tests (14 on every platform, 1 on Unix), and every existing picker and spawn test.

- [ ] **Step 5: Run the whole workspace, lint, and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --no-fail-fast`
Expected: all green.

```bash
git add dispatch/src/app.rs dispatch/src/main.rs
git commit -F - <<'EOF'
feat(dispatch): open a harness's settings with e in the new-pane picker

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 10: Documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/security-model.md`
- Modify: `docs/superpowers/specs/2026-09-28-harness-settings-design.md` (status line)

**Interfaces:** none.

- [ ] **Step 1: README: a section of its own**

Insert this section into `README.md` just before `## The sidebar` (after the `## Shell panes` section):

````markdown
## Harness settings

Every agent Dispatch ships starts **without its permission prompts**: Claude
Code in `bypassPermissions` mode, Codex with
`--dangerously-bypass-approvals-and-sandbox` (which also drops its sandbox),
agy with `--dangerously-skip-permissions`, and opencode with `--auto`. Read
[docs/security-model.md](docs/security-model.md) before running an agent you
do not trust this way.

In the new-pane picker, `e` opens the highlighted harness's settings:

| Key | Does |
|---|---|
| `↑` `↓` | move between settings |
| `←` `→` | step through a setting's values; `Space` flips one that is on or off |
| `Enter` | open a pane with what is shown, saving nothing |
| `s` | save what is shown as the harness's default |
| `Esc` | back to the picker |

A model that is not on the list is typed on the setting's **Custom…** value.
**agent default** passes nothing, and the agent decides as its own
configuration says. Beside each harness, the picker shows what you have saved
that differs from its file.

Saved defaults live beside `config.toml`:

```toml
# ~/.config/dispatch/harness-settings.toml
[claude]
model = "opus"
permissions = ""   # agent default: Claude asks, as it does outside Dispatch

[codex]
bypass = false     # Codex's prompts and sandbox are back
```

A subagent started by `dispatch delegate` uses them too.

A harness file says what its settings are, and how each becomes a flag, so a
harness you add can have them as well:

```toml
# ~/.config/dispatch/harnesses/claude.toml
[[settings]]
key = "effort"
label = "Effort"
kind = "choice"                 # choice | text | bool
options = ["low", "medium", "high", "xhigh", "max"]
custom = false                  # true adds a typed value to a choice
args = ["--effort", "{value}"]  # after the launch's own; nothing when unset
# env = { NAME = "{value}" }    # or a variable

[[settings]]
key = "permissions"
label = "Permissions"
kind = "choice"
options = ["bypassPermissions", "auto", "acceptEdits", "plan", "manual"]
default = "bypassPermissions"
args = ["--permission-mode", "{value}"]
```

A `bool` adds its `args` when it is on. A value may hold only letters, digits
and `. _ : / @ # + -`, and may not start with `-`: on Windows it sits on
`cmd.exe`'s command line. The daemon reads harness files when it starts, so
restart it after editing one.
````

- [ ] **Step 2: README: two sentences elsewhere**

- In `## Security model`, after the paragraph that ends "…it is not a sandbox for untrusted ones. See [docs/security-model.md](docs/security-model.md).", add a new paragraph: `Agents also start without their own permission prompts; see [Harness settings](#harness-settings) to turn them back on.`
- In `## Delegation`, after the sentence "`claude` and `codex` ship with one.", add: `A subagent starts with the harness's saved settings, auto-approve included, so it can use its tools without waiting on a prompt nobody would see.`

- [ ] **Step 3: The security model**

In `docs/security-model.md`, insert this section after the `## Approval is a workflow, not a sandbox` section (before `## Opening a project means trusting it`):

```markdown
## Agents start without permission prompts

Every harness Dispatch ships starts its agent in that agent's own
auto-approve mode: Claude Code's `bypassPermissions`, Codex's
`--dangerously-bypass-approvals-and-sandbox`, agy's
`--dangerously-skip-permissions`, opencode's `--auto`. An agent can then edit
files, run commands and reach the network as the user without asking first.
Codex's flag also turns off the sandbox it would otherwise run commands in.
A subagent started by delegation runs the same way: approval decides whether
it starts, not what it may do once it has.

This trades a guard rail for flow, and the user can trade it back. In the
new-pane picker, `e` on a harness opens its settings. Set Claude's
**Permissions** to another mode, or turn **Skip prompts** or
**Auto-approve** off, then press `s` to save the agent's own prompts as that
harness's default. The advice above stands: an agent that is not trusted
belongs under an operating-system boundary, not behind its own prompts.
```

- [ ] **Step 4: Mark the spec implemented**

In `docs/superpowers/specs/2026-09-28-harness-settings-design.md`, change `Status: designed, not yet planned.` to `Status: implemented on branch \`feat/harness-settings\`.` (with real backticks, not escaped).

- [ ] **Step 5: Commit**

```bash
git add README.md docs/security-model.md docs/superpowers/specs/2026-09-28-harness-settings-design.md
git commit -F - <<'EOF'
docs: describe harness settings and agents starting without prompts

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2
EOF
```

---

### Task 11: Check the real agents by hand

This is for the controller and the user, not a subagent. It needs the real CLIs, their accounts, and eyes on a TUI.

**Files:** possibly `crates/dispatch-config/harnesses/opencode.toml`, `README.md` and the spec, depending on Step 3.

- [ ] **Step 1: Each CLI takes its flags in the order Dispatch passes them**

These are cheap one-line prompts, with flags placed after the task exactly as `task_launch_with` does:

```bash
claude -p "Reply with OK" --model haiku --effort low --permission-mode bypassPermissions
codex exec "Reply with OK" -m gpt-5.5 -c model_reasoning_effort=low --dangerously-bypass-approvals-and-sandbox
agy -p "Reply with OK" --model gemini-3.8-flash-low --effort low --dangerously-skip-permissions
```

Expected: each prints a reply. If one rejects a flag after its prompt, the fix is in that harness's `[task]` args, not in the settings: report it before going on.

- [ ] **Step 2: Interactive panes**

Build and run with the new build on `PATH`, after stopping any running daemon:

```bash
cargo build --release
PATH="$PWD/target/release:$PATH" target/release/dispatch --attach .
```

For each of claude, codex and agy: open the new-pane picker, press `e`, pick a model and effort, and press `Enter`. The agent should start on that model, with no permission prompt when it edits a file. Claude asks once, the first time, to confirm bypass mode. That prompt is Claude's own and is expected.

- [ ] **Step 3: opencode's model row**

Press `e` on opencode, type a model it knows (for example one from `opencode models`), and press `Enter` twice. Check the model opencode shows.
- **It shows the chosen model:** nothing to change.
- **It does not:** in `opencode.toml`, give the model setting `args = ["--standalone"]` next to its `env`. That gives the pane a private server, which reads the variable. Rebuild and check again.
- **Still not:** delete opencode's `model` setting. In `crates/dispatch-config/src/tests.rs`, `built_in_models_and_efforts_ship_unset` asserts that every built-in offers a model; skip that assertion for opencode. Say in the README's Harness settings section that opencode's model is set in opencode's own configuration, and note the finding in the spec under "The shipped files".

Commit whatever changed, as `fix(config): …` with the attribution lines.

- [ ] **Step 4: Delegation**

In a Claude pane, ask Claude to run `dispatch delegate "create a file named delegated.txt containing hi"`. Approve it with `a`. The subagent should create the file without a permission prompt. Delete the file afterwards.
