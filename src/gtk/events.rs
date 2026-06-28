//! Re-export of the GDK ↔ `UiEvent` translators that vimcode
//! references externally. The canonical implementations live in
//! `quadraui::gtk::events` after the lift (#270).
//!
//! These were called from draw-area event callbacks wired in the
//! Relm4 `fn init` block (#448-A/B). After #448-C flips the main loop
//! to the quadraui ShellApp runner the runner owns all input
//! translators; these re-exports are preserved for sidebar-panel DAs
//! that will be re-wired in a follow-on task.

#[allow(unused_imports)]
pub use quadraui::gtk::events::{
    gdk_button_to_mouse_down, gdk_button_to_mouse_up, gdk_key_to_quadraui_key, gdk_key_to_uievent,
    gdk_modifiers_to_quadraui, gdk_motion_to_uievent, gdk_scroll_to_uievent,
};
