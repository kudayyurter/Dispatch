use super::*;
use crate::testing::TempDir;

#[test]
fn absent_keys_inherit_and_present_keys_win() {
    let config = crate::InterfaceConfig {
        motion: false,
        ..Default::default()
    };
    let present = crate::InterfaceKeysPresent {
        motion: true,
        ..Default::default()
    };
    let mut prefs = Preferences::default();
    let now = effective(&config, &present, &prefs);
    assert_eq!(
        now.motion,
        Sourced {
            value: false,
            source: Source::ConfigFile
        }
    );
    assert_eq!(now.focus_follows_pointer.source, Source::BuiltIn);
    assert_eq!(
        now.theme,
        Sourced {
            value: ThemeChoice::Terminal,
            source: Source::BuiltIn
        }
    );

    prefs.interface.motion = Some(true);
    prefs.appearance.theme = Some(ThemeChoice::Light);
    let now = effective(&config, &present, &prefs);
    assert_eq!(
        now.motion,
        Sourced {
            value: true,
            source: Source::Preferences
        }
    );
    assert_eq!(now.theme.value, ThemeChoice::Light);
}

#[test]
fn values_round_trip_through_the_file() {
    let dir = TempDir::new("prefs-round-trip");
    let base = Preferences::default();
    let mut draft = base.clone();
    draft.appearance.accent = Some(Accent::Custom(Rgb8(0x12, 0x34, 0x56)));
    draft.appearance.icons = Some(IconSet::Plain);
    apply(dir.path(), Section::Appearance, &base, &draft).expect("applied");
    let text = std::fs::read_to_string(dir.path().join(FILE)).expect("written");
    assert!(text.contains("accent = \"#123456\""), "{text}");
    assert_eq!(load(dir.path()).expect("loads"), draft);
}

#[test]
fn one_bad_value_costs_only_itself() {
    let dir = TempDir::new("prefs-bad");
    std::fs::write(
        dir.path().join(FILE),
        "[appearance]\naccent = \"chartreuse\"\ntheme = \"light\"\nicons = 3\n",
    )
    .expect("written");
    let prefs = load_or_default(dir.path());
    assert_eq!(prefs.appearance.theme, Some(ThemeChoice::Light));
    assert_eq!(prefs.appearance.accent, None);
    assert_eq!(prefs.appearance.icons, None);
}

#[test]
fn applying_one_section_leaves_the_other_alone() {
    let dir = TempDir::new("prefs-sections");
    std::fs::write(
        dir.path().join(FILE),
        "[interface]\nhover_claims_panes = true\n",
    )
    .expect("written");
    let base = load(dir.path()).expect("loads");
    let mut draft = base.clone();
    draft.appearance.theme = Some(ThemeChoice::Dark);
    apply(dir.path(), Section::Appearance, &base, &draft).expect("applied");
    let now = load(dir.path()).expect("loads");
    assert_eq!(now.interface.hover_claims_panes, Some(true));
    assert_eq!(now.appearance.theme, Some(ThemeChoice::Dark));
}

#[test]
fn a_field_changed_on_disk_since_opening_is_a_conflict_and_nothing_is_written() {
    let dir = TempDir::new("prefs-conflict");
    let base = Preferences::default();
    // Another Dispatch sets the theme after Settings opened.
    let mut other = base.clone();
    other.appearance.theme = Some(ThemeChoice::Light);
    apply(dir.path(), Section::Appearance, &base, &other).expect("the other applied");

    let mut draft = base.clone();
    draft.appearance.theme = Some(ThemeChoice::Dark);
    draft.appearance.icons = Some(IconSet::Plain);
    let result = apply(dir.path(), Section::Appearance, &base, &draft);
    assert!(
        matches!(result, Err(ApplyError::Conflict(ref fields)) if fields == &vec!["theme"]),
        "{result:?}"
    );
    let now = load(dir.path()).expect("loads");
    assert_eq!(
        now.appearance.theme,
        Some(ThemeChoice::Light),
        "theirs kept"
    );
    assert_eq!(now.appearance.icons, None, "nothing of ours written");
}

#[test]
fn an_unreadable_file_is_never_replaced() {
    let dir = TempDir::new("prefs-unreadable");
    std::fs::write(dir.path().join(FILE), "this is [ not toml").expect("written");
    let draft = Preferences {
        appearance: AppearancePrefs {
            theme: Some(ThemeChoice::Dark),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = apply(
        dir.path(),
        Section::Appearance,
        &Preferences::default(),
        &draft,
    );
    assert!(
        matches!(result, Err(ApplyError::Unreadable(_))),
        "{result:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join(FILE)).expect("still there"),
        "this is [ not toml"
    );
}

#[test]
fn resetting_a_field_removes_it_from_the_file() {
    let dir = TempDir::new("prefs-reset");
    std::fs::write(dir.path().join(FILE), "[appearance]\ntheme = \"dark\"\n").expect("written");
    let base = load(dir.path()).expect("loads");
    let mut draft = base.clone();
    draft.appearance.theme = None;
    apply(dir.path(), Section::Appearance, &base, &draft).expect("applied");
    assert!(
        !std::fs::read_to_string(dir.path().join(FILE))
            .expect("there")
            .contains("theme")
    );
}

#[test]
fn accents_parse_presets_and_hex_and_reject_the_rest() {
    let parse = |text: &str| Accent::parse(text);
    assert_eq!(parse("terminal"), Some(Accent::Terminal));
    assert_eq!(parse("violet"), Some(Accent::Violet));
    assert_eq!(
        parse("#AbCdEf"),
        Some(Accent::Custom(Rgb8(0xab, 0xcd, 0xef)))
    );
    assert_eq!(parse("#abc"), None);
    assert_eq!(parse("#12345g"), None);
    assert_eq!(parse("abcdef"), None);
    assert_eq!(parse("chartreuse"), None);
}
