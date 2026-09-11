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
//! `shell_config()` produces. The residue this
//! file adds on top is ~20 lines with no branches. See #859 for stage 3
//! (the `MacDriver` black-box test), which needs a Mach-O host and is gated
//! on vimcode's `test_command` becoming macOS-satisfiable.

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
