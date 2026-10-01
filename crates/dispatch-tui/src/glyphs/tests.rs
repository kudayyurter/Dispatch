use super::*;

#[test]
fn the_plain_set_is_ascii_only() {
    let plain = &Glyphs::PLAIN;
    for glyph in [
        plain.starting,
        plain.running,
        plain.idle,
        plain.blocked,
        plain.done,
        plain.failed,
        plain.closed,
        plain.unseen,
        plain.open,
        plain.shut,
        plain.leaf,
        plain.repository,
        plain.folder,
        plain.default_icon,
    ] {
        assert!(glyph.is_ascii(), "{glyph:?}");
        assert_eq!(glyph.chars().count(), 1, "{glyph:?}");
    }
}

#[test]
fn the_nerd_set_is_what_the_sidebar_drew_before() {
    assert_eq!(Glyphs::NERD.blocked, crate::sidebar::BLOCKED);
    assert_eq!(Glyphs::NERD.repository, crate::sidebar::REPOSITORY);
}

#[test]
fn a_plain_harness_icon_is_its_first_letter() {
    assert_eq!(Glyphs::PLAIN.harness_icon(None, "claude code"), "C");
    assert_eq!(
        Glyphs::PLAIN.harness_icon(None, ""),
        Glyphs::PLAIN.default_icon
    );
}
