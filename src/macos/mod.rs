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

use crate::app::App;

// #1497: the local `TextMetricsBackend for quadraui::macos::MacBackend`
// impl that used to live here is gone. It forwarded
// `set_current_line_height`/`set_current_char_width` onto `MacBackend`'s
// own inherent setters (quadraui#934, pinned rev `f3b3aed9`) purely because
// `quadraui::Backend` had no portable equivalent of them; JDonaghy/
// quadraui#1086 put both directly on `Backend`, with `MacBackend`'s own
// override doing exactly the forwarding this impl used to do by hand — see
// `crate::harness::assert_text_metrics_backend_applies_metrics`'s doc for
// the #967 stub-setter bug this conformance-checks against.

/// Entry point for the native macOS GUI, mirroring `crate::gtk::run`.
///
/// Panic hook + swap flush, choose the backend, construct the shared
/// [`App`], derive its [`quadraui::ShellConfig`] via [`build_shell_config`],
/// hand both to the runner. Nothing else — no `gtk4::init` equivalent,
/// because `quadraui::macos::run` does AppKit's own bootstrap (main-thread
/// check, `NSApplication`, default font) itself.
pub fn run(file_path: Option<PathBuf>) -> ExitCode {
    // The same panic hook `crate::gtk::run` installs: flush every dirty
    // buffer to its swap file, then write a crash log.
    crate::core::swap::install_gui_crash_hook();

    // The concrete backend is chosen here, at the entry point, and handed to
    // `App` — the seam #861 opened and `src/gtk/mod.rs::run` names in its own
    // comment as the one "a future non-GTK wrapper (#859) would pass a
    // different `quadraui::Backend` impl through". This is that wrapper.
    let backend: std::rc::Rc<std::cell::RefCell<Box<dyn quadraui::Backend>>> = std::rc::Rc::new(
        std::cell::RefCell::new(Box::new(quadraui::macos::MacBackend::new())),
    );

    let app = App::new_portable(file_path, backend, crate::render::UnitProfile::px());
    let config = build_shell_config(&app);
    quadraui::macos::shell_runner::run_with_shell(app, config)
}

/// Derive the runner's [`quadraui::ShellConfig`] from an [`App`]'s engine
/// state — the macOS twin of `crate::gtk::build_shell_config`.
///
/// Adds only [`quadraui::ShellConfig::with_app_icon`] on top of
/// `app.shell_config()`: #1531/quadraui#1142's Dock/Cmd-Tab icon. A bare
/// unbundled binary has no `Info.plist` `CFBundleIconFile` to supply one, so
/// it falls back to the generic executable glyph unless something sets one
/// at runtime — this is that something. Split out (rather than inlined in
/// [`run`]) so a headless test can assert the bytes reach `ShellConfig`
/// without needing a live `NSApplication`.
pub(crate) fn build_shell_config(app: &App) -> quadraui::ShellConfig {
    app.shell_config()
        .with_app_icon(quadraui::ImageSource::Bytes(
            crate::app_support::APP_ICON_PNG.to_vec(),
        ))
}

/// #1531/quadraui#1142: same reasoning as `crate::gtk`'s
/// `shell_config_identity_tests` — there is no headless Dock to render into
/// and assert on (that's the SMOKE_TESTS item, run on real hardware), but a
/// headless build *can* assert the bytes reach the `ShellConfig` the real
/// `run` hands `run_with_shell`, and that they decode as a real image.
/// `#[cfg(test)]` only, gated by the whole module's own
/// `all(feature = "macos", target_os = "macos")` — this never runs off a
/// Mach-O host, matching every other test in this file.
#[cfg(test)]
mod shell_config_identity_tests {
    use super::{build_shell_config, App};
    use crate::core::Engine;
    use quadraui::macos::MacBackend;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn app_icon_reaches_shell_config_as_a_decodable_image() {
        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        let backend: Rc<RefCell<Box<dyn quadraui::Backend>>> =
            Rc::new(RefCell::new(Box::new(MacBackend::new())));
        let app = App::new_headless_with_backend(engine, backend, crate::render::UnitProfile::px());
        let config = build_shell_config(&app);
        let quadraui::ImageSource::Bytes(bytes) = config
            .app_icon
            .expect("build_shell_config sets an app icon")
        else {
            panic!("app icon should be embedded bytes, not a path");
        };
        assert!(!bytes.is_empty());
        assert!(
            image::load_from_memory(&bytes).is_ok(),
            "app icon bytes must decode as an image"
        );
    }
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
    let backend: std::rc::Rc<std::cell::RefCell<Box<dyn quadraui::Backend>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Box::new(MacBackend::new())));
    let (app, config) = crate::harness::build_app_and_config(
        std::rc::Rc::clone(&engine),
        backend,
        crate::render::UnitProfile::px(),
    );
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

    use crate::app::App;
    use crate::core::Engine;

    /// Surface size in points — wide enough that the minimap's own column is
    /// laid out rather than clamped away, which is what puts
    /// `Backend::draw_minimap` on the paint path at all.
    const W: u32 = 1400;
    const H: u32 = 900;

    /// An in-memory engine with enough lines for the minimap to have
    /// something to draw, and the minimap explicitly **on** — (#1858: the
    /// minimap now defaults *off* everywhere) pinned explicitly here
    /// regardless of which way the default points, since the whole point of
    /// this fixture is to reach `draw_minimap`.
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
        let backend: Rc<RefCell<Box<dyn quadraui::Backend>>> =
            Rc::new(RefCell::new(Box::new(MacBackend::new())));
        let app = App::new_headless_with_backend(
            Rc::new(RefCell::new(engine)),
            backend,
            crate::render::UnitProfile::px(),
        );
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
        let backend: Rc<RefCell<Box<dyn quadraui::Backend>>> =
            Rc::new(RefCell::new(Box::new(MacBackend::new())));
        let app = App::new_headless_with_backend(
            Rc::clone(&engine),
            backend,
            crate::render::UnitProfile::px(),
        );
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
            let backend: Rc<RefCell<Box<dyn quadraui::Backend>>> =
                Rc::new(RefCell::new(Box::new(MacBackend::new())));
            let (app, config) = crate::harness::build_app_and_config(
                Rc::clone(&engine),
                backend,
                crate::render::UnitProfile::px(),
            );
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
            let backend: Rc<RefCell<Box<dyn quadraui::Backend>>> =
                Rc::new(RefCell::new(Box::new(MacBackend::new())));
            let (app, config) = crate::harness::build_app_and_config(
                Rc::clone(&engine),
                backend,
                crate::render::UnitProfile::px(),
            );
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

    /// #1576: the sidebar header row (the `" EXPLORER "` strip above the
    /// tree, painted by quadraui's `AppShell::render`) must come from
    /// vimcode's own theme on macOS too — the same regression
    /// `tui_main::app_on_tui_tests::tests::sidebar_panels::
    /// sidebar_header_paints_vimcode_theme_not_quadraui_dark_literal_via_shell_app`
    /// covers for the TUI backend. Before quadraui#1180 (picked up by this
    /// issue's pin bump), `AppShell::render` painted a hard-coded
    /// `Color::rgb(37, 37, 38)` (`#252526`) regardless of theme; under
    /// `vscode-light` that read as a dark band behind a light sidebar.
    /// Samples a pixel inside the header row's own painted-run bounds, on
    /// its leading literal space (background, not a glyph's anti-aliased
    /// edge) — a pixel probe, not `style_at`, because `MacDriver` paints
    /// real pixels via `CGBitmapContext`, not discrete cell styles.
    ///
    /// Confirmed red against the pre-#1576 pin: with the old rev, the
    /// sampled pixel is `(37, 37, 38)`, not `vscode_light`'s status colour
    /// `(0, 122, 204)`.
    #[test]
    fn sidebar_header_paints_vimcode_theme_not_quadraui_dark_literal() {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.settings.colorscheme = "vscode-light".to_string();
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_EXPLORER,
        ));
        engine.session.explorer_visible = true;

        let (_guards, mut driver) = driver(engine);
        // `App::sync_per_frame_backend_state` pushes vimcode's theme onto
        // the backend at the *start* of `render_content`, which quadraui's
        // shell runs *after* `AppShell::render` paints the header chrome —
        // so the header's very first frame still reads whatever
        // `Backend::theme()` returned before any `set_theme` call ever
        // landed (quadraui's own dark `Theme::default()`). A second frame
        // (no input, just a re-render) carries the now-set theme forward,
        // matching what a real window settles on after its first paint.
        driver.render();

        let bounds = driver
            .find_bounds("EXPLORER")
            .expect("the sidebar header must paint its panel title");
        // `find_bounds` returns the whole painted run's bounds — the
        // status-bar segment is `" EXPLORER "` (leading/trailing literal
        // space, `App::paint_sidebar_panel_rung`'s doc), so a couple of
        // points in from the run's own left edge lands on that leading
        // space: still inside the header's own background, not the glyph,
        // and not spilling left into the activity-bar rail's own column
        // (a separate, narrower `draw_activity_bar` paint immediately
        // adjacent — sampling *outside* this run's bounds landed there
        // instead, sampling neither colour).
        let probe_x = (bounds.x + 2.0) as u32;
        let probe_y = (bounds.y + bounds.height / 2.0) as u32;
        let (r, g, b, _a) = driver.pixel(probe_x, probe_y);

        let theme = crate::render::Theme::vscode_light();
        let expected = (theme.status_bg.r, theme.status_bg.g, theme.status_bg.b);
        assert_eq!(
            (r, g, b),
            expected,
            "sidebar header bg must come from theme.status_bg (vimcode's \
             theme, via `to_quadraui_theme_chrome`'s header_bg mapping), \
             not quadraui's hard-coded #252526 literal; painted text was {:?}",
            driver.painted_texts()
        );
        assert_ne!(
            (r, g, b),
            (37, 37, 38),
            "sidebar header bg must not be quadraui's old #252526 literal \
             under a light theme"
        );
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
    /// *real* CoreText font, not silently fail. Before that issue,
    /// `default_font_family()` (`src/core/settings.rs`) returned
    /// `"Monospace"` on every platform — a fontconfig *generic alias*
    /// `MacBackend::set_editor_font` -> `make_font_exact` couldn't resolve
    /// at the time. `App::render_content` pushes it onto the paint backend
    /// every frame via `backend.set_editor_font(family, size)`
    /// unconditionally (#947, no gate), so `current_font` stayed `None`
    /// forever and `char_width`/`line_height` stuck at quadraui's
    /// placeholder seed values (`MacBackend::new`'s `current_char_width:
    /// 8.0`, `current_line_height: 16.0`) regardless of `font_size` — the
    /// editor was laid out against numbers no installed font actually has.
    ///
    /// #1129: `default_font_family()` went back to the single shared
    /// `"Monospace"` value once quadraui#1023 taught
    /// `MacBackend::set_editor_font` to resolve that Pango alias directly
    /// to `system_monospace_font` (CoreText's
    /// `kCTFontUserFixedPitchFontType`) instead of routing it through
    /// `make_font_exact`'s installed-family lookup — so the positive
    /// control below now compares against `system_monospace_font`, not
    /// `make_font_exact`.
    ///
    /// RED-verified against unfixed `develop`: temporarily reverting
    /// `default_font_family()` to the pre-#1129 `cfg!(target_os =
    /// "macos")` branch (`"Menlo"`) and re-running this test with `cargo
    /// test --no-default-features --features macos` fails — the resolved
    /// metrics no longer match `system_monospace_font`'s (they match
    /// Menlo's instead).
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
             font",
            settings.font_family
        );
        assert_ne!(
            backend.line_height(),
            16.0,
            "current_line_height is still quadraui's placeholder seed value \
             — Settings::default().font_family ({:?}) never resolved a real \
             font",
            settings.font_family
        );

        // Positive control: the resolved metrics must be
        // `system_monospace_font`'s own — the CoreText face
        // `quadraui::GenericFamily::Monospace` (the Pango alias
        // `"Monospace"` parses to) resolves to on this backend — not some
        // other font silently substituted.
        let expected_font = quadraui::macos::text::system_monospace_font(settings.font_size as f64);
        let expected = quadraui::macos::text::font_metrics(&expected_font);
        assert!(
            (backend.char_width() as f64 - expected.char_width).abs() < 0.01,
            "char_width {} does not match system_monospace_font's metrics {}",
            backend.char_width(),
            expected.char_width
        );
        assert!(
            (backend.line_height() as f64 - expected.line_height).abs() < 0.01,
            "line_height {} does not match system_monospace_font's metrics {}",
            backend.line_height(),
            expected.line_height
        );
    }

    // ── #1864: VS Code line-height parity (native macOS GUI only) ───────

    /// VS Code's `editor.lineHeight` resolves to `round(fontSize × 1.5)` on
    /// macOS (its `GOLDEN_LINE_HEIGHT_RATIO`) — 18px for Menlo 12, the
    /// native macOS GUI's own default font (`MacBackend::default_fonts`,
    /// `Settings::effective_editor_font`'s "never customized" resolution —
    /// `plain_engine()` below never touches `font_family`/`font_size`, so
    /// this exercises that exact default path). Before this fix the editor
    /// used Core Text's natural `ascent + descent + leading` line height
    /// instead (~1.17x, ~14px for Menlo 12) — cramped next to VS Code on
    /// the same monitor, issue #1864's own side-by-side screenshot.
    ///
    /// RED-verified against unfixed `develop`: with the
    /// `resolve_editor_line_height_px` call around `App::render_content`'s
    /// `compose_editor_band_rungs` deleted (so nothing ever overrides
    /// `MacBackend`'s natural `current_line_height`), this test's row-pitch
    /// assertions fail — consecutive rows land ~14px apart instead of 18px.
    #[test]
    fn macos_default_font_rows_are_18px_apart_matching_vs_code() {
        // #1745-review's `TestSettingsPathGuard` dance (see
        // `cmd_b_does_not_toggle_sidebar_dead_panel_accelerator`'s doc, same
        // module): `render()` runs `Engine::check_settings_reload`, and a
        // real on-disk `settings.json` (the local developer's own) would
        // silently overwrite `font_family`/`font_size`/`line_height` here.
        // Point it at a path that cannot exist instead.
        use crate::core::settings::TestSettingsPathGuard;
        let tmp = std::env::temp_dir().join(format!(
            "vimcode_test_1864_row_pitch_{:?}.json",
            std::thread::current().id()
        ));
        let _settings_guard = TestSettingsPathGuard::install(tmp);

        let mut engine = plain_engine();
        engine.buffer_mut().insert(0, "alpha\nbravo\ncharlie\n");
        let (_guards, mut driver) = driver(engine);
        driver.render();

        assert!(
            driver.screen_contains("alpha")
                && driver.screen_contains("bravo")
                && driver.screen_contains("charlie"),
            "precondition: all three lines must paint; painted text was {:?}",
            driver.painted_texts()
        );

        let alpha = driver.find_bounds("alpha").expect("'alpha' must paint");
        let bravo = driver.find_bounds("bravo").expect("'bravo' must paint");
        let charlie = driver.find_bounds("charlie").expect("'charlie' must paint");

        let pitch_1 = bravo.y - alpha.y;
        let pitch_2 = charlie.y - bravo.y;

        assert!(
            (pitch_1 - 18.0).abs() < 0.5,
            "row pitch {pitch_1}px between 'alpha'/'bravo', want 18px \
             (VS Code's round(12 * 1.5) for Menlo 12)"
        );
        assert!(
            (pitch_2 - 18.0).abs() < 0.5,
            "row pitch {pitch_2}px between 'bravo'/'charlie', want 18px \
             (VS Code's round(12 * 1.5) for Menlo 12)"
        );
    }

    /// Non-blocking concern from review round 1: the sibling test above
    /// only exercises the *auto* (0.0 → macOS's 1.5x) path through
    /// `effective_line_height_multiplier` — `:set line_height=N`'s explicit
    /// branch had unit coverage (`src/core/settings.rs`) but no driver-tier
    /// assertion that the explicit multiplier actually reaches the painted
    /// row pitch, the same way `zoomin`/`zoomout` below are driver-tested
    /// rather than left to the unit layer alone.
    #[test]
    fn explicit_line_height_setting_reaches_the_painted_row_pitch() {
        use crate::core::settings::TestSettingsPathGuard;
        let tmp = std::env::temp_dir().join(format!(
            "vimcode_test_1864_explicit_row_pitch_{:?}.json",
            std::thread::current().id()
        ));
        let _settings_guard = TestSettingsPathGuard::install(tmp);

        let mut engine = plain_engine();
        engine.buffer_mut().insert(0, "alpha\nbravo\n");
        engine.settings.line_height = 2.0;
        let (_guards, mut driver) = driver(engine);
        driver.render();

        let alpha = driver.find_bounds("alpha").expect("'alpha' must paint");
        let bravo = driver.find_bounds("bravo").expect("'bravo' must paint");
        let pitch = bravo.y - alpha.y;

        assert!(
            (pitch - 24.0).abs() < 0.5,
            "row pitch {pitch}px between 'alpha'/'bravo', want 24px \
             (an explicit `line_height=2.0` on Menlo 12: round(12 * 2.0))"
        );
    }

    /// Acceptance criterion from #1864: `zoomin` grows both the font and
    /// the row pitch; `zoomout` shrinks both — the per-frame override in
    /// `App::sync_per_frame_backend_state` recomputes from the *current*
    /// `settings.font_size` every frame, same as `set_editor_font` beside
    /// it, so a runtime zoom reaches the painted row pitch immediately.
    #[test]
    fn zoomin_and_zoomout_grow_and_shrink_the_row_pitch() {
        // Same `TestSettingsPathGuard` dance as the sibling test above —
        // `execute_command("zoomin"/"zoomout")` below calls
        // `Settings::save()`, so without this guard the writes would land
        // on the real on-disk `settings.json`.
        use crate::core::settings::TestSettingsPathGuard;
        let tmp = std::env::temp_dir().join(format!(
            "vimcode_test_1864_zoom_{:?}.json",
            std::thread::current().id()
        ));
        let _settings_guard = TestSettingsPathGuard::install(tmp);

        let mut engine = plain_engine();
        engine.buffer_mut().insert(0, "alpha\nbravo\n");
        let (_guards, engine, mut driver) = driver_with_engine(engine);

        // #1542: `zoomin`/`zoomout` mutate the *stored* `settings.font_size`
        // literal, not the backend-resolved effective size the baseline
        // frame below is painted with (`Settings::effective_editor_font`'s
        // "never customized" sentinel resolves the stored default, 14, to
        // this backend's own default, 12pt Menlo, until the very first zoom
        // keystroke — see that method's own doc). So the very first
        // `zoomin` jumps the *effective* size from 12 to 15 (14 + 1), not
        // 13 — asserting monotonic growth/shrink against each *previous*
        // measurement (not against the very first baseline) is what
        // actually matches the acceptance criterion ("zoomin grows ...
        // zoomout shrinks") without being coupled to that unrelated #1542
        // jump.
        let mut row_pitch = |when: &str| -> f32 {
            driver.render();
            let alpha = driver
                .find_bounds("alpha")
                .unwrap_or_else(|| panic!("'alpha' must paint {when}"));
            let bravo = driver
                .find_bounds("bravo")
                .unwrap_or_else(|| panic!("'bravo' must paint {when}"));
            bravo.y - alpha.y
        };

        let pitch_0 = row_pitch("at the baseline");

        engine.borrow_mut().execute_command("zoomin");
        let pitch_1 = row_pitch("after the first zoomin");
        assert!(
            pitch_1 > pitch_0,
            "zoomin must grow the row pitch: {pitch_0} -> {pitch_1}"
        );

        engine.borrow_mut().execute_command("zoomin");
        let pitch_2 = row_pitch("after the second zoomin");
        assert!(
            pitch_2 > pitch_1,
            "a second zoomin must grow the row pitch further: {pitch_1} -> {pitch_2}"
        );

        engine.borrow_mut().execute_command("zoomout");
        let pitch_3 = row_pitch("after the first zoomout");
        assert!(
            pitch_3 < pitch_2,
            "zoomout must shrink the row pitch: {pitch_2} -> {pitch_3}"
        );

        engine.borrow_mut().execute_command("zoomout");
        let pitch_4 = row_pitch("after the second zoomout");
        assert!(
            pitch_4 < pitch_3,
            "a second zoomout must shrink the row pitch further: {pitch_3} -> {pitch_4}"
        );
    }

    /// #1864 review round 1: the VS Code editor row-pitch override must
    /// never move the file-explorer tree's own row pitch. An earlier
    /// version of this fix applied the override to `MacBackend`'s single,
    /// shared `current_line_height` field for the *entire* frame — which
    /// `tree_layout` (file-explorer rows) also reads directly, inflating
    /// the sidebar by the same multiplier (plus `tree_layout`'s own `*
    /// 1.4` on top): ~28%, untested and unmeasured.
    ///
    /// Renders the same expanded-explorer scenario at two very different
    /// `settings.line_height` values (the default "auto", VS Code's 1.5x,
    /// and an explicit 3.0x — double that) and asserts the tree's painted
    /// row pitch is identical either way. If the editor override still
    /// leaked into `backend.current_line_height` during this paint, the
    /// second render's rows would land roughly twice as far apart as the
    /// first's.
    ///
    /// RED-verified against the global-override version of this fix (the
    /// one `sync_per_frame_backend_state` applied for the whole frame):
    /// `pitch_default` was ~25px and `pitch_explicit` (`line_height=3.0`,
    /// i.e. 36px editor rows) was ~50px — the assertion below failed.
    #[test]
    fn explorer_row_pitch_is_unaffected_by_the_editor_line_height_setting() {
        use crate::core::settings::TestSettingsPathGuard;
        let tmp = std::env::temp_dir().join(format!(
            "vimcode_test_1864_explorer_row_pitch_{:?}.json",
            std::thread::current().id()
        ));
        let _settings_guard = TestSettingsPathGuard::install(tmp);

        let dir = scratch_explorer_dir("row_pitch_1864", 2);
        let (_guards, engine, mut driver) = driver_with_engine(engine_with_expanded_explorer(&dir));

        let mut row_pitch = |when: &str| -> f32 {
            driver.render();
            let src = driver
                .find_bounds("src")
                .unwrap_or_else(|| panic!("'src' must paint {when}"));
            let core = driver
                .find_bounds("core")
                .unwrap_or_else(|| panic!("'core' must paint {when}"));
            core.y - src.y
        };

        let pitch_default = row_pitch("at the default (auto, 1.5x) line_height");

        engine.borrow_mut().settings.line_height = 3.0;
        let pitch_explicit = row_pitch("with an explicit 3.0x line_height");

        assert!(
            (pitch_default - pitch_explicit).abs() < 0.5,
            "the file-explorer's own row pitch must not move when the \
             *editor's* line_height setting changes: {pitch_default}px \
             (line_height=0, auto) vs {pitch_explicit}px (line_height=3.0) \
             — the VS Code row-pitch override must be scoped to the \
             editor's own rows, never bleeding into `MacBackend::\
             tree_layout`'s unrelated row-height convention"
        );

        let _ = std::fs::remove_dir_all(&dir);
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
        use quadraui::Backend;

        let mut engine = plain_engine();
        // A distinctive, non-default `cwd` so the Command Center's "🔍
        // <project>" search label is unmistakable in `painted_texts()` --
        // mirrors `gtk::testing::command_center`'s
        // `engine_with_tab_history` fixture, which does the same for the
        // identical reason (an empty/default cwd would paint an empty
        // label, per `render::build_command_center_view`).
        engine.cwd = std::path::PathBuf::from("omnibar-fixture-939");

        let (_guards, driver) = driver(engine);

        // #1541 sanity check: the positive assertion below only proves the
        // Command Center paints when `titlebar_control_inset()` is the
        // trait's all-zero default -- the one value `MacDriver` can ever
        // report, per `control_inset_is_default_because_mac_driver_never_
        // sets_a_window`'s doc comment just above this test. It says
        // nothing about a *real* window: quadraui#1154 had
        // `titlebar_control_inset` return the full window width there,
        // which (via `render::measure_title_bar_bands`'s otherwise-correct
        // clamp-to-empty arithmetic, `render.rs`'s
        // `leading_inset_wider_than_the_row_clamps_to_an_empty_band`)
        // collapses the Command Center band to zero width -- the exact
        // "never paints" symptom this test exists to catch, and exactly
        // what this headless harness cannot reproduce. Pin the assumption
        // explicitly so this test cannot quietly start "passing" against a
        // degenerate real-window inset: if this fires, `MacDriver` has
        // started reporting a real inset, and the positive assertion below
        // needs re-verifying against it before it can be trusted (real-
        // window verification is this issue's SMOKE_TESTS item, not
        // something this harness can do).
        assert_eq!(
            driver.backend().titlebar_control_inset(),
            quadraui::Rect::default(),
            "MacDriver's backend reported a non-default titlebar control \
             inset -- the Command Center assertions below no longer prove \
             what this test's doc comment claims; see that comment"
        );

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

    // ── #902/#1580: native macOS context menus ────────────────────────────

    /// #1580: `menu_style` defaults to `Auto`, which resolves native
    /// whenever the backend advertises one (`BackendCaps::native_menu`,
    /// `true` for `MacBackend`) — see `quadraui::MenuStyle::resolve`, via
    /// `Backend::effective_menu_style`. So by default, a right-click on
    /// macOS must **not** paint the in-window `ContextMenuPanel` at all;
    /// `Backend::show_context_menu` takes over instead (from `App::handle`,
    /// see `right_click_opens_then_activated_item_runs_command_without_
    /// any_render_call` below for that half). This test never triggers
    /// that native popup at all (setting `engine.context_menu` directly,
    /// not going through a real right-click), so it only proves the
    /// *other* half of the acceptance bar — the one a headless test *can*
    /// prove: nothing paints in-window when native is resolved.
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
             defaults to `Auto`, so the in-window context menu must not \
             paint; painted text was {:?}",
            driver.painted_texts()
        );
    }

    /// #1580: `menu_style = Custom` opts back into the in-window path even
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

    /// #1580 root-cause fix: `Backend::show_context_menu` must be invoked
    /// from the event handler that opens the menu (`App::handle`'s
    /// `open_context_menu_now_if_native` choke point), never from
    /// `render_content`'s `FrameOp::ContextMenu` paint rung — calling it
    /// from inside a paint closure re-entered painting while the closure
    /// still held the borrows it needed to finish its own frame, which is
    /// exactly why right-click menus were broken in the macOS GUI (see
    /// this issue's root-cause writeup).
    ///
    /// `MacBackend::show_context_menu` degrades to a silent no-op off the
    /// real AppKit main thread (quadraui#930's `MainThreadMarker` guard),
    /// which every `#[test]` thread is — so this test cannot observe the
    /// native popup itself opening or watch it push its own activation
    /// event. What it *can* and does prove, purely through `App`'s own
    /// dispatch, with **no `driver.render()` call anywhere in the test**:
    /// a right-click resolves through `Engine::open_editor_context_menu`
    /// to a populated `engine.context_menu`, and a subsequently-dispatched
    /// `UiEvent::ContextMenuItemActivated` — exactly what
    /// `MacBackend::show_context_menu` pushes once AppKit's real modal
    /// loop picks an item — resolves and runs that item's command within
    /// that same next `dispatch`. The whole round trip never touches
    /// `render_content`, so nothing exercised here can be the render rung;
    /// combined with `native_menu_style_suppresses_the_in_window_context_
    /// menu` above (which proves the *paint* rung never draws the panel
    /// when native is resolved), this closes the loop the render rung
    /// used to own alone.
    ///
    /// RED against the pre-#1580 body in one concrete way: before this
    /// issue, `render.rs::paint_context_menu_rung` took a `native: bool`
    /// and called `Backend::show_context_menu` itself, reachable only from
    /// a `render()` call — a test written the same way but calling
    /// `driver.render()` before the `ContextMenuItemActivated` dispatch
    /// would still have passed then, which is precisely why "no render
    /// call anywhere" is the part of this test that is load-bearing, not
    /// incidental.
    #[test]
    fn right_click_opens_then_activated_item_runs_command_without_any_render_call() {
        let (_guards, engine, mut driver) = driver_with_engine(plain_engine());
        assert!(
            engine.borrow().context_menu.is_none(),
            "sanity: no context menu open before the right-click"
        );

        // Right-click in the editor content area — same coordinates
        // `gtk::testing`'s identical-purpose right-click tests use on the
        // same 1400x900 canvas.
        driver.dispatch(quadraui::UiEvent::MouseDown {
            widget: None,
            button: quadraui::MouseButton::Right,
            position: quadraui::Point::new(700.0, 400.0),
            modifiers: quadraui::Modifiers::default(),
        });

        let item_count = {
            let eng = engine.borrow();
            let cm = eng
                .context_menu
                .as_ref()
                .expect("right-click must open the editor context menu");
            cm.items.len()
        };
        assert!(item_count > 0, "fixture needs a non-empty context menu");

        // "Command Palette" is always enabled and always last in
        // `Engine::open_editor_context_menu`'s item list.
        let palette_idx = item_count - 1;
        assert_eq!(
            engine.borrow().context_menu.as_ref().unwrap().items[palette_idx].action,
            "command_palette",
            "fixture assumption: Command Palette is the last item"
        );

        // Simulate what `MacBackend::show_context_menu` pushes once
        // AppKit's modal loop resolves a pick — the exact `WidgetId`
        // `context_menu_panel_to_quadraui_context_menu` synthesises
        // (`context:N`) and the exact event `App::handle`'s
        // `UiEvent::ContextMenuItemActivated` arm consumes.
        driver.dispatch(quadraui::UiEvent::ContextMenuItemActivated(
            quadraui::WidgetId::new(format!("context:{palette_idx}")),
        ));

        assert!(
            engine.borrow().context_menu.is_none(),
            "activating an item must close the menu"
        );
        assert!(
            engine.borrow().picker_open,
            "activating the Command Palette item must run its command \
             (Engine::open_picker) within this same next dispatch"
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
    /// **RED-verification (#967):** reverting `MacBackend`'s
    /// `Backend::set_current_line_height`/`set_current_char_width` overrides
    /// (quadraui-side since #1497; this file's own wrapper impl before that)
    /// back to no-op bodies takes this test red — the sweep's lower sample
    /// points mis-hit `core` a row down instead of `src`, disagreeing with
    /// the top-of-row baseline, and `sweep_hit_band_integrity`'s
    /// `assert_eq!` fires. Confirmed locally with `cargo test
    /// --no-default-features --features macos
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
    /// sound by round-tripping a value through `quadraui::Backend` directly,
    /// no paint or click involved. See
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

    // #1089's own retirement: `ext_panel_header_click_hit_band_matches_the_
    // painted_row` and its fixture (`engine_with_marketplace_as_ext_panel`)
    // used to live here. That fixture set `ext_panel_active` to a name with
    // **no** `PanelRegistration` — a hack that only made sense while
    // `App::paint_sidebar_panel_rung`'s `ext:` arm unconditionally painted
    // the extension marketplace regardless of which plugin id was active.
    // Now that the arm paints a real `PanelRegistration`'s own sections
    // (#1089), that fixture paints nothing and the test fails on its own
    // precondition. The property it existed to pin — a section-header
    // click, swept across its whole painted band, always toggles *that*
    // header and no other — is now covered against a genuine plugin panel,
    // on this backend too, by `crate::harness::plugin_panel`'s
    // `plugin_panel_section_header_hit_band_on_macos`
    // (`src/harness/plugin_panel/tests.rs`), built on
    // `engine_with_plugin_panel` (a real registration + `ext_panel_items`)
    // per that issue's own "Shape of the work" item 3.

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
                // #1576: the quadraui pin bump brought in macOS
                // double-click folding (quadraui#486 — `MacBackend`'s
                // `DoubleClickDetector`, 400 ms window, 4 pt radius). The
                // sweep's samples are ~3 pt apart and back-to-back, so
                // sample 1 arrived as a `UiEvent::DoubleClick` — which
                // confirms the still-selected file `a` instead of
                // row-clicking `b`, a false "#971 hit-band drift". Unlike
                // `TuiDriver`/`GtkDriver`, `MacDriver` has no
                // `set_double_click_folding(false)` (see
                // docs/PENDING_QUADRAUI_ISSUES.md), so let the fold window
                // lapse instead: every sample is meant to be an
                // independent single click on a freshly re-opened picker.
                std::thread::sleep(std::time::Duration::from_millis(450));
                engine.borrow_mut().close_picker();
                engine.borrow_mut().open_picker(PickerSource::Files);
                d.render();
            },
            |d| ConformanceDriver::inventory(d).screen_has("ZQXW971PICKBBODY"),
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── #1745: VS Code mode's Cmd-key defaults on the macOS GUI ─────────
    //
    // #1730's own parity table (`tests/vscode_keybinding_parity.rs`) wrongly
    // assumed there was no macOS GUI backend in this repo to regress
    // against, and left every `MacDiverges` row ungated. There is one
    // (this file), and `normalize_mac_cmd_as_ctrl` (`src/app.rs`) is
    // the fix: before it existed, `quadraui::Modifiers::cmd` — the bit a
    // real Cmd keypress sets (confirmed directly from quadraui's own
    // `macos/events.rs`: `NS_FLAG_COMMAND` -> `cmd`, distinct from
    // `NS_FLAG_CONTROL` -> `ctrl`) — was read nowhere in
    // `App::handle_dispatch`, so Cmd+C/V/X/Z/S/P/F/B/J/, didn't merely
    // *diverge* from VS Code's Mac defaults, they did nothing at all.
    //
    // These tests drive the same `MacDriver` every other test in this file
    // does, through the real `App::handle_dispatch` both `super::run` and
    // the live AppKit event loop call — not a direct `Engine::handle_key`
    // call — because the bug this fixes is specifically in the
    // event-to-`ctrl`-bit translation, not in any engine-level binding
    // (`tests/vscode_keybinding_parity.rs`'s own 62+ engine-level tests
    // already cover that half).
    //
    // **RED-verification note** (all five tests below): confirmed red by
    // temporarily short-circuiting `normalize_mac_cmd_as_ctrl` to
    // `if true { return event; }` (disabling every arm, not just one at a
    // time) and re-running `cargo test --no-default-features --features
    // macos --lib vscode_mode_mac_cmd_1745` — all five failed with the
    // exact "fell back to the unmodified/un-translated key" symptom each
    // test's own doc below describes; reverted after confirming.
    mod vscode_mode_mac_cmd_1745 {
        use quadraui::{Key, Modifiers, NamedKey};

        use crate::core::{Cursor, Mode};

        /// A VSCode-mode, Insert-mode engine seeded with `buffer` — the
        /// nerd-fonts-off rationale is the same as [`super::plain_engine`]'s
        /// (the unrelated #620 tab-icon `debug_assert!`).
        fn vscode_engine(buffer: &str) -> crate::core::Engine {
            let mut engine = crate::core::Engine::new_for_test();
            engine.settings.use_nerd_fonts = Some(false);
            engine.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
            engine.mode = Mode::Insert;
            engine.buffer_mut().insert(0, buffer);
            engine.view_mut().cursor = Cursor { line: 0, col: 0 };
            engine
        }

        /// Dispatch exactly one `KeyPressed { key, modifiers, repeat: false }`
        /// — mirrors `src/gtk/testing.rs`'s `mod alt_rung_1744::press`.
        fn press<A: quadraui::AppLogic>(
            driver: &mut quadraui::macos::testing::MacDriver<A>,
            key: Key,
            modifiers: Modifiers,
        ) {
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key,
                modifiers,
                repeat: false,
            });
        }

        /// #1745: Cmd+/ (VS Code's Mac default for `editor.action.
        /// commentLine`) must toggle the line comment, exactly like Ctrl+/
        /// on Linux/Windows (`tests/vscode_keybinding_parity.rs`'s
        /// `test_vscode_ctrl_slash_toggles_line_comment`). Paint-based
        /// assertion per CLAUDE.md's rendered-output rule: the comment
        /// marker has to actually reach the screen, not just a buffer
        /// mutation.
        ///
        /// RED against the pre-#1745 tree (see the submodule-level
        /// RED-verification note above): `ctrl` stays `false`, the
        /// Insert-mode engine treats the keystroke as a plain character
        /// insertion, and the screen shows the literal `"print(1)/"`
        /// instead of a commented line.
        #[test]
        fn cmd_slash_toggles_line_comment() {
            let engine = vscode_engine("print(1)\n");
            let (_guards, mut driver) = super::driver(engine);

            press(
                &mut driver,
                Key::Char('/'),
                Modifiers {
                    cmd: true,
                    ..Default::default()
                },
            );
            driver.render();

            assert!(
                driver.screen_contains("# print(1)"),
                "Cmd+/ must toggle the line comment on the macOS GUI, the \
                 same as Ctrl+/ on Linux/Windows; painted text was {:?}",
                driver.painted_texts()
            );
        }

        /// #1745: Cmd+F (VS Code's Mac default for `actions.find`) must
        /// open find, exactly like Ctrl+F on Linux/Windows
        /// (`tests/vscode_keybinding_parity.rs`'s
        /// `test_vscode_ctrl_f_opens_find`). Paint-based per CLAUDE.md's
        /// rendered-output rule: the find/replace panel's own case-
        /// sensitivity toggle (`render.rs`'s `FindReplacePanel`, label
        /// `"Aa"`) is painted once the panel opens and nowhere else on this
        /// fixture — the only other `"Aa"` literal in `render.rs` belongs to
        /// the separate sidebar *Search* panel's own toggle, which this
        /// fixture never opens.
        ///
        /// RED against the pre-#1745 tree (see the submodule-level
        /// RED-verification note above): `ctrl` stays `false`, so the
        /// Insert-mode engine treats the keystroke as a plain character
        /// insertion instead — the find/replace panel never opens and
        /// `"Aa"` never paints.
        #[test]
        fn cmd_f_opens_find() {
            let engine = vscode_engine("hello\n");
            let (_guards, mut driver) = super::driver(engine);
            driver.render();
            assert!(
                !driver.screen_contains("Aa"),
                "precondition: find/replace must start closed, so its \"Aa\" \
                 toggle must not be painted yet; painted text was {:?}",
                driver.painted_texts()
            );

            press(
                &mut driver,
                Key::Char('f'),
                Modifiers {
                    cmd: true,
                    ..Default::default()
                },
            );
            driver.render();

            assert!(
                driver.screen_contains("Aa"),
                "Cmd+F must open find on the macOS GUI, the same as Ctrl+F \
                 on Linux/Windows — the find/replace panel's \"Aa\" toggle \
                 must now be painted; painted text was {:?}",
                driver.painted_texts()
            );
        }

        /// #1745: VS Code's real Mac default for word-wise navigation is
        /// **Option**+Left/Right (`cursorWordLeft`/`cursorWordEndRight`),
        /// not a plain Alt-to-something substitution of vimcode's existing
        /// Ctrl+Left/Right word-move — and critically, plain Alt+Right on
        /// this backend would otherwise be claimed by `route_alt_key`'s
        /// own VS-Code-mode tier as `workbench.action.navigateForward`
        /// (correct for Win/Linux, wrong for Mac — Mac's real default for
        /// that command is Ctrl+-/Ctrl+Shift+-, a still-open gap; see
        /// `tests/vscode_keybinding_parity.rs`'s `KNOWN_GAPS`).
        ///
        /// Paint-based per CLAUDE.md's rendered-output rule: presses the
        /// chord, then types an unmodified marker character so where it
        /// lands reveals where the cursor actually ended up — the same
        /// idiom `cmd_slash_toggles_line_comment` (above) uses, applied to
        /// a cursor-motion chord instead of an edit.
        /// `Engine::move_word_forward` on `"hello world\n"` from column 0
        /// lands on column 6 (the start of "world"), confirmed directly by
        /// driving `Engine::handle_key` the same way
        /// `test_vscode_ctrl_f_opens_find`'s sibling tests in
        /// `tests/vscode_keybinding_parity.rs` do.
        ///
        /// RED against the pre-#1745 tree (see the submodule-level
        /// RED-verification note above): with no translation, `alt` stays
        /// set and `ctrl` stays unset, so `route_alt_key` claims the chord
        /// as `navigateForward` instead — a no-op here (no jump-list entry
        /// was ever recorded), leaving the cursor at column 0 and the
        /// marker landing as "Xhello world" instead.
        #[test]
        fn option_right_moves_word_forward_not_navigate_forward() {
            let engine = vscode_engine("hello world\n");
            let (_guards, mut driver) = super::driver(engine);

            press(
                &mut driver,
                Key::Named(NamedKey::Right),
                Modifiers {
                    alt: true,
                    ..Default::default()
                },
            );
            press(&mut driver, Key::Char('X'), Modifiers::default());
            driver.render();

            assert!(
                driver.screen_contains("hello Xworld"),
                "Option+Right must move the cursor forward by a word \
                 (cursorWordEndRight) before the marker keystroke lands, \
                 not fall through to navigateForward (a no-op here, which \
                 would leave the marker at column 0 instead); painted text \
                 was {:?}",
                driver.painted_texts()
            );
        }

        /// #1745: VS Code's real Mac default for `cursorEnd` is **Cmd**+
        /// Right, not word-move — the two modifiers swap roles relative to
        /// Linux/Windows' Ctrl=word, Home/End=line split (see
        /// `tests/vscode_keybinding_parity.rs`'s `cursorWordEndRight /
        /// cursorWordLeft` row). Same marker-keystroke, paint-based idiom as
        /// `option_right_moves_word_forward_not_navigate_forward` above.
        ///
        /// RED against the pre-#1745 tree (see the submodule-level
        /// RED-verification note above): with no translation, the engine
        /// sees a plain, unmodified `Right` and moves the cursor by exactly
        /// one column instead of to the end of the line, so the marker
        /// lands as "hXello world" instead.
        #[test]
        fn cmd_right_moves_to_line_end_not_one_column() {
            let engine = vscode_engine("hello world\n");
            let (_guards, mut driver) = super::driver(engine);

            press(
                &mut driver,
                Key::Named(NamedKey::Right),
                Modifiers {
                    cmd: true,
                    ..Default::default()
                },
            );
            press(&mut driver, Key::Char('X'), Modifiers::default());
            driver.render();

            assert!(
                driver.screen_contains("hello worldX"),
                "Cmd+Right must move the cursor to the end of the line \
                 (\"hello world\" is 11 columns wide) before the marker \
                 keystroke lands, not by one column; painted text was {:?}",
                driver.painted_texts()
            );
        }

        /// #1745: VS Code's real Mac default for `cursorBottom` is
        /// **Cmd**+Down (document end), distinct from `cursorEnd`'s Cmd+
        /// Right (line end) tested just above — see
        /// `tests/vscode_keybinding_parity.rs`'s `cursorTop / cursorBottom`
        /// row. Same marker-keystroke, paint-based idiom as
        /// `option_right_moves_word_forward_not_navigate_forward` above.
        /// `Engine::handle_key("End", ..., true)` (Ctrl+End, the binding
        /// this translation reuses) from line 0 of `"aaa\nbbb\nccc\n"` lands
        /// on line 2, column 3 (the end of "ccc") — confirmed the same way
        /// as `option_right_moves_word_forward_not_navigate_forward`'s own
        /// doc describes.
        ///
        /// RED against the pre-#1745 tree (see the submodule-level
        /// RED-verification note above): with no translation, the engine
        /// sees a plain, unmodified `Down` and moves the cursor down by
        /// exactly one line instead of to the last line of the buffer, so
        /// the marker lands as "Xbbb" instead.
        #[test]
        fn cmd_down_moves_to_document_end_not_one_line() {
            let engine = vscode_engine("aaa\nbbb\nccc\n");
            let (_guards, mut driver) = super::driver(engine);

            press(
                &mut driver,
                Key::Named(NamedKey::Down),
                Modifiers {
                    cmd: true,
                    ..Default::default()
                },
            );
            press(&mut driver, Key::Char('X'), Modifiers::default());
            driver.render();

            assert!(
                driver.screen_contains("cccX"),
                "Cmd+Down must move the cursor to the last line of the \
                 buffer before the marker keystroke lands, not down by one \
                 line; painted text was {:?}",
                driver.painted_texts()
            );
        }

        /// #1745 review: the `toggleSidebarVisibility` row
        /// (`tests/vscode_keybinding_parity.rs`) originally claimed Cmd+B
        /// `Matches` on the macOS GUI, on the theory that
        /// `normalize_mac_cmd_as_ctrl`'s fold plus `Engine::handle_vscode_key`'s
        /// "b" arm is enough. It isn't: that arm returns
        /// `EngineAction::ToggleSidebar`, but `render::apply_engine_action`'s
        /// own arm for it is a bare `app.draw_needed.set(true)` — the real
        /// toggle (`Engine::toggle_sidebar`) only ever runs via the *separate*
        /// `panel_keys` accelerator path (`DeferredAction::ToggleSidebar`,
        /// drained by `tick`). That accelerator is registered as
        /// `quadraui::KeyBinding::Literal(pk.toggle_sidebar)` (default
        /// `"<C-b>"`), and quadraui's own `macos_universal_binding_modifiers`
        /// deliberately leaves `Literal` bindings untouched — so it matches a
        /// physical Ctrl+B and never a Cmd+B. quadraui's shared
        /// `runtime::preprocess_event` runs that accelerator match *before*
        /// an unmatched keypress ever reaches `App::handle_dispatch` (where
        /// this issue's Cmd-fold lives), so by the time the fold sees a real
        /// Cmd+B it is already too late — the fold can produce a `ctrl`-true
        /// event for `handle_vscode_key`, but that path's `ToggleSidebar`
        /// action is the dead one. `KNOWN_GAPS::
        /// PANEL_ACCELERATOR_CMD_CHORDS_DEAD_ON_MACOS_GUI` in
        /// `tests/vscode_keybinding_parity.rs` names this; this test is that
        /// entry's RED proof (that file's own `gap_gate` can't reach this —
        /// the defect is in `render::apply_engine_action`, `pub(crate)` and
        /// only reachable from inside this crate).
        ///
        /// Oracle: `engine.app_shell.sidebar_visible()`, not painted text.
        /// This is the one place in this submodule that reads state instead
        /// of paint, and it is deliberate, not a shortcut: this driver's
        /// `painted_texts()` recorder accumulates across frames rather than
        /// clearing per frame, so it cannot prove a widget *disappeared* —
        /// see [`ctrl_b_toggle_sidebar_proves_sidebar_visible_is_a_real_
        /// paint_oracle`] below, which presses the *working* accelerator
        /// chord and shows exactly that limitation directly. `sidebar_
        /// visible()` is not an unused parallel field like the #587
        /// `ScreenLayout.picker` incident the repo's testing rules warn
        /// about — it is the exact flag `render.rs`'s own sidebar-paint gate
        /// reads, and that sibling test proves reading it here is a real,
        /// production-faithful oracle, not a state-populated-but-never-
        /// painted field.
        ///
        /// RED against the pre-fix tree (today, unfixed): confirmed by the
        /// sibling test just below — Ctrl+B (the working accelerator path)
        /// flips `sidebar_visible()` from `true` to `false` on the exact
        /// same fixture; Cmd+B does not move it at all.
        #[test]
        fn cmd_b_does_not_toggle_sidebar_dead_panel_accelerator() {
            // #1745 review: avoid polluting this run with the real on-disk
            // `settings.json` — `handle_poll_tick` (run by `driver.tick()`
            // below) unconditionally calls `Engine::check_settings_reload`,
            // and a fresh `Engine::new_for_test()` leaves `settings_mtime`
            // at `None` specifically so ambient disk state is never loaded
            // implicitly (see that constructor's own doc) — which means the
            // *first* poll against a real settings file always looks like
            // an external edit and reloads it, overwriting `panel_keys` with
            // whatever the local developer's own settings happen to be.
            // Point it at a path that cannot exist instead.
            use crate::core::settings::TestSettingsPathGuard;
            let tmp = std::env::temp_dir().join(format!(
                "vimcode_test_1745_cmd_b_{:?}.json",
                std::thread::current().id()
            ));
            let _settings_guard = TestSettingsPathGuard::install(tmp);

            let mut engine = vscode_engine("hello\n");
            // Start from a known, deliberately-visible state via the real
            // `Engine::toggle_sidebar` — not the buggy dispatch path under
            // test — so a no-op bug can't hide behind "it was already
            // closed".
            if !engine.app_shell.sidebar_visible() {
                engine.toggle_sidebar();
            }
            let (_guards, engine, mut driver) = super::driver_with_engine(engine);
            assert!(
                engine.borrow().app_shell.sidebar_visible(),
                "precondition: sidebar must start visible"
            );

            press(
                &mut driver,
                Key::Char('b'),
                Modifiers {
                    cmd: true,
                    ..Default::default()
                },
            );
            driver.tick();

            assert!(
                engine.borrow().app_shell.sidebar_visible(),
                "KNOWN_GAPS::PANEL_ACCELERATOR_CMD_CHORDS_DEAD_ON_MACOS_GUI: \
                 today, Cmd+B does NOT toggle the sidebar on the macOS GUI \
                 (see this test's own doc for the full dispatch-ordering \
                 reason) — this assertion pins that wrong behaviour. If it \
                 ever fails, the gap closed: flip this assertion to \
                 `!engine.borrow().app_shell.sidebar_visible()`, delete the \
                 `KNOWN_GAPS` entry, and flip the matching \
                 `VSCODE_BINDINGS` row back to `Matches`."
            );
        }

        /// Companion to [`cmd_b_does_not_toggle_sidebar_dead_panel_
        /// accelerator`] above — proves two things that test's doc relies
        /// on, on the exact same fixture shape: (1) the `panel_keys`
        /// accelerator path genuinely works end to end (so the sibling
        /// test's Cmd+B failure is a real Cmd-vs-Ctrl gap, not a broken
        /// fixture), and (2) this driver's `painted_texts()` recorder
        /// cannot prove a widget's *disappearance* — it still reports
        /// `screen_contains("EXPLORER")` as `true` a full frame after the
        /// sidebar genuinely closed (`sidebar_visible()` is `false`),
        /// which is why the sibling test reads that state flag instead of
        /// painted text for its own assertion.
        #[test]
        fn ctrl_b_toggle_sidebar_proves_sidebar_visible_is_a_real_paint_oracle() {
            use crate::core::settings::TestSettingsPathGuard;
            let tmp = std::env::temp_dir().join(format!(
                "vimcode_test_1745_ctrl_b_{:?}.json",
                std::thread::current().id()
            ));
            let _settings_guard = TestSettingsPathGuard::install(tmp);

            let mut engine = vscode_engine("hello\n");
            if !engine.app_shell.sidebar_visible() {
                engine.toggle_sidebar();
            }
            let (_guards, engine, mut driver) = super::driver_with_engine(engine);
            driver.render();
            assert!(
                engine.borrow().app_shell.sidebar_visible(),
                "precondition: sidebar must start visible"
            );
            assert!(
                driver.screen_contains("EXPLORER"),
                "precondition: the visible sidebar's header must have \
                 painted"
            );

            press(
                &mut driver,
                Key::Char('b'),
                Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
            );
            driver.tick();
            driver.render();

            assert!(
                !engine.borrow().app_shell.sidebar_visible(),
                "Ctrl+B must close the sidebar via the real `panel_keys` \
                 accelerator — if this fails, the fixture itself is broken \
                 and the sibling Cmd+B test's failure would not mean what \
                 its doc says it means"
            );
            assert!(
                driver.screen_contains("EXPLORER"),
                "this assertion is expected to hold even though the \
                 sidebar just closed — it exists to document (not to \
                 regression-test) that this driver's `painted_texts()` \
                 recorder accumulates across frames and so cannot prove a \
                 widget disappeared; if this ever starts failing it means \
                 the recorder started clearing per frame, and the sibling \
                 Cmd+B test above should be revisited to use \
                 `screen_contains` instead of `sidebar_visible()`"
            );
        }
    }

    // ── #1877: activity bar must end above the status/command-line chrome ──

    /// Look up a `quadraui::testing::ZoneRec`'s bounds by its registered
    /// `WidgetId` string, for the assertions below. `AppShell::render`
    /// registers one zone per chrome region it lays out (`register_chrome_
    /// zones`) plus one per activity-bar item (keyed by the item's own
    /// panel id) — see `quadraui::testing::FrameInventory`'s own doc for
    /// the full catalogue. Panics with the full zone list on a miss so a
    /// failure names exactly what *was* registered instead of just "not
    /// found".
    fn zone_bounds(inv: &quadraui::testing::FrameInventory, id: &str) -> quadraui::Rect {
        inv.zones()
            .iter()
            .find(|z| z.id.as_str() == id)
            .unwrap_or_else(|| {
                panic!(
                    "no zone registered for {id:?} -- registered zones: {:?}",
                    inv.zones()
                )
            })
            .bounds
    }

    /// #1877: the activity bar's painted/hit-test rect must end above the
    /// shell's reserved bottom chrome, not run all the way to the window's
    /// bottom edge — the bug report's "status bar covers activity bar,
    /// Settings button half hidden". `plain_engine()`'s default settings
    /// (`laststatus=2`, `window_status_line=true`, single window) are
    /// exactly the configuration the report describes: a per-window status
    /// row plus the always-present command line at the bottom of the one
    /// open window, confined to `main_content_bounds` by `render_content`'s
    /// own `compute_editor_layout` call — `App::shell_config`'s
    /// `with_command_line()`/`with_status_bar()` reservation (the fix) is
    /// what makes `AppShell::compute_layout` carve the *same* two rows out
    /// of `activity_bar_bounds`'s height too.
    ///
    /// Asserts on registered **zones**, not a derived/recomputed rect —
    /// `activity-bar`/`command-line`/`status-bar`/`bottom:settings` are all
    /// registered from the exact `AppShellLayout` `AppShell::render` used
    /// to paint this frame (`register_chrome_zones` plus the per-item loop
    /// right after `draw_activity_bar` returns its hits), so this reads
    /// what was actually painted and hit-tested, not a parallel guess
    /// (CLAUDE.md's "rendered output, not state" rule).
    ///
    /// RED-verified against the pre-fix tree (temporarily dropping
    /// `shell_config`'s `.with_command_line().with_status_bar()` call):
    /// `zone_bounds` panics outright on `"app-shell:command-line"` and
    /// `"app-shell:status-bar"` — those two zones don't exist at all
    /// without the reservation, because nothing ever carved them out of
    /// the shell's layout — and `"bottom:settings"`'s own zone sits at
    /// `y: [852.0, 900.0]`, flush against the window's `900.0`-tall bottom
    /// edge, with no chrome zone above it to be "above" in the first
    /// place.
    #[test]
    fn settings_activity_bar_zone_ends_above_the_status_bar_via_mac_driver() {
        let (_guards, mut driver) = driver(plain_engine());
        driver.render();

        use quadraui::testing::ConformanceDriver;
        let inv = driver.inventory();

        let activity_bar = zone_bounds(&inv, "app-shell:activity-bar");
        let settings = zone_bounds(&inv, "bottom:settings");
        let command_line = zone_bounds(&inv, "app-shell:command-line");
        let status_bar = zone_bounds(&inv, "app-shell:status-bar");
        let window = zone_bounds(&inv, "app-shell:window");

        let activity_bar_bottom = activity_bar.y + activity_bar.height;
        let settings_bottom = settings.y + settings.height;

        assert!(
            activity_bar_bottom <= command_line.y + 0.5,
            "activity bar must end at or above the command line's top: \
             activity-bar bottom {activity_bar_bottom}, command-line top {}",
            command_line.y
        );
        assert!(
            activity_bar_bottom <= status_bar.y + 0.5,
            "activity bar must end at or above the status bar's top: \
             activity-bar bottom {activity_bar_bottom}, status-bar top {}",
            status_bar.y
        );
        assert!(
            settings_bottom <= command_line.y + 0.5,
            "the Settings button's own hit rect must lie entirely above \
             the command line: settings bottom {settings_bottom}, \
             command-line top {}",
            command_line.y
        );
        // The chrome must actually have shrunk, not merely "happen" to sit
        // above a chrome band that itself reaches the window edge — pins
        // the fix is doing real work, not a vacuously-true comparison.
        assert!(
            activity_bar_bottom + 1.0 < window.y + window.height,
            "activity bar bottom ({activity_bar_bottom}) is suspiciously \
             close to the window's bottom edge ({}) -- the bottom-chrome \
             reservation may not be taking effect",
            window.y + window.height
        );
    }

    /// #1877 companion: a click at the Settings button's own registered
    /// zone center must actually open the Settings panel — the other half
    /// of the issue's "...and a click on it opens settings". Resolves the
    /// click position from the registered zone (ground truth for both
    /// paint and hit-test) rather than `find("*")`: that helper reports
    /// the glyph's *bar-relative* paint position for anything painted
    /// inside `MacBackend::draw_activity_bar`'s own `CGContextTranslateCTM`
    /// save/restore block, not its absolute window position -- a
    /// `MacDriver` text-run-recording quirk unrelated to this issue (the
    /// real pixels paint at the right place; only the recorded text-run
    /// coordinates used by `find`/`find_bounds` are pre-translate).
    ///
    /// Asserts on *painted* Settings-panel content (`"Color Scheme"`, a
    /// settings-form field label — CLAUDE.md's "rendered output, not
    /// state" rule), not on `engine.app_shell.active_panel_id()`.
    #[test]
    fn settings_button_click_opens_settings_panel_via_mac_driver() {
        let (_guards, mut driver) = driver(plain_engine());
        driver.render();
        assert!(
            !driver.screen_contains("Color Scheme"),
            "bad fixture: the Settings panel should start closed"
        );

        let (cx, cy) = {
            use quadraui::testing::ConformanceDriver;
            let inv = driver.inventory();
            let z = zone_bounds(&inv, "bottom:settings");
            (z.x + z.width / 2.0, z.y + z.height / 2.0)
        };
        driver.click(cx, cy);
        driver.render();

        assert!(
            driver.screen_contains("Color Scheme"),
            "clicking the Settings activity-bar button's own registered \
             zone must open the Settings panel; painted text was {:?}",
            driver.painted_texts()
        );
    }

    // ── #1877: traffic-light inset themed background ───────────────────

    /// #1877's second report: the strip behind the macOS traffic lights
    /// (`render::inset_titlebar_row_leading_edge`'s `leading_inset`) must
    /// paint the theme's title-bar colour instead of showing a bare,
    /// unthemed surface.
    ///
    /// The fix (`App::render_content`'s `FrameOp::CommandCenter` arm) is
    /// unconditional on `!presence.menu_row` — true for every `MacDriver`
    /// fixture, real window or not, since `MacBackend::backend_caps()`
    /// declares `native_menu: true` unconditionally (`App::setup` reads
    /// that to suppress the drawn menu row, independent of any window) —
    /// so it reaches `backend.draw_menu_bar(menu_row_rect, &filler)` on
    /// every `MacDriver` run. What this test **cannot** independently
    /// prove is that this fill is *visible* — `Backend::titlebar_control_
    /// inset()` is provably always `Rect::default()` here (see
    /// `control_inset_is_default_because_mac_driver_never_sets_a_window`'s
    /// doc a few hundred lines up, and `#940`'s identical gap for the
    /// Command Center rect itself): with a zero inset, `measure_title_bar_
    /// bands`'s `menu_end` already sits at the row's leading edge, so the
    /// Command Center's own background fill (`quadraui::macos::
    /// command_center`'s `fill_rect(.., theme.tab_bar_bg)`) already paints
    /// this pixel the *same* colour this fix's fill paints underneath it
    /// -- the two are visually indistinguishable in this harness, pre-fix
    /// or post. A live NSWindow is the only thing that can put a non-zero
    /// inset there (this issue's SMOKE_TESTS item), the same gap #940
    /// already hit and documented as a quadraui-side limitation, not a
    /// vimcode workaround (CLAUDE.md's Platform-Neutrality Rule: file
    /// upstream, wait, then implement).
    ///
    /// So this test pins the one thing it *can* prove headlessly: the
    /// fill paints the theme's own `tab_bar_bg` colour (matching what
    /// `quadraui::primitives::menu_bar`'s `paint` and `quadraui::macos::
    /// command_center`'s `fill_rect` both use) at the row's leading edge,
    /// for two different colorschemes -- so a colorscheme switch really
    /// does reach this pixel, not a hardcoded literal.
    ///
    /// RED-verification note, stated plainly rather than silently omitted:
    /// this test stays GREEN even with the `FrameOp::CommandCenter` arm's
    /// fix commented out, because of the zero-inset reasoning right above
    /// -- it is not RED-verified against the real bug, and cannot be with
    /// `quadraui` 0.1.2's `MacDriver`. It is real coverage of the fill
    /// call's own correctness (colour, reach, no panic), just not of the
    /// visible symptom. Real verification of the symptom is this issue's
    /// real-Mac `SMOKE_TESTS` item.
    #[test]
    fn title_row_leading_edge_matches_theme_tab_bar_bg_for_two_colorschemes_via_mac_driver() {
        for colorscheme in ["onedark", "gruvbox"] {
            let mut engine = plain_engine();
            engine.settings.colorscheme = colorscheme.to_string();
            let (_guards, mut driver) = driver(engine);
            driver.render();

            let theme = crate::render::Theme::from_name(colorscheme);
            let expected = (theme.tab_bar_bg.r, theme.tab_bar_bg.g, theme.tab_bar_bg.b);

            // x=2 (just inside the left window edge, where the traffic-
            // light inset would sit on a real window), y=4 (a few px
            // down from the top, inside the title-bar band at any
            // reasonable `title_bar_height_lh`).
            let (r, g, b, _a) = driver.pixel(2, 4);
            assert_eq!(
                (r, g, b),
                expected,
                "{colorscheme}: title row leading edge (2, 4) = \
                 ({r}, {g}, {b}), expected theme.tab_bar_bg {expected:?}"
            );
        }
    }
}
