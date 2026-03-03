#![allow(dead_code)]
use vimcode_core::{Cursor, Engine, EngineAction, Mode};

// ── Construction ──────────────────────────────────────────────────────────────

/// Create an engine with pre-populated buffer content.
///
/// Resets settings and extension state to library defaults so integration tests
/// are hermetic: `Settings::load()` and `ExtensionState::load()` read real
/// config files on disk (no `#[cfg(test)]` guard in the compiled library),
/// which would vary between machines.
pub fn engine_with(text: &str) -> Engine {
    let mut e = Engine::new();
    e.settings = vimcode_core::Settings::default();
    e.extension_state = vimcode_core::core::session::ExtensionState::default();
    if !text.is_empty() {
        e.buffer_mut().insert(0, text);
    }
    e
}

// ── Key input ─────────────────────────────────────────────────────────────────

pub fn press(e: &mut Engine, ch: char) -> EngineAction {
    e.handle_key(&ch.to_string(), Some(ch), false)
}

pub fn press_key(e: &mut Engine, name: &str) -> EngineAction {
    e.handle_key(name, None, false)
}

pub fn ctrl(e: &mut Engine, ch: char) -> EngineAction {
    e.handle_key(&ch.to_string(), Some(ch), true)
}

pub fn type_chars(e: &mut Engine, s: &str) {
    for ch in s.chars() {
        e.handle_key(&ch.to_string(), Some(ch), false);
    }
}

/// Enter command mode, type cmd, press Return.
pub fn run_cmd(e: &mut Engine, cmd: &str) -> EngineAction {
    press(e, ':');
    type_chars(e, cmd);
    press_key(e, "Return")
}

/// Call execute_command directly (bypasses command-mode UI).
pub fn exec(e: &mut Engine, cmd: &str) -> EngineAction {
    e.execute_command(cmd)
}

pub fn search_fwd(e: &mut Engine, q: &str) {
    press(e, '/');
    type_chars(e, q);
    press_key(e, "Return");
}

pub fn search_bwd(e: &mut Engine, q: &str) {
    press(e, '?');
    type_chars(e, q);
    press_key(e, "Return");
}

/// Drain the macro playback queue (the engine fills it on @reg; UI normally pumps it).
pub fn drain_macro_queue(e: &mut Engine) {
    while !e.macro_playback_queue.is_empty() {
        e.advance_macro_playback();
    }
}

// ── Buffer state ───────────────────────────────────────────────────────────────

pub fn buf(e: &Engine) -> String {
    e.buffer().to_string()
}

pub fn get_lines(e: &Engine) -> Vec<String> {
    e.buffer()
        .to_string()
        .lines()
        .map(|l| l.to_string())
        .collect()
}

pub fn set_content(e: &mut Engine, text: &str) {
    let char_len = e.buffer().len_chars();
    e.buffer_mut().delete_range(0, char_len);
    if !text.is_empty() {
        e.buffer_mut().insert(0, text);
    }
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.set_dirty(false);
}

// ── Assertions ────────────────────────────────────────────────────────────────

pub fn assert_cursor(e: &Engine, line: usize, col: usize) {
    let c = e.cursor();
    assert_eq!(
        (c.line, c.col),
        (line, col),
        "cursor: expected ({line},{col}), got ({},{})",
        c.line,
        c.col
    );
}

pub fn assert_buf(e: &Engine, expected: &str) {
    assert_eq!(buf(e), expected, "buffer content mismatch");
}

pub fn assert_mode(e: &Engine, mode: Mode) {
    assert_eq!(e.mode, mode, "mode: expected {mode:?}, got {:?}", e.mode);
}

pub fn assert_register(e: &Engine, reg: char, text: &str, linewise: bool) {
    let (t, lw) = e
        .registers
        .get(&reg)
        .unwrap_or_else(|| panic!("register '{reg}' not set"));
    assert_eq!(t, text, "register '{reg}' text");
    assert_eq!(lw, &linewise, "register '{reg}' linewise");
}

pub fn assert_msg_contains(e: &Engine, substr: &str) {
    assert!(
        e.message.contains(substr),
        "message: expected {substr:?} in {:?}",
        e.message
    );
}
