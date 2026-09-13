//! `vimcode_core` — the whole editor as a library.
//!
//! #657 promoted `render`, `tui_main` and `gtk` out of the `vimcode` / `vcd`
//! binaries and into this crate, so that an **integration test** (a separate
//! crate that links only against `[lib] vimcode_core`) can reach the UI
//! backends and their black-box harnesses. Before the promotion, every
//! black-box test in this repo *had* to be in-crate, because `tests/*.rs`
//! could see nothing but `core` + `icons`. That made the coordinator's
//! sealed-`tests/acceptance/` oracle loop impossible here — see the
//! `tests/acceptance.rs` crate root, and `docs/ARCHITECTURE.md`.
//!
//! `src/main.rs` (GTK) and `src/tui_bin.rs` (TUI) are now thin shims over
//! this crate: argument parsing plus a call into `gtk::run` / `tui_main::run`.
//!
//! GTK lives behind the `gui` feature, exactly as it did in the bin, so
//! `--no-default-features` still builds on a machine with no GTK4 dev libs.
#![allow(clippy::collapsible_match)]
pub mod core;
pub mod icons;

// #657: promoted out of `src/main.rs` / `src/tui_bin.rs`, which used to
// declare these as private `mod`s. No lint allows are re-stated here: each of
// the three already carries the inner attributes it needs at the top of its
// own file (`render.rs`'s `#![allow(dead_code)]`, `tui_main/mod.rs`'s
// `#![allow(unused_assignments, ...)]`, `gtk/mod.rs`'s
// `#![allow(deprecated)]`), and repeating them here trips clippy's
// `duplicated_attributes`.
pub mod render;
pub mod tui_main;

/// Backend-neutral pixel→click-target resolution shared by `crate::app`
/// (#862). Split out of the formerly `gui`-gated `src/gtk/click.rs`, which
/// re-exports these names so its own tests and the rest of `crate::gtk` keep
/// resolving them unchanged.
pub(crate) mod click;

/// Backend-neutral shell support functions (UI-font helpers, tab-bar pixel
/// geometry, h-scrollbar geometry, ...) shared by `crate::app` (#862). Split
/// out of the formerly `gui`-gated `src/gtk/mod.rs`, which re-exports these
/// names so the rest of `crate::gtk` keeps resolving them unchanged.
pub(crate) mod app_support;

/// Backend-neutral theme CSS text generation shared by `crate::app` (#862).
/// Split out of the formerly `gui`-gated `src/gtk/css.rs`, which re-exports
/// `make_theme_css`/`STATIC_CSS` and keeps the GTK-only `load_css`.
pub(crate) mod css;

/// The GTK backend, behind the `gui` feature exactly as it was in the
/// `vimcode` bin — so `--no-default-features` still builds on a machine with
/// no GTK4 dev libs.
#[cfg(feature = "gui")]
pub mod gtk;

/// `struct App` — the backend-neutral editor shell application, hoisted out
/// of `src/gtk/mod.rs` by #785 (stage 1 of #47) so a second native backend
/// can reuse it instead of re-implementing ~6,900 lines of portable shell
/// logic. #862 dropped the `gui` gate itself: the three remaining
/// platform-typed fields (`window`, `css_provider`, `settings_monitor`) are
/// now type-erased (a small local trait for `window`/`css_provider`, an
/// opaque `Box<dyn Any>` drop-guard for `settings_monitor`), and the
/// `crate::gtk::{click, css, util}` reliance moved to the neutral
/// `crate::click`/`crate::app_support`/`crate::css` above. What's left
/// behind `#[cfg(feature = "gui")]` *inside* `src/app.rs` is the handful of
/// items that are genuinely platform-bound: `App::new`/`App::assemble`'s
/// display-dependent prologue, the `TextMetricsBackend`/window-handle/
/// css-provider trait impls for the concrete GTK types, and a few inline
/// `gtk4::Settings`/window-discovery call sites. See `src/app.rs`'s module
/// doc for the full inventory.
pub mod app;

/// The native macOS (AppKit) backend — a thin wrapper over
/// `quadraui::macos::shell_runner::run_with_shell` (#859, stage 2 of #47).
///
/// Double-gated, on the `macos` feature **and** `target_os = "macos"`, for
/// the same reason quadraui gates its own `macos` module that way
/// (`quadraui/src/lib.rs:128`): the module's whole dependency surface —
/// `MacBackend`, AppKit, Core Graphics, Core Text — does not exist off a
/// Mach-O host, so `--features macos` on Linux resolves cleanly and compiles
/// none of it. That makes the feature safe to enable anywhere (it is what
/// the `vimcode` bin's GUI-less build test in `src/main.rs` leans on) at the
/// price that a Linux build proves nothing about this code. See
/// `src/macos/mod.rs`'s "Verifying this file without a Mac" section for what
/// was done about that and why the obvious `--target aarch64-apple-darwin`
/// cross-check does not work here (`tree-sitter`'s C build script).
#[cfg(all(feature = "macos", target_os = "macos"))]
pub mod macos;

/// The native Windows GUI (Direct2D/DirectWrite) backend — a thin wrapper
/// over `quadraui::win::shell_runner::run_with_shell` (#866, stage 3 of
/// #47, the Win-GUI twin of `macos` above).
///
/// Gated on the `win` feature **alone** — unlike `macos`, which is also
/// gated on `target_os = "macos"` because quadraui gates its own `macos`
/// module that way. quadraui's `win` module (`quadraui/src/lib.rs`, next to
/// the `macos` arm) deliberately does **not** target-gate itself: every real
/// WinAPI call inside it is individually `cfg(target_os = "windows")`-gated
/// with a `todo!()` fallback everywhere else, specifically so `cargo check
/// --features win` type-checks `WinBackend` on an ordinary Linux CI runner
/// (see that module's own doc comment and `Cargo.toml`'s `win` feature
/// comment here). `src/win/mod.rs` inherits the same posture, so this line
/// mirrors it rather than diverging — target-gating this module too would
/// make `--features win` on Linux compile nothing, silently reintroducing
/// the #645 trap `macos`'s own `target_os` gate deliberately avoids getting
/// near for the "does the bin still build without `gui`?" probe. What *is*
/// `target_os`-gated is where `src/main.rs::launch_gui` actually calls
/// `vimcode_core::win::run` — see that file's doc comment.
#[cfg(feature = "win")]
pub mod win;

/// Backend-neutral conformance-test harness (#928) — adopts quadraui's
/// `ConformanceDriver`/`PixelClickConformance` so a black-box scenario is
/// written once and run against every GUI backend's own driver
/// (`GtkDriver`/`MacDriver`/`WinDriver`), instead of a GTK-only suite of
/// 134 tests and a 4-test macOS one. See that module's doc for the full
/// design and the proof slice it ships. Test-only; never compiled into a
/// release binary — same gate as `test_cwd`/`test_paint` below, which it
/// depends on.
#[cfg(any(test, feature = "test-support"))]
pub mod harness;

/// Process-wide working-directory arbitration for the test run (#785) — the
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
pub use core::engine::{Engine, EngineAction, RegType};
pub use core::mode::Mode;
pub use core::settings::Settings;
pub use core::view::View;
