use std::path::PathBuf;

fn main() {
    // Compile vendored tree-sitter-latex grammar (v0.3.0, language version 14)
    cc::Build::new()
        .include("vendor/tree-sitter-latex/src")
        .file("vendor/tree-sitter-latex/src/parser.c")
        .file("vendor/tree-sitter-latex/src/scanner.c")
        .warnings(false)
        .compile("tree_sitter_latex");

    // ── Stale-quadraui guard (#587) ─────────────────────────────────────────
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
