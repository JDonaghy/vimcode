//! Re-export of the GDK ↔ `UiEvent` translators that vimcode
//! references externally. The canonical implementations live in
//! `quadraui::gtk::events` after the lift (#270).

pub use quadraui::gtk::events::{
    gdk_button_to_mouse_down, gdk_button_to_mouse_up, gdk_key_to_quadraui_key, gdk_key_to_uievent,
    gdk_modifiers_to_quadraui, gdk_motion_to_uievent, gdk_scroll_to_uievent,
};
