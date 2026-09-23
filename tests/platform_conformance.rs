//! Black-box tests for `scripts/platform-conformance.sh` (#926).
//!
//! The script is the single entrypoint for "do the native builds behave the
//! same on every platform?" across the TUI, GTK, macOS and Win-GUI driver-tier
//! harnesses. Its one load-bearing property is the vacuous-pass guard: a lane
//! that ran but executed zero tests must be reported as a *failure*, not a
//! pass. That guard exists because this repo already paid for its absence
//! once (#645) -- `cargo test --no-default-features` silently omitted the
//! entire `vimcode` bin (all of `src/gtk/**`, `required-features = ["gui"]`),
//! so the Test stage compiled zero GTK assertions and every GTK issue earned
//! a vacuously green verdict with no error and no warning.
//!
//! These tests shell out to the real script with **stubbed lane commands and
//! probe overrides** (`PLATCONF_CMD_<LANE>` / `PLATCONF_OVERRIDE_<LANE>`, both
//! documented in the script's own header) so they run deterministically on
//! any host -- a CI runner with no gtk4, no cargo-xwin and no Darwin included
//! -- without ever invoking a real `cargo test`.
//!
//! Unix-only: the script is bash, and the tests shell out to it.
#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn script_path() -> PathBuf {
    repo_root().join("scripts/platform-conformance.sh")
}

/// Run the script with the given args and env overrides; returns
/// (stdout, stderr, exit code).
fn run_script(args: &[&str], envs: &[(&str, &str)]) -> (String, String, i32) {
    let mut cmd = Command::new("bash");
    cmd.arg(script_path()).args(args);
    // Start from a clean slate for every PLATCONF_* var so one test's
    // override can never leak into another via the ambient environment.
    for var in [
        "PLATCONF_OVERRIDE_TUI",
        "PLATCONF_OVERRIDE_GTK",
        "PLATCONF_OVERRIDE_MACOS",
        "PLATCONF_OVERRIDE_WIN",
        "PLATCONF_CMD_TUI",
        "PLATCONF_CMD_GTK",
        "PLATCONF_CMD_MACOS",
        "PLATCONF_CMD_WIN",
        "PLATCONF_CMD_WIN_CHECKONLY",
        "PLATCONF_TEST_FORCE_UNHANDLED_EXIT",
    ] {
        cmd.env_remove(var);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", script_path().display()));
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// A lane whose stubbed command reports 0 tests executed must fail the run,
/// and the matrix line for it must NOT read `passed`.
///
/// THE RED-VERIFIED ASSERTION for #926: with the vacuous-pass guard in
/// `run_lane()` (the `lines -eq 0` / `passed -eq 0` checks in
/// `scripts/platform-conformance.sh`) temporarily deleted, this test was
/// re-run and observed to fail (script exits 0, matrix printed `tui passed`)
/// before the guard was restored. See the PR description for the transcript.
#[test]
fn zero_tests_executed_fails_the_lane_not_passes_it() {
    let (stdout, _stderr, code) = run_script(
        &["--lane", "tui"],
        &[(
            "PLATCONF_CMD_TUI",
            "echo 'test result: ok. 0 passed; 0 failed; 0 ignored'",
        )],
    );
    assert_ne!(code, 0, "0-test lane must fail the run\n{stdout}");
    let tui_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("tui "))
        .unwrap_or_else(|| panic!("no tui line in matrix:\n{stdout}"));
    assert!(
        !tui_line.contains("passed"),
        "0-test lane must not read as passed: {tui_line}"
    );
    assert!(
        tui_line.contains("failed"),
        "0-test lane's matrix line should say failed: {tui_line}"
    );
}

/// #933 cause 2: the script must not exit 0 if it terminates unexpectedly
/// (a bash abort, a killing signal, a future bug that bypasses every
/// intentional exit point) -- that is a *separate* defect from the zero-test
/// vacuous-pass guard above, and needs its own guard (an EXIT trap that
/// checks whether a real exit point was reached) rather than relying on the
/// lane-result bookkeeping to happen to catch it.
///
/// This is exactly the shape of bug #933 reported on macOS: `declare -A`
/// failed (bash 3.2 has no associative arrays), a later reference to the
/// never-populated array tripped `set -u`, and the script died mid-run yet
/// still reported exit code 0 -- zero lanes run, vacuously green. The
/// `PLATCONF_TEST_FORCE_UNHANDLED_EXIT` hook reproduces "died mid-run with
/// an underlying exit status of 0" deterministically on any host's bash,
/// without needing an actual bash-3.2 install in CI.
///
/// THE RED-VERIFIED ASSERTION: with the `on_exit` trap / `finish` plumbing
/// in `scripts/platform-conformance.sh` temporarily reverted to a bare
/// `exit "$OVERALL_FAILURE"` (no trap), this test was re-run against the
/// `PLATCONF_TEST_FORCE_UNHANDLED_EXIT=1` hook and observed to fail (exit
/// code 0) before the trap was restored.
#[test]
fn unexpected_termination_forces_nonzero_exit() {
    let (stdout, stderr, code) = run_script(
        &["--print-plan"],
        &[("PLATCONF_TEST_FORCE_UNHANDLED_EXIT", "1")],
    );
    assert_ne!(
        code, 0,
        "a script that dies mid-run must not exit 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

/// A real failing run (non-zero exit, tests actually executed) is still
/// reported as failed -- the guard must not be the ONLY way to fail a lane.
#[test]
fn nonzero_exit_with_real_tests_still_fails() {
    let (stdout, _stderr, code) = run_script(
        &["--lane", "tui"],
        &[(
            "PLATCONF_CMD_TUI",
            "echo 'test result: FAILED. 3 passed; 1 failed; 0 ignored'; exit 101",
        )],
    );
    assert_ne!(code, 0);
    let tui_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("tui "))
        .unwrap();
    assert!(tui_line.contains("failed"), "{tui_line}");
}

/// A passing stub with a nonzero test count is reported `passed` and exits 0.
#[test]
fn nonzero_tests_executed_and_all_pass_reports_passed() {
    let (stdout, stderr, code) = run_script(
        &["--lane", "tui"],
        &[(
            "PLATCONF_CMD_TUI",
            "echo 'test result: ok. 12 passed; 0 failed; 0 ignored'",
        )],
    );
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let tui_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("tui "))
        .unwrap();
    assert!(tui_line.contains("passed"), "{tui_line}");
    assert!(tui_line.contains("12"), "{tui_line}");
}

/// A lane whose host probe fails is reported `skipped (<reason>)` and does
/// NOT fail the run -- this is the "not every unsupported lane is an error"
/// half of the rule; only a *forced* unsupported lane is an error (see
/// `forcing_an_unsupported_lane_is_an_error` below).
#[test]
fn probe_failure_is_skipped_not_failed() {
    let (stdout, stderr, code) = run_script(
        &[],
        &[
            (
                "PLATCONF_OVERRIDE_GTK",
                "capable=0;auto=0;reason=stubbed: gtk4 not present",
            ),
            (
                "PLATCONF_OVERRIDE_MACOS",
                "capable=0;auto=0;reason=stubbed: not darwin",
            ),
            (
                "PLATCONF_OVERRIDE_WIN",
                "capable=0;auto=0;reason=stubbed: no cargo-xwin",
            ),
            (
                "PLATCONF_CMD_TUI",
                "echo 'test result: ok. 1 passed; 0 failed; 0 ignored'",
            ),
        ],
    );
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let gtk_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("gtk "))
        .unwrap();
    assert!(gtk_line.contains("skipped"), "{gtk_line}");
    assert!(gtk_line.contains("stubbed: gtk4 not present"), "{gtk_line}");
}

/// `--lane <unsupported>` exits non-zero with the probe's reason -- forcing a
/// lane the host cannot support is an error, not a silent skip.
#[test]
fn forcing_an_unsupported_lane_is_an_error() {
    let (stdout, stderr, code) = run_script(
        &["--lane", "gtk"],
        &[(
            "PLATCONF_OVERRIDE_GTK",
            "capable=0;auto=0;reason=stubbed: gtk4 not present",
        )],
    );
    assert_ne!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let gtk_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("gtk "))
        .unwrap();
    assert!(gtk_line.contains("error"), "{gtk_line}");
    assert!(gtk_line.contains("stubbed: gtk4 not present"), "{gtk_line}");
}

/// An unknown `--lane` name is a usage error, independent of any probing.
#[test]
fn unknown_lane_name_is_a_usage_error() {
    let (stdout, stderr, code) = run_script(&["--lane", "amiga"], &[]);
    assert_ne!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.contains("amiga"), "stderr:\n{stderr}");
}

/// `--print-plan` resolves and prints the matrix without running any lane
/// command. Proven by a stub that would write a sentinel file if it were
/// actually invoked; the sentinel must not exist afterwards, and the plan
/// output must still name the command it *would* run.
#[test]
fn print_plan_runs_no_command() {
    let dir = std::env::temp_dir().join(format!(
        "vimcode_platform_conformance_test_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    let sentinel = dir.join("tui-was-run");
    let _ = std::fs::remove_file(&sentinel);

    let stub_cmd = format!("touch {}", sentinel.display());
    let (stdout, stderr, code) = run_script(
        &["--lane", "tui", "--print-plan"],
        &[("PLATCONF_CMD_TUI", &stub_cmd)],
    );

    let sentinel_exists = sentinel.exists();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        !sentinel_exists,
        "--print-plan must not execute the lane command"
    );
    assert!(
        stdout.contains(&stub_cmd),
        "--print-plan should print the command it would run:\n{stdout}"
    );
}

/// The win lane's check-only tier (cargo-xwin present, no WSL interop) is a
/// distinct state from `passed`/`failed`/`skipped`: real signal (it compiles)
/// without claiming tests ran.
#[test]
fn win_check_only_tier_is_reported_distinctly() {
    let (stdout, stderr, code) = run_script(
        &["--lane", "win"],
        &[
            (
                "PLATCONF_OVERRIDE_WIN",
                "capable=1;auto=1;tier=checkonly;reason=stubbed: no wsl interop",
            ),
            ("PLATCONF_CMD_WIN_CHECKONLY", "exit 0"),
        ],
    );
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let win_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("win "))
        .unwrap();
    assert!(win_line.contains("check-only"), "{win_line}");
    assert!(!win_line.contains("passed"), "{win_line}");
}

/// A check-only build that fails to compile is a real failure, not
/// check-only -- check-only means "compiled fine, just didn't run".
#[test]
fn win_check_only_build_failure_is_reported_failed() {
    let (stdout, stderr, code) = run_script(
        &["--lane", "win"],
        &[
            (
                "PLATCONF_OVERRIDE_WIN",
                "capable=1;auto=1;tier=checkonly;reason=stubbed: no wsl interop",
            ),
            ("PLATCONF_CMD_WIN_CHECKONLY", "exit 1"),
        ],
    );
    assert_ne!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let win_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("win "))
        .unwrap();
    assert!(win_line.contains("failed"), "{win_line}");
}

/// The script must be executable in the checkout, matching how
/// `docs/PLATFORM_CONFORMANCE.md` documents it as a directly runnable command.
#[test]
fn script_is_executable() {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(script_path())
        .expect("script must exist")
        .permissions()
        .mode();
    assert!(
        mode & 0o111 != 0,
        "scripts/platform-conformance.sh is not executable (mode {mode:o})"
    );
}

/// Static guard for the crt-static requirement called out in the issue: the
/// win lane must set `RUSTFLAGS` itself rather than relying on ambient env,
/// because the target Windows host has no vcruntime140.dll and a dynamically
/// linked .exe dies before `main()` with no output -- which reads as a
/// passing no-op, not a failure. Cheaper and more direct than driving an
/// actual cross-compile in this suite.
#[test]
fn win_lane_sets_crt_static_itself() {
    let script = std::fs::read_to_string(script_path()).expect("read script");
    assert!(
        script.contains("target-feature=+crt-static"),
        "win lane must bake in RUSTFLAGS=\"-C target-feature=+crt-static\" rather than \
         relying on ambient env (see docs/PLATFORM_CONFORMANCE.md)"
    );
}
