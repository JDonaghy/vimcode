mod common;
use common::*;
use vimcode_core::Mode;

// ── Forward search ────────────────────────────────────────────────────────────

#[test]
fn test_forward_search_moves_cursor() {
    let mut e = engine_with("alpha beta gamma\n");
    search_fwd(&mut e, "beta");
    // Cursor should be at start of "beta" (col 6)
    assert_cursor(&e, 0, 6);
}

#[test]
fn test_search_n_next_match() {
    let mut e = engine_with("foo bar foo baz\n");
    search_fwd(&mut e, "foo");
    // First match at col 0
    assert_cursor(&e, 0, 0);
    // n moves to second match
    press(&mut e, 'n');
    assert_cursor(&e, 0, 8);
}

#[test]
fn test_search_big_n_reverse() {
    let mut e = engine_with("foo bar foo baz\n");
    search_fwd(&mut e, "foo");
    // n to second match
    press(&mut e, 'n');
    assert_cursor(&e, 0, 8);
    // N goes back to first match
    press(&mut e, 'N');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_backward_search() {
    let mut e = engine_with("foo bar foo\n");
    // Start from end of line
    press(&mut e, '$');
    search_bwd(&mut e, "foo");
    // Should find the second "foo" (at col 8)
    assert_cursor(&e, 0, 8);
}

#[test]
fn test_search_wrap_around() {
    let mut e = engine_with("foo\nbar\nfoo\n");
    search_fwd(&mut e, "foo");
    assert_cursor(&e, 0, 0);
    press(&mut e, 'n');
    assert_cursor(&e, 2, 0);
    // n again wraps to first match
    press(&mut e, 'n');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_star_word_search() {
    let mut e = engine_with("word hello word\n");
    // Move cursor to first "word"
    assert_cursor(&e, 0, 0);
    // * searches for "word" forward
    press(&mut e, '*');
    // "word hello word" — second "word" starts at col 11
    // 'w'=0,'o'=1,'r'=2,'d'=3,' '=4,'h'=5,'e'=6,'l'=7,'l'=8,'o'=9,' '=10,'w'=11
    assert_cursor(&e, 0, 11);
}

#[test]
fn test_hash_backward_search() {
    let mut e = engine_with("word hello word\n");
    // Move to second "word" (col 12)
    press(&mut e, '$');
    press(&mut e, 'b');
    // # searches backward for "word"
    press(&mut e, '#');
    // Should land on first "word" at col 0
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_search_not_found_message() {
    let mut e = engine_with("hello world\n");
    search_fwd(&mut e, "xyzzy");
    // Should set a "not found" message
    assert_msg_contains(&e, "not found");
}

// ── :s substitute ─────────────────────────────────────────────────────────────

#[test]
fn test_substitute_basic() {
    let mut e = engine_with("hello world\n");
    exec(&mut e, "s/hello/goodbye/");
    assert_buf(&e, "goodbye world\n");
}

#[test]
fn test_substitute_global_flag() {
    let mut e = engine_with("foo foo foo\n");
    exec(&mut e, "s/foo/bar/g");
    assert_buf(&e, "bar bar bar\n");
}

#[test]
fn test_substitute_case_insensitive() {
    let mut e = engine_with("Hello World\n");
    exec(&mut e, "s/hello/goodbye/i");
    assert_buf(&e, "goodbye World\n");
}

#[test]
fn test_substitute_percent_range() {
    let mut e = engine_with("foo\nfoo\nfoo\n");
    exec(&mut e, "%s/foo/bar/g");
    assert_buf(&e, "bar\nbar\nbar\n");
}

#[test]
fn test_substitute_undo() {
    let mut e = engine_with("hello world\n");
    exec(&mut e, "s/hello/goodbye/");
    assert_buf(&e, "goodbye world\n");
    press(&mut e, 'u');
    assert_buf(&e, "hello world\n");
}

// ── Search + change ───────────────────────────────────────────────────────────

#[test]
fn test_search_then_cw_change_word() {
    let mut e = engine_with("foo bar\n");
    // Search lands cursor on "foo"
    search_fwd(&mut e, "foo");
    assert_cursor(&e, 0, 0);
    // cw changes the word
    press(&mut e, 'c');
    press(&mut e, 'w');
    assert_mode(&e, Mode::Insert);
    type_chars(&mut e, "baz");
    press_key(&mut e, "Escape");
    let content = buf(&e);
    assert!(
        content.contains("baz"),
        "expected 'baz' after cw, got: {content:?}"
    );
}

#[test]
fn test_search_n_multi_line() {
    let mut e = engine_with("alpha\nfoo\nbeta\nfoo\n");
    search_fwd(&mut e, "foo");
    assert_cursor(&e, 1, 0); // first "foo" on line 1
    press(&mut e, 'n');
    assert_cursor(&e, 3, 0); // second "foo" on line 3
}

// ── Incremental search ────────────────────────────────────────────────────────

#[test]
fn test_search_escape_restores_cursor() {
    let mut e = engine_with("hello world\n");
    let (orig_line, orig_col) = (e.cursor().line, e.cursor().col);
    // Enter search mode
    press(&mut e, '/');
    type_chars(&mut e, "world");
    // Escape before confirming — cursor should return to original position
    press_key(&mut e, "Escape");
    assert_cursor(&e, orig_line, orig_col);
    assert_mode(&e, Mode::Normal);
}
