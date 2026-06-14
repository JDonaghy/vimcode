mod common;
use common::*;

/// #514 stress-test regression: a mouse wheel must not leak into the shell's
/// scrollback while an app owns the alternate screen.
///
/// `terminal_forward_wheel` returns `true` when the wheel was handed to the
/// child (so the caller skips local scrollback) and `false` for an ordinary
/// shell (so the caller scrolls scrollback as before). This mirrors quadraui's
/// `TerminalSession::should_forward_wheel()` gate end-to-end through the engine.
#[test]
#[cfg(unix)]
fn wheel_forwarded_on_alt_screen_not_on_primary() {
    // Pin the shell to /bin/sh so the spawn + alt-screen escape are
    // deterministic regardless of the host's $SHELL.
    std::env::set_var("SHELL", "/bin/sh");

    let mut e = engine_with("hello\n");
    e.terminal_new_tab(80, 10);
    assert!(e.active_terminal().is_some(), "terminal failed to spawn");

    // Fresh shell: primary screen, no mouse reporting → the wheel must fall
    // back to local scrollback (NOT forwarded).
    assert!(
        !e.terminal_forward_wheel(true),
        "primary-screen wheel-up must fall back to scrollback"
    );

    // Drive the child onto the alternate screen.
    e.terminal_write(b"printf '\\033[?1049h'\n");
    let mut on_alt = false;
    for _ in 0..500 {
        e.poll_terminal();
        if e.active_terminal().is_some_and(|t| t.on_alt_screen()) {
            on_alt = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(on_alt, "child did not enter the alternate screen");

    // On the alt-screen the wheel is forwarded to the child in BOTH directions
    // and must NOT scroll local scrollback.
    assert!(
        e.terminal_forward_wheel(true),
        "alt-screen wheel-up must be forwarded to the child"
    );
    assert!(
        e.terminal_forward_wheel(false),
        "alt-screen wheel-down must be forwarded to the child"
    );

    // Leave the alt-screen and shut the shell down.
    e.terminal_write(b"printf '\\033[?1049l'\n");
    e.terminal_write(b"exit\n");
}

/// No terminal pane → nothing to forward → caller scrolls scrollback.
#[test]
fn wheel_no_pane_falls_back_to_scrollback() {
    let mut e = engine_with("hello\n");
    assert!(!e.terminal_forward_wheel(true));
    assert!(!e.terminal_forward_wheel(false));
}

// ── #533: unified scroll / mouse handler tests ────────────────────────────────

/// `handle_terminal_scroll` with a negative delta scrolls up (into history).
///
/// Without a live PTY or scrollback content the offset stays at 0 (nothing
/// to scroll past), but the call must not panic and the terminal must still
/// be alive.
#[test]
#[cfg(unix)]
fn handle_terminal_scroll_negative_is_up() {
    std::env::set_var("SHELL", "/bin/sh");
    let mut e = engine_with("hello\n");
    e.terminal_new_tab(80, 10);
    assert!(e.active_terminal().is_some());

    // Negative delta → scroll up (into history).  With an empty scrollback
    // this is a no-op, but the call must not panic.
    e.handle_terminal_scroll(-1.0);
    // Positive delta → scroll down (toward live).
    e.handle_terminal_scroll(1.0);

    e.terminal_write(b"exit\n");
}

/// `handle_terminal_scroll` with delta = 0.0 is a no-op.
#[test]
fn handle_terminal_scroll_zero_is_noop() {
    let mut e = engine_with("hello\n");
    // No pane: should not panic.
    e.handle_terminal_scroll(0.0);
}

/// `handle_terminal_pane_press` on a plain shell (no mouse reporting) starts
/// a local text selection at the given cell.
#[test]
#[cfg(unix)]
fn handle_terminal_pane_press_starts_selection_on_primary_screen() {
    std::env::set_var("SHELL", "/bin/sh");
    let mut e = engine_with("hello\n");
    e.terminal_new_tab(80, 10);
    assert!(e.active_terminal().is_some());

    // On the primary screen (no mouse reporting) the press must NOT be
    // forwarded — it should start a local selection instead.
    let forwarded = e.handle_terminal_pane_press(
        5,
        2,
        quadraui::MouseButton::Left,
        quadraui::Modifiers::default(),
    );
    assert!(
        !forwarded,
        "primary-screen press must fall back to local selection"
    );
    let sel = e.active_terminal().unwrap().selection.as_ref().unwrap();
    assert_eq!(sel.start_col, 5);
    assert_eq!(sel.start_row, 2);
    assert_eq!(sel.end_col, 5);
    assert_eq!(sel.end_row, 2);

    e.terminal_write(b"exit\n");
}

/// `handle_terminal_pane_drag` extends the selection endpoint when there is
/// no mouse reporting.
#[test]
#[cfg(unix)]
fn handle_terminal_pane_drag_extends_selection() {
    std::env::set_var("SHELL", "/bin/sh");
    let mut e = engine_with("hello\n");
    e.terminal_new_tab(80, 10);
    assert!(e.active_terminal().is_some());

    // Start a selection at (col=2, row=1) — primary screen, no forwarding.
    e.handle_terminal_pane_press(
        2,
        1,
        quadraui::MouseButton::Left,
        quadraui::Modifiers::default(),
    );

    // Drag to (col=10, row=3) — should extend the endpoint.
    e.handle_terminal_pane_drag(10, 3);

    let sel = e.active_terminal().unwrap().selection.as_ref().unwrap();
    assert_eq!(sel.start_col, 2, "start_col must be anchored");
    assert_eq!(sel.start_row, 1, "start_row must be anchored");
    assert_eq!(sel.end_col, 10, "end_col must follow drag");
    assert_eq!(sel.end_row, 3, "end_row must follow drag");

    e.terminal_write(b"exit\n");
}
