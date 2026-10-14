//! Spawns the compiled `vimcode` binary with `--skin` pointing at a file
//! that is not there, and asserts the skin is resolved before any backend
//! starts: the process must exit non-zero with a message naming the file,
//! not open a window (or a terminal UI) with the built-in skin and carry on
//! as if nothing was asked for.
//!
//! `DISPLAY`/`WAYLAND_DISPLAY` are removed and `$HOME` is pinned to a
//! throwaway directory, mirroring `tests/cli_help.rs`'s real-binary-spawn
//! pattern.

use std::process::Command;

#[test]
fn skin_flag_with_an_unreadable_file_fails_before_any_backend_starts() {
    let home = std::env::temp_dir().join(format!("vc-skin-cli-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create throwaway home dir");
    let missing = home.join("no-such-skin.toml");

    let output = Command::new(env!("CARGO_BIN_EXE_vimcode"))
        .arg("--skin")
        .arg(&missing)
        .env("HOME", &home)
        .env("VIMCODE_TEST_HOME_ISOLATED", "1")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("failed to run vimcode --skin");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let _ = std::fs::remove_dir_all(&home);

    assert!(
        !output.status.success(),
        "vimcode --skin <missing file> exited 0 (status: {:?})\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code(),
    );
    assert!(
        stderr.contains("skin file") && stderr.contains("no-such-skin.toml"),
        "stderr must name the unreadable skin file; got: {stderr}"
    );
}
