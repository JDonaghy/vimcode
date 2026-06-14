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
