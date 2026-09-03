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

/// The GTK backend, behind the `gui` feature exactly as it was in the
/// `vimcode` bin — so `--no-default-features` still builds on a machine with
/// no GTK4 dev libs.
#[cfg(feature = "gui")]
pub mod gtk;

/// `struct App` — the editor shell application, hoisted out of
/// `src/gtk/mod.rs` by #785 (stage 1 of #47) so a second native backend can
/// reuse it instead of re-implementing ~6,900 lines of portable shell logic.
///
/// Still `gui`-gated: `App` retains four platform-typed fields, ~11 platform
/// hook call sites and a dependency on `crate::gtk::{click, css, util}`. The
/// module doc in `src/app.rs` enumerates all three so the next stage does not
/// have to re-derive them.
#[cfg(feature = "gui")]
pub mod app;

// Re-export quadraui so integration tests + downstream consumers pin to the
// same version vimcode is built against.
pub use quadraui;

pub mod quadraui_pin;

// Convenience re-exports so integration tests can write `use vimcode_core::Engine` etc.
pub use core::buffer::Buffer;
pub use core::cursor::Cursor;
pub use core::engine::{Engine, EngineAction};
pub use core::mode::Mode;
pub use core::settings::Settings;
pub use core::view::View;
