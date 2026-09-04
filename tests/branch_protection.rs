//! Integration tests for the branch-protection config (#796).
//!
//! CI ran on every PR and push to `main`/`develop`, but nothing *enforced* it:
//! `develop` — the default branch, where all work lands — had no protection
//! object at all, and `main` had one with no `required_status_checks` key. A
//! PR with red checks was mergeable by clicking the button. `#796` closes that
//! by declaring the settings in `.github/branch-protection.json` and applying
//! them with `scripts/apply-branch-protection.sh`.
//!
//! The settings are versioned rather than living only in the repo's web UI so
//! that this test can exist. It guards the one failure mode most likely to
//! wedge the merge queue: **GitHub matches required status checks by name**,
//! and a required context naming a job that no longer exists is never reported
//! — it stays forever-pending and blocks *every* pull request. So renaming a
//! job in `.github/workflows/ci.yml` without updating the config must fail the
//! test suite, loudly, on the branch that renames it.
//!
//! `required_contexts_match_ci_job_names` is the assertion that does that.
//! `dry_run_*` drive the real script via `std::process::Command` and assert on
//! the JSON it actually emits — the same body it PUTs to the GitHub API — so a
//! change to the rendering logic is caught too, not just the config data.
//!
//! Unix-only: the script is bash, and the tests shell out to it.
#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn script_path() -> PathBuf {
    repo_root().join("scripts/apply-branch-protection.sh")
}

fn config_path() -> PathBuf {
    repo_root().join(".github/branch-protection.json")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

fn config() -> serde_json::Value {
    serde_json::from_str(&read(&config_path()))
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", config_path().display()))
}

fn strings(value: &serde_json::Value) -> BTreeSet<String> {
    value
        .as_array()
        .expect("expected a JSON array")
        .iter()
        .map(|v| v.as_str().expect("expected a JSON string").to_string())
        .collect()
}

/// Whether a tool the tests shell out to is present.
///
/// Missing on a dev machine → skip (the test prints why). Missing under CI →
/// hard fail, mirroring `tests/nvim_conformance.rs` (#795): a silent skip in
/// CI is how a gate quietly stops being a gate.
fn require_tool(name: &str) -> bool {
    let found = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !found {
        assert!(
            std::env::var_os("CI").is_none(),
            "{name} is required to run the #796 branch-protection tests under CI"
        );
        eprintln!("skipping: {name} not found on PATH");
    }
    found
}

/// Extract the display name of every job in a GitHub Actions workflow.
///
/// Deliberately a tiny hand-rolled scan rather than a new YAML dependency: the
/// shape it needs to understand is two levels deep and fully under our control.
/// Job keys sit at 2-space indent under `jobs:`; a job's `name:` sits at
/// 4-space indent (step names are `    - name:`, which does not match). A job
/// with no explicit `name:` is reported by GitHub under its key, which is what
/// this returns in that case.
fn ci_job_names(workflow: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut in_jobs = false;
    let mut pending_key: Option<String> = None;

    for line in workflow.lines() {
        if line.starts_with("jobs:") {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        // A non-indented, non-blank, non-comment line ends the `jobs:` block.
        if !line.starts_with(' ') && !line.trim().is_empty() && !line.trim_start().starts_with('#')
        {
            break;
        }

        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if indent == 2 {
            // New job key — flush the previous one if it never declared a name.
            if let Some(key) = pending_key.take() {
                names.insert(key);
            }
            if let Some(key) = trimmed.strip_suffix(':') {
                pending_key = Some(key.to_string());
            }
        } else if indent == 4 {
            if let Some(name) = trimmed.strip_prefix("name:") {
                if pending_key.take().is_some() {
                    names.insert(name.trim().trim_matches(['"', '\'']).to_string());
                }
            }
        }
    }
    if let Some(key) = pending_key {
        names.insert(key);
    }
    names
}

/// Run the script and return (stdout, stderr, success).
fn run_script(args: &[&str], extra_path: Option<&Path>) -> (String, String, bool) {
    let mut cmd = Command::new("bash");
    cmd.arg(script_path()).args(args);
    if let Some(dir) = extra_path {
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}:{}", dir.display(), path));
    }
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", script_path().display()));
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

/// Split `--dry-run` output into (branch, payload) pairs.
fn dry_run_payloads() -> Vec<(String, serde_json::Value)> {
    let (stdout, stderr, ok) = run_script(&["--dry-run"], None);
    assert!(ok, "--dry-run failed\nstdout:\n{stdout}\nstderr:\n{stderr}");

    let mut out = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("=== ") {
            if let Some((branch, body)) = current.take() {
                out.push((branch, body));
            }
            current = Some((
                rest.trim_end_matches(" ===").trim().to_string(),
                String::new(),
            ));
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some((branch, body)) = current.take() {
        out.push((branch, body));
    }

    out.into_iter()
        .map(|(branch, body)| {
            let json = serde_json::from_str(&body)
                .unwrap_or_else(|e| panic!("payload for {branch} is not valid JSON: {e}\n{body}"));
            (branch, json)
        })
        .collect()
}

/// THE test: every required status check must name a job that CI actually
/// reports, and every CI job must be required. Rename a job in `ci.yml` without
/// updating `.github/branch-protection.json` and this fails — instead of every
/// subsequent PR sitting forever-pending on a check that will never report.
#[test]
fn required_contexts_match_ci_job_names() {
    let workflow = read(&repo_root().join(".github/workflows/ci.yml"));
    let jobs = ci_job_names(&workflow);

    // Anti-vacuity: a parser that silently returned nothing would make the
    // equality below pass against an empty config. CI has two lanes (#645) —
    // the `--no-default-features` one and the GUI one — and both are load
    // bearing, so anything other than two named jobs means the scan drifted
    // from the file it is meant to read.
    assert_eq!(
        jobs.len(),
        2,
        "expected 2 jobs parsed from .github/workflows/ci.yml, got {jobs:?}"
    );
    assert!(
        jobs.iter().all(|n| !n.trim().is_empty()),
        "parsed a blank job name from ci.yml: {jobs:?}"
    );

    let required = strings(&config()["required_contexts"]);
    assert_eq!(
        required, jobs,
        "required contexts in .github/branch-protection.json must exactly match \
         the job `name:`s in .github/workflows/ci.yml.\n  required: {required:?}\n  ci jobs:  {jobs:?}\n\
         A required context naming a job CI never reports blocks EVERY pull request; \
         a CI job that is not required is advisory and gates nothing (#796)."
    );
}

/// Both branches named in `CLAUDE.md`'s workflow are protected — `develop`
/// (default, where every agent branch lands) and `main` (release).
#[test]
fn config_protects_develop_and_main() {
    let cfg = config();
    let branches: BTreeSet<String> = cfg["branches"]
        .as_object()
        .expect("branches must be an object")
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        branches,
        BTreeSet::from(["develop".to_string(), "main".to_string()]),
        "#796 protects exactly develop and main"
    );
}

/// Drive the real script and assert on the JSON body it would PUT to the
/// GitHub API — including the `strict` split that is the whole point of having
/// per-branch settings: `main` (release-only traffic) requires the branch be up
/// to date with base; `develop` does not, because strict mode there would force
/// a rebase and full re-run on every open branch each time another landed.
#[test]
fn dry_run_renders_expected_protection_payloads() {
    if !require_tool("python3") || !require_tool("bash") {
        return;
    }
    let payloads = dry_run_payloads();
    let branches: Vec<&str> = payloads.iter().map(|(b, _)| b.as_str()).collect();
    assert_eq!(
        branches.len(),
        2,
        "expected one payload per protected branch, got {branches:?}"
    );

    let contexts = strings(&config()["required_contexts"]);
    for (branch, payload) in &payloads {
        let checks = &payload["required_status_checks"];
        assert_eq!(
            strings(&checks["contexts"]),
            contexts,
            "{branch}: payload contexts must come from the config"
        );

        let want_strict = branch == "main";
        assert_eq!(
            checks["strict"].as_bool(),
            Some(want_strict),
            "{branch}: strict should be {want_strict} (#796)"
        );

        // enforce_admins:false is decision (c) — the gate binds PRs and
        // non-admin pushes while the owner keeps a deliberate escape hatch.
        assert_eq!(payload["enforce_admins"].as_bool(), Some(false));
        // The PUT is a full replacement: omitting these would silently drop
        // the force-push/deletion blocks `main` already had.
        assert_eq!(payload["allow_force_pushes"].as_bool(), Some(false));
        assert_eq!(payload["allow_deletions"].as_bool(), Some(false));
        assert!(payload["required_pull_request_reviews"].is_null());
        assert!(payload["restrictions"].is_null());
    }
}

/// `--dry-run` must be inert: no `gh`, no network, no writes. Enforced by
/// putting a booby-trapped `gh` first on PATH that records any invocation.
#[test]
fn dry_run_never_invokes_gh() {
    if !require_tool("python3") || !require_tool("bash") {
        return;
    }
    let dir = std::env::temp_dir().join(format!("vimcode_bp_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    let marker = dir.join("gh-was-called");
    let fake_gh = dir.join("gh");
    std::fs::write(
        &fake_gh,
        format!("#!/bin/sh\ntouch {}\nexit 1\n", marker.display()),
    )
    .expect("write fake gh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_gh, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake gh");
    }

    let (stdout, stderr, ok) = run_script(&["--dry-run"], Some(&dir));
    let called = marker.exists();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(ok, "--dry-run failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        !called,
        "--dry-run invoked gh; it must be previewable offline and without auth"
    );
    assert!(
        stdout.contains("required_status_checks"),
        "--dry-run printed no payload:\n{stdout}"
    );
}

/// The script must be executable in the checkout — `scripts/apply-branch-protection.sh`
/// is documented as a directly runnable command in CLAUDE.md.
#[test]
fn script_is_executable() {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(script_path())
        .expect("script must exist")
        .permissions()
        .mode();
    assert!(
        mode & 0o111 != 0,
        "scripts/apply-branch-protection.sh is not executable (mode {mode:o})"
    );
}

#[test]
fn ci_job_name_parser_handles_unnamed_jobs_and_step_names() {
    // A job without `name:` is reported by its key; step `name:`s (deeper
    // indent, list items) must not be mistaken for job names.
    let yaml = "\
name: CI

on:
  push:
    branches: [ main ]

jobs:
  test:
    name: Test (Linux, headless)
    runs-on: ubuntu-24.04
    steps:
    - name: Install Rust toolchain
      run: true
  lint:
    runs-on: ubuntu-24.04
    steps:
    - name: Clippy
      run: true
";
    assert_eq!(
        ci_job_names(yaml),
        BTreeSet::from(["Test (Linux, headless)".to_string(), "lint".to_string()])
    );
}
