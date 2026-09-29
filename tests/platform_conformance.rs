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
//! #1092 made the script *gateable*: a documented exit contract (0 pass /
//! 1 fail / 2 usage / 3 coverage-gap), a machine-readable per-lane summary
//! (`--summary <path>`, schema `vimcode.platform-conformance/1`) and a
//! single greppable `PLATFORM_CONFORMANCE_SUMMARY ...` line, so a release
//! step can refuse a roll from the exit code and one line of output alone.
//! The `exit_contract_*` / `summary_*` tests below pin that contract:
//! a failing lane, a supported-but-skipped lane, a zero-test lane and an
//! all-green run, each asserting both the exit code and the parsed summary
//! a release caller would actually read.
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

// ── #1092: the release-gate contract ──────────────────────────────────
//
// Everything below is what a release step actually consumes: an exit code,
// one summary line, and a JSON file. The stub mechanism above is what makes
// them runnable on any host -- no gtk4, no cargo-xwin, no Darwin needed.
//
// THE RED-VERIFIED ASSERTION for every `exit_contract_*` / `summary_*` /
// `print_plan_reports_verdict_plan_not_pass` / `docs_state_the_exit_contract_*`
// test below (#1092): each was re-run, unmodified, against the
// pre-#1092 `scripts/platform-conformance.sh` and `docs/PLATFORM_CONFORMANCE.md`
// (`git show <parent-of-9460a09>:scripts/platform-conformance.sh` -- the
// commit immediately before #1092's exit-contract/summary/lane-map work
// landed, i.e. unfixed `develop` for this change) and observed to fail:
// `--summary` was an unrecognized flag on that revision (no `--summary`
// support, no `--print-plan` verdict/mode fields, no `skipped-capable` /
// `not-in-scope` statuses, no JSON schema, no lane-to-machine map in the
// docs), so `run_with_summary()` never found a summary file to parse and
// `summary_line()` never found a `PLATFORM_CONFORMANCE_SUMMARY` line with
// the new fields. Result: 8 of the 9 tests failed outright (`cargo test
// --no-default-features --test platform_conformance -- <these test names>`
// reported "FAILED. 1 passed; 8 failed"); the 9th,
// `unwritable_summary_path_is_exit_2_before_any_lane_runs`, initially
// passed for the *wrong* reason (an unrecognized `--summary` flag also
// exits 2 on the old script, coincidentally), so its assertion was
// strengthened to also require the specific "not writable" wording the new
// validation emits -- re-verified RED against the same pre-#1092 revision
// (now correctly failing, since the old script only ever says "unknown
// argument") before being restored to green against this branch's script.

/// Exit codes the script documents in its header and in
/// `docs/PLATFORM_CONFORMANCE.md`. Anything but `PASS` means "do not roll".
const EXIT_PASS: i32 = 0;
const EXIT_FAIL: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_COVERAGE_GAP: i32 = 3;

/// A scratch directory unique per test, so concurrent `cargo test` threads
/// never fight over one summary path.
fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vimcode_platform_conformance_{}_{}",
        std::process::id(),
        tag
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Run the script with `--summary <tmpfile>` and return
/// (stdout, stderr, exit code, parsed summary JSON if one was written).
fn run_with_summary(
    tag: &str,
    args: &[&str],
    envs: &[(&str, &str)],
) -> (String, String, i32, Option<serde_json::Value>) {
    let dir = scratch_dir(tag);
    let path = dir.join("summary.json");
    let _ = std::fs::remove_file(&path);
    let path_str = path.display().to_string();

    let mut full_args: Vec<&str> = args.to_vec();
    full_args.push("--summary");
    full_args.push(&path_str);

    let (stdout, stderr, code) = run_script(&full_args, envs);
    let json = std::fs::read_to_string(&path)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            serde_json::from_str(&s)
                .unwrap_or_else(|e| panic!("summary is not valid JSON: {e}\n{s}"))
        });
    let _ = std::fs::remove_dir_all(&dir);
    (stdout, stderr, code, json)
}

/// Parse the single `PLATFORM_CONFORMANCE_SUMMARY k=v k=v ...` line a caller
/// that only reads stdout would grep for.
fn summary_line(stdout: &str) -> std::collections::HashMap<String, String> {
    let line = stdout
        .lines()
        .find(|l| l.starts_with("PLATFORM_CONFORMANCE_SUMMARY "))
        .unwrap_or_else(|| panic!("no PLATFORM_CONFORMANCE_SUMMARY line in:\n{stdout}"));
    line.split_whitespace()
        .skip(1)
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Lane status from the parsed summary document.
fn lane_status(summary: &serde_json::Value, lane: &str) -> String {
    summary["lanes"]
        .as_array()
        .expect("lanes array")
        .iter()
        .find(|l| l["lane"] == lane)
        .unwrap_or_else(|| panic!("lane {lane} missing from summary: {summary}"))["status"]
        .as_str()
        .expect("status string")
        .to_string()
}

/// CONTRACT, all-green: every in-scope lane green ⇒ exit 0, `verdict=pass`,
/// and every lane in the summary carries a green terminal status (`passed`
/// or the win cross-compile tier's `check-only`).
#[test]
fn exit_contract_all_green_run_is_exit_0_and_verdict_pass() {
    let (stdout, stderr, code, summary) = run_with_summary(
        "all_green",
        &[],
        &[
            (
                "PLATCONF_CMD_TUI",
                "echo 'test result: ok. 12 passed; 0 failed; 0 ignored'",
            ),
            ("PLATCONF_OVERRIDE_GTK", "capable=1;auto=1"),
            (
                "PLATCONF_CMD_GTK",
                "echo 'test result: ok. 7 passed; 0 failed; 0 ignored'",
            ),
            ("PLATCONF_OVERRIDE_MACOS", "capable=1;auto=1"),
            (
                "PLATCONF_CMD_MACOS",
                "echo 'test result: ok. 4 passed; 0 failed; 0 ignored'",
            ),
            (
                "PLATCONF_OVERRIDE_WIN",
                "capable=1;auto=1;tier=checkonly;reason=stubbed: no wsl interop",
            ),
            ("PLATCONF_CMD_WIN_CHECKONLY", "exit 0"),
        ],
    );
    assert_eq!(
        code, EXIT_PASS,
        "all-green run must exit 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let line = summary_line(&stdout);
    assert_eq!(line.get("verdict").map(String::as_str), Some("pass"));
    assert_eq!(line.get("exit").map(String::as_str), Some("0"));
    assert_eq!(line.get("tests_passed").map(String::as_str), Some("23"));
    assert_eq!(line.get("tests_failed").map(String::as_str), Some("0"));

    let summary = summary.expect("a --summary file must be written on a green run");
    assert_eq!(summary["verdict"], "pass");
    assert_eq!(summary["exit_code"], 0);
    assert_eq!(summary["mode"], "run");
    assert_eq!(lane_status(&summary, "tui"), "passed");
    assert_eq!(lane_status(&summary, "gtk"), "passed");
    assert_eq!(lane_status(&summary, "macos"), "passed");
    // check-only is green but explicitly NOT "passed" -- see #926.
    assert_eq!(lane_status(&summary, "win"), "check-only");
    assert_eq!(summary["totals"]["tests_passed"], 23);
}

/// CONTRACT, failing lane: a lane whose tests fail ⇒ exit 1, `verdict=fail`,
/// and the failing lane is named in the summary with its failure count, so a
/// release step knows *which* lane blocked the roll without scrollback.
#[test]
fn exit_contract_failing_lane_is_exit_1_and_names_the_lane() {
    let (stdout, stderr, code, summary) = run_with_summary(
        "failing_lane",
        &[],
        &[
            (
                "PLATCONF_CMD_TUI",
                "echo 'test result: ok. 12 passed; 0 failed; 0 ignored'",
            ),
            ("PLATCONF_OVERRIDE_GTK", "capable=1;auto=1"),
            (
                "PLATCONF_CMD_GTK",
                "echo 'test result: FAILED. 5 passed; 2 failed; 0 ignored'; exit 101",
            ),
            (
                "PLATCONF_OVERRIDE_MACOS",
                "capable=0;auto=0;reason=stubbed: not darwin",
            ),
            (
                "PLATCONF_OVERRIDE_WIN",
                "capable=0;auto=0;reason=stubbed: no cargo-xwin",
            ),
        ],
    );
    assert_eq!(
        code, EXIT_FAIL,
        "a failing lane must exit 1\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let line = summary_line(&stdout);
    assert_eq!(line.get("verdict").map(String::as_str), Some("fail"));
    assert_eq!(line.get("exit").map(String::as_str), Some("1"));
    assert!(
        line["lanes"].contains("gtk:failed"),
        "summary line must name the failing lane: {}",
        line["lanes"]
    );

    let summary = summary.expect("a --summary file must be written even when a lane fails");
    assert_eq!(summary["verdict"], "fail");
    assert_eq!(summary["exit_code"], 1);
    assert_eq!(lane_status(&summary, "gtk"), "failed");
    assert_eq!(lane_status(&summary, "tui"), "passed");
    assert_eq!(summary["totals"]["tests_failed"], 2);
}

/// CONTRACT, supported-but-skipped lane: the host's probe says the lane is
/// CAPABLE, but policy did not auto-select it (GTK on Darwin is the real
/// case). Nothing is broken, but this host's run does not cover what it
/// could have ⇒ its own exit code, 3, distinct from both a green run and a
/// real failure, and a distinct `skipped-capable` status so the gap is
/// visible in the record rather than inferred.
#[test]
fn exit_contract_capable_but_skipped_lane_is_exit_3_coverage_gap() {
    let (stdout, stderr, code, summary) = run_with_summary(
        "coverage_gap",
        &[],
        &[
            (
                "PLATCONF_CMD_TUI",
                "echo 'test result: ok. 12 passed; 0 failed; 0 ignored'",
            ),
            (
                "PLATCONF_OVERRIDE_GTK",
                "capable=1;auto=0;reason=stubbed: opt-in on Darwin",
            ),
            (
                "PLATCONF_OVERRIDE_MACOS",
                "capable=0;auto=0;reason=stubbed: not darwin",
            ),
            (
                "PLATCONF_OVERRIDE_WIN",
                "capable=0;auto=0;reason=stubbed: no cargo-xwin",
            ),
        ],
    );
    assert_eq!(
        code, EXIT_COVERAGE_GAP,
        "a capable-but-skipped lane must exit 3, not 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let line = summary_line(&stdout);
    assert_eq!(
        line.get("verdict").map(String::as_str),
        Some("coverage-gap")
    );
    assert_eq!(line.get("exit").map(String::as_str), Some("3"));

    let summary = summary.expect("a --summary file must be written on a coverage-gap run");
    assert_eq!(summary["verdict"], "coverage-gap");
    assert_eq!(summary["exit_code"], 3);
    assert_eq!(lane_status(&summary, "gtk"), "skipped-capable");
    // An *incapable* lane stays plain `skipped` and is not a gap: that host
    // genuinely cannot run it, so there is nothing for it to have covered.
    assert_eq!(lane_status(&summary, "macos"), "skipped");
    let gtk = summary["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["lane"] == "gtk")
        .unwrap()
        .clone();
    assert_eq!(gtk["capable"], true);
    assert_eq!(gtk["auto_selected"], false);
    assert!(
        gtk["detail"]
            .as_str()
            .unwrap()
            .contains("stubbed: opt-in on Darwin"),
        "the gap must carry its reason: {gtk}"
    );
}

/// CONTRACT, zero-test lane: the #645 vacuous-green trap reported through
/// the machine-readable surface a release step reads, not just the human
/// matrix. `tests_passed: 0` with a green exit would be exactly the bug the
/// script exists to prevent, so this pins exit 1 + `failed` + a detail that
/// names the guard.
#[test]
fn exit_contract_zero_test_lane_is_exit_1_in_the_summary_too() {
    let (stdout, stderr, code, summary) = run_with_summary(
        "zero_tests",
        &["--lane", "tui"],
        &[(
            "PLATCONF_CMD_TUI",
            "echo 'test result: ok. 0 passed; 0 failed; 0 ignored'",
        )],
    );
    assert_eq!(
        code, EXIT_FAIL,
        "a zero-test lane must exit 1\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let line = summary_line(&stdout);
    assert_eq!(line.get("verdict").map(String::as_str), Some("fail"));
    assert!(line["lanes"].contains("tui:failed"), "{}", line["lanes"]);

    let summary = summary.expect("summary must be written for a vacuous-pass failure");
    assert_eq!(summary["verdict"], "fail");
    assert_eq!(lane_status(&summary, "tui"), "failed");
    let tui = summary["lanes"].as_array().unwrap()[0].clone();
    assert_eq!(tui["tests_passed"], 0);
    assert!(
        tui["detail"].as_str().unwrap().contains("vacuous"),
        "the record must say why it failed: {tui}"
    );
}

/// A forced `--lane` subset leaves the other lanes unprobed. The summary
/// must still list them, as `not-in-scope` — "which lane was not covered on
/// this roll?" is the question the machine-readable matrix exists to answer,
/// and silence would be indistinguishable from a green lane.
#[test]
fn summary_records_lanes_this_host_never_probed_as_not_in_scope() {
    let (stdout, stderr, code, summary) = run_with_summary(
        "not_in_scope",
        &["--lane", "tui"],
        &[(
            "PLATCONF_CMD_TUI",
            "echo 'test result: ok. 3 passed; 0 failed; 0 ignored'",
        )],
    );
    assert_eq!(code, EXIT_PASS, "stdout:\n{stdout}\nstderr:\n{stderr}");

    let summary = summary.expect("summary file");
    assert_eq!(lane_status(&summary, "tui"), "passed");
    for lane in ["gtk", "macos", "win"] {
        assert_eq!(
            lane_status(&summary, lane),
            "not-in-scope",
            "lane {lane} was never probed and must say so"
        );
    }
    assert_eq!(summary["scope"], serde_json::json!(["tui"]));
    assert!(
        summary_line(&stdout)["lanes"].contains("gtk:not-in-scope"),
        "the one-line summary must carry the uncovered lanes too"
    );
}

/// `--print-plan` is never a gate result: nothing ran, so nothing is proven
/// either way. It must say so with its own verdict rather than borrowing
/// `pass` — a release step that accidentally passed `--print-plan` and read
/// exit 0 as "the lanes are green" would be the same vacuous-green class of
/// bug this whole script exists to prevent. The capable-but-skipped lane
/// here also proves plan mode does NOT raise the coverage-gap code: you
/// cannot have a coverage gap in a run that was never going to cover
/// anything.
#[test]
fn print_plan_reports_verdict_plan_not_pass() {
    let (stdout, stderr, code, summary) = run_with_summary(
        "print_plan_verdict",
        &["--print-plan"],
        &[
            (
                "PLATCONF_OVERRIDE_GTK",
                "capable=1;auto=0;reason=stubbed: opt-in on Darwin",
            ),
            (
                "PLATCONF_OVERRIDE_MACOS",
                "capable=0;auto=0;reason=stubbed: not darwin",
            ),
            (
                "PLATCONF_OVERRIDE_WIN",
                "capable=0;auto=0;reason=stubbed: no cargo-xwin",
            ),
        ],
    );
    assert_eq!(code, EXIT_PASS, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let line = summary_line(&stdout);
    assert_eq!(line.get("verdict").map(String::as_str), Some("plan"));
    assert_eq!(line.get("mode").map(String::as_str), Some("plan"));

    let summary = summary.expect("summary file");
    assert_eq!(summary["verdict"], "plan");
    assert_eq!(summary["mode"], "plan");
    assert_eq!(lane_status(&summary, "tui"), "plan:run");
}

/// `--summary -` emits the same document on stdout after a marker line, for
/// a caller that would rather pipe than write a file.
#[test]
fn summary_dash_writes_the_same_json_to_stdout() {
    let (stdout, stderr, code) = run_script(
        &["--lane", "tui", "--summary", "-"],
        &[(
            "PLATCONF_CMD_TUI",
            "echo 'test result: ok. 9 passed; 0 failed; 0 ignored'",
        )],
    );
    assert_eq!(code, EXIT_PASS, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let (_, json_part) = stdout
        .split_once("===PLATFORM-CONFORMANCE-JSON===\n")
        .unwrap_or_else(|| panic!("no JSON marker in stdout:\n{stdout}"));
    let parsed: serde_json::Value = serde_json::from_str(json_part)
        .unwrap_or_else(|e| panic!("stdout JSON did not parse: {e}\n{json_part}"));
    assert_eq!(parsed["schema"], "vimcode.platform-conformance/1");
    assert_eq!(parsed["verdict"], "pass");
    assert_eq!(lane_status(&parsed, "tui"), "passed");
}

/// CONTRACT, usage error: an unwritable `--summary` path is exit 2 and is
/// detected BEFORE any lane runs — a release caller passing a bad path
/// should find out in a second, not after a ten-minute `cargo test` whose
/// only machine-readable record is then lost. Proven with a sentinel stub
/// that would touch a file if the lane had been invoked.
#[test]
fn unwritable_summary_path_is_exit_2_before_any_lane_runs() {
    let dir = scratch_dir("unwritable_summary");
    let sentinel = dir.join("tui-was-run");
    let _ = std::fs::remove_file(&sentinel);
    let stub_cmd = format!("touch {}", sentinel.display());

    let (stdout, stderr, code) = run_script(
        &[
            "--lane",
            "tui",
            "--summary",
            "/definitely/not/a/directory/summary.json",
        ],
        &[("PLATCONF_CMD_TUI", &stub_cmd)],
    );
    let sentinel_exists = sentinel.exists();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(
        code, EXIT_USAGE,
        "a bad --summary path is a usage error\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !sentinel_exists,
        "the summary path must be validated before any lane command runs"
    );
    assert!(
        stderr.contains("--summary") && stderr.contains("not writable"),
        "the error must name the offending option and say *why* -- not just \
         reject `--summary` as an unrecognized flag (a pre-#1092 script with \
         no --summary support at all would coincidentally also exit 2 here, \
         for the wrong reason):\n{stderr}"
    );
}

/// The exit contract is only usable if it is written down where a release
/// operator will look. Pin the four codes and the lane→machine map to
/// `docs/PLATFORM_CONFORMANCE.md` so a future edit to one side cannot
/// silently drift from the other.
#[test]
fn docs_state_the_exit_contract_and_the_lane_to_machine_map() {
    let doc = std::fs::read_to_string(repo_root().join("docs/PLATFORM_CONFORMANCE.md"))
        .expect("docs/PLATFORM_CONFORMANCE.md must exist");
    for needle in [
        "## Exit contract",
        "`0`",
        "`1`",
        "`2`",
        "`3`",
        "coverage-gap",
        "skipped-capable",
        "not-in-scope",
        "--summary",
        "PLATFORM_CONFORMANCE_SUMMARY",
        "vimcode.platform-conformance/1",
        // lane → machine map (#1092 scope item 3)
        "## Lane-to-machine map",
        "dellserver",
        "macmini",
        "dell64",
        // what the release side calls (#1092 scope item 4)
        "## What the release side calls",
    ] {
        assert!(
            doc.contains(needle),
            "docs/PLATFORM_CONFORMANCE.md must document {needle:?}"
        );
    }
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
