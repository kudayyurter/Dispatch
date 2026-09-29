# Option labels and limited settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A choice's options may carry a clean label and say which values another setting may take while chosen, so agy gets its Effort row back (only the efforts each model offers, faded for a model with none) and every harness shows clean names.

**Architecture:** `dispatch-config` gains `ChoiceOption` (a plain string or a table in TOML), `SettingDef::limited_by`, and two free functions: `allowed` (which values a limited setting may take right now) and `fit_limits` (brings values within that). `HarnessDef::resolve` and `apply_settings` fit, so the client, the daemon and the saved file can never hand agy a pair it refuses. The popup in `dispatch-tui` keeps a copy of the settings, refits after every change, steps only through allowed values, and fades an unavailable row. The shipped TOMLs get labels and agy's limited Effort row; the old bodies become superseded.

**Tech Stack:** Rust 2024 (rust-version 1.89), serde + toml, ratatui.

**Spec:** `docs/superpowers/specs/2026-09-28-option-labels-and-limits-design.md`

## Global Constraints

- Branch `feat/option-labels-and-limits`. Every commit ends with exactly these two trailer lines, whatever model you are: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2` (the `git commit` commands below pass them with `--trailer`).
- A harness file written before this change must parse exactly as before (plain-string options keep working).
- Values keep the existing character rule: ASCII letters, digits and `. _ : / @ # + -`; 1 to 200 characters; no leading `-` (`is_safe_value`).
- A limited setting that is not allowed a value **never refuses a launch**: it is fitted. A chosen value that is not one of the setting's own options (and not a safe custom value where `custom = true`) is still refused, as today.
- "Highest" means the last allowed value in the limited setting's **own** `options` order.
- The shown text for an unavailable row is exactly `not available`; for agent default it stays `agent default`; a typed value stays `Custom: <text>`.
- Gates, all green before the PR: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings`; `cargo +1.89 build --workspace --all-targets --locked`; `cargo test --workspace --no-fail-fast`.
- Match the surrounding code: doc comments on every item, full sentences, the same naming voice as `settings.rs`.

## Review Focus

1. **A limited setting declared before its limiter** (effort above model in a user's file): resolve still fits it, and the popup does not open with an unavailable row selected. Tests in Task 2 (`a_limited_setting_declared_first_is_fitted_all_the_same`) and Task 4 (`an_unavailable_first_row_is_not_selected_when_the_popup_opens`).
2. **A label a user wrote with control characters** (an escape sequence would garble the popup): the label is dropped and the value is shown instead. Test in Task 1 (`a_label_the_popup_cannot_draw_falls_back_to_the_value`).
3. **An agy model saved before this change** (`gemini-3.8-flash-high`): it is no longer an option but still launches, as a typed model with the effort row free. Test in Task 3 (`an_agy_model_saved_before_this_change_still_launches`).
4. **A stale saved pair** (model `pro`, effort `medium` it does not offer): the popup opens showing High, and the picker detail says High. Tests in Task 4 (`values_are_fitted_when_the_popup_opens`) and Task 5 (`the_picker_names_saved_options_by_label_fitted_to_the_model`).
5. **Stepping off a model being typed** with `←`/`→`: the text is dropped, the model lands on a neighbour, and the effort refits at once. Test in Task 4 (`stepping_off_a_typed_model_refits_the_effort`).

---

### Task 1: Options that are tables, with labels

**Files:**
- Modify: `crates/dispatch-config/src/harness.rs` (the `SettingKind::Choice` variant; new `ChoiceOption`)
- Modify: `crates/dispatch-config/src/settings.rs` (`check`, `usable`, `describe_changes`; new `options`, `option`, `label_of`)
- Modify: `crates/dispatch-config/src/lib.rs:25-28` (export `ChoiceOption`)
- Modify: `crates/dispatch-tui/src/settings_form.rs` (compile only: options are now `ChoiceOption`)
- Test: `crates/dispatch-config/src/settings/tests.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `pub struct ChoiceOption { pub value: String, pub label: Option<String>, pub allows: BTreeMap<String, Vec<String>> }` with `ChoiceOption::new(value: impl Into<String>) -> ChoiceOption` and `ChoiceOption::label(&self) -> &str` (label, else value). Exported from `dispatch_config`.
  - `SettingKind::Choice { options: Vec<ChoiceOption>, default: Option<String> }`.
  - `SettingDef::options(&self) -> &[ChoiceOption]` (empty for text or a flag), `SettingDef::option(&self, value: &str) -> Option<&ChoiceOption>`, `SettingDef::label_of<'a>(&'a self, value: &'a str) -> &'a str`.
  - `describe_changes` names a chosen option by its label.

- [ ] **Step 1: Write the failing tests**

Append to `crates/dispatch-config/src/settings/tests.rs`:

```rust
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
    assert_eq!(model.options()[1].label(), "large", "a label defaults to the value");
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

    let values: Vec<&str> = setting(&def, "model")
        .options()
        .iter()
        .map(|o| o.value.as_str())
        .collect();
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

    assert_eq!(def.describe_changes(&values(&[("model", "small")])), vec!["Small"]);
    assert_eq!(def.describe_changes(&values(&[("model", "large")])), vec!["large"]);
    assert_eq!(def.describe_changes(&values(&[("model", "big-1")])), vec!["big-1"]);
}

#[test]
fn an_option_written_back_reads_the_same() {
    let model = setting(&demo_harness(LABELLED), "model");

    let written = toml::Value::try_from(&model).expect("a setting serialises");
    let back: SettingDef = written.try_into().expect("and reads back");

    assert_eq!(back, model);
}
```

Also change the existing assertion in `an_option_a_command_line_cannot_carry_is_dropped_and_the_rest_kept` (around line 140) from

```rust
    assert!(matches!(
        &setting(&def, "model").kind,
        SettingKind::Choice { options, .. } if options == &vec!["small".to_string()]
    ));
```

to

```rust
    assert_eq!(setting(&def, "model").options(), [ChoiceOption::new("small")]);
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch-config settings::tests`
Expected: compile errors (`options` method, `ChoiceOption` not found).

- [ ] **Step 3: Add `ChoiceOption` to `harness.rs`**

Replace the `Choice` variant of `SettingKind`:

```rust
    /// One of a fixed set of values, such as a model or effort level.
    Choice {
        /// The values on offer.
        options: Vec<ChoiceOption>,
        /// Value used when the user does not supply one.
        #[serde(default)]
        default: Option<String>,
    },
```

and add, just after the `SettingKind` enum:

```rust
/// One value a choice offers.
///
/// A harness file writes it as the value alone, or as a table naming how it
/// is shown too: `{ value = "gemini-3.1-pro", label = "Gemini 3.1 Pro",
/// effort = ["low", "high"] }`. Any other key in the table names a setting
/// `limited_by` this one, and lists the values that setting may take while
/// this option is chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "OptionForm", into = "OptionForm")]
pub struct ChoiceOption {
    /// What is passed to the agent and saved.
    pub value: String,
    /// What the popup and the picker show; the value when absent.
    pub label: Option<String>,
    /// For each setting limited by this one, by key: the values it may take
    /// while this option is chosen. A limited setting this names nothing
    /// for takes none.
    pub allows: BTreeMap<String, Vec<String>>,
}

impl ChoiceOption {
    /// An option with no label, limiting nothing.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: None,
            allows: BTreeMap::new(),
        }
    }

    /// What is shown for this option.
    #[must_use]
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.value)
    }
}

/// How an option is written in a harness file.
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum OptionForm {
    /// The value alone.
    Plain(String),
    /// A table.
    Table(OptionTable),
}

/// An option written as a table.
#[derive(Clone, Serialize, Deserialize)]
struct OptionTable {
    /// Missing is empty, which no value may be, so the option is dropped
    /// on load rather than failing the whole file.
    #[serde(default)]
    value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(flatten)]
    allows: BTreeMap<String, Vec<String>>,
}

impl From<OptionForm> for ChoiceOption {
    fn from(form: OptionForm) -> Self {
        match form {
            OptionForm::Plain(value) => ChoiceOption::new(value),
            OptionForm::Table(table) => ChoiceOption {
                value: table.value,
                label: table.label,
                allows: table.allows,
            },
        }
    }
}

impl From<ChoiceOption> for OptionForm {
    fn from(option: ChoiceOption) -> Self {
        if option.label.is_none() && option.allows.is_empty() {
            OptionForm::Plain(option.value)
        } else {
            OptionForm::Table(OptionTable {
                value: option.value,
                label: option.label,
                allows: option.allows,
            })
        }
    }
}
```

In `crates/dispatch-config/src/lib.rs`, add `ChoiceOption` to the `pub use harness::{…}` list (keep it sorted: `ChoiceOption, HarnessDef, Launch, …`).

- [ ] **Step 4: Teach `settings.rs` about option tables**

Change the import to `use crate::harness::{ChoiceOption, HarnessDef, Launch, SettingDef, SettingKind, TaskRun};`.

In `impl SettingDef`, add after `is_flag`:

```rust
    /// A choice's options, in file order; none for text or a flag.
    #[must_use]
    pub fn options(&self) -> &[ChoiceOption] {
        match &self.kind {
            SettingKind::Choice { options, .. } => options,
            SettingKind::Text { .. } | SettingKind::Bool { .. } => &[],
        }
    }

    /// The option whose value is `value`, if there is one.
    #[must_use]
    pub fn option(&self, value: &str) -> Option<&ChoiceOption> {
        self.options().iter().find(|option| option.value == value)
    }

    /// `value` as it is shown: an option by its label, anything else --
    /// typed, or text -- as itself.
    #[must_use]
    pub fn label_of<'a>(&'a self, value: &'a str) -> &'a str {
        self.option(value).map_or(value, ChoiceOption::label)
    }
```

In `check`, the choice arm's first test becomes:

```rust
                if value.is_empty() || options.iter().any(|option| option.value == value) {
```

In `usable`, replace the whole `SettingKind::Choice { options, default } => { … }` arm with:

```rust
            SettingKind::Choice { options, default } => {
                options.retain(|option| {
                    let safe = is_safe_value(&option.value);
                    if !safe {
                        tracing::warn!(
                            harness = id,
                            setting = %key,
                            option = %option.value,
                            "dropping an option a command line cannot safely carry"
                        );
                    }
                    safe
                });
                if options.is_empty() {
                    skip(id, &key, "a choice with no usable option");
                    continue;
                }

                // A label is written to the terminal as it is: an escape
                // sequence in one would redraw the screen.
                for option in options.iter_mut() {
                    let drawable = option.label.as_deref().is_none_or(|label| {
                        !label.trim().is_empty() && !label.chars().any(char::is_control)
                    });
                    if !drawable {
                        tracing::warn!(
                            harness = id,
                            setting = %key,
                            option = %option.value,
                            "dropping a label the popup cannot draw; the value is shown"
                        );
                        option.label = None;
                    }
                }

                let custom = setting.custom;
                let fits = default.as_deref().is_none_or(|value| {
                    options.iter().any(|option| option.value == value)
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
```

In `describe_changes`, the last branch `} else { value.clone() })` becomes `} else { setting.label_of(value).to_string() })`, and its doc comment's first sentence becomes: "Each value in `values` that differs from the file's default, as the new-pane picker names it beside the harness: an option by its label, typed text as itself, agent default and flags by the setting's label."

- [ ] **Step 5: Keep the popup compiling**

In `crates/dispatch-tui/src/settings_form.rs`, the popup still works on values only for now:
- `Shape::Choice { options: Vec<String>, custom: bool }` becomes `Shape::Choice { options: Vec<ChoiceOption>, custom: bool }`, and `use dispatch_config::{ChoiceOption, Choices, …}`.
- In `Row::slot`: `.position(|option| option.value == self.value)`.
- In `Row::widest`: `.map(|option| option.value.chars().count())`.
- In `step`: `row.value = options[index].value.clone();`.

(Task 4 replaces these with labels.)

- [ ] **Step 6: Run the tests**

Run: `cargo test -p dispatch-config && cargo test -p dispatch-tui settings_form`
Expected: all pass, including the five new tests.

- [ ] **Step 7: Commit**

```bash
git add crates/dispatch-config crates/dispatch-tui
git commit -m "feat(config): let a choice's options be tables with labels" --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" --trailer "Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2"
```

---

### Task 2: One setting limited by another

**Files:**
- Modify: `crates/dispatch-config/src/harness.rs` (`SettingDef::limited_by`)
- Modify: `crates/dispatch-config/src/settings.rs` (`Allowed`, `allowed`, `fit_limits`, `check_limits`; `usable`, `resolve`, `apply_settings`)
- Modify: `crates/dispatch-config/src/lib.rs:29` (exports)
- Test: `crates/dispatch-config/src/settings/tests.rs`

**Interfaces:**
- Consumes: `SettingDef::options`, `SettingDef::option`, `ChoiceOption::allows` (Task 1).
- Produces (all exported from `dispatch_config`):
  - `SettingDef.limited_by: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`).
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub enum Allowed { Free, Only(Vec<String>), Unavailable }`. `Only` lists values in the setting's own option order and is never empty.
  - `pub fn allowed(settings: &[SettingDef], key: &str, values: &Choices) -> Allowed`
  - `pub fn fit_limits(settings: &[SettingDef], values: &mut Choices)`
  - `HarnessDef::resolve` returns fitted values; `launch_with` and `task_launch_with` fit what they are handed.

- [ ] **Step 1: Write the failing tests**

Append to `crates/dispatch-config/src/settings/tests.rs`:

```rust
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
    assert_eq!(fitted(&[("model", "pro"), ("effort", "low")])["effort"], "low");
}

#[test]
fn an_effort_the_model_does_not_offer_becomes_its_highest() {
    assert_eq!(fitted(&[("model", "pro"), ("effort", "medium")])["effort"], "high");
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
        .resolve(&values(&[("model", "plain"), ("effort", "high")]), &Choices::new())
        .expect("fitted, not refused");

    assert_eq!(values["effort"], "");
    assert_eq!(def.launch_with("linux", &values).args, ["--tui", "--model", "plain"]);
}

#[test]
fn a_typed_or_unset_model_limits_nothing() {
    assert_eq!(fitted(&[("model", "big-1"), ("effort", "medium")])["effort"], "medium");
    assert_eq!(fitted(&[("model", "big-1"), ("effort", "")])["effort"], "");
    assert_eq!(fitted(&[("model", ""), ("effort", "medium")])["effort"], "medium");
}

#[test]
fn a_chosen_effort_that_is_no_option_at_all_is_still_refused() {
    let refused = demo_harness(LIMITED)
        .resolve(&values(&[("model", "pro"), ("effort", "ultra")]), &Choices::new());

    assert!(refused.is_err(), "{refused:?}");
}

#[test]
fn a_saved_pair_is_fitted_too() {
    let values = demo_harness(LIMITED)
        .resolve(&Choices::new(), &values(&[("model", "pro"), ("effort", "medium")]))
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

    assert_eq!(allowed_for("pro"), Allowed::Only(vec!["low".into(), "high".into()]));
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
    let reordered = LIMITED
        .split("[[settings]]")
        .filter(|part| !part.trim().is_empty())
        .rev()
        .map(|part| format!("[[settings]]{part}"))
        .collect::<String>();
    let def = demo_harness(&reordered);
    assert_eq!(def.settings[0].key, "effort");

    let values = def
        .resolve(&values(&[("model", "pro"), ("effort", "medium")]), &Choices::new())
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

        assert_eq!(setting(&def, "effort").limited_by, None, "limited_by = {limited_by:?}");
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
    let pro = setting(&def, "model").option("pro").cloned().expect("pro is offered");

    assert_eq!(pro.allows.get("effort"), Some(&vec!["low".to_string()]));
    assert!(!pro.allows.contains_key("colour"));
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch-config settings::tests`
Expected: compile errors (`limited_by`, `allowed`, `Allowed` not found).

- [ ] **Step 3: Add `limited_by` to `SettingDef`**

In `harness.rs`, add to `SettingDef` after `custom`:

```rust
    /// For a choice, the key of another choice whose chosen option decides
    /// which of this one's values may be used: the values that option lists
    /// under this setting's key, or none when it lists nothing. A value of
    /// the other's that is not one of its options limits nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limited_by: Option<String>,
```

- [ ] **Step 4: Add `Allowed`, `allowed` and `fit_limits` to `settings.rs`**

After `is_safe_value`:

```rust
/// Which values a setting may take, given the other settings' values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Allowed {
    /// Any of its options, a typed value where it takes one, or agent
    /// default.
    Free,
    /// Only these, in the setting's own option order, and never agent
    /// default. Never empty.
    Only(Vec<String>),
    /// None: it is unset, and passes nothing.
    Unavailable,
}

/// Which values the setting `key` of `settings` may take while the settings
/// hold `values`.
///
/// Only a setting `limited_by` another is ever limited, and only while that
/// other holds one of its options: a typed value, or agent default, limits
/// nothing.
#[must_use]
pub fn allowed(settings: &[SettingDef], key: &str, values: &Choices) -> Allowed {
    let find = |key: &str| settings.iter().find(|setting| setting.key == key);
    let Some(setting) = find(key) else {
        return Allowed::Free;
    };
    let Some(limiter) = setting.limited_by.as_deref().and_then(find) else {
        return Allowed::Free;
    };
    let chosen = values.get(&limiter.key).map_or("", String::as_str);
    let Some(option) = limiter.option(chosen) else {
        return Allowed::Free;
    };

    let listed = option.allows.get(key).map_or(&[][..], Vec::as_slice);
    let only: Vec<String> = setting
        .options()
        .iter()
        .filter(|offered| listed.contains(&offered.value))
        .map(|offered| offered.value.clone())
        .collect();
    if only.is_empty() {
        Allowed::Unavailable
    } else {
        Allowed::Only(only)
    }
}

/// Brings each limited setting in `values` within what [`allowed`] lets it
/// take: an unavailable one is unset, and one holding a value its list does
/// not have takes the highest the list does, the last in its own option
/// order.
///
/// A limit is followed, never refused: whatever the saved file or an older
/// client holds, the agent is only ever handed a pair it accepts.
pub fn fit_limits(settings: &[SettingDef], values: &mut Choices) {
    for setting in settings.iter().filter(|setting| setting.limited_by.is_some()) {
        let current = values.get(&setting.key).map_or("", String::as_str);
        let fitted = match allowed(settings, &setting.key, values) {
            Allowed::Free => continue,
            Allowed::Unavailable => String::new(),
            Allowed::Only(only) if only.iter().any(|value| value == current) => continue,
            Allowed::Only(only) => only.last().cloned().unwrap_or_default(),
        };
        values.insert(setting.key.clone(), fitted);
    }
}
```

- [ ] **Step 5: Check limits on load**

At the end of `usable`, replace the final `kept` with:

```rust
    check_limits(id, &mut kept);
    kept
```

and add after `usable`:

```rust
/// Drops each `limited_by` of harness `id` that cannot work, and from each
/// option's lists whatever does not name a setting it limits, or a value
/// that setting offers.
///
/// A limit must name another choice of the same harness that is not
/// limited itself: one level only, judged on the limits as written, so the
/// outcome does not depend on the order of the file.
fn check_limits(id: &str, settings: &mut [SettingDef]) {
    let written: Vec<(String, bool, bool)> = settings
        .iter()
        .map(|setting| {
            (
                setting.key.clone(),
                matches!(setting.kind, SettingKind::Choice { .. }),
                setting.limited_by.is_some(),
            )
        })
        .collect();

    for setting in settings.iter_mut() {
        let Some(by) = setting.limited_by.as_deref() else {
            continue;
        };
        let reason = if !matches!(setting.kind, SettingKind::Choice { .. }) {
            Some("only a choice can be limited")
        } else if by == setting.key {
            Some("a setting cannot limit itself")
        } else {
            match written.iter().find(|(key, ..)| key == by) {
                None => Some("limited_by names no setting of this harness"),
                Some((_, false, _)) => Some("limited_by must name a choice"),
                Some((_, _, true)) => Some("limited_by names a setting that is limited itself"),
                Some(_) => None,
            }
        };
        if let Some(reason) = reason {
            tracing::warn!(harness = id, setting = %setting.key, reason, "dropping a limit");
            setting.limited_by = None;
        }
    }

    // (limiter, limited, the values the limited one offers)
    let limits: Vec<(String, String, Vec<String>)> = settings
        .iter()
        .filter_map(|setting| {
            let by = setting.limited_by.clone()?;
            let offered = setting.options().iter().map(|o| o.value.clone()).collect();
            Some((by, setting.key.clone(), offered))
        })
        .collect();

    for setting in settings.iter_mut() {
        let key = setting.key.clone();
        let SettingKind::Choice { options, .. } = &mut setting.kind else {
            continue;
        };
        for option in options.iter_mut() {
            let value = option.value.clone();
            option.allows.retain(|target, listed| {
                let Some((.., offered)) = limits
                    .iter()
                    .find(|(by, limited, _)| *by == key && limited == target)
                else {
                    tracing::warn!(
                        harness = id,
                        setting = %key,
                        option = %value,
                        list = %target,
                        "dropping a list for a setting this one does not limit"
                    );
                    return false;
                };
                listed.retain(|allowed| {
                    let offers = offered.contains(allowed);
                    if !offers {
                        tracing::warn!(
                            harness = id,
                            setting = %target,
                            %allowed,
                            "dropping a listed value the setting does not offer"
                        );
                    }
                    offers
                });
                true
            });
        }
    }
}
```

- [ ] **Step 6: Fit in `resolve` and `apply_settings`**

In `resolve`, replace the final `Ok(values)` with:

```rust
        fit_limits(&self.settings, &mut values);
        Ok(values)
```

and add to its doc comment, after the first paragraph: "A setting limited by another is then fitted to it, as [`fit_limits`] says, rather than refused."

In `apply_settings`, fit a copy first. Its start becomes:

```rust
    /// Adds to `launch` what each setting's value in `values` turns on,
    /// once each limited setting is fitted to what limits it.
    fn apply_settings(&self, launch: &mut Launch, values: &Choices) {
        let mut values = values.clone();
        fit_limits(&self.settings, &mut values);

        for setting in &self.settings {
```

(the rest of the body is unchanged; `values.get(...)` now reads the fitted copy).

In `lib.rs`, the settings export becomes:

```rust
pub use settings::{
    Allowed, Choices, MAX_VALUE_CHARS, SAFE_CHARACTERS, allowed, fit_limits, is_safe_char,
    is_safe_value,
};
```

- [ ] **Step 7: Run the tests**

Run: `cargo test -p dispatch-config`
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add crates/dispatch-config
git commit -m "feat(config): let one choice limit another's values, fitted rather than refused" --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" --trailer "Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2"
```

---

### Task 3: Shipped harnesses get labels, and agy its Effort row back

**Files:**
- Create: `crates/dispatch-config/harnesses/superseded/claude-7.toml`, `codex-7.toml`, `agy-3.toml` (byte-for-byte copies of today's files)
- Modify: `crates/dispatch-config/harnesses/claude.toml`, `codex.toml`, `agy.toml`
- Modify: `crates/dispatch-config/src/defaults.rs` (superseded lists)
- Modify: `crates/dispatch-config/src/tests.rs` (upgrade list; replace `agy_takes_its_effort_from_the_model_name_alone`)
- Modify: `README.md` (the harness-settings section, after the claude example around line 273)

**Interfaces:**
- Consumes: option tables (Task 1), `limited_by` and fitting (Task 2).
- Produces: the shipped agy model values `gemini-3.8-flash`, `gemini-3.7-flash`, `gemini-3.6-flash`, `gemini-3.1-pro`, `gpt-oss-120b`, `claude-sonnet-4-6`, `claude-opus-4-6-thinking`, and a limited `effort` setting.

- [ ] **Step 1: Keep today's bodies as superseded**

```bash
cd crates/dispatch-config/harnesses
git show HEAD:crates/dispatch-config/harnesses/claude.toml > superseded/claude-7.toml
git show HEAD:crates/dispatch-config/harnesses/codex.toml > superseded/codex-7.toml
git show HEAD:crates/dispatch-config/harnesses/agy.toml > superseded/agy-3.toml
cmp superseded/claude-7.toml claude.toml && cmp superseded/codex-7.toml codex.toml && cmp superseded/agy-3.toml agy.toml
```

Expected: `cmp` prints nothing (identical).

In `defaults.rs`, append `include_str!("../harnesses/superseded/claude-7.toml")` to claude's list, `…/codex-7.toml` to codex's, and `…/agy-3.toml` to agy's. In `tests.rs`, `every_body_an_earlier_dispatch_wrote_is_upgraded`, append the same three lines to the matching lists.

- [ ] **Step 2: Write the failing tests**

In `crates/dispatch-config/src/tests.rs`, replace the whole test `agy_takes_its_effort_from_the_model_name_alone` with:

```rust
/// `pairs` as [`Choices`].
fn chosen(pairs: &[(&str, &str)]) -> Choices {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

#[test]
fn agy_is_passed_a_base_model_and_an_effort_it_offers() {
    // agy refuses an effort its model does not have (gemini-3.1-pro has no
    // medium) and any effort for a model with none (its Claude models).
    let (_dir, registry) = shipped();
    let agy = registry.get("agy").expect("it ships");
    let args = |pairs: &[(&str, &str)]| {
        let values = agy.resolve(&chosen(pairs), &Choices::new()).expect("fitted");
        agy.launch_with("linux", &values).args
    };

    assert_eq!(
        args(&[("model", "gemini-3.1-pro"), ("effort", "medium")]),
        ["--model", "gemini-3.1-pro", "--effort", "high", "--dangerously-skip-permissions"]
    );
    assert_eq!(
        args(&[("model", "gemini-3.8-flash"), ("effort", "medium")]),
        ["--model", "gemini-3.8-flash", "--effort", "medium", "--dangerously-skip-permissions"]
    );
    assert_eq!(
        args(&[("model", "claude-sonnet-4-6"), ("effort", "high")]),
        ["--model", "claude-sonnet-4-6", "--dangerously-skip-permissions"]
    );
    assert_eq!(
        args(&[("model", "gpt-oss-120b")]),
        ["--model", "gpt-oss-120b", "--effort", "medium", "--dangerously-skip-permissions"]
    );
}

#[test]
fn an_agy_model_saved_before_this_change_still_launches() {
    // `gemini-3.8-flash-high` was an option; now it is a typed model, which
    // agy takes as it is, and the effort is left to the user.
    let (_dir, registry) = shipped();
    let agy = registry.get("agy").expect("it ships");

    let values = agy
        .resolve(&Choices::new(), &chosen(&[("model", "gemini-3.8-flash-high")]))
        .expect("nothing chosen");

    assert_eq!(
        agy.launch_with("linux", &values).args,
        ["--model", "gemini-3.8-flash-high", "--dangerously-skip-permissions"]
    );
}

#[test]
fn every_shipped_option_has_a_clean_label() {
    let (_dir, registry) = shipped();

    for id in ["claude", "codex", "agy"] {
        let def = registry.get(id).expect("it ships");
        for setting in &def.settings {
            for option in setting.options() {
                let label = option.label.as_deref().unwrap_or_else(|| {
                    panic!("{id}'s {} option {:?} has no label", setting.key, option.value)
                });
                assert!(!label.contains('-') || label.starts_with("GPT"), "{id}: {label:?}");
            }
        }
    }
}

#[test]
fn the_picker_names_agys_choices_by_label() {
    let (_dir, registry) = shipped();
    let agy = registry.get("agy").expect("it ships");

    let values = agy
        .resolve(&chosen(&[("model", "gemini-3.1-pro")]), &Choices::new())
        .expect("fitted");

    assert_eq!(agy.describe_changes(&values), ["Gemini 3.1 Pro", "High"]);
}
```

(`GPT-OSS 120B`, `GPT-6 Astra` and `GPT-5.5` keep their hyphen: it is part of the product name, not a separator in an id.)

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -p dispatch-config -- agy_ every_shipped the_picker_names`
Expected: FAIL (agy has no effort row; options have no labels).

- [ ] **Step 4: Rewrite the shipped files' settings**

`agy.toml`: keep everything above the first comment line `# Each setting's args go after…` byte for byte (it holds the icon glyph). Replace from that comment to the end of the file with:

```toml
# Each setting's args go after the launch's own, with {value} replaced by
# what was chosen. Unset passes nothing and leaves the choice to agy.
#
# agy takes a base model and its effort apart, and each model has efforts of
# its own: a model's `effort` list is what the Effort row offers while it is
# chosen, the highest of them if the one set is not among them. A model with
# no list takes no --effort at all, and one typed in leaves the row free.
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = [
  { value = "gemini-3.8-flash", label = "Gemini 3.8 Flash", effort = ["low", "medium", "high"] },
  { value = "gemini-3.7-flash", label = "Gemini 3.7 Flash", effort = ["low", "medium", "high"] },
  { value = "gemini-3.6-flash", label = "Gemini 3.6 Flash", effort = ["low", "medium", "high"] },
  { value = "gemini-3.1-pro", label = "Gemini 3.1 Pro", effort = ["low", "high"] },
  { value = "gpt-oss-120b", label = "GPT-OSS 120B", effort = ["medium"] },
  { value = "claude-sonnet-4-6", label = "Claude Sonnet 4.6" },
  { value = "claude-opus-4-6-thinking", label = "Claude Opus 4.6 (Thinking)" },
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

[[settings]]
key = "bypass"
label = "Skip prompts"
kind = "bool"
default = true
args = ["--dangerously-skip-permissions"]
```

`claude.toml`: replace only the three `options = […]` lines:

```toml
options = [
  { value = "opus", label = "Opus" },
  { value = "sonnet", label = "Sonnet" },
  { value = "fable", label = "Fable" },
  { value = "haiku", label = "Haiku" },
]
```

```toml
options = [
  { value = "low", label = "Low" },
  { value = "medium", label = "Medium" },
  { value = "high", label = "High" },
  { value = "xhigh", label = "Extra high" },
  { value = "max", label = "Max" },
]
```

```toml
options = [
  { value = "bypassPermissions", label = "Bypass permissions" },
  { value = "auto", label = "Auto" },
  { value = "acceptEdits", label = "Accept edits" },
  { value = "plan", label = "Plan" },
  { value = "manual", label = "Manual" },
]
```

`codex.toml`: replace its two `options = […]` lines:

```toml
options = [
  { value = "gpt-6-astra", label = "GPT-6 Astra" },
  { value = "gpt-6-sol", label = "GPT-6 Sol" },
  { value = "gpt-6-luna", label = "GPT-6 Luna" },
  { value = "gpt-5.6-sol", label = "GPT-5.6 Sol" },
  { value = "gpt-5.6-terra", label = "GPT-5.6 Terra" },
  { value = "gpt-5.6-luna", label = "GPT-5.6 Luna" },
  { value = "gpt-5.5", label = "GPT-5.5" },
]
```

```toml
options = [
  { value = "low", label = "Low" },
  { value = "medium", label = "Medium" },
  { value = "high", label = "High" },
  { value = "xhigh", label = "Extra high" },
  { value = "max", label = "Max" },
  { value = "ultra", label = "Ultra" },
]
```

- [ ] **Step 5: Document it in the README**

In `README.md`, after the closing fence of the `claude.toml` settings example (the block ending `args = ["--permission-mode", "{value}"]`), and before the paragraph starting "A `bool` adds its `args`…", insert:

~~~markdown
An option can be a table instead, to show a clean name, and one choice can
limit another. While a model is chosen, a setting `limited_by` the model
offers only the values that model lists for it, and moves to the highest of
them when the one set is not among them. A model that lists none fades the
row out, and passes nothing for it. A model typed in by hand leaves it free.

```toml
# ~/.config/dispatch/harnesses/agy.toml
[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = [
  { value = "gemini-3.1-pro", label = "Gemini 3.1 Pro", effort = ["low", "high"] },
  { value = "claude-sonnet-4-6", label = "Claude Sonnet 4.6" },  # no effort
]
custom = true
args = ["--model", "{value}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
limited_by = "model"
options = [{ value = "low", label = "Low" }, { value = "high", label = "High" }]
args = ["--effort", "{value}"]
```
~~~

- [ ] **Step 6: Run the tests**

Run: `cargo test -p dispatch-config`
Expected: all pass, including `every_body_an_earlier_dispatch_wrote_is_upgraded`, `no_built_in_setting_or_option_is_dropped_on_load` (nothing shipped is dropped by the load checks) and `built_in_models_and_efforts_ship_unset`.

- [ ] **Step 7: Commit**

```bash
git add crates/dispatch-config README.md
git commit -m "feat(harnesses): clean option names, and agy's effort row limited by its model" --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" --trailer "Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2"
```

---

### Task 4: The popup shows labels, and follows limits

**Files:**
- Modify: `crates/dispatch-tui/src/settings_form.rs`
- Test: `crates/dispatch-tui/src/settings_form/tests.rs`

**Interfaces:**
- Consumes: `ChoiceOption::label`, `Allowed`, `allowed`, `fit_limits`, `SettingDef::limited_by` (Tasks 1 and 2).
- Produces: `pub const NOT_AVAILABLE: &str = "not available";` in `settings_form`. `SettingsForm::new` keeps its signature; `values()` always returns fitted values.

- [ ] **Step 1: Write the failing tests**

In `crates/dispatch-tui/src/settings_form/tests.rs`, change `settings()` to go through a new `parse`, and add the new helpers and tests:

```rust
fn parse(settings: &str) -> Vec<SettingDef> {
    let def: HarnessDef = toml::from_str(&format!(
        "id = \"demo\"\ndisplay_name = \"Demo\"\ncommand = \"demo\"\n{settings}"
    ))
    .expect("a valid harness");
    def.settings
}

fn settings() -> Vec<SettingDef> {
    parse(SETTINGS)
}

/// A model that decides which efforts may be chosen, and a flag after them.
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

[[settings]]
key = "bypass"
label = "Skip prompts"
kind = "bool"
default = true
args = ["--yolo"]
"#;

fn limited_with(values: &[(&str, &str)]) -> SettingsForm {
    let values: Choices = values
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    SettingsForm::new("New Demo pane", &parse(LIMITED), &values)
}

/// The style of the first cell of `text` where the popup draws it.
fn style_of(form: &SettingsForm, width: u16, height: u16, text: &str) -> Style {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    form.render(area, &mut buf);
    let len = u16::try_from(text.chars().count()).expect("short");
    for y in 0..height {
        for x in 0..width.saturating_sub(len) {
            let here: String = (x..x + len)
                .filter_map(|x| buf.cell((x, y)))
                .map(|cell| cell.symbol())
                .collect();
            if here == text {
                return buf.cell((x, y)).expect("in the buffer").style();
            }
        }
    }
    panic!("{text:?} is not drawn");
}

#[test]
fn a_choice_shows_its_options_by_label() {
    let form = limited_with(&[("model", "pro"), ("effort", "high")]);

    let drawn = render(&form, 60, 8);
    assert!(drawn.contains("◂ Pro ▸"), "{drawn}");
    assert!(drawn.contains("◂ High ▸"), "{drawn}");
}

#[test]
fn values_are_fitted_when_the_popup_opens() {
    assert_eq!(value(&limited_with(&[("model", "pro"), ("effort", "medium")]), "effort"), "high");
    assert_eq!(value(&limited_with(&[("model", "plain"), ("effort", "high")]), "effort"), "");
}

#[test]
fn a_limited_row_steps_only_through_what_the_model_offers() {
    let mut form = limited_with(&[("model", "pro"), ("effort", "high")]);
    press(&mut form, KeyCode::Down);

    press(&mut form, KeyCode::Right);
    assert_eq!(value(&form, "effort"), "low");
    press(&mut form, KeyCode::Right);
    assert_eq!(value(&form, "effort"), "high", "no medium, and no agent default");
    press(&mut form, KeyCode::Left);
    assert_eq!(value(&form, "effort"), "low");
}

#[test]
fn changing_the_model_refits_the_effort_at_once() {
    let mut form = limited_with(&[("model", "flash"), ("effort", "medium")]);

    press(&mut form, KeyCode::Right); // model: pro, which has no medium
    assert_eq!(value(&form, "model"), "pro");
    assert_eq!(value(&form, "effort"), "high");

    press(&mut form, KeyCode::Right); // model: plain, which has no effort
    assert_eq!(value(&form, "effort"), "");
}

#[test]
fn an_unavailable_row_reads_not_available_faded_and_is_skipped() {
    let mut form = limited_with(&[("model", "plain")]);

    assert!(render(&form, 60, 8).contains(NOT_AVAILABLE));
    assert_eq!(
        style_of(&form, 60, 8, NOT_AVAILABLE).fg,
        Some(Color::DarkGray),
        "drawn faded"
    );

    press(&mut form, KeyCode::Down);
    assert_eq!(form.selected_index(), 2, "down from the model skips the effort");
    press(&mut form, KeyCode::Up);
    assert_eq!(form.selected_index(), 0, "and so does up");
}

#[test]
fn a_typed_model_frees_the_effort() {
    let mut form = limited_with(&[("model", "big-1"), ("effort", "")]);
    press(&mut form, KeyCode::Down);

    for expected in ["low", "medium", "high", ""] {
        press(&mut form, KeyCode::Right);
        assert_eq!(value(&form, "effort"), expected);
    }
}

#[test]
fn stepping_off_a_typed_model_refits_the_effort() {
    let mut form = limited_with(&[("model", "big-1"), ("effort", "medium")]);
    press(&mut form, KeyCode::Right); // off the typed slot: wraps to agent default
    assert_eq!(value(&form, "model"), "");
    assert_eq!(value(&form, "effort"), "medium", "agent default limits nothing");

    press(&mut form, KeyCode::Left); // back onto typing
    assert!(form.is_editing());
    press(&mut form, KeyCode::Left); // drops the text, lands on plain
    assert_eq!(value(&form, "model"), "plain");
    assert_eq!(value(&form, "effort"), "");
}

#[test]
fn an_unavailable_first_row_is_not_selected_when_the_popup_opens() {
    // The effort row above the model row.
    let reordered = LIMITED
        .split("[[settings]]")
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
    let text = format!("[[settings]]{}[[settings]]{}[[settings]]{}", reordered[1], reordered[0], reordered[2]);
    let values: Choices = [("model".to_string(), "plain".to_string())].into_iter().collect();

    let form = SettingsForm::new("New Demo pane", &parse(&text), &values);

    assert_eq!(form.selected_index(), 1, "the model row, not the faded effort");
}
```

Add `use ratatui::style::{Color, Style};` to the test file's imports if `super::*` does not already bring them (it does: `settings_form.rs` imports both).

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p dispatch-tui settings_form`
Expected: compile error (`NOT_AVAILABLE`), then failures.

- [ ] **Step 3: Implement it in `settings_form.rs`**

Imports:

```rust
use dispatch_config::{
    Allowed, ChoiceOption, Choices, MAX_VALUE_CHARS, SAFE_CHARACTERS, SettingDef, SettingKind,
    allowed, fit_limits, is_safe_char,
};
```

After `AGENT_DEFAULT`:

```rust
/// What is shown for a setting the chosen value of another leaves nothing
/// to choose from.
pub const NOT_AVAILABLE: &str = "not available";
```

`Row` gains two fields, after `typed`:

```rust
    /// Whether another setting limits this one.
    limited: bool,
    /// What it may take while the others hold their values.
    allowed: Allowed,
```

and `Row::new` sets `limited: setting.limited_by.is_some(), allowed: Allowed::Free,`.

`Row::slots`, the choice arm becomes:

```rust
            Shape::Choice { options, custom } => match &self.allowed {
                Allowed::Unavailable => Vec::new(),
                Allowed::Only(only) => options
                    .iter()
                    .enumerate()
                    .filter(|(_, option)| only.contains(&option.value))
                    .map(|(index, _)| Slot::Option(index))
                    .collect(),
                Allowed::Free => {
                    let mut slots = vec![Slot::Default];
                    slots.extend((0..options.len()).map(Slot::Option));
                    if *custom {
                        slots.push(Slot::Typed);
                    }
                    slots
                }
            },
```

`Row::shown` becomes:

```rust
    /// The value as the popup shows it: an option by its label.
    fn shown(&self) -> String {
        match &self.shape {
            Shape::Flag if self.value == "true" => "on".to_string(),
            Shape::Flag => "off".to_string(),
            _ if self.allowed == Allowed::Unavailable => NOT_AVAILABLE.to_string(),
            _ if self.value.is_empty() => AGENT_DEFAULT.to_string(),
            Shape::Choice { options, .. } => options
                .iter()
                .find(|option| option.value == self.value)
                .map_or_else(|| format!("Custom: {}", self.value), |option| {
                    option.label().to_string()
                }),
            Shape::Text => self.value.clone(),
        }
    }
```

In `Row::widest`, the choice arm measures labels, and a limited row keeps room for `not available`:

```rust
            Shape::Choice { options, custom } => {
                let option = options
                    .iter()
                    .map(|option| option.label().chars().count())
                    .max()
                    .unwrap_or(0);
                let unavailable = if self.limited { NOT_AVAILABLE.len() } else { 0 };
                option
                    .max(AGENT_DEFAULT.len())
                    .max(unavailable)
                    .max(if *custom { typed } else { 0 })
            }
```

`SettingsForm` gains a field `settings: Vec<SettingDef>` (doc: "The settings the rows show, for working out what limits each one."). In `new`, build as now, then:

```rust
        let mut form = Self {
            title: title.into(),
            settings: settings.to_vec(),
            rows,
            selected: 0,
            editing: None,
            border: Style::default().fg(Color::Cyan),
            highlight: Style::default().bg(Color::DarkGray),
            faded: Style::default().fg(Color::DarkGray),
        };
        form.refit();
        if form
            .rows
            .first()
            .is_some_and(|row| row.allowed == Allowed::Unavailable)
        {
            form.move_row(true);
        }
        form
```

Add:

```rust
    /// Brings every limited row within what the others now allow, and
    /// notes what each may take.
    fn refit(&mut self) {
        let mut values = self.values();
        fit_limits(&self.settings, &mut values);
        for row in &mut self.rows {
            row.allowed = allowed(&self.settings, &row.key, &values);
            if let Some(value) = values.remove(&row.key) {
                row.value = value;
            }
        }
    }

    /// Moves to the next or previous row that has something to choose,
    /// wrapping.
    fn move_row(&mut self, forward: bool) {
        let count = self.rows.len();
        for _ in 0..count {
            self.selected = if forward {
                (self.selected + 1) % count
            } else {
                self.selected.checked_sub(1).unwrap_or(count - 1)
            };
            if self.rows[self.selected].allowed != Allowed::Unavailable {
                return;
            }
        }
    }
```

and make `previous_row` and `next_row` call `self.move_row(false)` and `self.move_row(true)` (the `count == 0` case: the `for` loop runs zero times, so nothing is indexed).

In `step`: return early when the row has nothing to step through, and refit after a value changes. After `let slots = row.slots();` add:

```rust
        if slots.is_empty() {
            return;
        }
```

change `row.value = options[index].clone();` to `row.value = options[index].value.clone();`, and after the closing brace of the `match slots[next] { … }` add `self.refit();`.

In `confirm` and `cancel`, add `self.refit();` as each one's last line.

In `render`, draw an unavailable row faded, with no arrows. Replace the lines from `write(buf, inner, inner.x + 1, y, &row.label, base);` to the end of the loop body with:

```rust
            if row.allowed == Allowed::Unavailable {
                let faded = base.patch(self.faded);
                write(buf, inner, inner.x + 1, y, &row.label, faded);
                write(buf, inner, value_x + 2, y, NOT_AVAILABLE, faded);
                continue;
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
```

(`value_x + 2` lines the text up with the values of the other rows, after where `◂ ` would be.)

- [ ] **Step 4: Run the tests**

Run: `cargo test -p dispatch-tui`
Expected: all pass, the existing popup tests too.

- [ ] **Step 5: Commit**

```bash
git add crates/dispatch-tui
git commit -m "feat(tui): show option labels, and step a limited row through what it may take" --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" --trailer "Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2"
```

---

### Task 5: The app sends fitted values and names them by label

**Files:**
- Modify: `dispatch/src/app.rs` (tests module only, around lines 11640-11960)

**Interfaces:**
- Consumes: `describe_changes` labels (Task 1), fitting in `resolve` (Task 2), the popup's fitted `values()` (Task 4).
- Produces: an `app_on(def, dir)` test helper; `app_on_demo` becomes a call to it.

No production code changes are expected: the picker detail and `Enter` already go through `resolve`, `describe_changes` and `form.values()`. If a test below fails for a reason in `app.rs`, fix it there and say so in the commit message.

- [ ] **Step 1: Write the tests**

Replace `app_on_demo` with a general helper and a wrapper:

```rust
    /// An attached app holding `def` and the user's shell, its settings
    /// kept in `dir` when one is given, with the new-pane picker open on
    /// `def`.
    fn app_on(
        def: dispatch_config::HarnessDef,
        dir: Option<&std::path::Path>,
    ) -> (App, Sender<ServerMessage>, Receiver<ClientMessage>) {
        let id = def.id.clone();
        let registry: HarnessRegistry = [
            def,
            dispatch_config::HarnessDef {
                id: SHELL.to_string(),
                display_name: "Shell".to_string(),
                ..dispatch_config::HarnessDef::default()
            },
        ]
        .into_iter()
        .collect();
        let (client, daemon, sent) = Client::for_test();
        let mut app = App::attached(registry, client);
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
        picker.select(&id);

        (app, daemon, sent)
    }

    /// [`app_on`] over `demo`, carrying [`DEMO_SETTINGS`].
    fn app_on_demo(
        dir: Option<&std::path::Path>,
    ) -> (App, Sender<ServerMessage>, Receiver<ClientMessage>) {
        app_on(with_settings("demo", "Demo", "demo"), dir)
    }
```

Then add:

```rust
    /// A harness `lim` whose Effort row follows its Model row, with labels.
    fn limited() -> dispatch_config::HarnessDef {
        toml::from_str(
            r#"
id = "lim"
display_name = "Lim"
command = "lim"

[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = [
  { value = "flash", label = "Flash", effort = ["low", "medium", "high"] },
  { value = "pro", label = "Pro", effort = ["low", "high"] },
  { value = "plain", label = "Plain" },
]
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
"#,
        )
        .expect("a valid harness")
    }

    #[test]
    fn the_picker_names_saved_options_by_label_fitted_to_the_model() {
        let dir = scratch("settings-labels");
        std::fs::write(
            dir.join("harness-settings.toml"),
            "[lim]\nmodel = \"pro\"\neffort = \"medium\"\n",
        )
        .expect("temp dir is writable");
        let (app, _daemon, _sent) = app_on(limited(), Some(&dir));

        let Some(Overlay::Harness(picker)) = &app.overlay else {
            panic!("the picker is open");
        };
        let lim = picker
            .items()
            .iter()
            .find(|item| item.id == "lim")
            .expect("lim is offered");
        assert_eq!(lim.detail.as_deref(), Some("lim · Pro · High"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn enter_in_the_settings_sends_an_effort_the_model_offers() {
        let (mut app, _daemon, sent) = app_on(limited(), None);
        press(&mut app, KeyCode::Char('e'));

        press(&mut app, KeyCode::Right); // model: flash, so effort: high
        press(&mut app, KeyCode::Enter);

        assert_eq!(
            sent_settings(&sent),
            Some(choices(&[("model", "flash"), ("effort", "high")]))
        );
    }

    #[test]
    fn enter_in_the_settings_sends_no_effort_for_a_model_with_none() {
        let (mut app, _daemon, sent) = app_on(limited(), None);
        press(&mut app, KeyCode::Char('e'));

        press(&mut app, KeyCode::Right); // flash
        press(&mut app, KeyCode::Right); // pro
        press(&mut app, KeyCode::Right); // plain
        press(&mut app, KeyCode::Enter);

        assert_eq!(
            sent_settings(&sent),
            Some(choices(&[("model", "plain"), ("effort", "")]))
        );
    }
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p dispatch --bin dispatch settings` (if the package's test target differs, `cargo test -p dispatch settings`)
Expected: all pass, the existing settings tests included.

- [ ] **Step 3: Commit**

```bash
git add dispatch/src/app.rs
git commit -m "test(app): the picker and the popup send and name limited settings by label" --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" --trailer "Claude-Session: https://claude.ai/code/session_01DAEKK1GGt15b7R1Hvn52s2"
```

---

### Task 6: Gates

- [ ] **Step 1: Run every gate**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
cargo +1.89 build --workspace --all-targets --locked
cargo test --workspace --no-fail-fast
```

Expected: all green. Fix anything that is not (run `cargo fmt --all` for formatting), and commit the fixes as `chore: satisfy fmt and clippy` with both trailers.
