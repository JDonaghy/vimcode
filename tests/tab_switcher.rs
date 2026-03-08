mod common;
use common::*;

// ── Tab switcher popup ───────────────────────────────────────────────────────

#[test]
fn tab_switcher_does_not_open_with_single_tab() {
    let mut e = engine_with("hello\n");
    // Ctrl+Tab with only one tab should not open the switcher
    e.handle_key("Tab", None, true);
    assert!(!e.tab_switcher_open, "should not open with single tab");
}

#[test]
fn tab_switcher_opens_with_multiple_tabs() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "tabnew");
    // Now we have 2 tabs — Ctrl+Tab should open
    e.handle_key("Tab", None, true);
    assert!(e.tab_switcher_open, "should open with multiple tabs");
    assert_eq!(e.tab_switcher_selected, 1, "should start on second item");
}

#[test]
fn tab_switcher_confirm_switches_tab() {
    let mut e = engine_with("first\n");
    exec(&mut e, "tabnew");
    // We're on tab 1 (the new tab). Tab 0 has "first\n".
    let tab1_idx = e.active_group().active_tab;
    assert_eq!(tab1_idx, 1);

    // Ctrl+Tab opens, selected=1 (the previous tab, tab 0)
    e.handle_key("Tab", None, true);
    assert!(e.tab_switcher_open);

    // Return confirms the selection
    press_key(&mut e, "Return");
    assert!(!e.tab_switcher_open, "should close after confirm");
    assert_eq!(e.active_group().active_tab, 0, "should switch to tab 0");
}

#[test]
fn tab_switcher_escape_cancels() {
    let mut e = engine_with("first\n");
    exec(&mut e, "tabnew");
    let original_tab = e.active_group().active_tab;

    e.handle_key("Tab", None, true);
    assert!(e.tab_switcher_open);

    press_key(&mut e, "Escape");
    assert!(!e.tab_switcher_open, "should close on Escape");
    assert_eq!(
        e.active_group().active_tab,
        original_tab,
        "should stay on original tab after cancel"
    );
}

#[test]
fn tab_switcher_cycles_forward() {
    let mut e = engine_with("a\n");
    exec(&mut e, "tabnew"); // tab 1
    exec(&mut e, "tabnew"); // tab 2
                            // MRU order after creating: tab2 (current), tab1, tab0

    e.handle_key("Tab", None, true);
    assert!(e.tab_switcher_open);
    assert_eq!(e.tab_switcher_selected, 1);

    // Another Ctrl+Tab cycles forward
    e.handle_key("Tab", None, true);
    assert_eq!(e.tab_switcher_selected, 2);

    // Wraps around
    e.handle_key("Tab", None, true);
    assert_eq!(e.tab_switcher_selected, 0);
}

#[test]
fn tab_switcher_cycles_backward() {
    let mut e = engine_with("a\n");
    exec(&mut e, "tabnew");
    exec(&mut e, "tabnew");

    // Ctrl+Shift+Tab opens at the last item
    e.handle_key("ISO_Left_Tab", None, true);
    assert!(e.tab_switcher_open);
    let len = e.tab_mru.len();
    assert_eq!(e.tab_switcher_selected, len - 1);
}

#[test]
fn tab_switcher_any_key_confirms() {
    let mut e = engine_with("first\n");
    exec(&mut e, "tabnew");

    e.handle_key("Tab", None, true);
    assert!(e.tab_switcher_open);

    // Any key (like 'j') should confirm and close
    press(&mut e, 'j');
    assert!(!e.tab_switcher_open, "any key should confirm");
}

#[test]
fn tab_mru_order_reflects_access() {
    let mut e = engine_with("a\n");
    exec(&mut e, "tabnew"); // tab 1
    exec(&mut e, "tabnew"); // tab 2 (current)

    // MRU: [tab2, tab1, tab0] — tab2 is current, tab1 was touched by tabnew
    e.open_tab_switcher();
    let items = e.tab_switcher_items();
    assert_eq!(items.len(), 3, "should have 3 MRU entries");
    e.tab_switcher_open = false;

    // Switch to tab 0
    e.goto_tab(0);
    // Now MRU: [tab0, tab2, tab1]
    e.open_tab_switcher();
    // Selected=1 means we'd switch to tab2 (second in MRU)
    assert_eq!(e.tab_switcher_selected, 1);
    let entry = e.tab_mru[1];
    assert_eq!(entry.1, 2, "second MRU entry should be tab 2");
}

#[test]
fn tab_switcher_items_returns_display_info() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "tabnew");

    e.open_tab_switcher();
    let items = e.tab_switcher_items();
    assert_eq!(items.len(), 2);
    // Each item is (filename, path, is_dirty)
    for (name, _path, _dirty) in &items {
        assert!(!name.is_empty(), "filename should not be empty");
    }
}

#[test]
fn tab_switcher_handles_closed_tabs_gracefully() {
    let mut e = engine_with("a\n");
    exec(&mut e, "tabnew");
    exec(&mut e, "tabnew");
    // 3 tabs, MRU has all 3

    // Close current tab (tab 2)
    exec(&mut e, "q!");
    // Now 2 tabs remain — MRU should be cleaned up on next open
    e.open_tab_switcher();
    let items = e.tab_switcher_items();
    assert_eq!(items.len(), 2, "should have 2 entries after closing a tab");
    e.tab_switcher_open = false;
}

#[test]
fn tab_switcher_confirm_then_reopen_has_correct_mru() {
    let mut e = engine_with("a\n");
    exec(&mut e, "tabnew"); // tab 1
    exec(&mut e, "tabnew"); // tab 2

    // Ctrl+Tab + Return → switch to tab 1 (second MRU entry)
    e.handle_key("Tab", None, true);
    press_key(&mut e, "Return");
    let switched_to = e.active_group().active_tab;

    // Now open again — the tab we just left (tab 2) should be at MRU[1]
    e.handle_key("Tab", None, true);
    assert!(e.tab_switcher_open);
    let second_entry = e.tab_mru[1];
    // The second entry should NOT be the same as where we are now
    assert_ne!(
        second_entry.1,
        e.active_group().active_tab,
        "second MRU entry should be the previous tab"
    );
    // It should be the tab we just left
    assert_ne!(second_entry.1, switched_to);
}
