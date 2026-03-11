mod common;
use common::*;
use vimcode_core::Mode;

#[test]
fn paste_in_insert_mode_simple() {
    let mut e = engine_with("hello\n");
    press(&mut e, 'i');
    e.paste_in_insert_mode("world ");
    assert_buf(&e, "world hello\n");
    assert_cursor(&e, 0, 6);
    assert_mode(&e, Mode::Insert);
}

#[test]
fn paste_in_insert_mode_multiline() {
    let mut e = engine_with("\n");
    press(&mut e, 'i');
    e.paste_in_insert_mode("line1\nline2\nline3");
    assert_eq!(e.buffer().to_string(), "line1\nline2\nline3\n");
    assert_cursor(&e, 2, 5);
}

#[test]
fn paste_in_insert_mode_with_crlf() {
    let mut e = engine_with("\n");
    press(&mut e, 'i');
    e.paste_in_insert_mode("a\r\nb\r\nc");
    assert_eq!(e.buffer().to_string(), "a\nb\nc\n");
    assert_cursor(&e, 2, 1);
}

#[test]
fn paste_in_insert_mode_marks_dirty() {
    let mut e = engine_with("foo\n");
    e.set_dirty(false);
    press(&mut e, 'i');
    e.paste_in_insert_mode("bar");
    assert!(e.dirty());
}

#[test]
fn paste_in_insert_mode_undoable() {
    let mut e = engine_with("hello\n");
    press(&mut e, 'i');
    e.paste_in_insert_mode("world ");
    assert_buf(&e, "world hello\n");
    // Exit insert and undo
    press_key(&mut e, "Escape");
    press(&mut e, 'u');
    assert_buf(&e, "hello\n");
}

#[test]
fn paste_in_insert_mode_empty_is_noop() {
    let mut e = engine_with("hello\n");
    press(&mut e, 'i');
    e.paste_in_insert_mode("");
    assert_buf(&e, "hello\n");
    assert_cursor(&e, 0, 0);
}

#[test]
fn paste_in_insert_mode_large_text_completes() {
    // Regression test: pasting large text should not freeze.
    let mut e = engine_with("\n");
    press(&mut e, 'i');
    let large = "word ".repeat(1000); // 5000 characters
    e.paste_in_insert_mode(&large);
    // Should complete without hanging; verify cursor advanced.
    assert_eq!(e.cursor().col, 5000);
}

#[test]
fn paste_in_insert_mode_mid_line() {
    let mut e = engine_with("abcd\n");
    press(&mut e, 'i');
    // Move to col 2
    press_key(&mut e, "Right");
    press_key(&mut e, "Right");
    e.paste_in_insert_mode("XY");
    assert_buf(&e, "abXYcd\n");
    assert_cursor(&e, 0, 4);
}
