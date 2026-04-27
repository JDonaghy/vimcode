//! Crossterm → `quadraui::UiEvent` translation.
//!
//! Phase B.4 Stage 4. This module is the boundary between crossterm's
//! native event types (`KeyEvent`, `MouseEvent`, `Event::Resize`,
//! `Event::Paste`, `Event::FocusGained`/`FocusLost`) and quadraui's
//! backend-agnostic [`UiEvent`] enum. Once the event loop migrates
//! (Stage 5), every native event reaches the engine via this layer
//! instead of being decoded inline.
//!
//! # What this does
//!
//! - **Keys** ([`crossterm_key_to_uievent`]): map `KeyCode` → `Key`,
//!   `KeyModifiers` → `Modifiers`. Press events emit
//!   `UiEvent::KeyPressed`; releases are dropped (the engine doesn't
//!   distinguish them today).
//! - **Mouse** ([`crossterm_mouse_to_uievent`]): map `MouseEventKind`
//!   to `MouseDown` / `MouseUp` / `MouseMoved` / `Scroll`. The
//!   `widget` field stays `None` — actual hit-testing happens in
//!   [`crate::tui_main::mouse`] / [`quadraui::dispatch::dispatch_mouse_down`].
//! - **Window resize** → `UiEvent::WindowResized { viewport }`.
//! - **Bracketed paste** → `UiEvent::ClipboardPaste(text)`.
//! - **Focus gained / lost** (kitty protocol) → `UiEvent::WindowFocused(true/false)`.
//!
//! # What this does NOT do
//!
//! - Vim-style key-name strings (`"Escape"`, `"Shift_Up"`, `"Return"`)
//!   that the engine's `handle_key` expects — those still come from
//!   the existing [`crate::tui_main::translate_key`] function.
//!   Stage 5 either keeps that as a separate adapter or refactors it
//!   to consume `UiEvent::KeyPressed` directly.
//! - Accelerator matching. Stage 6's [`Backend::register_accelerator`]
//!   path will compare the registered list against the translated
//!   `Key + Modifiers` and emit `UiEvent::Accelerator(id, mods)`.
//! - Hit-testing. Mouse events come out with `widget: None`; the
//!   trait-side dispatch fills that in via the modal stack and per-
//!   primitive layout hit_test.

use quadraui::{
    ButtonMask, Key, Modifiers, MouseButton, NamedKey, Point, ScrollDelta, UiEvent, Viewport,
};
use ratatui::crossterm::event::{
    Event as CtEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton as CtMouseButton,
    MouseEvent, MouseEventKind,
};

/// Translate one crossterm event to zero or more quadraui events.
///
/// Single events translate one-to-one (Vec of length 1); some
/// crossterm events have no quadraui equivalent and translate to
/// an empty Vec (e.g. `KeyEventKind::Release`). Returning `Vec`
/// instead of `Option` keeps the door open for future
/// composite events without a breaking change.
pub fn crossterm_to_uievents(event: CtEvent) -> Vec<UiEvent> {
    match event {
        CtEvent::Key(k) => crossterm_key_to_uievent(k).into_iter().collect(),
        CtEvent::Mouse(m) => crossterm_mouse_to_uievent(m).into_iter().collect(),
        CtEvent::Resize(w, h) => vec![UiEvent::WindowResized {
            viewport: Viewport::new(w as f32, h as f32, 1.0),
        }],
        CtEvent::Paste(text) => vec![UiEvent::ClipboardPaste(text)],
        CtEvent::FocusGained => vec![UiEvent::WindowFocused(true)],
        CtEvent::FocusLost => vec![UiEvent::WindowFocused(false)],
    }
}

/// Translate one crossterm key event. `None` means the event has no
/// quadraui counterpart (releases, repeats not surfaced as
/// `KeyPressed`, etc.).
pub fn crossterm_key_to_uievent(event: KeyEvent) -> Option<UiEvent> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let key = crossterm_keycode_to_key(event.code)?;
    let modifiers = crossterm_modifiers_to_quadraui(event.modifiers);
    let repeat = event.kind == KeyEventKind::Repeat;
    Some(UiEvent::KeyPressed {
        key,
        modifiers,
        repeat,
    })
}

/// Translate one crossterm mouse event.
pub fn crossterm_mouse_to_uievent(event: MouseEvent) -> Option<UiEvent> {
    let position = Point::new(event.column as f32, event.row as f32);
    let modifiers = crossterm_modifiers_to_quadraui(event.modifiers);
    match event.kind {
        MouseEventKind::Down(b) => Some(UiEvent::MouseDown {
            widget: None,
            button: crossterm_mouse_button_to_quadraui(b),
            position,
            modifiers,
        }),
        MouseEventKind::Up(b) => Some(UiEvent::MouseUp {
            widget: None,
            button: crossterm_mouse_button_to_quadraui(b),
            position,
        }),
        MouseEventKind::Drag(b) => Some(UiEvent::MouseMoved {
            position,
            buttons: button_mask_with_held(b),
        }),
        MouseEventKind::Moved => Some(UiEvent::MouseMoved {
            position,
            buttons: ButtonMask::default(),
        }),
        MouseEventKind::ScrollUp => Some(UiEvent::Scroll {
            widget: None,
            delta: ScrollDelta { x: 0.0, y: -1.0 },
            position,
        }),
        MouseEventKind::ScrollDown => Some(UiEvent::Scroll {
            widget: None,
            delta: ScrollDelta { x: 0.0, y: 1.0 },
            position,
        }),
        MouseEventKind::ScrollLeft => Some(UiEvent::Scroll {
            widget: None,
            delta: ScrollDelta { x: -1.0, y: 0.0 },
            position,
        }),
        MouseEventKind::ScrollRight => Some(UiEvent::Scroll {
            widget: None,
            delta: ScrollDelta { x: 1.0, y: 0.0 },
            position,
        }),
    }
}

fn crossterm_keycode_to_key(code: KeyCode) -> Option<Key> {
    let named = match code {
        KeyCode::Char(c) => return Some(Key::Char(c)),
        KeyCode::Esc => NamedKey::Escape,
        KeyCode::Tab => NamedKey::Tab,
        KeyCode::BackTab => NamedKey::BackTab,
        KeyCode::Enter => NamedKey::Enter,
        KeyCode::Backspace => NamedKey::Backspace,
        KeyCode::Delete => NamedKey::Delete,
        KeyCode::Insert => NamedKey::Insert,
        KeyCode::Home => NamedKey::Home,
        KeyCode::End => NamedKey::End,
        KeyCode::PageUp => NamedKey::PageUp,
        KeyCode::PageDown => NamedKey::PageDown,
        KeyCode::Up => NamedKey::Up,
        KeyCode::Down => NamedKey::Down,
        KeyCode::Left => NamedKey::Left,
        KeyCode::Right => NamedKey::Right,
        KeyCode::F(n) => NamedKey::F(n),
        KeyCode::CapsLock => NamedKey::CapsLock,
        KeyCode::NumLock => NamedKey::NumLock,
        KeyCode::ScrollLock => NamedKey::ScrollLock,
        KeyCode::Menu => NamedKey::Menu,
        // Crossterm has Null, Modifier, Pause, PrintScreen, Media,
        // KeypadBegin etc. that quadraui's NamedKey doesn't yet model.
        // Drop them — the existing engine doesn't act on these either.
        _ => return None,
    };
    Some(Key::Named(named))
}

fn crossterm_modifiers_to_quadraui(m: KeyModifiers) -> Modifiers {
    Modifiers {
        shift: m.contains(KeyModifiers::SHIFT),
        ctrl: m.contains(KeyModifiers::CONTROL),
        alt: m.contains(KeyModifiers::ALT),
        cmd: m.contains(KeyModifiers::SUPER) | m.contains(KeyModifiers::META),
    }
}

fn crossterm_mouse_button_to_quadraui(b: CtMouseButton) -> MouseButton {
    match b {
        CtMouseButton::Left => MouseButton::Left,
        CtMouseButton::Right => MouseButton::Right,
        CtMouseButton::Middle => MouseButton::Middle,
    }
}

/// Crossterm's `MouseEventKind::Drag(b)` carries the held button as
/// the variant payload; the quadraui `ButtonMask` reflects which
/// buttons are down at that moment. We only know the *one* button
/// crossterm reports, so the mask has just that bit set.
fn button_mask_with_held(b: CtMouseButton) -> ButtonMask {
    match b {
        CtMouseButton::Left => ButtonMask {
            left: true,
            right: false,
            middle: false,
        },
        CtMouseButton::Right => ButtonMask {
            left: false,
            right: true,
            middle: false,
        },
        CtMouseButton::Middle => ButtonMask {
            left: false,
            right: false,
            middle: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{
        KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton as CtMouseButton,
        MouseEvent, MouseEventKind,
    };

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: mods,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: col,
            row,
            modifiers: KeyModifiers::empty(),
        }
    }

    #[test]
    fn char_keypress_translates_to_keypressed_char() {
        let ev = crossterm_key_to_uievent(key(KeyCode::Char('a'), KeyModifiers::empty()));
        assert_eq!(
            ev,
            Some(UiEvent::KeyPressed {
                key: Key::Char('a'),
                modifiers: Modifiers::default(),
                repeat: false,
            })
        );
    }

    #[test]
    fn ctrl_p_translates_with_ctrl_modifier() {
        let ev = crossterm_key_to_uievent(key(KeyCode::Char('p'), KeyModifiers::CONTROL)).unwrap();
        match ev {
            UiEvent::KeyPressed { key, modifiers, .. } => {
                assert_eq!(key, Key::Char('p'));
                assert!(modifiers.ctrl);
                assert!(!modifiers.shift);
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn named_keys_translate() {
        for (code, named) in &[
            (KeyCode::Esc, NamedKey::Escape),
            (KeyCode::Enter, NamedKey::Enter),
            (KeyCode::Backspace, NamedKey::Backspace),
            (KeyCode::Tab, NamedKey::Tab),
            (KeyCode::BackTab, NamedKey::BackTab),
            (KeyCode::Up, NamedKey::Up),
            (KeyCode::Down, NamedKey::Down),
            (KeyCode::Left, NamedKey::Left),
            (KeyCode::Right, NamedKey::Right),
            (KeyCode::PageUp, NamedKey::PageUp),
            (KeyCode::PageDown, NamedKey::PageDown),
            (KeyCode::Home, NamedKey::Home),
            (KeyCode::End, NamedKey::End),
            (KeyCode::Delete, NamedKey::Delete),
            (KeyCode::Insert, NamedKey::Insert),
            (KeyCode::F(1), NamedKey::F(1)),
            (KeyCode::F(12), NamedKey::F(12)),
        ] {
            let ev = crossterm_key_to_uievent(key(*code, KeyModifiers::empty())).unwrap();
            match ev {
                UiEvent::KeyPressed { key, .. } => {
                    assert_eq!(key, Key::Named(*named), "for code {:?}", code);
                }
                other => panic!("unexpected variant: {:?}", other),
            }
        }
    }

    #[test]
    fn key_release_drops() {
        let mut ev = key(KeyCode::Char('a'), KeyModifiers::empty());
        ev.kind = KeyEventKind::Release;
        assert!(crossterm_key_to_uievent(ev).is_none());
    }

    #[test]
    fn key_repeat_marks_repeat_true() {
        let mut ev = key(KeyCode::Char('a'), KeyModifiers::empty());
        ev.kind = KeyEventKind::Repeat;
        let translated = crossterm_key_to_uievent(ev).unwrap();
        match translated {
            UiEvent::KeyPressed { repeat, .. } => assert!(repeat),
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn mouse_down_left_translates() {
        let ev =
            crossterm_mouse_to_uievent(mouse(MouseEventKind::Down(CtMouseButton::Left), 10, 5))
                .unwrap();
        assert!(matches!(
            ev,
            UiEvent::MouseDown {
                widget: None,
                button: MouseButton::Left,
                ..
            }
        ));
        if let UiEvent::MouseDown { position, .. } = ev {
            assert_eq!(position.x, 10.0);
            assert_eq!(position.y, 5.0);
        }
    }

    #[test]
    fn mouse_drag_carries_held_button() {
        let ev =
            crossterm_mouse_to_uievent(mouse(MouseEventKind::Drag(CtMouseButton::Right), 12, 8))
                .unwrap();
        match ev {
            UiEvent::MouseMoved { buttons, .. } => {
                assert!(!buttons.left);
                assert!(buttons.right);
                assert!(!buttons.middle);
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn mouse_scroll_up_translates_to_negative_y() {
        let ev = crossterm_mouse_to_uievent(mouse(MouseEventKind::ScrollUp, 0, 0)).unwrap();
        match ev {
            UiEvent::Scroll { delta, .. } => {
                assert_eq!(delta.y, -1.0);
                assert_eq!(delta.x, 0.0);
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn mouse_scroll_down_translates_to_positive_y() {
        let ev = crossterm_mouse_to_uievent(mouse(MouseEventKind::ScrollDown, 0, 0)).unwrap();
        match ev {
            UiEvent::Scroll { delta, .. } => assert_eq!(delta.y, 1.0),
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn resize_translates_to_window_resized() {
        let evs = crossterm_to_uievents(CtEvent::Resize(120, 40));
        assert_eq!(evs.len(), 1);
        match &evs[0] {
            UiEvent::WindowResized { viewport } => {
                assert_eq!(viewport.width, 120.0);
                assert_eq!(viewport.height, 40.0);
                assert_eq!(viewport.scale, 1.0);
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn paste_translates_to_clipboard_paste() {
        let evs = crossterm_to_uievents(CtEvent::Paste("hello".into()));
        assert_eq!(evs, vec![UiEvent::ClipboardPaste("hello".into())]);
    }

    #[test]
    fn focus_gained_translates() {
        let evs = crossterm_to_uievents(CtEvent::FocusGained);
        assert_eq!(evs, vec![UiEvent::WindowFocused(true)]);
    }

    #[test]
    fn focus_lost_translates() {
        let evs = crossterm_to_uievents(CtEvent::FocusLost);
        assert_eq!(evs, vec![UiEvent::WindowFocused(false)]);
    }

    #[test]
    fn shift_ctrl_modifiers_combine() {
        let mods = crossterm_modifiers_to_quadraui(KeyModifiers::SHIFT | KeyModifiers::CONTROL);
        assert!(mods.shift);
        assert!(mods.ctrl);
        assert!(!mods.alt);
        assert!(!mods.cmd);
    }

    #[test]
    fn super_modifier_maps_to_cmd() {
        let mods = crossterm_modifiers_to_quadraui(KeyModifiers::SUPER);
        assert!(mods.cmd);
    }
}
