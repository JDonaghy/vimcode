mod common;
use common::*;

// ── Open keymaps editor ─────────────────────────────────────────────────────

#[test]
fn keymaps_command_opens_scratch_buffer() {
    let mut e = engine_with("hello\n");
    e.settings.keymaps = vec!["n K :join".to_string(), "n J :nop".to_string()];
    e.rebuild_user_keymaps();

    exec(&mut e, "Keymaps");

    // Should now be in a keymaps buffer
    assert!(e.active_buffer_state().is_keymaps_buf);
    let lines = get_lines(&e);
    // Header comments come first, then user keymaps
    assert!(
        lines[0].starts_with('#'),
        "first line should be a comment header"
    );
    let user_lines: Vec<&str> = lines
        .iter()
        .map(|s| s.as_str())
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .collect();
    assert_eq!(user_lines, vec!["n K :join", "n J :nop"]);
}

#[test]
fn keymaps_editor_shows_keymaps_tab_name() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "Keymaps");
    assert_eq!(e.active_buffer_state().display_name(), "[Keymaps]");
}

#[test]
fn keymaps_editor_empty_settings() {
    let mut e = engine_with("hello\n");
    assert!(e.settings.keymaps.is_empty());
    exec(&mut e, "Keymaps");
    assert!(e.active_buffer_state().is_keymaps_buf);
    // Should have comment header but no user keymaps
    let lines = get_lines(&e);
    assert!(
        lines[0].starts_with('#'),
        "first line should be a comment header"
    );
    let user_lines: Vec<&str> = lines
        .iter()
        .map(|s| s.as_str())
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .collect();
    assert!(user_lines.is_empty(), "no user keymaps should be present");
}

#[test]
fn keymaps_editor_reuses_existing_buffer() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "Keymaps");
    assert!(e.active_buffer_state().is_keymaps_buf);

    // Open another tab
    exec(&mut e, "enew");
    assert!(!e.active_buffer_state().is_keymaps_buf);

    // Open keymaps again — should switch to existing buffer, not create new
    exec(&mut e, "Keymaps");
    assert!(e.active_buffer_state().is_keymaps_buf);
}

// ── Save keymaps buffer ─────────────────────────────────────────────────────

#[test]
fn save_keymaps_buffer_updates_settings() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "Keymaps");
    assert!(e.active_buffer_state().is_keymaps_buf);

    // Replace buffer content with new keymaps
    e.active_buffer_state_mut().buffer.content =
        ropey::Rope::from_str("n K :join\nv gc :Commentary\n");
    e.active_buffer_state_mut().dirty = true;

    let result = e.save();
    assert!(result.is_ok());
    assert_eq!(e.settings.keymaps.len(), 2);
    assert_eq!(e.settings.keymaps[0], "n K :join");
    assert_eq!(e.settings.keymaps[1], "v gc :Commentary");
    assert!(!e.active_buffer_state().dirty);
    assert!(e.message.contains("2 keymaps saved"));
}

#[test]
fn save_keymaps_buffer_rejects_invalid_lines() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "Keymaps");

    // Invalid keymap (no colon prefix on command)
    e.active_buffer_state_mut().buffer.content = ropey::Rope::from_str("n K join\n");
    e.active_buffer_state_mut().dirty = true;

    let result = e.save();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("Invalid keymap on line 1"));
}

#[test]
fn save_keymaps_buffer_skips_blank_lines() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "Keymaps");

    e.active_buffer_state_mut().buffer.content = ropey::Rope::from_str("n K :join\n\n\nn J :nop\n");
    e.active_buffer_state_mut().dirty = true;

    let result = e.save();
    assert!(result.is_ok());
    assert_eq!(e.settings.keymaps.len(), 2);
}

#[test]
fn save_keymaps_buffer_rebuilds_user_keymaps() {
    let mut e = engine_with("hello\nworld\n");
    assert!(e.user_keymaps.is_empty());

    exec(&mut e, "Keymaps");
    e.active_buffer_state_mut().buffer.content = ropey::Rope::from_str("n K :join\n");
    e.active_buffer_state_mut().dirty = true;
    let _ = e.save();

    // User keymaps should be rebuilt
    assert_eq!(e.user_keymaps.len(), 1);
    assert_eq!(e.user_keymaps[0].action, "join");
}

#[test]
fn save_empty_keymaps_buffer_clears_all() {
    let mut e = engine_with("hello\n");
    e.settings.keymaps = vec!["n K :join".to_string()];
    e.rebuild_user_keymaps();

    exec(&mut e, "Keymaps");
    e.active_buffer_state_mut().buffer.content = ropey::Rope::from_str("\n");
    e.active_buffer_state_mut().dirty = true;

    let _ = e.save();
    assert!(e.settings.keymaps.is_empty());
    assert!(e.user_keymaps.is_empty());
}

// ── Settings panel interaction ──────────────────────────────────────────────

#[test]
fn settings_panel_keymaps_row_opens_editor() {
    let mut e = engine_with("hello\n");
    e.settings_has_focus = true;

    // Find the keymaps setting in flat list
    use vimcode_core::core::engine::SettingsRow;
    let flat = e.settings_flat_list();
    let keymaps_idx = flat
        .iter()
        .position(|row| {
            matches!(row, SettingsRow::CoreSetting(idx)
                if vimcode_core::core::settings::SETTING_DEFS[*idx].key == "keymaps")
        })
        .expect("keymaps setting not found in flat list");

    e.settings_selected = keymaps_idx;
    e.handle_settings_key("Return", false, None);

    // Should have opened the keymaps editor and unfocused settings panel
    assert!(!e.settings_has_focus);
    assert!(e.active_buffer_state().is_keymaps_buf);
}

#[test]
fn keymaps_w_command_saves_buffer() {
    let mut e = engine_with("hello\nworld\n");
    exec(&mut e, "Keymaps");

    e.active_buffer_state_mut().buffer.content = ropey::Rope::from_str("n K :join\n");
    e.active_buffer_state_mut().dirty = true;

    // Use :w command
    run_cmd(&mut e, "w");
    assert_eq!(e.settings.keymaps.len(), 1);
    assert_eq!(e.settings.keymaps[0], "n K :join");
}
