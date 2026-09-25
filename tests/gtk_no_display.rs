//! #1106: `vimcode` used to silently invent `DISPLAY=:0` whenever neither
//! `DISPLAY` nor `WAYLAND_DISPLAY` was set — a guess, not a fallback, since
//! `:0` is not guaranteed to be a real display (Xvfb commonly picks `:99`, a
//! second X session `:1`, etc.). When the guess was wrong the failure didn't
//! disappear, it just moved later and got less legible: `gtk4::init()` would
//! still panic, just against a display name nobody chose and with the
//! swap-crash panic hook's chatter mixed in — the same "surfaces later and
//! less legibly" failure mode #979 fixed for `--help`.
//!
//! This spawns the actual compiled `vimcode` binary (not `Args::parse` unit
//! tests, which never reach `gtk::run`) with both display env vars removed
//! and asserts it prints a clear, immediate error and exits non-zero,
//! instead of a GTK panic.
//!
//! Observed RED against unfixed `develop`: with the `DISPLAY=:0` guess
//! restored (and the loud early exit removed), this process instead runs
//! past the top of `gtk::run` and panics inside `gtk4::init()` with "Failed
//! to initialize GTK" — a non-zero exit, but with none of this test's
//! specific assertions about *what* went wrong or *when*, and it doesn't
//! reliably return quickly (a bad guessed `DISPLAY` can leave GTK to spend
//! real time probing before giving up, unlike this fix's immediate check
//! before anything is touched).

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
