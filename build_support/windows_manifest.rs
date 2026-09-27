//! The Windows Common-Controls v6 application-manifest gate (#1554).
//!
//! `include!`d by **two** places, deliberately:
//!
//! * `build.rs`, which prints whatever this returns as
//!   `cargo:rustc-link-arg-bin=vimcode=…` lines, and
//! * `tests/windows_manifest.rs`, which asserts the gate's whole matrix on any
//!   host — no Windows toolchain, no cross-linker, no CI runner required.
//!
//! It lives in its own file rather than inline in `build.rs` for exactly that
//! second reason: a build script cannot be `include!`d by a test (it pulls in
//! `cc`, a build-dependency), so any logic left inside it is untestable from
//! Linux, and the *only* thing that could catch a regression would be the
//! `windows-latest` CI lane. That lane is the real end-to-end gate (it launches
//! the linked `vimcode.exe` and requires exit code 0), but it cannot fail on
//! the branch that breaks the gate if someone drops the flags while touching
//! `build.rs` for an unrelated reason.

/// Fully-qualified side-by-side identity of the Common-Controls **v6**
/// assembly, in `link.exe`'s `/MANIFESTDEPENDENCY:` syntax.
///
/// Every field is load bearing — a wrong `version` or `publicKeyToken` does not
/// fail the link, it produces a binary the Windows loader kills before `main`
/// with `STATUS_ENTRYPOINT_NOT_FOUND` (`0xC0000139`), because it falls back to
/// the legacy 5.82 `comctl32.dll` in `System32`, which has no
/// `TaskDialogIndirect` export for quadraui's `win` backend to import.
pub const COMCTL_V6_DEPENDENCY: &str = "type='win32' name='Microsoft.Windows.Common-Controls' \
     version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'";

/// Linker arguments the `vimcode` binary needs so the Windows loader binds
/// `comctl32.dll` to the Common-Controls v6 side-by-side assembly (#1554).
///
/// Returns an empty vector for every target/feature combination that must stay
/// byte-identical to its pre-#1554 output:
///
/// * non-Windows targets (Linux/macOS have no manifests at all),
/// * `*-pc-windows-gnu` (links with `ld`, which rejects these MSVC flags),
/// * Windows MSVC **without** the `win` feature — that is the `vcd.exe` TUI
///   build (`Build (Windows TUI, vcd.exe)` in CI), which never links
///   quadraui's `win` backend and so never imports `TaskDialogIndirect`.
///
/// The inputs are the *target* cfg values cargo hands a build script
/// (`CARGO_CFG_TARGET_OS` / `CARGO_CFG_TARGET_ENV` / `CARGO_FEATURE_WIN`),
/// never `cfg!(…)`, which would describe the build **host** and so be wrong
/// for every cross-compile — including the `cargo xwin` route in CLAUDE.md.
pub fn comctl_v6_link_args(target_os: &str, target_env: &str, win_feature: bool) -> Vec<String> {
    if target_os == "windows" && target_env == "msvc" && win_feature {
        vec![
            "/MANIFEST:EMBED".to_string(),
            format!("/MANIFESTDEPENDENCY:{COMCTL_V6_DEPENDENCY}"),
        ]
    } else {
        Vec::new()
    }
}
