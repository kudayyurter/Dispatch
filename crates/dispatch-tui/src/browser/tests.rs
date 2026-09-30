//! Tests for the directory browser.

use super::*;

/// A directory tree to browse, cleaned up when the test ends.
struct Tree(PathBuf);

impl Tree {
    /// Creates `dirs` under a directory of this test's own, marking any whose
    /// path ends in `+git` as a repository.
    fn new(label: &str, dirs: &[&str]) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);

        let root = std::env::temp_dir().join(format!(
            "dispatch-browser-{}-{label}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("temp dir is writable");

        for dir in dirs {
            let (dir, repo) = match dir.strip_suffix("+git") {
                Some(dir) => (dir, true),
                None => (*dir, false),
            };
            let path = root.join(dir);
            std::fs::create_dir_all(&path).expect("temp dir is writable");
            if repo {
                std::fs::create_dir_all(path.join(".git")).expect("temp dir is writable");
            }
        }

        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The labels the browser is offering, in order.
fn labels(browser: &Browser) -> Vec<String> {
    browser
        .visible()
        .iter()
        .map(|entry| entry.label.clone())
        .collect()
}

#[test]
fn a_directorys_subdirectories_are_offered_in_order() {
    let tree = Tree::new("list", &["beta", "alpha"]);
    let browser = Browser::new(tree.path());

    assert_eq!(labels(&browser), ["alpha", "beta"]);
}

#[test]
fn files_are_not_offered() {
    // A file is never a project, so offering one is a row that cannot be
    // chosen.
    let tree = Tree::new("files", &["alpha"]);
    std::fs::write(tree.path().join("notes.md"), "x").expect("temp dir is writable");

    let browser = Browser::new(tree.path());

    assert_eq!(labels(&browser), ["alpha"]);
}

#[test]
fn a_repository_is_marked_as_one() {
    let tree = Tree::new("repo", &["plain", "checkout+git"]);
    let browser = Browser::new(tree.path());

    let marked: Vec<(String, bool)> = browser
        .visible()
        .iter()
        .map(|entry| (entry.label.clone(), entry.repo))
        .collect();

    assert_eq!(
        marked,
        [("checkout".to_string(), true), ("plain".to_string(), false)]
    );
}

#[test]
fn typing_filters_the_listing() {
    let tree = Tree::new("filter", &["dispatch", "dispatch-notes", "other"]);
    let mut browser = Browser::new(tree.path());

    for c in "notes".chars() {
        browser.push(c);
    }

    assert_eq!(labels(&browser), ["dispatch-notes"]);
}

#[test]
fn a_filter_that_matches_nothing_offers_nothing() {
    let tree = Tree::new("filter-empty", &["alpha"]);
    let mut browser = Browser::new(tree.path());

    browser.push('z');

    assert!(browser.visible().is_empty());
    assert!(browser.selected().is_none(), "and nothing is chosen");
}

#[test]
fn a_dotted_directory_is_hidden_until_it_is_asked_for() {
    // Browsing is for the projects a user keeps, not for their dotfiles --
    // but typing the dot has to reach one.
    let tree = Tree::new("hidden", &[".config", "alpha"]);
    let mut browser = Browser::new(tree.path());

    assert_eq!(labels(&browser), ["alpha"]);

    browser.push('.');
    assert_eq!(labels(&browser), [".config"]);
}

#[test]
fn backspace_takes_the_filter_back() {
    let tree = Tree::new("backspace", &["alpha", "beta"]);
    let mut browser = Browser::new(tree.path());

    browser.push('l');
    assert_eq!(labels(&browser), ["alpha"], "'beta' has no l in it");

    browser.backspace();
    assert_eq!(labels(&browser), ["alpha", "beta"]);
}

#[test]
fn the_selection_walks_the_listing_and_stops_at_its_ends() {
    let tree = Tree::new("walk", &["alpha", "beta", "gamma"]);
    let mut browser = Browser::new(tree.path());

    assert_eq!(
        browser.selected().map(|e| e.label.clone()),
        Some("alpha".into())
    );

    browser.next();
    assert_eq!(
        browser.selected().map(|e| e.label.clone()),
        Some("beta".into())
    );

    browser.previous();
    browser.previous();
    assert_eq!(
        browser.selected().map(|e| e.label.clone()),
        Some("alpha".into()),
        "the top is the top"
    );

    for _ in 0..5 {
        browser.next();
    }
    assert_eq!(
        browser.selected().map(|e| e.label.clone()),
        Some("gamma".into()),
        "and the bottom is the bottom"
    );
}

#[test]
fn descending_lists_the_chosen_directory_and_clears_the_filter() {
    let tree = Tree::new("descend", &["outer/inner"]);
    let mut browser = Browser::new(tree.path());
    browser.push('o');

    browser.descend();

    assert_eq!(browser.dir(), tree.path().join("outer"));
    assert_eq!(labels(&browser), ["inner"]);
    assert_eq!(
        browser.input(),
        "",
        "a filter belongs to the directory it filtered"
    );
}

#[test]
fn ascending_goes_to_the_parent_and_sits_on_where_it_came_from() {
    // Walking up and finding the cursor on some unrelated row is how you lose
    // your place in a deep tree.
    let tree = Tree::new("ascend", &["alpha", "outer/inner"]);
    let mut browser = Browser::new(&tree.path().join("outer"));

    browser.ascend();

    assert_eq!(browser.dir(), tree.path());
    assert_eq!(
        browser.selected().map(|e| e.label.clone()),
        Some("outer".into())
    );
}

#[test]
fn ascending_from_the_root_stays_there() {
    let mut browser = Browser::new(Path::new("/"));

    browser.ascend();

    assert_eq!(browser.dir(), Path::new("/"));
}

#[test]
fn a_typed_path_is_a_path_rather_than_a_filter() {
    let tree = Tree::new("typed", &["outer/inner"]);
    let mut browser = Browser::new(tree.path());

    for c in tree.path().join("outer").to_string_lossy().chars() {
        browser.push(c);
    }
    assert!(browser.is_path(), "a separator makes it a path");

    assert!(browser.jump(), "the directory exists");
    assert_eq!(browser.dir(), tree.path().join("outer"));
    assert_eq!(labels(&browser), ["inner"]);
}

#[test]
fn a_typed_path_that_is_not_there_is_refused_and_left_to_edit() {
    let tree = Tree::new("typed-missing", &["alpha"]);
    let mut browser = Browser::new(tree.path());

    for c in "/nowhere/at/all".chars() {
        browser.push(c);
    }

    assert!(!browser.jump());
    assert_eq!(browser.dir(), tree.path(), "it stays where it was");
    assert_eq!(
        browser.input(),
        "/nowhere/at/all",
        "with the path still typed"
    );
}

#[test]
fn completing_a_typed_path_fills_in_the_rest_of_it() {
    let tree = Tree::new("complete", &["outer"]);
    let mut browser = Browser::new(tree.path());

    // Joined rather than formatted with a literal `/`: `complete` fills in
    // the rest through `Path::display`, which uses the platform's own
    // separator, so the expectation has to be built the same way or the two
    // strings only agree on Unix.
    let typed = tree.path().join("out").display().to_string();
    for c in typed.chars() {
        browser.push(c);
    }
    browser.complete();

    assert_eq!(
        browser.input(),
        tree.path().join("outer").display().to_string(),
        "one candidate completes"
    );
}

#[test]
fn a_scan_finds_the_repositories_under_a_directory() {
    // The point of the scan: ~/DEV gives every checkout at once rather than
    // one descent per project.
    let tree = Tree::new(
        "scan",
        &[
            "work/one+git",
            "work/two/nested+git",
            "work/plain",
            "solo+git",
        ],
    );
    let mut browser = Browser::new(tree.path());

    browser.toggle_scan();

    let mut found: Vec<String> = browser.visible().iter().map(|e| e.label.clone()).collect();
    found.sort();

    assert_eq!(found, ["nested", "one", "solo"]);
    assert!(
        browser.visible().iter().all(|entry| entry.repo),
        "a scan finds repositories and nothing else"
    );
}

#[test]
fn a_scan_is_turned_off_again() {
    let tree = Tree::new("scan-off", &["work/one+git", "plain"]);
    let mut browser = Browser::new(tree.path());

    browser.toggle_scan();
    browser.toggle_scan();

    assert_eq!(labels(&browser), ["plain", "work"]);
}

/// Renders the browser over `width` by `height` and returns its rows.
fn render(browser: &Browser, width: u16, height: u16) -> Vec<String> {
    use ratatui::widgets::Widget;

    let area = ratatui::layout::Rect::new(0, 0, width, height);
    let mut buf = ratatui::buffer::Buffer::empty(area);
    browser.render(area, &mut buf);

    (0..height)
        .map(|y| {
            (0..width)
                .filter_map(|x| buf.cell((x, y)))
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[test]
fn the_browser_shows_where_it_is_and_what_is_in_it() {
    let tree = Tree::new("render", &["alpha", "checkout+git"]);
    let browser = Browser::new(tree.path());

    let rows = render(&browser, 160, 12).join("\n");

    assert!(
        rows.contains("Open project"),
        "it says what it is for: {rows}"
    );
    assert!(
        rows.contains(&tree.path().display().to_string()),
        "and where it is: {rows}"
    );
    assert!(rows.contains("alpha"), "{rows}");
    assert!(rows.contains("checkout"), "{rows}");
}

#[test]
fn a_repository_is_drawn_with_its_git_mark() {
    let tree = Tree::new("render-repo", &["plain", "checkout+git"]);
    let browser = Browser::new(tree.path());

    let rows = render(&browser, 60, 12);
    let repo = rows
        .iter()
        .find(|row| row.contains("checkout"))
        .expect("the repository has a row");
    let plain = rows
        .iter()
        .find(|row| row.contains("plain"))
        .expect("the plain directory has a row");

    assert!(repo.contains(REPOSITORY), "{repo:?}");
    assert!(!plain.contains(REPOSITORY), "{plain:?}");
}

#[test]
fn what_is_typed_is_drawn_where_it_was_typed() {
    let tree = Tree::new("render-input", &["alpha"]);
    let mut browser = Browser::new(tree.path());
    browser.push('a');
    browser.push('l');

    let rows = render(&browser, 60, 12).join("\n");

    assert!(rows.contains("al"), "the filter is visible: {rows}");
}

#[test]
fn the_row_the_user_is_on_is_highlighted() {
    let tree = Tree::new("render-selected", &["alpha", "beta"]);
    let mut browser = Browser::new(tree.path());
    browser.next();

    use ratatui::widgets::Widget;
    let area = ratatui::layout::Rect::new(0, 0, 60, 12);
    let mut buf = ratatui::buffer::Buffer::empty(area);
    browser.render(area, &mut buf);

    let row_of = |needle: &str| {
        (0..area.height).find(|y| {
            (0..area.width)
                .filter_map(|x| buf.cell((x, *y)))
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .contains(needle)
        })
    };

    let beta = row_of("beta").expect("beta has a row");
    let alpha = row_of("alpha").expect("alpha has a row");

    let bar = crate::theme::Chrome::default().selection.bg;
    let reversed =
        |y: u16| (0..area.width).any(|x| Some(buf.cell((x, y)).expect("cell exists").bg) == bar);

    assert!(reversed(beta), "the selected row is marked");
    assert!(!reversed(alpha), "and the others are not");
}

#[test]
fn a_scan_says_that_is_what_it_is_showing() {
    // The listing is suddenly of paths from elsewhere in the tree; without a
    // word for it, that reads as the directory having changed under you.
    let tree = Tree::new("render-scan", &["work/one+git"]);
    let mut browser = Browser::new(tree.path());
    browser.toggle_scan();

    let rows = render(&browser, 60, 12).join("\n");

    assert!(rows.to_lowercase().contains("repositories"), "{rows}");
}

#[test]
fn a_path_too_long_to_show_keeps_its_end() {
    // The end of a path says which directory it is; its start is what every
    // path under one home directory has in common.
    let tree = Tree::new("render-long-path", &["alpha"]);
    let browser = Browser::new(tree.path());

    let rows = render(&browser, 30, 12);
    let header = rows
        .iter()
        .find(|row| row.contains('…'))
        .unwrap_or_else(|| panic!("the path is cut: {rows:#?}"));

    let path = tree.path().display().to_string();
    let tail: String = path.chars().skip(path.chars().count() - 6).collect();

    assert!(
        header.contains(&tail),
        "{header:?} keeps the end of the path"
    );
}

#[test]
fn the_browser_draws_only_in_its_chrome() {
    let tree = Tree::new("chrome", &["alpha", "beta+git"]);
    let mut browser = Browser::new(tree.path());
    browser.set_chrome(crate::theme::loud_chrome());
    let area = Rect::new(0, 0, 60, 16);
    let mut buf = Buffer::empty(area);
    (&browser).render(area, &mut buf);

    crate::theme::assert_no_fixed_colours(&buf);
}

fn open_cancel() -> Vec<crate::button::Button> {
    use crate::button::{Button, ButtonId};
    vec![
        Button {
            id: ButtonId::Cancel,
            label: "Cancel",
            default: false,
        },
        Button {
            id: ButtonId::Open,
            label: "Open",
            default: true,
        },
    ]
}

/// Renders into a buffer, for reading back cells.
fn buffer(browser: &Browser, width: u16, height: u16) -> ratatui::buffer::Buffer {
    use ratatui::widgets::Widget;
    let area = ratatui::layout::Rect::new(0, 0, width, height);
    let mut buf = ratatui::buffer::Buffer::empty(area);
    browser.render(area, &mut buf);
    buf
}

fn drawn_in(buf: &ratatui::buffer::Buffer, rect: ratatui::layout::Rect) -> String {
    (rect.x..rect.right())
        .filter_map(|x| buf.cell((x, rect.y)))
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}

#[test]
fn buttons_are_laid_out_and_drawn_below_the_list_and_the_keys() {
    let tree = Tree::new("buttons", &["alpha", "beta"]);
    let browser = Browser::new(tree.path()).with_buttons(open_cancel());
    let area = ratatui::layout::Rect::new(0, 0, 100, 24);
    let layout = browser.layout(area);
    let buf = buffer(&browser, 100, 24);

    assert_eq!(layout.buttons.len(), 2);
    let foot = layout.buttons[0].0.y;
    for (rect, id) in &layout.buttons {
        let label = if *id == crate::button::ButtonId::Open {
            "[ Open ]"
        } else {
            "[ Cancel ]"
        };
        assert_eq!(drawn_in(&buf, *rect), label);
    }
    assert!(layout.rows.iter().all(|(row, _)| row.y < foot - 1));
    let keys = drawn_in(
        &buf,
        ratatui::layout::Rect::new(layout.rect.x + 1, foot - 1, layout.rect.width - 2, 1),
    );
    assert!(keys.contains("⏎ open"), "the keys stay above: {keys}");
}

#[test]
fn a_browser_without_buttons_keeps_its_listing_rows() {
    let tree = Tree::new("no-buttons", &["alpha", "beta", "gamma", "delta", "eps"]);
    let area = ratatui::layout::Rect::new(0, 0, 100, 12);
    let bare = Browser::new(tree.path()).layout(area);
    let with = Browser::new(tree.path())
        .with_buttons(open_cancel())
        .layout(area);

    assert!(bare.buttons.is_empty());
    assert_eq!(bare.rect, with.rect);
    assert_eq!(bare.rows.len(), with.rows.len() + 1);
}

#[test]
fn buttons_that_do_not_fit_are_dropped_and_the_listing_keeps_its_rows() {
    let tree = Tree::new("no-room", &["alpha", "beta"]);
    let area = ratatui::layout::Rect::new(0, 0, 30, 9);
    let bare = Browser::new(tree.path()).layout(area);
    let with = Browser::new(tree.path())
        .with_buttons(open_cancel())
        .layout(area);

    assert!(with.buttons.is_empty());
    assert_eq!(with.rows, bare.rows);
}

#[test]
fn rows_are_where_each_visible_entry_is_drawn() {
    let tree = Tree::new("rows", &["alpha", "beta"]);
    let browser = Browser::new(tree.path()).with_buttons(open_cancel());
    let layout = browser.layout(ratatui::layout::Rect::new(0, 0, 100, 24));
    let buf = buffer(&browser, 100, 24);

    assert_eq!(layout.rows.len(), 2);
    for ((rect, index), label) in layout.rows.iter().zip(["alpha", "beta"]) {
        assert_eq!(rect.height, 1);
        assert!(drawn_in(&buf, *rect).contains(label));
        assert_eq!(labels(&browser)[*index], label);
    }
}

#[test]
fn selecting_a_visible_entry_moves_to_it_and_ignores_the_out_of_range() {
    let tree = Tree::new("select", &["alpha", "beta"]);
    let mut browser = Browser::new(tree.path());
    browser.select_visible(1);
    assert_eq!(browser.selected().map(|e| e.label.as_str()), Some("beta"));
    browser.select_visible(7);
    assert_eq!(browser.selected().map(|e| e.label.as_str()), Some("beta"));
}

#[test]
fn hover_underlines_an_entry_and_leaves_the_chosen_one_alone() {
    let tree = Tree::new("hover", &["alpha", "beta"]);
    let mut browser = Browser::new(tree.path());
    browser.set_hovered(Some(1));
    let layout = browser.layout(ratatui::layout::Rect::new(0, 0, 100, 24));
    let buf = buffer(&browser, 100, 24);
    let underlined = |rect: ratatui::layout::Rect| {
        buf.cell((rect.x + 2, rect.y))
            .unwrap()
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    };

    assert!(underlined(layout.rows[1].0));
    assert!(!underlined(layout.rows[0].0));
}

#[test]
fn a_browser_with_buttons_draws_at_any_size_without_panicking() {
    let tree = Tree::new("sizes", &["alpha", "beta"]);
    let browser = Browser::new(tree.path()).with_buttons(open_cancel());
    for width in 1..=30 {
        for height in 1..=10 {
            render(&browser, width, height);
        }
    }
}

#[test]
fn clicking_a_visible_row_of_a_scrolled_listing_does_not_move_the_listing() {
    let names: Vec<String> = (0..20).map(|i| format!("dir-{i:02}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let tree = Tree::new("scrolled-click", &names);
    let mut browser = Browser::new(tree.path());
    let area = ratatui::layout::Rect::new(0, 0, 60, 12);
    for _ in 0..15 {
        browser.next();
    }
    let before = browser.layout(area).rows;
    let (rect, index) = before[1];

    browser.select_visible(index);

    assert_eq!(browser.layout(area).rows, before);
    let buf = buffer(&browser, 60, 12);
    assert!(drawn_in(&buf, rect).contains(&format!("dir-{index:02}")));
}
