use std::path::PathBuf;

fn main() {
    // Compile vendored tree-sitter-latex grammar (v0.3.0, language version 14)
    cc::Build::new()
        .include("vendor/tree-sitter-latex/src")
        .file("vendor/tree-sitter-latex/src/parser.c")
        .file("vendor/tree-sitter-latex/src/scanner.c")
        .warnings(false)
        .compile("tree_sitter_latex");

    // ── quadraui rev (#691) ─────────────────────────────────────────────────
    //
    // quadraui is a git dependency pinned to a `rev` in `Cargo.toml` (see the
    // dependency comment there for the history of why — it used to be an
    // unpinned sibling path dep, #638/#625/#659). Cargo/rustc give no built-in
    // way to name "the rev this crate was built against" at runtime, so bake
    // it into the binary here for `vimcode --version` / `vcd --version`
    // (`src/quadraui_pin.rs::version_line`).
    export_quadraui_rev();

    // ── Windows Common-Controls v6 manifest (#1554) ────────────────────────
    embed_windows_comctl_v6_manifest();
}

/// Embed a Common-Controls v6 side-by-side manifest dependency in the
/// binary's linker output (#1554).
///
/// # Why this is needed
///
/// quadraui's `win` backend calls `TaskDialogIndirect`
/// (`quadraui/src/win/services.rs`, quadraui#744), which the `windows` crate
/// imports **statically** from `comctl32.dll`. That export exists only in
/// the Common-Controls **v6** side-by-side assembly —
/// `%SystemRoot%\System32\comctl32.dll` is still the legacy 5.82 build. A
/// Win32 binary only gets the v6 assembly if its own application manifest
/// declares a dependency on it; without that, the Windows loader resolves
/// `comctl32.dll` to 5.82, fails to find `TaskDialogIndirect` in its export
/// table, and kills the process **before `main` runs** — no output, no
/// panic, just `STATUS_ENTRYPOINT_NOT_FOUND` (`0xC0000139`).
///
/// quadraui's own `build.rs` embeds this manifest for *its* bins/tests/
/// examples, but link args from a build script do **not** propagate to a
/// downstream crate (quadraui's `build.rs` "Downstream note" says so
/// explicitly) — vimcode links the `win` backend and must embed an
/// equivalent manifest of its own, which is what this function does.
///
/// `/MANIFEST:EMBED` + `/MANIFESTDEPENDENCY:` is the linker spelling of the
/// classic `#pragma comment(linker, "/manifestdependency:…")` every C++
/// TaskDialog sample carries. Both MSVC's `link.exe` (what `windows-latest`
/// CI uses) and `lld-link` (what `cargo xwin` uses to cross-build from
/// Linux) implement these two flags natively — neither needs `mt.exe`.
fn embed_windows_comctl_v6_manifest() {
    // Host-independent gate: read the *target* cfg cargo hands the build
    // script, never `cfg!(…)` (which would describe the build host and so
    // would be wrong for every cross-compile, including the `cargo xwin`
    // route documented in CLAUDE.md).
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let win_feature = std::env::var_os("CARGO_FEATURE_WIN").is_some();

    // `target_env == "msvc"` because these are MSVC linker flags; a
    // `*-pc-windows-gnu` build links with `ld`, which would reject them. The
    // `win` feature gate keeps the `vcd.exe` TUI build
    // (`build-windows-tui`) byte-identical to what it was before this
    // function existed.
    if target_os == "windows" && target_env == "msvc" && win_feature {
        println!("cargo:rustc-link-arg-bin=vimcode=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=vimcode=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}

/// Resolve the quadraui git rev this build is locked to, and export it as
/// `VIMCODE_QUADRAUI_REV` for `src/quadraui_pin.rs` to bake into the binary.
///
/// Prefers `Cargo.lock`'s resolved rev — the actual commit Cargo fetched and
/// compiled — falling back to the `rev = "..."` in `Cargo.toml` (e.g. a
/// from-scratch build before a lockfile exists). A `paths` override in
/// `.cargo/config.toml` (the local-quadraui co-development workflow; see
/// `cargo-config-local-quadraui.toml.example`) redirects compilation to a
/// local checkout without changing either file, so this still reports the
/// pinned rev in that case — accurate for "what does vimcode intend to build
/// against", not necessarily "what's on disk right now".
fn export_quadraui_rev() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let lock_path = manifest_dir.join("Cargo.lock");
    let toml_path = manifest_dir.join("Cargo.toml");

    println!("cargo:rerun-if-changed={}", lock_path.display());
    println!("cargo:rerun-if-changed={}", toml_path.display());

    let rev = std::fs::read_to_string(&lock_path)
        .ok()
        .and_then(|s| rev_from_lockfile(&s))
        .or_else(|| {
            std::fs::read_to_string(&toml_path)
                .ok()
                .and_then(|s| rev_from_manifest(&s))
        })
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=VIMCODE_QUADRAUI_REV={rev}");
}

/// Pull the resolved 40-char SHA out of `Cargo.lock`'s `quadraui` package
/// entry, e.g. `source = "git+https://.../quadraui.git?rev=<rev>#<sha>"`.
/// The `#<sha>` suffix is Cargo's *resolved* commit — authoritative, and
/// present even if `rev` in `Cargo.toml` is a branch name or short SHA.
fn rev_from_lockfile(lock: &str) -> Option<String> {
    let mut lines = lock.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim() == "name = \"quadraui\"" {
            // The `source` line follows `name`/`version` within the same
            // `[[package]]` block.
            for follow in lines.by_ref().take(4) {
                if let Some(rest) = follow.trim().strip_prefix("source = \"") {
                    if let Some((_, sha)) = rest.rsplit_once('#') {
                        let sha = sha.trim_end_matches('"');
                        if is_full_sha(sha) {
                            return Some(sha.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Fall back to the `rev = "..."` pinned on the `quadraui` dependency line in
/// `Cargo.toml`, for a from-scratch build with no `Cargo.lock` yet.
fn rev_from_manifest(manifest: &str) -> Option<String> {
    let line = manifest
        .lines()
        .find(|l| l.trim_start().starts_with("quadraui = ") && l.contains("git ="))?;
    let after = line.split_once("rev = \"")?.1;
    let rev = after.split('"').next()?;
    Some(rev.to_string())
}

fn is_full_sha(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
}
