mod common;

use common::*;
use vimcode_core::core::window::GroupId;
use vimcode_core::Engine;

// ── Helper: open N files as tabs ───────────────────────────────────────────────

fn engine_with_tabs(texts: &[&str]) -> Engine {
    let mut e = engine_with(texts[0]);
    for text in &texts[1..] {
        // Create a temp file to open as a new tab
        let dir = std::env::temp_dir().join(format!("ctx_menu_{}", rand_name()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file.txt");
        std::fs::write(&path, text).unwrap();
        e.execute_command(&format!("tabnew {}", path.display()));
    }
    e
}

fn rand_name() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    format!("{}_{}", d.as_secs(), d.subsec_nanos())
}

// ── close_other_tabs ───────────────────────────────────────────────────────────

#[test]
fn test_close_other_tabs_single_tab_noop() {
    let mut e = engine_with("hello");
    e.close_other_tabs();
    assert_eq!(e.active_group().tabs.len(), 1);
}

#[test]
fn test_close_other_tabs_with_two_tabs() {
    let mut e = engine_with_tabs(&["aaa", "bbb"]);
    assert_eq!(e.active_group().tabs.len(), 2);
    // active tab is the last opened (index 1)
    e.close_other_tabs();
    assert_eq!(e.active_group().tabs.len(), 1);
}

#[test]
fn test_close_other_tabs_with_five_tabs() {
    let mut e = engine_with_tabs(&["a", "b", "c", "d", "e"]);
    assert_eq!(e.active_group().tabs.len(), 5);
    // Switch to tab 2 (middle)
    e.active_group_mut().active_tab = 2;
    e.tab_mru_touch();
    e.close_other_tabs();
    assert_eq!(e.active_group().tabs.len(), 1);
}

// ── close_tabs_to_right ────────────────────────────────────────────────────────

#[test]
fn test_close_tabs_to_right_on_last_tab_noop() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.active_group_mut().active_tab = 2;
    e.close_tabs_to_right();
    assert_eq!(e.active_group().tabs.len(), 3);
}

#[test]
fn test_close_tabs_to_right_from_first() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.active_group_mut().active_tab = 0;
    e.close_tabs_to_right();
    assert_eq!(e.active_group().tabs.len(), 1);
    assert_eq!(e.active_group().active_tab, 0);
}

#[test]
fn test_close_tabs_to_right_from_middle() {
    let mut e = engine_with_tabs(&["a", "b", "c", "d"]);
    e.active_group_mut().active_tab = 1;
    e.close_tabs_to_right();
    assert_eq!(e.active_group().tabs.len(), 2);
    assert_eq!(e.active_group().active_tab, 1);
}

// ── close_tabs_to_left ─────────────────────────────────────────────────────────

#[test]
fn test_close_tabs_to_left_on_first_tab_noop() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.active_group_mut().active_tab = 0;
    e.close_tabs_to_left();
    assert_eq!(e.active_group().tabs.len(), 3);
}

#[test]
fn test_close_tabs_to_left_from_last() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.active_group_mut().active_tab = 2;
    e.close_tabs_to_left();
    assert_eq!(e.active_group().tabs.len(), 1);
    assert_eq!(e.active_group().active_tab, 0);
}

#[test]
fn test_close_tabs_to_left_from_middle() {
    let mut e = engine_with_tabs(&["a", "b", "c", "d"]);
    e.active_group_mut().active_tab = 2;
    e.close_tabs_to_left();
    assert_eq!(e.active_group().tabs.len(), 2);
    assert_eq!(e.active_group().active_tab, 0);
}

// ── close_saved_tabs ───────────────────────────────────────────────────────────

#[test]
fn test_close_saved_tabs_single_tab() {
    let mut e = engine_with("hello");
    e.close_saved_tabs();
    assert_eq!(e.active_group().tabs.len(), 1);
}

#[test]
fn test_close_saved_tabs_keeps_dirty() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    // Make tab 0 dirty
    e.active_group_mut().active_tab = 0;
    e.tab_mru_touch();
    e.set_dirty(true);
    // Active is tab 0 (dirty), so close_saved closes tabs 1 and 2
    e.close_saved_tabs();
    // Only the active dirty tab remains
    assert_eq!(e.active_group().tabs.len(), 1);
}

#[test]
fn test_close_saved_tabs_mixed_dirty() {
    let mut e = engine_with_tabs(&["a", "b", "c", "d"]);
    // Make tab 1 dirty
    e.active_group_mut().active_tab = 1;
    e.tab_mru_touch();
    e.set_dirty(true);
    // Set active to tab 3
    e.active_group_mut().active_tab = 3;
    e.tab_mru_touch();
    // Close saved: tabs 0 and 2 are clean non-active → close them.
    // Tab 1 is dirty → keep. Tab 3 is active → keep.
    e.close_saved_tabs();
    assert_eq!(e.active_group().tabs.len(), 2);
}

// ── close_tab_at ───────────────────────────────────────────────────────────────

#[test]
fn test_close_tab_at_valid() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.active_group_mut().active_tab = 0;
    // Close tab 1 (non-active)
    let result = e.close_tab_at(GroupId(0), 1);
    assert!(result);
    assert_eq!(e.active_group().tabs.len(), 2);
}

#[test]
fn test_close_tab_at_invalid_group() {
    let mut e = engine_with_tabs(&["a", "b"]);
    let result = e.close_tab_at(GroupId(99), 0);
    assert!(!result);
}

#[test]
fn test_close_tab_at_invalid_index() {
    let mut e = engine_with_tabs(&["a", "b"]);
    let result = e.close_tab_at(GroupId(0), 5);
    assert!(!result);
}

#[test]
fn test_close_tab_at_last_tab_cannot_close() {
    let mut e = engine_with("hello");
    let result = e.close_tab_at(GroupId(0), 0);
    assert!(!result);
    assert_eq!(e.active_group().tabs.len(), 1);
}

// ── copy_relative_path ─────────────────────────────────────────────────────────

#[test]
fn test_copy_relative_path_subdir() {
    let e = engine_with("hello");
    let cwd = e.cwd.clone();
    let path = cwd.join("src").join("main.rs");
    let rel = e.copy_relative_path(&path);
    assert_eq!(rel, "src/main.rs");
}

#[test]
fn test_copy_relative_path_outside_cwd() {
    let e = engine_with("hello");
    let path = std::path::PathBuf::from("/tmp/other/file.txt");
    let rel = e.copy_relative_path(&path);
    assert_eq!(rel, "/tmp/other/file.txt");
}

// ── tab_file_path ──────────────────────────────────────────────────────────────

#[test]
fn test_tab_file_path_no_file() {
    let e = engine_with("hello");
    let fp = e.tab_file_path(GroupId(0), 0);
    assert!(fp.is_none());
}

#[test]
fn test_tab_file_path_with_file() {
    let dir = std::env::temp_dir().join(format!("ctx_tfp_{}", rand_name()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.txt");
    std::fs::write(&path, "content").unwrap();

    let mut e = engine_with("");
    // Open a file via tabnew so it has a file path
    e.execute_command(&format!("tabnew {}", path.display()));
    // The new tab is now active (index 1)
    let fp = e.tab_file_path(GroupId(0), e.active_group().active_tab);
    assert!(fp.is_some());
    assert_eq!(fp.unwrap(), path);
}

// ── context_menu_confirm dispatch ──────────────────────────────────────────────

#[test]
fn test_context_menu_confirm_close() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.active_group_mut().active_tab = 0;
    // Open context menu on tab 1
    e.open_tab_context_menu(GroupId(0), 1, 10, 5);
    assert!(e.context_menu.is_some());
    // Select "Close" (first item, index 0)
    e.context_menu.as_mut().unwrap().selected = 0;
    let action = e.context_menu_confirm();
    assert_eq!(action.as_deref(), Some("close"));
    assert_eq!(e.active_group().tabs.len(), 2);
}

#[test]
fn test_context_menu_confirm_close_others() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.open_tab_context_menu(GroupId(0), 1, 10, 5);
    // Select "Close Others" (index 1)
    e.context_menu.as_mut().unwrap().selected = 1;
    let action = e.context_menu_confirm();
    assert_eq!(action.as_deref(), Some("close_others"));
    assert_eq!(e.active_group().tabs.len(), 1);
}

#[test]
fn test_context_menu_confirm_disabled_item() {
    let mut e = engine_with_tabs(&["a"]);
    e.open_tab_context_menu(GroupId(0), 0, 10, 5);
    // "Close Others" should be disabled (only 1 tab)
    e.context_menu.as_mut().unwrap().selected = 1;
    let action = e.context_menu_confirm();
    assert!(action.is_none());
}

// ── handle_context_menu_key ────────────────────────────────────────────────────

#[test]
fn test_context_menu_key_navigation() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.open_tab_context_menu(GroupId(0), 1, 10, 5);
    let initial_sel = e.context_menu.as_ref().unwrap().selected;
    assert_eq!(initial_sel, 0);

    // j moves down
    let (consumed, _) = e.handle_context_menu_key("j");
    assert!(consumed);
    assert_eq!(e.context_menu.as_ref().unwrap().selected, 1);

    // k moves up
    let (consumed, _) = e.handle_context_menu_key("k");
    assert!(consumed);
    assert_eq!(e.context_menu.as_ref().unwrap().selected, 0);
}

#[test]
fn test_context_menu_key_escape_closes() {
    let mut e = engine_with_tabs(&["a", "b"]);
    e.open_tab_context_menu(GroupId(0), 0, 10, 5);
    assert!(e.context_menu.is_some());
    let (consumed, _) = e.handle_context_menu_key("Escape");
    assert!(consumed);
    assert!(e.context_menu.is_none());
}

#[test]
fn test_context_menu_key_enter_confirms() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.open_tab_context_menu(GroupId(0), 1, 10, 5);
    e.context_menu.as_mut().unwrap().selected = 0; // "Close"
    let (consumed, action) = e.handle_context_menu_key("Return");
    assert!(consumed);
    assert_eq!(action.as_deref(), Some("close"));
    assert!(e.context_menu.is_none());
}

// ── explorer context menu ──────────────────────────────────────────────────────

#[test]
fn test_explorer_context_menu_file() {
    let mut e = engine_with("hello");
    let path = std::path::PathBuf::from("/tmp/test.rs");
    e.open_explorer_context_menu(path, false, 5, 10);
    assert!(e.context_menu.is_some());
    let menu = e.context_menu.as_ref().unwrap();
    assert_eq!(menu.items.len(), 8);
    assert_eq!(menu.items[0].label, "Open to the Side");
    assert_eq!(menu.items[4].label, "Copy Path");
}

#[test]
fn test_explorer_context_menu_dir() {
    let mut e = engine_with("hello");
    let path = std::path::PathBuf::from("/tmp/mydir");
    e.open_explorer_context_menu(path, true, 5, 10);
    assert!(e.context_menu.is_some());
    let menu = e.context_menu.as_ref().unwrap();
    assert_eq!(menu.items.len(), 9);
    assert_eq!(menu.items[0].label, "New File...");
    assert_eq!(menu.items[1].label, "New Folder...");
}

// ── :tabclose subcommands ──────────────────────────────────────────────────────

#[test]
fn test_tabclose_others_command() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.active_group_mut().active_tab = 1;
    e.tab_mru_touch();
    exec(&mut e, "tabclose others");
    assert_eq!(e.active_group().tabs.len(), 1);
}

#[test]
fn test_tabclose_right_command() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.active_group_mut().active_tab = 0;
    exec(&mut e, "tabclose right");
    assert_eq!(e.active_group().tabs.len(), 1);
}

#[test]
fn test_tabclose_left_command() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.active_group_mut().active_tab = 2;
    exec(&mut e, "tabclose left");
    assert_eq!(e.active_group().tabs.len(), 1);
}

#[test]
fn test_tabclose_saved_command() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.active_group_mut().active_tab = 1;
    e.tab_mru_touch();
    e.set_dirty(true);
    e.active_group_mut().active_tab = 0;
    e.tab_mru_touch();
    exec(&mut e, "tabclose saved");
    // Tab 1 is dirty + non-active → kept. Tab 2 is clean + non-active → closed.
    assert_eq!(e.active_group().tabs.len(), 2);
}

// ── context menu + handle_key integration ──────────────────────────────────────

#[test]
fn test_context_menu_intercepts_handle_key() {
    let mut e = engine_with_tabs(&["a", "b"]);
    e.open_tab_context_menu(GroupId(0), 0, 10, 5);
    // Pressing 'j' should be consumed by context menu, not move cursor
    let action = e.handle_key("j", Some('j'), false);
    assert_eq!(action, vimcode_core::EngineAction::None);
    assert!(e.context_menu.is_some());

    // Escape closes it
    e.handle_key("Escape", None, false);
    assert!(e.context_menu.is_none());
}

#[test]
fn test_context_menu_key_wraps_around() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    e.open_tab_context_menu(GroupId(0), 1, 10, 5);
    // Press k from position 0 → should wrap to last item
    let (consumed, _) = e.handle_context_menu_key("k");
    assert!(consumed);
    let sel = e.context_menu.as_ref().unwrap().selected;
    let len = e.context_menu.as_ref().unwrap().items.len();
    assert_eq!(sel, len - 1);
}

// ── Tab context menu items correctness ─────────────────────────────────────────

#[test]
fn test_tab_context_menu_items_enabled_state() {
    let mut e = engine_with_tabs(&["a", "b", "c"]);
    // Tab 2 is the last tab
    e.open_tab_context_menu(GroupId(0), 2, 10, 5);
    let menu = e.context_menu.as_ref().unwrap();
    // "Close to the Right" should be disabled (last tab)
    let close_right = menu
        .items
        .iter()
        .find(|i| i.action == "close_right")
        .unwrap();
    assert!(!close_right.enabled);
    // "Close Others" should be enabled (3 tabs)
    let close_others = menu
        .items
        .iter()
        .find(|i| i.action == "close_others")
        .unwrap();
    assert!(close_others.enabled);
}

#[test]
fn test_tab_context_menu_single_tab_disables_close_others() {
    let mut e = engine_with("hello");
    e.open_tab_context_menu(GroupId(0), 0, 10, 5);
    let menu = e.context_menu.as_ref().unwrap();
    let close_others = menu
        .items
        .iter()
        .find(|i| i.action == "close_others")
        .unwrap();
    assert!(!close_others.enabled);
}

#[test]
fn test_tab_context_menu_no_file_disables_path_actions() {
    let mut e = engine_with("hello");
    e.open_tab_context_menu(GroupId(0), 0, 10, 5);
    let menu = e.context_menu.as_ref().unwrap();
    let copy_path = menu.items.iter().find(|i| i.action == "copy_path").unwrap();
    assert!(!copy_path.enabled);
    let reveal = menu.items.iter().find(|i| i.action == "reveal").unwrap();
    assert!(!reveal.enabled);
}

// ── context_menu_confirm split actions ─────────────────────────────────────────

#[test]
fn test_context_menu_split_right() {
    let mut e = engine_with("hello\nworld");
    e.open_tab_context_menu(GroupId(0), 0, 10, 5);
    // Find split_right index
    let idx = e
        .context_menu
        .as_ref()
        .unwrap()
        .items
        .iter()
        .position(|i| i.action == "split_right")
        .unwrap();
    e.context_menu.as_mut().unwrap().selected = idx;
    let action = e.context_menu_confirm();
    assert_eq!(action.as_deref(), Some("split_right"));
    // Should have created a window split
    assert!(e.active_tab().layout.window_ids().len() >= 2);
}
