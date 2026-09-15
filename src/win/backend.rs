//! Re-export of the Win-GUI backend, lifted into `quadraui::win::backend`
//! for cross-app reuse (#866, the Win-GUI twin of #270's GTK re-export).
//!
//! Kept as a module (rather than a bare `pub use` at `src/win/mod.rs`'s top
//! level) so a future `use super::backend::WinBackend` reads exactly like
//! `src/gtk/backend.rs`'s `use super::backend::GtkBackend`. vimcode does not
//! own a backend here either — see that file's doc comment.

pub use quadraui::win::WinBackend;
