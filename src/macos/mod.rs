//! Native macOS (AppKit) backend — a **wrapper**, not a backend (#859,
//! stage 2 of #47).
//!
//! quadraui already ships the whole macOS backend: `MacBackend` implements
//! every `Backend` trait method `AppShell` renders through, and
//! [`quadraui::macos::shell_runner::run_with_shell`] composes it with the
//! shared `ShellAdapter` exactly the way `quadraui::gtk::shell_runner` and
//! `quadraui::tui::shell_runner` do (quadraui#465). So there is nothing to
//! rasterise here and nothing to decide here: this file is the AppKit
//! sibling of `src/gtk/mod.rs::run`, and it is deliberately the only thing
//! in `src/macos/`.
//!
//! **It contains no layout, hit-test, paint or dispatch decision** — the
//! acceptance bar #859 sets. All of that lives in `crate::app::App`, whose
//! single `impl quadraui::ShellApp` both GUI entry points run; `grep -rn
//! 'impl.*ShellApp for' src/` still returns one GUI implementation, not two.
//! If you find yourself about to add a decision here, `CLAUDE.md`'s
//! Platform-Neutrality Rule says stop: the gap belongs in quadraui or in
//! `crate::app`, not in a backend directory.
//!
//! # Why this is possible now
//!
//! Until #862 it was not. `crate::app::App` was `#[cfg(feature = "gui")]`
//! and held `gtk4::Window` / `gtk4::CssProvider` / `gio::FileMonitor`
//! fields, so the only `ShellApp` a non-GTK runner could have been handed
//! was a second, duplicated one — the exact outcome the north star exists to
//! prevent. #861 type-erased [`TextMetricsBackend`]'s context setter and
//! #862 type-erased the three platform fields and lifted the portable half
//! of `crate::gtk::{click,css,util}` into `crate::{click,css,app_support}`.
//! What is left for this file is the two things that genuinely are per-
//! backend: pick the concrete backend, and start the event loop.
//!
//! # Verifying this file without a Mac
//!
//! quadraui gates its own module `#[cfg(all(feature = "macos", target_os =
//! "macos"))]`, and so does `crate::macos` in `src/lib.rs`, so a plain Linux
//! build compiles **none** of this and proves nothing.
//!
//! #859 planned to close that with a cross-target type-check —
//! `cargo check --no-default-features --features macos --target
//! aarch64-apple-darwin`, the trick quadraui's own macOS clippy stage uses.
//! **That does not work for vimcode**, and the difference is not the Rust
//! target (which *is* installed on 1.97.1) but the C one: vimcode depends on
//! `tree-sitter`, whose `build.rs` compiles `lib.c` for the *target*, so the
//! check dies inside `cc-rs` long before rustc sees this file —
//! `cc: error: unrecognized command-line option '-arch'`. Closing it needs a
//! macOS cross-toolchain (osxcross / `zig cc` / clang + a macOS SDK); none is
//! installed on this fleet. That is a fleet-provisioning task, not something
//! to work around by weakening a gate here.
//!
//! What holds the line on every lane meanwhile: everything this file *uses*
//! is backend-neutral and compiled by the ordinary Linux lanes —
//! [`crate::app::App::new_portable`] and [`crate::app::App::shell_config`]
//! are un-gated and type-checked everywhere, and
//! `app.rs::portable_entry_point_tests` pins both the `A: ShellApp +
//! 'static` bound `run_with_shell` requires and the `ShellConfig`
//! `shell_config()` produces. The residue this file adds on top is ~20
//! lines with no branches.
//!
//! **#896 closes #859's stage 3.** [`mac_driver_tests`] below is that
//! `MacDriver` black-box test, and it exists now because the issue needed
//! it: the pinned quadraui had reachable `todo!()`s in
//! `MacBackend::draw_minimap` / `minimap_layout`, so vimcode's *first
//! painted frame* aborted the process on a real Mac while every Linux lane
//! stayed green. The test is double-gated exactly like the module (`macos`
//! feature + `target_os = "macos"`), so it runs only on a Mach-O host and
//! is simply absent from the Linux lanes — the same shape as quadraui's own
//! macOS tests, not a weakened gate.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::app::{App, TextMetricsBackend};

/// [`TextMetricsBackend`] for quadraui's `MacBackend`.
///
/// - `set_text_measurement_context` stays a no-op, and that is what the
///   trait's own doc comment predicts for this backend rather than an
///   omission: "a backend with no persistent-context concept … can
///   implement this as a no-op"; macOS text measurement
///   (`quadraui::macos::text::measure_text(&CTFont, &str)`) takes the font
///   per call instead of storing a context. The only producer of a context
///   (`click::build_editor_click_context`) is GTK-only and its call site in
///   `render_content` is `#[cfg(feature = "gui")]`, so nothing ever calls
///   this here anyway.
/// - the two metric setters forward to `MacBackend`'s own public
///   `set_current_line_height`/`set_current_char_width` (`f64`, matching
///   Pango's unit — quadraui#934, pinned rev `f3b3aed9`), the macOS
///   counterparts of `GtkBackend`'s methods of the same name and the
///   `WinBackend` impl below. Before quadraui#934 `MacBackend` had no such
///   setters and both were stubbed no-ops (#859); #967 found that stub left
///   `App::explorer_ui_event`'s #540 drift guard — which re-applies the
///   metrics the tree was *painted* with immediately before hit-testing —
///   silently doing nothing on macOS, so hit-testing ran against
///   `MacBackend::new()`'s default `current_line_height` instead of the
///   real CoreText metric the paint used. `tree_layout`'s row pitch is
///   `(line_height * 1.4).round()`, so a stale default drifted the hit
///   bands by a pixel per row, growing with row index until clicks
///   resolved to the row below.
impl TextMetricsBackend for quadraui::macos::MacBackend {
    // `set_text_measurement_context` is deliberately not overridden here —
    // the trait's default (empty) body is exactly this backend's no-op, per
    // the reasoning above (#969).

    fn set_current_line_height(&mut self, line_height: f64) {
        quadraui::macos::MacBackend::set_current_line_height(self, line_height);
    }

    fn set_current_char_width(&mut self, char_width: f64) {
        quadraui::macos::MacBackend::set_current_char_width(self, char_width);
    }
}

/// Entry point for the native macOS GUI, mirroring `crate::gtk::run`.
///
/// Panic hook + swap flush, choose the backend, construct the shared
/// [`App`], derive its [`quadraui::ShellConfig`], hand both to the runner.
/// Nothing else — no `gtk4::init` equivalent, because
/// `quadraui::macos::run` does AppKit's own bootstrap (main-thread check,
/// `NSApplication`, default font) itself.
pub fn run(file_path: Option<PathBuf>) -> ExitCode {
    // The same panic hook `crate::gtk::run` installs: flush every dirty
    // buffer to its swap file, then write a crash log.
    crate::core::swap::install_gui_crash_hook();

    // The concrete backend is chosen here, at the entry point, and handed to
    // `App` — the seam #861 opened and `src/gtk/mod.rs::run` names in its own
    // comment as the one "a future non-GTK wrapper (#859) would pass a
    // different `TextMetricsBackend` impl through". This is that wrapper.
    let backend: std::rc::Rc<std::cell::RefCell<Box<dyn TextMetricsBackend>>> = std::rc::Rc::new(
        std::cell::RefCell::new(Box::new(quadraui::macos::MacBackend::new())),
    );

    let app = App::new_portable(file_path, backend);
    let config = app.shell_config();
    quadraui::macos::shell_runner::run_with_shell(app, config)
}

/// The `MacDriver` instantiation of [`crate::harness::ConformanceHarness`]
/// — the macOS twin of `crate::gtk::testing::conformance_harness` and
/// `crate::tui_main::testing::conformance_harness`, lifted out of
/// [`mac_driver_tests`]'s own private `conformance_proof_slice` module
/// (#1090) so a shared scenario registered outside this file can reach it.
///
/// Thin wiring only, per this module's own "no decision lives here" bar:
/// the concrete driver type is the one thing a backend module has to
/// supply, and [`crate::harness::build_app_and_config`] owns everything
/// above it. `#[cfg(test)]` because it is test-only plumbing and the
/// `macos` feature already gates the whole module.
#[cfg(test)]
pub(crate) fn conformance_harness(
    engine: crate::core::Engine,
    width: u32,
    height: u32,
) -> crate::harness::ConformanceHarness<quadraui::macos::testing::MacDriver<impl quadraui::AppLogic>>
{
    use quadraui::macos::testing::driver_with_shell;
    use quadraui::macos::MacBackend;

    let paint = crate::test_paint::PaintGuard::acquire();
    let cwd = crate::test_cwd::CwdReadGuard::acquire();
    let engine = std::rc::Rc::new(std::cell::RefCell::new(engine));
    let backend: std::rc::Rc<std::cell::RefCell<Box<dyn TextMetricsBackend>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Box::new(MacBackend::new())));
    let (app, config) = crate::harness::build_app_and_config(std::rc::Rc::clone(&engine), backend);
    let driver = driver_with_shell(app, config, width, height);
    crate::harness::ConformanceHarness::new(driver, engine, paint, cwd)
}

#[cfg(test)]
mod mac_driver_tests {
    //! Driver-tier coverage for the native macOS GUI (#896, closing #859's
    //! stage 3).
    //!
    //! These drive the **same** `impl ShellApp for App` that
    //! [`super::run`] hands `quadraui::macos::shell_runner::run_with_shell`,
    //! through `quadraui::macos::testing::driver_with_shell` — the macOS twin
    //! of `crate::gtk::testing::harness` and the TUI's
    //! `render_content_paints_*_via_shell_app`. Headless: a `CGBitmapContext`,
    //! no `NSApplication`, no window, so they run in an ordinary `cargo test`.
    //!
    //! ## Why an assertion on *painted text* is the right one here
    //!
    //! #896's failure mode is not a wrong pixel — it is that the process
    //! **aborts mid-frame**: `MacBackend::draw_minimap` was a `todo!()`, and
    //! the panic unwinds into objc2's `drawRect:` trampoline, which is
    //! `extern "C"` and therefore aborts rather than unwinding. So the
    //! black-box statement that distinguishes fixed from unfixed is "with the
    //! minimap enabled, the frame completes and the buffer's own lines reach
    //! the screen". Asserting a layout field were populated could not catch
    //! it, and neither could a `render()` with no assertion after it — the
    //! frame has to be shown to have produced content.

    use std::cell::RefCell;
    use std::rc::Rc;

    use quadraui::macos::testing::driver_with_shell;
    use quadraui::macos::MacBackend;

    use crate::app::{App, TextMetricsBackend};
    use crate::core::Engine;

    /// Surface size in points — wide enough that the minimap's own column is
    /// laid out rather than clamped away, which is what puts
    /// `Backend::draw_minimap` on the paint path at all.
    const W: u32 = 1400;
    const H: u32 = 900;

    /// An in-memory engine with enough lines for the minimap to have
    /// something to draw, and the minimap explicitly **on** — the default is
    /// not this test's business to depend on, since the whole point is to
    /// reach `draw_minimap`.
    fn engine_with_minimap() -> Engine {
        let mut engine = Engine::new_for_test();
        let text: String = (0..500).map(|i| format!("line {i}\n")).collect();
        engine.buffer_mut().insert(0, &text);
        engine.settings.minimap = true;
        // Nerd-font tab icons OFF, deliberately. `render::build_tab_bar_icons`
        // returns an empty sidecar when they are off, which is the only input
        // `MacBackend::draw_tab_bar_icons` accepts without firing its
        // `debug_assert!` — per-tab icon glyphs (#620) are a *documented*
        // macOS gap in quadraui, not #896's bug, and because it is a
        // `debug_assert!` rather than a `todo!()` a release binary paints
        // icon-less tabs instead of aborting. Leaving them on here would make
        // both tests below fail on that gap and assert nothing about the
        // minimap, so it is scoped out on purpose; the gap itself needs a
        // quadraui issue (see this PR's notes), never a fix in `src/macos/`.
        engine.settings.use_nerd_fonts = Some(false);
        engine
    }

    /// Wrap `engine` in the real [`App`] on a `MacBackend` and hand back a
    /// headless `MacDriver`. Mirrors `crate::gtk::testing::harness`, including
    /// its two process-wide guards (see `src/test_paint.rs` for the ordering
    /// note) so this lane cannot race a `chdir`-ing test.
    fn driver(
        engine: Engine,
    ) -> (
        (crate::test_paint::PaintGuard, crate::test_cwd::CwdReadGuard),
        quadraui::macos::testing::MacDriver<impl quadraui::AppLogic>,
    ) {
        let guards = (
            crate::test_paint::PaintGuard::acquire(),
            crate::test_cwd::CwdReadGuard::acquire(),
        );
        let backend: Rc<RefCell<Box<dyn TextMetricsBackend>>> =
            Rc::new(RefCell::new(Box::new(MacBackend::new())));
        let app = App::new_headless_with_backend(Rc::new(RefCell::new(engine)), backend);
        let config = app.shell_config();
        // `driver_with_shell` paints the first frame inside `new` — which is
        // precisely where #896 aborted.
        (guards, driver_with_shell(app, config, W, H))
    }

    /// Like [`driver`], but also hands back the `Rc<RefCell<Engine>>` —
    /// needed by a sweep whose `setup` closure mutates the engine directly
    /// *between* samples (#971's picker sweep re-opens the popup before
    /// every sample; see `crate::harness::sweep_hit_band_integrity_resetting`'s
    /// own doc for why a plain click-then-click-to-restore pattern can't be
    /// reused there).
    fn driver_with_engine(
        engine: Engine,
    ) -> (
        (crate::test_paint::PaintGuard, crate::test_cwd::CwdReadGuard),
        Rc<RefCell<Engine>>,
        quadraui::macos::testing::MacDriver<impl quadraui::AppLogic>,
    ) {
        let guards = (
            crate::test_paint::PaintGuard::acquire(),
            crate::test_cwd::CwdReadGuard::acquire(),
        );
        let engine = Rc::new(RefCell::new(engine));
        let backend: Rc<RefCell<Box<dyn TextMetricsBackend>>> =
            Rc::new(RefCell::new(Box::new(MacBackend::new())));
        let app = App::new_headless_with_backend(Rc::clone(&engine), backend);
        let config = app.shell_config();
        let driver = driver_with_shell(app, config, W, H);
        (guards, engine, driver)
    }

    // ── #928 proof slice: `crate::harness::ConformanceHarness` on `MacDriver` ──
    //
    // `driver()` above is #896's own bespoke constructor, kept as-is; these
    // three helpers instead build a `crate::harness::ConformanceHarness`
    // (#928) so the shared scenario bodies in `crate::harness` — the same
    // ones `crate::gtk::testing`'s `conformance_proof_slice` module runs
    // against `GtkDriver` — also run, unmodified, against `MacDriver`.
    mod conformance_proof_slice {
        use std::cell::RefCell;
        use std::path::PathBuf;
        use std::rc::Rc;

        use quadraui::macos::testing::driver_with_shell;
        use quadraui::macos::MacBackend;

        use crate::app::TextMetricsBackend;
        use crate::core::Engine;
        use crate::harness::ConformanceHarness;

        fn conformance_harness(
            engine: Engine,
            width: u32,
            height: u32,
        ) -> ConformanceHarness<quadraui::macos::testing::MacDriver<impl quadraui::AppLogic>>
        {
            let paint = crate::test_paint::PaintGuard::acquire();
            let cwd = crate::test_cwd::CwdReadGuard::acquire();
            let engine = Rc::new(RefCell::new(engine));
            let backend: Rc<RefCell<Box<dyn TextMetricsBackend>>> =
                Rc::new(RefCell::new(Box::new(MacBackend::new())));
            let (app, config) = crate::harness::build_app_and_config(Rc::clone(&engine), backend);
            let driver = driver_with_shell(app, config, width, height);
            ConformanceHarness::new(driver, engine, paint, cwd)
        }

        fn conformance_harness_with_folder_picker(
            engine: Engine,
            dir: PathBuf,
            width: u32,
            height: u32,
        ) -> ConformanceHarness<quadraui::macos::testing::MacDriver<impl quadraui::AppLogic>>
        {
            let paint = crate::test_paint::PaintGuard::acquire();
            let cwd = crate::test_cwd::CwdReadGuard::acquire();
            let engine = Rc::new(RefCell::new(engine));
            let backend: Rc<RefCell<Box<dyn TextMetricsBackend>>> =
                Rc::new(RefCell::new(Box::new(MacBackend::new())));
            let (app, config) = crate::harness::build_app_and_config(Rc::clone(&engine), backend);
            crate::harness::install_folder_picker(&app, dir);
            let driver = driver_with_shell(app, config, width, height);
            ConformanceHarness::new(driver, engine, paint, cwd)
        }

        fn scratch_dir(tag: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_928_macos_conformance_{tag}_{:?}",
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        /// Scenario 1 (#928): open/filter/Esc-dismiss the folder picker via
        /// `MacDriver` — the identical body
        /// `crate::gtk::testing::conformance_proof_slice` runs against
        /// `GtkDriver`. RED-verified on real Mach-O hardware (an
        /// `aarch64-apple-darwin` Mac mini, not inferred from the GTK
        /// result): temporarily making `App::apply_folder_picker_event`
        /// (`src/app.rs`) an unconditional no-op — the same vimcode-side
        /// mutation `crate::gtk::testing::conformance_proof_slice`'s own doc
        /// describes, since both backends drive that one shared dispatch
        /// path — takes *this* test red too (fails on the `screen_has(other)`
        /// assertion, same as the GTK copy), confirmed with
        /// `cargo test --no-default-features --features macos` and reverted
        /// after confirming; see this issue's PR notes.
        #[test]
        fn folder_picker_filters_and_escape_dismisses() {
            let dir = scratch_dir("scenario1");
            std::fs::create_dir_all(dir.join("kkxxqq_distinctive_928")).unwrap();
            std::fs::create_dir_all(dir.join("another_unrelated_dir_928")).unwrap();

            let mut h = conformance_harness_with_folder_picker(
                super::plain_engine(),
                dir.clone(),
                1400,
                900,
            );

            crate::harness::folder_picker_filters_and_escape_dismisses(
                &mut h.driver,
                "kkxxqq_distinctive_928",
                "another_unrelated_dir_928",
                "kkxxqq_distinctive_928",
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// Scenario 2 (#928): the command palette's open/filter/Esc cycle,
        /// via `MacDriver`.
        #[test]
        fn command_palette_filters_and_escape_dismisses() {
            let mut h = conformance_harness(super::plain_engine(), 1400, 900);

            crate::harness::command_palette_filters_and_escape_dismisses(&mut h.driver);
        }

        /// Scenario 3 (#928): a click outside the open folder picker's
        /// popup must dismiss it, via `MacDriver::click`'s raw
        /// pixel-coordinate dispatch.
        #[test]
        fn folder_picker_click_outside_dismisses_it() {
            let dir = scratch_dir("scenario3");
            std::fs::create_dir_all(dir.join("kkxxqq_distinctive_928")).unwrap();
            std::fs::create_dir_all(dir.join("another_unrelated_dir_928")).unwrap();

            let (width, height) = (1400.0, 900.0);
            let mut h = conformance_harness_with_folder_picker(
                super::plain_engine(),
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

    /// #896: the first painted frame must complete with the minimap enabled.
    ///
    /// RED against the pre-fix quadraui pin (`9eede7fd`): the process aborts
    /// inside `driver()` before any assertion runs, because
    /// `MacBackend::draw_minimap`'s `todo!()` panics through objc2's
    /// non-unwinding `drawRect:`.
    #[test]
    fn first_frame_paints_the_buffer_with_the_minimap_enabled() {
        let (_guards, driver) = driver(engine_with_minimap());

        assert!(
            driver.screen_contains("line 0"),
            "the editor's first line never reached the screen; painted text was {:?}",
            driver.painted_texts()
        );
    }

    /// The frame is not merely survivable once — the app keeps repainting
    /// past the minimap. `G` jumps to the end of the buffer, so the *last*
    /// line has to appear and the first has to leave.
    ///
    /// Also RED against the pre-fix pin, and for a second reason on top of
    /// `draw_minimap`: the repaint re-enters `minimap_layout`, the other
    /// `todo!()` quadraui#802 removed.
    #[test]
    fn repaint_after_jumping_to_end_of_buffer_still_completes() {
        let (_guards, mut driver) = driver(engine_with_minimap());
        assert!(
            driver.screen_contains("line 0"),
            "bad fixture: no first line"
        );

        driver.type_char('G');
        driver.render();

        assert!(
            driver.screen_contains("line 499"),
            "`G` must repaint the end of the buffer; painted text was {:?}",
            driver.painted_texts()
        );
        assert!(
            !driver.screen_contains("line 0 "),
            "the viewport should have scrolled away from the top of the buffer"
        );
    }

    /// #1069: on macOS, `Settings::default().font_family` must resolve a
    /// *real* CoreText family, not silently fail. Before this issue,
    /// `default_font_family()` (`src/core/settings.rs`) returned
    /// `"Monospace"` on every platform — a fontconfig *generic alias* with
    /// no CoreText equivalent. `App::render_content` pushes it onto the
    /// paint backend every frame via `backend.set_editor_font(family,
    /// size)` unconditionally (#947, no gate) — on macOS that reaches
    /// `MacBackend::set_editor_font`, which no-ops unless `make_font_exact`
    /// finds an *exact* installed family. `"Monospace"` never matched, so
    /// `current_font` stayed `None` forever and `char_width`/`line_height`
    /// stuck at quadraui's placeholder seed values (`MacBackend::new`'s
    /// `current_char_width: 8.0`, `current_line_height: 16.0`) regardless
    /// of `font_size` — the editor was laid out against numbers no
    /// installed font actually has.
    ///
    /// RED-verified against unfixed `develop`: temporarily reverting
    /// `default_font_family()`'s macOS arm back to `"Monospace".to_string()`
    /// and re-running this test with `cargo test --no-default-features
    /// --features macos` fails — `char_width`/`line_height` land exactly on
    /// the 8.0/16.0 placeholders instead of matching Menlo's real metrics.
    ///
    /// Goes straight through `Backend::set_editor_font` on a bare
    /// `MacBackend` — same shape as the sibling
    /// `mac_backend_applies_line_height_and_char_width` above — rather than
    /// through a full `driver()`/`App` frame: this is specifically about
    /// `default_font_family()` resolving on CoreText, not about paint or
    /// dispatch, and a full frame drags in `install_menu_bar`'s unrelated
    /// (and here, harmless) main-thread panic noise.
    #[test]
    fn editor_font_family_default_resolves_a_real_font_on_macos() {
        use quadraui::Backend;

        let settings = crate::core::settings::Settings::default();
        let mut backend = MacBackend::new();
        backend.set_editor_font(&settings.font_family, settings.font_size as f32);

        // The exact 8.0/16.0 placeholders `MacBackend::new` seeds
        // `current_char_width`/`current_line_height` with before any font is
        // successfully installed — a real font's metrics landing on these
        // exact values is not a realistic coincidence.
        assert_ne!(
            backend.char_width(),
            8.0,
            "current_char_width is still quadraui's placeholder seed value \
             — Settings::default().font_family ({:?}) never resolved a real \
             font via make_font_exact",
            settings.font_family
        );
        assert_ne!(
            backend.line_height(),
            16.0,
            "current_line_height is still quadraui's placeholder seed value \
             — Settings::default().font_family ({:?}) never resolved a real \
             font via make_font_exact",
            settings.font_family
        );

        // Positive control: the resolved metrics must be Menlo's own, not
        // some other font `make_font` silently substituted.
        let menlo = quadraui::macos::text::make_font_exact(
            &settings.font_family,
            settings.font_size as f64,
        )
        .expect("Settings::default().font_family must be exactly resolvable on macOS (#1069)");
        let expected = quadraui::macos::text::font_metrics(&menlo);
        assert!(
            (backend.char_width() as f64 - expected.char_width).abs() < 0.01,
            "char_width {} does not match Menlo's own metrics {}",
            backend.char_width(),
            expected.char_width
        );
        assert!(
            (backend.line_height() as f64 - expected.line_height).abs() < 0.01,
            "line_height {} does not match Menlo's own metrics {}",
            backend.line_height(),
            expected.line_height
        );
    }

    // ── #901: native menu bar adoption ──────────────────────────────────

    /// A plain engine, safe to drive through `MacBackend` — same
    /// nerd-fonts-off rationale as [`engine_with_minimap`] (the tab-icon
    /// `debug_assert!`, unrelated to menus), without the minimap/500-line
    /// buffer that test doesn't need here.
    fn plain_engine() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
    }

    /// #901: a backend declaring `BackendCaps::native_menu` (macOS's
    /// `MacBackend`) must not *also* paint the in-window `MenuSystem` row —
    /// that was the bug (a menu bar drawn inside the window on top of the
    /// real system one). `App::setup` installs the native bar and sets
    /// `menu_bar_visible = false` when this cap is set.
    ///
    /// RED against the pre-#901 body (`App::setup` set
    /// `menu_bar_visible = true` unconditionally): the drawn menu row
    /// paints its first label, "File", every frame — confirmed by
    /// temporarily reverting the gate and re-running this test, which then
    /// fails on this exact assertion.
    #[test]
    fn native_menu_backend_suppresses_the_drawn_menu_row() {
        let (_guards, driver) = driver(plain_engine());

        assert!(
            !driver.screen_contains("File"),
            "the in-window menu row must not paint when the backend has a \
             native menu bar; painted text was {:?}",
            driver.painted_texts()
        );
    }

    /// #939: the omnibar (quadraui `CommandCenter`) must still paint in the
    /// title-bar band on a native-menu backend, even though the drawn menu
    /// row sharing that band stays suppressed (the test right above this
    /// one, unchanged). #901 coupled all three title-bar rungs -- menu row,
    /// its dropdown, and the Command Center -- to one `menu_bar_visible`
    /// flag; `App::setup` sets that flag `false` on a native-menu backend to
    /// suppress the redundant in-window row (#901's own fix), which dragged
    /// the Command Center down with it as unintended collateral. VS Code's
    /// own macOS title bar has no drawn menu labels at all but still shows
    /// the Command Center, which is the behaviour this pins.
    ///
    /// RED against the pre-#939 tree: `command_center_rect` was populated
    /// only inside the `FrameOp::MenuRow` match arm, itself gated on
    /// `presence.menu_row` (== `menu_bar_visible`), so on this exact backend
    /// (`native_menu: true` -> `menu_bar_visible = false`) the arm never ran,
    /// `command_center_rect` stayed `None`, and the search-box label below
    /// never reached the screen even after separately splitting
    /// `FramePresence`'s liveness gate -- confirmed by reverting just the
    /// `app.rs` band-measurement hoist (restoring it to run only inside the
    /// `MenuRow` arm) and re-running: this assertion fails while the sibling
    /// test above keeps passing, exactly pinning the second, independent
    /// coupling the issue describes.
    #[test]
    fn command_center_paints_on_a_native_menu_backend() {
        let mut engine = plain_engine();
        // A distinctive, non-default `cwd` so the Command Center's "🔍
        // <project>" search label is unmistakable in `painted_texts()` --
        // mirrors `gtk::testing::command_center`'s
        // `engine_with_tab_history` fixture, which does the same for the
        // identical reason (an empty/default cwd would paint an empty
        // label, per `render::build_command_center_view`).
        engine.cwd = std::path::PathBuf::from("omnibar-fixture-939");

        let (_guards, driver) = driver(engine);

        assert!(
            !driver.screen_contains("File"),
            "sibling assertion to native_menu_backend_suppresses_the_drawn_menu_row \
             -- the drawn menu row must stay suppressed; painted text was {:?}",
            driver.painted_texts()
        );
        assert!(
            driver.screen_contains("omnibar-fixture-939"),
            "the Command Center's search label must paint in the title-bar \
             band even though the drawn menu row sharing that band is \
             suppressed; painted text was {:?}",
            driver.painted_texts()
        );
    }

    /// #901: `UiEvent::MenuActivated` (fired by the native NSMenu installed
    /// via `Backend::install_menu_bar`) must reach the exact same
    /// `App::handle_menu_action` dispatch the drawn `MenuSystem` dropdown's
    /// `MenuEvent::Activated` already uses — one action path, not two.
    ///
    /// Drives this through `MacDriver::dispatch`, which calls
    /// `AppLogic::handle` directly — it does not depend on
    /// `Backend::install_menu_bar` actually having installed a real NSMenu
    /// (impossible off the main thread AppKit requires; see `App::setup`'s
    /// comment), only on the *routing* once an activation arrives.
    ///
    /// Uses the View menu's "Command Palette" (`action: "palette"`,
    /// `MENU_STRUCTURE` in `render.rs`) because its effect is unmistakably
    /// observable in painted output: activating it must open the palette
    /// overlay, which paints a "Command Palette" title and the full command
    /// list — none of which exists on screen beforehand.
    ///
    /// RED against a build with no `UiEvent::MenuActivated` arm in
    /// `App::handle` (the pre-#901 body — `grep -rn MenuActivated src/` was
    /// empty): the event falls through unhandled, the palette never opens,
    /// and this assertion fails.
    #[test]
    fn menu_activated_reaches_the_same_action_as_the_drawn_menu() {
        let (_guards, mut driver) = driver(plain_engine());
        assert!(
            !driver.screen_contains("Command Palette"),
            "bad fixture: the palette should start closed; painted text was {:?}",
            driver.painted_texts()
        );

        driver.dispatch(quadraui::UiEvent::MenuActivated(quadraui::WidgetId::new(
            "palette",
        )));
        driver.render();

        assert!(
            driver.screen_contains("Command Palette"),
            "MenuActivated(\"palette\") must open the command palette the \
             same way the drawn menu's identical action string does; \
             painted text was {:?}",
            driver.painted_texts()
        );
    }

    // ── #902: native macOS context menus ────────────────────────────────

    /// #902: `menu_style` defaults to `Inherit`, which resolves to native
    /// whenever the backend advertises one (`BackendCaps::native_menu`,
    /// `true` for `MacBackend`) — see `render::context_menu_should_be_native`.
    /// So by default, a right-click on macOS must **not** paint the
    /// in-window `ContextMenuPanel` at all; `Backend::show_context_menu`
    /// takes over instead. This test never triggers that native popup at
    /// all (setting `engine.context_menu` directly, not going through a
    /// real right-click), so it only proves the *other* half of the
    /// acceptance bar — the one a headless test *can* prove: nothing paints
    /// in-window when native is resolved.
    ///
    /// RED against the pre-#902 body (`paint_context_menu_rung` had no
    /// native branch and always drew in-window): every item label,
    /// including "Go to Definition", painted onto the screen regardless of
    /// backend capability.
    #[test]
    fn native_menu_style_suppresses_the_in_window_context_menu() {
        let mut engine = plain_engine();
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine.open_editor_context_menu(4, 4);
        assert!(
            engine
                .context_menu
                .as_ref()
                .is_some_and(|m| !m.items.is_empty()),
            "fixture needs a non-empty context menu — an empty one is not \
             painted either way and would make this test meaningless"
        );

        let (_guards, driver) = driver(engine);

        assert!(
            !driver.screen_contains("Go to Definition"),
            "MacBackend declares `native_menu: true` and `menu_style` \
             defaults to `Inherit`, so the in-window context menu must not \
             paint; painted text was {:?}",
            driver.painted_texts()
        );
    }

    /// #902: `menu_style = Custom` opts back into the in-window path even
    /// on a backend that has a native one — the VS Code-parity escape
    /// hatch (`window.menuStyle: custom`) this setting exists to mirror.
    ///
    /// RED against a body that ignores `menu_style` and always resolves
    /// native on a capable backend: the in-window menu never paints even
    /// with `Custom` explicitly set, and this assertion fails.
    #[test]
    fn custom_menu_style_still_paints_the_in_window_context_menu() {
        let mut engine = plain_engine();
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine.settings.menu_style = crate::core::settings::MenuStyle::Custom;
        engine.open_editor_context_menu(4, 4);

        let (_guards, driver) = driver(engine);

        assert!(
            driver.screen_contains("Go to Definition"),
            "menu_style = Custom must still paint the in-window context \
             menu even though MacBackend has a native one; painted text \
             was {:?}",
            driver.painted_texts()
        );
    }

    // ── #937: Nerd-Font fallback registration ───────────────────────────

    /// A plain engine with nerd fonts explicitly **on**. Every fixture above
    /// (`plain_engine`, `engine_with_minimap`) deliberately sets it `false`
    /// to dodge the #620 tab-icon `debug_assert!`, an already-documented,
    /// unrelated macOS gap — which means no test in this file, before this
    /// one, ever exercised the nerd-fonts-on paint path on `MacBackend` at
    /// all.
    fn engine_with_nerd_fonts_on() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine.settings.use_nerd_fonts = Some(true);
        engine
    }

    /// #937: `App::setup` now calls `render::register_nerd_font_fallback`,
    /// which registers the bundled Nerd Font subset with the backend and
    /// points its glyph-fallback cascade at it
    /// (`Backend::register_font_from_memory` + `Backend::
    /// set_nerd_font_fallback`, quadraui#929) — the two methods this issue
    /// is about ("app never calls register_font_from_memory/
    /// set_nerd_font_fallback anywhere in src/").
    ///
    /// **What this test can and cannot prove.** It proves the first frame
    /// completes — with activity bar / status bar icon glyphs actually on
    /// the paint path — the moment nerd fonts are genuinely on, which no
    /// other test in this file exercised before (see
    /// `engine_with_nerd_fonts_on`'s doc). A broken
    /// `register_nerd_font_fallback` call (e.g. bytes `MacBackend::
    /// register_font_from_memory` can't parse, or a panic in the new call
    /// itself) would surface here first.
    ///
    /// It does **not** prove the icon glyph actually resolves against the
    /// registered Nerd Font rather than painting as a tofu box. quadraui
    /// reaches the same conclusion about its own conformance suite, and
    /// says so in writing. Verified by reading the pinned rev's source
    /// (`68f0ef9075ad87ab5f0641beb3cb8f57c0ce4f9f`, the full hash
    /// `Cargo.lock` resolves for this branch):
    ///
    /// - `quadraui/src/backend.rs:359` — `BackendCaps` has a
    ///   `pub app_font_registration: bool` field, and it is part of the
    ///   `vocabulary()` name list (`backend.rs:484`).
    /// - `quadraui/src/macos/backend.rs:1255` — `MacBackend` declares
    ///   `app_font_registration: true`, with a dedicated test at
    ///   `backend.rs:4072`
    ///   (`mac_backend_declares_app_font_registration_capability`).
    /// - `quadraui/quadraui/tests/conformance.rs:1105` — the crate-local
    ///   `UNGATED_CAPS` array (nine entries, in
    ///   `every_capability_is_required_by_some_scenario_or_named_as_unused`)
    ///   lists `app_font_registration` with this reason: "declared by GTK
    ///   (which already had a working fallback before #929 and overrides
    ///   `set_nerd_font_fallback` for portability) plus macOS/Win-GUI,
    ///   neither of which has a `ConformanceDriver` (#493) — and even GTK's
    ///   own coverage would need per-glyph font-resolution inspection
    ///   `FrameInventory` doesn't do (it records painted text runs, not
    ///   which family resolved each character), so there is no headless
    ///   assertion this suite's vocabulary can gate on".
    ///
    /// That last point is the same wall this test hits, for the same
    /// reason: `FrameInventory`'s painted *text runs* record the requested
    /// string, not the family each character resolved against. On top of
    /// it, `Backend` is sealed (`backend.rs:494`, a `pub(crate) mod sealed`
    /// supertrait) so vimcode cannot substitute a spying implementation,
    /// and `MacBackend`'s `nerd_font_fallback_family` field
    /// (`macos/backend.rs:166`) is private with no public getter — nothing
    /// outside quadraui's own crate can read it.
    ///
    /// So this is an honest, unverified gap, independently corroborated by
    /// quadraui's own reasoning above: confirming the glyph itself paints
    /// correctly needs either a human looking at a real Mac's screen (see
    /// this PR's smoke items), or a new quadraui-side introspection API
    /// (e.g. exposing which font family a painted run actually resolved
    /// against) added for exactly this. No such API exists at `68f0ef9` —
    /// a quadraui issue requesting it should be filed as a follow-up
    /// rather than assumed to already exist.
    ///
    /// **RED-verification honesty note.** This test's assertion
    /// (`screen_contains("fn main")`) does not depend on nerd-font
    /// resolution at all — it would stay green even with the entire body
    /// of `register_nerd_font_fallback` deleted, or with
    /// `set_nerd_font_fallback` never called, because the buffer text it
    /// checks paints through an unrelated path. That is not a claim this
    /// test happens to be weak; it is analytically true from reading the
    /// assertion, independent of platform, and it could not be
    /// double-checked by actually deleting the fix and re-running here
    /// (`#[cfg(all(feature = "macos", target_os = "macos"))]` above means
    /// this module does not even compile on the Linux host this fix was
    /// written on — see the file's "Verifying this file without a Mac"
    /// doc). What this test *does* cover, honestly: that `App::setup`
    /// calling `register_nerd_font_fallback` does not panic or otherwise
    /// break the first paint on `MacBackend` with nerd fonts on, a path no
    /// prior test in this file exercised. Actual glyph-resolution coverage
    /// remains the open gap described above.
    #[test]
    fn setup_with_nerd_fonts_on_registers_the_fallback_and_still_paints() {
        let (_guards, driver) = driver(engine_with_nerd_fonts_on());

        assert!(
            driver.screen_contains("fn main"),
            "the first frame must still complete and paint the buffer with \
             nerd fonts on, exercising the new `register_nerd_font_fallback` \
             call from `App::setup`; painted text was {:?}",
            driver.painted_texts()
        );
    }

    // ── #940: client-side titlebar / native traffic-light inset ────────

    /// #940's acceptance section asks for a `MacDriver` black-box test
    /// proving two things end to end: vimcode's drawn window controls do
    /// not paint when the backend reports it draws its own, and painted
    /// title-band content starts clear of the reported control inset. No
    /// such test exists in this file, and — as of the pinned rev
    /// (`b6000c4`) — none can: `MacDriver::new` above never calls
    /// `MacBackend::set_window`, and there is no other way to make it do
    /// so from this crate.
    ///
    /// Three independent things all have to be true simultaneously for
    /// `Backend::titlebar_control_inset()` to return anything other than
    /// `Rect::default()`, and all three currently hold:
    ///
    /// 1. `MacBackend::titlebar_control_inset` (`macos/backend.rs:1351`)
    ///    short-circuits to `Rect::default()` whenever no window has been
    ///    set — pinned by quadraui's own
    ///    `titlebar_control_inset_default_without_window` test right next
    ///    to it (`macos/backend.rs:3816`).
    /// 2. The only setter, `MacBackend::set_window`
    ///    (`macos/backend.rs:451`), is `pub(crate)` to quadraui — this
    ///    crate cannot call it, and `MacDriver::new` (this file's
    ///    quadraui counterpart, `macos/testing.rs:130`) never does either.
    /// 3. `Backend` itself is sealed (`backend.rs:488-614`, a
    ///    `pub(crate) sealed::Sealed` supertrait), so vimcode cannot work
    ///    around (2) by substituting its own `Backend` impl that reports a
    ///    fake inset — not even a thin wrapper delegating everything else
    ///    to a real backend. `quadraui::testing::RecordingBackend`, the
    ///    one other publicly constructible `Backend` impl this crate can
    ///    reach, does not override `titlebar_control_inset` either, so it
    ///    inherits the same all-zero default the trait itself declares
    ///    (`backend.rs:1193`) — checked directly against the pinned
    ///    source, not assumed.
    ///
    /// A live `NSWindow` is unavoidable, and quadraui's own `MacDriver`
    /// doc says as much in its "Limitations" section: real `NSEvent`
    /// delivery and window-backed behaviour "need a live-window smoke
    /// test instead" of the headless `CGBitmapContext` path every test in
    /// this file uses. This gap is exactly that category — a real Mac
    /// running the actual app is the only thing that can drive a non-zero
    /// inset today, which is why it is one of this PR's `SMOKE_TESTS`
    /// items rather than an automated test here.
    ///
    /// Closing this properly needs a quadraui-side testing hook (e.g. a
    /// `MacDriver` constructor, or a `MacBackend` setter, that can inject
    /// a non-default `titlebar_control_inset()` without a real window) —
    /// a quadraui issue to file, not a vimcode workaround
    /// (`CLAUDE.md`'s Platform-Neutrality Rule: file upstream, wait, then
    /// implement). Until it lands, the logic this test would otherwise
    /// exercise is covered as pure, backend-independent unit tests
    /// instead — `render::backend_draws_own_window_controls`,
    /// `render::should_draw_window_controls`, and
    /// `render::inset_titlebar_row_leading_edge` in `src/render.rs` — and
    /// the trip wire right below pins today's status quo so this comment
    /// cannot silently go stale if quadraui ever does start setting a
    /// window here.
    ///
    /// RED-verification note: there is nothing to make RED here — this
    /// test asserts what the pinned quadraui rev provably always returns,
    /// not a vimcode behaviour that could regress. Its job is the
    /// opposite: it goes RED the day `MacDriver` starts setting a window
    /// (or a version bump otherwise changes this), which is exactly the
    /// signal that the black-box test the issue actually asks for finally
    /// becomes possible.
    #[test]
    fn control_inset_is_default_because_mac_driver_never_sets_a_window() {
        use quadraui::Backend;

        let (_guards, driver) = driver(plain_engine());

        assert_eq!(
            driver.backend().titlebar_control_inset(),
            quadraui::Rect::default(),
            "MacDriver's backend reported a non-default titlebar control \
             inset without ever calling MacBackend::set_window -- if this \
             fires, quadraui has changed and the #940 suppression/inset \
             path can likely now get real MacDriver black-box coverage; \
             see this test's doc comment"
        );
    }

    // ── #967: explorer row hit-band integrity ───────────────────────────

    /// Build a temp directory with `filler_dirs` sibling directories (so
    /// `src` is not row 1 — #967's drift is row-index-dependent, with no
    /// visible effect on the first two or three rows and a growing mis-hit
    /// below that) plus a `src/core` child, matching this issue's own
    /// live-use repro: "with a folder open and `src/` expanded, clicking
    /// near the bottom of the word `src` toggles the `core/` row beneath
    /// it".
    fn scratch_explorer_dir(tag: &str, filler_dirs: usize) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_967_macos_explorer_{tag}_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..filler_dirs {
            std::fs::create_dir_all(dir.join(format!("aaa_filler_{i}"))).unwrap();
        }
        std::fs::create_dir_all(dir.join("src").join("core")).unwrap();
        dir
    }

    /// An engine whose explorer tree is open, rooted at `dir`, with both
    /// the root and `src` expanded — the precondition #967's repro needs:
    /// `src`'s child `core` painted directly beneath it.
    fn engine_with_expanded_explorer(dir: &std::path::Path) -> Engine {
        let mut engine = plain_engine();
        engine.cwd = dir.to_path_buf();
        engine.explorer_expanded.insert(dir.to_path_buf());
        engine.explorer_expanded.insert(dir.join("src"));
        engine.explorer_rebuild_rows();
        engine.session.explorer_visible = true;
        engine
    }

    /// #967: a click anywhere inside the `src` row's own painted glyphs
    /// must resolve to `src` — the row it is painted on — never to `core`,
    /// its child painted immediately below it. A single mouse-down on a
    /// directory row toggles that row's expansion
    /// (`Engine::handle_explorer_mouse_event`'s `RowSelected` arm →
    /// `explorer_toggle_dir`), and toggling twice at the same point always
    /// restores whichever row actually got hit — so
    /// [`crate::harness::sweep_hit_band_integrity`] can use "is `core` still
    /// painted after one click-then-click-again round-trip" as its
    /// fingerprint with no per-sample bookkeeping of its own: correctly
    /// hitting `src` collapses it (hiding `core`) before the second click
    /// re-expands it; incorrectly hitting `core` merely flips `core`'s own
    /// (empty, so invisible) expansion twice, leaving `core` visible the
    /// whole time. Any sweep point that disagrees with the top-of-row
    /// baseline is exactly #967's bug.
    ///
    /// **RED-verification (#967):** reverting `TextMetricsBackend for
    /// quadraui::macos::MacBackend`'s two metric setters in this file back
    /// to their pre-fix no-op bodies takes this test red — the sweep's
    /// lower sample points mis-hit `core` a row down instead of `src`,
    /// disagreeing with the top-of-row baseline, and
    /// `sweep_hit_band_integrity`'s `assert_eq!` fires. Confirmed locally
    /// with `cargo test --no-default-features --features macos
    /// explorer_click_hit_band_matches_the_painted_row` before restoring
    /// the fix; see this issue's PR notes.
    #[test]
    fn explorer_click_hit_band_matches_the_painted_row() {
        use quadraui::testing::ConformanceDriver;

        let dir = scratch_explorer_dir("scenario1", 6);
        let (_guards, mut driver) = driver(engine_with_expanded_explorer(&dir));

        assert!(
            driver.screen_contains("src") && driver.screen_contains("core"),
            "precondition: the explorer must paint both `src` and its \
             expanded child `core`; painted text was {:?}",
            driver.painted_texts()
        );

        crate::harness::sweep_hit_band_integrity(&mut driver, "src", 5, |d| {
            ConformanceDriver::inventory(d).screen_has("core")
        });

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #969: the direct, driver-free half of the coverage above —
    /// `explorer_click_hit_band_matches_the_painted_row` proves #967's
    /// *symptom* (a mis-hit row) is fixed; this proves the *mechanism* is
    /// sound by round-tripping a value through `TextMetricsBackend` and
    /// `quadraui::Backend` directly, no paint or click involved. See
    /// `crate::harness::assert_text_metrics_backend_applies_metrics`'s doc
    /// for why a `&mut self`-with-no-return setter needs exactly this kind
    /// of check to catch a silent stub — the class of bug #967 was.
    #[test]
    fn mac_backend_applies_line_height_and_char_width() {
        let mut backend = MacBackend::new();
        crate::harness::assert_text_metrics_backend_applies_metrics(&mut backend);
    }

    // ── #971: hit-band integrity, the remaining surfaces ────────────────
    //
    // #967/#968 wired `crate::harness::sweep_hit_band_integrity` to one
    // surface (the explorer tree, just above). This issue points it at
    // the source-control panel, the tab bar, the plugin ext-panel body,
    // and the unified picker — one fixture + one selector per surface, per
    // CLAUDE.md's "add tests incrementally" rule.

    /// A git-panel engine with three filler unstaged files (pushes
    /// "RECENT COMMITS" a few rows down the panel — mirrors #967's own
    /// `filler_dirs`: a row-index-dependent hit-band bug can pass on row 0
    /// and only surface further down) and two log entries, so there is a
    /// content row directly beneath the header a mis-hit could land on.
    fn engine_with_sc_recent_commits() -> Engine {
        let mut engine = plain_engine();
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_GIT,
        ));
        engine.sc_file_statuses = (0..3)
            .map(|i| crate::core::git::FileStatus {
                path: format!("filler_971_{i}.rs"),
                staged: None,
                unstaged: Some(crate::core::git::StatusKind::Modified),
                unmerged: None,
            })
            .collect();
        engine.sc_log = (0..2)
            .map(|i| crate::core::git::GitLogEntry {
                hash: format!("{i:07x}"),
                message: format!("ZQXW971SCLOG{i}"),
            })
            .collect();
        engine
    }

    /// #971: the source-control panel's "RECENT COMMITS" section header,
    /// clicked anywhere inside its own painted glyphs, must always toggle
    /// *that* section — never the log row painted immediately below it.
    ///
    /// The SC panel routes every press through the *cached* `SidebarSystem`
    /// layout (`Engine::handle_sc_sidebar_ui_event` ->
    /// `sc_sidebar_system.handle_cached`) — the "good" pattern #971 calls
    /// out, with no per-frame row arithmetic re-derived from a raw `y`.
    ///
    /// Uses `crate::harness::sweep_hit_band_integrity_resetting`, not
    /// `sweep_hit_band_integrity`: quadraui's `SidebarSystem` treats two
    /// `MouseDown`s at the same point in quick succession as a
    /// `DoubleClick` (the same real-world double-click detection a live
    /// GUI does), and `SidebarSystem::double_click` has no header case
    /// (`_ => SidebarEvent::Ignored`) — so a headless driver's
    /// back-to-back click-then-click-to-restore on a header (no real-world
    /// delay between them, unlike a real user) gets coalesced into a
    /// double-click that silently does nothing, rather than toggling back
    /// the way it does for #967's tree row. `setup` instead re-expands the
    /// section directly (`set_collapsed(SC_SECTION_LOG, false)`) before every
    /// sample — no second click involved at all.
    ///
    /// Fingerprint: is the first log entry's distinctive message still
    /// painted? A correct hit collapses the log section, hiding the log rows; a
    /// mis-hit lands on the log row itself (`SidebarEvent::RowSelected`),
    /// which changes nothing painted, disagreeing with the header-hit
    /// baseline.
    #[test]
    fn sc_panel_header_click_hit_band_matches_the_painted_row() {
        use quadraui::testing::ConformanceDriver;

        let (_guards, engine, mut driver) = driver_with_engine(engine_with_sc_recent_commits());

        assert!(
            driver.screen_contains("RECENT COMMITS") && driver.screen_contains("ZQXW971SCLOG0"),
            "precondition: the SC panel must paint both the RECENT COMMITS \
             header and its first log entry; painted text was {:?}",
            driver.painted_texts()
        );

        // Sanity: the sweep below only compares samples against *each
        // other*, so a header click that silently did nothing would still
        // pass every sample uniformly. Prove the click has real effect
        // first, so the sweep cannot pass vacuously.
        let center = center_of(&driver, "RECENT COMMITS");
        driver.click(center.0, center.1);
        assert!(
            !driver.screen_contains("ZQXW971SCLOG0"),
            "sanity: a header click must actually collapse the section, \
             hiding the log entry; painted text was {:?}",
            driver.painted_texts()
        );
        engine
            .borrow_mut()
            .sc_sidebar_system
            .borrow_mut()
            .set_collapsed(crate::core::engine::SC_SECTION_LOG, false);
        driver.render();
        assert!(
            driver.screen_contains("ZQXW971SCLOG0"),
            "sanity restore: re-expanding the section directly must bring \
             the log entry back; painted text was {:?}",
            driver.painted_texts()
        );

        crate::harness::sweep_hit_band_integrity_resetting(
            &mut driver,
            "RECENT COMMITS",
            5,
            |d| {
                // Consecutive samples click within ~3.8px of each other —
                // inside `MacBackend`'s own 4px/400ms `DoubleClickDetector`
                // radius (real double-click detection, the same a live
                // click would trigger), so back-to-back probe clicks would
                // otherwise fold into a `DoubleClick`, which
                // `SidebarSystem::double_click` doesn't handle for headers
                // (this test's own doc). A throwaway click far outside the
                // panel breaks the position match before every real probe.
                d.click(W as f32 - 20.0, H as f32 - 20.0);
                engine
                    .borrow_mut()
                    .sc_sidebar_system
                    .borrow_mut()
                    .set_collapsed(crate::core::engine::SC_SECTION_LOG, false);
                d.render();
            },
            |d| ConformanceDriver::inventory(d).screen_has("ZQXW971SCLOG0"),
        );
    }

    /// Centre point of the first painted text run containing `needle` — the
    /// same lookup `crate::harness::sweep_hit_band_integrity` does
    /// internally, exposed here for the sanity clicks each #971 test does
    /// *before* handing off to the sweep.
    fn center_of<D: quadraui::testing::ConformanceDriver>(driver: &D, needle: &str) -> (f32, f32) {
        let bounds = driver
            .inventory()
            .text_runs()
            .iter()
            .find(|r| r.text.contains(needle))
            .unwrap_or_else(|| panic!("center_of: {needle:?} not painted"))
            .bounds;
        (
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        )
    }

    /// A scratch directory for the tab-bar / picker sweeps below, mirroring
    /// [`scratch_explorer_dir`]'s own naming/cleanup shape.
    fn scratch_dir_971(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_971_macos_{tag}_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Two tabs in a single group, both backed by real files with distinct
    /// bodies so which tab is *active* is readable from painted editor
    /// content, not tab-bar chrome. `b` is opened last (and so starts
    /// active).
    ///
    /// Deliberately just two tabs, no filler: `AppShell`'s sidebar is
    /// visible by default and paints at `x: 48..374`, squarely under
    /// where an *earlier* tab (`a`, at `x` ~145..297) would land at this
    /// window width — `App::try_route_sidebar_mouse_event` arbitrates the
    /// sidebar ahead of the tab bar, so a click on `a` never reaches
    /// `click::pixel_to_click_target` at all. Padding tabs past the
    /// sidebar to reach `a` was tried and reverted (#971 PR notes): a
    /// filler wide enough to clear `x=374` throws off `TabBarPixelHits`'
    /// slot indices enough that a "click on `a`" lands on the filler
    /// instead, and hiding the sidebar needs the real
    /// `EngineAction::ToggleSidebar` -> `App::sync_sidebar_from_engine`
    /// round trip a driver-level click takes, not a fixture-time `Engine`
    /// mutation (`engine.app_shell` is a snapshot `App::shell_config`
    /// reads once at construction, not the runner's own live copy).
    /// `b`'s own row (`x` ~344..496, centre ~421) already clears the
    /// sidebar as the *last* tab in a two-tab bar, which is what this
    /// test's own sweep exploits instead: [`tab_bar_click_hit_band_matches_the_painted_tab`]'s
    /// own doc has the fingerprint this shape enables.
    fn engine_with_two_tabs(dir: &std::path::Path) -> Engine {
        let file_a = dir.join("zqxw971_tab_a.txt");
        let file_b = dir.join("zqxw971_tab_b.txt");
        std::fs::write(&file_a, "ZQXW971TABBODYA\n").unwrap();
        std::fs::write(&file_b, "ZQXW971TABBODYB\n").unwrap();
        let mut engine = plain_engine();
        engine.cwd = dir.to_path_buf();
        engine.new_tab(Some(&file_a));
        engine.new_tab(Some(&file_b));
        engine
    }

    /// #971: `b`'s own painted label, clicked anywhere inside its glyph
    /// band, must always resolve to `b` — never `a`, its left neighbour.
    /// #515's right-edge mis-hit ("clicking near a tab's right edge lands
    /// on the next tab") is exactly the defect class this sweep's
    /// top/middle/bottom probes exist to catch (see
    /// `crate::harness::sweep_hit_band_integrity`'s own doc) — `b` is the
    /// *second* of two tabs, so a rightward mis-hit has nowhere further
    /// to land and a leftward one lands squarely on `a`.
    ///
    /// `b` starts active (opened last), so a click anywhere correctly
    /// inside its own band is a no-op — same reasoning
    /// `sweep_hit_band_integrity`'s own doc gives for why a non-restoring
    /// click is fine here: the fingerprint reads "which tab is active
    /// now", a pure function of the click's actual target, not of
    /// history. A correct hit leaves `b` active (`ZQXW971TABBODYB` stays
    /// painted); a mis-hit onto `a` flips the editor to `a`'s body
    /// instead, disagreeing with the top-of-row baseline.
    ///
    /// The sanity check below can only probe the *positive* baseline (a
    /// hit inside `b`'s own band keeps `b` active) rather than proving a
    /// hit can also switch tabs at all — `a`'s own row is the one behind
    /// the sidebar (see [`engine_with_two_tabs`]'s doc), so this test
    /// cannot drive a real activating click the way
    /// `sc_panel_header_click_hit_band_matches_the_painted_row`'s sanity
    /// check does. Forcing `active_tab` to `a` directly and re-rendering
    /// instead proves the *fingerprint* itself can read both states —
    /// ruling out a fingerprint that is vacuously always-true, the one
    /// failure mode a same-tab-only click could not otherwise catch.
    #[test]
    fn tab_bar_click_hit_band_matches_the_painted_tab() {
        use quadraui::testing::ConformanceDriver;

        let dir = scratch_dir_971("tabbar");
        let (_guards, engine, mut driver) = driver_with_engine(engine_with_two_tabs(&dir));

        assert!(
            driver.screen_contains("zqxw971_tab_b.txt")
                && driver.screen_contains("ZQXW971TABBODYB"),
            "precondition: tab b must have painted its label and be the \
             active tab showing its body; painted text was {:?}",
            driver.painted_texts()
        );

        // Sanity: the fingerprint below (`screen_has("ZQXW971TABBODYB")`)
        // must actually be capable of reading `false` — otherwise the
        // sweep would pass even if every click were a no-op. Force `a`
        // active directly (no click involved) and confirm the painted
        // body follows. Indices are relative to the *end* of the tab
        // list (`b` is last, `a` second-to-last) rather than hardcoded —
        // `Engine::new_for_test`'s own initial "[No Name]" tab (never
        // closed by this fixture) sits at index 0, ahead of both.
        let (a_idx, b_idx) = {
            let e = engine.borrow();
            let n = e.editor_groups[&e.active_group].tabs.len();
            (n - 2, n - 1)
        };
        {
            let mut e = engine.borrow_mut();
            let group = e.active_group;
            e.editor_groups.get_mut(&group).unwrap().active_tab = a_idx;
        }
        driver.render();
        assert!(
            !driver.screen_contains("ZQXW971TABBODYB") && driver.screen_contains("ZQXW971TABBODYA"),
            "sanity: the fingerprint must read false once `a` (not `b`) is \
             active, or the sweep below could pass vacuously; painted \
             text was {:?}",
            driver.painted_texts()
        );
        {
            let mut e = engine.borrow_mut();
            let group = e.active_group;
            e.editor_groups.get_mut(&group).unwrap().active_tab = b_idx;
        }
        driver.render();
        assert!(
            driver.screen_contains("ZQXW971TABBODYB"),
            "sanity restore: b must be active again; painted text was {:?}",
            driver.painted_texts()
        );

        crate::harness::sweep_hit_band_integrity(&mut driver, "zqxw971_tab_b.txt", 5, |d| {
            ConformanceDriver::inventory(d).screen_has("ZQXW971TABBODYB")
        });

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A plugin ext-panel engine: `ext_panel_active` is set directly
    /// (bypassing `AppShell` registration, which real ext panels also
    /// bypass — see `render::apply_activity_panel_switch`'s own doc) and
    /// the sidebar shown via the raw `AppShell::toggle_sidebar`, not
    /// `Engine::toggle_sidebar` — the latter persists to the developer's
    /// real session file, the exact reason `crate::gtk::testing::
    /// sidebar_panel_clicks::panel_harness` avoids it too.
    ///
    /// One "available" (not-installed) extension in `ext_registry` gives
    /// the "AVAILABLE" section content, while "INSTALLED" stays empty —
    /// pushing the target header down a row, the same filler technique
    /// [`engine_with_sc_recent_commits`] uses.
    fn engine_with_marketplace_as_ext_panel() -> Engine {
        let mut engine = plain_engine();
        engine.ext_registry = Some(vec![crate::core::extensions::ExtensionManifest {
            name: "zqxw971-avail".to_string(),
            display_name: "ZQXW971 Available Ext".to_string(),
            ..Default::default()
        }]);
        engine.ext_panel_active = Some("zqxw971-marketplace-via-ext-panel".to_string());
        engine.ext_panel_has_focus = true;
        if !engine.app_shell.sidebar_visible() {
            engine.app_shell.toggle_sidebar();
        }
        engine
    }

    /// #971: the ext-panel body's "AVAILABLE" section header, clicked
    /// anywhere inside its own painted glyphs, must always toggle *that*
    /// section — never the row painted immediately below it.
    ///
    /// This exercises `Engine::handle_ext_sidebar_ui_event` ->
    /// `ext_sidebar_system.handle_cached`, the same cached-`SidebarSystem`
    /// pattern the SC panel test above pins. It does **not** exercise
    /// `render::SidebarBodyGeometry::content_row` — #971's own
    /// "highest-suspicion" independent row formula — because that formula
    /// is wired *only* to `render::route_sidebar_hover`'s `ExtPanel` arm
    /// (a `MouseMoved`-only path, never a click), and the hover it drives
    /// only ever produces a *delayed* (350ms dwell) popup gated on a
    /// second, currently-disconnected registry
    /// (`Engine::resolve_panel_hover_item_id` reads `ext_panels`/
    /// `ext_panel_items`, populated only for a plugin with a live
    /// registration — unrelated to what `ext_sidebar_system` actually
    /// paints here). Neither half produces an immediately-painted signal a
    /// headless driver can read without first fixing that unrelated
    /// mismatch, which is out of this issue's scope. See this issue's PR
    /// notes for the follow-up this gap needs.
    ///
    /// Uses `sweep_hit_band_integrity_resetting`, not
    /// `sweep_hit_band_integrity` — see
    /// `sc_panel_header_click_hit_band_matches_the_painted_row`'s own
    /// comment on the double-click coalescing this sidesteps. "AVAILABLE"
    /// is section 1 (`ext_sidebar_system`'s own `SidebarSectionDef` order:
    /// `["installed", "available"]`, `Engine::new`).
    ///
    /// Fingerprint: is the one available extension's distinctive display
    /// name still painted? A correct hit collapses the "available"
    /// section, hiding its one row; a mis-hit lands on the row itself
    /// (`SidebarEvent::RowSelected`), which changes nothing painted,
    /// disagreeing with the header-hit baseline.
    #[test]
    fn ext_panel_header_click_hit_band_matches_the_painted_row() {
        use quadraui::testing::ConformanceDriver;

        let (_guards, engine, mut driver) =
            driver_with_engine(engine_with_marketplace_as_ext_panel());

        assert!(
            driver.screen_contains("AVAILABLE") && driver.screen_contains("ZQXW971 Available Ext"),
            "precondition: the ext panel must paint the AVAILABLE header \
             and its one row; painted text was {:?}",
            driver.painted_texts()
        );

        // Sanity — see `sc_panel_header_click_hit_band_matches_the_painted_row`'s
        // own comment on why this is needed before trusting the sweep below.
        let center = center_of(&driver, "AVAILABLE");
        driver.click(center.0, center.1);
        assert!(
            !driver.screen_contains("ZQXW971 Available Ext"),
            "sanity: a header click must actually collapse the AVAILABLE \
             section, hiding its one row; painted text was {:?}",
            driver.painted_texts()
        );
        engine
            .borrow_mut()
            .ext_sidebar_system
            .borrow_mut()
            .set_collapsed(1, false);
        driver.render();
        assert!(
            driver.screen_contains("ZQXW971 Available Ext"),
            "sanity restore: re-expanding the section directly must bring \
             its row back; painted text was {:?}",
            driver.painted_texts()
        );

        crate::harness::sweep_hit_band_integrity_resetting(
            &mut driver,
            "AVAILABLE",
            5,
            |d| {
                // Break `MacBackend`'s `DoubleClickDetector` position match
                // before every real probe — see
                // `sc_panel_header_click_hit_band_matches_the_painted_row`'s
                // identical comment for the full story.
                d.click(W as f32 - 20.0, H as f32 - 20.0);
                engine
                    .borrow_mut()
                    .ext_sidebar_system
                    .borrow_mut()
                    .set_collapsed(1, false);
                d.render();
            },
            |d| ConformanceDriver::inventory(d).screen_has("ZQXW971 Available Ext"),
        );
    }

    /// #971: a unified-picker (fuzzy file finder) result row, clicked
    /// anywhere inside its own painted glyphs, must always select *that*
    /// row — never its neighbour.
    ///
    /// Uses `crate::harness::sweep_hit_band_integrity_resetting`, not
    /// `sweep_hit_band_integrity`: `render::apply_picker_row_click` closes
    /// the popup outright the moment a click lands on an *already*-selected
    /// row (see that function's own #971 doc comment), so a plain
    /// click-then-click-to-restore would dismiss the picker after sample
    /// 0's restore click. `setup` instead re-opens the picker fresh (file
    /// `a` selected, its preview loaded) before every sample.
    ///
    /// Fingerprint: is file `b`'s distinctive body painted in the preview
    /// pane? A correct hit on `b`'s row is not yet selected (the picker
    /// always re-opens onto `a`), so it only selects `b` and loads its
    /// preview — never confirms. A neighbour mis-hit (back onto `a`, still
    /// selected) is a no-op, leaving `a`'s preview on screen instead.
    #[test]
    fn picker_row_click_hit_band_matches_the_painted_row() {
        use crate::core::engine::PickerSource;
        use quadraui::testing::ConformanceDriver;

        let dir = scratch_dir_971("picker");
        std::fs::write(dir.join("zqxw971_picka.txt"), "ZQXW971PICKABODY\n").unwrap();
        std::fs::write(dir.join("zqxw971_pickb.txt"), "ZQXW971PICKBBODY\n").unwrap();

        let mut engine = plain_engine();
        engine.cwd = dir.clone();
        engine.open_picker(PickerSource::Files);
        let (_guards, engine, mut driver) = driver_with_engine(engine);

        assert!(
            driver.screen_contains("zqxw971_picka.txt")
                && driver.screen_contains("zqxw971_pickb.txt"),
            "precondition: the picker must paint both files; painted text \
             was {:?}",
            driver.painted_texts()
        );
        assert!(
            driver.screen_contains("ZQXW971PICKABODY"),
            "precondition: the default selection (file a, alphabetically \
             first) must preview file a's body; painted text was {:?}",
            driver.painted_texts()
        );

        // Sanity — see `sc_panel_header_click_hit_band_matches_the_painted_row`'s
        // own comment on why this is needed before trusting the sweep
        // below: `sweep_hit_band_integrity_resetting` only compares samples
        // against each other, so a row click that silently did nothing
        // would still pass every sample uniformly.
        let center_b = center_of(&driver, "zqxw971_pickb.txt");
        driver.click(center_b.0, center_b.1);
        assert!(
            driver.screen_contains("ZQXW971PICKBBODY"),
            "sanity: a click on file b's own row must select it and load \
             its preview; painted text was {:?}",
            driver.painted_texts()
        );

        crate::harness::sweep_hit_band_integrity_resetting(
            &mut driver,
            "zqxw971_pickb.txt",
            5,
            |d| {
                engine.borrow_mut().close_picker();
                engine.borrow_mut().open_picker(PickerSource::Files);
                d.render();
            },
            |d| ConformanceDriver::inventory(d).screen_has("ZQXW971PICKBBODY"),
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
