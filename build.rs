use std::path::{Path, PathBuf};
use std::process::Command;

/// Environment escape hatch: downgrade the quadraui pin mismatch from a hard
/// build failure to a warning, for local quadraui co-development. Documented
/// in `quadraui-pin.txt` and `docs/QUADRAUI_GUIDE.md`. Never set in CI.
const UNPIN_ENV: &str = "VIMCODE_QUADRAUI_UNPINNED";

fn main() {
    // Compile vendored tree-sitter-latex grammar (v0.3.0, language version 14)
    cc::Build::new()
        .include("vendor/tree-sitter-latex/src")
        .file("vendor/tree-sitter-latex/src/parser.c")
        .file("vendor/tree-sitter-latex/src/scanner.c")
        .warnings(false)
        .compile("tree_sitter_latex");

    // ── quadraui pin (#638) ─────────────────────────────────────────────────
    //
    // Cargo does not pin path deps, so an upstream quadraui merge can restate
    // vimcode's behaviour with zero vimcode commits (#625 / quadraui#472).
    // `quadraui-pin.txt` records the rev vimcode's snapshots and behaviour were
    // produced against; this check makes a drift loud and attributable, and
    // bakes the *resolved* rev into the binary so build/test output can name
    // it. Runs first: when the checkout is wrong, that is the message the
    // developer needs, not a downstream symptom.
    check_quadraui_pin();

    // ── Stale-quadraui guard (#587) ─────────────────────────────────────────
    //
    // Belt-and-braces behind the #638 pin above: the pinned rev is far past
    // quadraui#445, so a pin-clean checkout always passes this. It still earns
    // its keep for builds taken with `VIMCODE_QUADRAUI_UNPINNED=1`, where the
    // checkout is deliberately unverified.
    //
    // vimcode depends on quadraui via an *unpinned* path dependency to a
    // sibling checkout (see Cargo.toml). quadraui#445 taught the GTK key
    // controller to dispatch `UiEvent::Accelerator` by consulting
    // `backend.match_keypress(...)` on the live key path. That fix is a
    // *behavioural* change with no new public symbol, so a checkout that
    // predates it still *compiles* vimcode cleanly — but every GTK global
    // accelerator (Ctrl+Shift+P command palette, Ctrl+B sidebar, Ctrl+P
    // quick-open, ...) silently does nothing at runtime. That exact silent
    // failure is issue #587, and it has repeatedly slipped through as a
    // build-against-stale-quadraui trap (the artifact compiles and runs, the
    // shortcuts just never fire).
    //
    // Turn that silent runtime breakage into a loud, actionable *build*
    // failure: if the quadraui checkout's GTK key-controller source exists
    // but does not call `match_keypress`, the accelerator-dispatch fix is
    // missing and the checkout must be updated before this build is trusted.
    check_quadraui_accelerator_dispatch();
}

/// Fail the build if the sibling quadraui checkout predates quadraui#445
/// (GTK accelerator dispatch). See the call site for the full rationale.
fn check_quadraui_accelerator_dispatch() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    // Mirrors the `path = "../quadraui/quadraui"` dependency in Cargo.toml.
    let run_rs = manifest_dir.join("../quadraui/quadraui/src/gtk/run.rs");

    // Re-run this check whenever the quadraui key-controller source changes.
    println!("cargo:rerun-if-changed={}", run_rs.display());

    let Ok(src) = std::fs::read_to_string(&run_rs) else {
        // Path dep points elsewhere, or an unusual layout — don't second-guess
        // it, just skip the guard rather than risk a false failure.
        println!(
            "cargo:warning=vimcode: could not locate quadraui GTK run.rs at {} to \
             verify the #445 accelerator-dispatch fix; skipping stale-quadraui guard.",
            run_rs.display()
        );
        return;
    };

    // `match_keypress` is the Backend method the #445 fix calls from the live
    // GTK key-press closure to turn a registered Global accelerator into a
    // dispatched `UiEvent::Accelerator`. Its absence from run.rs means the
    // fix is not present and GTK global shortcuts (incl. #587's Ctrl+Shift+P)
    // will silently not fire.
    if !src.contains("match_keypress") {
        panic!(
            "\n\n\
             ┌─ STALE QUADRAUI CHECKOUT (issue #587) ─────────────────────────────┐\n\
             │ The sibling quadraui checkout used by this build predates          │\n\
             │ quadraui#445 (GTK accelerator dispatch). Building against it would  │\n\
             │ produce a binary where Ctrl+Shift+P (command palette), Ctrl+B       │\n\
             │ (sidebar), Ctrl+P (quick-open) and the other 11 global shortcuts    │\n\
             │ SILENTLY DO NOTHING — the exact #587 regression.                    │\n\
             │                                                                     │\n\
             │ Fix: update the quadraui checkout, then rebuild:                    │\n\
             │     cd ../quadraui && git checkout develop && git pull && cd -      │\n\
             │     cargo build                                                     │\n\
             │                                                                     │\n\
             │ Checked: {}\n\
             └─────────────────────────────────────────────────────────────────────┘\n",
            run_rs.display()
        );
    }
}

// ───────────────────────── quadraui pin (#638) ──────────────────────────────

/// The sibling quadraui *crate* directory, mirroring the `path =
/// "../quadraui/quadraui"` dependency in `Cargo.toml`.
fn quadraui_crate_dir() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../quadraui/quadraui")
}

/// Parse the pinned quadraui SHA out of `quadraui-pin.txt`.
///
/// Format: the first non-comment (`#`), non-blank line is a full 40-char SHA.
fn read_pinned_rev(pin_file: &Path) -> Result<String, String> {
    let src = std::fs::read_to_string(pin_file)
        .map_err(|e| format!("cannot read {}: {e}", pin_file.display()))?;

    let rev = src
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .ok_or_else(|| format!("{} contains no pinned rev", pin_file.display()))?
        .to_string();

    if rev.len() != 40 || !rev.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "{} holds {rev:?}, which is not a full 40-character git SHA",
            pin_file.display()
        ));
    }
    Ok(rev)
}

/// Run `git` inside the quadraui checkout, returning trimmed stdout on success.
fn git_in(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Verify the sibling quadraui checkout is at the rev recorded in
/// `quadraui-pin.txt`, and export the resolved rev to the crate.
///
/// See the call site in `main` and `quadraui-pin.txt` for the rationale (#638).
fn check_quadraui_pin() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let pin_file = manifest_dir.join("quadraui-pin.txt");
    // Canonicalised so the diagnostics below name a path a human can paste,
    // rather than a `..`-laden one that hides which checkout was really used.
    let crate_dir = quadraui_crate_dir();
    let crate_dir = std::fs::canonicalize(&crate_dir).unwrap_or(crate_dir);

    println!("cargo:rerun-if-changed={}", pin_file.display());
    println!("cargo:rerun-if-env-changed={UNPIN_ENV}");

    let pinned = match read_pinned_rev(&pin_file) {
        Ok(rev) => rev,
        // A malformed pin file is a vimcode bug, not a checkout problem, and it
        // would otherwise silently disable the whole guard. Fail hard.
        Err(why) => panic!("\n\nvimcode: quadraui pin file is unusable — {why}\n"),
    };
    println!("cargo:rustc-env=VIMCODE_QUADRAUI_PINNED_REV={pinned}");

    // Re-run whenever the sibling checkout moves. `HEAD` alone is not enough:
    // it only changes on checkout/detach, so a plain `git pull` that fast-
    // forwards the *current branch* would leave a stale verdict cached — and a
    // fast-forward of `develop` is exactly what happened in #625.  Watch the
    // branch's ref file and `packed-refs` too.
    if let Some(git_dir) = git_in(&crate_dir, &["rev-parse", "--absolute-git-dir"]) {
        println!("cargo:rerun-if-changed={git_dir}/HEAD");
        println!("cargo:rerun-if-changed={git_dir}/packed-refs");
        // Empty when HEAD is detached (CI, and any pin-clean checkout) — then
        // HEAD itself already holds the sha and is sufficient.
        if let Some(head_ref) = git_in(&crate_dir, &["symbolic-ref", "-q", "HEAD"]) {
            if !head_ref.is_empty() {
                println!("cargo:rerun-if-changed={git_dir}/{head_ref}");
            }
        }
    }

    let Some(resolved) = git_in(&crate_dir, &["rev-parse", "HEAD"]) else {
        // No git, or quadraui unpacked from a tarball rather than cloned. Don't
        // invent a failure, but never let the build claim to be pinned either:
        // an "unknown" rev is exactly the ambiguity #638 exists to kill.
        println!("cargo:rustc-env=VIMCODE_QUADRAUI_REV=unknown");
        println!(
            "cargo:warning=vimcode: could not resolve a git rev for the quadraui checkout at \
             {} — the #638 pin is NOT verified for this build (expected {pinned}).",
            crate_dir.display()
        );
        return;
    };
    println!("cargo:rustc-env=VIMCODE_QUADRAUI_REV={resolved}");

    if resolved == pinned {
        // Uncommitted edits in the sibling are the co-development loop working
        // as intended — allowed, but named, because they are still a way for
        // rendering to change with no vimcode commit.
        if git_in(&crate_dir, &["status", "--porcelain"]).is_some_and(|s| !s.is_empty()) {
            println!(
                "cargo:warning=vimcode: quadraui is at the pinned rev {} but its working tree is \
                 dirty — this build includes uncommitted quadraui changes.",
                &pinned[..12]
            );
        }
        return;
    }

    let unpinned = std::env::var(UNPIN_ENV).is_ok_and(|v| !v.is_empty() && v != "0");
    if unpinned {
        println!(
            "cargo:warning=vimcode: {UNPIN_ENV} is set — building against quadraui {} instead of \
             the pinned {}. Snapshot/rendering differences from this build are NOT attributable \
             to vimcode. See quadraui-pin.txt.",
            &resolved[..12],
            &pinned[..12]
        );
        return;
    }

    panic!(
        "\n\n\
         ┌─ QUADRAUI PIN MISMATCH (issue #638) ───────────────────────────────\n\
         │ vimcode is pinned to a specific quadraui commit, but the sibling\n\
         │ checkout is on a different one. quadraui is a path dependency, so\n\
         │ Cargo.lock cannot pin it and this difference would otherwise change\n\
         │ vimcode's behaviour with no vimcode commit to explain it (#625).\n\
         │\n\
         │   pinned (quadraui-pin.txt): {pinned}\n\
         │   checkout HEAD:             {resolved}\n\
         │   checkout path:             {}\n\
         │\n\
         │ Pick one:\n\
         │\n\
         │ 1. Build against the pin (normal case):\n\
         │        git -C ../quadraui fetch origin\n\
         │        git -C ../quadraui checkout {pinned}\n\
         │\n\
         │ 2. Bump the pin deliberately, then run the tests so any rendering\n\
         │    change lands as a reviewable, attributable commit:\n\
         │        # put the new sha in quadraui-pin.txt\n\
         │        cargo test\n\
         │\n\
         │ 3. Co-developing quadraui? Opt out for this build only:\n\
         │        {UNPIN_ENV}=1 cargo build\n\
         │    (Uncommitted edits at the pinned rev need no opt-out.)\n\
         └────────────────────────────────────────────────────────────────────\n",
        crate_dir.display()
    );
}
