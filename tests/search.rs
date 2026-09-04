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
    // #801: `/` starts *after* the cursor, so the match under the cursor at
    // col 0 is skipped (verified against Neovim).
    assert_cursor(&e, 0, 8);
    // n wraps back round to the first match
    press(&mut e, 'n');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_search_big_n_reverse() {
    let mut e = engine_with("foo bar foo baz\n");
    search_fwd(&mut e, "foo");
    assert_cursor(&e, 0, 8);
    // n wraps to the first match
    press(&mut e, 'n');
    assert_cursor(&e, 0, 0);
    // N reverses, wrapping back to the second
    press(&mut e, 'N');
    assert_cursor(&e, 0, 8);
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
    assert_cursor(&e, 2, 0);
    // n wraps past the end of the buffer to the first match
    press(&mut e, 'n');
    assert_cursor(&e, 0, 0);
    press(&mut e, 'n');
    assert_cursor(&e, 2, 0);
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

// ── #801: the Vim regex engine, offsets and n/N direction ────────────────────
//
// These are black-box: they drive the engine through the same `/` command line
// a user types and assert on the resulting cursor. Every expectation below was
// checked against `nvim --headless` (the oracle the conformance suite uses),
// and every one of them fails against the pre-#801 `text.find(&query)` search —
// which matched metacharacters literally, so `/^foo`, `/\<foo\>`, `/\d\+` and
// friends simply found nothing and left the cursor where it was.

#[test]
fn test_regex_caret_anchor() {
    let mut e = engine_with("a foo\nfoo b\n");
    search_fwd(&mut e, "^foo");
    // Only the line-2 "foo" is at the start of a line.
    assert_cursor(&e, 1, 0);
}

#[test]
fn test_regex_dollar_anchor() {
    let mut e = engine_with("foo a\na foo\n");
    search_fwd(&mut e, "foo$");
    assert_cursor(&e, 1, 2);
}

#[test]
fn test_regex_word_boundaries() {
    let mut e = engine_with("foobar foo\n");
    search_fwd(&mut e, "\\<foo\\>");
    // "foobar" is not a whole-word match; the standalone "foo" at col 7 is.
    assert_cursor(&e, 0, 7);
}

#[test]
fn test_regex_quantifier_and_class() {
    let mut e = engine_with("ab 123 cd\n");
    search_fwd(&mut e, "\\d\\+");
    assert_cursor(&e, 0, 3);
}

#[test]
fn test_regex_very_magic() {
    let mut e = engine_with("fooo bar\n");
    e.view_mut().cursor.col = 2;
    search_fwd(&mut e, "\\vo+");
    // Vim enumerates matches non-overlapping, so "ooo" at col 1 is the only
    // match and the search wraps back to it.
    assert_cursor(&e, 0, 1);
}

#[test]
fn test_regex_alternation_and_literal_dot() {
    let mut e = engine_with("xx bar foo\n");
    search_fwd(&mut e, "foo\\|bar");
    assert_cursor(&e, 0, 3);

    let mut e = engine_with("abc a.c\n");
    search_fwd(&mut e, "a\\.c");
    assert_cursor(&e, 0, 4);
}

#[test]
fn test_regex_zs_and_ze_trim_the_match() {
    let mut e = engine_with("foobar\n");
    search_fwd(&mut e, "foo\\zsbar");
    assert_cursor(&e, 0, 3);

    let mut e = engine_with("xbar foobar\n");
    search_fwd(&mut e, "foo\\zebar");
    assert_cursor(&e, 0, 5);
}

#[test]
fn test_regex_inline_case_override() {
    let mut e = engine_with("x FOO foo\n");
    search_fwd(&mut e, "\\cfoo");
    assert_cursor(&e, 0, 2);
}

#[test]
fn test_search_offset_end_and_begin() {
    let mut e = engine_with("foo bar baz\n");
    search_fwd(&mut e, "bar/e");
    assert_cursor(&e, 0, 6); // last char of the match

    let mut e = engine_with("foo bar baz\n");
    search_fwd(&mut e, "bar/e+1");
    assert_cursor(&e, 0, 7);

    let mut e = engine_with("foo bar baz\n");
    search_fwd(&mut e, "bar/b+2");
    assert_cursor(&e, 0, 6);
}

#[test]
fn test_search_offset_linewise() {
    let mut e = engine_with("a\nfoo\nb\nc\n");
    search_fwd(&mut e, "foo/+1");
    assert_cursor(&e, 2, 0);
}

#[test]
fn test_search_offset_survives_n() {
    let mut e = engine_with("foo bar foo bar\n");
    search_fwd(&mut e, "bar/e");
    assert_cursor(&e, 0, 6);
    press(&mut e, 'n');
    assert_cursor(&e, 0, 14);
}

#[test]
fn test_search_chained_with_semicolon() {
    let mut e = engine_with("a foo b bar\n");
    search_fwd(&mut e, "foo/;/bar");
    assert_cursor(&e, 0, 8);
}

#[test]
fn test_empty_pattern_reuses_last_search() {
    let mut e = engine_with("foo x foo x foo\n");
    search_fwd(&mut e, "foo");
    assert_cursor(&e, 0, 6);
    // `//<CR>` repeats the previous pattern.
    search_fwd(&mut e, "");
    assert_cursor(&e, 0, 12);
}

#[test]
fn test_count_before_slash() {
    let mut e = engine_with("a foo foo foo\n");
    press(&mut e, '3');
    press(&mut e, '/');
    type_chars(&mut e, "foo");
    press_key(&mut e, "Return");
    assert_cursor(&e, 0, 10);
}

#[test]
fn test_n_and_big_n_after_backward_search() {
    let mut e = engine_with("bar\nfoo\nbar\nbar\n");
    e.view_mut().cursor.line = 3;
    search_bwd(&mut e, "bar");
    assert_cursor(&e, 2, 0);
    // After `?`, `n` keeps going backwards …
    press(&mut e, 'n');
    assert_cursor(&e, 0, 0);
    // … and `N` reverses, i.e. goes forward.
    press(&mut e, 'N');
    assert_cursor(&e, 2, 0);
}

#[test]
fn test_forward_then_backward_resets_n_direction() {
    let mut e = engine_with("bar\nfoo\nbar\nbar\n");
    search_fwd(&mut e, "bar");
    assert_cursor(&e, 2, 0);
    search_bwd(&mut e, "bar");
    assert_cursor(&e, 0, 0);
    // `n` now follows the *backward* direction of the last search.
    press(&mut e, 'n');
    assert_cursor(&e, 3, 0);
}

#[test]
fn test_star_sets_a_whole_word_pattern_reusable_by_sub() {
    let mut e = engine_with("foo bar foo\n");
    press(&mut e, '*');
    assert_cursor(&e, 0, 8);
    // `*` set the last search pattern, so `:%s//X/g` reuses it.
    exec(&mut e, "%s//X/g");
    assert_eq!(buf(&e).trim_end(), "X bar X");
}

#[test]
fn test_invalid_pattern_is_rejected_not_matched_literally() {
    // #801 acceptance: a pattern the engine cannot translate must produce an
    // error, never a silent fall-back to literal matching.
    let mut e = engine_with("a\\(foo\\)\\1 b\n");
    search_fwd(&mut e, "\\(foo\\)\\1");
    assert!(
        e.message.contains("back-reference"),
        "expected a rejection message, got {:?}",
        e.message
    );
    // The cursor did not move to a bogus "literal" match.
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_substitute_confirm_flag_is_rejected_not_silently_dropped() {
    let mut e = engine_with("a a\n");
    exec(&mut e, "%s/a/b/gc");
    assert!(
        e.message.contains("confirm"),
        "expected the c flag to be rejected, got {:?}",
        e.message
    );
    // Nothing was substituted behind the user's back.
    assert_eq!(buf(&e).trim_end(), "a a");
}
