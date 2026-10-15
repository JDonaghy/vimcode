//! Black-box tests for `scripts/regress`.
//!
//! `scripts/regress` is the developer-iteration counterpart to
//! `scripts/platform-conformance.sh`: it runs the `backend_conformance!`
//! scenarios for **one** backend, optionally narrowed to a feature area, and
//! reports a per-scenario table plus a one-line greppable summary.
//!
//! Its load-bearing property is that the table and the summary agree with
//! `cargo test`'s own verdict:
//!
//! * A scenario cargo reported as `FAILED` must read `fail` in the table.
//! * A scenario cargo reported as `ok` must read `pass` — never `skip`.
//! * The totals must sum **every** `test result:` line, because `cargo test`
//!   emits one per test binary (lib unittests, each `tests/*.rs`, each bin)
//!   and only the binary holding the harness tests reports a nonzero count.
//!   Reading just the last line reports `0 passed; 0 failed` and then trips
//!   the zero-test guard on a run that was genuinely green.
//! * Zero tests executed is a failure, not a pass — the vacuous-green trap.
//! * The full cargo log survives the run and its path is printed, so the
//!   short table is never the only record of what happened.
//! * The exit status follows the documented contract (0 pass / 1 fail /
//!   2 backend unavailable / 3 usage), not cargo's raw 101.
//!
//! These tests shell out to the real script with the stub command, probe and
//! label-source overrides documented in its own header (`REGRESS_CMD_<BACKEND>`,
//! `REGRESS_OVERRIDE_<BACKEND>`, `REGRESS_HARNESS_FILE`), so they are
//! deterministic on any host — no gtk4, no cargo-xwin, no Darwin, and never a
//! real `cargo test`.
//!
//! Unix-only: the script is bash, and the tests shell out to it.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn script_path() -> PathBuf {
    repo_root().join("scripts/regress")
}

/// Run the script with the given args and env overrides; returns
/// (stdout, stderr, exit code).
fn run_script(args: &[&str], envs: &[(&str, &str)]) -> (String, String, i32) {
    let mut cmd = Command::new("bash");
    cmd.arg(script_path()).args(args);
    // Start from a clean slate for every REGRESS_* var so one test's override
    // can never leak into another via the ambient environment.
    for var in [
        "REGRESS_CMD_TUI",
        "REGRESS_CMD_GTK",
        "REGRESS_CMD_MACOS",
        "REGRESS_CMD_WIN",
        "REGRESS_OVERRIDE_TUI",
        "REGRESS_OVERRIDE_GTK",
        "REGRESS_OVERRIDE_MACOS",
        "REGRESS_OVERRIDE_WIN",
        "REGRESS_HARNESS_FILE",
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

/// A throwaway directory for one test's fixture files, removed on drop.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_regress_script_{tag}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        Self { dir }
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.dir.join(name);
        std::fs::write(&path, contents).expect("write fixture");
        path
    }

    /// A two-scenario stand-in for `src/harness.rs`, in the shape the script's
    /// label scraper reads.
    fn harness_fixture(&self) -> PathBuf {
        self.write(
            "harness_fixture.rs",
            "backend_conformance! {\n    \
                 label: alpha_scenario,\n    \
                 backends: [gtk, tui, tui_prod],\n\
             }\n\
             backend_conformance! {\n    \
                 label: beta_scenario,\n    \
                 backends: [gtk, tui, tui_prod],\n\
             }\n",
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A shell snippet that replays `lines` on stdout and exits with `code`.
fn stub_cmd(lines: &[&str], code: i32) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(&format!("printf '%s\\n' {};\n", shell_quote(line)));
    }
    out.push_str(&format!("exit {code}\n"));
    out
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The per-scenario table row for `scenario` on `backend`, as printed.
/// `None` when the table has no row for that pair at all.
fn table_row<'a>(stdout: &'a str, scenario: &str, backend: &str) -> Option<&'a str> {
    let table = stdout
        .split("=== regress: per-scenario results ===")
        .nth(1)?
        // Everything after the table body is the log path + summary.
        .split("Full log:")
        .next()?;
    table.lines().find(|l| {
        let mut cols = l.split_whitespace();
        cols.next() == Some(scenario) && cols.next() == Some(backend)
    })
}

/// The `RESULT` column of a table row.
fn row_result(row: &str) -> String {
    let mut cols = row.split_whitespace();
    cols.next();
    cols.next();
    cols.collect::<Vec<_>>().join(" ")
}

/// Parse the single greppable `REGRESS_SUMMARY ...` line into key=value pairs.
fn summary_fields(stdout: &str) -> Vec<(String, String)> {
    let line = stdout
        .lines()
        .find(|l| l.starts_with("REGRESS_SUMMARY "))
        .unwrap_or_else(|| panic!("no REGRESS_SUMMARY line in output:\n{stdout}"));
    line.split_whitespace()
        .skip(1)
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn summary_field(stdout: &str, key: &str) -> String {
    summary_fields(stdout)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
        .unwrap_or_else(|| panic!("no `{key}=` in REGRESS_SUMMARY line:\n{stdout}"))
}

/// The path printed after `Full log:`.
fn logged_path(stdout: &str) -> PathBuf {
    let line = stdout
        .lines()
        .find(|l| l.starts_with("Full log:"))
        .unwrap_or_else(|| panic!("no `Full log:` line in output:\n{stdout}"));
    PathBuf::from(line.trim_start_matches("Full log:").trim())
}

// ── The table must agree with cargo's own verdict ────────────────────────────

/// A scenario `cargo test` reported as `FAILED` must read `fail` in the table,
/// and the run must not exit 0. A table that disagreed with cargo here would
/// be worse than no table at all: an operator reads the short table, not the
/// thousand-line log.
#[test]
fn table_reports_fail_for_the_scenario_cargo_marked_failed() {
    let scratch = Scratch::new("table_fail");
    let harness = scratch.harness_fixture();
    let cmd = stub_cmd(
        &[
            "test harness::tests::alpha_scenario::tui ... ok",
            "test harness::tests::beta_scenario::tui ... FAILED",
            "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out",
        ],
        101,
    );

    let (stdout, stderr, code) = run_script(
        &["--backend", "tui"],
        &[
            ("REGRESS_HARNESS_FILE", harness.to_str().unwrap()),
            ("REGRESS_CMD_TUI", &cmd),
        ],
    );

    assert_ne!(
        code, 0,
        "a failing scenario must not exit 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let failed_row = table_row(&stdout, "beta_scenario", "tui")
        .unwrap_or_else(|| panic!("no table row for beta_scenario/tui:\n{stdout}"));
    assert_eq!(
        row_result(failed_row),
        "fail",
        "cargo said FAILED, so the table must say fail: {failed_row}"
    );

    let passed_row = table_row(&stdout, "alpha_scenario", "tui")
        .unwrap_or_else(|| panic!("no table row for alpha_scenario/tui:\n{stdout}"));
    assert_eq!(
        row_result(passed_row),
        "pass",
        "cargo said ok, so the table must say pass: {passed_row}"
    );

    assert_eq!(summary_field(&stdout, "verdict"), "fail", "{stdout}");
    assert_eq!(summary_field(&stdout, "failed"), "1", "{stdout}");
}

/// A scenario that ran and passed must never be reported as skipped. The
/// skip rows exist only for scenarios the name filter excluded, so a passing
/// scenario landing in that bucket turns a green run into a misleading one.
#[test]
fn a_scenario_that_ran_is_never_reported_as_skipped() {
    let scratch = Scratch::new("no_false_skip");
    let harness = scratch.harness_fixture();
    let cmd = stub_cmd(
        &[
            "test harness::tests::alpha_scenario::tui ... ok",
            "test harness::tests::alpha_scenario::tui_prod ... ok",
            "test harness::tests::beta_scenario::tui ... ok",
            "test harness::tests::beta_scenario::tui_prod ... ok",
            "test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
        ],
        0,
    );

    let (stdout, stderr, code) = run_script(
        &["--backend", "tui"],
        &[
            ("REGRESS_HARNESS_FILE", harness.to_str().unwrap()),
            ("REGRESS_CMD_TUI", &cmd),
        ],
    );

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    for (scenario, backend) in [
        ("alpha_scenario", "tui"),
        ("alpha_scenario", "tui_prod"),
        ("beta_scenario", "tui"),
        ("beta_scenario", "tui_prod"),
    ] {
        let row = table_row(&stdout, scenario, backend)
            .unwrap_or_else(|| panic!("no table row for {scenario}/{backend}:\n{stdout}"));
        assert_eq!(
            row_result(row),
            "pass",
            "{scenario} ran and passed, so its row must read pass: {row}"
        );
    }
}

/// A scenario the name filter excluded is still shown, as a skip naming the
/// filter, rather than silently omitted — otherwise `--area` narrowing looks
/// like a full run.
#[test]
fn a_filtered_out_scenario_is_shown_as_a_skip() {
    let scratch = Scratch::new("filtered_skip");
    let harness = scratch.harness_fixture();
    let cmd = stub_cmd(
        &[
            "test harness::tests::alpha_scenario::tui ... ok",
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out",
        ],
        0,
    );

    let (stdout, stderr, code) = run_script(
        &["--backend", "tui"],
        &[
            ("REGRESS_HARNESS_FILE", harness.to_str().unwrap()),
            ("REGRESS_CMD_TUI", &cmd),
        ],
    );

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let row = table_row(&stdout, "beta_scenario", "tui")
        .unwrap_or_else(|| panic!("no table row for the un-run beta_scenario:\n{stdout}"));
    assert!(
        row_result(row).starts_with("skip"),
        "a scenario that produced no cargo line must read as a skip: {row}"
    );
}

// ── The summary must agree with cargo's own verdict ──────────────────────────

/// `cargo test` prints one `test result:` line per test binary, and only the
/// binary holding the harness tests reports a nonzero count. The totals must
/// sum all of them: reading just the last line sees `0 passed; 0 failed` and
/// reports a genuinely green run as a zero-test failure.
#[test]
fn totals_sum_every_test_result_line_not_just_the_last() {
    let scratch = Scratch::new("sum_results");
    let harness = scratch.harness_fixture();
    let cmd = stub_cmd(
        &[
            // The lib unittests: nothing matched here.
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
            // The binary that actually holds the harness scenarios.
            "test harness::tests::alpha_scenario::tui ... ok",
            "test harness::tests::alpha_scenario::tui_prod ... ok",
            "test harness::tests::beta_scenario::tui ... ok",
            "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out",
            // Trailing integration-test binaries, none matching the filter.
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
        ],
        0,
    );

    let (stdout, stderr, code) = run_script(
        &["--backend", "tui"],
        &[
            ("REGRESS_HARNESS_FILE", harness.to_str().unwrap()),
            ("REGRESS_CMD_TUI", &cmd),
        ],
    );

    assert_eq!(
        code, 0,
        "a green multi-binary run must exit 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(summary_field(&stdout, "passed"), "3", "{stdout}");
    assert_eq!(summary_field(&stdout, "total"), "3", "{stdout}");
    assert_eq!(summary_field(&stdout, "verdict"), "pass", "{stdout}");
    assert!(
        !stdout.contains("vacuous-green"),
        "a run with 3 passing tests must not trip the zero-test guard:\n{stdout}"
    );
}

/// The vacuous-green guard: a run where cargo executed zero tests is a
/// failure, however cleanly cargo itself exited.
#[test]
fn zero_tests_executed_is_a_failure_not_a_pass() {
    let scratch = Scratch::new("zero_tests");
    let harness = scratch.harness_fixture();
    let cmd = stub_cmd(
        &[
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 86 filtered out",
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
        ],
        0,
    );

    let (stdout, stderr, code) = run_script(
        &["--backend", "tui"],
        &[
            ("REGRESS_HARNESS_FILE", harness.to_str().unwrap()),
            ("REGRESS_CMD_TUI", &cmd),
        ],
    );

    assert_eq!(
        code, 1,
        "zero tests must fail with the documented exit 1\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("zero tests ran"),
        "the operator must be told why a cleanly-exiting run failed:\n{stdout}"
    );
    assert_eq!(summary_field(&stdout, "total"), "0", "{stdout}");
    assert_eq!(summary_field(&stdout, "verdict"), "fail", "{stdout}");
}

/// `cargo test` signals test failure with 101. A caller gating on this script
/// must be able to tell "scenarios failed" (1) from "backend unavailable" (2)
/// and "bad invocation" (3) from the exit code alone, so cargo's raw status is
/// normalised and reported as such in the summary too.
#[test]
fn exit_status_follows_the_documented_contract() {
    let scratch = Scratch::new("exit_contract");
    let harness = scratch.harness_fixture();

    // Failure -> 1, not cargo's 101.
    let cmd = stub_cmd(
        &[
            "test harness::tests::alpha_scenario::tui ... FAILED",
            "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out",
        ],
        101,
    );
    let (stdout, stderr, code) = run_script(
        &["--backend", "tui"],
        &[
            ("REGRESS_HARNESS_FILE", harness.to_str().unwrap()),
            ("REGRESS_CMD_TUI", &cmd),
        ],
    );
    assert_eq!(
        code, 1,
        "a failing run must exit 1, not cargo's raw 101\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(summary_field(&stdout, "exit"), "1", "{stdout}");

    // Backend not available on this host -> 2, and nothing ran.
    let (stdout, _stderr, code) = run_script(
        &["--backend", "gtk"],
        &[
            ("REGRESS_HARNESS_FILE", harness.to_str().unwrap()),
            ("REGRESS_OVERRIDE_GTK", "0"),
            ("REGRESS_CMD_GTK", "exit 0"),
        ],
    );
    assert_eq!(code, 2, "an unavailable backend must exit 2:\n{stdout}");
    assert!(
        !stdout.contains("REGRESS_SUMMARY"),
        "an unavailable backend must not report a run summary:\n{stdout}"
    );

    // Bad invocation -> 3.
    let (_stdout, stderr, code) = run_script(&["--backend", "bogus"], &[]);
    assert_eq!(code, 3, "an unknown backend must exit 3:\n{stderr}");
    let (_stdout, stderr, code) = run_script(&[], &[]);
    assert_eq!(code, 3, "a missing --backend must exit 3:\n{stderr}");
}

// ── The full log is kept, not thrown away ────────────────────────────────────

/// The short table is a summary, not the record. The complete cargo output
/// must still exist on disk at the printed path after the script exits, so an
/// operator can read the failure's actual assertion message.
#[test]
fn the_full_cargo_log_survives_the_run_at_the_printed_path() {
    let scratch = Scratch::new("log_survives");
    let harness = scratch.harness_fixture();
    let needle = "assertion failed: screen_contains(\"Tab 2\")";
    let cmd = stub_cmd(
        &[
            "test harness::tests::beta_scenario::tui ... FAILED",
            needle,
            "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out",
        ],
        101,
    );

    let (stdout, stderr, _code) = run_script(
        &["--backend", "tui"],
        &[
            ("REGRESS_HARNESS_FILE", harness.to_str().unwrap()),
            ("REGRESS_CMD_TUI", &cmd),
        ],
    );

    let path = logged_path(&stdout);
    let kept = std::fs::read_to_string(&path);
    // Clean up before asserting so a failure never leaves the file behind.
    let _ = std::fs::remove_file(&path);

    let kept = kept.unwrap_or_else(|e| {
        panic!(
            "log at {} must still exist after the run ({e})\nstdout:\n{stdout}\nstderr:\n{stderr}",
            path.display()
        )
    });
    assert!(
        kept.contains(needle),
        "the kept log must hold the full cargo output, including {needle:?}:\n{kept}"
    );
    assert_eq!(
        summary_field(&stdout, "log"),
        path.display().to_string(),
        "the summary line must name the same log path:\n{stdout}"
    );
}

// ── --list is backed by the same label source as the table ──────────────────

/// `--list` and the per-scenario table read one label source, so the set of
/// scenarios can never drift between "what this will run" and "what it ran".
#[test]
fn list_and_the_table_name_the_same_scenarios() {
    let scratch = Scratch::new("list_parity");
    let harness = scratch.harness_fixture();

    let (list_stdout, stderr, code) = run_script(
        &["--backend", "tui", "--list"],
        &[("REGRESS_HARNESS_FILE", harness.to_str().unwrap())],
    );
    assert_eq!(code, 0, "stdout:\n{list_stdout}\nstderr:\n{stderr}");

    let listed: Vec<&str> = list_stdout
        .lines()
        .skip(2)
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    assert_eq!(
        listed,
        vec!["alpha_scenario", "beta_scenario"],
        "--list must name exactly the fixture's labels:\n{list_stdout}"
    );

    // The same labels, with nothing run at all, must all appear in the table.
    let cmd = stub_cmd(
        &["test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out"],
        0,
    );
    let (run_stdout, _stderr, _code) = run_script(
        &["--backend", "tui"],
        &[
            ("REGRESS_HARNESS_FILE", harness.to_str().unwrap()),
            ("REGRESS_CMD_TUI", &cmd),
        ],
    );
    for scenario in listed {
        assert!(
            table_row(&run_stdout, scenario, "tui").is_some(),
            "{scenario} is listed by --list, so it must appear in the table:\n{run_stdout}"
        );
    }
}

/// The script is committed executable, so `scripts/regress --backend tui`
/// works as documented without a `bash` prefix.
#[test]
fn the_script_is_executable() {
    use std::os::unix::fs::PermissionsExt;
    let path: &Path = &script_path();
    let mode = std::fs::metadata(path)
        .expect("stat scripts/regress")
        .permissions()
        .mode();
    assert!(
        mode & 0o111 != 0,
        "scripts/regress is not executable (mode {mode:o})"
    );
}
