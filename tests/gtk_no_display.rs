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
//!
//! `gtk::run()`'s no-display guard calls `std::process::exit(1)` before the
//! first-run icon/desktop-file install (`src/gtk/util.rs`) ever runs, so this
//! scenario does not currently write under `$HOME`. Still, this test spawns
//! the real binary, and nothing stops a future reordering of `gtk::run()`
//! from moving a write ahead of the guard. Pin `$HOME` to a throwaway
//! directory anyway, mirroring `tests/ext_install_cli.rs`'s real-binary-spawn
//! pattern, so a spawned-binary test here can never land on the real
//! developer home.

#![cfg(not(any(target_os = "macos", target_os = "windows")))]

use std::process::Command;

#[test]
fn vimcode_with_no_display_prints_a_clear_error_and_exits_nonzero() {
    let home = std::env::temp_dir().join(format!("vc-gtk-no-display-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create throwaway home dir");

    let exe = env!("CARGO_BIN_EXE_vimcode");
    let output = Command::new(exe)
        .env("HOME", &home)
        .env("VIMCODE_TEST_HOME_ISOLATED", "1")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("failed to run vimcode");

    let _ = std::fs::remove_dir_all(&home);

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
