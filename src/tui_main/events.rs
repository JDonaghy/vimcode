//! Re-export of the crossterm ↔ `UiEvent` translators that vimcode
//! references externally. The canonical implementations live in
//! `quadraui::tui::events` after the lift (#268).

pub use quadraui::tui::events::uievent_to_crossterm;
