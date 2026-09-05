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
    // #805: M's target is `scroll_top + (visible_lines - 1) / 2`, matching
    // real Vim's own `(height - 1) / 2` — NOT `viewport_lines / 2`, which
    // this test asserted before the fix and which is off by one for an
    // even-height window (verified against real `nvim`: a freshly opened
    // 4-row window with cursor on line 1 sends `M` to line 2, i.e. topline
    // (1) + 1, not topline + 2).
    let mut e = engine_with("line1\nline2\nline3\nline4\nline5\n");
    e.set_viewport_lines(4);
    e.view_mut().scroll_top = 0;
    press(&mut e, 'M');
    assert_cursor(&e, 1, 0); // scroll_top + (viewport_lines - 1) / 2 = 0 + 1 = 1
}

#[test]
fn test_H_respects_scrolloff() {
    // #805: 'scrolloff' keeps H at least that many lines below the window's
    // top edge, not pinned exactly to scroll_top.
    let mut e = engine_with("l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\n");
    e.set_viewport_lines(6);
    e.settings.scrolloff = 2;
    e.view_mut().scroll_top = 2;
    e.view_mut().cursor.line = 6;
    press(&mut e, 'H');
    assert_cursor(&e, 4, 0); // scroll_top + scrolloff = 2 + 2 = 4
}

#[test]
fn test_L_respects_scrolloff() {
    // #805: mirror of `test_H_respects_scrolloff` for the bottom edge.
    let mut e = engine_with("l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\n");
    e.set_viewport_lines(6);
    e.settings.scrolloff = 2;
    e.view_mut().scroll_top = 2;
    press(&mut e, 'L');
    // window bottom = scroll_top + viewport - 1 = 7; minus scrolloff (2) = 5
    assert_cursor(&e, 5, 0);
}

#[test]
fn test_ctrl_d_count_sets_sticky_scroll_value() {
    // #805: an explicit count on <C-d> SETS Vim's 'scroll' option (replacing,
    // not multiplying, any previous value), and a later bare <C-d> reuses it.
    let mut e = engine_with(&(1..=60).map(|n| format!("L{n}\n")).collect::<String>());
    e.set_viewport_lines(20);
    type_chars(&mut e, "5"); // count
    ctrl(&mut e, 'd'); // sets 'scroll' = 5, moves 5 lines down
    assert_cursor(&e, 5, 0);
    ctrl(&mut e, 'd'); // bare <C-d>: reuses 'scroll' = 5, NOT the default half-page
    assert_cursor(&e, 10, 0);
}

/// A 60-line buffer in a 22-row window — the exact fixture the `scroll:*`
/// conformance cases use, so these expectations can be read straight off real
/// interactive Neovim (`'scroll'` = 22 / 2 = 11).
fn scroll_fixture() -> vimcode_core::Engine {
    let mut e = engine_with(&(1..=60).map(|n| format!("L{n}\n")).collect::<String>());
    e.set_viewport_lines(22);
    e
}

// #805 review: the conformance labels `scroll:C-d C-d`, `scroll:5C-d C-d`,
// `scroll:C-d twice then C-u` and `scroll:C-f C-f` stay in KNOWN_DEVIATIONS
// because the *headless* nvim oracle mis-handles the second and later scroll
// command of a single feedkeys burst (see the group-B comment block in
// `tests/nvim_conformance.rs`). The expectations below are the values real
// *interactive* Neovim produces for the same fixture — captured with
// `scripts/nvim_headless_vs_interactive_repro.sh` — so a regression in
// vimcode's own chained-scroll behaviour still fails a test rather than
// hiding behind a deviation label.

#[test]
fn test_ctrl_d_chain_moves_a_full_scroll_each_time() {
    // interactive nvim: <C-d><C-d> from line 1 lands on line 23 (1-indexed).
    // headless nvim wrongly says 22.
    let mut e = scroll_fixture();
    ctrl(&mut e, 'd');
    assert_cursor(&e, 11, 0);
    ctrl(&mut e, 'd');
    assert_cursor(&e, 22, 0);
}

#[test]
fn test_ctrl_d_chain_then_ctrl_u_returns_one_scroll() {
    // interactive nvim: <C-d><C-d><C-u> from line 1 lands back on line 12.
    let mut e = scroll_fixture();
    ctrl(&mut e, 'd');
    ctrl(&mut e, 'd');
    ctrl(&mut e, 'u');
    assert_cursor(&e, 11, 0);
}

#[test]
fn test_counted_ctrl_d_then_bare_ctrl_d_full_window() {
    // interactive nvim: 5<C-d><C-d> from line 1 lands on line 11 — the count
    // SETS 'scroll' to 5 and the bare <C-d> reuses it. headless says 10.
    let mut e = scroll_fixture();
    type_chars(&mut e, "5");
    ctrl(&mut e, 'd');
    assert_cursor(&e, 5, 0);
    ctrl(&mut e, 'd');
    assert_cursor(&e, 10, 0);
}

#[test]
fn test_ctrl_f_chain_pages_forward_twice() {
    // interactive nvim: <C-f><C-f> from line 1 lands on line 41. headless
    // says 19 — i.e. *above* where a single <C-f> (line 21) lands.
    let mut e = scroll_fixture();
    ctrl(&mut e, 'f');
    assert_cursor(&e, 20, 0);
    ctrl(&mut e, 'f');
    assert_cursor(&e, 40, 0);
}

#[test]
fn test_dollar_sticks_through_ctrl_e_scrolloff_push() {
    // #805 review: <C-e>/<C-y> are curswant-preserving, so when scrolloff
    // forces the cursor onto a new line the column must be re-derived from
    // the `$`-set CURSWANT_EOL, not clamped from the old column.
    let mut e = engine_with("aaaaaaaa\nbb\ncccccccc\ndddddddd\neeeeeeee\n");
    e.set_viewport_lines(3);
    e.view_mut().scroll_top = 0;
    e.view_mut().cursor.line = 0;
    press(&mut e, '$'); // curswant = end-of-line
    assert_cursor(&e, 0, 7);
    ctrl(&mut e, 'e'); // scroll down: cursor pushed off the top onto line 1
    assert_cursor(&e, 1, 1); // "bb" is short — clamped, but curswant survives
    ctrl(&mut e, 'e'); // pushed onto line 2, a long line again
    assert_cursor(&e, 2, 7); // back to end-of-line, not stuck at col 1
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
fn test_gJ_join_preserves_leading_whitespace() {
    // gJ joins without inserting space AND without stripping whitespace (Neovim-verified)
    let mut e = engine_with("hello\n   world\n");
    press(&mut e, 'g');
    press(&mut e, 'J');
    assert_buf(&e, "hello   world\n");
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

/// `<C-a>` / `<C-x>` across every 'nrformats' shape VimCode supports (#807).
///
/// Expectations are the **oracle's**, not hand-authored: each row was taken
/// from `nvim --headless` on the same buffer + keys (they are also covered
/// case-by-case by the `num:*` entries in `tests/nvim_conformance.rs`, which
/// only run when nvim is on PATH — this table is the always-on regression
/// net). VimCode pins Neovim's default 'nrformats' of `bin,hex`, so `007` is
/// decimal-with-leading-zeros, not octal.
#[test]
fn test_number_formats_table() {
    // (start, keys-are-<C-a>?, expected buffer, expected cursor col)
    let cases: &[(&str, bool, &str, usize)] = &[
        // Leading zeros: the width is preserved, and the run is *decimal*
        // (before #807, `0099<C-x>` parsed as octal into an i64 and underflowed
        // to "1777777777777777777777").
        ("007", true, "008", 2),
        ("009", true, "010", 2),
        ("0099", false, "0098", 3),
        ("000", false, "-001", 3),
        // Hex keeps its `0x`/`0X` prefix case and the case of its last letter.
        ("0x0", true, "0x1", 2),
        ("0x0", false, "0xffffffffffffffff", 17),
        ("0xaB", true, "0xAC", 3),
        ("0xAb", true, "0xac", 3),
        ("0X0f", true, "0X10", 3),
        // A leading `-` is not part of a hex literal: only the digits change.
        ("-0x1", true, "-0x2", 3),
        // Binary.
        ("0b101", true, "0b110", 4),
        ("0B101", false, "0B100", 4),
        // Decimal signs and u64 saturation.
        ("-1", true, "0", 0),
        ("-1", false, "-2", 1),
        ("99999999999999999999", true, "18446744073709551615", 19),
    ];
    for &(start, add, expect, col) in cases {
        let mut e = engine_with(&format!("{start}\n"));
        ctrl(&mut e, if add { 'a' } else { 'x' });
        assert_eq!(
            buf(&e),
            format!("{expect}\n"),
            "{start} {}",
            if add { "<C-a>" } else { "<C-x>" }
        );
        assert_eq!(
            e.cursor().col,
            col,
            "{start} {} cursor",
            if add { "<C-a>" } else { "<C-x>" }
        );
    }
}

/// Visual-mode `<C-a>` bumps every line's first selected number by the same
/// amount, `g<C-a>` steps per changed line, and the cursor lands on the first
/// change — not on the last line touched (#807).
#[test]
fn test_visual_ctrl_a_variants() {
    let mut e = engine_with("1\n1\n1\n");
    press(&mut e, 'V');
    press(&mut e, 'j');
    press(&mut e, 'j');
    ctrl(&mut e, 'a');
    assert_buf(&e, "2\n2\n2\n");
    assert_cursor(&e, 0, 0);

    let mut e = engine_with("1\n1\n1\n");
    press(&mut e, 'V');
    press(&mut e, 'j');
    press(&mut e, 'j');
    press(&mut e, 'g');
    ctrl(&mut e, 'a');
    assert_buf(&e, "2\n3\n4\n");
    assert_cursor(&e, 0, 0);

    // Lines with no number do not advance the g<C-a> counter.
    let mut e = engine_with("1\nx\n1\n");
    press(&mut e, 'V');
    press(&mut e, 'j');
    press(&mut e, 'j');
    press(&mut e, 'g');
    ctrl(&mut e, 'a');
    assert_buf(&e, "2\nx\n3\n");

    // Only the first number *inside the selection* on each line changes.
    let mut e = engine_with("1 2\n3 4\n");
    press(&mut e, 'V');
    press(&mut e, 'j');
    ctrl(&mut e, 'a');
    assert_buf(&e, "2 2\n4 4\n");

    // Blockwise: the selection picks which number on the line is hit.
    let mut e = engine_with("1 1\n1 1\n");
    press(&mut e, 'l');
    press(&mut e, 'l');
    ctrl(&mut e, 'v');
    press(&mut e, 'j');
    ctrl(&mut e, 'a');
    assert_buf(&e, "1 2\n1 2\n");
    assert_cursor(&e, 0, 2);

    // A `-` outside the selection is not a sign: `vl<C-a>` on the `5` of
    // `x -5` gives `x -6`, not `x -4`.
    let mut e = engine_with("x -5\n");
    press(&mut e, '$');
    press(&mut e, 'v');
    press(&mut e, 'l');
    ctrl(&mut e, 'a');
    assert_buf(&e, "x -6\n");
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
    // Default is true
    assert!(e.settings.cursorline);
    exec(&mut e, "set nocursorline");
    assert!(!e.settings.cursorline);
    exec(&mut e, "set cursorline");
    assert!(e.settings.cursorline);
    // Abbreviation also works
    exec(&mut e, "set nocul");
    assert!(!e.settings.cursorline);
    exec(&mut e, "set cul");
    assert!(e.settings.cursorline);
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

// ── #806: mark adjustment, '' toggling, macro failure-stop/recursion ───────────

#[test]
fn test_mark_shifts_after_line_inserted_above() {
    // `:h mark-motions` — inserting a line above a mark must shift the
    // mark's line number down with it, not leave it pointing at whatever
    // text slid into its old slot.
    let mut e = engine_with("a\nb\nc\n");
    press(&mut e, 'j'); // line 1, "b"
    press(&mut e, 'm');
    press(&mut e, 'a'); // mark a := (line 1, "b")
    press(&mut e, 'g');
    press(&mut e, 'g'); // back to line 0, "a"
    press(&mut e, 'O'); // open a new blank line above line 0
    type_chars(&mut e, "x");
    press_key(&mut e, "Escape");
    // Buffer is now ["x", "a", "b", "c"] — "b" (and mark a) shifted to line 2.
    assert_eq!(
        get_lines(&e),
        vec![
            "x".to_string(),
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
        ]
    );
    press(&mut e, '\'');
    press(&mut e, 'a');
    assert_cursor(&e, 2, 0);
}

#[test]
fn test_mark_on_same_line_shifts_when_o_inserts_above() {
    // #806 review: `O` splices a new blank line in at COLUMN 0 of the
    // cursor's own line, so the entire original line — including a mark
    // sitting on that same line, not just marks strictly below it — slides
    // down by one. Before this fix, `shift_marks_for_line_insert` only
    // shifted `cursor.line > at_line`, silently leaving a same-line mark
    // pointing at the new, blank line instead of the text it used to mark.
    let mut e = engine_with("a\nb\nc\n");
    press(&mut e, 'm');
    press(&mut e, 'a'); // mark a := (line 0, "a") -- the line `O` fires from
    press(&mut e, 'O'); // open a new blank line above line 0
    type_chars(&mut e, "x");
    press_key(&mut e, "Escape");
    // Buffer is now ["x", "a", "b", "c"] -- "a" (and mark a) shifted to line 1.
    assert_eq!(
        get_lines(&e),
        vec![
            "x".to_string(),
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
        ]
    );
    press(&mut e, '\'');
    press(&mut e, 'a');
    assert_cursor(&e, 1, 0);
}

#[test]
fn test_visual_expr_register_prompts() {
    // #806 review: the Visual-mode `'"'` pending-key arm mirrored the
    // Normal-mode register-name set but dropped the `'='` branch that opens
    // `expr_register_pending` — typing `"=` in Visual mode just set
    // `selected_register = Some('=')` with no prompt ever shown, so the
    // keys meant for the expression ("1+1<CR>") fell through and were
    // executed as ordinary Visual-mode motions instead of being consumed by
    // the expression prompt. Round-trip through `p` to prove the expression
    // was actually evaluated and landed in the buffer, not just that some
    // internal flag got set.
    let mut e = engine_with("hello world\n");
    press(&mut e, 'v'); // Visual mode, selects "h"
    press(&mut e, '"');
    press(&mut e, '=');
    type_chars(&mut e, "1+1");
    press_key(&mut e, "Return"); // evaluates to "2", stored in register '='
    press(&mut e, 'p'); // replace the visual selection with register '='
    assert_eq!(get_lines(&e), vec!["2ello world".to_string()]);
}

#[test]
fn test_double_quote_mark_toggles() {
    // `''` jumps to the position before the last jump — and IS ITSELF a
    // jump, so a second `''` toggles back (`:h ''`).
    let mut e = engine_with("a\nb\nc\nd\n");
    press(&mut e, '3');
    press(&mut e, 'G'); // line 2 ("c") — a real jump, so '' now targets line 0
    assert_cursor(&e, 2, 0);
    press(&mut e, '\'');
    press(&mut e, '\'');
    assert_cursor(&e, 0, 0); // back to where 3G started
    press(&mut e, '\'');
    press(&mut e, '\'');
    assert_cursor(&e, 2, 0); // toggled forward again, to where '' jumped from
}

#[test]
fn test_macro_count_stops_at_first_failure() {
    // Vim aborts the whole repeat count when a command inside the macro
    // fails — `100@a` must behave exactly like `10@a` here (stop once `f,`
    // can't find a comma), not run all 100 requested repetitions.
    let mut e = engine_with("a,b\nc,d\ne f\ng,h\n");
    press(&mut e, 'q');
    press(&mut e, 'a');
    press(&mut e, '0');
    press(&mut e, 'f');
    press(&mut e, ',');
    press(&mut e, 'x');
    press(&mut e, 'j');
    press(&mut e, 'q');
    press(&mut e, '1');
    press(&mut e, '0');
    press(&mut e, '0');
    press(&mut e, '@');
    press(&mut e, 'a');
    drain_macro_queue(&mut e);
    assert_eq!(
        get_lines(&e),
        vec![
            "ab".to_string(),
            "cd".to_string(),
            "e f".to_string(),
            "g,h".to_string(),
        ],
        "the count must stop at the first line without a comma, not paper \
         over it and keep going"
    );
    assert_cursor(&e, 2, 0);
}

#[test]
fn test_recursive_macro_terminates_at_eof() {
    // The idiomatic Vim "run this macro over the whole file" trick: record
    // an empty macro (`qaq`), then re-record it ending in a self-referential
    // `@a` — at record time that refers to the still-empty macro (a no-op),
    // but once saved, playing it back makes `@a` recurse for real. It must
    // stop when `j` fails at the last line, not spin forever.
    let mut e = engine_with("a\nb\nc\nd\n");
    press(&mut e, 'q');
    press(&mut e, 'a');
    press(&mut e, 'q'); // qaq: register 'a' is now "" (empty)
    press(&mut e, 'q');
    press(&mut e, 'a'); // start recording 'a' for real
    press(&mut e, 'A');
    type_chars(&mut e, "!");
    press_key(&mut e, "Escape");
    press(&mut e, 'j');
    press(&mut e, '@');
    press(&mut e, 'a'); // during recording, plays back the still-empty macro
    press(&mut e, 'q'); // stop: register 'a' is now "A!<Esc>j@a"
    drain_macro_queue(&mut e); // nothing should be queued, but be safe
    press(&mut e, '@');
    press(&mut e, 'a'); // the real, recursive invocation
                        // Bounded drain: assert it terminates well before any "safety cap"
                        // would — a regression back to the old behavior spins until
                        // MAX_MACRO_RECURSION, which this loop deliberately doesn't reach.
    let mut iterations = 0;
    while !e.macro_playback_queue.is_empty() && iterations < 1000 {
        e.advance_macro_playback();
        iterations += 1;
    }
    assert!(
        e.macro_playback_queue.is_empty(),
        "recursive macro should terminate at EOF, not keep running \
         (queue still has {} keys after {iterations} iterations)",
        e.macro_playback_queue.len()
    );
    assert_eq!(
        get_lines(&e),
        vec![
            "a!".to_string(),
            "b!".to_string(),
            "c!".to_string(),
            "d!".to_string(),
        ]
    );
}

/// `r<CR>` / `r<Tab>` and Replace-mode `<BS>` (#807).
///
/// Expectations are the nvim oracle's (`op:r<CR>`, `op:3r<CR>`, `misc:r Tab`,
/// `op:5r beyond eol`, `op:R BS restores`, `op:2R`, `op:R <CR>`).
#[test]
fn test_r_special_keys_and_replace_mode_backspace() {
    // r<CR> replaces the character with a line break and lands on the new line.
    let mut e = engine_with("abc def\n");
    type_chars(&mut e, "lll");
    press(&mut e, 'r');
    press_key(&mut e, "Return");
    assert_buf(&e, "abc\ndef\n");
    assert_cursor(&e, 1, 0);

    // A count replaces N characters with ONE line break.
    let mut e = engine_with("abcdef\n");
    press(&mut e, 'l');
    type_chars(&mut e, "3");
    press(&mut e, 'r');
    press_key(&mut e, "Return");
    assert_buf(&e, "a\nef\n");

    // r<Tab> honours 'expandtab'.
    let mut e = engine_with("ab\n");
    press(&mut e, 'r');
    press_key(&mut e, "Tab");
    assert_buf(&e, "    b\n");
    assert_cursor(&e, 0, 3);

    // A count that does not fit on the line makes `r` do nothing at all.
    let mut e = engine_with("abc\n");
    press(&mut e, 'l');
    type_chars(&mut e, "5");
    press(&mut e, 'r');
    press(&mut e, 'x');
    assert_buf(&e, "abc\n");

    // Replace-mode <BS> restores the overwritten characters.
    let mut e = engine_with("abcdef\n");
    press(&mut e, 'l');
    type_chars(&mut e, "Rxyz");
    assert_buf(&e, "axyzef\n");
    press_key(&mut e, "BackSpace");
    press_key(&mut e, "BackSpace");
    press_key(&mut e, "Escape");
    assert_buf(&e, "axcdef\n");

    // 2R re-applies the typed text, still overwriting.
    let mut e = engine_with("abcdef\n");
    type_chars(&mut e, "2Rxy");
    press_key(&mut e, "Escape");
    assert_buf(&e, "xyxyef\n");

    // <CR> in Replace mode breaks the line without consuming a character.
    let mut e = engine_with("abcdef\n");
    press(&mut e, 'l');
    type_chars(&mut e, "Rx");
    press_key(&mut e, "Return");
    type_chars(&mut e, "y");
    press_key(&mut e, "Escape");
    assert_buf(&e, "ax\nydef\n");
}
