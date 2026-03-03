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
