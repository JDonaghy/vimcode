mod common;
use common::*;
use vimcode_core::Mode;

// =============================================================================
// Ctrl-G: show file info
// =============================================================================

#[test]
fn test_ctrl_g_shows_file_info() {
    let mut e = engine_with("line one\nline two\nline three\n");
    ctrl(&mut e, 'g');
    assert!(e.message.contains("line 1 of 3"), "msg: {}", e.message);
    assert!(e.message.contains("--33%--"), "msg: {}", e.message);
    assert!(e.message.contains("col 1"), "msg: {}", e.message);
}

#[test]
fn test_ctrl_g_shows_no_name_for_unsaved() {
    let mut e = engine_with("hello\n");
    ctrl(&mut e, 'g');
    assert!(e.message.contains("[No Name]"), "msg: {}", e.message);
}

#[test]
fn test_ctrl_g_shows_modified() {
    let mut e = engine_with("hello\n");
    // Make a change to mark dirty
    type_chars(&mut e, "iX");
    press_key(&mut e, "Escape");
    ctrl(&mut e, 'g');
    assert!(e.message.contains("[Modified]"), "msg: {}", e.message);
}

// =============================================================================
// gi: insert at last insert position
// =============================================================================

#[test]
fn test_gi_enters_insert_at_last_pos() {
    let mut e = engine_with("hello world\nsecond line\n");
    // Move to line 2, col 3, enter insert, type, escape
    type_chars(&mut e, "jllli");
    assert_eq!(e.mode, Mode::Insert);
    type_chars(&mut e, "X");
    press_key(&mut e, "Escape");
    assert_eq!(e.mode, Mode::Normal);
    // Now go to line 1 and use gi
    type_chars(&mut e, "gg");
    type_chars(&mut e, "gi");
    assert_eq!(e.mode, Mode::Insert);
    // Cursor should be back near where we left insert mode on line 2
    let c = e.cursor();
    assert_eq!(c.line, 1, "should return to line 1");
}

#[test]
fn test_gi_no_previous_insert_still_enters_insert() {
    let mut e = engine_with("hello\n");
    type_chars(&mut e, "gi");
    assert_eq!(e.mode, Mode::Insert);
}

#[test]
fn test_gi_clamps_to_buffer_bounds() {
    let mut e = engine_with("line1\nline2\nline3\n");
    // Insert at end of buffer
    type_chars(&mut e, "GA");
    type_chars(&mut e, "X");
    press_key(&mut e, "Escape");
    // Delete last lines to make buffer shorter
    type_chars(&mut e, "Gdd");
    type_chars(&mut e, "Gdd");
    // gi should clamp to valid buffer position
    type_chars(&mut e, "gi");
    assert_eq!(e.mode, Mode::Insert);
}

// =============================================================================
// Ctrl-W r / R: rotate windows
// =============================================================================

#[test]
fn test_ctrl_w_r_rotates_windows() {
    let mut e = engine_with("file1\n");
    // Create a split
    exec(&mut e, "split");
    // Modify the second window content
    set_content(&mut e, "file2\n");
    // Get the window IDs and their buffers before rotation
    let tab = e.active_tab();
    let ids = tab.layout.window_ids();
    assert!(ids.len() >= 2, "should have at least 2 windows");
    let buf0_before = e.windows[&ids[0]].buffer_id;
    let buf1_before = e.windows[&ids[1]].buffer_id;
    // Rotate forward (Ctrl-W r)
    press(&mut e, '\x17'); // Ctrl-W
    press(&mut e, 'r');
    let tab = e.active_tab();
    let ids = tab.layout.window_ids();
    let buf0_after = e.windows[&ids[0]].buffer_id;
    let buf1_after = e.windows[&ids[1]].buffer_id;
    // After rotation, buffers should be swapped
    assert_eq!(buf0_after, buf1_before);
    assert_eq!(buf1_after, buf0_before);
}

#[test]
fn test_ctrl_w_capital_r_rotates_reverse() {
    let mut e = engine_with("file1\n");
    exec(&mut e, "split");
    set_content(&mut e, "file2\n");
    let tab = e.active_tab();
    let ids = tab.layout.window_ids();
    let buf0_before = e.windows[&ids[0]].buffer_id;
    let buf1_before = e.windows[&ids[1]].buffer_id;
    // Rotate backward (Ctrl-W R)
    press(&mut e, '\x17');
    press(&mut e, 'R');
    let tab = e.active_tab();
    let ids = tab.layout.window_ids();
    let buf0_after = e.windows[&ids[0]].buffer_id;
    let buf1_after = e.windows[&ids[1]].buffer_id;
    assert_eq!(buf0_after, buf1_before);
    assert_eq!(buf1_after, buf0_before);
}

// =============================================================================
// [* / ]* and [/ / ]/: comment block navigation
// =============================================================================

#[test]
fn test_bracket_star_jump_comment_end() {
    let mut e = engine_with("/* start\n * middle\n */ end\nnormal\n");
    // ]* should jump to the line with */
    type_chars(&mut e, "]*");
    assert_eq!(e.cursor().line, 2);
}

#[test]
fn test_bracket_star_jump_comment_start() {
    let mut e = engine_with("normal\n/* start\n * middle\n */ end\n");
    // Move to last line, then [* should jump to /* line
    type_chars(&mut e, "G");
    type_chars(&mut e, "[*");
    assert_eq!(e.cursor().line, 1);
}

#[test]
fn test_bracket_slash_jump_comment_end() {
    let mut e = engine_with("/* comment */\nnormal\n");
    // ]/ is alias for ]* — should find */
    type_chars(&mut e, "]/");
    assert_eq!(e.cursor().line, 0);
}

#[test]
fn test_bracket_slash_jump_comment_start() {
    let mut e = engine_with("normal\n/* comment */\n");
    type_chars(&mut e, "G");
    type_chars(&mut e, "[/");
    assert_eq!(e.cursor().line, 1);
}

#[test]
fn test_bracket_star_no_comment_stays() {
    let mut e = engine_with("no comments here\nstill none\n");
    let orig_line = e.cursor().line;
    type_chars(&mut e, "]*");
    assert_eq!(e.cursor().line, orig_line);
}

// =============================================================================
// do / dp: diff obtain / diff put
// =============================================================================

#[test]
fn test_do_not_in_diff_mode() {
    let mut e = engine_with("hello\n");
    type_chars(&mut e, "do");
    assert!(e.message.contains("Not in diff mode"), "msg: {}", e.message);
}

#[test]
fn test_dp_not_in_diff_mode() {
    let mut e = engine_with("hello\n");
    type_chars(&mut e, "dp");
    assert!(e.message.contains("Not in diff mode"), "msg: {}", e.message);
}

// =============================================================================
// o_CTRL-V: force blockwise motion
// =============================================================================

#[test]
fn test_force_blockwise_delete() {
    let mut e = engine_with("abcde\nfghij\nklmno\n");
    // d<C-v>j should delete a block (cursor col on two lines)
    // Set force_motion_mode via Ctrl-V, then motion j
    type_chars(&mut e, "d");
    ctrl(&mut e, 'v'); // Force blockwise
    type_chars(&mut e, "j"); // Motion: down one line
                             // Should have deleted column 0 on lines 0 and 1
    let text = buf(&e);
    // The 'a' from line 0 and 'f' from line 1 should be deleted
    assert!(
        text.starts_with("bcde\n"),
        "first line should lose col 0: {}",
        text
    );
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "ghij", "second line should lose col 0");
}

#[test]
fn test_force_blockwise_yank() {
    let mut e = engine_with("abcde\nfghij\nklmno\n");
    // y<C-v>j should yank a block
    type_chars(&mut e, "y");
    ctrl(&mut e, 'v');
    type_chars(&mut e, "j");
    // Check that something was yanked (register should have content)
    let reg = e.registers.get(&'"').cloned();
    assert!(reg.is_some(), "should have yanked to default register");
    let (text, _) = reg.unwrap();
    assert!(
        text.contains('a'),
        "yanked text should contain 'a': {}",
        text
    );
    assert!(
        text.contains('f'),
        "yanked text should contain 'f': {}",
        text
    );
}

// =============================================================================
// leader gi: LSP go to implementation (remapped)
// =============================================================================

#[test]
fn test_leader_gi_stays_in_normal_mode() {
    let mut e = engine_with("fn foo() {}\n");
    // Press leader key (default is space)
    press(&mut e, ' ');
    type_chars(&mut e, "gi");
    // Should stay in Normal mode — LSP request sent (no-op without file/server)
    assert_eq!(e.mode, Mode::Normal);
}

// =============================================================================
// g' / g` mark jumps without jumplist (doc fix verification)
// =============================================================================

#[test]
fn test_g_apostrophe_mark_without_jumplist() {
    let mut e = engine_with("line1\nline2\nline3\nline4\n");
    // Set mark 'a' at line 2
    type_chars(&mut e, "jjma");
    // Go to line 0
    type_chars(&mut e, "gg");
    // Use g'a (mark jump without jumplist)
    type_chars(&mut e, "g'a");
    assert_eq!(e.cursor().line, 2, "should jump to mark a on line 2");
}

// =============================================================================
// Verify [z / ]z fold navigation (already implemented — regression test)
// =============================================================================

#[test]
fn test_bracket_z_fold_start() {
    let mut e = engine_with("fn main() {\n    inner\n    code\n}\n");
    // Create a fold from line 0 to line 2
    e.view_mut()
        .folds
        .push(vimcode_core::core::view::FoldRegion { start: 0, end: 2 });
    // Move cursor into the fold (line 1)
    e.view_mut().cursor.line = 1;
    // [z should go to fold start
    type_chars(&mut e, "[z");
    assert_eq!(e.cursor().line, 0);
}

#[test]
fn test_bracket_z_fold_end() {
    let mut e = engine_with("fn main() {\n    inner\n    code\n}\n");
    e.view_mut()
        .folds
        .push(vimcode_core::core::view::FoldRegion { start: 0, end: 2 });
    e.view_mut().cursor.line = 1;
    // ]z should go to fold end
    type_chars(&mut e, "]z");
    assert_eq!(e.cursor().line, 2);
}
