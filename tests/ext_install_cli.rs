//! #1719 acceptance criterion: "Black-box (via the headless entry point):
//! with `npm` absent, `--ext-install yaml` reports `npm` missing and the
//! platform instruction, exits non-zero, and runs no install."
//!
//! This spawns the actual compiled `vimcode` binary (not an in-process call
//! into `Engine`) with a throwaway `$HOME` holding a local extension
//! manifest (`ext_available_manifests` merges `$HOME/.config/vimcode/
//! extensions/*/manifest.toml` — see `src/core/engine/lsp_ops.rs` — exactly
//! the mechanism a developer uses to test an extension before publishing it
//! to the registry, so no network access is needed for this test) and a
//! `PATH` that cannot resolve `npm`, mirroring `tests/cli_help.rs`'s
//! real-binary-spawn pattern (#979).
//!
//! `VIMCODE_TEST_HOMEBREW_PREFIXES` is pinned to a directory that does not
//! exist so `resolve_command`'s Homebrew-prefix probe (#917) cannot
//! accidentally find a real `npm` under `/opt/homebrew` or `/usr/local` on a
//! macOS runner — see `lsp_manager::homebrew_prefixes`'s own doc for why
//! this override exists.

use std::io::Write;
use std::process::Command;

/// Build a throwaway `$HOME` containing `.config/vimcode/extensions/<name>/
/// manifest.toml` with the given TOML body, returning the temp dir (kept
/// alive by the caller) and its path.
fn home_with_local_extension(ext_name: &str, manifest_toml: &str) -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!(
        "vc-ext-install-cli-test-{}-{}",
        std::process::id(),
        ext_name
    ));
    let ext_dir = base
        .join(".config")
        .join("vimcode")
        .join("extensions")
        .join(ext_name);
    std::fs::create_dir_all(&ext_dir).expect("create extension dir");
    let mut f = std::fs::File::create(ext_dir.join("manifest.toml")).expect("create manifest");
    f.write_all(manifest_toml.as_bytes())
        .expect("write manifest");
    base
}

#[test]
fn ext_install_reports_missing_npm_and_runs_no_install_without_prerequisite() {
    let ext_name = "vc-test-yaml-1719";
    let home = home_with_local_extension(
        ext_name,
        &format!(
            r#"
name = "{ext_name}"
display_name = "YAML (test)"
language_ids = ["vc-test-yaml-1719-lang"]

[lsp]
binary = "vc-test-yaml-1719-language-server"
install_linux = "npm install -g vc-test-yaml-1719-language-server"
install_macos = "npm install -g vc-test-yaml-1719-language-server"
install_windows = "npm install -g vc-test-yaml-1719-language-server"
dependencies = ["npm"]
"#
        ),
    );

    // A deliberately minimal PATH that cannot resolve `npm` (or the
    // manifest's own `vc-test-yaml-1719-language-server`, which obviously
    // doesn't exist anywhere) — but still has `sh`/`which` for vimcode's own
    // internal probing to run without erroring out.
    let exe = env!("CARGO_BIN_EXE_vimcode");
    let output = Command::new(exe)
        .arg("--ext-install")
        .arg(ext_name)
        .arg("--json")
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env(
            "VIMCODE_TEST_HOMEBREW_PREFIXES",
            home.join("nonexistent-homebrew-prefix"),
        )
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("failed to run vimcode --ext-install");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "vimcode --ext-install must exit non-zero when a prerequisite is \
         missing; status: {:?}\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code()
    );

    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("npm"),
        "output must name the missing prerequisite (npm); got stdout: \
         {stdout}\nstderr: {stderr}"
    );
    // The platform instruction (#918's actionable hint, not just a bare
    // "install npm" — `extensions::prereq_install_cmd("npm")`) must be
    // present verbatim, not merely implied.
    #[cfg(target_os = "macos")]
    let expected_hint = "brew install node";
    #[cfg(target_os = "windows")]
    let expected_hint = "winget install OpenJS.NodeJS";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let expected_hint = "sudo apt install nodejs npm";
    assert!(
        combined.contains(expected_hint),
        "output must carry the platform-specific install instruction \
         ({expected_hint:?}); got stdout: {stdout}\nstderr: {stderr}"
    );

    // JSON shape: a `--json` run must be parseable and carry the same facts
    // a human-readable run carries in prose.
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("not valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(json["status"], "missing_prerequisite");
    assert_eq!(json["missing"][0], "npm");
    assert!(
        json["instructions"]["npm"]
            .as_str()
            .is_some_and(|s| s.contains(expected_hint)),
        "got JSON: {json}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn ext_install_unknown_extension_reports_json_error_and_exits_nonzero() {
    let home = home_with_local_extension(
        "vc-test-unused-ext-1719",
        r#"
name = "vc-test-unused-ext-1719"
display_name = "Unused"
"#,
    );

    let exe = env!("CARGO_BIN_EXE_vimcode");
    let output = Command::new(exe)
        .arg("--ext-install")
        .arg("vc-test-totally-unknown-extension-1719")
        .arg("--json")
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env(
            "VIMCODE_TEST_HOMEBREW_PREFIXES",
            home.join("nonexistent-homebrew-prefix"),
        )
        .output()
        .expect("failed to run vimcode --ext-install");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("not valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(json["status"], "unknown_extension");

    let _ = std::fs::remove_dir_all(&home);
}

/// #1807: the headless `--ext-install` entry point must refuse an
/// extension whose `requires_vimcode` constraint isn't met by the running
/// vimcode — the same check the interactive marketplace applies
/// (`Engine::ext_install_from_registry_with_runtime_check`) — rather than
/// diverging from it and installing scripts that would error at load
/// time on a missing `vimcode.*` API.
///
/// RED-verified by hand: with the `incompatibility_reason_for_running_
/// vimcode` early-out removed from `run_ext_install` in `src/main.rs`,
/// this fails — the process proceeds past the new check straight to the
/// (absent) prerequisite check and exits 0 instead of reporting
/// `incompatible_vimcode`.
#[test]
fn ext_install_refuses_incompatible_requires_vimcode_and_exits_nonzero() {
    let ext_name = "vc-test-1807-cli-future";
    let home = home_with_local_extension(
        ext_name,
        &format!(
            r#"
name = "{ext_name}"
display_name = "1807 CLI Future Extension"
requires_vimcode = ">=9999.0.0"
"#
        ),
    );

    let exe = env!("CARGO_BIN_EXE_vimcode");
    let output = Command::new(exe)
        .arg("--ext-install")
        .arg(ext_name)
        .arg("--json")
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env(
            "VIMCODE_TEST_HOMEBREW_PREFIXES",
            home.join("nonexistent-homebrew-prefix"),
        )
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("failed to run vimcode --ext-install");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "an incompatible extension must exit non-zero; status: {:?}\n\
         stdout: {stdout}\nstderr: {stderr}",
        output.status.code()
    );

    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("not valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(json["status"], "incompatible_vimcode");
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|s| s.contains("9999.0.0")),
        "got JSON: {json}"
    );

    let _ = std::fs::remove_dir_all(&home);
}
