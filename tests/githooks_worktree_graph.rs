//! Integration tests for `.githooks/` — the versioned git hooks that make the
//! graphify knowledge graph usable from a *linked worktree* (see issue #611).
//!
//! `graphify-out/` is gitignored by design (only `graphify-out/.gitignore` is
//! tracked), so `git worktree add` normally materialises an empty
//! `graphify-out/` in the new worktree, and `graphify query` — which resolves
//! `graphify-out/graph.json` strictly relative to cwd — fails there. The
//! `post-checkout` hook in `.githooks/` fixes this by symlinking each *entry*
//! of the base checkout's graph into the new worktree's `graphify-out/`.
//!
//! It deliberately does NOT replace `graphify-out/` itself with a symlink.
//! That was the shape of the original port, and it was a live incident
//! (claude-coordinator#1617): `git worktree add` checks out the tracked
//! `graphify-out/.gitignore`, so `rm -rf graphify-out && ln -sfn ...` deleted
//! a tracked file out from under git, leaving every fresh worktree with a
//! deleted `.gitignore` plus an untracked, machine-local, absolute-path
//! symlink — both of which the worktree-rescue sweep then committed onto
//! unrelated worker branches. `graphify-out/.gitignore` is `*` / `!.gitignore`,
//! so entries placed *inside* the directory are invisible to git for free,
//! provided that tracked file survives. `worktree_add_leaves_git_status_empty`
//! below is the assertion whose absence let that ship; treat it as the
//! acceptance bar for any future change to this hook.
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
//!
//! These tests drive real `sh`-executed hooks, real symlinks, and POSIX file
//! modes, so the whole module is unix-only — it skips cleanly (no tests
//! collected) rather than failing to compile on a Windows CI leg.
#![cfg(unix)]

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
///
/// The chmod exists purely so the *behavioral* tests below (symlink
/// creation, no-clobber, shim reachability) don't depend on this checkout's
/// on-disk executable bits — they only care that the hooks *run*. It is
/// deliberately NOT used as evidence for the mode-regression test: that test
/// (`hooks_are_committed_as_executable`) reads `CARGO_MANIFEST_DIR`'s own git
/// index instead, since asserting against a mode this function just forced
/// on would be circular and could never catch a real regression.
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
        use std::os::unix::fs::PermissionsExt;
        let mode = if name == "_lib.sh" { 0o644 } else { 0o755 };
        fs::set_permissions(&dst, fs::Permissions::from_mode(mode)).unwrap();
    }

    let graphify_out = dest.join("graphify-out");
    fs::create_dir_all(&graphify_out).unwrap();
    fs::write(graphify_out.join(".gitignore"), "*\n!.gitignore\n").unwrap();

    run_ok(dest, &["add", ".githooks", "graphify-out/.gitignore"]);
    run_ok(dest, &["commit", "-q", "-m", "init"]);
    run_ok(dest, &["config", "core.hooksPath", ".githooks"]);
}

/// Seed a "real" graph in a base checkout, as if graphify had built it:
/// `graph.json` plus a sibling file and a subdirectory, so the per-entry
/// linking loop is exercised on more than one entry kind.
fn seed_base_graph(base: &Path) {
    let out = base.join("graphify-out");
    fs::write(out.join("graph.json"), r#"{"fake":"basegraph"}"#).unwrap();
    fs::write(out.join("manifest.json"), r#"{"fake":"manifest"}"#).unwrap();
    fs::create_dir_all(out.join("cache")).unwrap();
    fs::write(out.join("cache/entry"), "cached\n").unwrap();
}

fn porcelain_status(dir: &Path) -> String {
    let out = run_ok(dir, &["status", "--porcelain"]);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// ── 1. `worktree add` links the graph *contents*, keeping the directory ─────

#[test]
fn worktree_add_links_base_graph_contents_into_worktree() {
    let root = ScratchDir::new("wt_add_ok");
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    init_base_repo(&base);
    seed_base_graph(&base);

    let wt = root.join("wt");
    let (out, trace) = run_traced(
        &base,
        &["worktree", "add", wt.to_str().unwrap(), "-b", "feature-1"],
    );
    assert!(out.status.success(), "worktree add failed: {}", trace);
    assert_hook_ran(&trace, "post-checkout");

    // The progress message the hook prints — kept in sync with the hook so a
    // silent behaviour change can't slip past this test. Checked across both
    // streams: git routes hook output to stderr for some subcommands.
    let printed = format!("{}{}", String::from_utf8_lossy(&out.stdout), trace);
    assert!(
        printed.contains("[graphify] linked graphify-out"),
        "expected the hook's progress message in its output, got:\n{}",
        printed
    );

    // claude-coordinator#1617: `graphify-out/` itself must stay a real
    // directory. Replacing it with a symlink deletes the tracked
    // `graphify-out/.gitignore` and pollutes the worker's branch.
    let dir = wt.join("graphify-out");
    let meta = fs::symlink_metadata(&dir).expect("graphify-out should exist in worktree");
    assert!(
        !meta.file_type().is_symlink() && meta.is_dir(),
        "graphify-out/ must stay a real directory, not become a symlink (got {:?})",
        meta.file_type()
    );

    // Each entry of the base graph is linked *into* that directory, and
    // resolves back to the base checkout's copy with matching content.
    for name in ["graph.json", "manifest.json", "cache"] {
        let entry = dir.join(name);
        assert!(
            fs::symlink_metadata(&entry)
                .unwrap_or_else(|e| panic!("graphify-out/{} missing: {}", name, e))
                .file_type()
                .is_symlink(),
            "graphify-out/{} should be a symlink into the base graph",
            name
        );
        assert_eq!(
            fs::canonicalize(&entry).unwrap(),
            fs::canonicalize(base.join("graphify-out").join(name)).unwrap(),
            "graphify-out/{} should resolve to the base checkout's copy",
            name
        );
    }

    // And it is actually usable — the graph reads through the link.
    let content = fs::read_to_string(dir.join("graph.json")).unwrap();
    assert_eq!(content, r#"{"fake":"basegraph"}"#);
    assert_eq!(
        fs::read_to_string(dir.join("cache/entry")).unwrap(),
        "cached\n"
    );
}

// ── 1b. the acceptance bar: a fresh linked worktree is `git status` clean ───
//
// This is the assertion whose absence let claude-coordinator#1617 ship. The
// original bug showed up here as a *deleted* tracked `graphify-out/.gitignore`
// plus a *new untracked* absolute-path symlink — both invisible to any check
// that only looks at what the link points to.

#[test]
fn worktree_add_leaves_git_status_empty() {
    let root = ScratchDir::new("wt_status_clean");
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    init_base_repo(&base);
    seed_base_graph(&base);

    let wt = root.join("wt");
    let (out, trace) = run_traced(
        &base,
        &["worktree", "add", wt.to_str().unwrap(), "-b", "feature-1b"],
    );
    assert!(out.status.success(), "worktree add failed: {}", trace);
    assert_hook_ran(&trace, "post-checkout");

    let status = porcelain_status(&wt);
    assert!(
        status.is_empty(),
        "a fresh linked worktree must be git-clean after the hook runs, got:\n{}",
        status
    );

    // Same after the hook re-fires on an in-worktree checkout (the idempotent
    // re-link path, which `ln -sfn`s over links it already created).
    let (out, trace) = run_traced(&wt, &["checkout", "-q", "-b", "feature-1b-2"]);
    assert!(out.status.success(), "checkout in worktree failed: {}", trace);
    assert_hook_ran(&trace, "post-checkout");
    let status = porcelain_status(&wt);
    assert!(
        status.is_empty(),
        "re-firing the hook must not dirty the worktree, got:\n{}",
        status
    );
}

// ── 1c. the tracked `.gitignore` survives, unshadowed ──────────────────────

#[test]
fn worktree_add_preserves_the_tracked_gitignore() {
    let root = ScratchDir::new("wt_keeps_gitignore");
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    init_base_repo(&base);
    seed_base_graph(&base);

    let wt = root.join("wt");
    run_ok(
        &base,
        &["worktree", "add", wt.to_str().unwrap(), "-b", "feature-1c"],
    );

    // Anti-vacuity: check the *parent* first. Under the old hook this test
    // passed for the wrong reason — `wt/graphify-out/.gitignore` resolved
    // through the directory symlink to the base checkout's real file, and
    // `ls-files` reads the (untouched) index either way. Both assertions
    // below are only meaningful once we know we're looking at the worktree's
    // own checked-out copy.
    let dir = wt.join("graphify-out");
    assert!(
        !fs::symlink_metadata(&dir).unwrap().file_type().is_symlink(),
        "graphify-out/ is a symlink — the .gitignore assertions below would \
         be inspecting the base checkout's file, not the worktree's"
    );

    // The tracked file must still be a real file — not deleted, and not
    // shadowed by a symlink to the base checkout's copy. It is the whole
    // reason the linked entries are invisible to git.
    let gitignore = wt.join("graphify-out/.gitignore");
    let meta = fs::symlink_metadata(&gitignore).expect("graphify-out/.gitignore should exist");
    assert!(
        !meta.file_type().is_symlink() && meta.is_file(),
        "graphify-out/.gitignore must stay a real tracked file, got {:?}",
        meta.file_type()
    );
    assert!(fs::read_to_string(&gitignore).unwrap().contains("!.gitignore"));

    let tracked = run_ok(&wt, &["ls-files", "graphify-out/.gitignore"]);
    assert_eq!(
        String::from_utf8_lossy(&tracked.stdout).trim(),
        "graphify-out/.gitignore",
        "graphify-out/.gitignore must still be tracked in the worktree"
    );
    // ...and git agrees it is present, not deleted from the work tree.
    let status = run_ok(&wt, &["status", "--porcelain", "--", "graphify-out/.gitignore"]);
    assert!(
        String::from_utf8_lossy(&status.stdout).is_empty(),
        "graphify-out/.gitignore must be unmodified, got:\n{}",
        String::from_utf8_lossy(&status.stdout)
    );
    assert!(wt.join("graphify-out/graph.json").is_file());
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

    let dir = wt.join("graphify-out");
    let meta = fs::symlink_metadata(&dir).expect("graphify-out should exist in worktree");
    assert!(
        !meta.file_type().is_symlink(),
        "must not replace graphify-out with a symlink"
    );
    assert!(meta.is_dir(), "graphify-out should still be a plain dir");
    // The stub materialised by `worktree add` itself (its tracked .gitignore).
    assert!(dir.join(".gitignore").is_file());
    // No dangling link left behind for a graph that does not exist.
    assert!(!dir.join("graph.json").exists());
    assert!(fs::symlink_metadata(dir.join("graph.json")).is_err());

    // And nothing half-linked dirtied the worktree.
    let status = porcelain_status(&wt);
    assert!(
        status.is_empty(),
        "worktree must stay git-clean when there is no base graph, got:\n{}",
        status
    );
}

// ── 3. a real graph already in the worktree is never clobbered ─────────────

#[test]
fn real_graph_in_worktree_is_never_clobbered() {
    let root = ScratchDir::new("wt_no_clobber");
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    init_base_repo(&base);
    seed_base_graph(&base);

    let wt = root.join("wt");
    run_ok(
        &base,
        &["worktree", "add", wt.to_str().unwrap(), "-b", "feature-3"],
    );
    // Sanity: the link was created by test 1's mechanism.
    assert!(fs::symlink_metadata(wt.join("graphify-out/graph.json"))
        .unwrap()
        .file_type()
        .is_symlink());

    // Simulate a real, worktree-local graph having been built here by hand,
    // replacing the linked graph.json with an actual file.
    fs::remove_file(wt.join("graphify-out/graph.json")).unwrap();
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

    let dir = wt.join("graphify-out");
    assert!(
        !fs::symlink_metadata(&dir).unwrap().file_type().is_symlink(),
        "graphify-out/ must stay a real directory"
    );
    assert!(
        !fs::symlink_metadata(dir.join("graph.json"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "a real worktree-local graph must never be replaced by a symlink"
    );
    let content = fs::read_to_string(dir.join("graph.json")).unwrap();
    assert_eq!(
        content, r#"{"fake":"worktree-local-graph"}"#,
        "worktree-local graph content must be untouched"
    );
}

// ── 3b. removing the worktree must never reach into the base checkout ──────
//
// The per-entry links include a *directory* symlink (`cache/`). Both
// `git worktree remove` and the coordinator's rmtree cleanup sweep unlink
// directory symlinks rather than recursing into them — but only if we never
// hand them a symlink they'd follow. This pins that invariant.

#[test]
fn worktree_remove_leaves_the_base_graph_intact() {
    let root = ScratchDir::new("wt_remove_safe");
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    init_base_repo(&base);
    seed_base_graph(&base);

    let wt = root.join("wt");
    run_ok(
        &base,
        &["worktree", "add", wt.to_str().unwrap(), "-b", "feature-3b"],
    );
    assert!(wt.join("graphify-out/cache/entry").is_file());

    run_ok(&base, &["worktree", "remove", "--force", wt.to_str().unwrap()]);

    let out = base.join("graphify-out");
    assert!(
        !fs::symlink_metadata(&out).unwrap().file_type().is_symlink(),
        "the base checkout's graphify-out/ must still be a real directory"
    );
    assert!(out.join("graph.json").is_file(), "base graph.json was destroyed");
    assert!(
        out.join("cache/entry").is_file(),
        "base graph cache/ was destroyed through the directory symlink"
    );
    assert!(out.join(".gitignore").is_file());
    let status = porcelain_status(&base);
    assert!(
        status.is_empty(),
        "the base checkout must stay git-clean after worktree removal, got:\n{}",
        status
    );
}

// ── 4. checked-in hooks are mode 100755 (except _lib.sh) ───────────────────
//
// This asserts against the *real* repo's own git index (CARGO_MANIFEST_DIR),
// not a scratch copy: `init_base_repo` deliberately force-chmods its copied
// hooks to 0o755/0o644 (via `fs::set_permissions`) so the *behavioral* tests
// above don't depend on this checkout's on-disk mode bits (which may be
// mangled by tarball extraction, an editor, etc.) — but that same chmod would
// make a mode check against the scratch repo vacuous, since it always
// re-derives the mode it's about to assert on rather than reading what's
// actually committed. Querying `CARGO_MANIFEST_DIR` directly is the only way
// this test can catch a hook silently losing its executable bit in a real
// commit.

#[test]
fn hooks_are_committed_as_executable() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let out = run_ok(&repo_root, &["ls-files", "-s", ".githooks"]);
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

// ── 5. post-commit / post-merge shims: skip in a linked worktree, chain in the main one ──

/// Install a fake "machine-local" hook — the kind graphify itself installs
/// into `$GIT_COMMON_DIR/hooks/`, and which `.githooks/post-commit` and
/// `.githooks/post-merge` hand off to via `gfy_chain` — that appends a line
/// to `marker` each time it runs.
fn write_fake_local_hook(common_dir: &Path, name: &str, marker: &Path) {
    let hooks_dir = common_dir.join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    let dst = hooks_dir.join(name);
    fs::write(
        &dst,
        format!("#!/bin/sh\necho ran >> \"{}\"\n", marker.to_str().unwrap()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn post_commit_and_post_merge_shims_skip_in_linked_worktree_and_chain_in_main() {
    let root = ScratchDir::new("commit_merge_shims");
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    init_base_repo(&base);

    // Install fake machine-local hooks in the *base* repo's common git dir.
    // Without these, "committing in the worktree didn't touch anything"
    // can't distinguish "skipped because this is a linked worktree" from
    // "chained to a machine-local hook that was never installed" — both
    // look identical if $GIT_COMMON_DIR/hooks/ is empty. Installing a real
    // one, and separately proving it *does* fire from the main worktree,
    // pins down the actual skip-vs-chain branch.
    let common_dir = base.join(".git");
    let commit_marker = root.join("local-post-commit-marker");
    let merge_marker = root.join("local-post-merge-marker");
    write_fake_local_hook(&common_dir, "post-commit", &commit_marker);
    write_fake_local_hook(&common_dir, "post-merge", &merge_marker);

    let wt = root.join("wt");
    run_ok(
        &base,
        &["worktree", "add", wt.to_str().unwrap(), "-b", "feature-5"],
    );

    // ── post-commit, linked worktree: shim runs but must NOT chain ──
    fs::write(wt.join("f.txt"), "hello\n").unwrap();
    run_ok(&wt, &["add", "f.txt"]);
    let (out, trace) = run_traced(&wt, &["commit", "-q", "-m", "wt commit"]);
    assert!(out.status.success(), "commit in worktree failed: {}", trace);
    assert_hook_ran(&trace, "post-commit");
    assert!(
        !commit_marker.exists(),
        "post-commit shim must skip (not chain to the machine-local hook) inside a linked worktree"
    );

    // ── post-commit, main worktree: shim must chain to the local hook ──
    fs::write(base.join("g.txt"), "hello\n").unwrap();
    run_ok(&base, &["add", "g.txt"]);
    let (out, trace) = run_traced(&base, &["commit", "-q", "-m", "base commit"]);
    assert!(out.status.success(), "commit in base repo failed: {}", trace);
    assert_hook_ran(&trace, "post-commit");
    assert!(
        commit_marker.exists(),
        "post-commit shim must chain to the machine-local hook outside a linked worktree"
    );

    // ── post-merge, linked worktree: shim runs but must NOT chain ──
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
    assert!(
        !merge_marker.exists(),
        "post-merge shim must skip (not chain to the machine-local hook) inside a linked worktree"
    );

    // ── post-merge, main worktree: shim must chain to the local hook ──
    let (out, trace) = run_traced(&base, &["merge", "-q", "other-line"]);
    assert!(out.status.success(), "merge in base repo failed: {}", trace);
    assert_hook_ran(&trace, "post-merge");
    assert!(
        merge_marker.exists(),
        "post-merge shim must chain to the machine-local hook outside a linked worktree"
    );
}
