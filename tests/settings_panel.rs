mod common;
use common::*;

#[test]
fn settings_panel_focus() {
    let mut e = engine_with("hello\n");
    assert!(!e.settings_has_focus);
    e.settings_has_focus = true;
    assert!(e.settings_has_focus);
    // q unfocuses
    e.handle_settings_key("q", false, None);
    assert!(!e.settings_has_focus);
}

#[test]
fn settings_panel_escape_unfocuses() {
    let mut e = engine_with("hello\n");
    e.settings_has_focus = true;
    e.handle_settings_key("Escape", false, None);
    assert!(!e.settings_has_focus);
}

#[test]
fn settings_panel_toggle_bool() {
    let mut e = engine_with("hello\n");
    e.settings_has_focus = true;

    // Find cursorline setting index in flat list
    let flat = e.settings_flat_list();
    let cursorline_idx = flat
        .iter()
        .position(|&(is_cat, idx)| {
            !is_cat && vimcode_core::core::settings::SETTING_DEFS[idx].key == "cursorline"
        })
        .expect("cursorline setting not found in flat list");

    let original = e.settings.cursorline;
    e.settings_selected = cursorline_idx;

    // Space toggles
    e.handle_settings_key("Space", false, None);
    assert_ne!(e.settings.cursorline, original);

    // Toggle back
    e.handle_settings_key("Space", false, None);
    assert_eq!(e.settings.cursorline, original);
}

#[test]
fn settings_panel_cycle_enum() {
    let mut e = engine_with("hello\n");
    e.settings_has_focus = true;

    // Find line_numbers setting
    let flat = e.settings_flat_list();
    let ln_idx = flat
        .iter()
        .position(|&(is_cat, idx)| {
            !is_cat && vimcode_core::core::settings::SETTING_DEFS[idx].key == "line_numbers"
        })
        .expect("line_numbers setting not found");

    e.settings_selected = ln_idx;
    let original = e.settings.get_value_str("line_numbers");

    // Enter cycles forward
    e.handle_settings_key("Return", false, None);
    let after_cycle = e.settings.get_value_str("line_numbers");
    assert_ne!(after_cycle, original, "enum should cycle forward");

    // h cycles backward (back to original)
    e.handle_settings_key("h", false, None);
    let after_back = e.settings.get_value_str("line_numbers");
    assert_eq!(after_back, original, "enum should cycle backward");
}

#[test]
fn settings_panel_search_filter() {
    let mut e = engine_with("hello\n");
    e.settings_has_focus = true;

    let full_count = e.settings_flat_list().len();
    assert!(full_count > 10, "should have many settings");

    // Activate search
    e.handle_settings_key("/", false, None);
    assert!(e.settings_input_active);

    // Type "font"
    e.handle_settings_key("", false, Some('f'));
    e.handle_settings_key("", false, Some('o'));
    e.handle_settings_key("", false, Some('n'));
    e.handle_settings_key("", false, Some('t'));

    let filtered_count = e.settings_flat_list().len();
    assert!(
        filtered_count < full_count,
        "filter should reduce items: {} vs {}",
        filtered_count,
        full_count
    );
    assert!(filtered_count > 0, "font filter should match something");

    // Exit search
    e.handle_settings_key("Escape", false, None);
    assert!(!e.settings_input_active);
}

#[test]
fn settings_panel_edit_int() {
    let mut e = engine_with("hello\n");
    e.settings_has_focus = true;

    // Find tabstop setting
    let flat = e.settings_flat_list();
    let ts_idx = flat
        .iter()
        .position(|&(is_cat, idx)| {
            !is_cat && vimcode_core::core::settings::SETTING_DEFS[idx].key == "tabstop"
        })
        .expect("tabstop not found");

    e.settings_selected = ts_idx;

    // Enter to start editing
    e.handle_settings_key("Return", false, None);
    assert!(e.settings_editing.is_some());

    // Clear and type new value
    // First clear the prefilled buffer
    while !e.settings_edit_buf.is_empty() {
        e.handle_settings_key("BackSpace", false, None);
    }
    e.handle_settings_key("", false, Some('8'));

    // Non-digit should be ignored
    e.handle_settings_key("", false, Some('x'));
    assert_eq!(e.settings_edit_buf, "8");

    // Confirm
    e.handle_settings_key("Return", false, None);
    assert!(e.settings_editing.is_none());
    assert_eq!(e.settings.tabstop, 8);
}

#[test]
fn settings_panel_edit_string() {
    let mut e = engine_with("hello\n");
    e.settings_has_focus = true;

    // Find colorcolumn setting
    let flat = e.settings_flat_list();
    let cc_idx = flat
        .iter()
        .position(|&(is_cat, idx)| {
            !is_cat && vimcode_core::core::settings::SETTING_DEFS[idx].key == "colorcolumn"
        })
        .expect("colorcolumn not found");

    e.settings_selected = cc_idx;

    // Enter to start editing
    e.handle_settings_key("Return", false, None);
    assert!(e.settings_editing.is_some());

    // Clear and type
    while !e.settings_edit_buf.is_empty() {
        e.handle_settings_key("BackSpace", false, None);
    }
    for ch in "80,120".chars() {
        e.handle_settings_key("", false, Some(ch));
    }

    // Confirm
    e.handle_settings_key("Return", false, None);
    assert_eq!(e.settings.colorcolumn, "80,120");
}

#[test]
fn settings_panel_collapse_category() {
    let mut e = engine_with("hello\n");
    e.settings_has_focus = true;

    let initial_count = e.settings_flat_list().len();
    // First row should be a category header
    let flat = e.settings_flat_list();
    assert!(flat[0].0, "first row should be a category header");

    e.settings_selected = 0;

    // Tab collapses
    e.handle_settings_key("Tab", false, None);
    let collapsed_count = e.settings_flat_list().len();
    assert!(
        collapsed_count < initial_count,
        "collapsing should reduce rows: {} vs {}",
        collapsed_count,
        initial_count
    );

    // Tab again expands
    e.handle_settings_key("Tab", false, None);
    let expanded_count = e.settings_flat_list().len();
    assert_eq!(expanded_count, initial_count);
}

#[test]
fn settings_panel_escape_cancels_edit() {
    let mut e = engine_with("hello\n");
    e.settings_has_focus = true;

    let flat = e.settings_flat_list();
    let ts_idx = flat
        .iter()
        .position(|&(is_cat, idx)| {
            !is_cat && vimcode_core::core::settings::SETTING_DEFS[idx].key == "tabstop"
        })
        .expect("tabstop not found");

    e.settings_selected = ts_idx;
    let original_tabstop = e.settings.tabstop;

    // Enter edit mode
    e.handle_settings_key("Return", false, None);
    assert!(e.settings_editing.is_some());

    // Type something different
    while !e.settings_edit_buf.is_empty() {
        e.handle_settings_key("BackSpace", false, None);
    }
    e.handle_settings_key("", false, Some('2'));

    // Escape cancels
    e.handle_settings_key("Escape", false, None);
    assert!(e.settings_editing.is_none());
    assert_eq!(
        e.settings.tabstop, original_tabstop,
        "value should be unchanged after Escape"
    );
}

#[test]
fn settings_panel_cycle_colorscheme() {
    let mut e = engine_with("hello\n");
    e.settings_has_focus = true;

    // Find colorscheme setting (DynamicEnum)
    let flat = e.settings_flat_list();
    let cs_idx = flat
        .iter()
        .position(|&(is_cat, idx)| {
            !is_cat && vimcode_core::core::settings::SETTING_DEFS[idx].key == "colorscheme"
        })
        .expect("colorscheme setting not found");

    e.settings_selected = cs_idx;
    assert_eq!(e.settings.colorscheme, "onedark");

    // Enter cycles forward
    e.handle_settings_key("Return", false, None);
    assert_eq!(e.settings.colorscheme, "gruvbox-dark");

    // l cycles forward again
    e.handle_settings_key("l", false, None);
    assert_eq!(e.settings.colorscheme, "tokyo-night");

    // h cycles backward
    e.handle_settings_key("h", false, None);
    assert_eq!(e.settings.colorscheme, "gruvbox-dark");
}
