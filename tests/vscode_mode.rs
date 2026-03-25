mod common;
use common::*;
use vimcode_core::{Cursor, Mode};

/// Switch engine to VSCode mode.
fn vscode_mode(e: &mut vimcode_core::Engine) {
    e.settings.editor_mode = vimcode_core::core::settings::EditorMode::Vscode;
    e.mode = Mode::Insert;
    e.visual_anchor = None;
}

/// Send a key in VSCode mode (delegates to handle_key which dispatches to handle_vscode_key).
fn vk(e: &mut vimcode_core::Engine, key_name: &str) {
    e.handle_key(key_name, None, false);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 1: Line Operations
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_vscode_move_line_up() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 1, col: 0 };
    // Alt+Up → move "bbb" up
    e.handle_key("Alt_Up", None, false);
    assert_eq!(buf(&e), "bbb\naaa\nccc\n");
    assert_eq!(e.cursor().line, 0);
}

#[test]
fn test_vscode_move_line_down() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Alt_Down", None, false);
    assert_eq!(buf(&e), "bbb\naaa\nccc\n");
    assert_eq!(e.cursor().line, 1);
}

#[test]
fn test_vscode_move_line_up_at_top_noop() {
    let mut e = engine_with("aaa\nbbb\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Alt_Up", None, false);
    assert_eq!(buf(&e), "aaa\nbbb\n");
    assert_eq!(e.cursor().line, 0);
}

#[test]
fn test_vscode_move_line_down_at_bottom_noop() {
    let mut e = engine_with("aaa\nbbb\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 1, col: 0 };
    e.handle_key("Alt_Down", None, false);
    assert_eq!(buf(&e), "aaa\nbbb\n");
}

#[test]
fn test_vscode_move_line_up_with_selection() {
    let mut e = engine_with("aaa\nbbb\nccc\nddd\n");
    vscode_mode(&mut e);
    // Select lines 1-2 (bbb, ccc)
    e.visual_anchor = Some(Cursor { line: 1, col: 0 });
    e.mode = Mode::Visual;
    e.view_mut().cursor = Cursor { line: 2, col: 2 };
    e.handle_key("Alt_Up", None, false);
    assert_eq!(buf(&e), "bbb\nccc\naaa\nddd\n");
}

#[test]
fn test_vscode_alt_shift_down_adds_cursor_below() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 1 };
    e.handle_key("Alt_Shift_Down", None, false);
    assert_eq!(e.view().extra_cursors.len(), 1);
    assert_eq!(e.view().extra_cursors[0], Cursor { line: 1, col: 1 });
}

#[test]
fn test_vscode_alt_shift_up_adds_cursor_above() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 2, col: 0 };
    e.handle_key("Alt_Shift_Up", None, false);
    assert_eq!(e.view().extra_cursors.len(), 1);
    assert_eq!(e.view().extra_cursors[0], Cursor { line: 1, col: 0 });
}

#[test]
fn test_vscode_alt_shift_down_multiple() {
    let mut e = engine_with("aaa\nbbb\nccc\nddd\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Alt_Shift_Down", None, false);
    e.handle_key("Alt_Shift_Down", None, false);
    // Should have 2 extra cursors (lines 1 and 2)
    assert_eq!(e.view().extra_cursors.len(), 2);
}

#[test]
fn test_vscode_delete_line() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 1, col: 0 };
    e.handle_key("K", None, true);
    assert_eq!(buf(&e), "aaa\nccc\n");
}

#[test]
fn test_vscode_delete_line_last() {
    let mut e = engine_with("aaa\nbbb\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 1, col: 0 };
    e.handle_key("K", None, true);
    // Deleting last line removes the trailing newline too
    assert!(buf(&e) == "aaa\n" || buf(&e) == "aaa");
}

#[test]
fn test_vscode_insert_line_below() {
    let mut e = engine_with("aaa\nbbb\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 1 };
    e.handle_key("Return", None, true);
    // Should insert blank line after "aaa", cursor stays on line 0
    let lines = get_lines(&e);
    assert_eq!(lines[0], "aaa");
    assert_eq!(lines[1], "");
    assert_eq!(lines[2], "bbb");
    assert_eq!(e.cursor().line, 0);
}

#[test]
fn test_vscode_insert_line_above() {
    let mut e = engine_with("aaa\nbbb\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 1, col: 0 };
    e.handle_key("Shift_Return", None, true);
    let lines = get_lines(&e);
    assert_eq!(lines[0], "aaa");
    assert_eq!(lines[1], "");
    assert_eq!(lines[2], "bbb");
    // Cursor stays on original line (now line 2)
    assert_eq!(e.cursor().line, 2);
}

#[test]
fn test_vscode_select_line() {
    let mut e = engine_with("hello world\nsecond line\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 3 };
    // Ctrl+L selects current line (anchor at start, cursor at start of next line)
    e.handle_key("l", Some('l'), true);
    assert!(e.visual_anchor.is_some());
    assert_eq!(e.visual_anchor.unwrap().line, 0);
    assert_eq!(e.visual_anchor.unwrap().col, 0);
    // Cursor should be at start of next line (selecting whole line + newline)
    assert_eq!(e.cursor().line, 1);
    assert_eq!(e.cursor().col, 0);
}

#[test]
fn test_vscode_select_line_extends() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    // First Ctrl+L selects line 0 (cursor moves to line 1)
    e.handle_key("l", Some('l'), true);
    assert_eq!(e.cursor().line, 1);
    // Second Ctrl+L extends to line 2
    e.handle_key("l", Some('l'), true);
    assert_eq!(e.cursor().line, 2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 2: Multi-Cursor + Indentation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_vscode_ctrl_d_selects_word() {
    let mut e = engine_with("hello world hello\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 1 }; // in "hello"
                                                      // Ctrl+D should select "hello" (no trailing space)
    e.handle_key("d", Some('d'), true);
    assert!(e.visual_anchor.is_some());
    assert_eq!(e.mode, Mode::Visual);
    assert_eq!(e.visual_anchor.unwrap().col, 0); // anchor at word start
    assert_eq!(e.cursor().col, 4); // cursor at last char of "hello" (inclusive)
}

#[test]
fn test_vscode_ctrl_d_adds_next_occurrence() {
    let mut e = engine_with("foo bar foo baz foo\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 1 }; // in "foo"
                                                      // First Ctrl+D: select word "foo"
    e.handle_key("d", Some('d'), true);
    assert_eq!(e.visual_anchor.unwrap().col, 0);
    assert_eq!(e.cursor().col, 2); // "foo" is cols 0-2
                                   // Second Ctrl+D: add cursor at END of next "foo" (col 8+2=10)
    e.handle_key("d", Some('d'), true);
    assert_eq!(e.view().extra_cursors.len(), 1);
    assert_eq!(e.view().extra_cursors[0].col, 10); // second "foo" at cols 8-10
}

#[test]
fn test_vscode_ctrl_shift_l_selects_all() {
    let mut e = engine_with("foo bar foo baz foo\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 1 }; // in "foo"
    e.handle_key("L", None, true);
    // Should have 2 extra cursors (3 "foo" total)
    assert_eq!(e.view().extra_cursors.len(), 2);
    // Visual anchor at word start, primary cursor at word end
    assert_eq!(e.visual_anchor.unwrap(), Cursor { line: 0, col: 0 });
    assert_eq!(e.cursor().col, 2); // end of "foo"
                                   // Extra cursors at end of other "foo" occurrences
    assert_eq!(e.view().extra_cursors[0], Cursor { line: 0, col: 10 });
    assert_eq!(e.view().extra_cursors[1], Cursor { line: 0, col: 18 });
}

#[test]
fn test_vscode_ctrl_shift_l_then_type_replaces_all() {
    let mut e = engine_with("foo bar foo baz foo\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("L", None, true);
    assert_eq!(e.view().extra_cursors.len(), 2);
    // Type "qux" — should replace all "foo" with "qux"
    e.handle_key("q", Some('q'), false);
    e.handle_key("u", Some('u'), false);
    e.handle_key("x", Some('x'), false);
    assert_eq!(buf(&e), "qux bar qux baz qux\n");
}

#[test]
fn test_vscode_ctrl_shift_l_multiline_cursor_mid_word() {
    let mut e = engine_with("use gio;\nuse gtk4;\nuse std;\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 1 }; // on 's' of "use"
    e.handle_key("L", None, true);
    assert_eq!(e.view().extra_cursors.len(), 2);
    // Primary: anchor=0, cursor=2 on line 0
    assert_eq!(e.visual_anchor.unwrap(), Cursor { line: 0, col: 0 });
    assert_eq!(e.cursor().col, 2);
    // Extra cursors at end of "use" on lines 1 and 2
    assert_eq!(e.view().extra_cursors[0], Cursor { line: 1, col: 2 });
    assert_eq!(e.view().extra_cursors[1], Cursor { line: 2, col: 2 });
    // Type space — should replace all "use" with " "
    e.handle_key("space", Some(' '), false);
    assert_eq!(buf(&e), "  gio;\n  gtk4;\n  std;\n");
}

#[test]
fn test_vscode_indent_single_line() {
    let mut e = engine_with("hello\nworld\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("bracketright", None, true);
    let lines = get_lines(&e);
    assert!(lines[0].starts_with("    ")); // default shift_width = 4
    assert_eq!(lines[1], "world");
}

#[test]
fn test_vscode_indent_multi_cursor() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    // Add extra cursors on lines 1 and 2
    e.view_mut().extra_cursors = vec![Cursor { line: 1, col: 0 }, Cursor { line: 2, col: 0 }];
    e.handle_key("bracketright", None, true);
    let lines = get_lines(&e);
    assert!(lines[0].starts_with("    "));
    assert!(lines[1].starts_with("    "));
    assert!(lines[2].starts_with("    "));
}

#[test]
fn test_vscode_outdent_multi_cursor() {
    let mut e = engine_with("    aaa\n    bbb\n    ccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 4 };
    e.view_mut().extra_cursors = vec![Cursor { line: 1, col: 4 }, Cursor { line: 2, col: 4 }];
    e.handle_key("bracketleft", None, true);
    let lines = get_lines(&e);
    assert_eq!(lines[0], "aaa");
    assert_eq!(lines[1], "bbb");
    assert_eq!(lines[2], "ccc");
}

#[test]
fn test_vscode_outdent_single_line() {
    let mut e = engine_with("    hello\nworld\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 4 };
    e.handle_key("bracketleft", None, true);
    let lines = get_lines(&e);
    assert_eq!(lines[0], "hello");
}

#[test]
fn test_vscode_indent_with_selection() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.visual_anchor = Some(Cursor { line: 0, col: 0 });
    e.mode = Mode::Visual;
    e.view_mut().cursor = Cursor { line: 1, col: 2 };
    e.handle_key("bracketright", None, true);
    let lines = get_lines(&e);
    assert!(lines[0].starts_with("    "));
    assert!(lines[1].starts_with("    "));
    assert_eq!(lines[2], "ccc");
}

#[test]
fn test_vscode_shift_tab_outdent() {
    let mut e = engine_with("    hello\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 4 };
    vk(&mut e, "ISO_Left_Tab");
    let lines = get_lines(&e);
    assert_eq!(lines[0], "hello");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 3: Panel Toggles + Quick Navigation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_vscode_ctrl_g_goto_line() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.handle_key("g", Some('g'), true);
    assert_eq!(e.mode, Mode::Command);
}

#[test]
fn test_vscode_ctrl_p_fuzzy_finder() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.handle_key("p", Some('p'), true);
    assert!(e.picker_open);
}

#[test]
fn test_vscode_ctrl_shift_p_command_palette() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.handle_key("P", None, true);
    assert!(e.picker_open);
}

#[test]
fn test_vscode_ctrl_b_toggle_sidebar() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    let action = e.handle_key("b", Some('b'), true);
    assert_eq!(action, vimcode_core::EngineAction::ToggleSidebar);
}

#[test]
fn test_vscode_ctrl_j_toggle_terminal() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    // First press with no panes returns OpenTerminal so the backend can create a pane
    let action = e.handle_key("j", Some('j'), true);
    assert_eq!(action, vimcode_core::EngineAction::OpenTerminal);
}

#[test]
fn test_vscode_ctrl_backtick_toggle_terminal() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    // First press with no panes returns OpenTerminal so the backend can create a pane
    let action = e.handle_key("grave", None, true);
    assert_eq!(action, vimcode_core::EngineAction::OpenTerminal);
}

#[test]
fn test_vscode_ctrl_comma_settings() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.handle_key("comma", None, true);
    assert!(e.settings_has_focus);
}

#[test]
fn test_vscode_ctrl_k_chord_pending() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.handle_key("k", Some('k'), true);
    assert!(e.vscode_pending_ctrl_k);
    assert!(e.message.contains("Ctrl+K"));
}

#[test]
fn test_vscode_ctrl_k_ctrl_f_format() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    // Set up chord: Ctrl+K first
    e.handle_key("k", Some('k'), true);
    assert!(e.vscode_pending_ctrl_k);
    // Then Ctrl+F
    e.handle_key("f", Some('f'), true);
    assert!(!e.vscode_pending_ctrl_k);
}

#[test]
fn test_vscode_ctrl_k_invalid_clears() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.handle_key("k", Some('k'), true);
    assert!(e.vscode_pending_ctrl_k);
    // Press something that isn't a valid chord continuation
    e.handle_key("x", Some('x'), true);
    assert!(!e.vscode_pending_ctrl_k);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 4: Folding + Miscellaneous
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_vscode_alt_z_toggle_wrap() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    let before = e.settings.wrap;
    e.handle_key("Alt_z", None, false);
    assert_ne!(e.settings.wrap, before);
    // Toggle back
    e.handle_key("Alt_z", None, false);
    assert_eq!(e.settings.wrap, before);
}

#[test]
fn test_vscode_fold_unfold() {
    // Create foldable content (indented block)
    let mut e = engine_with("fn main() {\n    let x = 1;\n    let y = 2;\n}\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    // Update syntax so fold detection works
    e.update_syntax();
    // Ctrl+Shift+[ → fold
    e.handle_key("Shift_bracketleft", None, true);
    // Ctrl+Shift+] → unfold
    e.handle_key("Shift_bracketright", None, true);
    // Just verify no crash; fold behavior depends on tree-sitter
}

// ═══════════════════════════════════════════════════════════════════════════════
// Edge Cases
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_vscode_move_line_preserves_content() {
    let mut e = engine_with("line1\nline2\nline3\nline4\nline5\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 2, col: 0 };
    // Move line3 up twice
    e.handle_key("Alt_Up", None, false);
    e.handle_key("Alt_Up", None, false);
    assert_eq!(buf(&e), "line3\nline1\nline2\nline4\nline5\n");
}

#[test]
fn test_vscode_alt_shift_preserves_cursor_col() {
    let mut e = engine_with("hello world\nsecond\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 5 };
    e.handle_key("Alt_Shift_Down", None, false);
    // Primary cursor stays at col 5, extra cursor added at (1, 5)
    assert_eq!(e.cursor().col, 5);
    assert_eq!(e.view().extra_cursors[0].col, 5);
}

#[test]
fn test_vscode_delete_line_single_line_buffer() {
    let mut e = engine_with("only line\n");
    vscode_mode(&mut e);
    e.handle_key("K", None, true);
    // Should handle gracefully
    assert!(buf(&e).is_empty() || buf(&e) == "\n");
}

#[test]
fn test_vscode_ctrl_l_then_delete_replaces_line() {
    let mut e = engine_with("hello\nworld\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    // Select line
    e.handle_key("l", Some('l'), true);
    // Type replacement (BackSpace to delete selection, then type)
    e.handle_key("BackSpace", None, false);
    let content = buf(&e);
    // The selected text should be gone
    assert!(!content.starts_with("hello"));
}

#[test]
fn test_vscode_indent_outdent_roundtrip() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    // Indent
    e.handle_key("bracketright", None, true);
    assert_eq!(get_lines(&e)[0], "    hello");
    // Outdent
    e.handle_key("bracketleft", None, true);
    assert_eq!(get_lines(&e)[0], "hello");
}

#[test]
fn test_vscode_mode_basic_typing_still_works() {
    let mut e = engine_with("");
    vscode_mode(&mut e);
    e.handle_key("h", Some('h'), false);
    e.handle_key("i", Some('i'), false);
    assert_eq!(buf(&e), "hi");
}

#[test]
fn test_vscode_undo_redo_still_works() {
    let mut e = engine_with("original\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 8 };
    e.handle_key("!", Some('!'), false);
    assert!(buf(&e).contains('!'));
    // Undo
    e.handle_key("z", Some('z'), true);
    assert_eq!(buf(&e), "original\n");
    // Redo
    e.handle_key("y", Some('y'), true);
    assert!(buf(&e).contains('!'));
}

#[test]
fn test_vscode_move_multiline_selection_down() {
    let mut e = engine_with("aaa\nbbb\nccc\nddd\n");
    vscode_mode(&mut e);
    e.visual_anchor = Some(Cursor { line: 0, col: 0 });
    e.mode = Mode::Visual;
    e.view_mut().cursor = Cursor { line: 1, col: 2 };
    e.handle_key("Alt_Down", None, false);
    assert_eq!(buf(&e), "ccc\naaa\nbbb\nddd\n");
}

#[test]
fn test_vscode_escape_dismisses_completion() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    // Simulate completion popup being open
    e.completion_candidates = vec!["hello".to_string(), "help".to_string()];
    e.completion_idx = Some(0);
    e.completion_display_only = true;
    // Press Escape
    vk(&mut e, "Escape");
    assert!(e.completion_candidates.is_empty());
    assert!(e.completion_idx.is_none());
}

#[test]
fn test_vscode_escape_clears_extra_cursors() {
    let mut e = engine_with("hello\nworld\n");
    vscode_mode(&mut e);
    e.add_cursor_at_pos(1, 0);
    assert_eq!(e.view().extra_cursors.len(), 1);
    vk(&mut e, "Escape");
    assert!(e.view().extra_cursors.is_empty());
}

#[test]
fn test_vscode_escape_priority_completion_then_cursors_then_selection() {
    let mut e = engine_with("hello\nworld\n");
    vscode_mode(&mut e);
    // Set up: completion + extra cursors + selection
    e.completion_candidates = vec!["test".to_string()];
    e.completion_idx = Some(0);
    e.add_cursor_at_pos(1, 0);
    e.visual_anchor = Some(Cursor { line: 0, col: 0 });
    e.mode = Mode::Visual;
    // First Escape: dismiss completion
    vk(&mut e, "Escape");
    assert!(e.completion_candidates.is_empty());
    assert!(!e.view().extra_cursors.is_empty()); // cursors still there
                                                 // Second Escape: clear cursors
    vk(&mut e, "Escape");
    assert!(e.view().extra_cursors.is_empty());
    // Third Escape: clear selection
    vk(&mut e, "Escape");
    assert!(e.visual_anchor.is_none());
}

#[test]
fn test_vscode_ctrl_d_no_word_noop() {
    let mut e = engine_with("   \n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 1 };
    e.handle_key("d", Some('d'), true);
    // Should not crash; no selection if no word
}

#[test]
fn test_vscode_ctrl_d_at_word_start() {
    let mut e = engine_with("mod core;\nmod icons;\nmod render;\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 }; // on 'm' of "mod"
    e.handle_key("d", Some('d'), true);
    assert!(e.visual_anchor.is_some());
    assert_eq!(e.visual_anchor.unwrap().col, 0);
    assert_eq!(e.cursor().col, 2); // "mod" is cols 0-2
                                   // Second Ctrl+D: add cursor at next "mod" on line 1
    e.handle_key("d", Some('d'), true);
    assert_eq!(e.view().extra_cursors.len(), 1);
    assert_eq!(e.view().extra_cursors[0], Cursor { line: 1, col: 2 });
    // Third Ctrl+D: add cursor at next "mod" on line 2
    e.handle_key("d", Some('d'), true);
    assert_eq!(e.view().extra_cursors.len(), 2);
    assert_eq!(e.view().extra_cursors[1], Cursor { line: 2, col: 2 });
}

// ═══════════════════════════════════════════════════════════════════════════════
// Multi-cursor typing
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_vscode_multi_cursor_type_char() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    // Add cursors on lines 1 and 2
    e.handle_key("Alt_Shift_Down", None, false);
    e.handle_key("Alt_Shift_Down", None, false);
    assert_eq!(e.view().extra_cursors.len(), 2);
    // Type 'X' — should insert at all 3 cursor positions
    e.handle_key("X", Some('X'), false);
    let lines = get_lines(&e);
    assert_eq!(lines[0], "Xaaa");
    assert_eq!(lines[1], "Xbbb");
    assert_eq!(lines[2], "Xccc");
}

#[test]
fn test_vscode_multi_cursor_backspace() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 1 };
    e.handle_key("Alt_Shift_Down", None, false);
    e.handle_key("Alt_Shift_Down", None, false);
    // Backspace — should delete first char on all 3 lines
    e.handle_key("BackSpace", None, false);
    let lines = get_lines(&e);
    assert_eq!(lines[0], "aa");
    assert_eq!(lines[1], "bb");
    assert_eq!(lines[2], "cc");
}

#[test]
fn test_vscode_ctrl_d_then_type_replaces_all() {
    let mut e = engine_with("mod core;\nmod icons;\nmod render;\nmod tui_main;\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    // Ctrl+D four times to select all "mod" occurrences
    e.handle_key("d", Some('d'), true); // select "mod" on line 0
    e.handle_key("d", Some('d'), true); // add cursor at "mod" on line 1
    e.handle_key("d", Some('d'), true); // add cursor at "mod" on line 2
    e.handle_key("d", Some('d'), true); // add cursor at "mod" on line 3
    assert_eq!(e.view().extra_cursors.len(), 3);
    // Type 'use' — should replace "mod" with "use" at all 4 positions
    e.handle_key("u", Some('u'), false);
    e.handle_key("s", Some('s'), false);
    e.handle_key("e", Some('e'), false);
    let lines = get_lines(&e);
    assert_eq!(lines[0], "use core;");
    assert_eq!(lines[1], "use icons;");
    assert_eq!(lines[2], "use render;");
    assert_eq!(lines[3], "use tui_main;");
}

#[test]
fn test_vscode_ctrl_d_then_backspace_deletes_all() {
    let mut e = engine_with("mod core;\nmod icons;\nmod render;\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("d", Some('d'), true); // select "mod" on line 0
    e.handle_key("d", Some('d'), true); // add "mod" on line 1
    e.handle_key("d", Some('d'), true); // add "mod" on line 2
    assert_eq!(e.view().extra_cursors.len(), 2);
    // Backspace should delete all selected "mod" words
    e.handle_key("BackSpace", None, false);
    let lines = get_lines(&e);
    assert_eq!(lines[0], " core;");
    assert_eq!(lines[1], " icons;");
    assert_eq!(lines[2], " render;");
}

#[test]
fn test_vscode_multi_cursor_escape_clears() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Alt_Shift_Down", None, false);
    assert_eq!(e.view().extra_cursors.len(), 1);
    e.handle_key("Escape", None, false);
    assert_eq!(e.view().extra_cursors.len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 4: Fold operations + Tab indicators + Menu
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_vscode_ctrl_shift_bracket_left_folds_from_header() {
    // Cursor on fold header line — should fold the block.
    let mut e = engine_with("fn main() {\n    let x = 1;\n    let y = 2;\n}\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Shift_bracketleft", None, true);
    assert!(e.view().fold_at(0).is_some(), "fold should exist at line 0");
    assert!(e.view().is_line_hidden(1));
    assert!(e.view().is_line_hidden(2));
}

#[test]
fn test_vscode_ctrl_shift_bracket_left_folds_from_body() {
    // Cursor inside the body — should find enclosing block and fold it.
    let mut e = engine_with("fn main() {\n    let x = 1;\n    let y = 2;\n}\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 1, col: 4 };
    e.handle_key("Shift_bracketleft", None, true);
    assert!(e.view().fold_at(0).is_some(), "fold should exist at line 0");
    assert!(e.view().is_line_hidden(1));
    assert!(e.view().is_line_hidden(2));
    // Cursor should move to the fold header
    assert_eq!(e.cursor().line, 0);
}

#[test]
fn test_vscode_ctrl_shift_bracket_right_unfolds_region() {
    let mut e = engine_with("fn main() {\n    let x = 1;\n    let y = 2;\n}\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    // Fold first
    e.handle_key("Shift_bracketleft", None, true);
    assert!(e.view().fold_at(0).is_some());
    // Unfold
    e.handle_key("Shift_bracketright", None, true);
    assert!(e.view().fold_at(0).is_none(), "fold should be removed");
    assert!(!e.view().is_line_hidden(1));
}

#[test]
fn test_vscode_progressive_fold_parent() {
    // Nested indentation: first fold inner block, then fold parent.
    let src = "fn outer() {\n    fn inner() {\n        let x = 1;\n    }\n}\n";
    let mut e = engine_with(src);
    vscode_mode(&mut e);
    // Cursor on inner function
    e.view_mut().cursor = Cursor { line: 1, col: 0 };
    // First Ctrl+Shift+[ folds the inner block
    e.handle_key("Shift_bracketleft", None, true);
    assert!(e.view().fold_at(1).is_some(), "inner fold should exist");
    assert!(!e.view().is_line_hidden(0), "outer line 0 still visible");
    // Second press: cursor is now on fold header (line 1), should fold parent (line 0)
    e.handle_key("Shift_bracketleft", None, true);
    assert!(e.view().fold_at(0).is_some(), "outer fold should now exist");
    // Cursor should move to the parent fold header
    assert_eq!(e.cursor().line, 0);
}

#[test]
fn test_vscode_progressive_unfold() {
    // Fold a region, then unfold it. The flat fold model merges nested folds
    // into one, so unfolding the outer fold reveals all nested content.
    let src = "fn outer() {\n    fn inner() {\n        let x = 1;\n    }\n}\n";
    let mut e = engine_with(src);
    vscode_mode(&mut e);
    // Fold inner first
    e.view_mut().cursor = Cursor { line: 1, col: 0 };
    e.handle_key("Shift_bracketleft", None, true);
    assert!(e.view().fold_at(1).is_some(), "inner fold exists");
    // Progressive fold: now fold outer (cursor is on fold header line 1)
    e.handle_key("Shift_bracketleft", None, true);
    assert_eq!(e.cursor().line, 0);
    assert!(e.view().fold_at(0).is_some(), "outer fold exists");
    // The inner fold was absorbed by the outer fold
    assert!(e.view().fold_at(1).is_none(), "inner fold absorbed");
    // Unfold the outer fold — all lines become visible
    e.handle_key("Shift_bracketright", None, true);
    assert!(e.view().fold_at(0).is_none(), "outer fold removed");
    assert!(!e.view().is_line_hidden(1), "line 1 visible");
    assert!(!e.view().is_line_hidden(2), "line 2 visible");
    assert!(!e.view().is_line_hidden(3), "line 3 visible");
}

#[test]
fn test_tab_dirty_marker_no_asterisk() {
    // Tab display_name() should not contain asterisk — the modified dot
    // indicator is rendered separately by the backends.
    let mut e = engine_with("hello\n");
    // Make buffer dirty
    e.handle_key("i", None, false);
    e.handle_key("x", Some('x'), false);
    e.handle_key("Escape", None, false);
    let win_id = e.active_window_id();
    let buf_id = e.windows[&win_id].buffer_id;
    let state = e.buffer_manager.get(buf_id).unwrap();
    assert!(state.dirty, "buffer should be dirty");
    let name = state.display_name();
    // The display_name itself never had an asterisk; we verify the dirty
    // flag is set so that backends can show the ● indicator.
    assert!(
        !name.contains('*'),
        "display_name should not contain asterisk: {}",
        name
    );
}

#[test]
fn test_word_wrap_toggle_via_menu_action() {
    let mut e = engine_with("hello\n");
    assert!(!e.settings.wrap, "wrap should default to false");
    // The menu dispatches "set_wrap_toggle" through execute_command.
    e.execute_command("set_wrap_toggle");
    assert!(e.settings.wrap, "wrap should be true after toggle");
    e.execute_command("set_wrap_toggle");
    assert!(!e.settings.wrap, "wrap should be false after second toggle");
}
