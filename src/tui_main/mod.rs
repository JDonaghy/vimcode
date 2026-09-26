//! TUI (terminal UI) entry point for VimCode.
//!
//! Activated with the `--tui` CLI flag. Uses ratatui + crossterm to render
//! the same `ScreenLayout` produced by `render::build_screen_layout` that the
//! GTK backend consumes — just rendered to a terminal instead of a Cairo
//! surface.
//!
//! **No GTK/Cairo/Pango imports here.** All editor logic comes from `core`.
//! All rendering data comes from `render`.
//!
//! #1433 flipped [`run`] onto the shared [`crate::app::App`] — the same
//! cross-backend shell GTK/macOS/Win-GUI run; #1434 deleted the
//! independently hand-written production TUI shell this module used to
//! build before that flip (see `docs/IRREDUCIBLE_SURFACE.md` §4 and
//! `GOALS.md`'s sizing table for the before/after line counts). What's
//! left is thin wiring: the `TuiBackend` construction and panic/crash-
//! recovery scaffolding [`run`] wraps around
//! `quadraui::tui::shell_runner::run_with_shell`, and the [`testing`] seam
//! the driver-tier suite (`app_on_tui_tests.rs`) and the sealed acceptance
//! crate (`feature = "test-support"`) build on.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

#[cfg(test)]
mod app_on_tui_tests;
mod backend;

/// [`crate::app::TextMetricsBackend`] for quadraui's `TuiBackend` (#982) —
/// the TUI sibling of `impl TextMetricsBackend for GtkBackend`/`WinBackend`
/// (`src/app.rs`) and `MacBackend` (`src/macos/mod.rs`). Both setters are
/// genuine no-ops: one ratatui cell is one row/column by construction, so
/// there is no pixel metric here to ever disagree with (#540/#819).
impl crate::app::TextMetricsBackend for backend::TuiBackend {
    fn set_current_line_height(&mut self, _line_height: f64) {}
    fn set_current_char_width(&mut self, _char_width: f64) {}
}

/// Global debug log file handle, set once at startup via `--debug <path>`.
static DEBUG_LOG: std::sync::OnceLock<Mutex<std::fs::File>> = std::sync::OnceLock::new();

/// Initialise the debug log.  Call once before the shell runner starts.
fn init_debug_log(path: &str) {
    match std::fs::File::create(path) {
        Ok(f) => {
            let _ = DEBUG_LOG.set(Mutex::new(f));
            std::env::set_var("VIMCODE_LSP_DEBUG", "1");
        }
        Err(e) => {
            eprintln!("Warning: cannot open debug log {path}: {e}");
        }
    }
}

/// Write a formatted message to the debug log (if enabled). No-op when
/// `--debug` was not passed.
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if let Some(mtx) = $crate::tui_main::DEBUG_LOG.get() {
            if let Ok(mut f) = mtx.lock() {
                let _ = writeln!(f, $($arg)*);
                let _ = f.flush();
            }
        }
    };
}

/// The TUI entry point: initialise the shared [`crate::app::App`] and drive
/// it through `quadraui::tui::shell_runner::run_with_shell` — the TUI twin
/// of `crate::macos::run`. `run_with_shell` already does all raw-mode /
/// alternate-screen / mouse-capture setup and teardown internally, and
/// always restores the terminal — even on panic — before propagating; the
/// outer `catch_unwind` below only exists to print the crash message after
/// the terminal is already back to normal.
pub fn run(file_path: Option<PathBuf>, debug_log_path: Option<String>) {
    if let Some(ref path) = debug_log_path {
        init_debug_log(path);
        debug_log!("=== VimCode TUI debug log started ===");
    }

    crate::core::swap::install_gui_crash_hook_with_sink(|line| {
        debug_log!("{}", line);
    });

    let backend: std::rc::Rc<std::cell::RefCell<Box<dyn crate::app::TextMetricsBackend>>> =
        std::rc::Rc::new(std::cell::RefCell::new(
            Box::new(backend::TuiBackend::new()),
        ));

    let app = crate::app::App::new_portable(file_path, backend, crate::render::UnitProfile::cell());
    // #557: `shell_config()` reads plugin-registered sidebar panels, so it
    // must run after `App::new_portable` and before `app` moves below.
    let config = app.shell_config();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        quadraui::tui::shell_runner::run_with_shell(app, config);
    }));

    if let Err(e) = result {
        // The panic hook already ran the emergency swap flush via the
        // registered emergency-engine pointer before unwinding started;
        // this block only reproduces the user-facing crash message.
        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            format!("VimCode internal error: {s}")
        } else if let Some(s) = e.downcast_ref::<String>() {
            format!("VimCode internal error: {s}")
        } else {
            "VimCode internal error (unknown panic payload)".to_string()
        };
        let crash_path = crate::core::swap::crash_log_path();
        eprintln!("{msg}");
        eprintln!("Unsaved buffers written to swap files for recovery.");
        eprintln!("Crash details written to {}", crash_path.display());
        eprintln!("Please report this at https://github.com/JDonaghy/vimcode/issues");
        std::process::exit(1);
    }
}

/// Test/acceptance driver seam — the TUI half of what the in-crate
/// driver-tier suite (`app_on_tui_tests.rs`) and the sealed acceptance
/// crate (`tests/acceptance.rs`, via `feature = "test-support"`) need to
/// drive [`crate::app::App`] on a `quadraui::tui::TuiDriver`.
///
/// # `tui` vs `tui_prod` (#1043, converged #1434)
///
/// `crate::harness`'s `backend_conformance!` macro still registers two TUI
/// arms, `tui` (via [`conformance_harness`]) and `tui_prod` (via
/// [`conformance_harness_prod`]). Before #1433 these wrapped two
/// independently-implemented shells — the shared [`crate::app::App`] and a
/// hand-rolled, TUI-only production shell — so a scenario green on `tui`
/// but red on `tui_prod` meant the two had diverged. #1433 flipped
/// production [`run`] onto `App`, and #1434 deleted the hand-rolled shell
/// entirely, so both arms now build the identical `App` —
/// [`conformance_harness_prod`] is a thin alias of [`conformance_harness`].
/// Both names are kept (rather than collapsing `backend_conformance!` back
/// to two arms) so the many existing `tui_prod`-suffixed scenarios and
/// `KNOWN_BUGS` labels in `src/harness.rs` keep resolving unchanged.
#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use quadraui::tui::testing::{driver_with_shell, TuiDriver};
    use quadraui::tui::TuiBackend;

    use crate::app::{App, TextMetricsBackend};
    use crate::core::Engine;
    use crate::harness::ConformanceHarness;
    use crate::render::UnitProfile;

    /// The `TuiDriver` instantiation of `crate::harness::ConformanceHarness`
    /// (#982) — mirrors `crate::gtk::testing::conformance_harness`
    /// (`src/gtk/testing.rs`) exactly, modulo the backend-specific pieces:
    /// `TuiBackend` instead of `GtkBackend`, and `width`/`height` in
    /// terminal cells (`u16`) rather than pixels (`i32`), matching
    /// `quadraui::tui::testing::driver_with_shell`'s own signature.
    ///
    /// `TuiDriver` does not implement `quadraui::testing::PixelClickConformance`
    /// (only `GtkDriver`/`MacDriver`/`WinDriver` do — pixel-precise native
    /// click delivery has no ratatui equivalent). A scenario bounded by
    /// `ConformanceDriver + DriverInput` (e.g.
    /// `crate::harness::sweep_hit_band_integrity`, which only needs
    /// `DriverInput::click`) still runs on both; one that needs
    /// `PixelClickConformance` specifically stays GTK-only. See
    /// `crate::harness`'s own module doc for this boundary spelled out once,
    /// rather than re-explained at every call site.
    pub fn conformance_harness(
        engine: Engine,
        width: u16,
        height: u16,
    ) -> ConformanceHarness<TuiDriver<impl quadraui::AppLogic>> {
        let paint = crate::test_paint::PaintGuard::acquire();
        let cwd = crate::test_cwd::CwdReadGuard::acquire();
        let engine = Rc::new(RefCell::new(engine));
        let backend: Rc<RefCell<Box<dyn TextMetricsBackend>>> =
            Rc::new(RefCell::new(Box::new(TuiBackend::new())));
        let (app, config) = crate::harness::build_app_and_config(
            Rc::clone(&engine),
            backend,
            crate::render::UnitProfile::cell(),
        );
        let screen_layout = Rc::clone(&app.cached_screen_layout);
        let driver = driver_with_shell(app, config, width, height);
        ConformanceHarness::new_with_screen_layout(driver, engine, screen_layout, paint, cwd)
    }

    #[cfg(test)]
    pub fn conformance_harness_with_folder_picker(
        engine: Engine,
        dir: std::path::PathBuf,
        width: u16,
        height: u16,
    ) -> ConformanceHarness<TuiDriver<impl quadraui::AppLogic>> {
        // #1431: the TUI twin of `crate::gtk::testing::conformance_harness_
        // with_folder_picker` — needed because the picker has to be seeded
        // on `App` before it is moved into `driver_with_shell`.
        let paint = crate::test_paint::PaintGuard::acquire();
        let cwd = crate::test_cwd::CwdReadGuard::acquire();
        let engine = Rc::new(RefCell::new(engine));
        let backend: Rc<RefCell<Box<dyn TextMetricsBackend>>> =
            Rc::new(RefCell::new(Box::new(TuiBackend::new())));
        let (app, config) = crate::harness::build_app_and_config(
            Rc::clone(&engine),
            backend,
            crate::render::UnitProfile::cell(),
        );
        crate::harness::install_folder_picker(&app, dir);
        let driver = driver_with_shell(app, config, width, height);
        ConformanceHarness::new(driver, engine, paint, cwd)
    }

    /// #1434: now a thin alias of [`conformance_harness`] — see this
    /// module's own doc for why the `tui_prod` name survives rather than
    /// being collapsed away. Kept `#[cfg(test)]`-only, matching its
    /// pre-#1434 gate: every call site lives inside a `#[cfg(test)]`-gated
    /// scenario module in `src/harness.rs`.
    #[cfg(test)]
    pub fn conformance_harness_prod(
        engine: Engine,
        width: u16,
        height: u16,
    ) -> ConformanceHarness<TuiDriver<impl quadraui::AppLogic>> {
        conformance_harness(engine, width, height)
    }

    /// Bundles a [`TuiDriver`] built by [`tui_driver`]/[`tui_driver_with`]
    /// with the two process-wide guards
    /// ([`crate::test_paint::PaintGuard`], [`crate::test_cwd::CwdReadGuard`])
    /// every other App/Engine-backed driver constructor in this codebase
    /// takes before handing back a driver — [`conformance_harness`] and
    /// [`conformance_harness_prod`] above, and every GTK/macOS/Win
    /// equivalent (`src/gtk/testing.rs`, `src/macos/mod.rs`,
    /// `src/win/mod.rs`). See `crate::test_paint`'s and `crate::test_cwd`'s
    /// own module docs for the concurrent-Pango segfault and CWD-read race
    /// this protects against; `App::new_portable_for_test`'s real
    /// `Engine::startup` (explorer root, ambient sidebar restore) is exactly
    /// the CWD-dependent read `crate::test_cwd`'s doc warns about.
    ///
    /// Implements `Deref`/`DerefMut` to the wrapped driver, so a caller
    /// drives it exactly like a bare `TuiDriver` (`driver.render()`,
    /// `driver.screen()`, `driver.screen_contains(..)`, …) — this wrapper
    /// only exists to keep the two guards alive for the driver's whole
    /// lifetime, the same "held for the harness's whole lifetime" contract
    /// [`crate::harness::ConformanceHarness`]'s own `_paint`/`_cwd` fields
    /// document, not to add a new API surface a test would need to learn.
    pub struct TuiAppDriver<D> {
        driver: D,
        _paint: crate::test_paint::PaintGuard,
        _cwd: crate::test_cwd::CwdReadGuard,
    }

    impl<D> std::ops::Deref for TuiAppDriver<D> {
        type Target = D;
        fn deref(&self) -> &D {
            &self.driver
        }
    }

    impl<D> std::ops::DerefMut for TuiAppDriver<D> {
        fn deref_mut(&mut self) -> &mut D {
            &mut self.driver
        }
    }

    /// Build a TUI driver of the given cell size with no other setup, built
    /// through the shared `App` construction path
    /// (`App::new_portable_for_test` + `App::shell_config`).
    ///
    /// Runs the real `Engine::startup` (via `App::new_portable_for_test`),
    /// so sidebar visibility, scroll offsets, and restored session state
    /// are *ambient*, read from the developer's real `~/.config/vimcode`,
    /// not fixed. A caller that needs a known starting buffer/scroll
    /// position (so a marker painted at a known location can't drift by
    /// whatever the machine's own session happens to restore) should use
    /// [`tui_driver_with`] instead.
    pub fn tui_driver(
        file_path: Option<PathBuf>,
        width: u16,
        height: u16,
    ) -> TuiAppDriver<TuiDriver<impl quadraui::AppLogic>> {
        tui_driver_with(file_path, width, height, |_| {})
    }

    /// [`tui_driver`], plus a `setup` hook that runs against the live
    /// `Engine` after construction but before the first frame renders —
    /// e.g. seeding buffer text and pinning every window's
    /// `view.scroll_top` to `0`.
    ///
    /// `App` stores its `Engine` behind `Rc<RefCell<Engine>>` (`App::engine`
    /// in `src/app.rs`) rather than owning it directly, and `App` itself is
    /// `pub(crate)` — so a public accessor handing back the `Rc` would
    /// either leak `App`'s crate-private shape through the accessor's own
    /// return type, or need a wrapper type solely to hide it. A
    /// `FnOnce(&mut Engine)` callback sidesteps both: the caller gets a
    /// mutable window onto the same engine [`tui_driver`] would otherwise
    /// hand straight to `driver_with_shell` unseen, without `App` ever
    /// crossing the module boundary.
    ///
    /// Acquires [`crate::test_paint::PaintGuard`] and
    /// [`crate::test_cwd::CwdReadGuard`] *before* constructing the `App` (so
    /// `Engine::startup`'s own CWD reads are covered, not just the later
    /// `render()` calls a caller makes on the returned driver) and returns
    /// them bundled into the driver via [`TuiAppDriver`] — see that type's
    /// own doc for why they must outlive this function's return, not just
    /// its body.
    pub fn tui_driver_with(
        file_path: Option<PathBuf>,
        width: u16,
        height: u16,
        setup: impl FnOnce(&mut Engine),
    ) -> TuiAppDriver<TuiDriver<impl quadraui::AppLogic>> {
        let paint = crate::test_paint::PaintGuard::acquire();
        let cwd = crate::test_cwd::CwdReadGuard::acquire();
        let backend: Rc<RefCell<Box<dyn TextMetricsBackend>>> =
            Rc::new(RefCell::new(Box::new(TuiBackend::new())));
        let app = App::new_portable_for_test(file_path, backend, UnitProfile::cell());
        setup(&mut app.engine.borrow_mut());
        let config = app.shell_config();
        let driver = driver_with_shell(app, config, width, height);
        TuiAppDriver {
            driver,
            _paint: paint,
            _cwd: cwd,
        }
    }
}
