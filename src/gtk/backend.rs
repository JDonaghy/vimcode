//! Re-export of the GTK backend, lifted into `quadraui::gtk::backend`
//! for cross-app reuse (#270).
//!
//! Kept as a module so existing `use super::backend::GtkBackend`
//! references at vimcode call sites keep working unchanged.

pub use quadraui::gtk::GtkBackend;
