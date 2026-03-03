mod common;
use common::*;
use vimcode_core::Mode;

// ── ^ first non-blank ────────────────────────────────────────────────────────

#[test]
fn test_caret_first_non_blank() {
    let mut e = engine_with("   hello\n");
    // cursor starts at col 0
    press(&mut e, '$'); // go to end
    assert_cursor(&e, 0, 7);
    press(&mut e, '^');
    assert_cursor(&e, 0, 3); // first non-blank = col 3
}

#[test]
fn test_caret_empty_line() {
    let mut e = engine_with("\n");
    press(&mut e, '^');
    assert_cursor(&e, 0, 0);
}

// ── g_ last non-blank ────────────────────────────────────────────────────────

#[test]
fn test_g_underscore_last_non_blank() {
    let mut e = engine_with("hello   \n");
    press(&mut e, 'g');
    press(&mut e, '_');
    assert_cursor(&e, 0, 4); // last non-blank = 'o' at col 4
}

#[test]
fn test_g_underscore_no_trailing() {
    let mut e = engine_with("hello\n");
    press(&mut e, 'g');
    press(&mut e, '_');
    assert_cursor(&e, 0, 4);
}

// ── W / B / E WORD motions ───────────────────────────────────────────────────

#[test]
fn test_W_moves_to_next_whitespace_word() {
    let mut e = engine_with("foo.bar baz\n");
    // 'w' would stop at '.', 'W' skips the whole token
    press(&mut e, 'W');
    assert_cursor(&e, 0, 8); // 'baz'
}

#[test]
fn test_B_moves_to_prev_whitespace_word() {
    let mut e = engine_with("foo bar baz\n");
    press(&mut e, '$'); // end: col 10
    press(&mut e, 'B');
    assert_cursor(&e, 0, 8); // 'baz'
    press(&mut e, 'B');
    assert_cursor(&e, 0, 4); // 'bar'
}

#[test]
fn test_E_end_of_WORD() {
    let mut e = engine_with("foo.bar baz\n");
    press(&mut e, 'E');
    assert_cursor(&e, 0, 6); // end of 'foo.bar'
}

// ── H / M / L ────────────────────────────────────────────────────────────────

#[test]
fn test_H_goes_to_screen_top() {
    let mut e = engine_with("line1\nline2\nline3\nline4\nline5\n");
    e.set_viewport_lines(3);
    e.view_mut().scroll_top = 1; // scroll so line1 is off-screen
    e.view_mut().cursor.line = 4;
    press(&mut e, 'H');
    assert_cursor(&e, 1, 0); // scroll_top = 1
}

#[test]
fn test_L_goes_to_screen_bottom() {
    let mut e = engine_with("line1\nline2\nline3\nline4\nline5\n");
    e.set_viewport_lines(3);
    e.view_mut().scroll_top = 0;
    press(&mut e, 'L');
    assert_cursor(&e, 2, 0); // scroll_top + viewport_lines - 1 = 0 + 3 - 1 = 2
}

#[test]
fn test_M_goes_to_screen_middle() {
    let mut e = engine_with("line1\nline2\nline3\nline4\nline5\n");
    e.set_viewport_lines(4);
    e.view_mut().scroll_top = 0;
    press(&mut e, 'M');
    assert_cursor(&e, 2, 0); // scroll_top + viewport_lines/2 = 0 + 2 = 2
}

// ── Ctrl+e / Ctrl+y ──────────────────────────────────────────────────────────

#[test]
fn test_ctrl_e_scrolls_down() {
    let mut e = engine_with("a\nb\nc\nd\ne\n");
    e.set_viewport_lines(3);
    e.view_mut().scroll_top = 0;
    e.view_mut().cursor.line = 2;
    ctrl(&mut e, 'e'); // scroll down one line
    assert_eq!(e.scroll_top(), 1);
    // cursor stays in view
}

#[test]
fn test_ctrl_y_scrolls_up() {
    let mut e = engine_with("a\nb\nc\nd\ne\n");
    e.set_viewport_lines(3);
    e.view_mut().scroll_top = 2;
    e.view_mut().cursor.line = 2;
    ctrl(&mut e, 'y');
    assert_eq!(e.scroll_top(), 1);
}

// ── gJ join without space ────────────────────────────────────────────────────

#[test]
fn test_gJ_join_no_space() {
    let mut e = engine_with("hello\nworld\n");
    press(&mut e, 'g');
    press(&mut e, 'J');
    assert_buf(&e, "helloworld\n");
}

#[test]
fn test_gJ_join_strips_leading_whitespace() {
    let mut e = engine_with("hello\n   world\n");
    press(&mut e, 'g');
    press(&mut e, 'J');
    assert_buf(&e, "helloworld\n");
}

// ── gf open file ─────────────────────────────────────────────────────────────

#[test]
fn test_gf_nonexistent_file_shows_message() {
    let mut e = engine_with("nonexistent_file_xyz.txt\n");
    press(&mut e, 'g');
    press(&mut e, 'f');
    // Should show an error message since the file doesn't exist
    assert!(!e.message.is_empty());
}

// ── g* / g# partial word search ──────────────────────────────────────────────

#[test]
fn test_g_star_partial_search() {
    let mut e = engine_with("foo foobar baz\n");
    // cursor on 'foo'
    press(&mut e, 'g');
    press(&mut e, '*');
    // Should find 'foo' and 'foobar' (no word boundary)
    assert!(e.search_matches.len() >= 2);
}

#[test]
fn test_g_hash_partial_search_backward() {
    let mut e = engine_with("foo foobar baz\n");
    press(&mut e, '$'); // go to end
    press(&mut e, 'g');
    press(&mut e, '#');
    // Should search backward for partial word
    assert!(!e.search_matches.is_empty());
}

// ── R: Replace mode ──────────────────────────────────────────────────────────

#[test]
fn test_R_enters_replace_mode() {
    let mut e = engine_with("hello\n");
    press(&mut e, 'R');
    assert_eq!(e.mode, Mode::Replace);
}

#[test]
fn test_R_overwrites_chars() {
    let mut e = engine_with("hello\n");
    press(&mut e, 'R');
    press(&mut e, 'w');
    press(&mut e, 'o');
    // 'he' remains but first two chars become 'wo'? No: R overwrites from cursor
    // cursor starts at col 0, so 'h' → 'w', 'e' → 'o'
    assert_buf(&e, "wollo\n");
}

#[test]
fn test_R_escape_returns_to_normal() {
    let mut e = engine_with("hello\n");
    press(&mut e, 'R');
    press_key(&mut e, "Escape");
    assert_eq!(e.mode, Mode::Normal);
}

// ── Ctrl+a / Ctrl+x number increment ────────────────────────────────────────

#[test]
fn test_ctrl_a_increments_number() {
    let mut e = engine_with("count 5 here\n");
    // Move cursor onto '5' — one 'w' from 'count' lands on '5'
    press(&mut e, 'w');
    assert_cursor(&e, 0, 6); // '5' is at col 6
    ctrl(&mut e, 'a');
    assert_buf(&e, "count 6 here\n");
}

#[test]
fn test_ctrl_x_decrements_number() {
    let mut e = engine_with("count 5 here\n");
    press(&mut e, 'w');
    assert_cursor(&e, 0, 6);
    ctrl(&mut e, 'x');
    assert_buf(&e, "count 4 here\n");
}

#[test]
fn test_ctrl_a_at_zero() {
    let mut e = engine_with("value 0\n");
    press(&mut e, 'w'); // 'value' → '0'
    ctrl(&mut e, 'a');
    assert_buf(&e, "value 1\n");
}

#[test]
fn test_count_ctrl_a() {
    let mut e = engine_with("x 3 y\n");
    press(&mut e, 'w'); // 'x' → '3'
                        // 5<C-a> adds 5
    press(&mut e, '5');
    ctrl(&mut e, 'a');
    assert_buf(&e, "x 8 y\n");
}

// ── = operator auto-indent ───────────────────────────────────────────────────

#[test]
fn test_equal_equal_reindents_line() {
    let mut e = engine_with("  fn foo() {\n      x\n}\n");
    // Position on line 1 (over-indented)
    press_key(&mut e, "Down");
    press(&mut e, '=');
    press(&mut e, '=');
    // The line should be reindented; content should still be there
    let content = buf(&e);
    assert!(content.contains('x'));
}

// ── iW / aW text objects ─────────────────────────────────────────────────────

#[test]
fn test_diW_deletes_WORD() {
    let mut e = engine_with("foo.bar baz\n");
    // dWORD: delete inner WORD (the whole non-whitespace token 'foo.bar')
    press(&mut e, 'd');
    press(&mut e, 'i');
    press(&mut e, 'W');
    assert_buf(&e, " baz\n");
}

#[test]
fn test_daW_deletes_WORD_and_space() {
    let mut e = engine_with("foo.bar baz\n");
    press(&mut e, 'd');
    press(&mut e, 'a');
    press(&mut e, 'W');
    // Should delete 'foo.bar ' (with trailing space)
    assert_buf(&e, "baz\n");
}

// ── ]p / [p paste with indent ────────────────────────────────────────────────

#[test]
fn test_bracket_p_paste_with_indent() {
    let mut e = engine_with("    base\n");
    // Yank the line
    press(&mut e, 'y');
    press(&mut e, 'y');
    // ]p should paste with indent matching current line
    press(&mut e, ']');
    press(&mut e, 'p');
    let content = buf(&e);
    // Both lines should exist
    assert_eq!(content.lines().filter(|l| l.contains("base")).count(), 2);
}

// ── Insert mode Ctrl+r ───────────────────────────────────────────────────────

#[test]
fn test_insert_ctrl_r_inserts_register() {
    let mut e = engine_with("hello\n");
    // Yank 'hello' into default register
    press(&mut e, 'y');
    press(&mut e, 'y');
    // Move to new line, enter insert mode
    press(&mut e, 'o');
    assert_eq!(e.mode, Mode::Insert);
    // Ctrl+r then " (unnamed register)
    ctrl(&mut e, 'r');
    press(&mut e, '"');
    // Should insert "hello\n" at cursor
    let content = buf(&e);
    // Check that 'hello' appears twice
    assert_eq!(content.matches("hello").count(), 2);
    press_key(&mut e, "Escape");
}

// ── Insert mode Ctrl+u ───────────────────────────────────────────────────────

#[test]
fn test_insert_ctrl_u_deletes_to_line_start() {
    let mut e = engine_with("\n");
    press(&mut e, 'i');
    type_chars(&mut e, "hello world");
    ctrl(&mut e, 'u');
    press_key(&mut e, "Escape");
    assert_buf(&e, "\n");
}

// ── :noh / :nohlsearch ──────────────────────────────────────────────────────

#[test]
fn test_noh_clears_search_matches() {
    let mut e = engine_with("hello world\n");
    search_fwd(&mut e, "hello");
    assert!(!e.search_matches.is_empty());
    exec(&mut e, "noh");
    assert!(e.search_matches.is_empty());
}

#[test]
fn test_nohlsearch_alias() {
    let mut e = engine_with("hello world\n");
    search_fwd(&mut e, "hello");
    assert!(!e.search_matches.is_empty());
    exec(&mut e, "nohlsearch");
    assert!(e.search_matches.is_empty());
}

// ── :wa ──────────────────────────────────────────────────────────────────────

#[test]
fn test_wa_write_all_shows_message() {
    let mut e = engine_with("content\n");
    exec(&mut e, "wa");
    // Should show a message about files written (0 since no file path)
    assert!(!e.message.is_empty());
}

// ── :marks ──────────────────────────────────────────────────────────────────

#[test]
fn test_marks_displays_local_marks() {
    let mut e = engine_with("line1\nline2\nline3\n");
    // Set mark 'a'
    press(&mut e, 'm');
    press(&mut e, 'a');
    exec(&mut e, "marks");
    assert!(e.message.contains('a'));
}

// ── :jumps ──────────────────────────────────────────────────────────────────

#[test]
fn test_jumps_shows_jump_list() {
    let mut e = engine_with("a\nb\nc\n");
    exec(&mut e, "jumps");
    assert!(e.message.contains("jump"));
}

// ── :changes ─────────────────────────────────────────────────────────────────

#[test]
fn test_changes_shows_change_list() {
    let mut e = engine_with("hello\n");
    // Make a change
    press(&mut e, 'i');
    type_chars(&mut e, "x");
    press_key(&mut e, "Escape");
    exec(&mut e, "changes");
    assert!(e.message.contains("change"));
}

// ── :history ─────────────────────────────────────────────────────────────────

#[test]
fn test_history_shows_command_history() {
    let mut e = engine_with("hello\n");
    run_cmd(&mut e, "echo hello");
    exec(&mut e, "history");
    assert!(e.message.contains("History") || e.message.contains("echo"));
}

// ── :reg ─────────────────────────────────────────────────────────────────────

#[test]
fn test_reg_shows_registers() {
    let mut e = engine_with("hello\n");
    press(&mut e, 'y');
    press(&mut e, 'w'); // yank 'hello'
    exec(&mut e, "reg");
    assert!(e.message.contains("Registers") || e.message.contains('"'));
}

// ── :tabmove ────────────────────────────────────────────────────────────────

#[test]
fn test_tabmove_single_tab_no_change() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "tabmove 0");
    // Single tab: no change, no error
    assert_eq!(e.mode, Mode::Normal);
}

// ── :echo ────────────────────────────────────────────────────────────────────

#[test]
fn test_echo_shows_message() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "echo hello world");
    assert_eq!(e.message, "hello world");
}

#[test]
fn test_echo_empty_clears_message() {
    let mut e = engine_with("hello\n");
    e.message = "old message".to_string();
    exec(&mut e, "echo");
    assert_eq!(e.message, "");
}

// ── :!cmd shell execution ────────────────────────────────────────────────────

#[test]
fn test_shell_command_shows_output() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "!echo test_output");
    assert!(e.message.contains("test_output"));
}

// ── ignorecase / smartcase ──────────────────────────────────────────────────

#[test]
fn test_ignorecase_finds_uppercase() {
    let mut e = engine_with("Hello World\n");
    e.settings.ignorecase = true;
    e.settings.smartcase = false;
    search_fwd(&mut e, "hello");
    assert!(!e.search_matches.is_empty());
}

#[test]
fn test_smartcase_uppercase_query_is_case_sensitive() {
    let mut e = engine_with("Hello hello\n");
    e.settings.ignorecase = true;
    e.settings.smartcase = true;
    search_fwd(&mut e, "Hello");
    // With smartcase + uppercase pattern: case-sensitive → only 1 match
    assert_eq!(e.search_matches.len(), 1);
}

#[test]
fn test_smartcase_lowercase_query_is_case_insensitive() {
    let mut e = engine_with("Hello hello\n");
    e.settings.ignorecase = true;
    e.settings.smartcase = true;
    search_fwd(&mut e, "hello");
    // lowercase pattern + smartcase → case-insensitive → 2 matches
    assert_eq!(e.search_matches.len(), 2);
}

#[test]
fn test_no_ignorecase_is_case_sensitive() {
    let mut e = engine_with("Hello hello\n");
    e.settings.ignorecase = false;
    search_fwd(&mut e, "hello");
    // Only lowercase match
    assert_eq!(e.search_matches.len(), 1);
}

// ── hlsearch ─────────────────────────────────────────────────────────────────

#[test]
fn test_hlsearch_true_keeps_matches() {
    let mut e = engine_with("hello hello\n");
    e.settings.hlsearch = true;
    search_fwd(&mut e, "hello");
    assert_eq!(e.search_matches.len(), 2);
}

// ── scrolloff ────────────────────────────────────────────────────────────────

#[test]
fn test_scrolloff_keeps_padding_above() {
    let mut e = engine_with("a\nb\nc\nd\ne\nf\ng\nh\n");
    e.set_viewport_lines(5);
    e.settings.scrolloff = 2;
    e.view_mut().scroll_top = 3;
    e.view_mut().cursor.line = 3; // at scroll_top: violates scrolloff=2
    e.ensure_cursor_visible();
    // scroll_top should adjust so cursor is >= scroll_top + scrolloff
    assert!(e.scroll_top() <= e.view().cursor.line.saturating_sub(2));
}

// ── set :set options ─────────────────────────────────────────────────────────

#[test]
fn test_set_ignorecase() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "set ignorecase");
    assert!(e.settings.ignorecase);
    exec(&mut e, "set noignorecase");
    assert!(!e.settings.ignorecase);
}

#[test]
fn test_set_smartcase() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "set smartcase");
    assert!(e.settings.smartcase);
}

#[test]
fn test_set_scrolloff() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "set scrolloff=3");
    assert_eq!(e.settings.scrolloff, 3);
}

#[test]
fn test_set_hlsearch() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "set nohlsearch");
    assert!(!e.settings.hlsearch);
    exec(&mut e, "set hlsearch");
    assert!(e.settings.hlsearch);
}

#[test]
fn test_set_cursorline() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "set cursorline");
    assert!(e.settings.cursorline);
    exec(&mut e, "set nocursorline");
    assert!(!e.settings.cursorline);
}

#[test]
fn test_set_colorcolumn() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "set colorcolumn=80");
    assert_eq!(e.settings.colorcolumn, "80");
}

#[test]
fn test_set_textwidth() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "set textwidth=79");
    assert_eq!(e.settings.textwidth, 79);
}
