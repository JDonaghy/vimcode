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
    //
    // #1004 moved back-references out of the untranslatable set (they now
    // compile via `fancy_regex` — see the two tests below), so this uses
    // look-around, which stays genuinely unsupported. The buffer contains the
    // pattern verbatim, so a literal fall-back *would* find a match here.
    let mut e = engine_with("a foo\\@= b\n");
    search_fwd(&mut e, "foo\\@=");
    assert!(
        e.message.contains("look-around"),
        "expected a rejection message, got {:?}",
        e.message
    );
    // The cursor did not move to a bogus "literal" match.
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_search_pattern_backreference_matches_repeated_text() {
    // #1004: `\1` inside a *pattern* back-references the text `\(foo\)`
    // captured earlier in that same pattern, so `/\(foo\)\1` must land on
    // "foofoo" — not on the lone "foo" at col 0, and not on a literal "1".
    let mut e = engine_with("foo foofoo bar\n");
    search_fwd(&mut e, "\\(foo\\)\\1");
    assert_cursor(&e, 0, 4);
    // The search-count indicator counts matches through the same compiled
    // pattern: "foofoo" is the only one, so the lone "foo" at col 0 is not
    // being counted as a match of `\(foo\)\1`.
    assert_eq!(e.message, "match 1 of 1");
}

#[test]
fn test_search_pattern_backreference_reports_not_found_when_unrepeated() {
    // The same pattern must *fail* when the capture is not immediately
    // repeated — i.e. `\1` is executed, not dropped on the floor.
    let mut e = engine_with("foo bar foo\n");
    search_fwd(&mut e, "\\(foo\\)\\1");
    assert!(
        e.message.contains("Pattern not found"),
        "expected not-found, got {:?}",
        e.message
    );
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_substitute_with_pattern_backreference() {
    // `:s` shares the same compiled pattern path, so `\1` must work there too
    // — and the replacement's own `\1` still refers to group 1's text.
    let mut e = engine_with("xx aa yy\n");
    exec(&mut e, "s/\\(a\\)\\1/[\\1]/");
    assert_eq!(buf(&e).trim_end(), "xx [a] yy");
}

#[test]
fn test_substitute_confirm_flag_prompts_before_touching_the_buffer() {
    // #1031 (#801 Phase 2): the `c` flag used to be rejected outright
    // ("not implemented"). It now opens a confirm prompt — and, critically,
    // nothing is substituted behind the user's back before they answer.
    let mut e = engine_with("a a\n");
    exec(&mut e, "%s/a/b/gc");
    assert!(
        e.message
            .contains("replace with b? (y)es/(n)o/(a)ll/(q)uit/(l)ast"),
        "expected the :s_c prompt, got {:?}",
        e.message
    );
    assert_eq!(buf(&e).trim_end(), "a a");
    // The prompt parks the cursor on the pending match, not the line's
    // first non-blank.
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_substitute_confirm_y_and_n_replace_only_the_confirmed_matches() {
    let mut e = engine_with("a a a\n");
    exec(&mut e, "%s/a/b/gc");
    // y on the first match — applied immediately (see the dedicated
    // mid-loop test below for proof it doesn't wait for the loop to end).
    press(&mut e, 'y');
    // n skips the second, y takes the third.
    press(&mut e, 'n');
    press(&mut e, 'y');
    assert_eq!(buf(&e).trim_end(), "b a b");
    // Prompt is gone and a report replaced it.
    assert!(
        !e.message.contains("replace with"),
        "prompt should be dismissed, got {:?}",
        e.message
    );
}

#[test]
fn test_substitute_confirm_y_applies_immediately_not_at_loop_end() {
    // #1031 review (blocking): the confirm loop used to only fold each
    // answer into an in-memory string, splicing every confirmed match into
    // the real buffer in one shot only once the *whole* loop ended (`a`,
    // `q`/`<Esc>`, or running off the end). That's backwards from the
    // entire point of an interactive confirm prompt: real Vim/Neovim
    // applies (and paints) each confirmed match right away, so the user
    // watches it change before deciding on the next one. A test that only
    // checks the buffer after the loop finishes can't tell "applied
    // per-answer" from "applied all at once at the end" — this one asserts
    // on the buffer *mid-loop*, between individual answers.
    let mut e = engine_with("a a a\n");
    exec(&mut e, "%s/a/b/gc");
    assert_eq!(
        buf(&e).trim_end(),
        "a a a",
        "nothing is substituted before the first answer"
    );
    press(&mut e, 'y');
    assert_eq!(
        buf(&e).trim_end(),
        "b a a",
        "the confirmed match must already be visible in the buffer right \
         after 'y', while the other two candidates are still pending"
    );
    press(&mut e, 'y');
    assert_eq!(buf(&e).trim_end(), "b b a");
    press(&mut e, 'n');
    assert_eq!(buf(&e).trim_end(), "b b a", "'n' must not touch the buffer");
}

#[test]
fn test_substitute_confirm_next_prompt_cursor_tracks_earlier_length_change() {
    // #1031 review: every candidate's position is precomputed once against
    // the *original*, unmodified buffer text, but each confirmed answer is
    // now spliced into the live buffer immediately (previous test). A
    // replacement that's a different length than what it replaced shifts
    // every later match's actual position in the live buffer -- the next
    // prompt must land on the real match, not on the original (now-stale)
    // column.
    let mut e = engine_with("aa aa\n");
    exec(&mut e, "%s/aa/bbbb/gc");
    press(&mut e, 'y');
    assert_eq!(buf(&e).trim_end(), "bbbb aa");
    // The first match's replacement grew the line by 2 chars ("aa" ->
    // "bbbb"), so the second "aa" -- originally at column 3 -- now lives at
    // column 5.
    assert_cursor(&e, 0, 5);
    assert!(
        e.message.contains("replace with bbbb?"),
        "expected the prompt for the second match, got {:?}",
        e.message
    );
}

#[test]
fn test_substitute_confirm_loop_undoes_as_a_single_step() {
    // #1031 review: applying each answer live (instead of one combined
    // splice at the end) must not turn `:s///gc` into several separate
    // undo steps -- `u` still undoes the whole confirm loop's worth of
    // confirmed substitutions at once, exactly like a plain `:s///g` would.
    let mut e = engine_with("a a a\n");
    exec(&mut e, "%s/a/b/gc");
    press(&mut e, 'y');
    press(&mut e, 'y');
    press(&mut e, 'y');
    assert_eq!(buf(&e).trim_end(), "b b b");
    press(&mut e, 'u');
    assert_eq!(
        buf(&e).trim_end(),
        "a a a",
        "a single 'u' must undo every confirmed match from the loop, not \
         just the last one"
    );
}

#[test]
fn test_substitute_confirm_a_replaces_the_rest_and_q_stops() {
    // `a` answers "all remaining".
    let mut e = engine_with("a a a\n");
    exec(&mut e, "%s/a/b/gc");
    press(&mut e, 'n');
    press(&mut e, 'a');
    assert_eq!(buf(&e).trim_end(), "a b b");

    // `q` quits, keeping only what was already confirmed.
    let mut e = engine_with("a a a\n");
    exec(&mut e, "%s/a/b/gc");
    press(&mut e, 'y');
    press(&mut e, 'q');
    assert_eq!(buf(&e).trim_end(), "b a a");
}

#[test]
fn test_substitute_confirm_ctrl_c_quits_like_escape() {
    // #1031 review (non-blocking): `<C-c>`/`<C-[>` aren't in `:h :s_c`'s
    // documented answer set, but they're the same "get me out of here"
    // aliases for `<Esc>` this codebase already recognizes in Insert mode
    // (#804) -- a stuck confirm prompt should quit on them too, not
    // silently re-prompt the same candidate forever.
    let mut e = engine_with("a a a\n");
    exec(&mut e, "%s/a/b/gc");
    press(&mut e, 'y');
    ctrl(&mut e, 'c');
    assert_eq!(
        buf(&e).trim_end(),
        "b a a",
        "<C-c> must quit the loop, keeping only what was already confirmed"
    );
    assert!(
        !e.message.contains("replace with"),
        "prompt should be dismissed after <C-c>, got {:?}",
        e.message
    );
}

#[test]
fn test_substitute_confirm_prompt_swallows_normal_mode_keys() {
    // While the prompt is up, an unrelated key must not fall through to
    // Normal mode and edit the buffer — it re-prompts the same candidate.
    let mut e = engine_with("a a\n");
    exec(&mut e, "%s/a/b/gc");
    press(&mut e, 'x');
    assert_eq!(buf(&e).trim_end(), "a a");
    assert!(
        e.message.contains("replace with b?"),
        "expected the prompt to persist, got {:?}",
        e.message
    );
    // The loop is still live: y still answers the first match.
    press(&mut e, 'y');
    press(&mut e, 'y');
    assert_eq!(buf(&e).trim_end(), "b b");
}
