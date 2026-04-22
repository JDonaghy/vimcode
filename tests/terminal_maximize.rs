mod common;
use common::*;
use vimcode_core::EngineAction;

#[test]
fn maximize_sets_flag_and_opens_terminal() {
    let mut e = engine_with("hello\n");
    assert!(!e.terminal_maximized);
    assert!(!e.terminal_open);

    e.toggle_terminal_maximize();

    assert!(e.terminal_maximized);
    assert!(e.terminal_open);
    assert!(e.terminal_has_focus);
}

#[test]
fn maximize_does_not_mutate_stored_rows() {
    // The stored user preference must not be touched — the effective
    // height comes from the backend-supplied target at render time.
    let mut e = engine_with("hello\n");
    e.session.terminal_panel_rows = 14;

    e.toggle_terminal_maximize();
    assert!(e.terminal_maximized);
    assert_eq!(e.session.terminal_panel_rows, 14);

    e.toggle_terminal_maximize();
    assert!(!e.terminal_maximized);
    assert_eq!(e.session.terminal_panel_rows, 14);
}

#[test]
fn effective_rows_returns_target_when_maximized() {
    let mut e = engine_with("hello\n");
    e.session.terminal_panel_rows = 12;

    assert_eq!(e.effective_terminal_panel_rows(40), 12);

    e.toggle_terminal_maximize();
    assert_eq!(e.effective_terminal_panel_rows(40), 40);
    // Target smaller than stored keeps stored (monotone-grow invariant
    // preserves user's hand-dragged size on small viewports).
    assert_eq!(e.effective_terminal_panel_rows(8), 12);
}

#[test]
fn effective_rows_floor_is_five() {
    let mut e = engine_with("hello\n");
    e.session.terminal_panel_rows = 3;
    e.toggle_terminal_maximize();
    assert_eq!(e.effective_terminal_panel_rows(2), 5);
}

#[test]
fn close_terminal_clears_maximize_flag() {
    let mut e = engine_with("hello\n");
    e.toggle_terminal_maximize();
    assert!(e.terminal_maximized);

    e.close_terminal();

    assert!(!e.terminal_maximized);
    assert!(!e.terminal_open);
}

#[test]
fn ex_command_terminal_maximize_returns_action() {
    let mut e = engine_with("hello\n");
    let act = exec(&mut e, "TerminalMaximize");
    assert_eq!(act, EngineAction::ToggleTerminalMaximize);

    let act = exec(&mut e, "TerminalMax");
    assert_eq!(act, EngineAction::ToggleTerminalMaximize);
}
