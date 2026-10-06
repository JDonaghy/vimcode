//! #1795: `editor_mode: "vscode"` in `settings.json` must be honoured at
//! **cold startup** (`Engine::new()`'s own `Settings::load()`), not only on
//! the hot-reload path `check_settings_reload` already covers.
//!
//! # Why an integration test, not an in-crate `#[cfg(test)]` one
//!
//! `Settings::load()` has a hard `#[cfg(test)]` short-circuit that always
//! returns `Settings::default()` ("Tests must be hermetic — never read the
//! user's settings.json", `src/core/settings.rs`) — so an in-crate unit test
//! can never observe what that function does with a real file on disk, by
//! construction. `Settings::settings_file_path()`'s thread-local test
//! override (`TestSettingsPathGuard`) is `#[cfg(test)]`-gated too, so it is
//! unreachable from here either. This file is a `tests/*.rs` integration
//! test, which cargo compiles against the library *without* `cfg(test)`
//! (the same reason `tests/common/mod.rs::engine_with` already documents
//! calling the real, disk-reading `Engine::new()` and then resetting state
//! by hand) — the only build configuration in which `Settings::load()`'s
//! real, non-test-gated branch runs at all, and so the only place this
//! exact gap can be covered.
//!
//! # The actual gap this closes
//!
//! Every existing mode-switch test either flips `engine.settings.editor_mode`
//! directly in memory (`tests/vscode_mode.rs`'s `vscode_mode` helper) or
//! drives the *hot-reload* path (`App::handle_poll_tick` ->
//! `Engine::check_settings_reload`, see `src/app.rs`'s
//! `handle_poll_tick_reloads_settings_changed_on_disk`). Nothing drove the
//! *cold-start* path — `Engine::new()` -> `Settings::load()` ->
//! `Settings::load_with_validation()` — against a literal on-disk
//! `settings.json`, which is exactly the path #1795 reported broken on
//! win-native (status bar read NORMAL at launch despite a correct
//! `"editor_mode": "vscode"` in the resolved `settings.json`).
//!
//! # Investigation result (see PR for the full writeup)
//!
//! Reproduced on real Windows hardware (`cargo xwin build --features win`,
//! run both directly and via a PowerShell session with `$env:APPDATA`
//! pointed at a throwaway settings.json containing exactly #1795's
//! reported content): `Engine::new()`/`Engine::startup()` correctly loaded
//! `editor_mode: Vscode` and `Engine::mode_str()` read
//! `"EDIT  F1:palette  Alt-M:vim"`, never `"NORMAL"`, in every case tried.
//! The one case that *did* reproduce "settings edited but a different file
//! got read" was launching the exe from a WSL shell with `APPDATA=<custom>`
//! set only on the *parent* shell's command line (`env APPDATA=... ./a.exe`)
//! — a WSL-interop process-creation quirk that silently drops that
//! override, so the exe reads the real `%APPDATA%\vimcode\settings.json`
//! instead of the one just edited for the test. That is a test-harness
//! environment-propagation gap, not a vimcode code bug — this test instead
//! pins the one thing that *is* a vimcode-code question, so it cannot
//! silently regress: does `Engine::new()` honour `editor_mode` from a real
//! on-disk `settings.json` at cold startup. It is green on unfixed
//! `develop`, by design (there is no code bug here) — see the PR body for
//! why this is coverage, not a RED-then-GREEN regression fix.
//!
//! # Why this mutates the real `HOME`/`APPDATA` env var (unlike every
//! `#[cfg(test)]` test in this crate)
//!
//! `vimcode_config_dir()`'s thread-local test override
//! (`core::paths::set_test_home`) is `#[cfg(test)]`-gated, same reasoning as
//! `TestSettingsPathGuard` above — unreachable from an integration test
//! binary. `std::env::set_var` is process-global, which is exactly why
//! `src/core/paths.rs`'s own module doc says not to use it from a
//! `#[cfg(test)]` unit test (many tests, run in parallel, in one process).
//! None of that applies to *this* file: it is its own separate test binary
//! (cargo gives every `tests/*.rs` file its own process) with exactly one
//! `#[test]` fn, so there is no other test in this process to race against,
//! and the guard below restores the previous value on drop (including on
//! panic) so the process's env is never left pointing at a deleted temp dir.

use std::path::PathBuf;

/// Points `HOME` (`APPDATA` on Windows) at a throwaway directory for the
/// lifetime of the guard, restoring whatever was there before on drop.
struct RealHomeGuard {
    old: Option<std::ffi::OsString>,
    dir: PathBuf,
}

impl RealHomeGuard {
    fn install(dir: PathBuf) -> Self {
        #[cfg(target_os = "windows")]
        {
            let old = std::env::var_os("APPDATA");
            std::env::set_var("APPDATA", &dir);
            Self { old, dir }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let old = std::env::var_os("HOME");
            std::env::set_var("HOME", &dir);
            Self { old, dir }
        }
    }

    /// The config dir `vimcode_config_dir()` resolves to while this guard
    /// is installed — mirrors that function's own per-platform layout
    /// exactly (`%APPDATA%\vimcode` on Windows, `$HOME/.config/vimcode`
    /// elsewhere) rather than re-deriving it, so this test fails loudly if
    /// the two ever disagree.
    fn config_dir(&self) -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            self.dir.join("vimcode")
        }
        #[cfg(not(target_os = "windows"))]
        {
            self.dir.join(".config").join("vimcode")
        }
    }
}

impl Drop for RealHomeGuard {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        let var = "APPDATA";
        #[cfg(not(target_os = "windows"))]
        let var = "HOME";

        match self.old.take() {
            Some(v) => std::env::set_var(var, v),
            None => std::env::remove_var(var),
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn engine_new_honours_editor_mode_vscode_from_disk_settings_json() {
    vimcode_core::core::session::suppress_disk_saves();
    vimcode_core::core::session::suppress_disk_loads();

    let tmp = std::env::temp_dir().join(format!(
        "vimcode_test_1795_home_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let guard = RealHomeGuard::install(tmp);

    let cfg_dir = guard.config_dir();
    std::fs::create_dir_all(&cfg_dir).expect("create fake config dir");
    // The exact content #1795 reported (`lsp_enabled`/`use_nerd_fonts` are
    // along for the ride, unused by this assertion, but kept to match the
    // report's reproduction verbatim).
    std::fs::write(
        cfg_dir.join("settings.json"),
        r#"{"lsp_enabled": false, "use_nerd_fonts": false, "editor_mode": "vscode"}"#,
    )
    .expect("write fake settings.json");

    let engine = vimcode_core::Engine::new();

    assert!(
        engine.is_vscode_mode(),
        "Engine::new() must resolve editor_mode to Vscode from a real \
         on-disk settings.json at cold startup — got {:?}",
        engine.settings.editor_mode
    );
    let mode_str = engine.mode_str();
    assert!(
        mode_str.contains("EDIT"),
        "status bar's mode_str() must read EDIT (vscode mode), not NORMAL, \
         for a cold-started engine whose on-disk settings.json says \
         editor_mode: vscode — got {mode_str:?}"
    );
    assert!(
        !mode_str.contains("NORMAL"),
        "#1795's reported symptom verbatim: status bar must never read \
         NORMAL when settings.json says editor_mode: vscode — got {mode_str:?}"
    );
}
