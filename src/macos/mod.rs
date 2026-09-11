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
}
