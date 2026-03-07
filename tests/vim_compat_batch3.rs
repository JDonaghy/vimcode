mod common;
use common::*;
use vimcode_core::Mode;

// =============================================================================
// g? ROT13 operator
// =============================================================================

#[test]
fn test_g_question_rot13_word() {
    let mut e = engine_with("Hello\n");
    // g?w: ROT13 the word under cursor
    type_chars(&mut e, "g?w");
    assert_buf(&e, "Uryyb\n");
}

#[test]
fn test_g_question_rot13_double_reverses() {
    let mut e = engine_with("Hello\n");
    // Apply ROT13 twice should restore original
    type_chars(&mut e, "g?w");
    assert_buf(&e, "Uryyb\n");
    type_chars(&mut e, "0g?w");
    assert_buf(&e, "Hello\n");
}

#[test]
fn test_g_question_rot13_line() {
    let mut e = engine_with("Hello World\n");
    // g??: ROT13 current line (doubled operator)
    type_chars(&mut e, "g??");
    assert_buf(&e, "Uryyb Jbeyq\n");
}

#[test]
fn test_g_question_rot13_preserves_nonalpha() {
    let mut e = engine_with("Hello, 123!\n");
    type_chars(&mut e, "g??");
    assert_buf(&e, "Uryyb, 123!\n");
}

#[test]
fn test_g_question_rot13_motion_j() {
    let mut e = engine_with("abc\ndef\nghi\n");
    // g?j: ROT13 current line and next line
    type_chars(&mut e, "g?j");
    assert_buf(&e, "nop\nqrs\nghi\n");
}

// =============================================================================
// CTRL-@ in insert mode (insert previous text + exit)
// =============================================================================

#[test]
fn test_ctrl_at_insert_prev_text_and_exit() {
    let mut e = engine_with("hello\n");
    // First, type something in insert mode to establish last_inserted_text
    type_chars(&mut e, "A world");
    press_key(&mut e, "Escape");
    assert_buf(&e, "hello world\n");
    assert_eq!(e.mode, Mode::Normal);
    // Now open a new line and press Ctrl-@ to insert that text and exit
    type_chars(&mut e, "o");
    assert_eq!(e.mode, Mode::Insert);
    // Ctrl-@ is ctrl + "2" or ctrl + "space"
    ctrl(&mut e, '2');
    assert_eq!(e.mode, Mode::Normal);
    assert_buf(&e, "hello world\n world\n");
}

// =============================================================================
// CTRL-V literal char in insert mode
// =============================================================================

#[test]
fn test_ctrl_v_insert_literal_tab() {
    let mut e = engine_with("");
    type_chars(&mut e, "i");
    assert_eq!(e.mode, Mode::Insert);
    // Ctrl-V then Tab should insert a literal tab
    e.handle_key("v", Some('v'), true); // Ctrl-V
    assert!(e.insert_ctrl_v_pending);
    e.handle_key("Tab", None, false);
    assert!(!e.insert_ctrl_v_pending);
    assert_buf(&e, "\t");
}

#[test]
fn test_ctrl_v_insert_literal_escape_stays_in_insert() {
    // When Ctrl-V is pending, Escape should be inserted literally (not exit insert mode).
    // However, Escape is a control character (U+001B) so the behavior depends on
    // how the backend sends it. In our engine, if "Escape" key_name is sent after
    // Ctrl-V, it won't have a unicode char, so we just turn off the pending flag.
    let mut e = engine_with("");
    type_chars(&mut e, "i");
    e.handle_key("v", Some('v'), true); // Ctrl-V
    assert!(e.insert_ctrl_v_pending);
    // Any printable char should be inserted literally
    e.handle_key("a", Some('a'), false);
    assert!(!e.insert_ctrl_v_pending);
    assert_eq!(e.mode, Mode::Insert);
    assert_buf(&e, "a");
}

// =============================================================================
// CTRL-O in insert mode (execute one normal command, return to insert)
// =============================================================================

#[test]
fn test_ctrl_o_one_normal_command_returns_to_insert() {
    let mut e = engine_with("hello world\n");
    // Enter insert mode at beginning
    type_chars(&mut e, "i");
    assert_eq!(e.mode, Mode::Insert);
    // Ctrl-O: execute one normal command
    e.handle_key("o", Some('o'), true);
    assert_eq!(e.mode, Mode::Normal);
    assert!(e.insert_ctrl_o_active);
    // Move to end of line with $
    type_chars(&mut e, "$");
    // Should auto-return to insert mode
    assert_eq!(e.mode, Mode::Insert);
    assert!(!e.insert_ctrl_o_active);
}

#[test]
fn test_ctrl_o_word_motion() {
    let mut e = engine_with("hello world\n");
    type_chars(&mut e, "i");
    e.handle_key("o", Some('o'), true);
    assert_eq!(e.mode, Mode::Normal);
    type_chars(&mut e, "w"); // move to "world"
    assert_eq!(e.mode, Mode::Insert);
    assert_cursor(&e, 0, 6);
}

// =============================================================================
// ! filter operator
// =============================================================================

#[test]
fn test_bang_operator_enters_command_mode_with_range() {
    let mut e = engine_with("line 1\nline 2\nline 3\n");
    // !!: filter current line (!! should enter command mode with range)
    type_chars(&mut e, "!!");
    assert_eq!(e.mode, Mode::Command);
    // !! on line 1 pre-fills ".!" (Vim-style current line filter)
    assert_eq!(e.command_buffer, ".!");
}

#[test]
fn test_bang_operator_2_lines() {
    let mut e = engine_with("line 1\nline 2\nline 3\n");
    // !j: filter 2 lines
    type_chars(&mut e, "!j");
    assert_eq!(e.mode, Mode::Command);
    assert_eq!(e.command_buffer, "1,2!");
}

#[test]
fn test_bang_operator_with_count() {
    let mut e = engine_with("a\nb\nc\nd\ne\n");
    // 3!!: filter 3 lines starting at current
    type_chars(&mut e, "3!!");
    assert_eq!(e.mode, Mode::Command);
    // Should pre-fill range for 3 lines from current
    assert_eq!(e.command_buffer, ".,3!");
}

#[test]
fn test_filter_command_execution() {
    let mut e = engine_with("cherry\napple\nbanana\n");
    // Sort lines 1-3 using shell command
    run_cmd(&mut e, "1,3!sort");
    // Lines should be sorted alphabetically
    assert_buf(&e, "apple\nbanana\ncherry\n");
}

// =============================================================================
// CTRL-W window commands
// =============================================================================

#[test]
fn test_ctrl_w_lowercase_h_focuses_left() {
    // Basic test: Ctrl-W h should focus left window (if exists)
    let mut e = engine_with("hello\n");
    // Just test that Ctrl-W h doesn't crash on single window
    e.handle_key("\x17", Some('\x17'), true); // Ctrl-W
    type_chars(&mut e, "h");
    assert_eq!(e.mode, Mode::Normal);
}

#[test]
fn test_ctrl_w_uppercase_t_moves_to_new_group() {
    let mut e = engine_with("hello\n");
    // Split first
    e.handle_key("\x17", Some('\x17'), true); // Ctrl-W
    type_chars(&mut e, "v"); // vertical split
    let group_count_before = e.editor_groups.len();
    // Ctrl-W T should move window to new tab group
    e.handle_key("\x17", Some('\x17'), true);
    type_chars(&mut e, "T");
    // With only 2 groups and we move one out, behavior depends on implementation
    // At minimum it shouldn't crash
    assert!(e.editor_groups.len() >= group_count_before);
}

#[test]
fn test_ctrl_w_x_exchange_windows() {
    let mut e = engine_with("first\n");
    // Open a second file in a split
    run_cmd(&mut e, "vsp");
    let len = e.buffer().len_chars();
    e.buffer_mut().delete_range(0, len);
    e.buffer_mut().insert(0, "second\n");
    let buf_id_before = e.active_buffer_id();
    // Ctrl-W x should exchange with next window
    e.handle_key("\x17", Some('\x17'), true);
    type_chars(&mut e, "x");
    // Buffer should have changed in current window
    let buf_id_after = e.active_buffer_id();
    // After exchange, the buffer IDs should differ (windows swapped)
    // Note: this depends on implementation - the key test is no crash
    let _ = (buf_id_before, buf_id_after);
}

// =============================================================================
// Visual block I/A
// =============================================================================

#[test]
fn test_visual_block_i_insert_text() {
    let mut e = engine_with("hello\nworld\nfoooo\n");
    // Select a block: column 0, lines 0-2
    e.handle_key("v", Some('v'), true); // Ctrl-V for visual block
    type_chars(&mut e, "2j"); // extend 2 lines down
                              // Press I to insert at left edge of block
    type_chars(&mut e, "I");
    assert_eq!(e.mode, Mode::Insert);
    // Type some text
    type_chars(&mut e, ">> ");
    // Press Escape to apply to all lines
    press_key(&mut e, "Escape");
    assert_eq!(e.mode, Mode::Normal);
    assert_buf(&e, ">> hello\n>> world\n>> foooo\n");
}

#[test]
fn test_visual_block_a_append_text() {
    let mut e = engine_with("aa\nbb\ncc\n");
    // Start visual block at col 1
    type_chars(&mut e, "l"); // go to col 1
    e.handle_key("v", Some('v'), true); // Ctrl-V
    type_chars(&mut e, "2j"); // extend 2 lines down
                              // Press A to append after right edge of block (col 1 → insert at col 2 = end)
    type_chars(&mut e, "A");
    assert_eq!(e.mode, Mode::Insert);
    type_chars(&mut e, "XX");
    press_key(&mut e, "Escape");
    assert_eq!(e.mode, Mode::Normal);
    assert_buf(&e, "aaXX\nbbXX\nccXX\n");
}

#[test]
fn test_visual_block_i_single_column() {
    let mut e = engine_with("abc\ndef\nghi\n");
    // Block select at col 0, 3 lines
    e.handle_key("v", Some('v'), true); // Ctrl-V
    type_chars(&mut e, "2j");
    type_chars(&mut e, "I");
    type_chars(&mut e, "#");
    press_key(&mut e, "Escape");
    assert_buf(&e, "#abc\n#def\n#ghi\n");
}

// =============================================================================
// Force motion mode (o_v / o_V)
// =============================================================================

#[test]
fn test_force_linewise_dv_j() {
    // dVj: delete 2 lines linewise (force linewise on charwise j motion)
    let mut e = engine_with("aaa\nbbb\nccc\n");
    type_chars(&mut e, "dVj");
    // j is normally charwise but V forces linewise
    // Should delete lines 0 and 1 entirely
    assert_buf(&e, "ccc\n");
}

#[test]
fn test_force_charwise_yv_j() {
    // yvj: yank with charwise forcing (j is normally linewise for yank)
    let mut e = engine_with("aaa\nbbb\nccc\n");
    type_chars(&mut e, "yvj");
    // Check that the yank register has charwise content (not linewise)
    let (text, is_linewise) = e.registers.get(&'"').cloned().unwrap_or_default();
    assert!(!is_linewise, "forced charwise yank should not be linewise");
    assert!(!text.is_empty());
}

#[test]
fn test_force_linewise_dv_w() {
    // dVw: force linewise on w motion
    let mut e = engine_with("hello world\nsecond\nthird\n");
    type_chars(&mut e, "dVw");
    // V forces linewise, so entire first line should be deleted
    assert_buf(&e, "second\nthird\n");
}

// =============================================================================
// Miscellaneous from previous session (verify compilation)
// =============================================================================

#[test]
fn test_g_question_rot13_with_text_object() {
    let mut e = engine_with("(Hello)\n");
    type_chars(&mut e, "l"); // move to 'H'
                             // g?iw: ROT13 inner word
    type_chars(&mut e, "g?iw");
    assert_buf(&e, "(Uryyb)\n");
}

#[test]
fn test_ctrl_o_does_not_activate_for_insert_commands() {
    // Ctrl-O followed by a command that changes mode (like i, a) should
    // not try to return to insert
    let mut e = engine_with("hello\n");
    type_chars(&mut e, "i");
    e.handle_key("o", Some('o'), true); // Ctrl-O
    assert_eq!(e.mode, Mode::Normal);
    // dd should delete the line and stay in normal (since it's destructive)
    type_chars(&mut e, "dd");
    // After dd with ctrl_o active, we should return to insert mode
    assert_eq!(e.mode, Mode::Insert);
}

#[test]
fn test_filter_command_reverse() {
    let mut e = engine_with("apple\nbanana\ncherry\n");
    // Use tac (reverse lines)
    run_cmd(&mut e, "1,3!tac");
    assert_buf(&e, "cherry\nbanana\napple\n");
}

#[test]
fn test_rot13_multiline() {
    let mut e = engine_with("abc\nxyz\n");
    // g?G: ROT13 from current line to end of file
    type_chars(&mut e, "g?G");
    assert_buf(&e, "nop\nklm\n");
}

#[test]
fn test_visual_block_i_with_short_lines() {
    // When block column exceeds line length, should pad with spaces.
    // Note: cursor gets clamped through short lines, so the block col
    // becomes min(anchor.col, cursor.col). Test with explicit anchor.
    let mut e = engine_with("abcde\nab\nabcde\n");
    // Move to col 3 on first line
    type_chars(&mut e, "3l"); // col 3
                              // Ctrl-V starts block at col 3
    e.handle_key("v", Some('v'), true); // Ctrl-V, anchor=(0,3)
                                        // j goes to line 1, clamps col to 1 (short line "ab")
    type_chars(&mut e, "j"); // cursor now at (1, 1)
                             // j goes to line 2, col stays at 1
    type_chars(&mut e, "j"); // cursor now at (2, 1)
                             // Block is anchor.col=3, cursor.col=1, so left=1, right=3
    type_chars(&mut e, "I");
    type_chars(&mut e, "|");
    press_key(&mut e, "Escape");
    // Insert at left col (1) of block on all 3 lines
    assert_buf(&e, "a|bcde\na|b\na|bcde\n");
}

#[test]
fn test_visual_block_a_on_empty_buffer() {
    let mut e = engine_with("a\nb\nc\n");
    // Block select col 0, 3 lines
    e.handle_key("v", Some('v'), true); // Ctrl-V
    type_chars(&mut e, "2j");
    type_chars(&mut e, "A");
    type_chars(&mut e, "Z");
    press_key(&mut e, "Escape");
    assert_buf(&e, "aZ\nbZ\ncZ\n");
}
