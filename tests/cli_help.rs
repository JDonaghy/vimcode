//! This test spawns the compiled `vimcode`/`vcd` binary with
//! `DISPLAY`/`WAYLAND_DISPLAY` removed and asserts it prints usage
//! and exits 0 rather than panicking or printing GTK's generic help.
//! The `--help` flag must be processed before any GTK initialization.

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
