mod common;
use common::*;
use vimcode_core::Mode;

/// Ensure engine is in Normal mode (workaround for user's vscode mode setting).
fn ensure_normal(e: &mut vimcode_core::Engine) {
    if e.mode != Mode::Normal {
        press_key(e, "Escape");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Feature 1: Indent Guides
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_indent_guides_default_on() {
    let e = engine_with("hello\n");
    assert!(e.settings.indent_guides);
}

#[test]
fn test_indent_guides_setting_toggle() {
    let mut e = engine_with("hello\n");
    ensure_normal(&mut e);
    run_cmd(&mut e, "set noindentguides");
    assert!(!e.settings.indent_guides);
    run_cmd(&mut e, "set indentguides");
    assert!(e.settings.indent_guides);
}

#[test]
fn test_indent_guides_setting_persists() {
    let e = engine_with("hello\n");
    assert_eq!(e.settings.get_value_str("indent_guides"), "true");
}

#[test]
fn test_indent_guides_set_value_str() {
    let mut e = engine_with("hello\n");
    e.settings.set_value_str("indent_guides", "false").unwrap();
    assert!(!e.settings.indent_guides);
    e.settings.set_value_str("indentguides", "true").unwrap();
    assert!(e.settings.indent_guides);
}

#[test]
fn test_indent_guides_query() {
    let mut e = engine_with("hello\n");
    let r = e.settings.parse_set_option("indentguides?");
    assert_eq!(r, Ok("indentguides".to_string()));
    e.settings.indent_guides = false;
    let r = e.settings.parse_set_option("indentguides?");
    assert_eq!(r, Ok("noindentguides".to_string()));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Feature 2: Bracket Pair Highlighting
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_bracket_match_on_open_paren() {
    let mut e = engine_with("(hello)\n");
    ensure_normal(&mut e);
    // Cursor at (0,0) = '('
    // Trigger update by pressing any key
    press(&mut e, 'l');
    press(&mut e, 'h');
    // Match should point to ')' at col 6
    assert_eq!(e.bracket_match, Some((0, 6)));
}

#[test]
fn test_bracket_match_on_close_paren() {
    let mut e = engine_with("(hello)\n");
    ensure_normal(&mut e);
    // Move to end — ')'
    for _ in 0..6 {
        press(&mut e, 'l');
    }
    assert_cursor(&e, 0, 6);
    assert_eq!(e.bracket_match, Some((0, 0)));
}

#[test]
fn test_bracket_match_not_on_bracket() {
    let mut e = engine_with("hello\n");
    ensure_normal(&mut e);
    press(&mut e, 'l');
    assert_eq!(e.bracket_match, None);
}

#[test]
fn test_bracket_match_nested() {
    let mut e = engine_with("((()))\n");
    ensure_normal(&mut e);
    // Outer '(' at col 0
    press(&mut e, 'l');
    press(&mut e, 'h');
    assert_eq!(e.bracket_match, Some((0, 5)));
    // Inner '(' at col 1
    press(&mut e, 'l');
    assert_eq!(e.bracket_match, Some((0, 4)));
    // Innermost '(' at col 2
    press(&mut e, 'l');
    assert_eq!(e.bracket_match, Some((0, 3)));
}

#[test]
fn test_bracket_match_unbalanced() {
    let mut e = engine_with("(hello\n");
    ensure_normal(&mut e);
    press(&mut e, 'l');
    press(&mut e, 'h');
    assert_eq!(e.bracket_match, None);
}

#[test]
fn test_bracket_match_disabled() {
    let mut e = engine_with("(hello)\n");
    e.settings.match_brackets = false;
    ensure_normal(&mut e);
    press(&mut e, 'l');
    press(&mut e, 'h');
    assert_eq!(e.bracket_match, None);
}

#[test]
fn test_bracket_match_curly() {
    let mut e = engine_with("{foo}\n");
    ensure_normal(&mut e);
    press(&mut e, 'l');
    press(&mut e, 'h');
    assert_eq!(e.bracket_match, Some((0, 4)));
}

#[test]
fn test_bracket_match_square() {
    let mut e = engine_with("[bar]\n");
    ensure_normal(&mut e);
    press(&mut e, 'l');
    press(&mut e, 'h');
    assert_eq!(e.bracket_match, Some((0, 4)));
}

#[test]
fn test_bracket_match_setting_toggle() {
    let mut e = engine_with("hello\n");
    ensure_normal(&mut e);
    assert!(e.settings.match_brackets);
    run_cmd(&mut e, "set nomatchbrackets");
    assert!(!e.settings.match_brackets);
    run_cmd(&mut e, "set matchbrackets");
    assert!(e.settings.match_brackets);
}

#[test]
fn test_bracket_match_query() {
    let mut e = engine_with("hello\n");
    let r = e.settings.parse_set_option("matchbrackets?");
    assert_eq!(r, Ok("matchbrackets".to_string()));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Feature 3: Auto-close Brackets/Quotes
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_auto_pair_paren() {
    let mut e = engine_with("");
    ensure_normal(&mut e);
    set_content(&mut e, "\n");
    press(&mut e, 'i'); // enter insert mode
    press(&mut e, '(');
    assert_eq!(buf(&e), "()\n");
    assert_cursor(&e, 0, 1);
}

#[test]
fn test_auto_pair_bracket() {
    let mut e = engine_with("");
    ensure_normal(&mut e);
    set_content(&mut e, "\n");
    press(&mut e, 'i');
    press(&mut e, '[');
    assert_eq!(buf(&e), "[]\n");
    assert_cursor(&e, 0, 1);
}

#[test]
fn test_auto_pair_curly() {
    let mut e = engine_with("");
    ensure_normal(&mut e);
    set_content(&mut e, "\n");
    press(&mut e, 'i');
    press(&mut e, '{');
    assert_eq!(buf(&e), "{}\n");
    assert_cursor(&e, 0, 1);
}

#[test]
fn test_auto_pair_skip_over_closing() {
    let mut e = engine_with("");
    ensure_normal(&mut e);
    set_content(&mut e, "\n");
    press(&mut e, 'i');
    press(&mut e, '(');
    // Buffer: "()", cursor at col 1
    press(&mut e, ')');
    // Should skip over ')' rather than inserting another
    assert_eq!(buf(&e), "()\n");
    assert_cursor(&e, 0, 2);
}

#[test]
fn test_auto_pair_backspace_deletes_both() {
    let mut e = engine_with("");
    ensure_normal(&mut e);
    set_content(&mut e, "\n");
    press(&mut e, 'i');
    press(&mut e, '(');
    assert_eq!(buf(&e), "()\n");
    // Backspace should delete both ( and )
    press_key(&mut e, "BackSpace");
    assert_eq!(buf(&e), "\n");
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_auto_pair_quote_after_space() {
    let mut e = engine_with("");
    ensure_normal(&mut e);
    set_content(&mut e, " \n");
    // Go to end of line and enter insert
    press(&mut e, 'A');
    press(&mut e, '"');
    assert_eq!(buf(&e), " \"\"\n");
}

#[test]
fn test_auto_pair_quote_after_letter_no_pair() {
    let mut e = engine_with("");
    ensure_normal(&mut e);
    set_content(&mut e, "abc\n");
    press(&mut e, 'A'); // append at end
    press(&mut e, '"');
    // After 'c' → no auto-pair for quotes
    assert_eq!(buf(&e), "abc\"\n");
}

#[test]
fn test_auto_pair_disabled() {
    let mut e = engine_with("");
    ensure_normal(&mut e);
    e.settings.auto_pairs = false;
    set_content(&mut e, "\n");
    press(&mut e, 'i');
    press(&mut e, '(');
    assert_eq!(buf(&e), "(\n");
}

#[test]
fn test_auto_pair_setting_toggle() {
    let mut e = engine_with("hello\n");
    ensure_normal(&mut e);
    assert!(e.settings.auto_pairs);
    run_cmd(&mut e, "set noautopairs");
    assert!(!e.settings.auto_pairs);
    run_cmd(&mut e, "set autopairs");
    assert!(e.settings.auto_pairs);
}

#[test]
fn test_auto_pair_multiple_pairs() {
    let mut e = engine_with("");
    ensure_normal(&mut e);
    set_content(&mut e, "\n");
    press(&mut e, 'i');
    press(&mut e, '(');
    press(&mut e, '[');
    assert_eq!(buf(&e), "([])\n");
    assert_cursor(&e, 0, 2);
}

#[test]
fn test_auto_pair_normal_mode_unaffected() {
    let mut e = engine_with("hello\n");
    ensure_normal(&mut e);
    assert_eq!(e.mode, Mode::Normal);
    press(&mut e, 'l');
    assert_cursor(&e, 0, 1);
}

#[test]
fn test_auto_pair_backtick() {
    let mut e = engine_with("");
    ensure_normal(&mut e);
    set_content(&mut e, " \n");
    press(&mut e, 'A');
    press(&mut e, '`');
    assert_eq!(buf(&e), " ``\n");
}

#[test]
fn test_auto_pair_single_quote_after_bracket() {
    let mut e = engine_with("");
    ensure_normal(&mut e);
    set_content(&mut e, "(\n");
    press(&mut e, 'l'); // cursor on \n? no, l stays on (
    press(&mut e, 'A'); // append after (
    press(&mut e, '\'');
    assert_eq!(buf(&e), "(''\n");
}

#[test]
fn test_auto_pair_query() {
    let mut e = engine_with("hello\n");
    let r = e.settings.parse_set_option("autopairs?");
    assert_eq!(r, Ok("autopairs".to_string()));
}

// ── VSCode mode auto-pairs ──────────────────────────────────────────────────

#[test]
fn test_auto_pair_vscode_mode_paren() {
    let mut e = engine_with("");
    e.toggle_editor_mode(); // switch to vscode mode (Insert)
    set_content(&mut e, "\n");
    e.view_mut().cursor = vimcode_core::Cursor { line: 0, col: 0 };
    press(&mut e, '(');
    assert_eq!(buf(&e), "()\n");
    assert_cursor(&e, 0, 1);
}

#[test]
fn test_auto_pair_vscode_mode_skip_over() {
    let mut e = engine_with("");
    e.toggle_editor_mode(); // switch to vscode mode (Insert)
    set_content(&mut e, "\n");
    e.view_mut().cursor = vimcode_core::Cursor { line: 0, col: 0 };
    press(&mut e, '(');
    press(&mut e, ')');
    assert_eq!(buf(&e), "()\n");
    assert_cursor(&e, 0, 2);
}

#[test]
fn test_auto_pair_vscode_mode_backspace() {
    let mut e = engine_with("");
    e.toggle_editor_mode(); // switch to vscode mode (Insert)
    set_content(&mut e, "\n");
    e.view_mut().cursor = vimcode_core::Cursor { line: 0, col: 0 };
    press(&mut e, '(');
    press_key(&mut e, "BackSpace");
    assert_eq!(buf(&e), "\n");
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_auto_pair_vscode_mode_quote_smart_context() {
    let mut e = engine_with("");
    e.toggle_editor_mode(); // switch to vscode mode (Insert)
    set_content(&mut e, "a\n");
    e.view_mut().cursor = vimcode_core::Cursor { line: 0, col: 1 };
    press(&mut e, '"');
    // After letter — no auto-pair
    assert_eq!(buf(&e), "a\"\n");
}
