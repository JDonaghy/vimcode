//! This test spawns the compiled `vimcode`/`vcd` binary with
//! `DISPLAY`/`WAYLAND_DISPLAY` removed and asserts it prints usage
//! and exits 0 rather than panicking or printing GTK's generic help.
//! The `--help` flag must be processed before any GTK initialization.
//!
//! `--help` itself exits before anything would read or write
//! `$HOME/.config/vimcode`, but both tests still pin `$HOME` to a
//! throwaway directory (mirroring `tests/ext_install_cli.rs`'s
//! real-binary-spawn pattern) so that stays true even if `--help`
//! handling ever moves later in startup.

use std::process::Command;

/// Build a throwaway, empty `$HOME` for a spawned binary and return its path.
fn throwaway_home(label: &str) -> std::path::PathBuf {
    let home =
        std::env::temp_dir().join(format!("vc-cli-help-test-{}-{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create throwaway home dir");
    home
}

#[test]
fn vimcode_help_prints_usage_and_exits_0_with_no_display() {
    let home = throwaway_home("vimcode");
    let exe = env!("CARGO_BIN_EXE_vimcode");
    let output = Command::new(exe)
        .arg("--help")
        .env("HOME", &home)
        .env("VIMCODE_TEST_HOME_ISOLATED", "1")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("failed to run vimcode --help");
    let _ = std::fs::remove_dir_all(&home);

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
    let home = throwaway_home("vcd");
    let exe = env!("CARGO_BIN_EXE_vcd");
    let output = Command::new(exe)
        .arg("--help")
        .env("HOME", &home)
        .env("VIMCODE_TEST_HOME_ISOLATED", "1")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("failed to run vcd --help");
    let _ = std::fs::remove_dir_all(&home);

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
