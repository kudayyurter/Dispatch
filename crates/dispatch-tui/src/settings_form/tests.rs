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
    assert_eq!(
        value(
            &limited_with(&[("model", "pro"), ("effort", "medium")]),
            "effort"
        ),
        "high"
    );
    assert_eq!(
        value(
            &limited_with(&[("model", "plain"), ("effort", "high")]),
            "effort"
        ),
        ""
    );
}

#[test]
fn a_limited_row_steps_only_through_what_the_model_offers() {
    let mut form = limited_with(&[("model", "pro"), ("effort", "high")]);
    press(&mut form, KeyCode::Down);

    press(&mut form, KeyCode::Right);
    assert_eq!(value(&form, "effort"), "low");
    press(&mut form, KeyCode::Right);
    assert_eq!(
        value(&form, "effort"),
        "high",
        "no medium, and no agent default"
    );
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
        crate::theme::Chrome::default().secondary.fg,
        "drawn faded"
    );

    press(&mut form, KeyCode::Down);
    assert_eq!(
        form.selected_index(),
        2,
        "down from the model skips the effort"
    );
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
    assert_eq!(
        value(&form, "effort"),
        "medium",
        "agent default limits nothing"
    );

    press(&mut form, KeyCode::Left); // back onto typing
    assert!(form.is_editing());
    press(&mut form, KeyCode::Left); // drops the text, lands on plain
    assert_eq!(value(&form, "model"), "plain");
    assert_eq!(value(&form, "effort"), "");
}

#[test]
fn an_unavailable_first_row_is_not_selected_when_the_popup_opens() {
    // The effort row above the model row.
    let parts: Vec<&str> = LIMITED
        .split("[[settings]]")
        .filter(|part| !part.trim().is_empty())
        .collect();
    let text = format!(
        "[[settings]]{}[[settings]]{}[[settings]]{}",
        parts[1], parts[0], parts[2]
    );
    let values: Choices = [("model".to_string(), "plain".to_string())]
        .into_iter()
        .collect();

    let form = SettingsForm::new("New Demo pane", &parse(&text), &values);

    assert_eq!(
        form.selected_index(),
        1,
        "the model row, not the faded effort"
    );
}

#[test]
fn the_settings_form_draws_only_in_its_chrome() {
    let mut form = form();
    form.set_chrome(crate::theme::loud_chrome());
    let area = Rect::new(0, 0, 60, 12);
    let mut buf = Buffer::empty(area);
    (&form).render(area, &mut buf);

    crate::theme::assert_no_fixed_colours(&buf);
}

fn buttons() -> Vec<crate::button::Button> {
    use crate::button::{Button, ButtonId};
    vec![
        Button {
            id: ButtonId::Cancel,
            label: "Cancel",
            default: false,
        },
        Button {
            id: ButtonId::SaveDefault,
            label: "Save as default",
            default: false,
        },
        Button {
            id: ButtonId::OpenPane,
            label: "Open pane",
            default: true,
        },
    ]
}

fn buffer(form: &SettingsForm, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    form.render(area, &mut buf);
    buf
}

fn drawn_in(buf: &Buffer, rect: Rect) -> String {
    (rect.x..rect.right())
        .filter_map(|x| buf.cell((x, rect.y)))
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn buttons_are_laid_out_and_drawn_below_the_rows() {
    let form = form().with_buttons(buttons());
    let layout = form.layout(Rect::new(0, 0, 100, 20));
    let buf = buffer(&form, 100, 20);

    assert_eq!(layout.buttons.len(), 3);
    let foot = layout.buttons[0].0.y;
    assert!(layout.rows.iter().all(|(row, _)| row.y < foot));
    assert_eq!(layout.rows.len(), 3);
    let labels: Vec<String> = layout.buttons.iter().map(|b| drawn_in(&buf, b.0)).collect();
    assert_eq!(
        labels,
        ["[ Cancel ]", "[ Save as default ]", "[ Open pane ]"]
    );
}

#[test]
fn buttons_that_do_not_fit_are_dropped_and_the_box_keeps_its_old_shape() {
    let bare = form();
    let with = form().with_buttons(buttons());
    // Room for the rows and the border and no more.
    let area = Rect::new(0, 0, 100, 5);
    assert!(with.layout(area).buttons.is_empty());
    assert_eq!(with.layout(area).rect, bare.layout(area).rect);
    assert_eq!(with.layout(area).rows, bare.layout(area).rows);
}

#[test]
fn rows_are_where_each_row_is_drawn() {
    let form = form().with_buttons(buttons());
    let layout = form.layout(Rect::new(0, 0, 100, 20));
    let buf = buffer(&form, 100, 20);

    for ((rect, index), label) in layout.rows.iter().zip(["Model", "Effort", "Skip prompts"]) {
        assert!(drawn_in(&buf, *rect).contains(label), "{label} at {index}");
    }
}

#[test]
fn steps_sit_on_the_arrows_either_side_of_each_value() {
    let form = form();
    let layout = form.layout(Rect::new(0, 0, 100, 20));
    let buf = buffer(&form, 100, 20);

    assert_eq!(layout.steps.len(), 6);
    for (rect, index, forward) in &layout.steps {
        assert_eq!(rect.width, 2);
        let cells = drawn_in(&buf, *rect);
        let arrow = if *forward { "▸" } else { "◂" };
        assert!(cells.contains(arrow), "row {index} {forward}: {cells:?}");
    }
}

#[test]
fn clicking_a_row_selects_it_and_its_arrows_step_it() {
    let mut form = form();
    form.select_row(1);
    assert_eq!(form.selected_index(), 1);

    assert_eq!(form.step_row(1, true), FormAction::None);
    assert_eq!(value(&form, "effort"), "low");
    form.step_row(1, false);
    assert_eq!(value(&form, "effort"), "");

    // Stepping another row moves to it, as a click on its arrow does.
    form.step_row(2, true);
    assert_eq!(form.selected_index(), 2);
    assert_eq!(value(&form, "bypass"), "false");

    form.select_row(99);
    assert_eq!(form.selected_index(), 2);
}

#[test]
fn a_row_with_nothing_to_choose_is_not_selected_or_stepped() {
    let mut form = limited_with(&[("model", "plain")]);
    let before = form.values();
    let layout = form.layout(Rect::new(0, 0, 100, 20));
    let unavailable: Vec<usize> = layout
        .rows
        .iter()
        .map(|r| r.1)
        .filter(|i| layout.steps.iter().all(|s| s.1 != *i))
        .collect();

    assert!(
        !unavailable.is_empty(),
        "the effort row has nothing to choose"
    );
    for index in unavailable {
        form.step_row(index, true);
        assert_ne!(form.selected_index(), index);
    }
    assert_eq!(form.values(), before);
}

#[test]
fn hover_underlines_a_row_and_leaves_the_chosen_one_alone() {
    let mut form = form();
    form.set_hovered(Some(1));
    let layout = form.layout(Rect::new(0, 0, 100, 20));
    let buf = buffer(&form, 100, 20);
    let underlined = |rect: Rect| {
        buf.cell((rect.x + 1, rect.y))
            .unwrap()
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    };

    assert!(underlined(layout.rows[1].0));
    assert!(!underlined(layout.rows[0].0));
}

#[test]
fn a_form_with_buttons_draws_at_any_size_without_panicking() {
    let form = form().with_buttons(buttons());
    for width in 1..=30 {
        for height in 1..=10 {
            render(&form, width, height);
            let _ = form.layout(Rect::new(0, 0, width, height));
        }
    }
}
