//! This spawns the actual compiled `vimcode` binary with both display
//! env vars removed and asserts it prints a clear, immediate error and
//! exits non-zero when neither `DISPLAY` nor `WAYLAND_DISPLAY` is set.
//!
//! The guard in `gtk::run` is deliberately gated to X11/Wayland platforms
//! (a no-op on macOS/quartz and Windows/win32, where there is no display
//! variable to set and refusing to start would break a perfectly good GUI
//! build). On macOS this test's child process does *not* exit early — it
//! goes on to call `gtk4::init()`, succeeds, opens a real window and runs
//! the GTK main loop, so `Command::output()` would block forever.
//!
//! The guard's platform split is already covered without spawning anything
//! by the `no_display_guard_tests` module beside it in `src/gtk/mod.rs`,
//! including the macOS/Windows "never trips" direction, so nothing is lost
//! by not spawning the binary off-Linux.

#![cfg(not(any(target_os = "macos", target_os = "windows")))]

use std::process::Command;

#[test]
fn vimcode_with_no_display_prints_a_clear_error_and_exits_nonzero() {
    let exe = env!("CARGO_BIN_EXE_vimcode");
    let output = Command::new(exe)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("failed to run vimcode");

    assert!(
        !output.status.success(),
        "vimcode with no display should not succeed (status: {:?})\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no display found") && stderr.contains("DISPLAY"),
        "expected a clear 'no display' message naming DISPLAY/WAYLAND_DISPLAY \
         on stderr, got: {stderr}"
    );
    assert!(
        !stderr.contains("Failed to initialize GTK"),
        "must fail before ever touching gtk4::init(), got: {stderr}"
    );
    assert!(
        !stderr.contains("swap files"),
        "no buffer was ever opened, so the crash-recovery hook must never \
         fire here, got: {stderr}"
    );
}
