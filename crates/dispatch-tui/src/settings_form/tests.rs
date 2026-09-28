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
fn the_box_grows_to_fit_a_long_typed_value() {
    let mut form = form();
    press(&mut form, KeyCode::Left);
    for c in "openrouter/anthropic/claude-sonnet-4.5".chars() {
        press(&mut form, KeyCode::Char(c));
    }

    let text = render(&form, 80, 10);

    assert!(
        text.contains("Custom: openrouter/anthropic/claude-sonnet-4.5▏"),
        "{text}"
    );
}

#[test]
fn a_popup_bigger_than_the_screen_is_clipped_not_a_panic() {
    let long = "a".repeat(dispatch_config::MAX_VALUE_CHARS);
    let form = form_with(&[("model", long.as_str())]);

    for (width, height) in [(80, 10), (30, 4), (20, 3), (8, 3), (5, 2), (1, 1)] {
        let _ = render(&form, width, height);
    }
}
