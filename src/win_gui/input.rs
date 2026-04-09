//! Translate Win32 keyboard messages into engine key names.
//!
//! The engine expects key names matching the GTK/TUI convention:
//! - Named keys: "Escape", "Return", "BackSpace", "Tab", "Delete", "Up", etc.
//! - Printable chars: unicode passed via `Option<char>`
//! - Ctrl combos: key_name = lowercase letter, ctrl = true
//!
//! Win32 sends two message types:
//! - WM_KEYDOWN/WM_SYSKEYDOWN with a virtual-key code (VK_*)
//! - WM_CHAR with the translated Unicode character

use windows::Win32::UI::Input::KeyboardAndMouse::*;

/// Result of translating a Win32 key event.
pub struct KeyInput {
    pub key_name: String,
    pub unicode: Option<char>,
    pub ctrl: bool,
}

/// Translate a WM_KEYDOWN / WM_SYSKEYDOWN virtual-key code.
/// Returns `None` for keys we don't handle (e.g. lone Shift/Ctrl/Alt).
pub fn translate_vk(vk: u16, ctrl: bool, shift: bool, alt: bool) -> Option<KeyInput> {
    let vk = VIRTUAL_KEY(vk);
    match vk {
        VK_ESCAPE => Some(named("Escape")),
        VK_RETURN if shift && ctrl => Some(named_ctrl("Shift_Return")),
        VK_RETURN if ctrl => Some(named_ctrl("Return")),
        VK_RETURN => Some(named("Return")),
        VK_BACK => Some(named("BackSpace")),
        VK_DELETE => Some(named("Delete")),
        VK_TAB if shift => Some(named_with_ctrl("ISO_Left_Tab", ctrl)),
        VK_TAB => Some(named_with_ctrl("Tab", ctrl)),

        // Arrow keys
        VK_UP if shift && !ctrl => Some(named("Shift_Up")),
        VK_DOWN if shift && !ctrl => Some(named("Shift_Down")),
        VK_LEFT if shift && !ctrl => Some(named("Shift_Left")),
        VK_RIGHT if shift && !ctrl => Some(named("Shift_Right")),
        VK_LEFT if shift && ctrl => Some(named_ctrl("Shift_Left")),
        VK_RIGHT if shift && ctrl => Some(named_ctrl("Shift_Right")),
        VK_UP => Some(named("Up")),
        VK_DOWN => Some(named("Down")),
        VK_LEFT => Some(named_with_ctrl("Left", ctrl)),
        VK_RIGHT => Some(named_with_ctrl("Right", ctrl)),

        VK_HOME if shift => Some(named("Shift_Home")),
        VK_END if shift => Some(named("Shift_End")),
        VK_HOME => Some(named_with_ctrl("Home", ctrl)),
        VK_END => Some(named_with_ctrl("End", ctrl)),
        VK_PRIOR => Some(named("Page_Up")),
        VK_NEXT => Some(named("Page_Down")),

        // Function keys
        VK_F1 => Some(named("F1")),
        VK_F2 => Some(named("F2")),
        VK_F3 => Some(named("F3")),
        VK_F4 => Some(named("F4")),
        VK_F5 => Some(named("F5")),
        VK_F6 => Some(named("F6")),
        VK_F7 => Some(named("F7")),
        VK_F8 => Some(named("F8")),
        VK_F9 => Some(named("F9")),
        VK_F10 => Some(named("F10")),
        VK_F11 => Some(named("F11")),
        VK_F12 => Some(named("F12")),

        // Alt+letter combos — handled here because WM_SYSKEYDOWN doesn't
        // generate WM_CHAR for Alt combos.
        _ if alt && !ctrl => translate_alt_key(vk, shift),

        // Ctrl+letter combos — handled here so they don't go through WM_CHAR
        // (WM_CHAR turns Ctrl+A into char 0x01, etc.)
        _ if ctrl && !alt => translate_ctrl_key(vk, shift),

        // All other VK codes: let WM_CHAR handle them
        _ => None,
    }
}

/// Translate a WM_CHAR Unicode character (non-ctrl context).
pub fn translate_char(ch: char) -> Option<KeyInput> {
    if ch.is_control() {
        return None; // Ctrl+letter generates 0x01..0x1A — handled by translate_vk
    }
    Some(KeyInput {
        key_name: String::new(),
        unicode: Some(ch),
        ctrl: false,
    })
}

fn translate_alt_key(vk: VIRTUAL_KEY, _shift: bool) -> Option<KeyInput> {
    let code = vk.0;
    if (0x41..=0x5A).contains(&code) {
        let lower = (code as u8 + 32) as char;
        return Some(KeyInput {
            key_name: format!("Alt-{}", lower),
            unicode: Some(lower),
            ctrl: false,
        });
    }
    match vk {
        VK_OEM_COMMA => Some(KeyInput {
            key_name: "Alt-,".to_string(),
            unicode: Some(','),
            ctrl: false,
        }),
        VK_OEM_PERIOD => Some(KeyInput {
            key_name: "Alt-.".to_string(),
            unicode: Some('.'),
            ctrl: false,
        }),
        _ => None,
    }
}

fn translate_ctrl_key(vk: VIRTUAL_KEY, shift: bool) -> Option<KeyInput> {
    // VK_A (0x41) through VK_Z (0x5A) map to 'a'..'z'
    let code = vk.0;
    if (0x41..=0x5A).contains(&code) {
        let lower = (code as u8 + 32) as char; // 'A'(0x41) -> 'a'(0x61)
        let name = if lower == ' ' {
            "space".to_string()
        } else if shift {
            lower.to_ascii_uppercase().to_string()
        } else {
            lower.to_string()
        };
        return Some(KeyInput {
            key_name: name,
            unicode: Some(lower),
            ctrl: true,
        });
    }

    // Ctrl+special keys matching TUI conventions
    match vk {
        VK_SPACE => Some(KeyInput {
            key_name: "space".to_string(),
            unicode: Some(' '),
            ctrl: true,
        }),
        VK_OEM_5 => Some(KeyInput {
            // backslash key
            key_name: "backslash".to_string(),
            unicode: Some('\\'),
            ctrl: true,
        }),
        VK_OEM_2 => Some(KeyInput {
            // forward slash key
            key_name: "slash".to_string(),
            unicode: Some('/'),
            ctrl: true,
        }),
        VK_OEM_3 => Some(KeyInput {
            // backtick/grave key
            key_name: "grave".to_string(),
            unicode: Some('`'),
            ctrl: true,
        }),
        VK_OEM_COMMA => Some(KeyInput {
            key_name: "comma".to_string(),
            unicode: Some(','),
            ctrl: true,
        }),
        VK_OEM_4 if shift => Some(KeyInput {
            // [ key, shifted = {
            key_name: "Shift_bracketleft".to_string(),
            unicode: Some('['),
            ctrl: true,
        }),
        VK_OEM_6 if shift => Some(KeyInput {
            // ] key, shifted = }
            key_name: "Shift_bracketright".to_string(),
            unicode: Some(']'),
            ctrl: true,
        }),
        VK_OEM_4 => Some(KeyInput {
            // [
            key_name: "bracketleft".to_string(),
            unicode: Some('['),
            ctrl: true,
        }),
        VK_OEM_6 => Some(KeyInput {
            // ]
            key_name: "bracketright".to_string(),
            unicode: Some(']'),
            ctrl: true,
        }),
        _ => None,
    }
}

fn named(name: &str) -> KeyInput {
    KeyInput {
        key_name: name.to_string(),
        unicode: None,
        ctrl: false,
    }
}

fn named_ctrl(name: &str) -> KeyInput {
    KeyInput {
        key_name: name.to_string(),
        unicode: None,
        ctrl: true,
    }
}

fn named_with_ctrl(name: &str, ctrl: bool) -> KeyInput {
    KeyInput {
        key_name: name.to_string(),
        unicode: None,
        ctrl,
    }
}
