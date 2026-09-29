//! TUI entry point for VimCode. Activated with `--tui`; renders the same
//! `ScreenLayout` the GTK backend consumes, via ratatui + crossterm instead
//! of Cairo. No GTK/Cairo/Pango imports — editor logic is `core`'s,
//! rendering data is `render`'s.
//!
//! #1433 flipped [`run`] onto the shared [`crate::app::App`]; #1434 deleted
//! the independently hand-written production TUI shell this module used to
//! build (see `docs/IRREDUCIBLE_SURFACE.md` §4, `GOALS.md`'s sizing table).
//! What's left: `TuiBackend` construction, crash-recovery scaffolding around
//! `quadraui::tui::shell_runner::run_with_shell`, and the [`testing`] seam.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

#[cfg(test)]
mod app_on_tui_tests;
mod backend;

// #1497: the local `impl crate::app::TextMetricsBackend for
// backend::TuiBackend` that used to live here is gone —
// `quadraui::Backend::set_current_line_height`/`set_current_char_width`
// (JDonaghy/quadraui#1086) already default to a no-op, which is exactly
// what this impl's two bodies were: one ratatui cell is one row/column by
// construction (#540/#819), so `TuiBackend` never needs to override them.

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

/// The TUI entry point: build the shared [`crate::app::App`] and drive it
/// through `quadraui::tui::shell_runner::run_with_shell` (the TUI twin of
/// `crate::macos::run`), which handles all raw-mode/mouse-capture setup and
/// teardown, restoring the terminal even on panic. The outer `catch_unwind`
/// only exists to print the crash message afterward.
pub fn run(file_path: Option<PathBuf>, debug_log_path: Option<String>) {
    if let Some(ref path) = debug_log_path {
        init_debug_log(path);
        debug_log!("=== VimCode TUI debug log started ===");
    }

    crate::core::swap::install_gui_crash_hook_with_sink(|line| {
        debug_log!("{}", line);
    });

    let backend: std::rc::Rc<std::cell::RefCell<Box<dyn quadraui::Backend>>> = std::rc::Rc::new(
        std::cell::RefCell::new(Box::new(backend::TuiBackend::new())),
    );

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

/// Test/acceptance driver seam: what `app_on_tui_tests.rs` and the sealed
/// `tests/acceptance.rs` (via `feature = "test-support"`) use to drive
/// [`crate::app::App`] on a `quadraui::tui::TuiDriver`.
///
/// `crate::harness`'s `backend_conformance!` macro still registers two TUI
/// arms, `tui` ([`conformance_harness`]) and `tui_prod`
/// ([`conformance_harness_prod`]), which diverged before #1433/#1434 but now
/// both build the identical `App`; `tui_prod` is a thin alias, kept only so
/// existing `tui_prod`-suffixed scenarios and `KNOWN_BUGS` labels in
/// `src/harness.rs` keep resolving unchanged.
#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use quadraui::tui::testing::{driver_with_shell, TuiDriver};
    use quadraui::tui::TuiBackend;

    use crate::app::App;
    use crate::core::Engine;
    use crate::harness::ConformanceHarness;
    use crate::render::UnitProfile;

    /// The `TuiDriver` instantiation of `crate::harness::ConformanceHarness`
    /// (#982) — mirrors `crate::gtk::testing::conformance_harness`, modulo
    /// `TuiBackend` and cell (`u16`) vs pixel (`i32`) dimensions.
    ///
    /// `TuiDriver` doesn't implement `PixelClickConformance` (no ratatui
    /// equivalent for pixel-precise clicks); see `crate::harness`'s module
    /// doc for that boundary.
    pub fn conformance_harness(
        engine: Engine,
        width: u16,
        height: u16,
    ) -> ConformanceHarness<TuiDriver<impl quadraui::AppLogic>> {
        let paint = crate::test_paint::PaintGuard::acquire();
        let cwd = crate::test_cwd::CwdReadGuard::acquire();
        let engine = Rc::new(RefCell::new(engine));
        let backend: Rc<RefCell<Box<dyn quadraui::Backend>>> =
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
        let backend: Rc<RefCell<Box<dyn quadraui::Backend>>> =
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
    /// with the two process-wide guards ([`crate::test_paint::PaintGuard`],
    /// [`crate::test_cwd::CwdReadGuard`]) every App/Engine-backed driver
    /// constructor here takes — see those guards' own module docs for the
    /// Pango-segfault/CWD-race they cover.
    ///
    /// `Deref`/`DerefMut`s to the wrapped driver, so a caller drives it
    /// exactly like a bare `TuiDriver`; this only exists to keep the guards
    /// alive for the driver's whole lifetime.
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

    /// Build a TUI driver of the given cell size via the shared `App`
    /// construction path (`App::new_portable_for_test` + `shell_config`).
    ///
    /// Runs the real `Engine::startup`, so sidebar/scroll/session state is
    /// *ambient* (read from `~/.config/vimcode`), not fixed. Use
    /// [`tui_driver_with`] instead if a caller needs a known starting state.
    pub fn tui_driver(
        file_path: Option<PathBuf>,
        width: u16,
        height: u16,
    ) -> TuiAppDriver<TuiDriver<impl quadraui::AppLogic>> {
        tui_driver_with(file_path, width, height, |_| {})
    }

    /// [`tui_driver`], plus a `setup` hook run against the live `Engine`
    /// after construction but before the first frame renders (e.g. seeding
    /// buffer text, pinning scroll positions).
    ///
    /// Takes a `FnOnce(&mut Engine)` rather than exposing `App`'s
    /// `Rc<RefCell<Engine>>` directly, since `App` is `pub(crate)` and a
    /// public accessor would leak its shape.
    ///
    /// Acquires the paint/cwd guards *before* constructing `App` (covering
    /// `Engine::startup`'s own CWD reads) and returns them bundled via
    /// [`TuiAppDriver`].
    pub fn tui_driver_with(
        file_path: Option<PathBuf>,
        width: u16,
        height: u16,
        setup: impl FnOnce(&mut Engine),
    ) -> TuiAppDriver<TuiDriver<impl quadraui::AppLogic>> {
        let paint = crate::test_paint::PaintGuard::acquire();
        let cwd = crate::test_cwd::CwdReadGuard::acquire();
        let backend: Rc<RefCell<Box<dyn quadraui::Backend>>> =
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
