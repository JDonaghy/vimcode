//! Re-export of the TUI backend, lifted into `quadraui::tui::backend`
//! for cross-app reuse (#268).
//!
//! Kept as a module so existing `use super::backend::TuiBackend`
//! references at vimcode call sites keep working unchanged.

pub use quadraui::tui::TuiBackend;
