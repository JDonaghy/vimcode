mod common;
use common::*;

// ── zM: close all folds ─────────────────────────────────────────────────────

#[test]
fn test_zm_close_all_folds() {
    let mut e =
        engine_with("fn main() {\n    let x = 1;\n    let y = 2;\n}\nfn foo() {\n    bar();\n}\n");
    type_chars(&mut e, "zM");
    // Both functions should have folds
    assert!(e.view().fold_at(0).is_some(), "fold at line 0");
    assert!(e.view().fold_at(4).is_some(), "fold at line 4");
}

#[test]
fn test_zm_cursor_clamp() {
    let mut e = engine_with("fn main() {\n    let x = 1;\n    let y = 2;\n}\n");
    // Move cursor inside the fold body
    e.view_mut().cursor.line = 2;
    type_chars(&mut e, "zM");
    // Cursor should be moved to the fold header
    assert_eq!(e.cursor().line, 0);
}

// ── zA: toggle recursive ────────────────────────────────────────────────────

#[test]
fn test_za_big_toggle_recursive_open() {
    let mut e = engine_with("fn main() {\n    if true {\n        x();\n    }\n}\n");
    // Close all, then zA on outer should open all
    type_chars(&mut e, "zM");
    assert!(e.view().fold_at(0).is_some());
    type_chars(&mut e, "zA");
    // After zA on header, all nested folds should be opened
    assert!(e.view().fold_at(0).is_none());
}

#[test]
fn test_za_big_toggle_recursive_close() {
    let mut e = engine_with("fn main() {\n    let x = 1;\n}\n");
    // No fold exists, zA should create one (same as zc)
    type_chars(&mut e, "zA");
    assert!(e.view().fold_at(0).is_some());
}

// ── zO: open recursive ──────────────────────────────────────────────────────

#[test]
fn test_zo_big_open_recursive() {
    let mut e = engine_with("fn main() {\n    if true {\n        x();\n    }\n}\n");
    type_chars(&mut e, "zM");
    // zO on outer header should open all nested
    type_chars(&mut e, "zO");
    assert!(e.view().folds.is_empty(), "all folds should be opened");
}

// ── zC: close recursive ─────────────────────────────────────────────────────

#[test]
fn test_zc_big_close_recursive() {
    let mut e = engine_with("fn main() {\n    let x = 1;\n}\n");
    type_chars(&mut e, "zC");
    assert!(e.view().fold_at(0).is_some(), "fold should be created");
}

// ── zd: delete fold ─────────────────────────────────────────────────────────

#[test]
fn test_zd_delete_fold() {
    let mut e = engine_with("fn main() {\n    let x = 1;\n}\n");
    type_chars(&mut e, "zc"); // create fold
    assert!(e.view().fold_at(0).is_some());
    type_chars(&mut e, "zd"); // delete fold
    assert!(e.view().fold_at(0).is_none());
    // Lines should be visible again
    assert!(!e.view().is_line_hidden(1));
}

#[test]
fn test_zd_error_no_fold() {
    let mut e = engine_with("hello\nworld\n");
    type_chars(&mut e, "zd");
    assert_msg_contains(&e, "E490");
}

// ── zD: delete fold recursive ───────────────────────────────────────────────

#[test]
fn test_zd_big_delete_recursive() {
    let mut e = engine_with("fn main() {\n    if true {\n        x();\n    }\n}\n");
    type_chars(&mut e, "zM");
    // Should have folds at line 0 and nested at line 1
    type_chars(&mut e, "zD");
    // All folds starting within the outer fold range should be deleted
    assert!(e.view().folds.is_empty());
}

#[test]
fn test_zd_big_error_no_fold() {
    let mut e = engine_with("hello\nworld\n");
    type_chars(&mut e, "zD");
    assert_msg_contains(&e, "E490");
}

// ── zf{motion}: create fold ─────────────────────────────────────────────────

#[test]
fn test_zf_j_create_fold() {
    let mut e = engine_with("line 0\nline 1\nline 2\nline 3\n");
    type_chars(&mut e, "zfj");
    assert!(e.view().fold_at(0).is_some());
    let fold = e.view().fold_at(0).unwrap();
    assert_eq!(fold.end, 1);
    assert!(e.view().is_line_hidden(1));
}

#[test]
fn test_zf_2j_create_fold() {
    let mut e = engine_with("line 0\nline 1\nline 2\nline 3\n");
    type_chars(&mut e, "2zfj");
    // With count 2 on j, folds lines 0..=2
    assert!(e.view().fold_at(0).is_some());
    let fold = e.view().fold_at(0).unwrap();
    assert_eq!(fold.end, 2);
}

#[test]
fn test_zf_g_create_fold_to_end() {
    let mut e = engine_with("line 0\nline 1\nline 2\nline 3\n");
    type_chars(&mut e, "zfG");
    assert!(e.view().fold_at(0).is_some());
    let fold = e.view().fold_at(0).unwrap();
    // Should fold to last line
    assert!(fold.end >= 3);
}

#[test]
fn test_zf_gg_create_fold_to_start() {
    let mut e = engine_with("line 0\nline 1\nline 2\nline 3\n");
    e.view_mut().cursor.line = 3;
    type_chars(&mut e, "zfgg");
    // Should fold from line 0 to line 3
    assert!(e.view().fold_at(0).is_some());
    let fold = e.view().fold_at(0).unwrap();
    assert_eq!(fold.end, 3);
}

#[test]
fn test_zf_paragraph_create_fold() {
    let mut e = engine_with("line 0\nline 1\n\nline 3\nline 4\n");
    type_chars(&mut e, "zf}");
    // Fold from cursor to next paragraph boundary
    assert!(e.view().fold_at(0).is_some());
}

// ── zF: create fold for N lines ─────────────────────────────────────────────

#[test]
fn test_zf_big_create_fold_n_lines() {
    let mut e = engine_with("line 0\nline 1\nline 2\nline 3\n");
    type_chars(&mut e, "3zF");
    assert!(e.view().fold_at(0).is_some());
    let fold = e.view().fold_at(0).unwrap();
    assert_eq!(fold.end, 3);
    assert_msg_contains(&e, "3 lines folded");
}

// ── zv: open folds to make cursor visible ───────────────────────────────────

#[test]
fn test_zv_open_cursor_visible() {
    let mut e = engine_with("fn main() {\n    let x = 1;\n    let y = 2;\n}\n");
    type_chars(&mut e, "zc"); // close fold at line 0
                              // Move cursor to hidden line
    e.view_mut().cursor.line = 1;
    type_chars(&mut e, "zv");
    // Fold should be opened
    assert!(!e.view().is_line_hidden(1));
}

// ── zx: recompute folds ────────────────────────────────────────────────────

#[test]
fn test_zx_recompute() {
    let mut e = engine_with("fn main() {\n    let x = 1;\n}\nfn foo() {\n    bar();\n}\n");
    // Manually create a fold, then zx should reset
    e.view_mut().close_fold(0, 2);
    type_chars(&mut e, "zx");
    // After recompute, folds should be re-detected by indentation
    assert!(e.view().fold_at(0).is_some());
}

// ── zj/zk: fold navigation ─────────────────────────────────────────────────

#[test]
fn test_zj_move_to_next_fold() {
    let mut e = engine_with("top\nfn main() {\n    body;\n}\nbottom\n");
    // Cursor at line 0, zj should find next foldable line
    type_chars(&mut e, "zj");
    assert_eq!(e.cursor().line, 1, "should move to fn main (foldable)");
}

#[test]
fn test_zk_move_to_prev_fold() {
    let mut e = engine_with("fn main() {\n    body;\n}\nbottom\n");
    e.view_mut().cursor.line = 3;
    type_chars(&mut e, "zk");
    assert_eq!(e.cursor().line, 0, "should move back to fn main");
}

#[test]
fn test_zj_no_next_fold() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    let start_line = e.cursor().line;
    type_chars(&mut e, "zj");
    // No foldable content, cursor should stay
    assert_eq!(e.cursor().line, start_line);
}

// ── z<CR>: scroll top + first non-blank ─────────────────────────────────────

#[test]
fn test_z_cr_scroll_top_first_nonblank() {
    let mut e = engine_with("    hello\nworld\nfoo\n");
    e.view_mut().cursor.line = 0;
    e.view_mut().cursor.col = 0;
    press(&mut e, 'z');
    press_key(&mut e, "Return");
    assert_eq!(e.view().scroll_top, 0);
    assert_eq!(e.cursor().col, 4, "should be at first non-blank");
}

// ── z.: scroll center + first non-blank ─────────────────────────────────────

#[test]
fn test_z_dot_scroll_center_first_nonblank() {
    let mut e = engine_with("    hello\nworld\nfoo\n");
    e.view_mut().cursor.line = 0;
    e.view_mut().cursor.col = 0;
    type_chars(&mut e, "z.");
    assert_eq!(e.cursor().col, 4);
}

// ── z-: scroll bottom + first non-blank ─────────────────────────────────────

#[test]
fn test_z_minus_scroll_bottom_first_nonblank() {
    let mut e = engine_with("    hello\nworld\nfoo\n");
    e.view_mut().cursor.line = 0;
    e.view_mut().cursor.col = 0;
    type_chars(&mut e, "z-");
    assert_eq!(e.cursor().col, 4);
}

// ── zh/zl: horizontal scroll ────────────────────────────────────────────────

#[test]
fn test_zh_scroll_left() {
    let mut e = engine_with("hello world\n");
    e.view_mut().scroll_left = 5;
    type_chars(&mut e, "zh");
    assert_eq!(e.view().scroll_left, 4);
}

#[test]
fn test_zl_scroll_right() {
    let mut e = engine_with("hello world\n");
    type_chars(&mut e, "zl");
    assert_eq!(e.view().scroll_left, 1);
}

#[test]
fn test_zh_with_count() {
    let mut e = engine_with("hello world\n");
    e.view_mut().scroll_left = 10;
    type_chars(&mut e, "3zh");
    assert_eq!(e.view().scroll_left, 7);
}

#[test]
fn test_zl_with_count() {
    let mut e = engine_with("hello world\n");
    type_chars(&mut e, "5zl");
    assert_eq!(e.view().scroll_left, 5);
}

// ── zH/zL: half-screen horizontal scroll ────────────────────────────────────

#[test]
fn test_zh_big_scroll_half_left() {
    let mut e = engine_with("hello world\n");
    e.view_mut().scroll_left = 50;
    e.view_mut().viewport_cols = 80;
    type_chars(&mut e, "zH");
    assert_eq!(e.view().scroll_left, 10); // 50 - 40 = 10
}

#[test]
fn test_zl_big_scroll_half_right() {
    let mut e = engine_with("hello world\n");
    e.view_mut().viewport_cols = 80;
    type_chars(&mut e, "zL");
    assert_eq!(e.view().scroll_left, 40); // 0 + 40 = 40
}

// ── zR: open all folds (existing, verify) ───────────────────────────────────

#[test]
fn test_zr_big_open_all() {
    let mut e = engine_with("fn main() {\n    let x = 1;\n}\n");
    type_chars(&mut e, "zc"); // close fold
    assert!(!e.view().folds.is_empty());
    type_chars(&mut e, "zR"); // open all
    assert!(e.view().folds.is_empty());
}

// ── Combined: fold + cursor movement interaction ────────────────────────────

#[test]
fn test_fold_then_j_skips_hidden_lines() {
    let mut e = engine_with("fn main() {\n    let x = 1;\n    let y = 2;\n}\nafter\n");
    type_chars(&mut e, "zc"); // fold fn main body
    assert_eq!(e.cursor().line, 0);
    // j should skip hidden lines and land on "}"
    press(&mut e, 'j');
    // Cursor should land on a visible line after the fold
    assert!(!e.view().is_line_hidden(e.cursor().line));
}

#[test]
fn test_zf_k_create_fold_upward() {
    let mut e = engine_with("line 0\nline 1\nline 2\nline 3\n");
    e.view_mut().cursor.line = 2;
    type_chars(&mut e, "zfk");
    // Should fold from line 1 to line 2
    assert!(e.view().fold_at(1).is_some());
    let fold = e.view().fold_at(1).unwrap();
    assert_eq!(fold.end, 2);
}
