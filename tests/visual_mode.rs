mod common;
use common::*;
use vimcode_core::Mode;

// ── Mode entry ────────────────────────────────────────────────────────────────

#[test]
fn test_visual_char_mode() {
    let mut e = engine_with("hello\n");
    press(&mut e, 'v');
    assert_mode(&e, Mode::Visual);
    // Escape returns to Normal
    press_key(&mut e, "Escape");
    assert_mode(&e, Mode::Normal);
}

#[test]
fn test_visual_line_mode() {
    let mut e = engine_with("hello\n");
    press(&mut e, 'V');
    assert_mode(&e, Mode::VisualLine);
    press_key(&mut e, "Escape");
    assert_mode(&e, Mode::Normal);
}

#[test]
fn test_visual_block_mode() {
    let mut e = engine_with("hello\nworld\n");
    ctrl(&mut e, 'v');
    assert_mode(&e, Mode::VisualBlock);
    press_key(&mut e, "Escape");
    assert_mode(&e, Mode::Normal);
}

// ── Yank operations ───────────────────────────────────────────────────────────

#[test]
fn test_visual_yank_to_eol() {
    let mut e = engine_with("hello world\n");
    // Move to col 6 to start selection at "world"
    for _ in 0..6 {
        press(&mut e, 'l');
    }
    press(&mut e, 'v');
    press(&mut e, '$');
    press(&mut e, 'y');
    // Unnamed register should contain "world"
    assert_mode(&e, Mode::Normal);
    let (text, _) = e
        .registers
        .get(&'"')
        .expect("unnamed register should be set");
    assert!(
        text.contains("world"),
        "yanked text should contain 'world', got: {text:?}"
    );
}

#[test]
fn test_visual_line_yank_paste_dup() {
    let mut e = engine_with("hello\nworld\n");
    // V selects line, y yanks, p pastes below → duplicates line
    press(&mut e, 'V');
    press(&mut e, 'y');
    press(&mut e, 'p');
    let lines = get_lines(&e);
    assert_eq!(lines.len(), 3, "expected 3 lines after Vyp, got: {lines:?}");
    assert_eq!(lines[0], "hello");
    assert_eq!(lines[1], "hello"); // duplicate
}

// ── Delete operations ─────────────────────────────────────────────────────────

#[test]
fn test_visual_line_delete() {
    let mut e = engine_with("keep\ndelete me\nkeep\n");
    // Move to second line, V, d
    press(&mut e, 'j');
    press(&mut e, 'V');
    press(&mut e, 'd');
    let lines = get_lines(&e);
    assert_eq!(lines.len(), 2, "expected 2 lines after Vd, got: {lines:?}");
    assert_eq!(lines[0], "keep");
    assert_eq!(lines[1], "keep");
}

#[test]
fn test_visual_delete_chars() {
    let mut e = engine_with("hello world\n");
    // v then 4l to select 5 chars ("hello"), then d
    press(&mut e, 'v');
    press(&mut e, '4');
    press(&mut e, 'l');
    press(&mut e, 'd');
    let content = buf(&e);
    // "hello" (5 chars) deleted, " world" remains
    assert!(
        content.starts_with(" world") || content.starts_with("world"),
        "expected ' world' after deleting 'hello', got: {content:?}"
    );
}

// ── Indent/dedent ─────────────────────────────────────────────────────────────

#[test]
fn test_visual_line_indent() {
    let mut e = engine_with("hello\n");
    press(&mut e, 'V');
    press(&mut e, '>');
    let line = get_lines(&e)[0].clone();
    assert!(
        line.starts_with("    ") || line.starts_with('\t'),
        "expected indented line after V>, got: {line:?}"
    );
}

#[test]
fn test_visual_line_dedent() {
    let mut e = engine_with("    hello\n");
    press(&mut e, 'V');
    press(&mut e, '<');
    let line = get_lines(&e)[0].clone();
    assert_eq!(
        line, "hello",
        "expected dedented line after V<, got: {line:?}"
    );
}

// ── Change operations ─────────────────────────────────────────────────────────

#[test]
fn test_visual_change_enters_insert() {
    let mut e = engine_with("hello world\n");
    // Select "hello" with v4l, then c to change
    press(&mut e, 'v');
    press(&mut e, '4');
    press(&mut e, 'l');
    press(&mut e, 'c');
    assert_mode(&e, Mode::Insert);
    // Type replacement, escape, verify
    type_chars(&mut e, "bye");
    press_key(&mut e, "Escape");
    let content = buf(&e);
    assert!(
        content.contains("bye"),
        "expected 'bye' in buffer, got: {content:?}"
    );
}

// ── Visual paste (p/P) ──────────────────────────────────────────────────────

#[test]
fn test_visual_paste_replaces_selection() {
    // Yank "hello", select "world", paste → replaces "world" with "hello"
    let mut e = engine_with("hello world\n");
    // yank "hello" (yiw)
    type_chars(&mut e, "yiw");
    // move to "world"
    type_chars(&mut e, "w");
    // select "world" (viw)
    type_chars(&mut e, "viw");
    // paste
    press(&mut e, 'p');
    assert_mode(&e, Mode::Normal);
    assert_buf(&e, "hello hello\n");
}

#[test]
fn test_visual_paste_stores_deleted_text_in_unnamed_register() {
    let mut e = engine_with("hello world\n");
    // yank "hello"
    type_chars(&mut e, "yiw");
    // select "world" and paste
    type_chars(&mut e, "w");
    type_chars(&mut e, "viw");
    press(&mut e, 'p');
    // unnamed register should now have "world" (the deleted selection)
    assert_register(&e, '"', "world", false);
}

#[test]
fn test_visual_line_paste_replaces_lines() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    // yank first line (yy)
    type_chars(&mut e, "yy");
    // move to second line, select it linewise
    press(&mut e, 'j');
    press(&mut e, 'V');
    // paste
    press(&mut e, 'p');
    assert_mode(&e, Mode::Normal);
    assert_buf(&e, "aaa\naaa\nccc\n");
}

#[test]
fn test_visual_paste_with_uppercase_p() {
    // P in visual mode should also replace (same as p in Vim)
    let mut e = engine_with("hello world\n");
    type_chars(&mut e, "yiw");
    type_chars(&mut e, "w");
    type_chars(&mut e, "viw");
    press(&mut e, 'P');
    assert_mode(&e, Mode::Normal);
    assert_buf(&e, "hello hello\n");
}

#[test]
fn test_visual_paste_multichar_selection() {
    // Yank "xx" from register, select "cd" and replace
    let mut e = engine_with("xxcdef\n");
    // yiw yanks "xxcdef" — use yy + specific register instead
    // Just yank "xx" using visual (cols 0-1)
    type_chars(&mut e, "vly");
    // After visual yank, cursor stays at col 1. Move to col 2 ('c').
    press(&mut e, 'l');
    // select "cd" (cols 2-3)
    press(&mut e, 'v');
    press(&mut e, 'l');
    press(&mut e, 'p');
    assert_mode(&e, Mode::Normal);
    assert_buf(&e, "xxxxef\n");
}

#[test]
fn test_visual_paste_with_named_register() {
    let mut e = engine_with("aaa bbb\n");
    // yank "aaa" into register a
    type_chars(&mut e, "\"ayiw");
    // select "bbb"
    type_chars(&mut e, "w");
    type_chars(&mut e, "viw");
    // paste from register a
    type_chars(&mut e, "\"a");
    press(&mut e, 'p');
    assert_mode(&e, Mode::Normal);
    assert_buf(&e, "aaa aaa\n");
}

#[test]
fn test_visual_paste_empty_register_still_deletes() {
    // If register is empty, selection should still be deleted
    let mut e = engine_with("hello\n");
    // Don't yank anything — registers are empty
    // Select "ell"
    type_chars(&mut e, "l");
    press(&mut e, 'v');
    type_chars(&mut e, "ll");
    press(&mut e, 'p');
    assert_mode(&e, Mode::Normal);
    assert_buf(&e, "ho\n");
}

#[test]
fn test_visual_paste_preserves_ip_text_object() {
    // Ensure "ip" still works as inner-paragraph text object, not paste
    let mut e = engine_with("aaa\nbbb\n\nccc\n");
    press(&mut e, 'v');
    type_chars(&mut e, "ip");
    assert_mode(&e, Mode::Visual);
    // Selection should span lines 0-1 (inner paragraph)
    let anchor = e.visual_anchor.unwrap();
    assert_eq!(anchor.line, 0);
    assert_eq!(e.cursor().line, 1);
}
