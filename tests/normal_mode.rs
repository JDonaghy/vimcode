mod common;
use common::*;
use vimcode_core::Mode;

// ── Navigation ────────────────────────────────────────────────────────────────

#[test]
fn test_hjkl() {
    let mut e = engine_with("hello\nworld\n");
    // 'l' moves right
    press(&mut e, 'l');
    assert_cursor(&e, 0, 1);
    press(&mut e, 'l');
    assert_cursor(&e, 0, 2);
    // 'h' moves left
    press(&mut e, 'h');
    assert_cursor(&e, 0, 1);
    // 'j' moves down
    press(&mut e, 'j');
    assert_cursor(&e, 1, 1);
    // 'k' moves up
    press(&mut e, 'k');
    assert_cursor(&e, 0, 1);
}

#[test]
fn test_hjkl_bounds() {
    let mut e = engine_with("hi\nby\n");
    // Can't go above first line
    press(&mut e, 'k');
    assert_cursor(&e, 0, 0);
    // Can't go left of col 0
    press(&mut e, 'h');
    assert_cursor(&e, 0, 0);
    // Go to last line
    press(&mut e, 'j');
    press(&mut e, 'j');
    assert_cursor(&e, 1, 0); // stays on last line
}

#[test]
fn test_gg_and_big_g() {
    let mut e = engine_with("line1\nline2\nline3\n");
    // G jumps to last line
    press(&mut e, 'G');
    assert_cursor(&e, 2, 0);
    // gg jumps back to first line
    press(&mut e, 'g');
    press(&mut e, 'g');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_word_motion_w_b() {
    let mut e = engine_with("foo bar baz\n");
    // 'w' jumps to start of next word
    press(&mut e, 'w');
    assert_cursor(&e, 0, 4); // "bar"
    press(&mut e, 'w');
    assert_cursor(&e, 0, 8); // "baz"
                             // 'b' jumps back to start of previous word
    press(&mut e, 'b');
    assert_cursor(&e, 0, 4);
    press(&mut e, 'b');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_line_bounds_0_dollar() {
    let mut e = engine_with("hello world\n");
    // '$' moves to end of line (last char)
    press(&mut e, '$');
    assert_cursor(&e, 0, 10); // 'hello world' has 11 chars, last is at col 10
                              // '0' moves to start of line
    press(&mut e, '0');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_paragraph_motion() {
    let mut e = engine_with("line1\nline2\n\nline4\n");
    // '}' moves to next blank line / paragraph end
    press(&mut e, '}');
    // Should be on or past the blank line
    assert!(e.cursor().line >= 2, "expected past blank line");
    // '{' moves back
    press(&mut e, '{');
    assert_eq!(e.cursor().line, 0);
}

#[test]
fn test_paragraph_forward_reaches_last_line() {
    // Pressing } repeatedly must eventually reach the last line of the file.
    // Regression: previously stopped one jump short when the file ended without
    // a trailing blank line.
    let mut e = engine_with("aaa\n\nbbb\nccc\n");
    // line 0: "aaa", line 1: "", line 2: "bbb", line 3: "ccc"
    press(&mut e, '}'); // -> blank line 1
    assert_eq!(e.cursor().line, 1);
    press(&mut e, '}'); // -> last line (3)
    assert_eq!(
        e.cursor().line,
        3,
        "}} should reach the last line of the file"
    );
}

#[test]
fn test_paragraph_forward_no_blanks() {
    // File with no blank lines: } should jump straight to the last line.
    let mut e = engine_with("aaa\nbbb\nccc\n");
    press(&mut e, '}');
    assert_eq!(
        e.cursor().line,
        2,
        "}} with no blank lines should go to last line"
    );
}

// ── Operators ─────────────────────────────────────────────────────────────────

#[test]
fn test_dd_delete_line() {
    let mut e = engine_with("first\nsecond\nthird\n");
    // dd deletes current line
    press(&mut e, 'd');
    press(&mut e, 'd');
    let lines = get_lines(&e);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "second");
    assert_eq!(lines[1], "third");
}

#[test]
fn test_2dd_delete_two_lines() {
    let mut e = engine_with("a\nb\nc\nd\n");
    press(&mut e, '2');
    press(&mut e, 'd');
    press(&mut e, 'd');
    let lines = get_lines(&e);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "c");
}

#[test]
fn test_dw_delete_word() {
    let mut e = engine_with("hello world\n");
    press(&mut e, 'd');
    press(&mut e, 'w');
    // "hello " is deleted, leaving "world"
    let content = buf(&e);
    assert!(
        content.starts_with("world"),
        "expected 'world' after dw, got: {content:?}"
    );
}

#[test]
fn test_big_d_delete_to_eol() {
    let mut e = engine_with("hello world\n");
    // Move to col 6 ("world"), then D
    for _ in 0..6 {
        press(&mut e, 'l');
    }
    press(&mut e, 'D');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "hello ");
}

#[test]
fn test_cc_enters_insert() {
    let mut e = engine_with("hello\nworld\n");
    press(&mut e, 'c');
    press(&mut e, 'c');
    // cc clears the line and enters insert mode
    assert_mode(&e, Mode::Insert);
    // First line should be empty (or contain just newline)
    let lines = get_lines(&e);
    assert_eq!(lines[0], "");
}

#[test]
fn test_yy_paste_below() {
    let mut e = engine_with("hello\nworld\n");
    // yy yanks current line
    press(&mut e, 'y');
    press(&mut e, 'y');
    // p pastes below
    press(&mut e, 'p');
    let lines = get_lines(&e);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "hello");
    assert_eq!(lines[1], "hello"); // pasted copy
    assert_eq!(lines[2], "world");
}

#[test]
fn test_big_p_paste_above() {
    let mut e = engine_with("hello\nworld\n");
    // Move to line 1, yank it, paste above
    press(&mut e, 'j');
    press(&mut e, 'y');
    press(&mut e, 'y');
    press(&mut e, 'P');
    let lines = get_lines(&e);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[1], "world"); // pasted above line 1
}

// ── Text objects ──────────────────────────────────────────────────────────────

#[test]
fn test_diw_inner_word() {
    let mut e = engine_with("hello\n");
    // With cursor at start of "hello", diw deletes the word
    press(&mut e, 'd');
    press(&mut e, 'i');
    press(&mut e, 'w');
    // Buffer should be empty or just whitespace/newline
    let content = buf(&e);
    let trimmed = content.trim();
    assert!(
        trimmed.is_empty(),
        "expected word deleted, got: {content:?}"
    );
}

#[test]
fn test_di_double_quote() {
    let mut e = engine_with("say \"hello\" now\n");
    // Move cursor inside the quotes (col 5 = 'h')
    for _ in 0..5 {
        press(&mut e, 'l');
    }
    press(&mut e, 'd');
    press(&mut e, 'i');
    press(&mut e, '"');
    // Should delete contents between quotes
    let content = buf(&e);
    assert!(
        content.contains("\"\""),
        "expected empty quotes, got: {content:?}"
    );
}

#[test]
fn test_di_paren() {
    let mut e = engine_with("func(arg)\n");
    // Move inside parens (col 5 = 'a')
    for _ in 0..5 {
        press(&mut e, 'l');
    }
    press(&mut e, 'd');
    press(&mut e, 'i');
    press(&mut e, '(');
    // Should leave "func()"
    let content = buf(&e);
    assert!(
        content.contains("func()"),
        "expected func(), got: {content:?}"
    );
}

// ── Registers ─────────────────────────────────────────────────────────────────

#[test]
fn test_named_register_yank_paste() {
    let mut e = engine_with("alpha\nbeta\n");
    // "ayy — yank line into register 'a'
    press(&mut e, '"');
    press(&mut e, 'a');
    press(&mut e, 'y');
    press(&mut e, 'y');
    assert_register(&e, 'a', "alpha\n", true);
    // Move to line 1, paste from 'a' above
    press(&mut e, 'j');
    press(&mut e, '"');
    press(&mut e, 'a');
    press(&mut e, 'P');
    let lines = get_lines(&e);
    assert_eq!(lines[1], "alpha");
}

#[test]
fn test_black_hole_register() {
    let mut e = engine_with("keep\ndelete\n");
    // Yank "keep" into unnamed register first
    press(&mut e, 'y');
    press(&mut e, 'y');
    // Move to next line, delete into black hole
    press(&mut e, 'j');
    press(&mut e, '"');
    press(&mut e, '_');
    press(&mut e, 'd');
    press(&mut e, 'd');
    // Paste — should still paste "keep", not "delete"
    press(&mut e, 'p');
    let lines = get_lines(&e);
    assert!(
        lines.contains(&"keep".to_string()),
        "unnamed register should still have 'keep', lines: {lines:?}"
    );
}

#[test]
fn test_named_registers_independent() {
    // Verify two named registers hold different content independently
    let mut e = engine_with("line1\nline2\n");
    // "ayy — yank first line into 'a'
    press(&mut e, '"');
    press(&mut e, 'a');
    press(&mut e, 'y');
    press(&mut e, 'y');
    // move to second line, "byy — yank into 'b'
    press(&mut e, 'j');
    press(&mut e, '"');
    press(&mut e, 'b');
    press(&mut e, 'y');
    press(&mut e, 'y');
    // verify both registers have correct content
    assert_register(&e, 'a', "line1\n", true);
    assert_register(&e, 'b', "line2\n", true);
}

// ── Undo/redo ─────────────────────────────────────────────────────────────────

#[test]
fn test_undo_redo() {
    let mut e = engine_with("hello\nworld\n");
    // Delete first line
    press(&mut e, 'd');
    press(&mut e, 'd');
    assert_eq!(get_lines(&e).len(), 1);
    // Undo restores it
    press(&mut e, 'u');
    assert_eq!(get_lines(&e).len(), 2);
    assert_eq!(get_lines(&e)[0], "hello");
    // Redo re-deletes
    ctrl(&mut e, 'r');
    assert_eq!(get_lines(&e).len(), 1);
}

#[test]
fn test_multi_step_undo() {
    let mut e = engine_with("a\nb\nc\n");
    press(&mut e, 'd');
    press(&mut e, 'd');
    press(&mut e, 'd');
    press(&mut e, 'd');
    assert_eq!(get_lines(&e).len(), 1);
    press(&mut e, 'u');
    assert_eq!(get_lines(&e).len(), 2);
    press(&mut e, 'u');
    assert_eq!(get_lines(&e).len(), 3);
}

// ── Marks ─────────────────────────────────────────────────────────────────────

#[test]
fn test_marks() {
    let mut e = engine_with("line1\nline2\nline3\n");
    // Move to line 2
    press(&mut e, 'j');
    press(&mut e, 'j');
    assert_cursor(&e, 2, 0);
    // Set mark 'a'
    press(&mut e, 'm');
    press(&mut e, 'a');
    // Jump back to top
    press(&mut e, 'g');
    press(&mut e, 'g');
    assert_cursor(&e, 0, 0);
    // Jump to mark 'a' (line jump with ')
    press(&mut e, '\'');
    press(&mut e, 'a');
    assert_eq!(e.cursor().line, 2);
}

// ── Macros ────────────────────────────────────────────────────────────────────

#[test]
fn test_macro_record_and_play() {
    let mut e = engine_with("a\nb\nc\n");
    // qa — start recording to register a
    press(&mut e, 'q');
    press(&mut e, 'a');
    // dd — delete current line (inside macro)
    press(&mut e, 'd');
    press(&mut e, 'd');
    // q — stop recording; buffer now has "b\nc\n"
    press(&mut e, 'q');
    assert_eq!(
        get_lines(&e).len(),
        2,
        "after recording dd macro, should have 2 lines"
    );
    // @a — play the macro: deletes current line again; buffer now has "c\n"
    press(&mut e, '@');
    press(&mut e, 'a');
    drain_macro_queue(&mut e);
    assert_eq!(
        get_lines(&e).len(),
        1,
        "after replaying dd macro, should have 1 line"
    );
}

// ── Indentation ───────────────────────────────────────────────────────────────

#[test]
fn test_indent_dedent() {
    let mut e = engine_with("hello\n");
    // >> indents the line
    press(&mut e, '>');
    press(&mut e, '>');
    let line = get_lines(&e)[0].clone();
    assert!(
        line.starts_with("    ") || line.starts_with('\t'),
        "expected indented line, got: {line:?}"
    );
    // << dedents
    press(&mut e, '<');
    press(&mut e, '<');
    let line2 = get_lines(&e)[0].clone();
    assert_eq!(line2, "hello");
}

// ── Repeat ────────────────────────────────────────────────────────────────────

#[test]
fn test_dot_repeat() {
    let mut e = engine_with("a\nb\nc\n");
    // Delete first line
    press(&mut e, 'd');
    press(&mut e, 'd');
    assert_eq!(get_lines(&e).len(), 2);
    // '.' repeats — deletes again
    press(&mut e, '.');
    assert_eq!(get_lines(&e).len(), 1);
}

#[test]
fn test_cw_dot_repeat_no_trailing_space() {
    // cw should change the word without the trailing space.
    // Repeating with . must behave identically — change word, not word+space.
    let mut e = engine_with("foo bar baz\n");
    // cw on "foo" → delete "foo", enter insert → type "AAA" → Escape
    press(&mut e, 'c');
    press(&mut e, 'w');
    assert_eq!(e.mode, Mode::Insert);
    type_chars(&mut e, "AAA");
    press_key(&mut e, "Escape");
    assert_eq!(e.mode, Mode::Normal);
    assert_buf(&e, "AAA bar baz\n");

    // Move to "bar" (w moves to start of next word)
    press(&mut e, 'w');
    // Repeat with . — should change "bar" to "AAA", NOT "bar " to "AAA"
    press(&mut e, '.');
    assert_buf(&e, "AAA AAA baz\n");
}

#[test]
fn test_ce_dot_repeat() {
    // ce (change to end of word) + dot repeat should also work correctly
    let mut e = engine_with("foo bar baz\n");
    press(&mut e, 'c');
    press(&mut e, 'e');
    assert_eq!(e.mode, Mode::Insert);
    type_chars(&mut e, "XXX");
    press_key(&mut e, "Escape");
    assert_buf(&e, "XXX bar baz\n");

    press(&mut e, 'w');
    press(&mut e, '.');
    assert_buf(&e, "XXX XXX baz\n");
}

// #803: `.` records the *command* (replayed through the same key dispatcher
// that produced it), not the inserted text. Each test below is a family the
// old text-replay design got wrong — the assertion is the exact buffer, so a
// regression to "insert the same text at the cursor" fails loudly.

#[test]
fn test_dot_repeat_a_appends_at_new_eol_not_old_text() {
    // A appends at end-of-line. `.` on a new line must re-append there, not
    // insert ";" wherever the cursor happens to sit.
    let mut e = engine_with("a\nb\n");
    press(&mut e, 'A');
    press(&mut e, ';');
    press_key(&mut e, "Escape");
    assert_buf(&e, "a;\nb\n");

    press(&mut e, 'j');
    press(&mut e, '.');
    assert_buf(&e, "a;\nb;\n");
}

#[test]
fn test_dot_repeat_i_inserts_at_new_line_start() {
    let mut e = engine_with("a\nb\n");
    press(&mut e, 'I');
    press(&mut e, 'x');
    press_key(&mut e, "Escape");
    assert_buf(&e, "xa\nb\n");

    press(&mut e, 'j');
    press(&mut e, '.');
    assert_buf(&e, "xa\nxb\n");
}

#[test]
fn test_dot_repeat_o_opens_a_new_line_each_time() {
    // `o` then `.` must open a SECOND new line, not re-insert the text at
    // the cursor (the old bug: `ob<Esc>.` produced "bb").
    let mut e = engine_with("a\n");
    press(&mut e, 'o');
    press(&mut e, 'b');
    press_key(&mut e, "Escape");
    assert_buf(&e, "a\nb\n");

    press(&mut e, '.');
    assert_buf(&e, "a\nb\nb\n");
}

#[test]
fn test_dot_repeat_big_o_opens_a_new_line_above_each_time() {
    let mut e = engine_with("a\n");
    press(&mut e, 'O');
    press(&mut e, 'b');
    press_key(&mut e, "Escape");
    assert_buf(&e, "b\na\n");

    press(&mut e, '.');
    assert_buf(&e, "b\nb\na\n");
}

#[test]
fn test_dot_repeat_cc_deletes_then_inserts() {
    // The old design re-inserted "X" without deleting the line first
    // (`cc j .` gave "Xb" instead of two lines of just "X").
    let mut e = engine_with("a\nb\n");
    press(&mut e, 'c');
    press(&mut e, 'c');
    press(&mut e, 'X');
    press_key(&mut e, "Escape");
    assert_buf(&e, "X\nb\n");

    press(&mut e, 'j');
    press(&mut e, '.');
    assert_buf(&e, "X\nX\n");
}

#[test]
fn test_dot_repeat_big_c_changes_to_new_eol() {
    let mut e = engine_with("abc\ndef\n");
    e.view_mut().cursor.col = 1;
    press(&mut e, 'C');
    press(&mut e, 'X');
    press_key(&mut e, "Escape");
    assert_buf(&e, "aX\ndef\n");

    press(&mut e, 'j');
    press(&mut e, '.');
    assert_buf(&e, "aX\ndX\n");
}

#[test]
fn test_dot_repeat_s_substitutes_char_then_inserts() {
    let mut e = engine_with("abcd\n");
    press(&mut e, 's');
    press(&mut e, 'X');
    press_key(&mut e, "Escape");
    assert_buf(&e, "Xbcd\n");

    press(&mut e, 'l');
    press(&mut e, '.');
    assert_buf(&e, "XXcd\n");
}

#[test]
fn test_dot_repeat_p_repeats_the_paste_not_the_yank() {
    // `.` after `p` must repeat only the paste — not "yl" too, which would
    // yank different text each time. Was entirely non-repeatable before #803.
    let mut e = engine_with("ab\n");
    press(&mut e, 'y');
    press(&mut e, 'l');
    press(&mut e, 'p');
    assert_buf(&e, "aab\n");

    press(&mut e, '.');
    assert_buf(&e, "aaab\n");
}

#[test]
fn test_dot_repeat_indent_gtgt() {
    // `>>` was entirely non-repeatable before #803.
    let mut e = engine_with("a\n");
    e.settings.shift_width = 4;
    e.settings.expand_tab = true;
    press(&mut e, '>');
    press(&mut e, '>');
    assert_buf(&e, "    a\n");

    press(&mut e, '.');
    assert_buf(&e, "        a\n");
}

#[test]
fn test_dot_repeat_ctrl_a_increment() {
    // `<C-a>` (increment number under cursor) was entirely non-repeatable
    // before #803.
    let mut e = engine_with("1\n2\n");
    ctrl(&mut e, 'a');
    assert_buf(&e, "2\n2\n");

    press(&mut e, 'j');
    press(&mut e, '.');
    assert_buf(&e, "2\n3\n");
}

#[test]
fn test_dot_repeat_visual_line_delete_same_size_new_cursor() {
    // `:h visual-repeat`: `.` after a visual operator reselects a
    // same-sized region at the *new* cursor, not a fixed one-line delete.
    let mut e = engine_with("a\nb\nc\nd\ne\n");
    press(&mut e, 'V');
    press(&mut e, 'j');
    press(&mut e, 'd');
    assert_buf(&e, "c\nd\ne\n");

    press(&mut e, '.');
    assert_buf(&e, "e\n");
}

#[test]
fn test_dot_repeat_dap_text_object() {
    let mut e = engine_with("a\n\nb\n\nc\n");
    press(&mut e, 'd');
    press(&mut e, 'a');
    press(&mut e, 'p');
    assert_buf(&e, "b\n\nc\n");

    press(&mut e, '.');
    assert_buf(&e, "c\n");
}

#[test]
fn test_dot_repeat_visual_block_insert() {
    let mut e = engine_with("ab\nab\nab\nab\n");
    ctrl(&mut e, 'v');
    press(&mut e, 'j');
    press(&mut e, 'I');
    press(&mut e, 'x');
    press_key(&mut e, "Escape");
    assert_buf(&e, "xab\nxab\nab\nab\n");

    press(&mut e, 'j');
    press(&mut e, 'j');
    press(&mut e, '.');
    assert_buf(&e, "xab\nxab\nxab\nxab\n");
}

#[test]
fn test_dot_repeat_visual_block_change() {
    // Named exact-buffer coverage for `vb:c then .` (previously covered only
    // implicitly via its removal from KNOWN_DEVIATIONS in
    // nvim_conformance.rs — see #803 review nits). Blockwise `c` is a
    // blockwise delete followed by a blockwise insert at the same column
    // (reusing `visual_block_insert_info`, the same mechanism block `I`/`A`
    // use), so it should repeat the same way block `I` does.
    let mut e = engine_with("abc\nabc\nabc\nabc\n");
    ctrl(&mut e, 'v');
    press(&mut e, 'j');
    press(&mut e, 'c');
    press(&mut e, 'Z');
    press_key(&mut e, "Escape");
    assert_buf(&e, "Zbc\nZbc\nabc\nabc\n");

    press(&mut e, 'j');
    press(&mut e, 'j');
    press(&mut e, '.');
    assert_buf(&e, "Zbc\nZbc\nZbc\nZbc\n");
}

#[test]
fn test_dot_repeat_count_override_replaces_not_multiplies() {
    // `:h .`: a count given to `.` *replaces* the original count.
    let mut e = engine_with("abcdefghij\n");
    press(&mut e, '3');
    press(&mut e, 'x');
    assert_buf(&e, "defghij\n");

    press(&mut e, '2');
    press(&mut e, '.');
    // 3 + 2 = 5 characters deleted in total, not 3 + 3 or 3*2.
    assert_buf(&e, "fghij\n");
}

#[test]
fn test_dot_repeat_count_override_dd() {
    let mut e = engine_with("a\nb\nc\nd\ne\nf\n");
    press(&mut e, '2');
    press(&mut e, 'd');
    press(&mut e, 'd');
    assert_buf(&e, "c\nd\ne\nf\n");

    press(&mut e, '3');
    press(&mut e, '.');
    assert_buf(&e, "f\n");
}

#[test]
fn test_dot_repeat_count_override_then_bare_dot_uses_override_not_stale_prefix() {
    // Regression for #803 review: a count-override repeat (`2.`) must not
    // corrupt the remembered dot-count for a *later* bare `.`. Before the
    // fix, the "2" typed just before `.` leaked into the nested replay's own
    // dot-recording and got concatenated onto the replayed "2", leaving the
    // remembered count as 22 instead of 2 — so this third `.` would delete
    // far more than the 2 characters Vim's `:h .` promises ("Count ... [is]
    // remembered ... Use "4." ... Use "." to [repeat] again").
    let mut e = engine_with("abcdefghij\n");
    press(&mut e, '3');
    press(&mut e, 'x');
    assert_buf(&e, "defghij\n");

    press(&mut e, '2');
    press(&mut e, '.');
    assert_buf(&e, "fghij\n");

    press(&mut e, '.');
    // Exactly 2 more characters deleted (matching the "2." override that's
    // now remembered), not 22 (which would wipe the rest of the line).
    assert_buf(&e, "hij\n");
}

#[test]
fn test_dot_repeat_count_override_dd_then_bare_dot_uses_override_not_stale_prefix() {
    // Same regression as above, for the linewise `dd` family: `2dd` then
    // `3.` must leave `3` (not `33`) as the count a further bare `.` uses.
    let mut e = engine_with("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n");
    press(&mut e, '2');
    press(&mut e, 'd');
    press(&mut e, 'd');
    assert_buf(&e, "c\nd\ne\nf\ng\nh\ni\nj\nk\n");

    press(&mut e, '3');
    press(&mut e, '.');
    assert_buf(&e, "f\ng\nh\ni\nj\nk\n");

    press(&mut e, '.');
    // Exactly 3 more lines deleted, not 33 (which would wipe every
    // remaining line in the buffer).
    assert_buf(&e, "i\nj\nk\n");
}

// ── #803 CI follow-up: regressions the first cut of the keystroke-replay
// dot-repeat introduced, each caught by the nvim conformance oracle. These
// pin the behaviour with plain engine-level assertions so a re-break fails
// even on a machine without `nvim` installed.

#[test]
fn test_insert_count_is_abandoned_when_an_arrow_key_moves_the_cursor() {
    // `:h i_<Left>`: moving the cursor during a count-prefixed insert drops
    // the repeat. Vim yields "yxa" for `3ix<Left>y<Esc>`, not "yxyxyxa".
    let mut e = engine_with("a\n");
    press(&mut e, '3');
    press(&mut e, 'i');
    press(&mut e, 'x');
    press_key(&mut e, "Left");
    press(&mut e, 'y');
    press_key(&mut e, "Escape");
    assert_buf(&e, "yxa\n");
}

#[test]
fn test_insert_count_still_repeats_without_a_cursor_key() {
    // Guard the other side of the fix: an undisturbed `3ix<Esc>` still
    // repeats, so the cancellation above isn't just disabling the feature.
    let mut e = engine_with("a\n");
    press(&mut e, '3');
    press(&mut e, 'i');
    press(&mut e, 'x');
    press_key(&mut e, "Escape");
    assert_buf(&e, "xxxa\n");
}

#[test]
fn test_dot_inside_a_recorded_macro_does_not_pollute_the_register() {
    // `qax.jq` records exactly "x.j". Before the fix, the keys `.` replayed
    // were themselves appended to the recording buffer ("x.xj"), so `@a`
    // deleted one character too many on every line it ran over.
    let mut e = engine_with("abc\ndef\n");
    press(&mut e, 'q');
    press(&mut e, 'a');
    press(&mut e, 'x');
    press(&mut e, '.');
    press(&mut e, 'j');
    press(&mut e, 'q');
    assert_buf(&e, "c\ndef\n");
    assert_register(&e, 'a', "x.j", false);

    press(&mut e, '@');
    press(&mut e, 'a');
    drain_macro_queue(&mut e);
    assert_buf(&e, "c\nf\n");
}

#[test]
fn test_dot_after_at_colon_does_not_repeat_the_ex_command() {
    // `@:` re-runs the last ex command; `.` repeats the last *change*, and an
    // ex command is not one. `:d<CR>@:.` deletes exactly two lines in Vim.
    let mut e = engine_with("a\nb\nc\nd\n");
    exec(&mut e, "d");
    assert_buf(&e, "b\nc\nd\n");
    press(&mut e, '@');
    press(&mut e, ':');
    assert_buf(&e, "c\nd\n");
    press(&mut e, '.');
    assert_buf(&e, "c\nd\n");
}

#[test]
fn test_dot_after_at_register_repeats_the_macros_own_change() {
    // The `@a` keystrokes are dropped from the dot candidate, but the macro's
    // *contents* still record normally as they are pumped — so `.` after `@a`
    // repeats the macro's last change (`x`), exactly as Vim does.
    let mut e = engine_with("abcd\nefgh\n");
    press(&mut e, 'q');
    press(&mut e, 'a');
    press(&mut e, 'x');
    press(&mut e, 'q');
    assert_buf(&e, "bcd\nefgh\n");

    press(&mut e, 'j');
    press(&mut e, '@');
    press(&mut e, 'a');
    drain_macro_queue(&mut e);
    assert_buf(&e, "bcd\nfgh\n");

    press(&mut e, '.');
    assert_buf(&e, "bcd\ngh\n");
}

#[test]
fn test_dip_on_final_paragraph_removes_the_preceding_line_separator() {
    // `ip`/`ap` are linewise: deleting the buffer's last paragraph must take
    // a line separator with it. Leaving the one *before* it behind produced a
    // stray blank line and parked the cursor on a line Vim had removed.
    let mut e = engine_with("a\n\nb");
    press(&mut e, 'j');
    press(&mut e, 'j');
    press(&mut e, 'd');
    press(&mut e, 'i');
    press(&mut e, 'p');
    assert_buf(&e, "a\n");
    assert_cursor(&e, 1, 0);
}

#[test]
fn test_dip_on_a_middle_paragraph_is_unaffected() {
    // The EOF-only rule must not fire when a following separator exists.
    let mut e = engine_with("a\n\nb\n");
    press(&mut e, 'd');
    press(&mut e, 'i');
    press(&mut e, 'p');
    assert_buf(&e, "\nb\n");
    assert_cursor(&e, 0, 0);
}
