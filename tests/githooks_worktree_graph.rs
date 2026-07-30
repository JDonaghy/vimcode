//! Integration tests for `.githooks/` — the versioned git hooks that make the
//! graphify knowledge graph usable from a *linked worktree* (see issue #611).
//!
//! `graphify-out/` is gitignored by design (only `graphify-out/.gitignore` is
//! tracked), so `git worktree add` normally materialises an empty
//! `graphify-out/` in the new worktree, and `graphify query` — which resolves
//! `graphify-out/graph.json` strictly relative to cwd — fails there. The
//! `post-checkout` hook in `.githooks/` fixes this by symlinking the new
//! worktree's `graphify-out` at the base checkout's graph.
//!
//! These tests drive *real* `git` via `std::process::Command`: build a temp
//! "base" repo, copy in the actual `.githooks/` directory from this repo,
//! commit it with `core.hooksPath` pointed at `.githooks`, seed a fake
//! `graphify-out/graph.json`, and then exercise `git worktree add` /
//! `git checkout` and assert on the resulting filesystem state.
//!
//! Anti-vacuity: `core.hooksPath`, when relative, is resolved relative to the
//! directory a given git command actually runs in. For a worktree's *own*
//! checkouts (as opposed to the initial `worktree add`, which runs from the
//! base repo) that means `.githooks/` must exist *inside the worktree* — it
//! does, because it's a tracked directory and gets checked out there like any
//! other file. Every test that asserts "nothing happened" first confirms,
//! via `GIT_TRACE`, that the hook was actually invoked — otherwise a broken
//! hook wiring would make these tests pass for the wrong reason (see the
//! `assert_hook_ran` helper below).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A scratch directory that removes itself on drop, even if the test panics.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(label: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "vimcode_githooks_test_{}_{}_{}",
            std::process::id(),
            label,
            n
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap_or_else(|e| panic!("mkdir {:?}: {}", path, e));
        ScratchDir(path)
    }

    fn join(&self, p: &str) -> PathBuf {
        self.0.join(p)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The real `.githooks/` directory shipped in this repo — the thing under test.
fn githooks_src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".githooks")
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn git {:?} in {:?}: {}", args, dir, e))
}

fn run_ok(dir: &Path, args: &[&str]) -> std::process::Output {
    let out = run(dir, args);
    assert!(
        out.status.success(),
        "git {:?} in {:?} failed:\nstdout: {}\nstderr: {}",
        args,
        dir,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// Run `git {args}` in `dir` with `GIT_TRACE` enabled, and return (output, trace).
fn run_traced(dir: &Path, args: &[&str]) -> (std::process::Output, String) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TRACE", "1")
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn git {:?} in {:?}: {}", args, dir, e));
    let trace = String::from_utf8_lossy(&out.stderr).into_owned();
    (out, trace)
}

/// Anti-vacuity guard: assert git's own trace log shows it actually invoked
/// `hook_name`. Without this, a broken `core.hooksPath` wiring (e.g. the
/// relative path not resolving inside a worktree) would make "nothing
/// changed" assertions pass without the hook ever running.
fn assert_hook_ran(trace: &str, hook_name: &str) {
    let marker = format!("/{}", hook_name);
    let ran = trace
        .lines()
        .any(|line| line.contains("run_command:") && line.contains(&marker));
    assert!(
        ran,
        "expected git trace to show `{}` actually running (not just present on disk), got:\n{}",
        hook_name, trace
    );
}

/// Copy the real `.githooks/` into `dest/.githooks`, force hook scripts (not
/// `_lib.sh`) to mode 0o755, commit a `graphify-out/.gitignore` stub (mirroring
/// the real repo, where the graph itself is gitignored but its `.gitignore`
/// is tracked), and turn on `core.hooksPath`.
fn init_base_repo(dest: &Path) {
    run_ok(dest, &["init", "-q", "-b", "main"]);
    run_ok(dest, &["config", "user.email", "test@example.com"]);
    run_ok(dest, &["config", "user.name", "Test"]);

    let hooks_dir = dest.join(".githooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    for entry in fs::read_dir(githooks_src_dir()).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let src = entry.path();
        let dst = hooks_dir.join(&name);
        fs::copy(&src, &dst).unwrap_or_else(|e| panic!("copy {:?} -> {:?}: {}", src, dst, e));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if name == "_lib.sh" { 0o644 } else { 0o755 };
            fs::set_permissions(&dst, fs::Permissions::from_mode(mode)).unwrap();
        }
    }

    let graphify_out = dest.join("graphify-out");
    fs::create_dir_all(&graphify_out).unwrap();
    fs::write(graphify_out.join(".gitignore"), "*\n!.gitignore\n").unwrap();

    run_ok(dest, &["add", ".githooks", "graphify-out/.gitignore"]);
    run_ok(dest, &["commit", "-q", "-m", "init"]);
    run_ok(dest, &["config", "core.hooksPath", ".githooks"]);
}

// ── 1. `worktree add` produces the symlink ──────────────────────────────────

#[test]
fn worktree_add_creates_symlink_to_base_graph() {
    let root = ScratchDir::new("wt_add_ok");
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    init_base_repo(&base);

    // Seed a "real" graph in the base checkout, as if graphify had built it.
    fs::write(
        base.join("graphify-out/graph.json"),
        r#"{"fake":"basegraph"}"#,
    )
    .unwrap();

    let wt = root.join("wt");
    let (out, trace) = run_traced(
        &base,
        &[
            "worktree",
            "add",
            wt.to_str().unwrap(),
            "-b",
            "feature-1",
        ],
    );
    assert!(out.status.success(), "worktree add failed: {}", trace);
    assert_hook_ran(&trace, "post-checkout");

    let linked = wt.join("graphify-out");
    let meta = fs::symlink_metadata(&linked).expect("graphify-out should exist in worktree");
    assert!(
        meta.file_type().is_symlink(),
        "expected graphify-out to be a symlink, got {:?}",
        meta.file_type()
    );

    // It must resolve to the base checkout's graph, with matching content.
    let resolved = fs::canonicalize(&linked).unwrap();
    let base_graphify_out = fs::canonicalize(base.join("graphify-out")).unwrap();
    assert_eq!(resolved, base_graphify_out);

    let content = fs::read_to_string(linked.join("graph.json")).unwrap();
    assert_eq!(content, r#"{"fake":"basegraph"}"#);
}

// ── 2. no symlink when the base checkout has no graph.json ─────────────────

#[test]
fn no_symlink_when_base_has_no_graph() {
    let root = ScratchDir::new("wt_no_graph");
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    init_base_repo(&base);
    // Deliberately do NOT seed graphify-out/graph.json.

    let wt = root.join("wt");
    let (out, trace) = run_traced(
        &base,
        &["worktree", "add", wt.to_str().unwrap(), "-b", "feature-2"],
    );
    assert!(out.status.success(), "worktree add failed: {}", trace);
    // Anti-vacuity: prove the hook actually ran before asserting it did nothing.
    assert_hook_ran(&trace, "post-checkout");

    let linked = wt.join("graphify-out");
    let meta = fs::symlink_metadata(&linked).expect("graphify-out should exist in worktree");
    assert!(
        !meta.file_type().is_symlink(),
        "must not create a dangling symlink when the base has no graph.json"
    );
    assert!(meta.is_dir(), "graphify-out should still be a plain dir");
    // The stub materialised by `worktree add` itself (its tracked .gitignore).
    assert!(linked.join(".gitignore").is_file());
    assert!(!linked.join("graph.json").exists());
}

// ── 3. a real graph already in the worktree is never clobbered ─────────────

#[test]
fn real_graph_in_worktree_is_never_clobbered() {
    let root = ScratchDir::new("wt_no_clobber");
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    init_base_repo(&base);
    fs::write(
        base.join("graphify-out/graph.json"),
        r#"{"fake":"basegraph"}"#,
    )
    .unwrap();

    let wt = root.join("wt");
    run_ok(
        &base,
        &["worktree", "add", wt.to_str().unwrap(), "-b", "feature-3"],
    );
    // Sanity: the symlink was created by test 1's mechanism.
    assert!(fs::symlink_metadata(wt.join("graphify-out"))
        .unwrap()
        .file_type()
        .is_symlink());

    // Simulate a real, worktree-local graph having been built here by hand,
    // replacing the symlink with an actual directory + graph.json.
    fs::remove_file(wt.join("graphify-out")).unwrap();
    fs::create_dir_all(wt.join("graphify-out")).unwrap();
    fs::write(
        wt.join("graphify-out/graph.json"),
        r#"{"fake":"worktree-local-graph"}"#,
    )
    .unwrap();

    // Trigger another post-checkout run *inside* the worktree (a plain
    // `worktree add` only fires the hook once, from the base repo — so this
    // is also what proves the hook is reachable from in-worktree checkouts,
    // where the relative core.hooksPath resolves against the worktree itself).
    let (out, trace) = run_traced(&wt, &["checkout", "-b", "other-branch"]);
    assert!(out.status.success(), "checkout in worktree failed: {}", trace);
    assert_hook_ran(&trace, "post-checkout");

    let linked = wt.join("graphify-out");
    let meta = fs::symlink_metadata(&linked).unwrap();
    assert!(
        !meta.file_type().is_symlink(),
        "a real worktree-local graph must never be replaced by a symlink"
    );
    let content = fs::read_to_string(linked.join("graph.json")).unwrap();
    assert_eq!(
        content, r#"{"fake":"worktree-local-graph"}"#,
        "worktree-local graph content must be untouched"
    );
}

// ── 4. checked-in hooks are mode 100755 (except _lib.sh) ───────────────────

#[test]
fn hooks_are_committed_as_executable() {
    let root = ScratchDir::new("mode_check");
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    init_base_repo(&base);

    let out = run_ok(&base, &["ls-files", "-s", ".githooks"]);
    let listing = String::from_utf8_lossy(&out.stdout);

    let mut seen = Vec::new();
    for line in listing.lines() {
        // Format: "<mode> <sha> <stage>\t<path>"
        let mode = line.split_whitespace().next().unwrap_or("");
        let path = line.split('\t').nth(1).unwrap_or("");
        let name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        seen.push(name.to_string());
        if name == "_lib.sh" {
            assert_eq!(mode, "100644", "_lib.sh is sourced, not run — should not be executable");
        } else if !name.is_empty() {
            assert_eq!(
                mode, "100755",
                "hook `{}` must be committed as mode 100755 — git silently ignores non-executable hooks",
                name
            );
        }
    }
    // Make sure we actually checked something (not a vacuous empty loop).
    assert!(
        seen.contains(&"post-checkout".to_string())
            && seen.contains(&"post-commit".to_string())
            && seen.contains(&"post-merge".to_string())
            && seen.contains(&"_lib.sh".to_string()),
        "expected all four .githooks files, got: {:?}",
        seen
    );
}

// ── 5. post-commit / post-merge shims are reachable and don't error ────────

#[test]
fn post_commit_and_post_merge_shims_run_in_linked_worktree() {
    let root = ScratchDir::new("commit_merge_shims");
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    init_base_repo(&base);

    let wt = root.join("wt");
    run_ok(
        &base,
        &["worktree", "add", wt.to_str().unwrap(), "-b", "feature-5"],
    );

    // post-commit: committing inside the linked worktree must not error, and
    // the hook must actually be invoked (core.hooksPath resolves against the
    // worktree here, not the base repo).
    fs::write(wt.join("f.txt"), "hello\n").unwrap();
    run_ok(&wt, &["add", "f.txt"]);
    let (out, trace) = run_traced(&wt, &["commit", "-q", "-m", "wt commit"]);
    assert!(out.status.success(), "commit in worktree failed: {}", trace);
    assert_hook_ran(&trace, "post-commit");

    // post-merge: merging inside the linked worktree must not error either.
    // Give `other-line` a real extra commit so the merge is non-trivial.
    run_ok(&base, &["checkout", "-q", "-b", "other-line"]);
    fs::write(base.join("o.txt"), "other\n").unwrap();
    run_ok(&base, &["add", "o.txt"]);
    run_ok(&base, &["commit", "-q", "-m", "other commit"]);
    run_ok(&base, &["checkout", "-q", "main"]);

    run_ok(&wt, &["fetch", "-q", base.to_str().unwrap(), "other-line"]);
    let (out, trace) = run_traced(&wt, &["merge", "-q", "FETCH_HEAD"]);
    assert!(out.status.success(), "merge in worktree failed: {}", trace);
    assert_hook_ran(&trace, "post-merge");
}
