mod common;
use common::*;

// ── yw: yank-word motion ──────────────────────────────────────────────────────

#[test]
fn test_yw_middle_of_line() {
    // "foo bar" cursor at 'f': yw yanks "foo " (word + trailing space to next word)
    let mut e = engine_with("foo bar\n");
    press(&mut e, 'y');
    press(&mut e, 'w');
    assert_register(&e, '"', "foo ", false);
    assert_buf(&e, "foo bar\n"); // buffer unchanged
    assert_cursor(&e, 0, 0); // cursor stays at start
}

#[test]
fn test_yw_last_word_of_line() {
    // "foo bar\nqux" cursor at 'b' (start of "bar"): yw should yank "bar" NOT "bar\n"
    let mut e = engine_with("foo bar\nqux\n");
    press(&mut e, 'w'); // move to "bar"
    assert_cursor(&e, 0, 4);
    press(&mut e, 'y');
    press(&mut e, 'w');
    assert_register(&e, '"', "bar", false);
    assert_buf(&e, "foo bar\nqux\n"); // buffer unchanged
}

#[test]
fn test_yw_only_word_on_line() {
    // "foo\nbar" cursor at 'f': yw should yank "foo" NOT "foo\n"
    let mut e = engine_with("foo\nbar\n");
    press(&mut e, 'y');
    press(&mut e, 'w');
    assert_register(&e, '"', "foo", false);
    assert_buf(&e, "foo\nbar\n");
}

#[test]
fn test_yw_then_p_pastes_inline() {
    // "foo bar" cursor at 'f': yw then $ then p should paste "foo " at end of line (inline)
    let mut e = engine_with("foo bar\n");
    press(&mut e, 'y');
    press(&mut e, 'w');
    press(&mut e, '$'); // move to 'r'
    press(&mut e, 'p');
    // Should be "foo bafoo r\n" or similar — importantly NOT on a new line
    let b = buf(&e);
    assert!(
        !b.starts_with('\n'),
        "paste should not start with newline: {:?}",
        b
    );
    assert_eq!(
        e.cursor().line,
        0,
        "cursor should stay on line 0 after char-mode paste"
    );
}

#[test]
fn test_yw_last_word_then_p_inline() {
    // "hello\nworld" yw at 'h' yanks "hello" (no newline), p pastes inline
    let mut e = engine_with("hello\nworld\n");
    press(&mut e, 'y');
    press(&mut e, 'w');
    press(&mut e, 'j'); // move to "world"
    press(&mut e, 'p');
    // paste "hello" inline into "world" after 'w' → "whelloorld" or "whello..." etc.
    // Key: cursor should be on line 1, not line 2
    assert_eq!(
        e.cursor().line,
        1,
        "inline paste should keep cursor on line 1"
    );
    let b = buf(&e);
    assert!(b.contains("hello"), "pasted text should be present");
}

#[test]
fn test_named_yw_and_ap() {
    // "hello world" "ayw at 'h' yanks "hello " into register a, then "ap pastes it
    let mut e = engine_with("hello world\n");
    press(&mut e, '"');
    press(&mut e, 'a');
    press(&mut e, 'y');
    press(&mut e, 'w');
    assert_register(&e, 'a', "hello ", false);
    // Move to end and paste from "a
    press(&mut e, '$'); // at 'd'
    press(&mut e, '"');
    press(&mut e, 'a');
    press(&mut e, 'p');
    // Should paste "hello " inline after 'd'
    assert_eq!(
        e.cursor().line,
        0,
        "\"ap should paste inline, cursor stays on line 0"
    );
    let b = buf(&e);
    assert!(b.contains("hello"), "register a content pasted");
}

#[test]
fn test_yw_with_count() {
    // "one two three" cursor at start: 2yw yanks "one two " (two words)
    let mut e = engine_with("one two three\n");
    press(&mut e, '2');
    press(&mut e, 'y');
    press(&mut e, 'w');
    assert_register(&e, '"', "one two ", false);
    assert_buf(&e, "one two three\n");
}

// ── last-word-of-file (no trailing newline) ───────────────────────────────────

#[test]
fn test_yw_last_word_of_file_no_newline() {
    // Regression: "foo bar" (no trailing newline), cursor at 'b':
    // yw should yank "bar" (all 3 chars), not "ba" (missing last char)
    let mut e = engine_with("foo bar");
    press(&mut e, 'w'); // move to 'b'
    press(&mut e, 'y');
    press(&mut e, 'w');
    assert_register(&e, '"', "bar", false);
    assert_buf(&e, "foo bar");
}

#[test]
fn test_yw_only_word_no_newline() {
    // "hello" (no trailing newline), cursor at 'h':
    // yw should yank "hello" (5 chars)
    let mut e = engine_with("hello");
    press(&mut e, 'y');
    press(&mut e, 'w');
    assert_register(&e, '"', "hello", false);
    assert_buf(&e, "hello");
}

#[test]
fn test_dw_last_word_of_file_no_newline() {
    // "foo bar" (no trailing newline), cursor at 'b':
    // dw should delete "bar" (all 3 chars), leaving "foo "
    let mut e = engine_with("foo bar");
    press(&mut e, 'w'); // move to 'b'
    press(&mut e, 'd');
    press(&mut e, 'w');
    assert_buf(&e, "foo ");
    assert_register(&e, '"', "bar", false);
}

#[test]
fn test_yw_last_word_of_last_line_no_newline() {
    // "foo\nbar" (no trailing newline on last line), cursor at 'b' (line 1):
    // yw should yank "bar" (all 3 chars)
    let mut e = engine_with("foo\nbar");
    press(&mut e, 'j'); // move to line 1
    press(&mut e, 'y');
    press(&mut e, 'w');
    assert_register(&e, '"', "bar", false);
    assert_buf(&e, "foo\nbar");
}

// ── dw: delete-word motion ────────────────────────────────────────────────────

#[test]
fn test_dw_middle_of_line() {
    // "foo bar baz" cursor at 'f': dw deletes "foo " leaving "bar baz"
    let mut e = engine_with("foo bar baz\n");
    press(&mut e, 'd');
    press(&mut e, 'w');
    assert_buf(&e, "bar baz\n");
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_dw_last_word_of_line() {
    // "foo bar\nqux" cursor at 'b' (start of "bar"): dw should delete "bar" but NOT the \n
    // Lines should remain separate: line 0 = "foo ", line 1 = "qux"
    let mut e = engine_with("foo bar\nqux\n");
    press(&mut e, 'w'); // move to "bar" at col 4
    press(&mut e, 'd');
    press(&mut e, 'w');
    assert_buf(&e, "foo \nqux\n");
    assert_eq!(e.cursor().line, 0, "cursor should stay on line 0");
}

#[test]
fn test_dw_only_word_on_line() {
    // "foo\nbar" cursor at 'f': dw deletes "foo" but NOT the newline
    let mut e = engine_with("foo\nbar\n");
    press(&mut e, 'd');
    press(&mut e, 'w');
    // Line 0 should become empty (just newline), line 1 should still be "bar"
    let b = buf(&e);
    assert!(b.contains("bar"), "bar on line 1 should be preserved");
    // Should NOT have merged lines
    assert_eq!(
        b.lines().count(),
        2,
        "should still have 2 lines, got: {:?}",
        b
    );
}

#[test]
fn test_dw_saves_to_register() {
    // dw should save the deleted text (without newline if at eol)
    let mut e = engine_with("foo\nbar\n");
    press(&mut e, 'd');
    press(&mut e, 'w');
    assert_register(&e, '"', "foo", false);
}

#[test]
fn test_dw_then_p() {
    // dw then p should paste the deleted word back (char-mode, inline)
    let mut e = engine_with("foo bar\n");
    press(&mut e, 'd'); // dw: delete "foo "
    press(&mut e, 'w');
    // Now buffer is "bar\n", cursor at 'b'
    press(&mut e, 'p'); // paste "foo " after 'b'
    let b = buf(&e);
    assert!(b.contains("foo"), "dw+p should restore deleted word");
    assert_eq!(e.cursor().line, 0, "p should paste inline on line 0");
}

// ── ye: yank-end-of-word motion ───────────────────────────────────────────────

#[test]
fn test_ye_from_start_of_word() {
    // "hello world" cursor at 'h': ye yanks "hello" (to end of word, inclusive)
    let mut e = engine_with("hello world\n");
    press(&mut e, 'y');
    press(&mut e, 'e');
    assert_register(&e, '"', "hello", false);
    assert_buf(&e, "hello world\n");
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_ye_from_middle_of_word() {
    // "hello world" cursor at 'e' (col 1): ye yanks "ello" (to end of "hello")
    let mut e = engine_with("hello world\n");
    press(&mut e, 'l');
    press(&mut e, 'y');
    press(&mut e, 'e');
    assert_register(&e, '"', "ello", false);
}

#[test]
fn test_ye_last_word_of_line() {
    // "foo bar\nqux" cursor at 'b': ye yanks "bar" (no newline)
    let mut e = engine_with("foo bar\nqux\n");
    press(&mut e, 'w'); // move to "bar"
    press(&mut e, 'y');
    press(&mut e, 'e');
    assert_register(&e, '"', "bar", false);
}

// ── de: delete-end-of-word motion ────────────────────────────────────────────

#[test]
fn test_de_from_start_of_word() {
    // "hello world" cursor at 'h': de deletes "hello", leaving " world"
    let mut e = engine_with("hello world\n");
    press(&mut e, 'd');
    press(&mut e, 'e');
    assert_buf(&e, " world\n");
    assert_register(&e, '"', "hello", false);
}

// ── yb: yank-back-to-start-of-word ──────────────────────────────────────────

#[test]
fn test_yb_middle_of_line() {
    // "foo bar" cursor at 'b' (col 4): yb yanks "foo " (from start of "foo" to cursor)
    let mut e = engine_with("foo bar\n");
    press(&mut e, 'w'); // move to "bar" at col 4
    press(&mut e, 'y');
    press(&mut e, 'b');
    assert_register(&e, '"', "foo ", false);
    assert_buf(&e, "foo bar\n");
}

#[test]
fn test_db_middle_of_line() {
    // "foo bar" cursor at 'b': db deletes "foo " leaving "bar"
    let mut e = engine_with("foo bar\n");
    press(&mut e, 'w'); // move to "bar"
    press(&mut e, 'd');
    press(&mut e, 'b');
    assert_buf(&e, "bar\n");
}

// ── y$ / d$: yank/delete to end of line ──────────────────────────────────────

#[test]
fn test_y_dollar_from_start() {
    // "hello world" cursor at 'h': y$ yanks "hello world" (to end of line, no newline)
    let mut e = engine_with("hello world\n");
    press(&mut e, 'y');
    press(&mut e, '$');
    assert_register(&e, '"', "hello world", false);
    assert_buf(&e, "hello world\n"); // unchanged
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_y_dollar_from_middle() {
    // "hello world" cursor at col 6 ('w'): y$ yanks "world"
    let mut e = engine_with("hello world\n");
    press(&mut e, 'w'); // move to "world"
    press(&mut e, 'y');
    press(&mut e, '$');
    assert_register(&e, '"', "world", false);
}

#[test]
fn test_d_dollar_from_middle() {
    // "hello world" cursor at 'w' (col 6): d$ deletes "world", leaves "hello "
    let mut e = engine_with("hello world\n");
    press(&mut e, 'w'); // move to "world"
    press(&mut e, 'd');
    press(&mut e, '$');
    assert_buf(&e, "hello \n");
    assert_register(&e, '"', "world", false);
}

#[test]
fn test_c_dollar_from_middle() {
    // "hello world" cursor at 'w': c$ deletes "world" and enters insert mode
    let mut e = engine_with("hello world\n");
    press(&mut e, 'w');
    press(&mut e, 'c');
    press(&mut e, '$');
    assert_buf(&e, "hello \n");
    assert_mode(&e, vimcode_core::Mode::Insert);
}

// ── y0 / d0: yank/delete to start of line ────────────────────────────────────

#[test]
fn test_y_zero_from_end() {
    // "hello world" cursor at 'd' (last char, col 10): y0 yanks "hello worl"
    let mut e = engine_with("hello world\n");
    press(&mut e, '$'); // move to last char
    press(&mut e, 'y');
    press(&mut e, '0');
    assert_register(&e, '"', "hello worl", false);
    assert_buf(&e, "hello world\n");
}

#[test]
fn test_d_zero_from_middle() {
    // "hello world" cursor at 'w' (col 6): d0 deletes "hello " leaving "world"
    let mut e = engine_with("hello world\n");
    press(&mut e, 'w'); // col 6
    press(&mut e, 'd');
    press(&mut e, '0');
    assert_buf(&e, "world\n");
    assert_cursor(&e, 0, 0);
}

// ── paste cursor positioning ──────────────────────────────────────────────────

#[test]
fn test_p_after_yw_stays_on_same_line() {
    // Regression: pasting char-mode text should not push cursor to next line
    let mut e = engine_with("ab\n");
    press(&mut e, 'y');
    press(&mut e, 'w'); // yank "ab" (no newline since end of line)
    press(&mut e, 'p'); // paste after 'a'
                        // cursor should be on line 0, not line 1
    assert_eq!(e.cursor().line, 0, "p should not move to next line");
}

#[test]
fn test_p_char_mode_cursor_at_end_of_pasted_text() {
    // "abc" cursor at 'a', yank "ab" with yw then p: result should be "aabc" or "aabb..."
    // cursor at last char of pasted text, which is on line 0
    let mut e = engine_with("abc\n");
    press(&mut e, 'y');
    press(&mut e, 'w'); // yanks "abc" (no newline - only word on line)
                        // actually "abc" is the only word, yw from 'a' → "abc"
    press(&mut e, 'p'); // paste after 'a'
    assert_eq!(e.cursor().line, 0, "cursor should stay on line 0");
}

#[test]
#[allow(non_snake_case)]
fn test_P_char_mode_stays_on_same_line() {
    // P (paste before) should also not move to next line for char-mode
    let mut e = engine_with("hello\n");
    press(&mut e, 'y');
    press(&mut e, 'w'); // yank "hello"
    press(&mut e, '$'); // move to last char
    press(&mut e, 'P'); // paste before 'o'
    assert_eq!(e.cursor().line, 0, "P should not move to next line");
}

// ── yy / dd: linewise yank/delete ────────────────────────────────────────────

#[test]
fn test_yy_does_not_include_leading_content() {
    // "foo\nbar" yy on line 0 → register has "foo\n" (linewise)
    let mut e = engine_with("foo\nbar\n");
    press(&mut e, 'y');
    press(&mut e, 'y');
    assert_register(&e, '"', "foo\n", true);
    assert_buf(&e, "foo\nbar\n");
}

#[test]
fn test_dd_then_p_restores_line() {
    // dd on "foo\nbar" removes "foo\n", p pastes it back below current line
    let mut e = engine_with("foo\nbar\n");
    press(&mut e, 'd');
    press(&mut e, 'd');
    assert_buf(&e, "bar\n");
    press(&mut e, 'p');
    assert_buf(&e, "bar\nfoo\n");
}

// ── count variations ─────────────────────────────────────────────────────────

#[test]
fn test_2dw() {
    // "one two three" 2dw deletes "one two " leaving "three"
    let mut e = engine_with("one two three\n");
    press(&mut e, '2');
    press(&mut e, 'd');
    press(&mut e, 'w');
    assert_buf(&e, "three\n");
}

#[test]
fn test_2yw() {
    // "one two three" 2yw yanks "one two " (two words with spaces)
    let mut e = engine_with("one two three\n");
    press(&mut e, '2');
    press(&mut e, 'y');
    press(&mut e, 'w');
    assert_register(&e, '"', "one two ", false);
    assert_buf(&e, "one two three\n");
}

// ── dfx/dtx/dFx/dTx: operator + find char ──────────────────────────────────

#[test]
fn test_dfx_delete_forward_through_char() {
    let mut e = engine_with("abcdef\n");
    press(&mut e, 'd');
    press(&mut e, 'f');
    press(&mut e, 'd');
    assert_buf(&e, "ef\n");
    assert_register(&e, '"', "abcd", false);
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_dtx_delete_forward_till_char() {
    let mut e = engine_with("abcdef\n");
    press(&mut e, 'd');
    press(&mut e, 't');
    press(&mut e, 'd');
    assert_buf(&e, "def\n");
    assert_register(&e, '"', "abc", false);
}

#[test]
#[allow(non_snake_case)]
fn test_dFx_delete_backward_through_char() {
    let mut e = engine_with("abcdef\n");
    // Move cursor to 'e' (col 4)
    press(&mut e, '$'); // 'f' col 5
    press(&mut e, 'd');
    press(&mut e, 'F');
    press(&mut e, 'c');
    assert_buf(&e, "abf\n");
    assert_register(&e, '"', "cde", false);
}

#[test]
#[allow(non_snake_case)]
fn test_dTx_delete_backward_till_char() {
    let mut e = engine_with("abcdef\n");
    press(&mut e, '$'); // 'f' col 5
    press(&mut e, 'd');
    press(&mut e, 'T');
    press(&mut e, 'c');
    assert_buf(&e, "abcf\n");
    assert_register(&e, '"', "de", false);
}

#[test]
fn test_cfx_change_through_char() {
    let mut e = engine_with("abcdef\n");
    press(&mut e, 'c');
    press(&mut e, 'f');
    press(&mut e, 'd');
    assert_buf(&e, "ef\n");
    assert_mode(&e, vimcode_core::Mode::Insert);
}

#[test]
fn test_yfx_yank_through_char() {
    let mut e = engine_with("abcdef\n");
    press(&mut e, 'y');
    press(&mut e, 'f');
    press(&mut e, 'd');
    assert_register(&e, '"', "abcd", false);
    assert_buf(&e, "abcdef\n"); // unchanged
}

#[test]
fn test_dfx_no_match_noop() {
    let mut e = engine_with("abcdef\n");
    press(&mut e, 'd');
    press(&mut e, 'f');
    press(&mut e, 'z');
    assert_buf(&e, "abcdef\n"); // no match → no change
}

#[test]
fn test_d_semicolon_repeat_find() {
    let mut e = engine_with("axbxcxd\n");
    // First do f to set last_find
    press(&mut e, 'f');
    press(&mut e, 'x');
    assert_cursor(&e, 0, 1);
    // Now d; should delete from cursor to next 'x' (inclusive)
    press(&mut e, 'd');
    press(&mut e, ';');
    assert_buf(&e, "acxd\n");
    assert_register(&e, '"', "xbx", false);
}

#[test]
fn test_d_comma_reverse_find() {
    let mut e = engine_with("axbxcxd\n");
    // Move to last 'x' (col 5)
    press(&mut e, 'f');
    press(&mut e, 'x');
    press(&mut e, ';');
    press(&mut e, ';');
    assert_cursor(&e, 0, 5);
    // d, should delete backward to previous 'x' (reverse of f)
    press(&mut e, 'd');
    press(&mut e, ',');
    assert_buf(&e, "axbxd\n");
}

// ── dj/dk: linewise up/down ─────────────────────────────────────────────────

#[test]
fn test_dj_delete_two_lines() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    press(&mut e, 'd');
    press(&mut e, 'j');
    assert_buf(&e, "ccc\n");
    assert_register(&e, '"', "aaa\nbbb\n", true);
}

#[test]
fn test_dk_delete_two_lines_upward() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    press(&mut e, 'j'); // move to line 1
    press(&mut e, 'd');
    press(&mut e, 'k');
    assert_buf(&e, "ccc\n");
}

#[test]
fn test_yj_yank_two_lines_linewise() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    press(&mut e, 'y');
    press(&mut e, 'j');
    assert_register(&e, '"', "aaa\nbbb\n", true);
    assert_buf(&e, "aaa\nbbb\nccc\n"); // unchanged
}

#[test]
fn test_2dj_delete_three_lines() {
    let mut e = engine_with("aaa\nbbb\nccc\nddd\n");
    press(&mut e, '2');
    press(&mut e, 'd');
    press(&mut e, 'j');
    assert_buf(&e, "ddd\n");
}

#[test]
fn test_cj_change_two_lines() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    press(&mut e, 'c');
    press(&mut e, 'j');
    assert_mode(&e, vimcode_core::Mode::Insert);
    // aaa and bbb should be replaced with an empty line
    let b = buf(&e);
    assert!(b.contains("ccc"), "ccc should survive");
}

#[test]
fn test_indent_j() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    press(&mut e, '>');
    press(&mut e, 'j');
    let lines = get_lines(&e);
    assert!(lines[0].starts_with("    "), "line 0 should be indented");
    assert!(lines[1].starts_with("    "), "line 1 should be indented");
    assert!(!lines[2].starts_with(' '), "line 2 should not be indented");
}

// ── dG/dgg: linewise to end/beginning ───────────────────────────────────────

#[test]
#[allow(non_snake_case)]
fn test_dG_delete_to_end() {
    let mut e = engine_with("aaa\nbbb\nccc\nddd\n");
    press(&mut e, 'j'); // line 1
    press(&mut e, 'd');
    press(&mut e, 'G');
    // Deleting to end of file absorbs trailing newline of previous line
    let b = buf(&e);
    assert!(b.starts_with("aaa"), "line 0 should survive");
    assert!(!b.contains("bbb"), "bbb should be deleted");
}

#[test]
fn test_dgg_delete_to_beginning() {
    let mut e = engine_with("aaa\nbbb\nccc\nddd\n");
    press(&mut e, 'j'); // line 1
    press(&mut e, 'j'); // line 2
    press(&mut e, 'd');
    press(&mut e, 'g');
    press(&mut e, 'g');
    assert_buf(&e, "ddd\n");
}

#[test]
#[allow(non_snake_case)]
fn test_yG_yank_to_end_linewise() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    press(&mut e, 'y');
    press(&mut e, 'G');
    assert_register(&e, '"', "aaa\nbbb\nccc\n", true);
    assert_buf(&e, "aaa\nbbb\nccc\n"); // unchanged
}

#[test]
#[allow(non_snake_case)]
fn test_d5G_delete_to_line_5() {
    // 6 lines, cursor on line 0, d5G deletes lines 0-4 (1-indexed: lines 1-5)
    let mut e = engine_with("1\n2\n3\n4\n5\n6\n");
    press(&mut e, 'd');
    press(&mut e, '5');
    press(&mut e, 'G');
    assert_buf(&e, "6\n");
}

#[test]
#[allow(non_snake_case)]
fn test_cG_change_to_end() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    press(&mut e, 'j'); // line 1
    press(&mut e, 'c');
    press(&mut e, 'G');
    assert_mode(&e, vimcode_core::Mode::Insert);
    let b = buf(&e);
    assert!(b.starts_with("aaa\n"), "line 0 should survive");
}

// ── d{/d}: paragraph motions ────────────────────────────────────────────────

#[test]
fn test_d_right_brace_paragraph_forward() {
    let mut e = engine_with("aaa\nbbb\n\nccc\nddd\n");
    press(&mut e, 'd');
    press(&mut e, '}');
    // Should delete from line 0 to the blank line (inclusive)
    let b = buf(&e);
    assert!(
        b.starts_with("ccc"),
        "should start with ccc after deleting paragraph"
    );
}

#[test]
fn test_d_left_brace_paragraph_backward() {
    let mut e = engine_with("aaa\nbbb\n\nccc\nddd\n");
    press(&mut e, 'j'); // line 1
    press(&mut e, 'j'); // line 2 (blank)
    press(&mut e, 'j'); // line 3 (ccc)
    press(&mut e, 'd');
    press(&mut e, '{');
    // Should delete from blank line to ccc line (backward)
    let b = buf(&e);
    assert!(b.contains("aaa"), "aaa should survive");
    assert!(b.contains("ddd"), "ddd should survive");
}

#[test]
fn test_y_right_brace_yank_paragraph() {
    let mut e = engine_with("aaa\nbbb\n\nccc\n");
    press(&mut e, 'y');
    press(&mut e, '}');
    // Should yank lines 0 through the blank line
    let (content, is_linewise) = e.registers.get(&'"').unwrap();
    assert!(is_linewise, "y}} should be linewise");
    assert!(content.contains("aaa"), "should contain aaa");
    assert!(content.contains("bbb"), "should contain bbb");
}

// ── d(/d): sentence motions ─────────────────────────────────────────────────

#[test]
fn test_d_right_paren_sentence_forward() {
    let mut e = engine_with("Hello world. Goodbye world.\n");
    press(&mut e, 'd');
    press(&mut e, ')');
    // Should delete to next sentence start
    let b = buf(&e);
    assert!(
        b.contains("Goodbye") || b.starts_with("Goodbye"),
        "second sentence should remain: {:?}",
        b
    );
}

// ── dW/dB/dE: WORD motions ──────────────────────────────────────────────────

#[test]
#[allow(non_snake_case)]
fn test_dW_delete_bigword_forward() {
    let mut e = engine_with("foo-bar baz\n");
    press(&mut e, 'd');
    press(&mut e, 'W');
    assert_buf(&e, "baz\n");
    assert_register(&e, '"', "foo-bar ", false);
}

#[test]
#[allow(non_snake_case)]
fn test_dB_delete_bigword_backward() {
    let mut e = engine_with("foo bar-baz qux\n");
    // Use W to move by WORD to 'q' at col 12
    press(&mut e, 'W'); // col 4 (start of "bar-baz")
    press(&mut e, 'W'); // col 12 (start of "qux")
    press(&mut e, 'd');
    press(&mut e, 'B');
    assert_buf(&e, "foo qux\n");
    assert_register(&e, '"', "bar-baz ", false);
}

#[test]
#[allow(non_snake_case)]
fn test_dE_delete_to_end_of_bigword() {
    let mut e = engine_with("foo-bar baz\n");
    press(&mut e, 'd');
    press(&mut e, 'E');
    assert_buf(&e, " baz\n");
    assert_register(&e, '"', "foo-bar", false);
}

#[test]
#[allow(non_snake_case)]
fn test_cW_change_bigword() {
    // cW should behave like cE (Vim compat)
    let mut e = engine_with("foo-bar baz\n");
    press(&mut e, 'c');
    press(&mut e, 'W');
    assert_buf(&e, " baz\n");
    assert_mode(&e, vimcode_core::Mode::Insert);
}

#[test]
#[allow(non_snake_case)]
fn test_yW_yank_bigword() {
    let mut e = engine_with("foo-bar baz\n");
    press(&mut e, 'y');
    press(&mut e, 'W');
    assert_register(&e, '"', "foo-bar ", false);
    assert_buf(&e, "foo-bar baz\n"); // unchanged
}

// ── d^/dh/dl: charwise short motions ────────────────────────────────────────

#[test]
fn test_d_caret_delete_to_first_non_blank() {
    let mut e = engine_with("    hello world\n");
    press(&mut e, '$'); // move to end
    press(&mut e, 'd');
    press(&mut e, '^');
    assert_buf(&e, "    d\n");
}

#[test]
fn test_dh_delete_one_char_left() {
    let mut e = engine_with("abcdef\n");
    press(&mut e, 'l');
    press(&mut e, 'l'); // col 2
    press(&mut e, 'd');
    press(&mut e, 'h');
    assert_buf(&e, "acdef\n");
    assert_cursor(&e, 0, 1);
}

#[test]
fn test_dl_delete_one_char_right() {
    let mut e = engine_with("abcdef\n");
    press(&mut e, 'd');
    press(&mut e, 'l');
    assert_buf(&e, "bcdef\n");
    assert_register(&e, '"', "a", false);
}

#[test]
fn test_y_caret() {
    let mut e = engine_with("    hello\n");
    press(&mut e, '$'); // move to 'o' col 8
    press(&mut e, 'y');
    press(&mut e, '^');
    assert_register(&e, '"', "hell", false);
    assert_buf(&e, "    hello\n"); // unchanged
}

#[test]
fn test_3dl_delete_three_chars() {
    let mut e = engine_with("abcdef\n");
    press(&mut e, '3');
    press(&mut e, 'd');
    press(&mut e, 'l');
    assert_buf(&e, "def\n");
    assert_register(&e, '"', "abc", false);
}

// ── dH/dM/dL: screen motions (linewise) ─────────────────────────────────────

#[test]
#[allow(non_snake_case)]
fn test_dH_delete_to_top_of_screen() {
    let mut e = engine_with("1\n2\n3\n4\n5\n");
    press(&mut e, 'j');
    press(&mut e, 'j'); // line 2
    press(&mut e, 'd');
    press(&mut e, 'H');
    // scroll_top is 0, so dH deletes from line 0 to line 2
    assert_buf(&e, "4\n5\n");
}

#[test]
#[allow(non_snake_case)]
fn test_dL_delete_to_bottom_of_screen() {
    let mut e = engine_with("1\n2\n3\n4\n5\n");
    press(&mut e, 'd');
    press(&mut e, 'L');
    // viewport_lines defaults to some value; deletes current through bottom
    // At minimum, line 0 should be deleted
    let b = buf(&e);
    assert!(!b.starts_with("1\n"), "line 1 should be deleted");
}

// ── Case operators with new motions ─────────────────────────────────────────

#[test]
fn test_g_tilde_j_toggle_case_two_lines() {
    let mut e = engine_with("Hello\nWorld\nfoo\n");
    press(&mut e, 'g');
    press(&mut e, '~');
    press(&mut e, 'j');
    assert_buf(&e, "hELLO\nwORLD\nfoo\n");
}

#[test]
fn test_guj_lowercase_two_lines() {
    let mut e = engine_with("HELLO\nWORLD\nfoo\n");
    press(&mut e, 'g');
    press(&mut e, 'u');
    press(&mut e, 'j');
    assert_buf(&e, "hello\nworld\nfoo\n");
}

#[test]
#[allow(non_snake_case)]
fn test_gUG_uppercase_to_end() {
    let mut e = engine_with("hello\nworld\nfoo\n");
    press(&mut e, 'g');
    press(&mut e, 'U');
    press(&mut e, 'G');
    assert_buf(&e, "HELLO\nWORLD\nFOO\n");
}

#[test]
fn test_indent_right_brace() {
    let mut e = engine_with("aaa\nbbb\n\nccc\n");
    press(&mut e, '>');
    press(&mut e, '}');
    let lines = get_lines(&e);
    assert!(lines[0].starts_with("    "), "line 0 should be indented");
    assert!(lines[1].starts_with("    "), "line 1 should be indented");
}

#[test]
fn test_gufx_lowercase_to_char() {
    let mut e = engine_with("ABCDEF\n");
    press(&mut e, 'g');
    press(&mut e, 'u');
    press(&mut e, 'f');
    press(&mut e, 'D');
    assert_buf(&e, "abcdEF\n");
}

#[test]
#[allow(non_snake_case)]
fn test_eq_G_auto_indent_file() {
    let mut e = engine_with("  aaa\n    bbb\nccc\n");
    press(&mut e, '=');
    press(&mut e, 'G');
    // auto_indent_lines resets indent based on context — at minimum it shouldn't crash
    let b = buf(&e);
    assert!(!b.is_empty(), "buffer should not be empty after =G");
}

#[test]
fn test_g_tilde_tilde_toggle_current_line() {
    // Regression test: g~~ should still work
    let mut e = engine_with("Hello World\nfoo\n");
    press(&mut e, 'g');
    press(&mut e, '~');
    press(&mut e, '~');
    assert_buf(&e, "hELLO wORLD\nfoo\n");
}

// ── dge: delete backward to end of previous word ────────────────────────────

#[test]
fn test_dge_backward_to_end_of_previous_word() {
    let mut e = engine_with("foo bar baz\n");
    press(&mut e, 'w');
    press(&mut e, 'w'); // cursor at 'b' of "baz" (col 8)
    press(&mut e, 'd');
    press(&mut e, 'g');
    press(&mut e, 'e');
    // ge from 'b' of baz goes to end of "foo" (col 2, 'o')
    // dge deletes [2, 9) = "o bar b" → "fo" + "az\n" = "foaz\n"
    assert_buf(&e, "foaz\n");
}

#[test]
fn test_yge_yank_backward_to_end_of_previous_word() {
    let mut e = engine_with("foo bar baz\n");
    press(&mut e, 'w');
    press(&mut e, 'w'); // cursor at 'b' of "baz" (col 8)
    press(&mut e, 'y');
    press(&mut e, 'g');
    press(&mut e, 'e');
    let (content, is_linewise) = e.registers.get(&'"').unwrap();
    assert!(!is_linewise, "yge should be charwise");
    assert!(content.contains("r"), "should include 'r' (end of bar)");
    assert_buf(&e, "foo bar baz\n"); // unchanged
}

// ── g~w/guw/gUw: case with word motions ────────────────────────────────────

#[test]
fn test_g_tilde_w_toggle_case_word() {
    let mut e = engine_with("Hello world\n");
    press(&mut e, 'g');
    press(&mut e, '~');
    press(&mut e, 'w');
    assert_buf(&e, "hELLO world\n");
}

#[test]
fn test_guw_lowercase_word() {
    let mut e = engine_with("HELLO world\n");
    press(&mut e, 'g');
    press(&mut e, 'u');
    press(&mut e, 'w');
    assert_buf(&e, "hello world\n");
}

#[test]
#[allow(non_snake_case)]
fn test_gUw_uppercase_word() {
    let mut e = engine_with("hello world\n");
    press(&mut e, 'g');
    press(&mut e, 'U');
    press(&mut e, 'w');
    assert_buf(&e, "HELLO world\n");
}

// ── Indent/dedent with motions ──────────────────────────────────────────────

#[test]
fn test_dedent_j() {
    let mut e = engine_with("    aaa\n    bbb\nccc\n");
    press(&mut e, '<');
    press(&mut e, 'j');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "aaa", "line 0 should be dedented");
    assert_eq!(lines[1], "bbb", "line 1 should be dedented");
}

#[test]
#[allow(non_snake_case)]
fn test_indent_G_to_end() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    press(&mut e, '>');
    press(&mut e, 'G');
    let lines = get_lines(&e);
    for (i, line) in lines.iter().enumerate() {
        assert!(
            line.starts_with("    ") || line.is_empty(),
            "line {} should be indented: {:?}",
            i,
            line
        );
    }
}

// ── $ operator with case/indent ─────────────────────────────────────────────

#[test]
fn test_g_tilde_dollar_toggle_case_to_eol() {
    let mut e = engine_with("Hello World\n");
    press(&mut e, 'g');
    press(&mut e, '~');
    press(&mut e, '$');
    assert_buf(&e, "hELLO wORLD\n");
}

#[test]
fn test_gu_dollar_lowercase_to_eol() {
    let mut e = engine_with("HELLO WORLD\n");
    press(&mut e, 'g');
    press(&mut e, 'u');
    press(&mut e, '$');
    assert_buf(&e, "hello world\n");
}

// ── Edge cases ──────────────────────────────────────────────────────────────

#[test]
fn test_dj_at_last_line_noop_or_delete_last() {
    let mut e = engine_with("aaa\nbbb\n");
    press(&mut e, 'j'); // line 1 (last line)
    press(&mut e, 'd');
    press(&mut e, 'j');
    // dj at last line: no line below, deletes just current line
    let b = buf(&e);
    assert!(b.contains("aaa"), "aaa should survive");
}

#[test]
fn test_dk_at_first_line() {
    let mut e = engine_with("aaa\nbbb\n");
    press(&mut e, 'd');
    press(&mut e, 'k');
    // dk at first line: no line above, deletes just current line
    assert_buf(&e, "bbb\n");
}

#[test]
fn test_dfx_escape_cancels() {
    let mut e = engine_with("abcdef\n");
    press(&mut e, 'd');
    press(&mut e, 'f');
    press_key(&mut e, "Escape");
    // After escape, pending state should be cleared
    assert_buf(&e, "abcdef\n"); // no change
}

#[test]
fn test_dgg_from_last_line_deletes_entire_file() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    press(&mut e, 'G'); // go to last line
    press(&mut e, 'd');
    press(&mut e, 'g');
    press(&mut e, 'g');
    // Should delete all lines
    let b = buf(&e);
    assert!(
        b.trim().is_empty() || b == "\n",
        "file should be empty after dgg from last line, got: {:?}",
        b
    );
}
