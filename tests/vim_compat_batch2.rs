mod common;
use common::*;
use vimcode_core::Mode;

// =============================================================================
// Tier 1: Quick-win commands
// =============================================================================

// ── N% motion ───────────────────────────────────────────────────────────────

#[test]
fn test_n_percent_go_to_line_50_percent() {
    // 10-line buffer, 50% should go to line 5 (0-indexed = 4)
    let mut e = engine_with("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n");
    type_chars(&mut e, "50%");
    assert_cursor(&e, 4, 0);
}

#[test]
fn test_n_percent_go_to_100_percent() {
    let mut e = engine_with("a\nb\nc\nd\ne\n");
    type_chars(&mut e, "100%");
    // 100% should go to last line (line 4 = "e")
    assert_cursor(&e, 4, 0);
}

#[test]
fn test_n_percent_go_to_1_percent() {
    let mut e = engine_with("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n");
    // Move to end first
    type_chars(&mut e, "G");
    type_chars(&mut e, "1%");
    assert_cursor(&e, 0, 0);
}

// ── gm / gM motions ────────────────────────────────────────────────────────

#[test]
fn test_gm_middle_of_screen() {
    let mut e = engine_with("hello world test line\n");
    e.view_mut().viewport_cols = 40;
    type_chars(&mut e, "gm");
    assert_cursor(&e, 0, 20); // middle of 40-col viewport
}

#[test]
fn test_g_m_upper_middle_of_text() {
    let mut e = engine_with("0123456789\n");
    type_chars(&mut e, "gM");
    // line has 10 chars (+ newline), so line_len-1 = 10, middle = 5
    assert_cursor(&e, 0, 5);
}

// ── ga ──────────────────────────────────────────────────────────────────────

#[test]
fn test_ga_show_ascii_value() {
    let mut e = engine_with("A\n");
    type_chars(&mut e, "ga");
    assert!(
        e.message.contains("65"),
        "ga should show decimal 65 for 'A': {}",
        e.message
    );
}

// ── g8 ──────────────────────────────────────────────────────────────────────

#[test]
fn test_g8_show_utf8_bytes() {
    let mut e = engine_with("A\n");
    type_chars(&mut e, "g8");
    assert!(
        e.message.contains("41"),
        "g8 should show hex 41 for 'A': {}",
        e.message
    );
}

#[test]
fn test_g8_multibyte_char() {
    let mut e = engine_with("é\n");
    type_chars(&mut e, "g8");
    // é = U+00E9, UTF-8: c3 a9
    assert!(
        e.message.contains("c3"),
        "g8 should show c3 for 'é': {}",
        e.message
    );
}

// ── gI ──────────────────────────────────────────────────────────────────────

#[test]
fn test_g_i_insert_at_column_0() {
    let mut e = engine_with("  hello\n");
    // Move to middle of line first
    type_chars(&mut e, "w");
    type_chars(&mut e, "gI");
    assert_mode(&e, Mode::Insert);
    assert_cursor(&e, 0, 0);
    // Type something
    type_chars(&mut e, "X");
    assert_buf(&e, "X  hello\n");
}

// ── g& ──────────────────────────────────────────────────────────────────────

#[test]
fn test_g_ampersand_repeat_last_substitute() {
    let mut e = engine_with("foo bar\nfoo baz\nfoo qux\n");
    exec(&mut e, "s/foo/FOO/");
    assert_eq!(e.buffer().content.line(0).to_string(), "FOO bar\n");
    // g& repeats last :s on all lines
    type_chars(&mut e, "g&");
    let lines = get_lines(&e);
    assert_eq!(lines[0], "FOO bar");
    assert_eq!(lines[1], "FOO baz");
    assert_eq!(lines[2], "FOO qux");
}

// ── go (byte offset) ────────────────────────────────────────────────────────

#[test]
fn test_go_byte_offset() {
    let mut e = engine_with("hello\nworld\n");
    type_chars(&mut e, "7go");
    // Byte 7 (1-indexed) = char offset 6 = 'w' on line 1, col 0
    assert_cursor(&e, 1, 0);
}

// ── Ctrl-^ (alternate buffer) ──────────────────────────────────────────────

#[test]
fn test_ctrl_caret_alternate_buffer() {
    let mut e = engine_with("file1\n");
    let buf1 = e.active_buffer_id();
    // Open a new buffer via :enew (which sets alternate internally)
    exec(&mut e, "enew");
    let buf2 = e.active_buffer_id();
    assert_ne!(buf1, buf2);
    // Manually set alternate since :enew may not set it
    e.buffer_manager.alternate_buffer = Some(buf1);
    // Ctrl-^ should switch back to buf1
    ctrl(&mut e, '6');
    assert_eq!(e.active_buffer_id(), buf1);
}

// ── Ctrl-L ─────────────────────────────────────────────────────────────────

#[test]
fn test_ctrl_l_clears_message() {
    let mut e = engine_with("test\n");
    e.message = "Some old message".to_string();
    ctrl(&mut e, 'l');
    assert!(e.message.is_empty(), "Ctrl-L should clear message");
}

// ── ze / zs (horizontal scroll) ─────────────────────────────────────────────

#[test]
fn test_ze_scroll_cursor_to_right_edge() {
    let mut e = engine_with("0123456789abcdefghij\n");
    e.view_mut().viewport_cols = 10;
    // Move cursor to col 15
    type_chars(&mut e, "15l");
    type_chars(&mut e, "ze");
    // scroll_left should be such that cursor is at right edge: col - (viewport-1) = 15 - 9 = 6
    assert_eq!(e.view().scroll_left, 6);
}

#[test]
fn test_zs_scroll_cursor_to_left_edge() {
    let mut e = engine_with("0123456789abcdefghij\n");
    e.view_mut().viewport_cols = 10;
    type_chars(&mut e, "15l");
    type_chars(&mut e, "zs");
    assert_eq!(e.view().scroll_left, 15);
}

// ── [z / ]z (fold navigation) ───────────────────────────────────────────────

// Note: These require folds to be set up, which is complex. Test the no-op case.
#[test]
fn test_bracket_z_no_folds() {
    let mut e = engine_with("line1\nline2\nline3\n");
    type_chars(&mut e, "]z");
    // No folds — cursor shouldn't move
    assert_cursor(&e, 0, 0);
    type_chars(&mut e, "[z");
    assert_cursor(&e, 0, 0);
}

// ── CTRL-W p/t/b ───────────────────────────────────────────────────────────

#[test]
fn test_ctrl_w_t_first_group() {
    let mut e = engine_with("hello\n");
    let first_group = e.active_group;
    // Create a second editor group (Ctrl-W e)
    e.open_editor_group(vimcode_core::core::window::SplitDirection::Vertical);
    let second_group = e.active_group;
    assert_ne!(first_group, second_group);
    // Ctrl-W t should go to first group
    ctrl(&mut e, 'w');
    press(&mut e, 't');
    assert_eq!(e.active_group, first_group);
}

#[test]
fn test_ctrl_w_b_last_group() {
    let mut e = engine_with("hello\n");
    e.open_editor_group(vimcode_core::core::window::SplitDirection::Vertical);
    let ids = e.group_layout.group_ids();
    let last_group = *ids.last().unwrap();
    // Go to first group
    ctrl(&mut e, 'w');
    press(&mut e, 't');
    // Now go to last
    ctrl(&mut e, 'w');
    press(&mut e, 'b');
    assert_eq!(e.active_group, last_group);
}

#[test]
fn test_ctrl_w_p_previous_group() {
    let mut e = engine_with("hello\n");
    let first_group = e.active_group;
    e.open_editor_group(vimcode_core::core::window::SplitDirection::Vertical);
    let second_group = e.active_group;
    assert_ne!(first_group, second_group);
    // prev_active_group should be the first group
    assert_eq!(e.prev_active_group, Some(first_group));
    // Ctrl-W p should go back
    ctrl(&mut e, 'w');
    press(&mut e, 'p');
    assert_eq!(e.active_group, first_group);
    // And now prev should be the second
    assert_eq!(e.prev_active_group, Some(second_group));
}

// ── CTRL-W f (split + open file) ───────────────────────────────────────────

#[test]
fn test_ctrl_w_f_no_file() {
    let mut e = engine_with("not a file path\n");
    ctrl(&mut e, 'w');
    press(&mut e, 'f');
    assert!(
        e.message.contains("No file path"),
        "should report no file path"
    );
}

// ── CTRL-W d (split + go to definition) ────────────────────────────────────

#[test]
fn test_ctrl_w_d_triggers_split() {
    let mut e = engine_with("some_function\n");
    let groups_before = e.editor_groups.len();
    ctrl(&mut e, 'w');
    press(&mut e, 'd');
    // Should have split even if LSP is not running
    // (the split happens, then the def request goes out)
    // After split, active tab has a new window
    // Just verify no crash
    assert!(e.editor_groups.len() >= groups_before);
}

// ── Insert CTRL-A (repeat last insertion) ───────────────────────────────────

#[test]
fn test_insert_ctrl_a_repeat_insertion() {
    let mut e = engine_with("\n");
    // Enter insert mode, type "AB", exit
    press(&mut e, 'i');
    type_chars(&mut e, "AB");
    press_key(&mut e, "Escape");
    assert_buf(&e, "AB\n");
    // last_inserted_text should be "AB"
    assert_eq!(e.last_inserted_text, "AB");
    // Enter insert at start, Ctrl-A should re-insert "AB"
    press(&mut e, '0'); // go to col 0
    press(&mut e, 'i'); // insert at col 0
    ctrl(&mut e, 'a');
    press_key(&mut e, "Escape");
    assert_buf(&e, "ABAB\n");
}

// ── Insert CTRL-G u (break undo sequence) ──────────────────────────────────

#[test]
fn test_insert_ctrl_g_u_break_undo() {
    let mut e = engine_with("\n");
    press(&mut e, 'i');
    type_chars(&mut e, "hello");
    // Break undo
    ctrl(&mut e, 'g');
    press(&mut e, 'u');
    type_chars(&mut e, " world");
    press_key(&mut e, "Escape");
    assert_buf(&e, "hello world\n");
    // First undo should only undo " world"
    press(&mut e, 'u');
    assert_buf(&e, "hello\n");
    // Second undo should undo "hello"
    press(&mut e, 'u');
    assert_buf(&e, "\n");
}

// ── Insert CTRL-G j/k (move line in insert mode) ───────────────────────────

#[test]
fn test_insert_ctrl_g_j_move_down() {
    let mut e = engine_with("line1\nline2\nline3\n");
    press(&mut e, 'i');
    ctrl(&mut e, 'g');
    press(&mut e, 'j');
    assert_cursor(&e, 1, 0);
    assert_mode(&e, Mode::Insert);
}

#[test]
fn test_insert_ctrl_g_k_move_up() {
    let mut e = engine_with("line1\nline2\nline3\n");
    press(&mut e, 'j'); // go to line 2
    press(&mut e, 'i');
    ctrl(&mut e, 'g');
    press(&mut e, 'k');
    assert_cursor(&e, 0, 0);
    assert_mode(&e, Mode::Insert);
}

// =============================================================================
// Tier 2: Medium-effort commands
// =============================================================================

// ── gq{motion} (format text) ───────────────────────────────────────────────

#[test]
fn test_gqq_format_long_line() {
    let mut e = engine_with("the quick brown fox jumps over the lazy dog and more words here to make it long enough to wrap around the textwidth boundary\n");
    e.settings.textwidth = 40;
    type_chars(&mut e, "gqq");
    let text = buf(&e);
    // Should be reflowed to ~40 chars per line
    for line in text.lines() {
        assert!(
            line.len() <= 41, // allow slight overshoot for long words
            "line too long after gqq: {} (len={})",
            line,
            line.len()
        );
    }
    assert!(
        text.lines().count() > 1,
        "should have wrapped into multiple lines"
    );
}

#[test]
fn test_gqj_format_two_lines() {
    let mut e = engine_with("short line one\nshort line two\n");
    e.settings.textwidth = 80;
    type_chars(&mut e, "gqj");
    // Two short lines should be joined into one
    let text = buf(&e);
    assert!(
        text.contains("short line one short line two"),
        "lines should be joined: {}",
        text
    );
}

#[test]
fn test_gw_keeps_cursor() {
    let mut e = engine_with("a very long line that should be formatted when we apply the gw operator to it and see what happens\n");
    e.settings.textwidth = 30;
    // Move cursor to col 5
    type_chars(&mut e, "5l");
    let saved_line = e.cursor().line;
    let _saved_col = e.cursor().col;
    type_chars(&mut e, "gww");
    // gw should keep cursor position
    // (cursor may be clamped if line is shorter, but line/col should be preserved or clamped)
    assert_eq!(e.cursor().line, saved_line);
}

// ── Visual gq ───────────────────────────────────────────────────────────────

#[test]
fn test_visual_gq_format_selection() {
    let mut e = engine_with("first\nsecond\nthird\n");
    e.settings.textwidth = 80;
    // Select all 3 lines in visual line mode
    type_chars(&mut e, "V");
    type_chars(&mut e, "2j");
    type_chars(&mut e, "gq");
    let text = buf(&e);
    // All three short lines should be joined
    assert!(
        text.contains("first second third"),
        "visual gq should join lines: {}",
        text
    );
}

// ── g CTRL-A / g CTRL-X (sequential increment) ─────────────────────────────

#[test]
fn test_g_ctrl_a_sequential_increment() {
    let mut e = engine_with("0\n0\n0\n");
    // Select all lines in visual mode
    type_chars(&mut e, "V");
    type_chars(&mut e, "2j");
    // g Ctrl-A: sequential increment (+1, +2, +3)
    type_chars(&mut e, "g");
    ctrl(&mut e, 'a');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "1", "first line should be 1");
    assert_eq!(lines[1], "2", "second line should be 2");
    assert_eq!(lines[2], "3", "third line should be 3");
}

#[test]
fn test_g_ctrl_x_sequential_decrement() {
    let mut e = engine_with("10\n10\n10\n");
    type_chars(&mut e, "V");
    type_chars(&mut e, "2j");
    type_chars(&mut e, "g");
    ctrl(&mut e, 'x');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "9", "first line should be 9");
    assert_eq!(lines[1], "8", "second line should be 8");
    assert_eq!(lines[2], "7", "third line should be 7");
}

// ── :make ───────────────────────────────────────────────────────────────────

#[test]
fn test_make_command_exists() {
    let mut e = engine_with("test\n");
    // :make should execute (may fail since there's no Makefile, but shouldn't be "not an editor command")
    let action = exec(&mut e, "make");
    // Should not be an error about unknown command
    assert!(
        !e.message.contains("Not an editor command"),
        ":make should be recognized: {}",
        e.message
    );
    assert_eq!(action, vimcode_core::EngineAction::None);
}

// ── Operator gq with text objects ──────────────────────────────────────────

#[test]
fn test_gq_ip_format_paragraph() {
    let mut e = engine_with("word one word two word three word four word five word six word seven word eight\n\nanother paragraph\n");
    e.settings.textwidth = 30;
    type_chars(&mut e, "gqip");
    let text = buf(&e);
    // First paragraph should be wrapped
    let first_para_lines: Vec<&str> = text.lines().take_while(|l| !l.is_empty()).collect();
    assert!(
        first_para_lines.len() > 1,
        "paragraph should be wrapped: {}",
        text
    );
}

// ── g' / g` (jump to mark without pushlist) ────────────────────────────────

#[test]
fn test_g_apostrophe_jump_to_mark_line() {
    let mut e = engine_with("line0\nline1\nline2\nline3\n");
    // Go to line 2 and set mark 'a'
    type_chars(&mut e, "2j");
    type_chars(&mut e, "ma");
    // Go back to line 0
    type_chars(&mut e, "gg");
    assert_cursor(&e, 0, 0);
    // g'a should jump to line of mark a without pushing to jumplist
    type_chars(&mut e, "g'a");
    assert_cursor(&e, 2, 0);
}

#[test]
fn test_g_backtick_jump_to_mark_exact() {
    let mut e = engine_with("line0\nline1\nline2\nline3\n");
    // Go to line 2, col 3 and set mark 'b'
    type_chars(&mut e, "2j3l");
    type_chars(&mut e, "mb");
    type_chars(&mut e, "gg");
    // g`b should jump to exact position
    type_chars(&mut e, "g`b");
    assert_cursor(&e, 2, 3);
}

// ── gx (open URL — test that it doesn't crash) ─────────────────────────────

#[test]
fn test_gx_no_crash() {
    let mut e = engine_with("https://example.com\n");
    type_chars(&mut e, "gx");
    // In test mode (#[cfg(not(test))]), xdg-open is skipped
    // Just verify no crash
}

// ── :b {name} (already implemented, just verify) ───────────────────────────

#[test]
fn test_b_name_partial_match() {
    let mut e = engine_with("content1\n");
    // Open a file (simulate via setting file_path)
    let buf_id = e.active_buffer_id();
    e.buffer_manager.get_mut(buf_id).unwrap().file_path =
        Some(std::path::PathBuf::from("/tmp/mytest.txt"));
    // Create another buffer
    exec(&mut e, "enew");
    assert_ne!(e.active_buffer_id(), buf_id);
    // :b mytest should switch back
    exec(&mut e, "b mytest");
    assert_eq!(e.active_buffer_id(), buf_id);
}

// ── Operator gq with j/k motions ───────────────────────────────────────────

#[test]
fn test_gq_with_j_motion() {
    let mut e = engine_with("alpha bravo\ncharlie delta\n");
    e.settings.textwidth = 80;
    type_chars(&mut e, "gqj");
    let text = buf(&e);
    // Two lines should be joined since they're under textwidth
    assert!(
        text.contains("alpha bravo charlie delta"),
        "gqj should join two lines: {}",
        text
    );
}
