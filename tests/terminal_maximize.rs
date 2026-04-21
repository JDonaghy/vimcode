mod common;
use common::*;
use vimcode_core::EngineAction;

#[test]
fn maximize_sets_flag_and_grows_rows() {
    let mut e = engine_with("hello\n");
    let initial = e.session.terminal_panel_rows;
    assert!(!e.terminal_maximized);

    e.toggle_terminal_maximize(45);

    assert!(e.terminal_maximized);
    assert!(e.terminal_open);
    assert!(e.terminal_has_focus);
    assert_eq!(e.terminal_saved_rows, initial);
    assert!(e.session.terminal_panel_rows >= 45);
}

#[test]
fn unmaximize_restores_saved_rows() {
    let mut e = engine_with("hello\n");
    e.session.terminal_panel_rows = 14;

    e.toggle_terminal_maximize(40);
    assert!(e.terminal_maximized);
    assert!(e.session.terminal_panel_rows >= 40);

    e.toggle_terminal_maximize(40);
    assert!(!e.terminal_maximized);
    assert_eq!(e.session.terminal_panel_rows, 14);
}

#[test]
fn maximize_below_saved_rows_keeps_saved() {
    // If the target is smaller than the current rows, don't shrink —
    // maximize should only ever *grow* the panel.
    let mut e = engine_with("hello\n");
    e.session.terminal_panel_rows = 30;

    e.toggle_terminal_maximize(10);

    assert!(e.terminal_maximized);
    assert_eq!(e.session.terminal_panel_rows, 30);
    // Unmaximize should still restore the original (30) since that's
    // what was saved.
    e.toggle_terminal_maximize(10);
    assert!(!e.terminal_maximized);
    assert_eq!(e.session.terminal_panel_rows, 30);
}

#[test]
fn close_terminal_while_maximized_restores_saved_rows() {
    let mut e = engine_with("hello\n");
    e.session.terminal_panel_rows = 12;

    e.toggle_terminal_maximize(40);
    assert!(e.terminal_maximized);
    assert!(e.session.terminal_panel_rows >= 40);

    e.close_terminal();

    assert!(!e.terminal_maximized);
    assert!(!e.terminal_open);
    assert_eq!(e.session.terminal_panel_rows, 12);
}

#[test]
fn ex_command_terminal_maximize_returns_action() {
    let mut e = engine_with("hello\n");
    let act = exec(&mut e, "TerminalMaximize");
    assert_eq!(act, EngineAction::ToggleTerminalMaximize);

    let act = exec(&mut e, "TerminalMax");
    assert_eq!(act, EngineAction::ToggleTerminalMaximize);
}

#[test]
fn maximize_minimum_floor_is_five() {
    // Even if the backend passes a tiny target, the panel should not drop
    // below 5 content rows.
    let mut e = engine_with("hello\n");
    e.session.terminal_panel_rows = 3;

    e.toggle_terminal_maximize(1);

    assert!(e.terminal_maximized);
    assert!(e.session.terminal_panel_rows >= 5);
}
