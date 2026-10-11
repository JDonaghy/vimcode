//! `vimcode_core` — the whole editor as a library.
//!
//! This crate exposes `render`, `tui_main`, and `gtk` so that integration
//! tests can reach the UI backends and their black-box harnesses.
//!
//! `src/main.rs` (GTK) and `src/tui_bin.rs` (TUI) are thin shims over
//! this crate: argument parsing plus a call into `gtk::run` / `tui_main::run`.
//!
//! GTK lives behind the `gui` feature, exactly as it did in the bin, so
//! `--no-default-features` still builds on a machine with no GTK4 dev libs.
#![allow(clippy::collapsible_match)]
pub mod core;
pub mod icons;

// These modules are public because they are used by external tests.
// Lint allows are stated in each module's own file where dead code exists.
pub mod render;
pub mod tui_main;

/// Backend-neutral pixel→click-target resolution shared by `crate::app`.
/// Split out of the formerly `gui`-gated `src/gtk/click.rs`, which
/// re-exports these names so its own tests and the rest of `crate::gtk` keep
/// resolving them unchanged.
pub(crate) mod click;

/// Backend-neutral shell support functions (UI-font helpers, tab-bar pixel
/// geometry, h-scrollbar geometry, ...) shared by `crate::app`. Split
/// out of the formerly `gui`-gated `src/gtk/mod.rs`, which re-exports these
/// names so the rest of `crate::gtk` keeps resolving them unchanged.
pub(crate) mod app_support;

/// The GTK backend, behind the `gui` feature exactly as it was in the
/// `vimcode` bin — so `--no-default-features` still builds on a machine with
/// no GTK4 dev libs.
#[cfg(feature = "gui")]
pub mod gtk;

/// `struct App` — the backend-neutral editor shell application.
/// Reusable by multiple native backends instead of re-implementing portable
/// shell logic. Platform-specific fields have been removed or type-erased,
/// and display-dependent initialization steps are now called directly by
/// each backend's `run` function. See `src/app.rs`'s module doc for details.
pub mod app;

/// The native macOS (AppKit) backend — a thin wrapper over
/// `quadraui::macos::shell_runner::run_with_shell`.
///
/// Double-gated, on the `macos` feature **and** `target_os = "macos"`,
/// because the module's dependencies (AppKit, Core Graphics, Core Text)
/// do not exist off a Mach-O host. This allows `--features macos` on Linux
/// to resolve cleanly without compiling this module.
#[cfg(all(feature = "macos", target_os = "macos"))]
pub mod macos;

/// The native Windows GUI (Direct2D/DirectWrite) backend — a thin wrapper
/// over `quadraui::win::shell_runner::run_with_shell`.
///
/// Gated on the `win` feature **alone** — unlike `macos`, which is also
/// gated on `target_os = "macos"`. This allows `cargo check --features win`
/// to type-check `WinBackend` on non-Windows platforms: individual WinAPI
/// calls inside are `cfg(target_os = "windows")`-gated with `todo!()` fallbacks.
#[cfg(feature = "win")]
pub mod win;

/// Backend-neutral conformance-test harness — adopts quadraui's
/// `ConformanceDriver`/`PixelClickConformance` so a black-box scenario is
/// written once and run against every GUI backend's own driver
/// (`GtkDriver`/`MacDriver`/`WinDriver`), instead of a GTK-only suite of
/// 134 tests and a 4-test macOS one. See that module's doc for the full
/// design and the proof slice it ships. Test-only; never compiled into a
/// release binary — same gate as `test_cwd`/`test_paint` below, which it
/// depends on.
#[cfg(any(test, feature = "test-support"))]
pub mod harness;

/// Process-wide working-directory arbitration for the test run — the
/// lock that keeps a `chdir`-ing test from moving the ground under a
/// concurrently painting harness. Test-only; never compiled into a release
/// binary.
#[cfg(any(test, feature = "test-support"))]
pub mod test_cwd;

/// Process-wide arbitration of Pango/Cairo text work in the test run — the
/// lock that keeps two threads from being inside libpango/libfreetype at
/// once, which segfaults. Test-only; never compiled into a release binary.
#[cfg(any(test, feature = "test-support"))]
pub mod test_paint;

// Re-export quadraui so integration tests + downstream consumers pin to the
// same version vimcode is built against.
pub use quadraui;

pub mod quadraui_pin;

// Convenience re-exports so integration tests can write `use vimcode_core::Engine` etc.
pub use core::buffer::Buffer;
pub use core::cursor::Cursor;
pub use core::engine::{Engine, EngineAction, RegType, UserKeymap, UserKeymapAction};
pub use core::mode::Mode;
pub use core::settings::Settings;
pub use core::view::View;
