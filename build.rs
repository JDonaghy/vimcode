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
