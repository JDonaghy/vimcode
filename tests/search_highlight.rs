mod common;
use common::*;

// ── Bug fix: Escape clears search highlights ─────────────────────────────────

#[test]
fn escape_clears_search_highlights() {
    let mut e = engine_with("foo bar foo baz foo");
    search_fwd(&mut e, "foo");
    assert_eq!(e.search_matches.len(), 3);
    // Escape in normal mode should clear highlights
    press_key(&mut e, "Escape");
    assert!(e.search_matches.is_empty());
    assert_eq!(e.search_index, None);
    // But search query is preserved so n/N still work
    assert_eq!(e.search_query, "foo");
}

#[test]
fn escape_clears_highlights_n_restores_them() {
    let mut e = engine_with("foo bar foo");
    search_fwd(&mut e, "foo");
    assert_eq!(e.search_matches.len(), 2);
    press_key(&mut e, "Escape");
    assert!(e.search_matches.is_empty());
    // Pressing n should re-run search and find matches again
    press(&mut e, 'n');
    assert!(!e.search_matches.is_empty());
}

#[test]
fn escape_is_noop_when_no_search_highlights() {
    let mut e = engine_with("hello world");
    // No search performed yet — Escape should not panic or cause issues
    press_key(&mut e, "Escape");
    assert!(e.search_matches.is_empty());
}

// ── Bug fix: search highlights refresh after buffer edits ────────────────────

#[test]
fn search_highlights_refresh_after_insert_mode_typing() {
    let mut e = engine_with("ab ab ab");
    search_fwd(&mut e, "ab");
    assert_eq!(e.search_matches.len(), 3);
    // Enter insert mode at beginning and type a character
    press(&mut e, 'i');
    press(&mut e, 'X');
    // Highlights should have been recalculated for "Xab ab ab"
    assert_eq!(e.search_matches.len(), 3);
    // Verify positions shifted: first match should now start at char 1, not 0
    assert_eq!(e.search_matches[0].0, 1);
}

#[test]
fn search_highlights_refresh_after_normal_mode_delete() {
    let mut e = engine_with("foo bar foo baz foo");
    search_fwd(&mut e, "foo");
    assert_eq!(e.search_matches.len(), 3);
    // Delete first word with dw — removes "foo "
    press(&mut e, 'd');
    press(&mut e, 'w');
    // Buffer is now "bar foo baz foo" — only 2 matches
    assert_eq!(buf(&e), "bar foo baz foo");
    assert_eq!(e.search_matches.len(), 2);
}

#[test]
fn search_highlights_refresh_after_normal_mode_paste() {
    let mut e = engine_with("ab cd ab");
    search_fwd(&mut e, "ab");
    assert_eq!(e.search_matches.len(), 2);
    // Yank word "ab", go to end, paste — adds another "ab"
    press(&mut e, 'y');
    press(&mut e, 'w');
    press(&mut e, '$');
    press(&mut e, 'p');
    // Buffer should now contain 3 "ab" occurrences
    assert_eq!(e.search_matches.len(), 3);
}

#[test]
fn search_highlights_refresh_after_insert_mode_backspace() {
    let mut e = engine_with("ab ab ab");
    search_fwd(&mut e, "ab");
    assert_eq!(e.search_matches.len(), 3);
    // Go to col 1 (between 'a' and 'b'), enter insert mode, delete 'a' with backspace
    press(&mut e, 'l'); // on 'b'
    press(&mut e, 'i');
    press_key(&mut e, "BackSpace");
    // Buffer is now "b ab ab" — only 2 matches
    assert_eq!(buf(&e), "b ab ab");
    assert_eq!(e.search_matches.len(), 2);
}

#[test]
fn search_highlights_correct_positions_after_insert() {
    let mut e = engine_with("aa bb aa");
    search_fwd(&mut e, "aa");
    assert_eq!(e.search_matches.len(), 2);
    // First match at char 0-2, second at char 6-8
    assert_eq!(e.search_matches[0], (0, 2));
    assert_eq!(e.search_matches[1], (6, 8));
    // Insert "XX" at the beginning
    press(&mut e, 'i');
    press(&mut e, 'X');
    press(&mut e, 'X');
    // Buffer is "XXaa bb aa" — matches should be at (2,4) and (8,10)
    assert_eq!(buf(&e), "XXaa bb aa");
    assert_eq!(e.search_matches.len(), 2);
    assert_eq!(e.search_matches[0], (2, 4));
    assert_eq!(e.search_matches[1], (8, 10));
}

#[test]
fn search_highlights_disappear_when_all_matches_deleted() {
    let mut e = engine_with("xyzzy");
    search_fwd(&mut e, "xyzzy");
    assert_eq!(e.search_matches.len(), 1);
    // Delete entire line content
    press(&mut e, 'd');
    press(&mut e, 'd');
    assert!(e.search_matches.is_empty());
}

#[test]
fn no_search_refresh_when_no_active_highlights() {
    // When search_matches is empty, buffer edits should not trigger run_search
    let mut e = engine_with("hello world");
    assert!(e.search_matches.is_empty());
    // Type in insert mode — should not populate search_matches
    press(&mut e, 'i');
    press(&mut e, 'X');
    assert!(e.search_matches.is_empty());
}

#[test]
fn noh_command_still_works() {
    let mut e = engine_with("foo bar foo");
    search_fwd(&mut e, "foo");
    assert_eq!(e.search_matches.len(), 2);
    run_cmd(&mut e, "noh");
    assert!(e.search_matches.is_empty());
}

#[test]
fn search_highlights_refresh_after_replace_char() {
    let mut e = engine_with("abc abc");
    search_fwd(&mut e, "abc");
    assert_eq!(e.search_matches.len(), 2);
    // Replace 'a' with 'x' — first match becomes "xbc", no longer matches "abc"
    press(&mut e, 'r');
    press(&mut e, 'x');
    assert_eq!(buf(&e), "xbc abc");
    assert_eq!(e.search_matches.len(), 1);
    assert_eq!(e.search_matches[0], (4, 7));
}

#[test]
fn search_highlights_refresh_on_dd() {
    let mut e = engine_with("foo\nbar\nfoo\nbaz");
    search_fwd(&mut e, "foo");
    assert_eq!(e.search_matches.len(), 2);
    // Delete first line
    press(&mut e, 'd');
    press(&mut e, 'd');
    // Buffer: "bar\nfoo\nbaz" — only 1 match
    assert_eq!(e.search_matches.len(), 1);
}
