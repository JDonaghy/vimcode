// ─── Shared key decode ────────────────────────────────────────────────────────
//
// The single implementation of "raw key → engine-facing `(key_name, unicode,
// ctrl)`" both backends call. This function's `Key::Named` arm is the one place
// the table of named keys (`"Escape"`, `"Return"`, `"BackSpace"`, `"Tab"`,
// the arrows, `F1`-`F12`, …) is stated, shared by both GTK and TUI.
//
// `keyboard_enhanced` indicates whether the terminal supports the kitty protocol /
// `REPORT_ALL_KEYS_AS_ESCAPE_CODES` flag. With it off, a handful of Ctrl+symbol
// chords arrive from the terminal as byte sequences crossterm cannot distinguish
// from `Ctrl+<digit>` (Ctrl+\ arrives identically to Ctrl+4, Ctrl+/ to Ctrl+7,
// Ctrl+Shift+[ to Ctrl+3, Ctrl+Shift+] to Ctrl+5) — the `!keyboard_enhanced`
// arms below resolve the ambiguity back to the symbolic name. GTK has no such
// ambiguity (GDK always hands over the real keysym for the physical key pressed),
// so it always passes `true`, which disables those terminal-only fallback arms.
//
// `App::handle_dispatch` only calls this function from its `Key::Named` arm —
// its `Key::Char` arm has its own independent decode (`c.to_string()`). This
// function's own `Key::Char` arm therefore has no production caller at all:
// every one of its branches is exercised only by unit tests below, not by a real
// keypress on either backend.
pub fn engine_key_from_ui(
    key: &quadraui::Key,
    modifiers: quadraui::Modifiers,
    keyboard_enhanced: bool,
) -> Option<(String, Option<char>, bool)> {
    use quadraui::{Key, NamedKey};
    let ctrl = modifiers.ctrl;
    let shift = modifiers.shift;
    match key {
        Key::Char(c) => {
            let c = *c;
            let lower = c.to_ascii_lowercase();
            let (key_name, unicode) = if ctrl {
                // Engine dispatches Ctrl combos via key_name (e.g. "d" for Ctrl-D).
                // Space is a named key; use "space" to match GTK and the engine's convention.
                // Ctrl+Shift+X: the char arrives as uppercase (or SHIFT flag is set); keep
                // uppercase so the engine can distinguish Ctrl+P from Ctrl+Shift+P ("P").
                // Some special chars use GTK-style names to match GTK backend conventions.
                let name = if lower == ' ' {
                    "space".to_string()
                } else if lower == '|' {
                    // Ctrl+Shift+\: the literal already-shifted glyph —
                    // same pattern as the bracket pairs' literal '}'/'{'
                    // arms below.
                    "Shift_backslash".to_string()
                } else if lower == '\\' && shift {
                    // Ctrl+Shift+\ via an explicit Shift modifier bit
                    // (kitty/CSI-u reporting the base key plus SHIFT rather
                    // than the shifted glyph) — same pattern as the bracket
                    // pairs' `&& shift` arms below.
                    "Shift_backslash".to_string()
                } else if lower == '\\' || (!keyboard_enhanced && lower == '4') {
                    // Ctrl+\ sends byte 0x1C; without keyboard enhancement crossterm decodes
                    // 0x1C as KeyCode::Char('4')+CONTROL (formula: 0x1C-0x1C+'4'='4').
                    // Map both to "backslash" so Ctrl+\ works in all terminals.
                    //
                    // A *legacy* (non-keyboard-enhanced) terminal has
                    // no way to report Ctrl+Shift+\ distinctly from Ctrl+\ at
                    // all — Ctrl+\ is the ANSI C0 byte 0x1C regardless of
                    // Shift (same shift-blindness as the Ctrl+K/Ctrl+P
                    // family; see `tests/vscode_keybinding_parity.rs`'s
                    // `REACHABILITY_TABLE`), so this fallback arm stays
                    // shift-blind on purpose.
                    "backslash".to_string()
                } else if lower == '/' || (!keyboard_enhanced && lower == '7') {
                    // Ctrl+/ sends byte 0x1F; without keyboard enhancement crossterm
                    // decodes 0x1F as KeyCode::Char('7')+CONTROL (formula: 0x1F-0x1C+'4'='7').
                    // Map both to "slash" so Ctrl+/ works in all terminals.
                    "slash".to_string()
                } else if lower == '`' {
                    "grave".to_string()
                } else if lower == ',' {
                    "comma".to_string()
                } else if (lower == ']' || lower == '}' || (!keyboard_enhanced && lower == '5'))
                    && shift
                {
                    "Shift_bracketright".to_string()
                } else if (lower == '[' || lower == '{' || (!keyboard_enhanced && lower == '3'))
                    && shift
                {
                    "Shift_bracketleft".to_string()
                } else if lower == '}' {
                    // Ctrl+Shift+] without keyboard enhancement: terminal sends '}'
                    "Shift_bracketright".to_string()
                } else if lower == '{' {
                    // Ctrl+Shift+[ without keyboard enhancement: terminal sends '{'
                    "Shift_bracketleft".to_string()
                } else if lower == ']' || (!keyboard_enhanced && lower == '5') {
                    "bracketright".to_string()
                } else if lower == '[' || (!keyboard_enhanced && lower == '3') {
                    "bracketleft".to_string()
                } else if c.is_uppercase() || shift {
                    lower.to_ascii_uppercase().to_string()
                } else {
                    lower.to_string()
                };
                (name, Some(lower))
            } else {
                // With keyboard enhancement (Kitty protocol + REPORT_ALL_KEYS_AS_ESCAPE_CODES),
                // shifted symbol keys may arrive as the base key + SHIFT modifier instead of
                // the resulting character.  For example ':' comes as Char(';') + SHIFT, not
                // Char(':').  Apply the standard US keyboard shift mapping so the engine
                // receives the correct character.
                let resolved = if keyboard_enhanced && shift {
                    shift_map_us(c)
                } else {
                    c
                };
                (String::new(), Some(resolved))
            };
            Some((key_name, unicode, ctrl))
        }
        Key::Named(named) => match named {
            NamedKey::Escape => Some(("Escape".to_string(), None, false)),
            NamedKey::Enter if shift && ctrl => Some(("Shift_Return".to_string(), None, true)),
            NamedKey::Enter if ctrl => Some(("Return".to_string(), None, true)),
            NamedKey::Enter => Some(("Return".to_string(), None, false)),
            NamedKey::Backspace => Some(("BackSpace".to_string(), None, false)),
            NamedKey::Delete => Some(("Delete".to_string(), None, false)),
            NamedKey::Tab => Some(("Tab".to_string(), None, ctrl)),
            NamedKey::BackTab => Some(("ISO_Left_Tab".to_string(), None, ctrl)),
            // Shift+Arrow (no ctrl): emit as "Shift_X" for VSCode selection extension.
            NamedKey::Up if shift && !ctrl => Some(("Shift_Up".to_string(), None, false)),
            NamedKey::Down if shift && !ctrl => Some(("Shift_Down".to_string(), None, false)),
            NamedKey::Left if shift && !ctrl => Some(("Shift_Left".to_string(), None, false)),
            NamedKey::Right if shift && !ctrl => Some(("Shift_Right".to_string(), None, false)),
            NamedKey::Home if shift => Some(("Shift_Home".to_string(), None, false)),
            NamedKey::End if shift => Some(("Shift_End".to_string(), None, false)),
            // Ctrl+Shift+Arrow: emit as "Shift_X" with ctrl=true for word-level selection.
            NamedKey::Left if shift && ctrl => Some(("Shift_Left".to_string(), None, true)),
            NamedKey::Right if shift && ctrl => Some(("Shift_Right".to_string(), None, true)),
            NamedKey::Up => Some(("Up".to_string(), None, false)),
            NamedKey::Down => Some(("Down".to_string(), None, false)),
            NamedKey::Left => Some(("Left".to_string(), None, ctrl)),
            NamedKey::Right => Some(("Right".to_string(), None, ctrl)),
            NamedKey::Home => Some(("Home".to_string(), None, ctrl)),
            NamedKey::End => Some(("End".to_string(), None, ctrl)),
            NamedKey::PageUp => Some(("Page_Up".to_string(), None, false)),
            NamedKey::PageDown => Some(("Page_Down".to_string(), None, false)),
            NamedKey::F(n) => Some((format!("F{n}"), None, false)),
            // GTK used to bypass this decoder for `Insert` entirely
            // (returning `None` here would have silently dropped its
            // terminal PTY passthrough, which reads the literal `"Insert"`
            // name — see `terminal_ops::key_to_pty_bytes`). Naming it here
            // instead lets GTK route through the shared decoder like the
            // other named keys, and gives TUI a working `Insert` key (routed
            // through the same `"Insert"`-accepting PTY passthrough) instead
            // of the silent no-op it got before, since TUI already decodes
            // crossterm's `KeyCode::Insert` into this same `NamedKey::Insert`
            // (quadraui `tui/events.rs`) but had nowhere for it to go.
            NamedKey::Insert => Some(("Insert".to_string(), None, false)),
            NamedKey::CapsLock | NamedKey::NumLock | NamedKey::ScrollLock | NamedKey::Menu => None,
        },
    }
}

/// US-keyboard shift mapping for symbol keys, used by [`engine_key_from_ui`]
/// when `keyboard_enhanced` is set and a shifted symbol key arrives as the
/// base key + `SHIFT` (e.g. `Char(';')` + `SHIFT` for `:`) rather than the
/// resolved character.
fn shift_map_us(c: char) -> char {
    match c {
        '`' => '~',
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        ';' => ':',
        '\'' => '"',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        // Letters: Shift+a → 'A' (crossterm usually already sends uppercase).
        c if c.is_ascii_lowercase() => c.to_ascii_uppercase(),
        _ => c,
    }
}

#[cfg(test)]
mod engine_key_from_ui_tests {
    //! These tests build the `Key`/`Modifiers` that crossterm+quadraui produce
    //! for various terminal bytes (per crossterm's control-code decoding: bytes
    //! 0x01-0x1A become `Char(('a' - 1 + byte) as char)` + `CONTROL`, except those
    //! with dedicated named keys) and assert what [`engine_key_from_ui`] does with
    //! it. This verifies correct handling of control codes like `<C-h>`, `<C-j>`,
    //! `<C-c>` that arrive as ctrl+letter rather than as named keys.
    use super::*;
    use quadraui::{Key, Modifiers, NamedKey};

    fn ctrl_char(c: char) -> (Key, Modifiers) {
        (
            Key::Char(c),
            Modifiers {
                ctrl: true,
                ..Default::default()
            },
        )
    }

    /// 0x08 (^H): decodes as ctrl+'h', not as a `BackSpace` key — callers
    /// must special-case it themselves.
    #[test]
    fn byte_0x08_is_ctrl_h_not_backspace() {
        let (key, mods) = ctrl_char('h');
        let (name, unicode, ctrl) = engine_key_from_ui(&key, mods, false).unwrap();
        assert_eq!(name, "h");
        assert_eq!(unicode, Some('h'));
        assert!(ctrl);
    }

    /// 0x0A (^J / <NL>): decodes as ctrl+'j'.
    #[test]
    fn byte_0x0a_is_ctrl_j() {
        let (key, mods) = ctrl_char('j');
        let (name, unicode, ctrl) = engine_key_from_ui(&key, mods, false).unwrap();
        assert_eq!(name, "j");
        assert_eq!(unicode, Some('j'));
        assert!(ctrl);
    }

    /// 0x03 (^C / ETX): decodes as ctrl+'c'.
    #[test]
    fn byte_0x03_is_ctrl_c() {
        let (key, mods) = ctrl_char('c');
        let (name, unicode, ctrl) = engine_key_from_ui(&key, mods, false).unwrap();
        assert_eq!(name, "c");
        assert_eq!(unicode, Some('c'));
        assert!(ctrl);
    }

    /// 0x1B (ESC): a dedicated named key for this byte — unlike ^H/^J/^C it
    /// never arrives as ctrl+'['.
    #[test]
    fn byte_0x1b_is_escape() {
        let (name, unicode, ctrl) =
            engine_key_from_ui(&Key::Named(NamedKey::Escape), Modifiers::default(), false).unwrap();
        assert_eq!(name, "Escape");
        assert_eq!(unicode, None);
        assert!(!ctrl);
    }

    /// 0x7F (DEL): the physical Backspace key on most terminals; a dedicated
    /// named key for it.
    #[test]
    fn byte_0x7f_is_backspace() {
        let (name, unicode, ctrl) = engine_key_from_ui(
            &Key::Named(NamedKey::Backspace),
            Modifiers::default(),
            false,
        )
        .unwrap();
        assert_eq!(name, "BackSpace");
        assert_eq!(unicode, None);
        assert!(!ctrl);
    }

    /// Plain letter, no modifiers: name is empty (the general fallback reads
    /// `unicode`), consistent with the decoder's contract that call sites rely on
    /// (`if key_name.is_empty() { use unicode }`).
    #[test]
    fn plain_letter_has_empty_name_and_carries_unicode() {
        let (name, unicode, ctrl) =
            engine_key_from_ui(&Key::Char('j'), Modifiers::default(), false).unwrap();
        assert_eq!(name, "");
        assert_eq!(unicode, Some('j'));
        assert!(!ctrl);
    }

    /// Shifted letter, keyboard-enhanced terminal: still just the resolved
    /// char (`shift_map_us` on a letter is a no-op — crossterm already
    /// delivers the uppercase char).
    #[test]
    fn shifted_letter_keyboard_enhanced() {
        let mods = Modifiers {
            shift: true,
            ..Default::default()
        };
        let (name, unicode, ctrl) = engine_key_from_ui(&Key::Char('A'), mods, true).unwrap();
        assert_eq!(name, "");
        assert_eq!(unicode, Some('A'));
        assert!(!ctrl);
    }

    /// Keyboard-enhanced terminal: a shifted symbol key arrives as the base
    /// key + SHIFT (`;` + SHIFT for `:`), which `shift_map_us` resolves.
    #[test]
    fn shifted_symbol_keyboard_enhanced_resolves_via_shift_map() {
        let mods = Modifiers {
            shift: true,
            ..Default::default()
        };
        let (_, unicode, _) = engine_key_from_ui(&Key::Char(';'), mods, true).unwrap();
        assert_eq!(unicode, Some(':'));
    }

    /// Without keyboard enhancement, the base+SHIFT resolution does not
    /// apply — the terminal is assumed to have already sent the resolved
    /// character (which is how a non-enhanced terminal actually behaves).
    #[test]
    fn shifted_symbol_without_keyboard_enhancement_is_not_remapped() {
        let mods = Modifiers {
            shift: true,
            ..Default::default()
        };
        let (_, unicode, _) = engine_key_from_ui(&Key::Char(';'), mods, false).unwrap();
        assert_eq!(unicode, Some(';'));
    }

    /// Ctrl+Shift+letter: the name carries the uppercase letter so the engine
    /// can distinguish Ctrl+P from Ctrl+Shift+P.
    #[test]
    fn ctrl_shift_letter_is_uppercase_name() {
        let mods = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        let (name, unicode, ctrl) = engine_key_from_ui(&Key::Char('p'), mods, true).unwrap();
        assert_eq!(name, "P");
        assert_eq!(unicode, Some('p'));
        assert!(ctrl);
    }

    /// Ctrl+\: the GTK-style symbolic name, regardless of keyboard
    /// enhancement — unambiguous on every terminal for the literal backslash
    /// character.
    #[test]
    fn ctrl_backslash_is_backslash() {
        let (key, mods) = ctrl_char('\\');
        let (name, ..) = engine_key_from_ui(&key, mods, true).unwrap();
        assert_eq!(name, "backslash");
    }

    /// Without keyboard enhancement, Ctrl+\ and Ctrl+4 are indistinguishable
    /// at the byte level, so both must resolve to "backslash".
    #[test]
    fn ctrl_4_without_keyboard_enhancement_aliases_to_backslash() {
        let (key, mods) = ctrl_char('4');
        let (name, ..) = engine_key_from_ui(&key, mods, false).unwrap();
        assert_eq!(name, "backslash");
    }

    /// With keyboard enhancement, that ambiguity does not exist: Ctrl+4 stays
    /// Ctrl+4 (used for "focus editor group 4").
    #[test]
    fn ctrl_4_with_keyboard_enhancement_stays_ctrl_4() {
        let (key, mods) = ctrl_char('4');
        let (name, ..) = engine_key_from_ui(&key, mods, true).unwrap();
        assert_eq!(name, "4");
    }

    /// GTK (and a terminal that resolves the shifted glyph before
    /// reporting it) delivers Ctrl+Shift+\ as the literal shifted character
    /// '|', same pattern as the bracket pairs' literal '}'/'{' arms — must
    /// decode distinctly from plain Ctrl+\'s `"backslash"` (VS Code's
    /// `jumpToBracket` vs `open_editor_group`).
    #[test]
    fn ctrl_pipe_literal_is_shift_backslash() {
        let (key, mods) = ctrl_char('|');
        let (name, _, ctrl) = engine_key_from_ui(&key, mods, true).unwrap();
        assert_eq!(name, "Shift_backslash");
        assert!(ctrl);
    }

    /// The kitty/CSI-u counterpart — the base key plus an explicit
    /// Shift bit, rather than the shifted glyph — must decode to the same
    /// distinct name.
    #[test]
    fn ctrl_shift_backslash_with_explicit_shift_bit_is_shift_backslash() {
        let mods = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        let (name, _, ctrl) = engine_key_from_ui(&Key::Char('\\'), mods, true).unwrap();
        assert_eq!(name, "Shift_backslash");
        assert!(ctrl);
    }

    /// Without keyboard enhancement, Ctrl+\ and Ctrl+Shift+\ are
    /// indistinguishable at the byte level (same ANSI-C0 shift-blindness as
    /// the Ctrl+K/Ctrl+P family) — plain Ctrl+\ must still decode to
    /// `"backslash"`, not the Shift variant, when no Shift bit is present.
    #[test]
    fn ctrl_backslash_without_shift_stays_backslash() {
        let (key, mods) = ctrl_char('\\');
        let (name, ..) = engine_key_from_ui(&key, mods, false).unwrap();
        assert_eq!(name, "backslash");
    }

    /// Named keys that the engine has no binding for today.
    ///
    /// `NamedKey::Insert` used to return `None` — see
    /// `insert_key_decodes_to_the_gtk_terminal_pty_spelling` below for why
    /// it now has a real arm instead.
    #[test]
    fn unbound_named_keys_return_none() {
        for named in [
            NamedKey::CapsLock,
            NamedKey::NumLock,
            NamedKey::ScrollLock,
            NamedKey::Menu,
        ] {
            assert!(engine_key_from_ui(&Key::Named(named), Modifiers::default(), false).is_none());
        }
    }

    /// `NamedKey::Insert` now decodes to the terminal PTY spelling `"Insert"`.
    /// This enables both GTK and TUI to handle the Insert key correctly through
    /// the shared decoder.
    #[test]
    fn insert_key_decodes_to_the_gtk_terminal_pty_spelling() {
        let (name, unicode, ctrl) =
            engine_key_from_ui(&Key::Named(NamedKey::Insert), Modifiers::default(), true).unwrap();
        assert_eq!(name, "Insert");
        assert_eq!(unicode, None);
        assert!(!ctrl);
    }

    /// Shift+Up (no ctrl): VSCode-mode selection-extension spelling, shared
    /// by both backends.
    #[test]
    fn shift_up_is_shift_up() {
        let mods = Modifiers {
            shift: true,
            ..Default::default()
        };
        let (name, unicode, ctrl) =
            engine_key_from_ui(&Key::Named(NamedKey::Up), mods, false).unwrap();
        assert_eq!(name, "Shift_Up");
        assert_eq!(unicode, None);
        assert!(!ctrl);
    }

    /// Ctrl+Shift+Right: word-level selection spelling, ctrl carried through.
    #[test]
    fn ctrl_shift_right_is_shift_right_with_ctrl() {
        let mods = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        let (name, unicode, ctrl) =
            engine_key_from_ui(&Key::Named(NamedKey::Right), mods, false).unwrap();
        assert_eq!(name, "Shift_Right");
        assert_eq!(unicode, None);
        assert!(ctrl);
    }

    /// F-keys format as `F{n}` regardless of modifiers.
    #[test]
    fn f5_formats_as_f5() {
        let (name, unicode, ctrl) =
            engine_key_from_ui(&Key::Named(NamedKey::F(5)), Modifiers::default(), false).unwrap();
        assert_eq!(name, "F5");
        assert_eq!(unicode, None);
        assert!(!ctrl);
    }
}
