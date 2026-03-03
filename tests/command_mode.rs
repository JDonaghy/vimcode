mod common;
use common::*;
use std::fs;
use vimcode_core::EngineAction;

// ── :q ────────────────────────────────────────────────────────────────────────

#[test]
fn test_quit_clean_buffer() {
    let mut e = engine_with("hello\n");
    e.set_dirty(false);
    let action = exec(&mut e, "q");
    assert!(
        matches!(action, EngineAction::Quit),
        "expected Quit on clean buffer, got: {action:?}"
    );
}

#[test]
fn test_quit_dirty_buffer_blocked() {
    let mut e = engine_with("hello\n");
    e.set_dirty(true);
    let action = exec(&mut e, "q");
    // Dirty buffer should NOT quit — returns error or QuitWithUnsaved
    assert!(
        !matches!(action, EngineAction::Quit),
        "should not quit with unsaved changes"
    );
}

#[test]
fn test_force_quit_dirty() {
    let mut e = engine_with("hello\n");
    e.set_dirty(true);
    let action = exec(&mut e, "q!");
    assert!(
        matches!(action, EngineAction::Quit),
        "q! should force quit, got: {action:?}"
    );
}

// ── :w ────────────────────────────────────────────────────────────────────────

#[test]
fn test_write_to_file() {
    let path = std::env::temp_dir().join("vimcode_test_write_cmd.txt");
    let _ = fs::remove_file(&path);

    let mut e = engine_with("test content\n");
    // Set the file path so :w knows where to save
    e.active_buffer_state_mut().file_path = Some(path.clone());
    exec(&mut e, "w");

    let written = fs::read_to_string(&path).expect("file should exist after :w");
    assert!(
        written.contains("test content"),
        "file should contain 'test content'"
    );
    let _ = fs::remove_file(&path);
}

// ── :set ─────────────────────────────────────────────────────────────────────

#[test]
fn test_set_tabstop() {
    let mut e = engine_with("");
    exec(&mut e, "set tabstop=2");
    assert_eq!(e.settings.tabstop, 2);
}

#[test]
fn test_set_expandtab() {
    let mut e = engine_with("");
    exec(&mut e, "set expandtab");
    assert!(e.settings.expand_tab);
    exec(&mut e, "set noexpandtab");
    assert!(!e.settings.expand_tab);
}

// ── :norm with range ─────────────────────────────────────────────────────────

#[test]
fn test_norm_on_numeric_range() {
    // 2,3norm A! appends '!' to lines 2 and 3 only (1-indexed)
    let mut e = engine_with("a\nb\nc\nd\n");
    exec(&mut e, "2,3norm A!");
    let lines = get_lines(&e);
    assert_eq!(lines[0], "a", "line 1 should be unchanged");
    assert_eq!(lines[1], "b!", "line 2 should have '!'");
    assert_eq!(lines[2], "c!", "line 3 should have '!'");
    assert_eq!(lines[3], "d", "line 4 should be unchanged");
}

#[test]
fn test_norm_insert_mode_on_range() {
    // 1,2norm I>> prepends '>>' to lines 1 and 2
    let mut e = engine_with("foo\nbar\nbaz\n");
    exec(&mut e, "1,2norm I>>");
    let lines = get_lines(&e);
    assert!(lines[0].starts_with(">>"), "line 1 should start with '>>'");
    assert!(lines[1].starts_with(">>"), "line 2 should start with '>>'");
    assert_eq!(lines[2], "baz", "line 3 should be unchanged");
}

// ── :%norm ────────────────────────────────────────────────────────────────────

#[test]
fn test_norm_append_to_all_lines() {
    let mut e = engine_with("a\nb\nc\n");
    exec(&mut e, "%norm A!");
    let lines = get_lines(&e);
    assert!(
        lines.iter().all(|l| l.ends_with('!')),
        "all lines should end with '!' after :%norm A!, got: {lines:?}"
    );
}

// ── :tabnew ───────────────────────────────────────────────────────────────────

#[test]
fn test_tabnew_adds_tab() {
    let mut e = engine_with("hello\n");
    let initial_tabs = e.tabs.len();
    exec(&mut e, "tabnew");
    assert_eq!(
        e.tabs.len(),
        initial_tabs + 1,
        "expected one more tab after :tabnew"
    );
}

// ── :s substitute (via execute_command) ──────────────────────────────────────

#[test]
fn test_substitute_replaces_on_current_line() {
    let mut e = engine_with("foo bar\n");
    exec(&mut e, "s/foo/baz/");
    assert_buf(&e, "baz bar\n");
}

#[test]
fn test_substitute_only_first_occurrence_per_line() {
    // Without /g, only the first match on each line is replaced
    let mut e = engine_with("foo foo foo\n");
    exec(&mut e, "s/foo/bar/");
    // First occurrence replaced, rest unchanged
    assert_buf(&e, "bar foo foo\n");
}

#[test]
fn test_wq_clean_buffer() {
    let path = std::env::temp_dir().join("vimcode_test_wq.txt");
    let _ = fs::remove_file(&path);
    let mut e = engine_with("data\n");
    e.active_buffer_state_mut().file_path = Some(path.clone());
    let action = exec(&mut e, "wq");
    // wq returns SaveQuit (UI handles the actual write + quit)
    assert!(
        matches!(action, EngineAction::SaveQuit | EngineAction::Quit),
        "wq should return SaveQuit or Quit, got: {action:?}"
    );
    assert!(path.exists(), "file should have been written by :wq");
    let _ = fs::remove_file(&path);
}
