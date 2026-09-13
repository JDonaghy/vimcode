//! Native Win-GUI (Direct2D + DirectWrite) backend — a **wrapper**, not a
//! backend (#866, the Win-GUI twin of #859's `src/macos/mod.rs`, stage 3 of
//! the north star's #7 milestone).
//!
//! quadraui already ships the whole Win-GUI backend: `WinBackend` implements
//! every `Backend` trait method `AppShell` renders through, and
//! [`quadraui::win::shell_runner::run_with_shell`] composes it with the
//! shared `ShellAdapter` exactly the way `quadraui::gtk::shell_runner` /
//! `quadraui::macos::shell_runner` / `quadraui::tui::shell_runner` do
//! (quadraui#465, quadraui#707 for the Win-GUI leg). So there is nothing to
//! rasterise here and nothing to decide here: this file is the Win32
//! sibling of `src/gtk/mod.rs::run` / `src/macos/mod.rs::run`, and it is
//! deliberately the only thing in `src/win/` besides the backend
//! re-export (`src/win/backend.rs`).
//!
//! **It contains no layout, hit-test, paint or dispatch decision** — the
//! acceptance bar #859 set and #866 inherits verbatim. All of that lives in
//! `crate::app::App`, whose single `impl quadraui::ShellApp` every GUI entry
//! point runs; `grep -rn 'impl.*ShellApp for' src/` still returns one GUI
//! implementation, not three. If you find yourself about to add a decision
//! here, `CLAUDE.md`'s Platform-Neutrality Rule says stop: the gap belongs
//! in quadraui or in `crate::app`, not in a backend directory.
//!
//! # Unlike `src/macos/mod.rs`: no `PlatformWindowHandle` impl
//!
//! quadraui's `win` module (pinned rev `9eede7fd`) exposes no public
//! top-level-window handle — `WinBackend`'s `hwnd` field is private and
//! `target_os = "windows"`-gated, and nothing else in `win::run`/
//! `win::services` hands one back. Window *discovery* has no portable
//! equivalent on this backend either (same gap `src/macos/mod.rs` has for
//! `MacBackend` — see `src/app.rs`'s `PlatformWindowHandle` doc comment and
//! the `#866` note just below its GTK impl). `App::new_portable` already
//! leaves `window: None` for exactly this reason, so nothing here needs to
//! change to accommodate it.
//!
//! # Why `feature = "win"` alone, not target-gated like `macos`
//!
//! `macos` is gated `#[cfg(all(feature = "macos", target_os = "macos"))]`
//! in both quadraui's `lib.rs` and this crate's, because quadraui's own
//! `macos` module is gated the same double way — a Linux host compiles NONE
//! of it under `--features macos`. quadraui's `win` module is different by
//! design (see that module's own doc comment): it is gated on `feature =
//! "win"` alone, with every real WinAPI call *inside* it individually
//! `cfg(target_os = "windows")`-gated and falling back to a `todo!()` stub
//! everywhere else — specifically so `cargo check --features win` type-checks
//! `WinBackend` on an ordinary Linux CI runner (see quadraui's `ci.yml`
//! "Compile check (win feature)" step). `src/win/backend.rs` and this file
//! inherit that same posture: un-target-gated, so a plain Linux
//! `cargo build --no-default-features --features win` compiles this module
//! (proving the vimcode-side wrapper is well-typed) even though *running*
//! it productively needs an actual Windows target — [`run`] below only
//! becomes real once `quadraui::win::run::run_with` is invoked on
//! `target_os = "windows"`; off Windows it hits that function's own
//! `todo!()` stub instead of silently doing nothing.
//!
//! `src/main.rs`'s `COMPILED_GUI_BACKEND` selection *does* add a
//! `target_os = "windows"` check on top of `feature = "win"` before ever
//! calling [`run`] — not because this module needs it to compile, but
//! because calling it without a live Windows message loop underneath would
//! panic instead of degrading gracefully, the same reasoning that makes
//! `macos` win over `gui` in that file's precedence rule.
//!
//! # Verifying this file without cargo-xwin
//!
//! A plain `cargo check --no-default-features --features win` on any host
//! (Linux included) type-checks every line below — no cross-toolchain
//! needed, per the module doc above. Producing (and running) a real PE32+
//! binary needs `cargo xwin build --target x86_64-pc-windows-msvc
//! --no-default-features --features win` plus the `clang-cl`/`lld-link`/xwin
//! SDK toolchain #866 pins to the `windows`-capability host for — see that
//! issue's "Machine" section and `Cargo.toml`'s `win` feature comment for
//! why `mlua`'s vendored Lua C build is the thing that actually requires
//! the cross C toolchain, not anything in this file.

use std::path::PathBuf;
use std::process::ExitCode;

pub(crate) mod backend;

use crate::app::{App, TextMetricsBackend};

/// Entry point for the native Win-GUI, mirroring `crate::gtk::run` /
/// `crate::macos::run`.
///
/// Panic hook + swap flush, choose the backend, construct the shared
/// [`App`], derive its [`quadraui::ShellConfig`] via the same
/// `App::shell_config()` the macOS entry point calls (#866 — no
/// per-backend copy of that logic here, see `crate::gtk::build_shell_config`'s
/// doc comment), hand both to the runner. Nothing else — no `gtk4::init`
/// equivalent, because `quadraui::win::run`'s Win32 bootstrap
/// (`RegisterClassExW`/`CreateWindowExW`/the message loop) does its own
/// setup inside `run_with_shell`.
pub fn run(file_path: Option<PathBuf>) -> ExitCode {
    // The same panic hook `crate::gtk::run` / `crate::macos::run` install:
    // flush every dirty buffer to its swap file, then write a crash log.
    {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            crate::core::swap::run_emergency_flush();

            if let Some(path) = crate::core::swap::write_crash_log(info) {
                eprintln!("VimCode crashed. Details written to {}", path.display());
                eprintln!("Unsaved buffers written to swap files for recovery.");
                eprintln!("Please report this at https://github.com/JDonaghy/vimcode/issues");
            }
            prev_hook(info);
        }));
    }

    // The concrete backend is chosen here, at the entry point, and handed to
    // `App` — the seam #861 opened and `src/gtk/mod.rs::run` names in its
    // own comment as the one "a future non-GTK wrapper (#859) would pass a
    // different `TextMetricsBackend` impl through". This is that wrapper's
    // Win-GUI sibling.
    let text_metrics_backend: std::rc::Rc<std::cell::RefCell<Box<dyn TextMetricsBackend>>> =
        std::rc::Rc::new(std::cell::RefCell::new(
            Box::new(backend::WinBackend::new()),
        ));

    let app = App::new_portable(file_path, text_metrics_backend);
    let config = app.shell_config();
    quadraui::win::shell_runner::run_with_shell(app, config)
}

// ── #928: `crate::harness::ConformanceHarness` on `WinDriver` ──────────────
//
// `quadraui::win::testing` (the module holding `WinDriver`/`driver_with_shell`)
// is `#[cfg(target_os = "windows")]`-gated *inside* quadraui regardless of
// `feature = "win"` alone (unlike `quadraui::win::backend`/`run`/`shell_runner`,
// which this module's own doc explains are deliberately not target-gated so
// `cargo check --features win` type-checks `WinBackend` on an ordinary Linux
// host). So this test module has to carry the same double gate `src/macos/mod.rs`
// uses for its own driver-tier tests — on any host but real Windows it simply
// does not exist, proving nothing there (same posture, same reason).
//
// Bodies mirror `src/macos/mod.rs::mac_driver_tests::conformance_proof_slice`
// exactly, `MacDriver`/`MacBackend` swapped for `WinDriver`/`WinBackend` — see
// that module for the scenarios' own doc comments (RED-verification notes,
// why scenario 3 clicks outside the popup rather than a specific row, …).
#[cfg(target_os = "windows")]
#[cfg(test)]
mod win_driver_tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use quadraui::win::testing::driver_with_shell;

    use crate::app::TextMetricsBackend;
    use crate::core::Engine;
    use crate::harness::ConformanceHarness;

    fn plain_engine() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = false;
        engine
    }

    fn conformance_harness(
        engine: Engine,
        width: u32,
        height: u32,
    ) -> ConformanceHarness<quadraui::win::testing::WinDriver<impl quadraui::AppLogic>> {
        let paint = crate::test_paint::PaintGuard::acquire();
        let cwd = crate::test_cwd::CwdReadGuard::acquire();
        let engine = Rc::new(RefCell::new(engine));
        let backend: Rc<RefCell<Box<dyn TextMetricsBackend>>> =
            Rc::new(RefCell::new(Box::new(super::backend::WinBackend::new())));
        let (app, config) = crate::harness::build_app_and_config(Rc::clone(&engine), backend);
        let driver = driver_with_shell(app, config, width, height);
        ConformanceHarness::new(driver, engine, paint, cwd)
    }

    fn conformance_harness_with_folder_picker(
        engine: Engine,
        dir: PathBuf,
        width: u32,
        height: u32,
    ) -> ConformanceHarness<quadraui::win::testing::WinDriver<impl quadraui::AppLogic>> {
        let paint = crate::test_paint::PaintGuard::acquire();
        let cwd = crate::test_cwd::CwdReadGuard::acquire();
        let engine = Rc::new(RefCell::new(engine));
        let backend: Rc<RefCell<Box<dyn TextMetricsBackend>>> =
            Rc::new(RefCell::new(Box::new(super::backend::WinBackend::new())));
        let (app, config) = crate::harness::build_app_and_config(Rc::clone(&engine), backend);
        crate::harness::install_folder_picker(&app, dir);
        let driver = driver_with_shell(app, config, width, height);
        ConformanceHarness::new(driver, engine, paint, cwd)
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_928_win_conformance_{tag}_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Scenario 1 (#928): open/filter/Esc-dismiss the folder picker via
    /// `WinDriver` — the identical body `crate::gtk::testing`'s and
    /// `src/macos/mod.rs`'s own copies run against `GtkDriver`/`MacDriver`.
    #[test]
    fn folder_picker_filters_and_escape_dismisses() {
        let dir = scratch_dir("scenario1");
        std::fs::create_dir_all(dir.join("kkxxqq_distinctive_928")).unwrap();
        std::fs::create_dir_all(dir.join("another_unrelated_dir_928")).unwrap();

        let mut h = conformance_harness_with_folder_picker(plain_engine(), dir.clone(), 1400, 900);

        crate::harness::folder_picker_filters_and_escape_dismisses(
            &mut h.driver,
            "kkxxqq_distinctive_928",
            "another_unrelated_dir_928",
            "kkxxqq_distinctive_928",
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Scenario 2 (#928): the command palette's open/filter/Esc cycle, via
    /// `WinDriver`.
    #[test]
    fn command_palette_filters_and_escape_dismisses() {
        let mut h = conformance_harness(plain_engine(), 1400, 900);

        crate::harness::command_palette_filters_and_escape_dismisses(&mut h.driver);
    }

    /// Scenario 3 (#928): a click outside the open folder picker's popup
    /// must dismiss it, via `WinDriver::click`'s raw pixel-coordinate
    /// dispatch.
    #[test]
    fn folder_picker_click_outside_dismisses_it() {
        let dir = scratch_dir("scenario3");
        std::fs::create_dir_all(dir.join("kkxxqq_distinctive_928")).unwrap();
        std::fs::create_dir_all(dir.join("another_unrelated_dir_928")).unwrap();

        let (width, height) = (1400.0, 900.0);
        let mut h = conformance_harness_with_folder_picker(
            plain_engine(),
            dir.clone(),
            width as u32,
            height as u32,
        );

        crate::harness::folder_picker_click_outside_dismisses_it(
            &mut h.driver,
            "kkxxqq_distinctive_928",
            "another_unrelated_dir_928",
            width - 10.0,
            height - 10.0,
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
