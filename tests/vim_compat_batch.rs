mod common;
use common::*;

// ═══════════════════════════════════════════════════════════════════════════
// Tier 1 — Quick wins
// ═══════════════════════════════════════════════════════════════════════════

// ── + motion (first non-blank N lines down) ──────────────────────────────

#[test]
fn test_plus_motion() {
    let mut e = engine_with("  hello\n  world\n  third\n");
    type_chars(&mut e, "+");
    assert_cursor(&e, 1, 2); // first non-blank of line 2
}

#[test]
fn test_plus_motion_with_count() {
    let mut e = engine_with("  hello\n  world\n  third\n");
    type_chars(&mut e, "2+");
    assert_cursor(&e, 2, 2); // first non-blank of line 3
}

#[test]
fn test_plus_motion_at_end() {
    let mut e = engine_with("hello\nworld\n");
    e.view_mut().cursor.line = 1;
    type_chars(&mut e, "+");
    // Already on last line, stays there
    assert_cursor(&e, 1, 0);
}

// ── - motion (first non-blank N lines up) ────────────────────────────────

#[test]
fn test_minus_motion() {
    let mut e = engine_with("  hello\n  world\n");
    e.view_mut().cursor.line = 1;
    type_chars(&mut e, "-");
    assert_cursor(&e, 0, 2);
}

#[test]
fn test_minus_motion_with_count() {
    let mut e = engine_with("  first\n  second\n  third\n");
    e.view_mut().cursor.line = 2;
    type_chars(&mut e, "2-");
    assert_cursor(&e, 0, 2);
}

// ── _ motion (first non-blank N-1 lines down) ───────────────────────────

#[test]
fn test_underscore_motion() {
    let mut e = engine_with("  hello\n  world\n");
    type_chars(&mut e, "_");
    // 1_ = current line, first non-blank
    assert_cursor(&e, 0, 2);
}

#[test]
fn test_underscore_motion_with_count() {
    let mut e = engine_with("  hello\n  world\n  third\n");
    type_chars(&mut e, "3_");
    // 3_ = 2 lines down, first non-blank
    assert_cursor(&e, 2, 2);
}

// ── | motion (go to column N) ───────────────────────────────────────────

#[test]
fn test_pipe_motion_default() {
    let mut e = engine_with("hello world\n");
    e.view_mut().cursor.col = 5;
    type_chars(&mut e, "|");
    // Default count 1 → column 0
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_pipe_motion_with_count() {
    let mut e = engine_with("hello world\n");
    type_chars(&mut e, "6|");
    assert_cursor(&e, 0, 5); // column 6 → 0-indexed = 5
}

#[test]
fn test_pipe_motion_beyond_line() {
    let mut e = engine_with("hi\n");
    type_chars(&mut e, "99|");
    // Clamps to last column
    assert_cursor(&e, 0, 1);
}

// ── gp / gP (paste, cursor after) ───────────────────────────────────────

#[test]
fn test_gp_paste_after_charwise() {
    let mut e = engine_with("hello\n");
    type_chars(&mut e, "yiw"); // yank "hello"
    type_chars(&mut e, "$"); // go to end
    type_chars(&mut e, "gp");
    // After pasting "hello" after last char, cursor should be after pasted text
    let content = buf(&e);
    assert!(content.starts_with("hellohello"), "got: {}", content);
}

#[test]
fn test_gp_paste_after_linewise() {
    let mut e = engine_with("line1\nline2\n");
    type_chars(&mut e, "yy"); // yank line1
    type_chars(&mut e, "gp"); // paste below, cursor after
    assert_eq!(e.cursor().line, 2); // cursor on line after pasted line
}

#[test]
fn test_g_big_p_paste_before_linewise() {
    let mut e = engine_with("line1\nline2\n");
    e.view_mut().cursor.line = 1;
    type_chars(&mut e, "yy"); // yank line2
    type_chars(&mut e, "gP"); // paste before, cursor after
    assert_eq!(e.cursor().line, 2); // cursor on line after pasted text
}

// ── @: (repeat last ex command) ─────────────────────────────────────────

#[test]
fn test_at_colon_repeat_ex_command() {
    let mut e = engine_with("hello\nworld\n");
    run_cmd(&mut e, "s/hello/bye/");
    assert_eq!(get_lines(&e)[0], "bye");
    // Move to line 2 and repeat
    e.view_mut().cursor.line = 1;
    type_chars(&mut e, "@:");
    drain_macro_queue(&mut e);
    // world doesn't match "hello", message should say 0 subs
}

#[test]
fn test_at_colon_no_previous() {
    let mut e = engine_with("hello\n");
    // Clear last_ex_command
    e.last_ex_command = None;
    type_chars(&mut e, "@:");
    assert!(e.message.contains("No previous command"));
}

// ── Backtick text objects ───────────────────────────────────────────────

#[test]
fn test_backtick_inner_text_object() {
    let mut e = engine_with("let x = `hello`;\n");
    e.view_mut().cursor.col = 10; // inside backticks
    type_chars(&mut e, "di`");
    let content = buf(&e);
    assert!(content.starts_with("let x = ``"), "got: {}", content);
}

#[test]
fn test_backtick_around_text_object() {
    let mut e = engine_with("let x = `hello`;\n");
    e.view_mut().cursor.col = 10;
    type_chars(&mut e, "da`");
    let content = buf(&e);
    // da` deletes backticks + content + trailing space (Neovim-verified)
    assert!(content.starts_with("let x =;"), "got: {}", content);
}

// ── Insert CTRL-E (char below) ──────────────────────────────────────────

#[test]
fn test_insert_ctrl_e_char_below() {
    let mut e = engine_with("ab\nxy\n");
    type_chars(&mut e, "i"); // enter insert mode at (0,0)
    ctrl(&mut e, 'e'); // insert char from line below (x)
    let content = buf(&e);
    assert!(content.starts_with("xab"), "got: {}", content);
}

#[test]
fn test_insert_ctrl_e_no_line_below() {
    let mut e = engine_with("ab\n");
    type_chars(&mut e, "i");
    ctrl(&mut e, 'e'); // no line below, should do nothing
    assert_eq!(buf(&e), "ab\n");
}

// ── Insert CTRL-Y (char above) ──────────────────────────────────────────

#[test]
fn test_insert_ctrl_y_char_above() {
    let mut e = engine_with("ab\nxy\n");
    e.view_mut().cursor.line = 1;
    type_chars(&mut e, "i"); // enter insert mode at (1,0)
    ctrl(&mut e, 'y'); // insert char from line above (a)
    let content = buf(&e);
    let lines = content.lines().collect::<Vec<_>>();
    assert_eq!(lines[1], "axy", "got: {}", content);
}

// ── Visual r{char} ─────────────────────────────────────────────────────

#[test]
fn test_visual_replace_char() {
    let mut e = engine_with("hello\n");
    type_chars(&mut e, "viw"); // select "hello"
    type_chars(&mut e, "rx"); // replace all with 'x'
    assert_eq!(get_lines(&e)[0], "xxxxx");
}

#[test]
fn test_visual_line_replace_char() {
    let mut e = engine_with("hello\nworld\n");
    type_chars(&mut e, "Vjrx"); // select both lines, replace
    let lines = get_lines(&e);
    assert_eq!(lines[0], "xxxxx");
    assert_eq!(lines[1], "xxxxx");
}

// ── & (repeat last :s) ────────────────────────────────────────────────

#[test]
fn test_ampersand_repeat_substitute() {
    let mut e = engine_with("foo bar\nfoo baz\n");
    run_cmd(&mut e, "s/foo/XXX/");
    assert_eq!(get_lines(&e)[0], "XXX bar");
    e.view_mut().cursor.line = 1;
    type_chars(&mut e, "&");
    assert_eq!(get_lines(&e)[1], "XXX baz");
}

#[test]
fn test_ampersand_no_previous() {
    let mut e = engine_with("hello\n");
    type_chars(&mut e, "&");
    assert!(e.message.contains("No previous substitute"));
}

// ── CTRL-W q (quit window) ─────────────────────────────────────────────

#[test]
fn test_ctrl_w_q_close_window() {
    let mut e = engine_with("hello\n");
    // Split first
    exec(&mut e, "vsplit");
    let win_count_before = e.active_tab().layout.window_ids().len();
    assert!(win_count_before > 1);
    // CTRL-W q should close
    ctrl(&mut e, 'w');
    type_chars(&mut e, "q");
    let win_count_after = e.active_tab().layout.window_ids().len();
    assert_eq!(win_count_after, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 2 — Medium effort
// ═══════════════════════════════════════════════════════════════════════════

// ── CTRL-W +/- (resize height) ─────────────────────────────────────────

#[test]
fn test_ctrl_w_plus_resize() {
    let mut e = engine_with("hello\n");
    // Create a horizontal editor group split (Ctrl-W E) so +/- can adjust height
    ctrl(&mut e, 'w');
    press(&mut e, 'E');
    // Get initial ratio
    let initial = get_split_ratio(&e);
    assert!(initial.is_some(), "group split should exist");
    ctrl(&mut e, 'w');
    type_chars(&mut e, "+");
    // Ratio should have changed
    let after = get_split_ratio(&e);
    assert!(after.is_some(), "split should still exist");
    assert_ne!(initial, after, "ratio should change after Ctrl-W +");
}

#[test]
fn test_ctrl_w_minus_resize() {
    let mut e = engine_with("hello\n");
    // Horizontal group split for height resize
    ctrl(&mut e, 'w');
    press(&mut e, 'E');
    let initial = get_split_ratio(&e);
    assert!(initial.is_some());
    ctrl(&mut e, 'w');
    type_chars(&mut e, "-");
    let after = get_split_ratio(&e);
    assert!(after.is_some());
    assert_ne!(initial, after);
}

// ── CTRL-W = (equalize) ─────────────────────────────────────────────────

#[test]
fn test_ctrl_w_equal_equalize() {
    let mut e = engine_with("hello\n");
    ctrl(&mut e, 'w');
    type_chars(&mut e, "e");
    // Change ratio
    ctrl(&mut e, 'w');
    type_chars(&mut e, "+");
    ctrl(&mut e, 'w');
    type_chars(&mut e, "+");
    // Now equalize
    ctrl(&mut e, 'w');
    type_chars(&mut e, "=");
    let ratio = get_split_ratio(&e);
    assert_eq!(ratio, Some(0.5));
}

// ── [{ / ]} (unmatched braces) ──────────────────────────────────────────

#[test]
fn test_bracket_brace_forward() {
    let mut e = engine_with("if (x) {\n  foo();\n}\n");
    // Start inside the braces
    e.view_mut().cursor.line = 1;
    e.view_mut().cursor.col = 0;
    type_chars(&mut e, "]}");
    assert_cursor(&e, 2, 0); // should land on the '}'
}

#[test]
fn test_bracket_brace_backward() {
    let mut e = engine_with("if (x) {\n  foo();\n}\n");
    e.view_mut().cursor.line = 1;
    e.view_mut().cursor.col = 0;
    type_chars(&mut e, "[{");
    assert_cursor(&e, 0, 7); // should land on the '{'
}

// ── [( / ]) (unmatched parens) ──────────────────────────────────────────

#[test]
fn test_bracket_paren_forward() {
    let mut e = engine_with("fn(a, b, c)\n");
    e.view_mut().cursor.col = 4; // inside parens
    type_chars(&mut e, "])");
    assert_cursor(&e, 0, 10); // closing paren
}

#[test]
fn test_bracket_paren_backward() {
    let mut e = engine_with("fn(a, b, c)\n");
    e.view_mut().cursor.col = 4;
    type_chars(&mut e, "[(");
    assert_cursor(&e, 0, 2); // opening paren
}

// ── [[ / ]] (section navigation) ────────────────────────────────────────

#[test]
fn test_section_forward() {
    let mut e = engine_with("// comment\n{\n  code\n}\n{\n  more\n}\n");
    type_chars(&mut e, "]]");
    assert_cursor(&e, 1, 0); // first '{' in column 0
}

#[test]
fn test_section_forward_twice() {
    let mut e = engine_with("// comment\n{\n  code\n}\n{\n  more\n}\n");
    type_chars(&mut e, "2]]");
    assert_cursor(&e, 4, 0); // second '{' in column 0
}

#[test]
fn test_section_backward() {
    let mut e = engine_with("{\n  code\n}\n{\n  more\n}\n");
    e.view_mut().cursor.line = 4;
    type_chars(&mut e, "[[");
    assert_cursor(&e, 3, 0);
}

#[test]
fn test_section_end_forward() {
    let mut e = engine_with("// comment\n{\n  code\n}\n{\n  more\n}\n");
    type_chars(&mut e, "][");
    assert_cursor(&e, 3, 0); // first '}' in column 0
}

// ── [m / ]m (method navigation) ─────────────────────────────────────────

#[test]
fn test_method_start_forward() {
    let mut e = engine_with("fn main() {\n  let x = 1;\n}\n");
    type_chars(&mut e, "]m");
    assert_cursor(&e, 0, 10); // the '{' after main()
}

#[test]
fn test_method_start_backward() {
    let mut e = engine_with("fn main() {\n  let x = 1;\n}\n");
    e.view_mut().cursor.line = 2;
    type_chars(&mut e, "[m");
    assert_cursor(&e, 0, 10); // back to the '{'
}

#[test]
fn test_method_end_forward() {
    let mut e = engine_with("fn main() {\n  let x = 1;\n}\n");
    type_chars(&mut e, "]M");
    assert_cursor(&e, 2, 0); // the '}'
}

#[test]
fn test_method_end_backward() {
    let mut e = engine_with("fn main() {\n  let x = 1;\n}\nfn foo() {\n  bar();\n}\n");
    e.view_mut().cursor.line = 5;
    type_chars(&mut e, "[M");
    assert_cursor(&e, 2, 0); // the '}' of main
}

// ── Operator motions: d+, d-, d_, d| ────────────────────────────────────

#[test]
fn test_d_plus_operator() {
    let mut e = engine_with("line1\nline2\nline3\n");
    type_chars(&mut e, "d+");
    // Should delete current line + 1 line below (linewise)
    assert_eq!(get_lines(&e), vec!["line3"]);
}

#[test]
fn test_d_minus_operator() {
    let mut e = engine_with("line1\nline2\nline3\n");
    e.view_mut().cursor.line = 1;
    type_chars(&mut e, "d-");
    assert_eq!(get_lines(&e), vec!["line3"]);
}

#[test]
fn test_d_underscore_operator() {
    let mut e = engine_with("line1\nline2\nline3\n");
    type_chars(&mut e, "d_");
    // d_ = d1_ = delete current line only
    assert_eq!(get_lines(&e), vec!["line2", "line3"]);
}

#[test]
fn test_d_pipe_operator() {
    let mut e = engine_with("hello world\n");
    e.view_mut().cursor.col = 5;
    type_chars(&mut e, "d|");
    // d| with default count=1 → delete from col 5 to col 0 (charwise)
    assert_eq!(get_lines(&e)[0], " world");
}

// ── CTRL-W n (new window) ──────────────────────────────────────────────

#[test]
fn test_ctrl_w_n_new_window() {
    let mut e = engine_with("hello\n");
    let wins_before = e.active_tab().layout.window_ids().len();
    ctrl(&mut e, 'w');
    type_chars(&mut e, "n");
    let wins_after = e.active_tab().layout.window_ids().len();
    assert_eq!(wins_after, wins_before + 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Helper
// ═══════════════════════════════════════════════════════════════════════════

fn get_split_ratio(e: &vimcode_core::Engine) -> Option<f64> {
    use vimcode_core::core::window::GroupLayout;
    match &e.group_layout {
        GroupLayout::Split { ratio, .. } => Some(ratio.clone()),
        _ => None,
    }
}
