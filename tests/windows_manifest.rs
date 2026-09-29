//! Regression tests for the Windows Common-Controls v6 manifest gate (#1554).
//!
//! # The bug
//!
//! `vimcode.exe` built with `--features win` died before `main` ran: the
//! Windows loader resolved `comctl32.dll` to the legacy 5.82 build in
//! `System32`, found no `TaskDialogIndirect` export (quadraui's `win` backend
//! imports it statically), and killed the process with
//! `STATUS_ENTRYPOINT_NOT_FOUND` (`0xC0000139`) — no stdout, no stderr, no
//! panic. The fix embeds an application manifest declaring a dependency on the
//! Common-Controls **v6** side-by-side assembly, which is the only place that
//! export lives.
//!
//! # Why these tests, on Linux
//!
//! The end-to-end gate is CI's `Build (Windows GUI, vimcode.exe)` lane, which
//! links the real binary on a `windows-latest` runner, **launches** it and
//! requires exit code 0 — a loader kill is invisible to `cargo build`, to
//! `clippy`, and to a Linux `cargo xwin build`, so only a real Windows loader
//! can prove the fix. That lane cannot run here.
//!
//! What *can* run here is everything that makes the lane meaningful:
//!
//! * the flag-emitting decision itself, over its whole target/feature matrix
//!   (`build_support/windows_manifest.rs`, shared verbatim with `build.rs`), and
//! * the existence and shape of the CI lane, so deleting it — the one way to
//!   make this bug undetectable again — fails the suite on Linux instead of
//!   going quiet.
//!
//! **Verified RED against the unfixed state**, by reinstating the bug and
//! re-running (`2 passed; 2 failed`):
//!
//! * `/MANIFESTDEPENDENCY:` dropped from the emitted args →
//!   `emits_manifest_flags_for_windows_msvc_gui_build` fails with
//!   `expected /MANIFEST:EMBED plus one /MANIFESTDEPENDENCY:, got
//!   ["/MANIFEST:EMBED"]`.
//! * `build-windows-gui` deleted from `ci.yml` (i.e. `develop`'s state) →
//!   `ci_has_a_windows_gui_lane_that_launches_the_exe` fails with `ci.yml has
//!   no Windows GUI lane`.

use std::path::PathBuf;

// The same source `build.rs` compiles — not a copy. A test cannot `include!`
// `build.rs` itself (it pulls in the `cc` build-dependency), which is why the
// decision lives in its own file.
#[path = "../build_support/windows_manifest.rs"]
mod windows_manifest;

use windows_manifest::{comctl_v6_link_args, COMCTL_V6_DEPENDENCY};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The build that was broken: Windows + MSVC + the `win` feature (the
/// `vimcode.exe` GUI build) must get both flags, and the dependency must name
/// version 6.0.0.0 with the Microsoft public key token. A wrong version or
/// token still links — and still gets the process killed by the loader — so
/// assert on the identity, not merely that some flag was emitted.
#[test]
fn emits_manifest_flags_for_windows_msvc_gui_build() {
    let args = comctl_v6_link_args("windows", "msvc", true);
    assert_eq!(
        args.len(),
        2,
        "expected /MANIFEST:EMBED plus one /MANIFESTDEPENDENCY:, got {args:?}"
    );
    assert!(
        args.iter().any(|a| a == "/MANIFEST:EMBED"),
        "without /MANIFEST:EMBED the linker writes a side-by-side .manifest \
         file the loader never reads: {args:?}"
    );

    let dep = args
        .iter()
        .find(|a| a.starts_with("/MANIFESTDEPENDENCY:"))
        .expect("expected the v6 manifest dependency (#1554)");
    for needle in [
        "name='Microsoft.Windows.Common-Controls'",
        "version='6.0.0.0'",
        "publicKeyToken='6595b64144ccf1df'",
        "type='win32'",
        "processorArchitecture='*'",
    ] {
        assert!(
            dep.contains(needle),
            "manifest dependency is missing {needle} — a v6 identity that is \
             wrong in any field falls back to comctl32 5.82, which has no \
             TaskDialogIndirect export (#1554): {dep}"
        );
    }
    assert_eq!(dep, &format!("/MANIFESTDEPENDENCY:{COMCTL_V6_DEPENDENCY}"));
}

/// Every combination that must keep emitting nothing. The `win`-feature-off
/// row is the important one: it is the `vcd.exe` TUI build, which never links
/// quadraui's `win` backend, and #1554 must leave it byte-identical.
#[test]
fn emits_nothing_for_every_other_target_and_feature_combination() {
    for (os, env, win, why) in [
        (
            "windows",
            "msvc",
            false,
            "vcd.exe TUI build — no win backend",
        ),
        (
            "windows",
            "gnu",
            true,
            "*-windows-gnu links with ld, not link.exe",
        ),
        ("windows", "gnu", false, "windows-gnu TUI build"),
        ("linux", "gnu", true, "Linux has no application manifests"),
        ("linux", "gnu", false, "plain Linux build"),
        ("macos", "", true, "macOS has no application manifests"),
        ("", "", true, "unknown target cfg must not guess"),
    ] {
        let args = comctl_v6_link_args(os, env, win);
        assert!(
            args.is_empty(),
            "expected no linker args for target_os={os:?} target_env={env:?} \
             win_feature={win} ({why}), got {args:?}"
        );
    }
}

/// The decision reads the *target* cfg, so a cross-compile is gated by the
/// target and not the build host: the values above are exactly what cargo puts
/// in `CARGO_CFG_TARGET_OS` / `CARGO_CFG_TARGET_ENV`, and `build.rs` must read
/// them from the environment rather than using `cfg!(…)` (which describes the
/// host — wrong for the `cargo xwin` cross-build in CLAUDE.md).
#[test]
fn build_script_reads_target_cfg_not_host_cfg() {
    let build_rs = std::fs::read_to_string(repo_root().join("build.rs")).expect("read build.rs");
    let gate = build_rs
        .split_once("fn embed_windows_comctl_v6_manifest")
        .expect("build.rs must still call the #1554 manifest gate")
        .1;
    let body = gate.split("\nfn ").next().unwrap_or(gate);
    for needle in [
        "CARGO_CFG_TARGET_OS",
        "CARGO_CFG_TARGET_ENV",
        "CARGO_FEATURE_WIN",
        "comctl_v6_link_args",
        "cargo:rustc-link-arg-bin=vimcode=",
    ] {
        assert!(
            body.contains(needle),
            "build.rs's #1554 gate no longer references {needle}; a host `cfg!()` \
             gate would silently skip the manifest on every cross-build"
        );
    }
    assert!(
        !body.contains("cfg!(target_os"),
        "the gate must read the target cfg from the environment, not cfg!() \
         (which describes the build host)"
    );
}

/// The lane that actually proves the fix on Windows must exist, must build the
/// GUI binary with the `win` feature, and must *launch* it. Asserting it
/// merely builds would be the same mistake #1554 was: `cargo build` succeeded
/// the whole time the binary was unlaunchable.
#[test]
fn ci_has_a_windows_gui_lane_that_launches_the_exe() {
    let ci = std::fs::read_to_string(repo_root().join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    assert!(
        ci.contains("Build (Windows GUI, vimcode.exe)"),
        "ci.yml has no Windows GUI lane; nothing would link vimcode.exe for \
         Windows before a release does (#1554)"
    );
    assert!(
        ci.contains("--features win") && ci.contains("--bin vimcode"),
        "the Windows GUI lane must build `--features win --bin vimcode` — the \
         only configuration that links quadraui's win backend"
    );
    assert!(
        ci.contains("vimcode.exe") && ci.contains("ExitCode"),
        "the Windows GUI lane must launch vimcode.exe and assert on its exit \
         code: a loader kill (0xC0000139) prints nothing, so an output-only \
         check passes against the bug (#1554)"
    );
    assert!(
        ci.contains("Microsoft\\.Windows\\.Common-Controls")
            || ci.contains("Microsoft.Windows.Common-Controls"),
        "the Windows GUI lane must assert the manifest is embedded in the \
         binary; every windows-latest runner has the v6 assembly available, so \
         the launch smoke alone cannot distinguish a fix from luck (#1554)"
    );
}
