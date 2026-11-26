//! Black-box TUI-driver coverage for the two widgets vimcode paints through
//! quadraui's widget contract as `ListView` and `TabBar`: an editor tab bar
//! with more tabs than fit, and keyboard navigation of the welcome panel's
//! recent-folders list.
//!
//! Every assertion reads what the running `App` painted — the screen text,
//! cell styles, or the painted-widget record `TuiDriver::tab_center` /
//! `tab_close_center` resolve against — never a layout recomputed here.

use quadraui::tui::testing::TuiDriver;
use quadraui::WidgetId;

use crate::render::EDITOR_TAB_BAR_WIDGET_ID;

fn harness(
    engine: crate::core::Engine,
    cols: u16,
) -> crate::harness::ConformanceHarness<TuiDriver<impl quadraui::AppLogic>> {
    crate::tui_main::testing::conformance_harness(engine, cols, 24)
}

fn editor_tab_bar() -> WidgetId {
    WidgetId::new(EDITOR_TAB_BAR_WIDGET_ID)
}

fn active_tab(engine: &crate::core::Engine) -> (usize, usize) {
    let group = &engine.editor_groups[&engine.active_group];
    (group.active_tab, group.tabs.len())
}

const OVERFLOW_TABS: usize = 8;

fn overflow_tab_name(i: usize) -> String {
    format!("tabovf_{i:02}_long_name.txt")
}

/// One group holding [`OVERFLOW_TABS`] file tabs whose labels together are
/// far wider than an 80-column tab bar, with the last one active.
fn engine_with_overflowing_tab_bar() -> crate::core::Engine {
    let dir = crate::harness::scratch_dir("vimcode_tab_overflow");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture must create its scratch dir");
    let mut engine = crate::core::Engine::new_for_test();
    engine.settings.use_nerd_fonts = Some(false);
    for i in 0..OVERFLOW_TABS {
        let path = dir.join(overflow_tab_name(i));
        std::fs::write(&path, format!("contents of tab {i}\n")).unwrap();
        if i == 0 {
            engine
                .open_file_with_mode(&path, crate::core::engine::OpenMode::Permanent)
                .unwrap();
        } else {
            engine.new_tab(Some(&path));
        }
    }
    engine
}

/// Settle the frame the way a live session does: a paint reports the tab
/// bar's real width, the idle tick after it hands that width to the engine
/// (which scrolls the active tab into view), and the next paint shows it.
fn settle(driver: &mut TuiDriver<impl quadraui::AppLogic>) {
    driver.dispatch(quadraui::UiEvent::WindowFocused(true));
    driver.render();
    driver.tick();
    driver.render();
}

/// The tab bar overflows: the active (last) tab is scrolled into view and
/// the first tab is scrolled out, so neither its label nor its painted hit
/// part exists. Clicking a painted tab activates it, clicking its painted
/// close glyph closes it, and moving to the first tab scrolls the bar back.
#[test]
fn overflowing_tab_bar_scrolls_clicks_and_closes_through_the_painted_widget() {
    let mut h = harness(engine_with_overflowing_tab_bar(), 80);
    settle(&mut h.driver);
    let bar = editor_tab_bar();
    let last = OVERFLOW_TABS - 1;

    assert_eq!(active_tab(&h.engine.borrow()), (last, OVERFLOW_TABS));
    let screen = h.driver.screen();
    assert!(
        screen.contains(&overflow_tab_name(last)),
        "the active tab must be scrolled into view; screen:\n{screen}"
    );
    assert!(
        !screen.contains(&overflow_tab_name(0)),
        "eight long tabs cannot fit in 80 columns, so the first must be \
         scrolled out; screen:\n{screen}"
    );
    assert!(
        h.driver.tab_center(&bar, 0).is_none(),
        "a tab scrolled out of view must not paint a hit part"
    );
    assert!(h.driver.tab_center(&bar, last).is_some());

    // Click the painted body of the tab left of the active one.
    let target = last - 1;
    let (x, y) = h
        .driver
        .tab_center(&bar, target)
        .expect("the tab left of the active one must be painted");
    h.driver.click(x, y);
    assert_eq!(
        active_tab(&h.engine.borrow()),
        (target, OVERFLOW_TABS),
        "clicking a painted tab must activate exactly that tab"
    );

    // Close it through its painted close glyph.
    let (x, y) = h
        .driver
        .tab_close_center(&bar, target)
        .expect("a closable tab must paint its close glyph");
    h.driver.click(x, y);
    let (_, count) = active_tab(&h.engine.borrow());
    assert_eq!(count, OVERFLOW_TABS - 1, "the × click must close one tab");
    let screen = h.driver.screen();
    assert!(
        !screen.contains(&overflow_tab_name(target)),
        "the closed tab's label must be gone; screen:\n{screen}"
    );

    // Back to the first tab: the bar scrolls it into view.
    for c in ":tabfirst".chars() {
        h.driver.type_char(c);
    }
    h.driver.press_named(quadraui::NamedKey::Enter);
    settle(&mut h.driver);
    assert_eq!(active_tab(&h.engine.borrow()).0, 0);
    let screen = h.driver.screen();
    assert!(
        screen.contains(&overflow_tab_name(0)),
        "activating the first tab must scroll it back into view; \
         screen:\n{screen}"
    );
    assert!(h.driver.tab_center(&bar, 0).is_some());
    assert!(
        !screen.contains(&overflow_tab_name(last)),
        "with the bar scrolled to the start, the last tab is out of view \
         again; screen:\n{screen}"
    );
}

/// A no-workspace engine whose welcome panel lists two recent folders, each
/// holding one uniquely named child so opening it is visible in the explorer.
/// `recent_workspaces` is oldest first; the panel lists most recent first.
fn engine_with_two_recent_folders(tag: &str) -> (crate::core::Engine, [String; 2]) {
    crate::test_cwd::suppress_process_chdir_on_this_thread();
    let root = crate::harness::scratch_dir(&format!("vimcode_welcome_keys_{tag}"));
    let _ = std::fs::remove_dir_all(&root);
    let older = root.join("wkolder_folder");
    let newer = root.join("wknewer_folder");
    std::fs::create_dir_all(older.join("olderkid")).unwrap();
    std::fs::create_dir_all(newer.join("newerkid")).unwrap();
    let mut engine = crate::core::Engine::new_for_test();
    engine.settings.use_nerd_fonts = Some(false);
    engine.startup_into_no_workspace_for_test();
    engine.session.recent_workspaces = vec![older, newer];
    (engine, ["newerkid".to_string(), "olderkid".to_string()])
}

/// The background of the first cell of `needle`'s painted row.
fn row_bg(
    driver: &TuiDriver<impl quadraui::AppLogic>,
    needle: &str,
) -> quadraui::tui::testing::Color {
    let at = driver
        .find_bounds(needle)
        .unwrap_or_else(|| panic!("{needle:?} must be painted:\n{}", driver.screen()));
    driver
        .style_at(at.x as u16, at.y as u16)
        .expect("a painted cell has a style")
        .bg
}

/// `j`/`k` move the welcome list's selection row by row — the painted
/// highlight follows it — and Enter opens the selected folder, so the row
/// the keyboard landed on is the one that opens.
#[test]
fn welcome_recent_list_keyboard_navigation_moves_the_painted_selection_and_opens_it() {
    let (engine, [newer_child, older_child]) = engine_with_two_recent_folders("nav");
    // Wide enough that each full recent-folder path paints untruncated.
    let mut h = harness(engine, 220);
    settle(&mut h.driver);

    let unselected_newer = row_bg(&h.driver, "wknewer_folder");
    let unselected_older = row_bg(&h.driver, "wkolder_folder");

    // Down from the Open Folder button onto the first row (most recent).
    h.driver.type_char('j');
    let newer_selected = row_bg(&h.driver, "wknewer_folder");
    assert_ne!(
        newer_selected,
        unselected_newer,
        "`j` must paint the first recent row as selected; screen:\n{}",
        h.driver.screen()
    );
    assert_eq!(row_bg(&h.driver, "wkolder_folder"), unselected_older);

    // Down again onto the second row: the highlight moves with it.
    h.driver.type_char('j');
    assert_eq!(
        row_bg(&h.driver, "wkolder_folder"),
        newer_selected,
        "`j` must move the selection highlight to the second row"
    );
    assert_ne!(row_bg(&h.driver, "wknewer_folder"), newer_selected);

    // `j` at the last row stays there; `k` moves back up.
    h.driver.type_char('j');
    assert_eq!(row_bg(&h.driver, "wkolder_folder"), newer_selected);
    h.driver.type_char('k');
    assert_eq!(
        row_bg(&h.driver, "wknewer_folder"),
        newer_selected,
        "`k` must move the selection back to the first row"
    );
    h.driver.type_char('j');

    // Enter opens the selected (second, older) folder, not the first.
    h.driver.press_named(quadraui::NamedKey::Enter);
    settle(&mut h.driver);
    let screen = h.driver.screen();
    assert!(
        !screen.contains("Recent Folders"),
        "opening a recent folder must dismiss the welcome panel; screen:\n{screen}"
    );
    assert!(
        screen.contains(&older_child) && !screen.contains(&newer_child),
        "Enter must open the row the keyboard selected (the older folder); \
         screen:\n{screen}"
    );
}
