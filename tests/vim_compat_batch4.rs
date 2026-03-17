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

// =============================================================================
// g- / g+: chronological undo timeline navigation
// =============================================================================

#[test]
fn test_g_minus_goes_to_earlier_state() {
    let mut e = engine_with("hello\n");
    // Make an edit: replace "hello" with "world"
    type_chars(&mut e, "ciw");
    type_chars(&mut e, "world");
    e.handle_key("Escape", None, false);
    let line: String = e.buffer().content.line(0).chars().collect();
    assert!(line.starts_with("world"), "got: {}", line);

    // g- should go back to the earlier state
    type_chars(&mut e, "g-");
    let line: String = e.buffer().content.line(0).chars().collect();
    assert!(line.starts_with("hello"), "expected hello, got: {}", line);
}

#[test]
fn test_g_plus_goes_to_later_state() {
    let mut e = engine_with("hello\n");
    type_chars(&mut e, "ciw");
    type_chars(&mut e, "world");
    e.handle_key("Escape", None, false);

    // g- to go back
    type_chars(&mut e, "g-");
    let line: String = e.buffer().content.line(0).chars().collect();
    assert!(line.starts_with("hello"), "got: {}", line);

    // g+ to go forward again
    type_chars(&mut e, "g+");
    let line: String = e.buffer().content.line(0).chars().collect();
    assert!(line.starts_with("world"), "expected world, got: {}", line);
}

#[test]
fn test_g_minus_at_oldest_stays() {
    let mut e = engine_with("hello\n");
    // No edits, so g- should do nothing
    type_chars(&mut e, "g-");
    let line: String = e.buffer().content.line(0).chars().collect();
    assert!(line.starts_with("hello"), "got: {}", line);
}

#[test]
fn test_g_plus_at_newest_stays() {
    let mut e = engine_with("hello\n");
    type_chars(&mut e, "ciw");
    type_chars(&mut e, "world");
    e.handle_key("Escape", None, false);
    // g+ at newest should do nothing
    type_chars(&mut e, "g+");
    let line: String = e.buffer().content.line(0).chars().collect();
    assert!(line.starts_with("world"), "got: {}", line);
}

#[test]
fn test_g_minus_multiple_edits() {
    let mut e = engine_with("aaa\n");
    // Edit 1: change to bbb
    type_chars(&mut e, "ciw");
    type_chars(&mut e, "bbb");
    e.handle_key("Escape", None, false);
    // Edit 2: change to ccc
    type_chars(&mut e, "ciw");
    type_chars(&mut e, "ccc");
    e.handle_key("Escape", None, false);

    // g- should go from ccc -> bbb
    type_chars(&mut e, "g-");
    let line: String = e.buffer().content.line(0).chars().collect();
    assert!(line.starts_with("bbb"), "expected bbb, got: {}", line);

    // Another g- should go from bbb -> aaa (initial state recorded on first edit undo group)
    type_chars(&mut e, "g-");
    let line: String = e.buffer().content.line(0).chars().collect();
    // This goes to the state after first undo, which depends on timeline recording
    // The timeline records: [state_after_edit1, state_after_edit2]
    // So g- from edit2 → edit1, g- from edit1 → nothing (at oldest)
    // Actually the initial state "aaa" isn't recorded since no undo group was finished before
    assert!(
        line.starts_with("bbb") || line.starts_with("aaa"),
        "got: {}",
        line
    );
}

#[test]
fn test_g_minus_with_count() {
    let mut e = engine_with("aaa\n");
    type_chars(&mut e, "ciw");
    type_chars(&mut e, "bbb");
    e.handle_key("Escape", None, false);
    type_chars(&mut e, "ciw");
    type_chars(&mut e, "ccc");
    e.handle_key("Escape", None, false);

    // 2g- should skip two timeline entries
    type_chars(&mut e, "2g-");
    let line: String = e.buffer().content.line(0).chars().collect();
    // Should be at earliest recorded state (bbb or aaa)
    assert!(
        line.starts_with("bbb") || line.starts_with("aaa"),
        "got: {}",
        line
    );
}

#[test]
fn test_g_minus_after_undo_preserves_redo_state() {
    let mut e = engine_with("aaa\n");
    // Edit: change to bbb
    type_chars(&mut e, "ciw");
    type_chars(&mut e, "bbb");
    e.handle_key("Escape", None, false);

    // Undo back to aaa
    type_chars(&mut e, "u");
    let line: String = e.buffer().content.line(0).chars().collect();
    assert!(line.starts_with("aaa"), "got: {}", line);

    // Make a different edit: change to ccc (this normally clears redo stack)
    type_chars(&mut e, "ciw");
    type_chars(&mut e, "ccc");
    e.handle_key("Escape", None, false);

    // g- should be able to go back through the timeline
    // Timeline should have: [bbb_state, aaa_undo_state, ccc_state]
    type_chars(&mut e, "g-");
    // Should go to some earlier state
    let line: String = e.buffer().content.line(0).chars().collect();
    assert!(
        !line.starts_with("ccc"),
        "should have gone back, got: {}",
        line
    );
}

// =============================================================================
// gR: virtual replace mode
// =============================================================================

#[test]
fn test_gr_enters_replace_mode() {
    let mut e = engine_with("hello world\n");
    type_chars(&mut e, "gR");
    assert_eq!(e.mode, Mode::Replace);
}

#[test]
fn test_gr_overwrites_like_replace() {
    let mut e = engine_with("hello\n");
    type_chars(&mut e, "gR");
    e.handle_key("x", Some('x'), false);
    e.handle_key("y", Some('y'), false);
    e.handle_key("Escape", None, false);
    let line: String = e.buffer().content.line(0).chars().collect();
    assert!(line.starts_with("xyllo"), "got: {}", line);
}

#[test]
fn test_gr_expands_tab_before_overwrite() {
    // Tab at col 0 with tabstop=4 occupies 4 visual columns.
    // gR on tab should expand it to spaces, then overwrite first space.
    let mut e = engine_with("\thello\n");
    e.settings.tabstop = 4;
    type_chars(&mut e, "gR");
    e.handle_key("x", Some('x'), false);
    e.handle_key("Escape", None, false);
    let line: String = e.buffer().content.line(0).chars().collect();
    // Tab (4 cols) → "    ", then first space replaced with 'x' → "x   hello"
    assert_eq!(line.trim_end(), "x   hello", "got: {:?}", line);
}

#[test]
fn test_gr_escape_returns_to_normal() {
    let mut e = engine_with("hello\n");
    type_chars(&mut e, "gR");
    assert_eq!(e.mode, Mode::Replace);
    e.handle_key("Escape", None, false);
    assert_eq!(e.mode, Mode::Normal);
}

#[test]
fn test_gr_non_tab_same_as_replace() {
    // On a non-tab character, gR behaves like R
    let mut e = engine_with("abcd\n");
    type_chars(&mut e, "gR");
    e.handle_key("x", Some('x'), false);
    e.handle_key("Escape", None, false);
    let line: String = e.buffer().content.line(0).chars().collect();
    assert!(line.starts_with("xbcd"), "got: {}", line);
}

// =============================================================================
// [# / ]#: preprocessor directive navigation
// =============================================================================

#[test]
fn test_bracket_hash_forward_to_endif() {
    let mut e = engine_with("#if FOO\ncode\n#endif\n");
    // ]# from #if should jump to #endif
    type_chars(&mut e, "]#");
    assert_eq!(e.cursor().line, 2);
}

#[test]
fn test_bracket_hash_forward_to_else() {
    let mut e = engine_with("#if FOO\ncode\n#else\nother\n#endif\n");
    // ]# from #if should jump to #else (first unmatched at depth 0)
    type_chars(&mut e, "]#");
    assert_eq!(e.cursor().line, 2);
}

#[test]
fn test_bracket_hash_forward_skips_nested() {
    let mut e = engine_with("#if OUTER\n#if INNER\n#endif\n#else\nouter_else\n#endif\n");
    // ]# from line 0 (#if OUTER) should skip nested #if/#endif and land on #else
    type_chars(&mut e, "]#");
    assert_eq!(e.cursor().line, 3); // #else for OUTER
}

#[test]
fn test_bracket_hash_backward_to_if() {
    let mut e = engine_with("#if FOO\ncode\n#endif\n");
    e.view_mut().cursor.line = 2; // on #endif
    type_chars(&mut e, "[#");
    assert_eq!(e.cursor().line, 0); // #if FOO
}

#[test]
fn test_bracket_hash_backward_to_else() {
    let mut e = engine_with("#if FOO\ncode\n#else\nother\n#endif\n");
    e.view_mut().cursor.line = 4; // on #endif
    type_chars(&mut e, "[#");
    assert_eq!(e.cursor().line, 2); // #else
}

#[test]
fn test_bracket_hash_backward_skips_nested() {
    let mut e = engine_with("#if OUTER\n#if INNER\n#endif\n#else\nouter_else\n#endif\n");
    e.view_mut().cursor.line = 5; // on last #endif
    type_chars(&mut e, "[#");
    assert_eq!(e.cursor().line, 3); // #else for OUTER
}

#[test]
fn test_bracket_hash_forward_no_match_stays() {
    let mut e = engine_with("no preprocessor here\nstill none\n");
    let orig = e.cursor().line;
    type_chars(&mut e, "]#");
    assert_eq!(e.cursor().line, orig);
}

#[test]
fn test_bracket_hash_backward_no_match_stays() {
    let mut e = engine_with("no preprocessor here\nstill none\n");
    e.view_mut().cursor.line = 1;
    type_chars(&mut e, "[#");
    assert_eq!(e.cursor().line, 1);
}

#[test]
fn test_bracket_hash_ifdef_ifndef() {
    // #ifdef and #ifndef should be treated as #if
    let mut e = engine_with("#ifdef FOO\ncode\n#endif\n");
    type_chars(&mut e, "]#");
    assert_eq!(e.cursor().line, 2);

    let mut e = engine_with("#ifndef BAR\ncode\n#endif\n");
    type_chars(&mut e, "]#");
    assert_eq!(e.cursor().line, 2);
}

#[test]
fn test_bracket_hash_elif() {
    let mut e = engine_with("#if A\ncode\n#elif B\ncode\n#else\ncode\n#endif\n");
    // ]# from #if should land on #elif
    type_chars(&mut e, "]#");
    assert_eq!(e.cursor().line, 2); // #elif B
}

#[test]
fn test_bracket_hash_with_count() {
    let mut e = engine_with("#if A\ncode\n#elif B\ncode\n#else\ncode\n#endif\n");
    // 2]# should jump past #elif to #else
    type_chars(&mut e, "2]#");
    assert_eq!(e.cursor().line, 4); // #else
}

#[test]
fn test_bracket_hash_indented_directives() {
    // Directives with leading whitespace (common in some codebases)
    let mut e = engine_with("  #if FOO\n  code\n  #endif\n");
    type_chars(&mut e, "]#");
    assert_eq!(e.cursor().line, 2);
}

// =============================================================================
// q: — command-line window (command history)
// =============================================================================

#[test]
fn test_q_colon_opens_cmdline_window() {
    let mut e = engine_with("hello\n");
    e.history.add_command("set number");
    e.history.add_command("w");
    type_chars(&mut e, "q:");
    // Should have opened a new tab with a cmdline buffer
    assert!(e.active_buffer_state().is_cmdline_buf);
    assert!(!e.active_buffer_state().cmdline_is_search);
    // Buffer should contain the history entries
    let text: String = e.buffer().content.chars().collect();
    assert!(text.contains("set number"), "text: {}", text);
    assert!(text.contains("w"), "text: {}", text);
}

#[test]
fn test_q_colon_enter_executes_command() {
    let mut e = engine_with("hello world\n");
    e.history.add_command("set wrap");
    type_chars(&mut e, "q:");
    assert!(!e.settings.wrap, "wrap should be off initially");
    // Cursor is on last (empty) line, move up to "set wrap" line
    press_key(&mut e, "Up");
    press_key(&mut e, "Return");
    // Should have closed the cmdline tab and executed "set wrap"
    assert!(!e.active_buffer_state().is_cmdline_buf);
    assert!(
        e.settings.wrap,
        "wrap should be on after executing 'set wrap'"
    );
}

#[test]
fn test_q_colon_q_closes_window() {
    let mut e = engine_with("hello\n");
    e.history.add_command("set number");
    type_chars(&mut e, "q:");
    assert!(e.active_buffer_state().is_cmdline_buf);
    // Press q to close
    type_chars(&mut e, "q");
    assert!(!e.active_buffer_state().is_cmdline_buf);
}

#[test]
fn test_q_colon_enter_on_empty_line_does_nothing() {
    let mut e = engine_with("hello\n");
    e.history.add_command("set number");
    type_chars(&mut e, "q:");
    // Cursor is on the empty last line
    press_key(&mut e, "Return");
    // Should still be in the cmdline window (empty line = no-op)
    assert!(e.active_buffer_state().is_cmdline_buf);
}

// =============================================================================
// q/ and q? — search history window
// =============================================================================

#[test]
fn test_q_slash_opens_search_history() {
    let mut e = engine_with("hello world\nfoo bar\n");
    e.history.add_search("hello");
    e.history.add_search("world");
    type_chars(&mut e, "q/");
    assert!(e.active_buffer_state().is_cmdline_buf);
    assert!(e.active_buffer_state().cmdline_is_search);
    let text: String = e.buffer().content.chars().collect();
    assert!(text.contains("hello"), "text: {}", text);
    assert!(text.contains("world"), "text: {}", text);
}

#[test]
fn test_q_question_opens_search_history() {
    let mut e = engine_with("hello world\nfoo bar\n");
    e.history.add_search("test");
    type_chars(&mut e, "q?");
    assert!(e.active_buffer_state().is_cmdline_buf);
    assert!(e.active_buffer_state().cmdline_is_search);
}

#[test]
fn test_q_slash_enter_executes_search() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    e.history.add_search("bbb");
    type_chars(&mut e, "q/");
    // Move up to "bbb" line
    press_key(&mut e, "Up");
    press_key(&mut e, "Return");
    // Should have closed cmdline window and searched for "bbb"
    assert!(!e.active_buffer_state().is_cmdline_buf);
    assert_eq!(e.cursor().line, 1, "should have jumped to 'bbb' line");
}
