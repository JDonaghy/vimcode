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
