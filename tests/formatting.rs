mod common;
use common::*;
use vimcode_core::core::OpenMode;

// ── format_on_save setting ──────────────────────────────────────────────────

#[test]
fn test_format_on_save_default_false() {
    let e = engine_with("hello\n");
    assert!(
        !e.settings.format_on_save,
        "format_on_save should default to false"
    );
}

#[test]
fn test_format_on_save_toggle_via_set() {
    let mut e = engine_with("hello\n");
    run_cmd(&mut e, "set formatonsave");
    assert!(e.settings.format_on_save);
    run_cmd(&mut e, "set noformatonsave");
    assert!(!e.settings.format_on_save);
}

#[test]
fn test_format_on_save_query() {
    let mut e = engine_with("hello\n");
    run_cmd(&mut e, "set formatonsave?");
    assert!(e.message.contains("noformatonsave"));
    e.settings.format_on_save = true;
    run_cmd(&mut e, "set formatonsave?");
    assert!(e.message.contains("formatonsave"));
    assert!(!e.message.contains("noformatonsave"));
}

#[test]
fn test_format_on_save_alias_fos() {
    let mut e = engine_with("hello\n");
    run_cmd(&mut e, "set fos");
    assert!(e.settings.format_on_save);
    run_cmd(&mut e, "set nofos");
    assert!(!e.settings.format_on_save);
}

#[test]
fn test_format_on_save_display_all() {
    let e = engine_with("hello\n");
    let display = e.settings.display_all();
    assert!(
        display.contains("noformatonsave"),
        "display_all should include format_on_save state: {display}"
    );
}

#[test]
fn test_format_on_save_get_value_str() {
    let mut e = engine_with("hello\n");
    assert_eq!(e.settings.get_value_str("format_on_save"), "false");
    e.settings.format_on_save = true;
    assert_eq!(e.settings.get_value_str("format_on_save"), "true");
}

#[test]
fn test_format_on_save_set_value_str() {
    let mut e = engine_with("hello\n");
    e.settings.set_value_str("format_on_save", "true").unwrap();
    assert!(e.settings.format_on_save);
    e.settings.set_value_str("format_on_save", "false").unwrap();
    assert!(!e.settings.format_on_save);
}

// ── capability check ────────────────────────────────────────────────────────

#[test]
fn test_lformat_lsp_disabled() {
    let mut e = engine_with("hello\n");
    e.settings.lsp_enabled = false;
    exec(&mut e, "Lformat");
    // LSP disabled — silently returns, no crash.
}

#[test]
fn test_lformat_no_server() {
    let mut e = engine_with("hello\n");
    e.settings.lsp_enabled = true;
    exec(&mut e, "Lformat");
    // No file path set, so no server — should not crash.
}

// ── save_with_format when format_on_save disabled ───────────────────────────

#[test]
fn test_save_with_format_disabled_saves_immediately() {
    let dir = std::env::temp_dir().join("vimcode_test_fos_disabled");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test_fos.txt");
    std::fs::write(&path, "original\n").unwrap();

    let mut e = engine_with("");
    e.open_file_with_mode(&path, OpenMode::Permanent).unwrap();
    // Modify buffer
    press(&mut e, 'A');
    type_chars(&mut e, " modified");
    press_key(&mut e, "Escape");

    assert!(!e.settings.format_on_save);
    let result = e.save_with_format(false);
    assert!(result.is_ok());

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("modified"),
        "file should be saved: {content}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_save_with_format_enabled_no_lsp_saves_immediately() {
    let dir = std::env::temp_dir().join("vimcode_test_fos_nolsp");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test_fos2.txt");
    std::fs::write(&path, "original\n").unwrap();

    let mut e = engine_with("");
    e.open_file_with_mode(&path, OpenMode::Permanent).unwrap();
    press(&mut e, 'A');
    type_chars(&mut e, " modified");
    press_key(&mut e, "Escape");

    e.settings.format_on_save = true;
    e.settings.lsp_enabled = true;
    // No LSP manager exists — should fall through to immediate save.
    let result = e.save_with_format(false);
    assert!(result.is_ok());

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("modified"),
        "file should be saved without LSP: {content}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ── format_save_quit_ready ──────────────────────────────────────────────────

#[test]
fn test_format_save_quit_ready_default_false() {
    let e = engine_with("hello\n");
    assert!(!e.format_save_quit_ready);
}
