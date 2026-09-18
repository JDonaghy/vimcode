//! #979: `vimcode --help` used to initialize GTK before the flag was even
//! looked at, so on any host with no `DISPLAY`/`WAYLAND_DISPLAY` (SSH, a
//! container, CI) it panicked instead of printing usage — the exact repro
//! from the v0.11.0 release smoke on `dellserver`.
//!
//! This spawns the actual compiled `vimcode`/`vcd` binaries (not the
//! in-process `Args::parse` unit tests in `src/main.rs`, which only pin the
//! parsing logic and can't observe GTK ever being touched) with
//! `DISPLAY`/`WAYLAND_DISPLAY` removed — the one condition the original bug
//! required — and asserts they print usage and exit 0 rather than crashing.
//!
//! Observed RED against unfixed `develop`: with the `--help` check removed
//! from `src/main.rs`'s dispatcher (restoring the pre-fix ordering, where
//! `--help` fell through to `launch_gui`),
//! `vimcode_help_prints_usage_and_exits_0_with_no_display` failed — on the
//! host that reproduced this locally, GTK's own `GApplication` argument
//! parser intercepted `--help` and printed its generic `[OPTION…]` help
//! instead of vimcode's usage (the `assert!(stdout.contains("Usage:
//! vimcode"))` failed); on the release-smoke host in the original report,
//! the same code path instead panicked in `gtk4::rt::init` ("Failed to
//! initialize GTK") with a non-zero exit. Either way, `--help` never reaches
//! the dispatcher's own usage text without the fix.

use std::process::Command;

#[test]
fn vimcode_help_prints_usage_and_exits_0_with_no_display() {
    let exe = env!("CARGO_BIN_EXE_vimcode");
    let output = Command::new(exe)
        .arg("--help")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("failed to run vimcode --help");

    assert!(
        output.status.success(),
        "vimcode --help did not exit 0 (status: {:?})\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: vimcode"), "got stdout: {stdout}");
    assert!(stdout.contains("--tui"), "got stdout: {stdout}");
    assert!(stdout.contains("--version"), "got stdout: {stdout}");

    // The crash path's swap-recovery message must never appear here: no
    // buffer was ever opened, so claiming one was "written to swap files
    // for recovery" would be a lie (see issue notes).
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("swap files"),
        "unexpected crash/recovery chatter on stderr: {stderr}"
    );
}

#[test]
fn vcd_help_prints_usage_and_exits_0() {
    let exe = env!("CARGO_BIN_EXE_vcd");
    let output = Command::new(exe)
        .arg("--help")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("failed to run vcd --help");

    assert!(
        output.status.success(),
        "vcd --help did not exit 0 (status: {:?})\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.is_empty(),
        "vcd --help printed nothing (was: silent no-op)"
    );
    assert!(stdout.contains("Usage:"), "got stdout: {stdout}");
}
