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
/// All three methods are no-ops, and that is what the trait's own doc
/// comment predicts for this backend rather than an omission:
///
/// - `set_text_measurement_context` — "a backend with no persistent-context
///   concept … can implement this as a no-op"; macOS text measurement
///   (`quadraui::macos::text::measure_text(&CTFont, &str)`) takes the font
///   per call instead of storing a context. The only producer of a context
///   (`click::build_editor_click_context`) is GTK-only and its call site in
///   `render_content` is `#[cfg(feature = "gui")]`, so nothing ever calls
///   this here anyway.
/// - the two metric setters — `MacBackend` has no public counterparts
///   (`GtkBackend::set_current_line_height` / `set_current_char_width` have
///   no `MacBackend` twin at the pinned rev `9eede7fd`). It keeps the same
///   two fields but derives them from its own font inside `set_font`
///   (`quadraui/src/macos/backend.rs:414`), i.e. the macOS backend owns its
///   metrics where the GTK backend is told them.
///
/// **If that turns out to be wrong on a real Mac** — glyph-grid drift
/// between what `App` thinks a line is and what `MacBackend` paints — the
/// fix is a quadraui issue asking for public metric setters on
/// `MacBackend`, **not** arithmetic in this file. Recorded here so the next
/// person does not have to re-derive which side of the boundary it is on.
impl TextMetricsBackend for quadraui::macos::MacBackend {
    fn set_text_measurement_context(&mut self, _ctx: Box<dyn std::any::Any>) {}
    fn set_current_line_height(&mut self, _line_height: f64) {}
    fn set_current_char_width(&mut self, _char_width: f64) {}
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
        engine.settings.use_nerd_fonts = false;
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

    // ── #901: native menu bar adoption ──────────────────────────────────

    /// A plain engine, safe to drive through `MacBackend` — same
    /// nerd-fonts-off rationale as [`engine_with_minimap`] (the tab-icon
    /// `debug_assert!`, unrelated to menus), without the minimap/500-line
    /// buffer that test doesn't need here.
    fn plain_engine() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = false;
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
        engine.settings.use_nerd_fonts = true;
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
    /// registered Nerd Font rather than painting as a tofu box.
    /// quadraui's own conformance suite documents that no black-box
    /// vocabulary it exposes can observe that: `FrameInventory`'s painted
    /// *text runs* record the same requested string regardless of which
    /// font family Core Text resolved it against
    /// (`tests/conformance.rs`'s `UNGATED_CAPS` entry for
    /// `app_font_registration`), `Backend` is `sealed` so vimcode cannot
    /// substitute a spying implementation, and `MacBackend`'s
    /// `nerd_font_fallback_family`/`current_font` fields are private with no
    /// public getter — nothing outside quadraui's own crate can read them.
    /// Confirming the glyph itself paints correctly needs a human looking
    /// at a real Mac's screen, or a future quadraui-side introspection API
    /// added for exactly this; see this PR's notes.
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
}
