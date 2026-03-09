mod common;
use common::*;

// ── Helper: set language ID on the active buffer ─────────────────────────────

fn set_lang(e: &mut vimcode_core::Engine, lang: &str) {
    let buf_id = e.active_buffer_id();
    if let Some(state) = e.buffer_manager.get_mut(buf_id) {
        state.lsp_language_id = Some(lang.to_string());
    }
}

// ── gcc: single line toggle ──────────────────────────────────────────────────

#[test]
fn gcc_comments_rust_line() {
    let mut e = engine_with("let x = 1;\n");
    set_lang(&mut e, "rust");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "// let x = 1;\n");
}

#[test]
fn gcc_uncomments_rust_line() {
    let mut e = engine_with("// let x = 1;\n");
    set_lang(&mut e, "rust");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "let x = 1;\n");
}

#[test]
fn gcc_comments_python_line() {
    let mut e = engine_with("x = 1\n");
    set_lang(&mut e, "python");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "# x = 1\n");
}

#[test]
fn gcc_comments_lua_line() {
    let mut e = engine_with("local x = 1\n");
    set_lang(&mut e, "lua");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "-- local x = 1\n");
}

#[test]
fn gcc_comments_go_line() {
    let mut e = engine_with("x := 1\n");
    set_lang(&mut e, "go");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "// x := 1\n");
}

// ── gcc with count ───────────────────────────────────────────────────────────

#[test]
fn gcc_with_count_comments_multiple_lines() {
    let mut e = engine_with("a\nb\nc\nd\n");
    set_lang(&mut e, "rust");
    type_chars(&mut e, "3gcc");
    assert_eq!(get_lines(&e), vec!["// a", "// b", "// c", "d"]);
}

#[test]
fn gcc_with_count_uncomments_when_all_commented() {
    let mut e = engine_with("// a\n// b\n// c\nd\n");
    set_lang(&mut e, "rust");
    type_chars(&mut e, "3gcc");
    assert_eq!(get_lines(&e), vec!["a", "b", "c", "d"]);
}

// ── Visual gc ────────────────────────────────────────────────────────────────

#[test]
fn visual_gc_comments_selected_lines() {
    let mut e = engine_with("line1\nline2\nline3\nline4\n");
    set_lang(&mut e, "rust");
    // Select lines 1-3 (0-indexed), then gc
    press(&mut e, 'V'); // visual line
    press(&mut e, 'j');
    press(&mut e, 'j');
    type_chars(&mut e, "gc");
    assert_eq!(
        get_lines(&e),
        vec!["// line1", "// line2", "// line3", "line4"]
    );
}

#[test]
fn visual_gc_uncomments_when_all_commented() {
    let mut e = engine_with("// line1\n// line2\nline3\n");
    set_lang(&mut e, "rust");
    press(&mut e, 'V');
    press(&mut e, 'j');
    type_chars(&mut e, "gc");
    assert_eq!(get_lines(&e), vec!["line1", "line2", "line3"]);
}

// ── :Comment command ─────────────────────────────────────────────────────────

#[test]
fn comment_command_single_line() {
    let mut e = engine_with("hello\n");
    set_lang(&mut e, "python");
    exec(&mut e, "Comment");
    assert_buf(&e, "# hello\n");
}

#[test]
fn comment_command_with_count() {
    let mut e = engine_with("a\nb\nc\n");
    set_lang(&mut e, "python");
    exec(&mut e, "Comment 2");
    assert_eq!(get_lines(&e), vec!["# a", "# b", "c"]);
}

#[test]
fn commentary_alias_still_works() {
    let mut e = engine_with("hello\n");
    set_lang(&mut e, "python");
    exec(&mut e, "Commentary");
    assert_buf(&e, "# hello\n");
}

// ── Block comments ───────────────────────────────────────────────────────────

#[test]
fn gcc_html_block_comment() {
    let mut e = engine_with("<div>\n");
    set_lang(&mut e, "html");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "<!-- <div> -->\n");
}

#[test]
fn gcc_html_block_uncomment() {
    let mut e = engine_with("<!-- <div> -->\n");
    set_lang(&mut e, "html");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "<div>\n");
}

#[test]
fn gcc_css_block_comment() {
    let mut e = engine_with("color: red;\n");
    set_lang(&mut e, "css");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "/* color: red; */\n");
}

#[test]
fn gcc_css_block_uncomment() {
    let mut e = engine_with("/* color: red; */\n");
    set_lang(&mut e, "css");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "color: red;\n");
}

// ── Blank line handling ──────────────────────────────────────────────────────

#[test]
fn gcc_skips_blank_lines_in_range() {
    let mut e = engine_with("a\n\nb\n");
    set_lang(&mut e, "rust");
    type_chars(&mut e, "3gcc");
    assert_eq!(get_lines(&e), vec!["// a", "", "// b"]);
}

#[test]
fn gcc_all_blank_does_nothing() {
    let mut e = engine_with("\n\n\n");
    set_lang(&mut e, "rust");
    type_chars(&mut e, "gcc");
    // Buffer should remain unchanged
    assert_buf(&e, "\n\n\n");
}

// ── Indent preservation ──────────────────────────────────────────────────────

#[test]
fn gcc_preserves_indent() {
    let mut e = engine_with("    let x = 1;\n");
    set_lang(&mut e, "rust");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "    // let x = 1;\n");
}

#[test]
fn gcc_uncomment_preserves_indent() {
    let mut e = engine_with("    // let x = 1;\n");
    set_lang(&mut e, "rust");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "    let x = 1;\n");
}

// ── Undo after toggle ────────────────────────────────────────────────────────

#[test]
fn gcc_then_undo_restores_original() {
    let mut e = engine_with("let x = 1;\n");
    set_lang(&mut e, "rust");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "// let x = 1;\n");
    press(&mut e, 'u');
    assert_buf(&e, "let x = 1;\n");
}

#[test]
fn visual_gc_then_undo_restores_original() {
    let mut e = engine_with("a\nb\nc\n");
    set_lang(&mut e, "rust");
    press(&mut e, 'V');
    press(&mut e, 'j');
    type_chars(&mut e, "gc");
    assert_eq!(get_lines(&e), vec!["// a", "// b", "c"]);
    press(&mut e, 'u');
    assert_eq!(get_lines(&e), vec!["a", "b", "c"]);
}

// ── Fallback for unknown language ────────────────────────────────────────────

#[test]
fn gcc_unknown_language_falls_back_to_hash() {
    let mut e = engine_with("hello\n");
    set_lang(&mut e, "brainfuck");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "# hello\n");
}

#[test]
fn gcc_no_language_id_falls_back_to_hash() {
    let mut e = engine_with("hello\n");
    // No language set at all
    type_chars(&mut e, "gcc");
    assert_buf(&e, "# hello\n");
}

// ── Mixed comment detection ──────────────────────────────────────────────────

#[test]
fn visual_gc_mixed_adds_comments_to_all() {
    let mut e = engine_with("// a\nb\n// c\n");
    set_lang(&mut e, "rust");
    press(&mut e, 'V');
    press(&mut e, 'j');
    press(&mut e, 'j');
    type_chars(&mut e, "gc");
    // Not all commented, so add comments to all
    assert_eq!(get_lines(&e), vec!["// // a", "// b", "// // c"]);
}

// ── Comment style override ───────────────────────────────────────────────────

#[test]
fn comment_override_changes_style() {
    let mut e = engine_with("hello\n");
    set_lang(&mut e, "rust");
    // Override Rust to use # instead of //
    e.comment_overrides.insert(
        "rust".to_string(),
        vimcode_core::core::comment::CommentStyleOwned {
            line: "#".to_string(),
            block_open: String::new(),
            block_close: String::new(),
        },
    );
    type_chars(&mut e, "gcc");
    assert_buf(&e, "# hello\n");
}

// ── More language coverage ───────────────────────────────────────────────────

#[test]
fn gcc_comments_vim_line() {
    let mut e = engine_with("set number\n");
    set_lang(&mut e, "vim");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "\" set number\n");
}

#[test]
fn gcc_comments_tex_line() {
    let mut e = engine_with("\\begin{document}\n");
    set_lang(&mut e, "tex");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "% \\begin{document}\n");
}

#[test]
fn gcc_comments_haskell_line() {
    let mut e = engine_with("main = putStrLn\n");
    set_lang(&mut e, "haskell");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "-- main = putStrLn\n");
}

#[test]
fn gcc_comments_lisp_line() {
    let mut e = engine_with("(defun foo ())\n");
    set_lang(&mut e, "lisp");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "; (defun foo ())\n");
}

// ── Uncomment without space after prefix ─────────────────────────────────────

#[test]
fn gcc_uncomments_without_space() {
    let mut e = engine_with("//no space\n");
    set_lang(&mut e, "rust");
    type_chars(&mut e, "gcc");
    assert_buf(&e, "no space\n");
}
