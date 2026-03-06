mod common;
use common::*;
use vimcode_core::Mode;

// ── Normal mode % ────────────────────────────────────────────────────────────

#[test]
fn test_percent_paren_forward() {
    let mut e = engine_with("(hello)\n");
    // Cursor on '(' -> jump to ')'
    press(&mut e, '%');
    assert_cursor(&e, 0, 6);
}

#[test]
fn test_percent_paren_backward() {
    let mut e = engine_with("(hello)\n");
    // Move cursor to ')' then jump back to '('
    for _ in 0..6 {
        press(&mut e, 'l');
    }
    assert_cursor(&e, 0, 6);
    press(&mut e, '%');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_percent_curly_forward() {
    let mut e = engine_with("{foo}\n");
    press(&mut e, '%');
    assert_cursor(&e, 0, 4);
}

#[test]
fn test_percent_curly_backward() {
    let mut e = engine_with("{foo}\n");
    for _ in 0..4 {
        press(&mut e, 'l');
    }
    press(&mut e, '%');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_percent_square_forward() {
    let mut e = engine_with("[bar]\n");
    press(&mut e, '%');
    assert_cursor(&e, 0, 4);
}

#[test]
fn test_percent_square_backward() {
    let mut e = engine_with("[bar]\n");
    for _ in 0..4 {
        press(&mut e, 'l');
    }
    press(&mut e, '%');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_percent_nested_parens() {
    let mut e = engine_with("((()))\n");
    // From outer '(' to outer ')'
    press(&mut e, '%');
    assert_cursor(&e, 0, 5);
    // And back
    press(&mut e, '%');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_percent_nested_inner() {
    let mut e = engine_with("((()))\n");
    // Move to inner '(' at col 1
    press(&mut e, 'l');
    press(&mut e, '%');
    assert_cursor(&e, 0, 4); // inner ')'
}

#[test]
fn test_percent_forward_search_not_on_bracket() {
    // When not on a bracket, % searches forward for the next bracket on the line
    let mut e = engine_with("hello (world)\n");
    press(&mut e, '%');
    // Should find '(' at col 6, then jump to ')' at col 12
    assert_cursor(&e, 0, 12);
}

#[test]
fn test_percent_no_bracket_on_line() {
    let mut e = engine_with("hello world\n");
    press(&mut e, '%');
    // No bracket on line, cursor should not move
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_percent_cross_line() {
    let mut e = engine_with("(\nhello\n)\n");
    // '(' on line 0, ')' on line 2
    press(&mut e, '%');
    assert_cursor(&e, 2, 0);
    // And back
    press(&mut e, '%');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_percent_mixed_bracket_types() {
    let mut e = engine_with("({[]})\n");
    // From '(' to ')' — outermost
    press(&mut e, '%');
    assert_cursor(&e, 0, 5);
    // Go back, move to '{' at col 1
    press(&mut e, '%');
    assert_cursor(&e, 0, 0);
    press(&mut e, 'l');
    press(&mut e, '%');
    assert_cursor(&e, 0, 4); // '}'
                             // Move to '[' at col 2
    press(&mut e, '%');
    assert_cursor(&e, 0, 1); // back to '{'
    press(&mut e, 'l');
    press(&mut e, '%');
    assert_cursor(&e, 0, 3); // ']'
}

#[test]
fn test_percent_cursor_at_end_of_file() {
    let mut e = engine_with("hello");
    // Move to end
    press(&mut e, '$');
    press(&mut e, '%');
    // Should not crash, cursor stays
    assert_cursor(&e, 0, 4);
}

#[test]
fn test_percent_unmatched_bracket() {
    let mut e = engine_with("(hello\n");
    // No matching ')' — cursor should stay on '('
    press(&mut e, '%');
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_percent_empty_parens() {
    let mut e = engine_with("()\n");
    press(&mut e, '%');
    assert_cursor(&e, 0, 1);
    press(&mut e, '%');
    assert_cursor(&e, 0, 0);
}

// ── Operator motions with % ─────────────────────────────────────────────────

#[test]
fn test_d_percent_from_open() {
    let mut e = engine_with("(hello)\n");
    // d% from '(' deletes inclusive "(hello)"
    press(&mut e, 'd');
    press(&mut e, '%');
    assert_buf(&e, "\n");
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_d_percent_from_close() {
    let mut e = engine_with("(hello)\n");
    // Move to ')'
    for _ in 0..6 {
        press(&mut e, 'l');
    }
    press(&mut e, 'd');
    press(&mut e, '%');
    assert_buf(&e, "\n");
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_d_percent_curly() {
    let mut e = engine_with("before{inside}after\n");
    // Move to '{'
    for _ in 0..6 {
        press(&mut e, 'l');
    }
    assert_cursor(&e, 0, 6);
    press(&mut e, 'd');
    press(&mut e, '%');
    assert_buf(&e, "beforeafter\n");
}

#[test]
fn test_y_percent() {
    let mut e = engine_with("(abc)\n");
    press(&mut e, 'y');
    press(&mut e, '%');
    // Yank should capture "(abc)" — buffer unchanged
    assert_buf(&e, "(abc)\n");
    assert_register(&e, '"', "(abc)", false);
}

#[test]
fn test_c_percent() {
    let mut e = engine_with("(abc)rest\n");
    press(&mut e, 'c');
    press(&mut e, '%');
    assert_mode(&e, Mode::Insert);
    assert_buf(&e, "rest\n");
}

#[test]
fn test_d_percent_not_on_bracket() {
    let mut e = engine_with("hello world\n");
    press(&mut e, 'd');
    press(&mut e, '%');
    // No bracket — no-op
    assert_buf(&e, "hello world\n");
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_d_percent_nested() {
    let mut e = engine_with("((inner))\n");
    press(&mut e, 'd');
    press(&mut e, '%');
    assert_buf(&e, "\n");
}

#[test]
fn test_d_percent_cross_line() {
    let mut e = engine_with("start(\nmiddle\n)end\n");
    // Move to '(' at col 5
    for _ in 0..5 {
        press(&mut e, 'l');
    }
    press(&mut e, 'd');
    press(&mut e, '%');
    assert_buf(&e, "startend\n");
}

// ── Visual mode % ───────────────────────────────────────────────────────────

#[test]
fn test_v_percent_select_to_matching() {
    let mut e = engine_with("(hello)\n");
    // Enter visual mode, then %
    press(&mut e, 'v');
    assert_mode(&e, Mode::Visual);
    press(&mut e, '%');
    // Cursor should be on ')'
    assert_cursor(&e, 0, 6);
    // Delete selection to verify it covers "(hello)"
    press(&mut e, 'd');
    assert_buf(&e, "\n");
}

#[test]
fn test_v_percent_from_close() {
    let mut e = engine_with("(hello)\n");
    // Move to ')', enter visual, press %
    for _ in 0..6 {
        press(&mut e, 'l');
    }
    press(&mut e, 'v');
    press(&mut e, '%');
    assert_cursor(&e, 0, 0);
    // Delete selection
    press(&mut e, 'd');
    assert_buf(&e, "\n");
}

#[test]
fn test_v_percent_nested() {
    let mut e = engine_with("((inner))\n");
    // Start at outer '(', enter visual, press %
    press(&mut e, 'v');
    press(&mut e, '%');
    assert_cursor(&e, 0, 8);
    // Yank selection
    press(&mut e, 'y');
    assert_register(&e, '"', "((inner))", false);
}

#[test]
fn test_v_percent_cross_line() {
    let mut e = engine_with("{\n  body\n}\n");
    press(&mut e, 'v');
    press(&mut e, '%');
    assert_cursor(&e, 2, 0);
    // Delete to verify selection spans all lines
    press(&mut e, 'd');
    assert_buf(&e, "\n");
}

#[test]
fn test_v_percent_no_bracket() {
    let mut e = engine_with("hello\n");
    press(&mut e, 'v');
    press(&mut e, '%');
    // No bracket — cursor stays, still in visual mode
    assert_mode(&e, Mode::Visual);
    assert_cursor(&e, 0, 0);
}

#[test]
fn test_v_percent_forward_search() {
    let mut e = engine_with("abc (def)\n");
    // Cursor at col 0, not on bracket
    press(&mut e, 'v');
    press(&mut e, '%');
    // Should search forward, find '(' at col 4, jump to ')' at col 8
    assert_cursor(&e, 0, 8);
    press(&mut e, 'y');
    assert_register(&e, '"', "abc (def)", false);
}

#[test]
fn test_visual_line_percent() {
    let mut e = engine_with("(\nhello\n)\n");
    // Enter visual line mode, press %
    press(&mut e, 'V');
    assert_mode(&e, Mode::VisualLine);
    press(&mut e, '%');
    assert_cursor(&e, 2, 0);
    // Yank — should get all 3 lines
    press(&mut e, 'y');
    assert_register(&e, '"', "(\nhello\n)\n", true);
}
