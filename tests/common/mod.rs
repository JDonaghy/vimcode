#![allow(dead_code)]
use vimcode_core::{Cursor, Engine, EngineAction, Mode, RegType};

// ── Construction ──────────────────────────────────────────────────────────────

/// Create an engine with pre-populated buffer content.
///
/// Resets settings and extension state to library defaults so integration tests
/// are hermetic: `Settings::load()` and `ExtensionState::load()` read real
/// config files on disk (no `#[cfg(test)]` guard in the compiled library),
/// which would vary between machines.
///
/// Also calls `suppress_disk_saves()` so that commands like `:ExtInstall` that
/// trigger `save()` do not overwrite the user's real `~/.config/vimcode/` files
/// during `cargo test`, and `suppress_disk_loads()` so that
/// `Engine::new()`'s `HistoryState::load()` does not seed every test's
/// `:history`/`<C-r>` state with whatever command history happens to be
/// sitting in the developer machine's real config directory.
pub fn engine_with(text: &str) -> Engine {
    // Suppress all disk writes for the lifetime of this test process.
    vimcode_core::core::session::suppress_disk_saves();
    // Suppress opted-in disk loads (currently just history.json) for the
    // lifetime of this test process.
    vimcode_core::core::session::suppress_disk_loads();
    let mut e = Engine::new();
    e.settings = vimcode_core::Settings::default();
    e.extension_state = vimcode_core::core::session::ExtensionState::default();
    e.ext_registry = None;
    // Reset mode + keymaps in case Engine::new() loaded VSCode mode from disk.
    e.mode = Mode::Normal;
    e.rebuild_user_keymaps();
    // Re-register Phase B.2 accelerators against the reset settings so the
    // registry uses the default `<C-S-t>` binding (not whatever the user's
    // real settings.json had on disk).
    e.accelerators.clear();
    e.register_default_accelerators();
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

/// Assert a register's text and charwise/linewise-ness.
///
/// Kept boolean for the ~100 existing call sites; use
/// [`assert_register_typed`] when the case is about blockwise-ness.
/// A blockwise register fails here whichever bool you pass — that is
/// deliberate, so a charwise assertion cannot silently pass on a block.
pub fn assert_register(e: &Engine, reg: char, text: &str, linewise: bool) {
    assert_register_typed(e, reg, text, RegType::from_linewise(linewise));
}

/// Assert a register's text and its exact [`RegType`] — including
/// [`RegType::Blockwise`], which `<C-v>y` must set so a later `p` re-inserts
/// a rectangle instead of pasting the block's rows as plain lines.
pub fn assert_register_typed(e: &Engine, reg: char, text: &str, ty: RegType) {
    let (t, actual) = e
        .registers
        .get(&reg)
        .unwrap_or_else(|| panic!("register '{reg}' not set"));
    assert_eq!(t, text, "register '{reg}' text");
    assert_eq!(actual, &ty, "register '{reg}' type");
}

pub fn assert_msg_contains(e: &Engine, substr: &str) {
    assert!(
        e.message.contains(substr),
        "message: expected {substr:?} in {:?}",
        e.message
    );
}

// ── Isolated nvim runtime dirs ──────────────────────────────────────────────

/// A private `$XDG_RUNTIME_DIR` for one test-spawned `nvim` process, removed
/// (server socket and all) when this guard drops.
///
/// Every `nvim` process creates a server socket at
/// `$XDG_RUNTIME_DIR/nvim.<pid>.0` on startup and removes it only on a clean
/// `:quit`. A harness that kills a disposable per-case `nvim` instead of
/// quitting it (cheaper, and the usual choice when the process may be
/// mid-command or mid-prompt) leaves that socket behind in whichever
/// directory `XDG_RUNTIME_DIR` pointed at. Pointing every spawn at its own
/// throwaway directory means any such leak lands there instead of the host's
/// real (and often size-limited) runtime dir, and is deleted along with it —
/// whether or not `nvim` itself got to clean up first.
pub struct IsolatedNvimRuntimeDir {
    path: std::path::PathBuf,
}

impl IsolatedNvimRuntimeDir {
    /// Create a fresh, empty directory under the system temp dir and return
    /// a guard that removes it (recursively) on drop, including on panic —
    /// local drops run while a `#[test]` thread unwinds.
    pub fn new(tag: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "vimcode_nvim_xdg_{tag}_{}_{nanos}_{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create isolated XDG_RUNTIME_DIR for nvim");
        Self { path }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for IsolatedNvimRuntimeDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
