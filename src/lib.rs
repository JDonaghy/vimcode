// Library shim for integration tests. No UI deps (GTK/Relm4/Cairo) allowed here.
#![allow(clippy::collapsible_match)]
pub mod core;
pub mod icons;

// Re-export quadraui so integration tests + downstream consumers pin to the
// same version vimcode is built against.
pub use quadraui;

// Convenience re-exports so integration tests can write `use vimcode_core::Engine` etc.
pub use core::buffer::Buffer;
pub use core::cursor::Cursor;
pub use core::engine::{Engine, EngineAction};
pub use core::mode::Mode;
pub use core::settings::Settings;
pub use core::view::View;
