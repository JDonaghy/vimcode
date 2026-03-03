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
