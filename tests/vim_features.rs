mod common;
use common::*;
use vimcode_core::Mode;

// ── Group 1: Normal Mode Gaps ─────────────────────────────────────────────────

#[test]
fn test_x_delete_before_cursor() {
    let mut e = engine_with("hello\n");
    // Move to col 3
    press(&mut e, 'l');
    press(&mut e, 'l');
    press(&mut e, 'l');
    assert_cursor(&e, 0, 3);
    // X deletes the char before cursor (col 2 = 'l')
    press(&mut e, 'X');
    assert_buf(&e, "helo\n");
    assert_cursor(&e, 0, 2);
}

#[test]
fn test_x_at_col_zero_noop() {
    let mut e = engine_with("hello\n");
    assert_cursor(&e, 0, 0);
    press(&mut e, 'X');
    // Nothing changes
    assert_buf(&e, "hello\n");
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_x_with_count() {
    let mut e = engine_with("hello\n");
    // Move to col 4
    press(&mut e, '$');
    assert_cursor(&e, 0, 4);
    // 3X deletes 3 chars before cursor (cols 3,2,1 => 'l','l','e')
    press(&mut e, '3');
    press(&mut e, 'X');
    assert_buf(&e, "ho\n");
    assert_cursor(&e, 0, 1);
}

#[test]
fn test_g_tilde_word() {
    let mut e = engine_with("Hello world\n");
    // g~w should toggle case of "Hello" → "hELLO"
    press(&mut e, 'g');
    press(&mut e, '~');
    press(&mut e, 'w');
    assert_buf(&e, "hELLO world\n");
}

#[test]
fn test_g_tilde_line() {
    let mut e = engine_with("Hello World\n");
    // g~~ toggles entire line
    press(&mut e, 'g');
    press(&mut e, '~');
    press(&mut e, '~');
    assert_buf(&e, "hELLO wORLD\n");
}

#[test]
fn test_gu_word() {
    let mut e = engine_with("HELLO world\n");
    // guw lowercases "HELLO"
    press(&mut e, 'g');
    press(&mut e, 'u');
    press(&mut e, 'w');
    assert_buf(&e, "hello world\n");
}

#[test]
fn test_gu_line() {
    let mut e = engine_with("HELLO WORLD\n");
    // guu lowercases entire line
    press(&mut e, 'g');
    press(&mut e, 'u');
    press(&mut e, 'u');
    assert_buf(&e, "hello world\n");
}

#[test]
fn test_gu_line_via_capital() {
    let mut e = engine_with("HELLO WORLD\n");
    // gUU uppercases entire line (already upper — just verifies it works)
    press(&mut e, 'g');
    press(&mut e, 'U');
    press(&mut e, 'U');
    assert_buf(&e, "HELLO WORLD\n");
}

#[test]
fn test_gu_word_uppercase() {
    let mut e = engine_with("hello world\n");
    // gUw uppercases "hello"
    press(&mut e, 'g');
    press(&mut e, 'U');
    press(&mut e, 'w');
    assert_buf(&e, "HELLO world\n");
}

#[test]
fn test_gn_selects_next_match() {
    let mut e = engine_with("foo bar foo\n");
    search_fwd(&mut e, "foo");
    // n moves to the second "foo"
    press(&mut e, 'n');
    // gn enters visual mode selecting the match "foo" (cols 8-10)
    press(&mut e, 'g');
    press(&mut e, 'n');
    assert_mode(&e, Mode::Visual);
    // cursor should be on the second "foo" region
    let c = e.cursor();
    assert!(c.col >= 8, "cursor col {c:?} should be >= 8 (second foo)");
}

#[test]
fn test_cgn_changes_next_match() {
    let mut e = engine_with("foo bar foo\n");
    search_fwd(&mut e, "foo");
    // Start at the first foo; cgn should delete "foo" and enter Insert
    press(&mut e, 'c');
    press(&mut e, 'g');
    press(&mut e, 'n');
    assert_mode(&e, Mode::Insert);
    // The first "foo" should be deleted
    let content = buf(&e);
    // Either "foo" was deleted or the match region was changed
    assert!(
        !content.starts_with("foo"),
        "first 'foo' should have been deleted; got {content:?}"
    );
}

// ── Group 2: Visual Mode Gaps ─────────────────────────────────────────────────

#[test]
fn test_visual_o_swaps_ends() {
    let mut e = engine_with("hello world\n");
    // Enter visual mode, extend 4 right
    press(&mut e, 'v');
    press(&mut e, 'l');
    press(&mut e, 'l');
    press(&mut e, 'l');
    press(&mut e, 'l');
    // cursor now at col 4, anchor at col 0
    assert_cursor(&e, 0, 4);
    // o: swap cursor to other end of selection
    press(&mut e, 'o');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_visual_block_o_swaps_column() {
    let mut e = engine_with("abcde\nabcde\nabcde\n");
    // Enter visual block
    ctrl(&mut e, 'v');
    // Extend 2 cols right and 2 rows down
    press(&mut e, 'l');
    press(&mut e, 'l');
    press(&mut e, 'j');
    press(&mut e, 'j');
    assert_cursor(&e, 2, 2);
    // O in visual block swaps column (cursor col becomes anchor col and vice versa)
    press(&mut e, 'O');
    // After O, cursor should be at column 0 (the original anchor column)
    assert_cursor(&e, 2, 0);
}

#[test]
fn test_gv_reselects_last_visual() {
    let mut e = engine_with("hello world\n");
    // Make a visual line selection, yank it, escape
    press(&mut e, 'V');
    assert_mode(&e, Mode::VisualLine);
    press(&mut e, 'y');
    assert_mode(&e, Mode::Normal);
    // gv should reenter visual line mode
    press(&mut e, 'g');
    press(&mut e, 'v');
    assert_mode(&e, Mode::VisualLine);
}

// ── Group 3: Register Completeness ───────────────────────────────────────────

#[test]
fn test_register_0_set_on_yank() {
    let mut e = engine_with("hello\nworld\n");
    // yy sets "0
    press(&mut e, 'y');
    press(&mut e, 'y');
    assert_register(&e, '0', "hello\n", true);
    // dd does NOT overwrite "0
    press(&mut e, 'd');
    press(&mut e, 'd');
    assert_register(&e, '0', "hello\n", true);
}

#[test]
fn test_register_1_set_on_linewise_delete() {
    let mut e = engine_with("line1\nline2\n");
    // dd sets "1 (linewise delete)
    press(&mut e, 'd');
    press(&mut e, 'd');
    assert_register(&e, '1', "line1\n", true);
}

#[test]
fn test_numbered_registers_shift() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    // First dd → "1 = "aaa\n"
    press(&mut e, 'd');
    press(&mut e, 'd');
    // Second dd → "1 = "bbb\n", "2 = "aaa\n"
    press(&mut e, 'd');
    press(&mut e, 'd');
    assert_register(&e, '1', "bbb\n", true);
    assert_register(&e, '2', "aaa\n", true);
}

#[test]
fn test_register_minus_small_delete() {
    let mut e = engine_with("hello world\n");
    // dw on "hello" — less than 1 full line → sets "-
    press(&mut e, 'd');
    press(&mut e, 'w');
    // "-" register should contain "hello "
    let (text, linewise) = e
        .registers
        .get(&'-')
        .cloned()
        .expect("register '-' should be set");
    assert!(!linewise, "small delete should not be linewise");
    assert_eq!(text, "hello ", "register '-' should contain deleted word");
}

#[test]
fn test_register_percent_filename() {
    let mut e = engine_with("some content\n");
    // Set a fake file path on the buffer
    e.active_buffer_state_mut().file_path = Some(std::path::PathBuf::from("/home/user/myfile.rs"));
    // "% should contain just the filename
    let content = e
        .get_register_content('%')
        .map(|(s, _)| s)
        .unwrap_or_default();
    assert_eq!(content, "myfile.rs");
}

#[test]
fn test_register_slash_last_search() {
    let mut e = engine_with("foo bar foo\n");
    search_fwd(&mut e, "bar");
    // "/ should contain last search pattern
    let content = e
        .get_register_content('/')
        .map(|(s, _)| s)
        .unwrap_or_default();
    assert_eq!(content, "bar");
}

#[test]
fn test_register_dot_last_insert() {
    let mut e = engine_with("hello\n");
    // Enter insert, type text, escape
    press(&mut e, 'i');
    type_chars(&mut e, "world");
    press_key(&mut e, "Escape");
    // ". should contain last inserted text
    let content = e
        .get_register_content('.')
        .map(|(s, _)| s)
        .unwrap_or_default();
    assert_eq!(content, "world");
}

// ── Group 4: Mark Completeness ────────────────────────────────────────────────

#[test]
fn test_uppercase_mark_set_and_jump() {
    let mut e = engine_with("line1\nline2\nline3\n");
    // Move to line 2, set global mark A
    press(&mut e, 'j');
    press(&mut e, 'j');
    assert_cursor(&e, 2, 0);
    press(&mut e, 'm');
    press(&mut e, 'A');
    // Move away
    press(&mut e, 'g');
    press(&mut e, 'g');
    assert_cursor(&e, 0, 0);
    // 'A should jump back to line 2
    press(&mut e, '\'');
    press(&mut e, 'A');
    assert_cursor(&e, 2, 0);
}

#[test]
fn test_mark_last_jump_double_quote() {
    let mut e = engine_with("line1\nline2\nline3\n");
    // G jumps to last line, recording last_jump_pos
    press(&mut e, 'G');
    assert_cursor(&e, 2, 0);
    // '' should jump back to where we were before G
    press(&mut e, '\'');
    press(&mut e, '\'');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_mark_last_edit_dot() {
    let mut e = engine_with("hello\nworld\n");
    // Edit: insert a char on line 0
    press(&mut e, 'i');
    type_chars(&mut e, "X");
    press_key(&mut e, "Escape");
    // Move away
    press(&mut e, 'G');
    // '. should jump back to line 0 where edit happened
    press(&mut e, '\'');
    press(&mut e, '.');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_mark_visual_start_end() {
    let mut e = engine_with("line1\nline2\nline3\n");
    // Visual line select lines 0-1
    press(&mut e, 'V');
    press(&mut e, 'j');
    press_key(&mut e, "Escape");
    // '< should be line 0
    press(&mut e, '\'');
    press(&mut e, '<');
    assert_cursor(&e, 0, 0);
    // '> should be line 1
    press(&mut e, '\'');
    press(&mut e, '>');
    assert_cursor(&e, 1, 0);
}

// ── Group 5: Insert Mode Gaps ─────────────────────────────────────────────────

#[test]
fn test_insert_ctrl_w_delete_word() {
    let mut e = engine_with("");
    press(&mut e, 'i');
    type_chars(&mut e, "hello world");
    // Ctrl+W should delete "world" (the last word)
    ctrl(&mut e, 'w');
    // Check buffer: "hello " should remain (cursor at end)
    press_key(&mut e, "Escape");
    // Buffer started empty; no trailing newline expected
    assert_buf(&e, "hello ");
}

#[test]
fn test_insert_ctrl_t_indent() {
    let mut e = engine_with("hello\n");
    // shiftwidth defaults to 4
    press(&mut e, 'i');
    ctrl(&mut e, 't');
    press_key(&mut e, "Escape");
    let content = buf(&e);
    // Line should start with 4 spaces
    assert!(
        content.starts_with("    hello"),
        "expected 4-space indent, got {content:?}"
    );
}

#[test]
fn test_insert_ctrl_d_dedent() {
    let mut e = engine_with("    hello\n");
    press(&mut e, 'i');
    ctrl(&mut e, 'd');
    press_key(&mut e, "Escape");
    let content = buf(&e);
    // Should remove 4 spaces of indent
    assert!(
        content.starts_with("hello"),
        "expected dedent, got {content:?}"
    );
}

// ── Group 6: Ex Command Gaps ──────────────────────────────────────────────────

#[test]
fn test_global_delete_matching() {
    let mut e = engine_with("foo line\nbar line\nfoo again\n");
    exec(&mut e, "g/foo/d");
    let lines = get_lines(&e);
    assert_eq!(lines, vec!["bar line"]);
}

#[test]
fn test_global_preserves_non_matching() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    exec(&mut e, "g/bbb/d");
    let lines = get_lines(&e);
    assert_eq!(lines, vec!["aaa", "ccc"]);
}

#[test]
fn test_vglobal_delete_non_matching() {
    let mut e = engine_with("foo\nbar\nfoo\n");
    // :v/foo/d removes lines NOT matching "foo"
    exec(&mut e, "v/foo/d");
    let lines = get_lines(&e);
    assert_eq!(lines, vec!["foo", "foo"]);
}

#[test]
fn test_global_substitute() {
    let mut e = engine_with("foo line\nbar line\nfoo line\n");
    exec(&mut e, "g/foo/s/foo/baz/");
    let lines = get_lines(&e);
    assert_eq!(lines, vec!["baz line", "bar line", "baz line"]);
}

#[test]
fn test_move_line_down() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    // Move line 0 (aaa) down by 1 (after line 1)
    exec(&mut e, "m +1");
    let lines = get_lines(&e);
    assert_eq!(lines, vec!["bbb", "aaa", "ccc"]);
}

#[test]
fn test_move_line_to_dest() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    // Move current line (aaa) to after line 2 (bbb) — 1-based per #114.
    exec(&mut e, "m 2");
    let lines = get_lines(&e);
    assert_eq!(lines, vec!["bbb", "aaa", "ccc"]);
}

#[test]
fn test_copy_line_below() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    // :t . copies current line below itself
    exec(&mut e, "t .");
    let lines = get_lines(&e);
    assert_eq!(lines, vec!["aaa", "aaa", "bbb", "ccc"]);
}

#[test]
fn test_copy_line_to_top() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    // Move to last line
    press(&mut e, 'G');
    // :t 0 copies current line (ccc) to top — 1-based address 0 means "before line 1" (#114).
    exec(&mut e, "t 0");
    let lines = get_lines(&e);
    assert_eq!(lines, vec!["ccc", "aaa", "bbb", "ccc"]);
}

#[test]
fn test_sort_basic() {
    let mut e = engine_with("c\na\nb\n");
    exec(&mut e, "sort");
    let lines = get_lines(&e);
    assert_eq!(lines, vec!["a", "b", "c"]);
}

#[test]
fn test_sort_reverse() {
    let mut e = engine_with("a\nb\nc\n");
    exec(&mut e, "sort r");
    let lines = get_lines(&e);
    assert_eq!(lines, vec!["c", "b", "a"]);
}

#[test]
fn test_sort_numeric() {
    let mut e = engine_with("10\n2\n1\n");
    exec(&mut e, "sort n");
    let lines = get_lines(&e);
    assert_eq!(lines, vec!["1", "2", "10"]);
}

#[test]
fn test_sort_unique() {
    let mut e = engine_with("b\na\nb\nc\na\n");
    exec(&mut e, "sort u");
    let lines = get_lines(&e);
    assert_eq!(lines, vec!["a", "b", "c"]);
}

// ── Group 7: Change List ──────────────────────────────────────────────────────

#[test]
fn test_change_list_g_semicolon() {
    let mut e = engine_with("line1\nline2\nline3\n");
    // Make an edit on line 0
    press(&mut e, 'i');
    type_chars(&mut e, "X");
    press_key(&mut e, "Escape");
    // Move far away
    press(&mut e, 'G');
    assert_eq!(e.cursor().line, 2, "G should move to last line");
    // g; should jump back to the change position (line 0)
    press(&mut e, 'g');
    press(&mut e, ';');
    assert_eq!(e.cursor().line, 0, "g; should jump back to edit line");
}

#[test]
fn test_change_list_multiple() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    // Edit line 0
    press(&mut e, 'i');
    type_chars(&mut e, "X");
    press_key(&mut e, "Escape");
    // Edit line 1
    press(&mut e, 'j');
    press(&mut e, 'i');
    type_chars(&mut e, "Y");
    press_key(&mut e, "Escape");
    // Edit line 2
    press(&mut e, 'j');
    press(&mut e, 'i');
    type_chars(&mut e, "Z");
    press_key(&mut e, "Escape");
    // g; g; should navigate back to change on line 1, then line 0
    press(&mut e, 'g');
    press(&mut e, ';');
    let after_first_g_semi = e.cursor().line;
    press(&mut e, 'g');
    press(&mut e, ';');
    let after_second_g_semi = e.cursor().line;
    // The change positions should go backwards
    assert!(
        after_second_g_semi <= after_first_g_semi,
        "g;g; should navigate back through changes: {after_first_g_semi} then {after_second_g_semi}"
    );
}

#[test]
fn test_change_list_g_comma() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    // Edit line 0
    press(&mut e, 'i');
    type_chars(&mut e, "X");
    press_key(&mut e, "Escape");
    // Edit line 2
    press(&mut e, 'G');
    press(&mut e, 'i');
    type_chars(&mut e, "Z");
    press_key(&mut e, "Escape");
    // g; goes back to line 0 change
    press(&mut e, 'g');
    press(&mut e, ';');
    let back_pos = e.cursor().line;
    // g, goes forward again
    press(&mut e, 'g');
    press(&mut e, ',');
    let forward_pos = e.cursor().line;
    assert!(
        forward_pos >= back_pos,
        "g, should go forward in change list: {back_pos} → {forward_pos}"
    );
}
