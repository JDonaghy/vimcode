//! TUI (terminal UI) entry point for VimCode.
//!
//! Activated with the `--tui` CLI flag. Uses ratatui + crossterm to render
//! the same `ScreenLayout` produced by `render::build_screen_layout` that the
//! GTK backend consumes — just rendered to a terminal instead of a Cairo
//! surface.
//!
//! **No GTK/Cairo/Pango imports here.** All editor logic comes from `core`.
//! All rendering data comes from `render`.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Stdout, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

// ─── Debug logging ────────────────────────────────────────────────────────────

/// Global debug log file handle, set once at startup via `--debug <path>`.
static DEBUG_LOG: std::sync::OnceLock<Mutex<std::fs::File>> = std::sync::OnceLock::new();

/// Initialise the debug log.  Call once before the event loop starts.
fn init_debug_log(path: &str) {
    match std::fs::File::create(path) {
        Ok(f) => {
            let _ = DEBUG_LOG.set(Mutex::new(f));
        }
        Err(e) => {
            eprintln!("Warning: cannot open debug log {path}: {e}");
        }
    }
}

/// Write a formatted message to the debug log (if enabled).  No-op when
/// `--debug` was not passed.
#[allow(unused_macros)]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if let Some(mtx) = $crate::tui_main::DEBUG_LOG.get() {
            if let Ok(mut f) = mtx.lock() {
                let _ = writeln!(f, $($arg)*);
                let _ = f.flush();
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use debug_log;

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{
    self as ct_event, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste,
    EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    KeyboardEnhancementFlags, MouseButton, MouseEvent, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen,
    LeaveAlternateScreen, SetTitle,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color as RColor, Modifier};
use ratatui::Terminal;

use crate::core::engine::{DiffLine, EngineAction};
use crate::core::lsp::DiagnosticSeverity;
use crate::core::settings::ExplorerAction;
use crate::core::window::SplitDirection;
use crate::core::{Engine, GitLineStatus, Mode, OpenMode, WindowRect};
use crate::render::{
    self, build_screen_layout, Color, CompletionMenu, CursorShape, RenderedLine, RenderedWindow,
    SelectionKind, Theme, WildmenuData,
};

// ─── Key binding helpers ──────────────────────────────────────────────────────

/// Returns true if the given crossterm key event matches a panel_keys binding string.
/// Binding strings use Vim notation: `<C-b>`, `<C-S-e>`, `<A-x>`.
fn matches_tui_key(binding: &str, code: KeyCode, mods: KeyModifiers) -> bool {
    let Some((ctrl, shift, alt, key_name)) =
        crate::core::settings::parse_key_binding_named(binding)
    else {
        return false;
    };
    if ctrl != mods.contains(KeyModifiers::CONTROL) {
        return false;
    }
    if shift != mods.contains(KeyModifiers::SHIFT) {
        return false;
    }
    if alt != mods.contains(KeyModifiers::ALT) {
        return false;
    }
    match key_name.as_str() {
        "Tab" | "tab" => matches!(code, KeyCode::Tab),
        "Space" | "space" => matches!(code, KeyCode::Char(' ')),
        "Escape" | "Esc" => matches!(code, KeyCode::Esc),
        s if s.chars().count() == 1 => {
            let ch = s.chars().next().unwrap().to_ascii_lowercase();
            matches!(code, KeyCode::Char(c) if c.to_ascii_lowercase() == ch)
        }
        _ => false,
    }
}

// ─── Sidebar constants ────────────────────────────────────────────────────────

const SIDEBAR_WIDTH: u16 = 30;
const ACTIVITY_BAR_WIDTH: u16 = 3;
/// Number of terminal columns the explorer toolbar occupies:
/// 3 Nerd Font icons × 3 cols each (2-col icon + 1 space) = 9.
const EXPLORER_TOOLBAR_LEN: u16 = 9;

// ─── Activity bar panels ──────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum TuiPanel {
    Explorer,
    Search,
    Settings,
    Debug,
    Git,
    Extensions,
    Ai,
}

// ─── Sidebar data structures ──────────────────────────────────────────────────

struct ExplorerRow {
    depth: usize,
    name: String,
    path: PathBuf,
    is_dir: bool,
    is_expanded: bool,
}

struct TuiSidebar {
    visible: bool,
    has_focus: bool,
    active_panel: TuiPanel,
    selected: usize,
    scroll_top: usize,
    rows: Vec<ExplorerRow>,
    root: PathBuf,
    /// Set of directory paths that are currently expanded.
    expanded: HashSet<PathBuf>,
    /// True while typing in the search input box (Search panel only).
    search_input_mode: bool,
    /// When true and `search_input_mode` is true, the replace input is focused.
    replace_input_focused: bool,
    /// Scroll offset for the search results area (written back by render_search_panel).
    search_scroll_top: usize,
    /// Whether to show dotfiles in the explorer (mirrors Settings.show_hidden_files).
    show_hidden_files: bool,
    /// When true, the activity bar (toolbar) has keyboard focus.
    toolbar_focused: bool,
    /// Currently highlighted row in the activity bar (0=hamburger, 1-6=panels, 7=settings).
    toolbar_selected: u16,
    /// True after Ctrl-W is pressed in a sidebar panel, waiting for h/j/k/l.
    pending_ctrl_w: bool,
    /// When set, sidebar renders an extension panel instead of the fixed panels.
    ext_panel_name: Option<String>,
}

impl TuiSidebar {
    fn new(root: PathBuf, visible: bool) -> Self {
        let mut expanded = HashSet::new();
        // Root folder starts expanded so the tree is visible
        expanded.insert(root.clone());
        let mut sb = TuiSidebar {
            visible,
            has_focus: false,
            active_panel: TuiPanel::Explorer,
            selected: 0,
            scroll_top: 0,
            rows: Vec::new(),
            root,
            expanded,
            search_input_mode: true,
            replace_input_focused: false,
            search_scroll_top: 0,
            show_hidden_files: false,
            toolbar_focused: false,
            toolbar_selected: 1, // Start on Explorer
            pending_ctrl_w: false,
            ext_panel_name: None,
        };
        sb.build_rows();
        sb
    }

    fn build_rows(&mut self) {
        self.rows.clear();
        let root = self.root.clone();
        // Root folder entry at the top (like VSCode project name)
        let root_name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| root.to_string_lossy().to_string());
        let root_expanded = self.expanded.contains(&root);
        self.rows.push(ExplorerRow {
            depth: 0,
            name: root_name.to_uppercase(),
            path: root.clone(),
            is_dir: true,
            is_expanded: root_expanded,
        });
        if root_expanded {
            collect_rows(
                &root,
                1,
                &self.expanded,
                self.show_hidden_files,
                &mut self.rows,
            );
        }
        if !self.rows.is_empty() && self.selected >= self.rows.len() {
            self.selected = self.rows.len() - 1;
        }
    }

    fn toggle_dir(&mut self, idx: usize) {
        if idx < self.rows.len() && self.rows[idx].is_dir {
            let path = self.rows[idx].path.clone();
            if self.expanded.contains(&path) {
                self.expanded.remove(&path);
            } else {
                self.expanded.insert(path);
            }
        }
        self.build_rows();
    }

    /// Scroll so `selected` is visible within the given viewport height.
    fn ensure_visible(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.selected < self.scroll_top {
            self.scroll_top = self.selected;
        } else if self.selected >= self.scroll_top + viewport_height {
            self.scroll_top = self.selected + 1 - viewport_height;
        }
    }

    /// Expand all ancestor directories of `target`, rebuild the row list,
    /// select the row matching `target`, and scroll it into view.
    fn reveal_path(&mut self, target: &Path, viewport_height: usize) {
        // Expand every ancestor directory between root and target.
        if let Ok(rel) = target.strip_prefix(&self.root) {
            let mut accum = self.root.clone();
            for component in rel.parent().into_iter().flat_map(|p| p.components()) {
                accum.push(component);
                self.expanded.insert(accum.clone());
            }
        }
        self.build_rows();
        // Select the row whose path matches target, then scroll into view.
        if let Some(idx) = self.rows.iter().position(|r| r.path == target) {
            self.selected = idx;
            self.ensure_visible(viewport_height);
        }
    }
}

/// Recursively build the flat list of visible rows, respecting the `expanded` set.
fn collect_rows(
    dir: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    show_hidden: bool,
    out: &mut Vec<ExplorerRow>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    // Dirs first, then alphabetical
    entries.sort_by(|a, b| {
        let ad = a.path().is_dir();
        let bd = b.path().is_dir();
        match (ad, bd) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip dotfiles unless show_hidden_files is enabled
        if name.starts_with('.') && !show_hidden {
            continue;
        }
        let is_dir = path.is_dir();
        let is_expanded = is_dir && expanded.contains(&path);
        out.push(ExplorerRow {
            depth,
            name,
            path: path.clone(),
            is_dir,
            is_expanded,
        });
        if is_expanded {
            collect_rows(&path, depth + 1, expanded, show_hidden, out);
        }
    }
}

/// ─── Prompt kind for CRUD operations ─────────────────────────────────────────
#[derive(Clone, Debug)]
enum PromptKind {
    /// New file inside the given directory.
    NewFile(PathBuf),
    /// New folder inside the given directory.
    NewFolder(PathBuf),
    DeleteConfirm(PathBuf),
    /// Move: source path; input is destination dir (relative to project root).
    MoveFile(PathBuf),
}

/// State for an active sidebar prompt shown in the command line area.
struct SidebarPrompt {
    kind: PromptKind,
    input: String,
    cursor: usize, // byte offset into input
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// State for an active scrollbar drag (vertical or horizontal).
struct ScrollDragState {
    window_id: crate::core::WindowId,
    /// `false` = vertical scrollbar, `true` = horizontal scrollbar.
    is_horizontal: bool,
    /// For vertical: absolute terminal row of track top.
    /// For horizontal: absolute terminal column of track start.
    track_abs_start: u16,
    /// For vertical: track height in rows.
    /// For horizontal: track width in columns.
    track_len: u16,
    /// For vertical: total buffer lines.
    /// For horizontal: max line length (max_col).
    total: usize,
}

/// State for an active drag on the sidebar search-panel vertical scrollbar.
struct SidebarScrollDrag {
    /// Absolute terminal row of the first row of the scrollbar track.
    track_abs_start: u16,
    /// Height of the track in rows.
    track_len: u16,
    /// Total number of display rows in the results list.
    total: usize,
}

/// State for an active drag on a debug sidebar section scrollbar.
struct DebugSidebarScrollDrag {
    /// Section index (0–3).
    sec_idx: usize,
    /// Absolute terminal row of the first content row in this section.
    track_abs_start: u16,
    /// Number of content rows in this section.
    track_len: u16,
    /// Total number of items in this section.
    total: usize,
}

/// What the folder picker should do when the user confirms a selection.
#[derive(Clone, PartialEq)]
enum FolderPickerMode {
    /// Open as a workspace folder (`engine.open_folder()`).
    OpenFolder,
    /// Pick from the list of recently opened workspaces.
    OpenRecent,
}

/// TUI folder/workspace directory picker modal.
struct FolderPickerState {
    mode: FolderPickerMode,
    /// Current browsing root (may differ from engine.cwd when user navigates up/down).
    root: PathBuf,
    query: String,
    /// All candidate directories (and .vimcode-workspace files) relative to root.
    all_entries: Vec<PathBuf>,
    /// Currently filtered + sorted entries.
    filtered: Vec<PathBuf>,
    selected: usize,
    scroll_top: usize,
    show_hidden: bool,
}

impl FolderPickerState {
    fn new(cwd: &Path, mode: FolderPickerMode, show_hidden: bool) -> Self {
        let root = cwd.to_path_buf();
        let all_entries = collect_dir_entries(&root, show_hidden);
        let filtered = all_entries.iter().take(50).cloned().collect();
        Self {
            mode,
            root,
            query: String::new(),
            all_entries,
            filtered,
            selected: 0,
            scroll_top: 0,
            show_hidden,
        }
    }

    /// Navigate to a new root directory (clears query, reloads entries).
    fn navigate_to(&mut self, new_root: PathBuf) {
        self.root = new_root;
        self.query.clear();
        self.all_entries = collect_dir_entries(&self.root, self.show_hidden);
        self.filtered = self.all_entries.iter().take(50).cloned().collect();
        self.selected = 0;
        self.scroll_top = 0;
    }

    /// Navigate up to the parent directory.
    fn navigate_up(&mut self) {
        if let Some(parent) = self.root.parent() {
            self.navigate_to(parent.to_path_buf());
        }
    }

    /// Create an Open Recent picker pre-populated with recent workspace paths.
    fn new_recent(recents: &[std::path::PathBuf]) -> Self {
        // Show most-recent first
        let all_entries: Vec<PathBuf> = recents.iter().rev().cloned().collect();
        let filtered = all_entries.clone();
        Self {
            mode: FolderPickerMode::OpenRecent,
            root: PathBuf::new(), // not used for OpenRecent
            query: String::new(),
            all_entries,
            filtered,
            selected: 0,
            scroll_top: 0,
            show_hidden: false,
        }
    }

    fn push_char(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    fn pop_char(&mut self) {
        self.query.pop();
        self.refilter();
    }

    fn refilter(&mut self) {
        self.filtered = filter_dir_entries(&self.all_entries, &self.query);
        self.selected = 0;
        self.scroll_top = 0;
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn move_down(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + 1).min(self.filtered.len() - 1);
        }
    }

    fn selected_path(&self) -> Option<PathBuf> {
        let rel = self.filtered.get(self.selected)?;
        if rel.as_os_str() == ".." {
            self.root.parent().map(|p| p.to_path_buf())
        } else {
            Some(self.root.join(rel))
        }
    }

    /// Clamp `scroll_top` so `selected` is always in the visible window.
    fn sync_scroll(&mut self, visible_rows: usize) {
        if self.selected < self.scroll_top {
            self.scroll_top = self.selected;
        }
        if self.selected >= self.scroll_top + visible_rows {
            self.scroll_top = self.selected + 1 - visible_rows;
        }
    }
}

/// Walk `root` collecting relative subdirectory paths (depth ≤ 5) plus any
/// `.vimcode-workspace` files. Skips hidden dirs, `target/`, `node_modules/`.
/// The entry `"."` (current directory) is prepended so the user can open root.
fn collect_dir_entries(root: &Path, show_hidden: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    // Prepend ".." so the user can navigate up (unless already at filesystem root)
    if root.parent().is_some() {
        out.push(PathBuf::from(".."));
    }
    out.push(PathBuf::from("."));
    walk_dir_entries_recursive(root, root, &mut out, 0, show_hidden);
    out
}

fn walk_dir_entries_recursive(
    root: &Path,
    dir: &Path,
    out: &mut Vec<PathBuf>,
    depth: usize,
    show_hidden: bool,
) {
    if depth > 5 {
        return;
    }
    let mut entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(e) => e.flatten().collect(),
        Err(_) => return,
    };
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };
        // Skip hidden entries unless show_hidden_files is enabled (except .vimcode-workspace file specifically)
        if name.starts_with('.') && !show_hidden {
            if path.is_file() && name == ".vimcode-workspace" {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_path_buf());
                }
            }
            continue;
        }
        // Skip heavy build/dep directories
        if name == "target" || name == "node_modules" || name == "__pycache__" {
            continue;
        }
        if path.is_dir() {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
            walk_dir_entries_recursive(root, &path, out, depth + 1, show_hidden);
        }
    }
}

/// Filter `all` by `query` using subsequence matching (no score needed here).
fn filter_dir_entries(all: &[PathBuf], query: &str) -> Vec<PathBuf> {
    const CAP: usize = 50;
    if query.is_empty() {
        return all.iter().take(CAP).cloned().collect();
    }
    let q = query.to_lowercase();
    let mut scored: Vec<(i32, &PathBuf)> = all
        .iter()
        .filter_map(|p| {
            let display = p.to_string_lossy().to_lowercase();
            dir_fuzzy_score(&display, &q).map(|s| (s, p))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored
        .into_iter()
        .take(CAP)
        .map(|(_, p)| p.clone())
        .collect()
}

/// Simple subsequence fuzzy match returning a score, or `None` if no match.
fn dir_fuzzy_score(path: &str, query: &str) -> Option<i32> {
    let pb = path.as_bytes();
    let qb = query.as_bytes();
    let mut qi = 0usize;
    let mut score = 100i32;
    let mut last_pi = 0usize;
    for (pi, &byte) in pb.iter().enumerate() {
        if qi < qb.len() && byte == qb[qi] {
            if qi > 0 {
                score -= (pi - last_pi - 1) as i32;
            }
            if pi == 0 || matches!(pb[pi - 1], b'/' | b'_' | b'-' | b'.') {
                score += 5;
            }
            last_pi = pi;
            qi += 1;
        }
    }
    if qi == qb.len() {
        Some(score)
    } else {
        None
    }
}

// =============================================================================
// Stderr suppression (prevents "Can't open display" from corrupting TUI)
// =============================================================================

/// RAII guard that redirects stderr to /dev/null and restores on drop.
struct StderrGuard {
    saved_fd: i32,
}

/// Temporarily suppress stderr output. Returns `None` if the operation fails.
fn suppress_stderr() -> Option<StderrGuard> {
    unsafe {
        let saved = libc::dup(2);
        if saved < 0 {
            return None;
        }
        let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY);
        if devnull < 0 {
            libc::close(saved);
            return None;
        }
        libc::dup2(devnull, 2);
        libc::close(devnull);
        Some(StderrGuard { saved_fd: saved })
    }
}

impl Drop for StderrGuard {
    fn drop(&mut self) {
        unsafe {
            libc::dup2(self.saved_fd, 2);
            libc::close(self.saved_fd);
        }
    }
}

// =============================================================================
// Clipboard setup helpers
// =============================================================================

/// Build the best clipboard context for the current platform.
///
/// On X11 we explicitly use `x11_bin::ClipboardContext` (xclip/xsel subprocesses)
/// rather than letting `try_context()` pick `x11_fork` first.  The fork-based
/// provider delegates `get_contents()` to `X11ClipboardContext::get_contents()`
/// which can stall or return empty when another app owns the clipboard (competing
/// X11 events).  Subprocess reads each open their own independent X11 connection
/// and have no such conflict.
fn build_clipboard_ctx() -> Option<Box<dyn copypasta_ext::ClipboardProviderExt>> {
    // Suppress stderr to prevent "Can't open display" from corrupting the TUI
    // when clipboard providers probe for X11/Wayland availability.
    let _guard = suppress_stderr();

    // On Unix (Linux / BSDs) but not macOS, prefer the binary (subprocess) X11
    // context when running under X11.
    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
    ))]
    {
        if copypasta_ext::display::is_x11() {
            if let Ok(ctx) = copypasta_ext::x11_bin::ClipboardContext::new() {
                return Some(Box::new(ctx));
            }
        }
    }
    copypasta_ext::try_context()
}

/// Set up system clipboard callbacks on the engine.
///
/// Backends (first match wins):
///   X11      → X11BinClipboardContext (xclip/xsel subprocesses — no X11 event conflict)
///   Wayland  → WaylandBinClipboardContext (wl-paste/wl-copy)
///   macOS    → native NSPasteboard
///   Windows  → native Win32
///   headless → None (message shown to user)
fn setup_tui_clipboard(engine: &mut Engine) {
    match build_clipboard_ctx() {
        Some(ctx) => {
            use std::sync::{Arc, Mutex};
            let cb = Arc::new(Mutex::new(ctx));
            let cb_read = cb.clone();
            engine.clipboard_read = Some(Box::new(move || {
                let mut g = cb_read.lock().map_err(|e| format!("clipboard: {e}"))?;
                g.get_contents().map_err(|e| format!("{e}"))
            }));
            let cb_write = cb;
            engine.clipboard_write = Some(Box::new(move |text: &str| {
                let mut g = cb_write.lock().map_err(|e| format!("clipboard: {e}"))?;
                g.set_contents(text.to_string()).map_err(|e| format!("{e}"))
            }));
        }
        None => {
            engine.message = "Clipboard unavailable — \"+/\"* registers unavailable".to_string();
        }
    }
}

/// Sync the unnamed `"` register to the system clipboard if its content changed.
/// Must be called after every keypress that might have yanked/cut text.
fn sync_tui_clipboard(engine: &mut Engine, last: &mut Option<String>) {
    let current = engine
        .registers
        .get(&'"')
        .filter(|(s, _)| !s.is_empty())
        .map(|(s, _)| s.clone());
    if current != *last {
        if let (Some(ref text), Some(ref cb_write)) = (&current, &engine.clipboard_write) {
            let _ = cb_write(text.as_str());
        }
        *last = current;
    }
}

/// Intercept paste keys (`p`/`P`) to load the system clipboard into registers
/// before the engine processes the keypress (clipboard=unnamedplus semantics).
/// Returns true if the key was intercepted and processed.
fn intercept_paste_key(engine: &mut Engine, before: bool) -> bool {
    use crate::core::Mode;
    // Intercept in Normal and Visual modes with a default/clipboard register.
    if !matches!(
        engine.mode,
        Mode::Normal | Mode::Visual | Mode::VisualLine | Mode::VisualBlock
    ) {
        return false;
    }
    if !matches!(
        engine.selected_register,
        None | Some('"') | Some('+') | Some('*')
    ) {
        return false;
    }
    // Read from system clipboard synchronously.
    // Capture any error to show after handle_key (which clears engine.message).
    let clip_err: Option<String> = match engine.clipboard_read {
        None => Some("Clipboard unavailable — install xclip or xsel".to_string()),
        Some(ref cb_read) => match cb_read() {
            Ok(text) if !text.is_empty() => {
                engine.load_clipboard_for_paste(text);
                None
            }
            Ok(_) => None, // empty clipboard — fall through to use internal register
            Err(e) => Some(format!("Clipboard read failed: {e}")),
        },
    };
    // Let engine execute the paste from the (now-updated) register.
    // TUI uses key_name="" for regular chars; unicode carries the actual character.
    let uni = if before { Some('P') } else { Some('p') };
    engine.handle_key("", uni, false);
    // Restore error message after handle_key clears it.
    if let Some(err) = clip_err {
        engine.message = err;
    }
    true
}

/// Initialise the engine, set up the terminal, run the event loop, and restore
/// the terminal on exit.
pub fn run(file_path: Option<PathBuf>, debug_log_path: Option<String>) {
    if let Some(ref path) = debug_log_path {
        init_debug_log(path);
        debug_log!("=== VimCode TUI debug log started ===");
    }

    let mut engine = Engine::new();
    engine.plugin_init();
    engine.restore_session_files();
    if let Some(path) = file_path {
        if path.is_dir() {
            // Directory argument: set cwd and open as workspace folder
            debug_log!("Opening directory from CLI: {:?}", path);
            engine.open_folder(&path);
        } else {
            // Open the CLI file in a tab (on top of any restored session files)
            debug_log!("Opening file from CLI: {:?}", path);
            engine.open_file_in_tab(&path);
        }
    }

    setup_tui_clipboard(&mut engine);

    enable_raw_mode().expect("enable raw mode");
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )
    .expect("enter alternate screen");

    // Enable keyboard enhancement protocol (Kitty protocol) so terminals that support it
    // will send distinct escape sequences for Ctrl+Shift+X vs Ctrl+X.
    // DISAMBIGUATE_ESCAPE_CODES alone is insufficient: it doesn't guarantee that
    // Ctrl+letter combos arrive as CSI u sequences (they may still come as raw
    // control characters, losing the Shift modifier).  REPORT_ALL_KEYS_AS_ESCAPE_CODES
    // forces every keypress to be a CSI u sequence, so Ctrl+Shift+L is unambiguous.
    let keyboard_enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if keyboard_enhanced {
        let _ = execute!(
            stdout,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            )
        );
    }

    // Always install a panic hook that writes crash info to /tmp/vimcode-crash.log
    // AND to the debug log (if --debug is active).  This gives post-mortem diagnostics
    // without requiring the user to reproduce the crash with --debug every time.
    {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let bt = std::backtrace::Backtrace::force_capture();
            let loc_str = info
                .location()
                .map(|l| format!("  at {}:{}:{}\n", l.file(), l.line(), l.column()))
                .unwrap_or_default();
            let crash_msg = format!("PANIC: {}\n{}backtrace:\n{}\n", info, loc_str, bt);
            // Write to always-on crash log so it survives without --debug.
            let _ = std::fs::write("/tmp/vimcode-crash.log", &crash_msg);
            // Also mirror to the debug log when --debug is active.
            debug_log!("{}", crash_msg);
            prev_hook(info);
        }));
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.clear().expect("clear terminal");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        event_loop(&mut terminal, &mut engine, keyboard_enhanced);
    }));

    restore_terminal(&mut terminal, keyboard_enhanced);

    if let Err(e) = result {
        // Extract the panic message before aborting — resume_unwind would call
        // abort() on Linux (via the default panic handler), producing a core dump.
        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            format!("VimCode internal error: {s}")
        } else if let Some(s) = e.downcast_ref::<String>() {
            format!("VimCode internal error: {s}")
        } else {
            "VimCode internal error (unknown panic payload)".to_string()
        };
        eprintln!("{msg}");
        eprintln!("Crash details written to /tmp/vimcode-crash.log");
        eprintln!("Please report this at https://github.com/anthropics/claude-code/issues");
        std::process::exit(1);
    }
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>, keyboard_enhanced: bool) {
    if keyboard_enhanced {
        let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    }
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen
    );
    let _ = terminal.show_cursor();
}

// ─── Event loop ───────────────────────────────────────────────────────────────

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    engine: &mut Engine,
    keyboard_enhanced: bool,
) {
    let mut theme = Theme::from_name(&engine.settings.colorscheme);

    // Initialise sidebar from session/settings
    let initial_visible = if engine.settings.autohide_panels {
        false
    } else {
        engine.session.explorer_visible || engine.settings.explorer_visible_on_startup
    };
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut sidebar = TuiSidebar::new(root, initial_visible);
    sidebar.show_hidden_files = engine.settings.show_hidden_files;

    // Optional active prompt (for sidebar CRUD operations)
    let mut sidebar_prompt: Option<SidebarPrompt> = None;

    // Mutable sidebar width (default SIDEBAR_WIDTH, clamped 15..60)
    let mut sidebar_width: u16 = SIDEBAR_WIDTH;
    // Folder picker modal state (None = closed)
    let mut folder_picker: Option<FolderPickerState> = None;
    // Scroll offset for the fuzzy finder results list
    let mut fuzzy_scroll_top: usize = 0;
    // Scroll offset for the live grep results list
    let mut grep_scroll_top: usize = 0;
    // Scroll offset for the quickfix panel
    let mut quickfix_scroll_top: usize = 0;
    // True while user is dragging the sidebar resize handle
    let mut dragging_sidebar = false;
    // Non-None while user is dragging a scrollbar thumb
    let mut dragging_scrollbar: Option<ScrollDragState> = None;
    // Non-None while user is dragging the search-results scrollbar thumb
    let mut dragging_sidebar_search: Option<SidebarScrollDrag> = None;
    // Non-None while user is dragging a debug sidebar section scrollbar
    let mut dragging_debug_sb: Option<DebugSidebarScrollDrag> = None;
    // Non-None while user is dragging the terminal panel's scrollbar thumb.
    // Stores (track_start_row, track_len, total_scrollback_rows).
    let mut dragging_terminal_sb: Option<(u16, u16, usize)> = None;
    // Scroll offset for the debug output panel (0 = newest/bottom, n = n lines up from bottom).
    let mut debug_output_scroll: usize = 0;
    // Non-None while user is dragging the debug output panel's scrollbar thumb.
    // Stores (track_start_row, track_len, total_lines).
    let mut dragging_debug_output_sb: Option<(u16, u16, usize)> = None;
    // Non-None while user is dragging the settings panel scrollbar.
    let mut dragging_settings_sb: Option<SidebarScrollDrag> = None;
    // True while user drags the terminal header row to resize the panel.
    let mut dragging_terminal_resize: bool = false;
    // True while user drags the terminal split divider left/right.
    let mut dragging_terminal_split: bool = false;
    // Non-None while user is dragging a group divider (stores split_index).
    let mut dragging_group_divider: Option<usize> = None;
    // Cache of the last rendered layout for mouse hit-testing
    let mut last_layout: Option<render::ScreenLayout> = None;
    // Double-click detection state
    let mut last_click_time = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    let mut last_click_pos: (u16, u16) = (0, 0);
    // Whether a mouse text drag is active (not scrollbar drag)
    let mut mouse_text_drag = false;
    // Command-line mouse text selection: (start_col, end_col) in the rendered row.
    let mut cmd_sel: Option<(usize, usize)> = None;
    let mut cmd_dragging = false;
    // Explorer drag-and-drop: row index where mouse-down occurred (potential drag source).
    let mut explorer_drag_src: Option<usize> = None;
    // Active explorer drag state: (source row index, current target row index or None).
    let mut explorer_drag_active: Option<(usize, Option<usize>)> = None;

    // Track unnamed register content so we only write to clipboard on changes.
    let mut last_clipboard_content: Option<String> = None;
    // True when the quit-confirm overlay is shown (unsaved changes on exit).
    let mut quit_confirm = false;
    // True when the close-tab-confirm overlay is shown (unsaved changes on tab close).
    let mut close_tab_confirm = false;

    let mut needs_redraw = true;
    // Track last draw time to cap frame rate at ~60 fps and keep CPU low.
    let min_frame = Duration::from_millis(16);
    let mut last_draw = Instant::now()
        .checked_sub(min_frame)
        .unwrap_or_else(Instant::now);
    // Auto-refresh sidebar to reflect external filesystem changes.
    let mut last_sidebar_refresh = Instant::now();
    // Auto-reload buffers whose files changed on disk.
    let mut last_file_check = Instant::now();
    // mtime of settings.json at last check — used to auto-reload when user edits the file.
    let mut settings_mtime: Option<std::time::SystemTime> = {
        let path = crate::core::settings::Settings::settings_file_path();
        fs::metadata(&path).ok().and_then(|m| m.modified().ok())
    };
    // Deadline to clear the yank highlight flash.
    let mut yank_hl_deadline: Option<Instant> = None;
    // Timestamp of the last Alt+t press (for tab switcher auto-confirm on timeout).
    let mut tab_switcher_last_cycle: Option<Instant> = None;

    loop {
        // Refresh theme in case :colorscheme was run.
        theme = Theme::from_name(&engine.settings.colorscheme);

        // Sync viewport dimensions so ensure_cursor_visible uses real terminal size.
        // Layout: [activity_bar(3)] [sidebar(sw+1sep, if visible)] [editor_col]
        // editor_col: [tab(1)] / [editor] then global [status(1)] [cmd(1)]
        if let Ok(size) = terminal.size() {
            let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
            let trm_rows: u16 = if engine.terminal_open || engine.bottom_panel_open {
                engine.session.terminal_panel_rows + 2 // match draw_frame: tab bar + header + content
            } else {
                0
            };
            let menu_row: u16 = if engine.menu_bar_visible { 1 } else { 0 };
            let dbg_row: u16 = if engine.debug_toolbar_visible { 1 } else { 0 };
            let wm_row: u16 = if !engine.wildmenu_items.is_empty() {
                1
            } else {
                0
            };
            let content_rows = size
                .height
                .saturating_sub(2 + qf_rows + trm_rows + menu_row + dbg_row + wm_row); // status + cmd + panels (tab bar inside content bounds)
            let gutter_approx = 4u16;
            let sidebar_cols = if sidebar.visible {
                sidebar_width + 1
            } else {
                0
            };
            let ab_w = if engine.settings.autohide_panels && !sidebar.visible {
                0
            } else {
                ACTIVITY_BAR_WIDTH
            };
            let content_cols = size
                .width
                .saturating_sub(ab_w + sidebar_cols + gutter_approx);
            engine.set_viewport_lines(content_rows.saturating_sub(1).max(1) as usize); // -1 for tab bar row inside content_rows
            engine.set_viewport_cols(content_cols.max(1) as usize);
        }

        if needs_redraw && last_draw.elapsed() >= min_frame {
            // Build layout before drawing so mouse handler can use it
            let screen = if let Ok(size) = terminal.size() {
                let area = Rect {
                    x: 0,
                    y: 0,
                    width: size.width,
                    height: size.height,
                };
                let s = build_screen_for_tui(engine, &theme, area, &sidebar, sidebar_width);
                last_layout = Some(s);
                last_layout.as_ref()
            } else {
                last_layout.as_ref()
            };

            // Update per-window viewport dimensions so ensure_cursor_visible uses
            // the actual pane width (critical for horizontal scrolling in vsplit).
            if let Some(ref layout) = last_layout {
                for rw in &layout.windows {
                    let gutter = rw.gutter_char_width as u16;
                    // -1 for the vertical scrollbar column
                    let pane_cols =
                        (rw.rect.width as u16).saturating_sub(gutter + 1).max(1) as usize;
                    let pane_rows = (rw.rect.height as u16).max(1) as usize;
                    engine.set_viewport_for_window(rw.window_id, pane_rows, pane_cols);
                }
            }

            // Compute debug sidebar section heights so ensure_visible and click
            // hit-testing use the same dimensions as the render function.
            if sidebar.visible && sidebar.active_panel == TuiPanel::Debug {
                if let Ok(size) = terminal.size() {
                    // Mirror the draw_frame v_chunks layout to get the exact
                    // sidebar area height: subtract all rows that appear above or
                    // below main_area (menu, quickfix, bottom-panel, debug-toolbar,
                    // status bar, command bar).
                    let menu_h: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                    let qf_h: u16 = if engine.quickfix_open { 6 } else { 0 };
                    let debug_out_open = engine.bottom_panel_kind
                        == render::BottomPanelKind::DebugOutput
                        && !engine.dap_output_lines.is_empty();
                    let bp_h: u16 = if engine.terminal_open || debug_out_open {
                        engine.session.terminal_panel_rows + 2
                    } else {
                        0
                    };
                    let dt_h: u16 = if engine.debug_toolbar_visible { 1 } else { 0 };
                    // 2 fixed rows: status bar + command bar
                    let overhead = menu_h + qf_h + bp_h + dt_h + 2;
                    let sidebar_h = size.height.saturating_sub(overhead) as usize;
                    // 2 overhead rows in sidebar (header + button) + 4 section headers
                    let content_rows = sidebar_h.saturating_sub(6);
                    let base = content_rows / 4;
                    let remainder = content_rows % 4;
                    for i in 0..4 {
                        engine.dap_sidebar_section_heights[i] =
                            (base + if i < remainder { 1 } else { 0 }) as u16;
                    }
                }
            }

            debug_log!(">>> terminal.draw() begin");
            terminal
                .draw(|frame| {
                    if let Some(s) = &screen {
                        let drop_target = explorer_drag_active.as_ref().and_then(|&(_, t)| t);
                        draw_frame(
                            frame,
                            s,
                            &theme,
                            &mut sidebar,
                            engine,
                            &sidebar_prompt,
                            sidebar_width,
                            fuzzy_scroll_top,
                            grep_scroll_top,
                            quickfix_scroll_top,
                            debug_output_scroll,
                            folder_picker.as_ref(),
                            quit_confirm,
                            close_tab_confirm,
                            cmd_sel,
                            drop_target,
                        );
                    }
                })
                .expect("draw frame");
            debug_log!("<<< terminal.draw() done");

            // Set terminal cursor shape to match mode / pending key.
            let cursor_style = if !sidebar.has_focus && engine.pending_key == Some('r') {
                SetCursorStyle::SteadyUnderScore
            } else if !sidebar.has_focus {
                match engine.mode {
                    Mode::Insert => SetCursorStyle::BlinkingBar,
                    _ => SetCursorStyle::SteadyBlock,
                }
            } else {
                SetCursorStyle::SteadyBlock
            };
            let _ = execute!(terminal.backend_mut(), cursor_style);

            // Sync terminal emulator title bar with the active file name.
            let tui_title = engine
                .active_buffer_name()
                .map(|n| format!("VimCode \u{2014} {}", n))
                .unwrap_or_else(|| "VimCode".to_string());
            let _ = execute!(terminal.backend_mut(), SetTitle(tui_title.as_str()));

            last_draw = Instant::now();
            needs_redraw = false;
        }

        // Clear yank highlight after 200 ms deadline.
        if let Some(dl) = yank_hl_deadline {
            if Instant::now() >= dl {
                engine.clear_yank_highlight();
                yank_hl_deadline = None;
                needs_redraw = true;
            }
        }

        // When a redraw is pending but rate-limited, wait only until the next frame is due.
        // When idle, poll slowly to keep CPU near zero.
        // If a yank highlight is active, cap the wait so we clear it on time.
        let poll_timeout = if engine.tab_switcher_open {
            // Short poll when tab switcher is open so we can auto-confirm quickly
            Duration::from_millis(10)
        } else if needs_redraw {
            min_frame
                .saturating_sub(last_draw.elapsed())
                .max(Duration::from_millis(1))
        } else if let Some(dl) = yank_hl_deadline {
            dl.saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1))
        } else {
            Duration::from_millis(50)
        };
        if !ct_event::poll(poll_timeout).expect("poll") {
            // Tab switcher auto-confirm: if open and no Alt+t press for 400ms, confirm.
            if engine.tab_switcher_open {
                if let Some(last) = tab_switcher_last_cycle {
                    if last.elapsed() >= Duration::from_millis(500) {
                        engine.tab_switcher_confirm();
                        tab_switcher_last_cycle = None;
                        needs_redraw = true;
                    }
                }
                continue;
            }
            // No input — good time to do background work without blocking typing.
            // Flush LSP didChange (may block briefly on pipe write for large buffers).
            engine.lsp_flush_changes();
            if engine.poll_lsp() {
                needs_redraw = true;
            }
            // Format-on-save + :wq/:x deferred quit
            if engine.format_save_quit_ready {
                engine.format_save_quit_ready = false;
                engine.cleanup_all_swaps();
                engine.lsp_shutdown();
                save_session(engine);
                break;
            }
            if engine.poll_project_search() && !engine.project_search_results.is_empty() {
                sidebar.search_scroll_top = 0;
                if sidebar.active_panel == TuiPanel::Search {
                    sidebar.search_input_mode = false;
                }
                needs_redraw = true;
            }
            if engine.poll_project_replace() {
                needs_redraw = true;
            }
            // Auto-refresh explorer and SC panel to reflect external filesystem changes.
            if sidebar.visible && last_sidebar_refresh.elapsed() >= Duration::from_secs(2) {
                sidebar.show_hidden_files = engine.settings.show_hidden_files;
                sidebar.build_rows();
                if sidebar.active_panel == TuiPanel::Git {
                    engine.sc_refresh();
                }
                last_sidebar_refresh = Instant::now();
                needs_redraw = true;
            }
            // Auto-reload buffers whose files changed on disk.
            if last_file_check.elapsed() >= Duration::from_secs(2) {
                last_file_check = Instant::now();
                if engine.check_file_changes() {
                    needs_redraw = true;
                }
            }
            // Auto-reload settings.json when its mtime changes (e.g. after :w in the editor).
            {
                let path = crate::core::settings::Settings::settings_file_path();
                if let Ok(meta) = fs::metadata(&path) {
                    if let Ok(mtime) = meta.modified() {
                        let changed = settings_mtime != Some(mtime);
                        if changed {
                            settings_mtime = Some(mtime);
                            if let Ok(new_settings) =
                                crate::core::settings::Settings::load_with_validation()
                            {
                                engine.settings = new_settings;
                                engine.message = "Settings reloaded".to_string();
                                needs_redraw = true;
                            }
                        }
                    }
                }
            }
            // Terminal: drain PTY output and refresh display if new data arrived.
            if engine.poll_terminal() {
                needs_redraw = true;
            }
            // Run pending terminal commands (e.g. extension installs).
            if let Some(cmd) = engine.pending_terminal_command.take() {
                let cols = terminal.size().ok().map(|s| s.width).unwrap_or(80);
                engine.terminal_run_command(&cmd, cols, engine.session.terminal_panel_rows);
                needs_redraw = true;
            }
            // DAP: drain adapter events (breakpoint hits, stops, output)
            if engine.poll_dap() {
                needs_redraw = true;
            }
            // Auto-switch to Debug sidebar when a session starts.
            if engine.dap_wants_sidebar {
                engine.dap_wants_sidebar = false;
                sidebar.active_panel = TuiPanel::Debug;
                sidebar.visible = true;
                needs_redraw = true;
            }
            // Poll for completed extension registry fetch.
            if engine.poll_ext_registry() {
                needs_redraw = true;
            }
            if engine.poll_ai() {
                needs_redraw = true;
            }
            // Poll for completed async shell tasks (plugin background commands).
            if engine.poll_async_shells() {
                needs_redraw = true;
            }
            // Tick AI inline completion debounce counter each event-loop frame.
            if engine.tick_ai_completion() {
                needs_redraw = true;
            }
            // Tick swap file writes (only does work when updatetime elapsed).
            engine.tick_swap_files();
            continue;
        }

        match ct_event::read().expect("read event") {
            Event::Key(key_event) => {
                // ── Quit confirm overlay — intercept all keys ───────────────
                if quit_confirm && key_event.kind != KeyEventKind::Release {
                    match key_event.code {
                        KeyCode::Char('s') | KeyCode::Char('S') => {
                            engine.save_all_dirty();
                            engine.cleanup_all_swaps();
                            engine.lsp_shutdown();
                            save_session(engine);
                            return;
                        }
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            engine.cleanup_all_swaps();
                            engine.lsp_shutdown();
                            save_session(engine);
                            return;
                        }
                        KeyCode::Esc
                        | KeyCode::Char('c')
                        | KeyCode::Char('C')
                        | KeyCode::Char('n')
                        | KeyCode::Char('N') => {
                            quit_confirm = false;
                        }
                        _ => {}
                    }
                    needs_redraw = true;
                    continue;
                }

                // ── Close-tab confirm overlay — intercept all keys ─────────
                if close_tab_confirm && key_event.kind != KeyEventKind::Release {
                    match key_event.code {
                        KeyCode::Char('s') | KeyCode::Char('S') => {
                            engine.escape_to_normal();
                            let _ = engine.save();
                            engine.close_tab();
                            close_tab_confirm = false;
                        }
                        KeyCode::Char('d') | KeyCode::Char('D') => {
                            engine.escape_to_normal();
                            engine.close_tab();
                            close_tab_confirm = false;
                        }
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                            engine.escape_to_normal();
                            close_tab_confirm = false;
                        }
                        _ => {}
                    }
                    needs_redraw = true;
                    continue;
                }

                // ── MRU tab switcher ──────────────────────────────────────
                // When open, intercept all press events. Alt+t / Ctrl+Tab
                // cycle; release events and non-cycling keys are swallowed.
                // Auto-confirm happens via poll timeout (400ms with no input).
                if engine.tab_switcher_open && key_event.kind != KeyEventKind::Release {
                    let alt = key_event.modifiers.contains(KeyModifiers::ALT);
                    let ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);
                    match key_event.code {
                        // Alt+t or Ctrl+Tab or plain Tab: cycle forward
                        KeyCode::Char('t') if alt => {
                            let len = engine.tab_mru.len();
                            if len > 0 {
                                engine.tab_switcher_selected =
                                    (engine.tab_switcher_selected + 1) % len;
                            }
                            tab_switcher_last_cycle = Some(Instant::now());
                        }
                        KeyCode::Tab if ctrl => {
                            let len = engine.tab_mru.len();
                            if len > 0 {
                                engine.tab_switcher_selected =
                                    (engine.tab_switcher_selected + 1) % len;
                            }
                            tab_switcher_last_cycle = Some(Instant::now());
                        }
                        KeyCode::Tab => {
                            let len = engine.tab_mru.len();
                            if len > 0 {
                                engine.tab_switcher_selected =
                                    (engine.tab_switcher_selected + 1) % len;
                            }
                            tab_switcher_last_cycle = Some(Instant::now());
                        }
                        // Shift+Tab or Ctrl+Shift+Tab: cycle backward
                        KeyCode::BackTab => {
                            let len = engine.tab_mru.len();
                            if len > 0 {
                                engine.tab_switcher_selected = if engine.tab_switcher_selected == 0
                                {
                                    len - 1
                                } else {
                                    engine.tab_switcher_selected - 1
                                };
                            }
                            tab_switcher_last_cycle = Some(Instant::now());
                        }
                        KeyCode::Esc => {
                            engine.tab_switcher_open = false;
                            tab_switcher_last_cycle = None;
                        }
                        KeyCode::Enter => {
                            engine.tab_switcher_confirm();
                            tab_switcher_last_cycle = None;
                        }
                        _ => {
                            // Any other press confirms immediately
                            engine.tab_switcher_confirm();
                            tab_switcher_last_cycle = None;
                        }
                    }
                    needs_redraw = true;
                    continue;
                }
                // Swallow release events while tab switcher is open
                if engine.tab_switcher_open && key_event.kind == KeyEventKind::Release {
                    continue;
                }

                // Tab switcher openers (only on Press, not Release)
                if key_event.kind != KeyEventKind::Release {
                    let ctrl_held = key_event.modifiers.contains(KeyModifiers::CONTROL);
                    let alt_held = key_event.modifiers.contains(KeyModifiers::ALT);
                    // Ctrl+Tab
                    if ctrl_held && key_event.code == KeyCode::Tab {
                        engine.open_tab_switcher();
                        tab_switcher_last_cycle = Some(Instant::now());
                        needs_redraw = true;
                        continue;
                    }
                    // Ctrl+Shift+Tab
                    if ctrl_held && key_event.code == KeyCode::BackTab {
                        engine.open_tab_switcher();
                        if engine.tab_switcher_open {
                            let len = engine.tab_mru.len();
                            if len > 0 {
                                engine.tab_switcher_selected = len - 1;
                            }
                        }
                        tab_switcher_last_cycle = Some(Instant::now());
                        needs_redraw = true;
                        continue;
                    }
                    // Alt+t (handled here for the initial open; cycling handled above)
                    if alt_held && !ctrl_held && key_event.code == KeyCode::Char('t') {
                        engine.open_tab_switcher();
                        tab_switcher_last_cycle = Some(Instant::now());
                        needs_redraw = true;
                        continue;
                    }
                }

                // ── Inline rename in explorer ────────────────────────────────
                if engine.explorer_rename.is_some() {
                    if let Some((key_name, unicode, ctrl)) =
                        translate_key(key_event, keyboard_enhanced)
                    {
                        engine.handle_explorer_rename_key(&key_name, unicode, ctrl);
                        if engine.explorer_needs_refresh {
                            sidebar.build_rows();
                            engine.explorer_needs_refresh = false;
                        }
                    }
                    needs_redraw = true;
                    continue;
                }

                // ── Prompt mode (sidebar CRUD) ──────────────────────────────
                if let Some(ref mut prompt) = sidebar_prompt {
                    match key_event.code {
                        KeyCode::Esc => {
                            sidebar_prompt = None;
                        }
                        KeyCode::Enter => {
                            let input = prompt.input.clone();
                            let kind = prompt.kind.clone();
                            sidebar_prompt = None;
                            let vh = terminal
                                .size()
                                .map(|s| s.height.saturating_sub(4) as usize)
                                .unwrap_or(40);
                            handle_sidebar_prompt(engine, &mut sidebar, kind, input, vh);
                        }
                        KeyCode::Backspace => {
                            if prompt.cursor > 0 {
                                // Find the previous char boundary
                                let prev = prompt.input[..prompt.cursor]
                                    .char_indices()
                                    .next_back()
                                    .map(|(i, _)| i)
                                    .unwrap_or(0);
                                prompt.input.remove(prev);
                                prompt.cursor = prev;
                            }
                        }
                        KeyCode::Delete => {
                            if prompt.cursor < prompt.input.len() {
                                prompt.input.remove(prompt.cursor);
                            }
                        }
                        KeyCode::Left => {
                            if prompt.cursor > 0 {
                                prompt.cursor = prompt.input[..prompt.cursor]
                                    .char_indices()
                                    .next_back()
                                    .map(|(i, _)| i)
                                    .unwrap_or(0);
                            }
                        }
                        KeyCode::Right => {
                            if prompt.cursor < prompt.input.len() {
                                let rest = &prompt.input[prompt.cursor..];
                                let next = rest
                                    .char_indices()
                                    .nth(1)
                                    .map(|(i, _)| prompt.cursor + i)
                                    .unwrap_or(prompt.input.len());
                                prompt.cursor = next;
                            }
                        }
                        KeyCode::Home => {
                            prompt.cursor = 0;
                        }
                        KeyCode::End => {
                            prompt.cursor = prompt.input.len();
                        }
                        KeyCode::Char(c)
                            if key_event.kind != KeyEventKind::Release
                                && !key_event.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            // For delete confirm only accept y/n
                            if matches!(prompt.kind, PromptKind::DeleteConfirm(_)) {
                                if c == 'y' || c == 'n' {
                                    let kind = prompt.kind.clone();
                                    sidebar_prompt = None;
                                    if c == 'y' {
                                        let vh = terminal
                                            .size()
                                            .map(|s| s.height.saturating_sub(3) as usize)
                                            .unwrap_or(40);
                                        handle_sidebar_prompt(
                                            engine,
                                            &mut sidebar,
                                            kind,
                                            "y".to_string(),
                                            vh,
                                        );
                                    }
                                }
                            } else {
                                prompt.input.insert(prompt.cursor, c);
                                prompt.cursor += c.len_utf8();
                            }
                        }
                        _ => {}
                    }
                    needs_redraw = true;
                    continue;
                }

                // ── Folder picker modal ─────────────────────────────────────
                if folder_picker.is_some() && key_event.kind != KeyEventKind::Release {
                    let ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);
                    let picker = folder_picker.as_mut().unwrap();
                    match key_event.code {
                        KeyCode::Esc => {
                            folder_picker = None;
                        }
                        KeyCode::Enter => {
                            let mode = picker.mode.clone();
                            if mode == FolderPickerMode::OpenRecent {
                                if let Some(path) = picker.filtered.get(picker.selected).cloned() {
                                    folder_picker = None;
                                    engine.open_folder(&path);
                                    sidebar = TuiSidebar::new(engine.cwd.clone(), sidebar.visible);
                                    sidebar.show_hidden_files = engine.settings.show_hidden_files;
                                }
                            } else {
                                // Check if ".." was selected — navigate up instead of opening
                                let is_dotdot = picker
                                    .filtered
                                    .get(picker.selected)
                                    .map(|p| p.as_os_str() == "..")
                                    .unwrap_or(false);
                                if is_dotdot {
                                    picker.navigate_up();
                                } else if let Some(path) = picker.selected_path() {
                                    folder_picker = None;
                                    match mode {
                                        FolderPickerMode::OpenFolder => {
                                            engine.open_folder(&path);
                                        }
                                        FolderPickerMode::OpenRecent => {}
                                    }
                                    sidebar = TuiSidebar::new(engine.cwd.clone(), sidebar.visible);
                                    sidebar.show_hidden_files = engine.settings.show_hidden_files;
                                }
                            }
                        }
                        // '-' navigates up to the parent directory (like vim netrw)
                        KeyCode::Char('-')
                            if !ctrl && picker.mode != FolderPickerMode::OpenRecent =>
                        {
                            picker.navigate_up();
                        }
                        KeyCode::Up | KeyCode::Char('k') if !ctrl => {
                            picker.move_up();
                        }
                        KeyCode::Down | KeyCode::Char('j') if !ctrl => {
                            picker.move_down();
                        }
                        KeyCode::Backspace => {
                            picker.pop_char();
                        }
                        KeyCode::Char(c) if !ctrl => {
                            picker.push_char(c);
                        }
                        _ => {}
                    }
                    // Keep scroll in sync with selection
                    if let Some(ref mut picker) = folder_picker {
                        if let Ok(size) = terminal.size() {
                            let popup_h = ((size.height as usize) * 55 / 100).max(15);
                            let visible_rows = popup_h.saturating_sub(4);
                            picker.sync_scroll(visible_rows);
                        }
                    }
                    needs_redraw = true;
                    continue;
                }

                // ── Activity bar (toolbar) focused ────────────────────────────
                if sidebar.toolbar_focused
                    && !engine.fuzzy_open
                    && !engine.grep_open
                    && key_event.kind != KeyEventKind::Release
                {
                    match key_event.code {
                        KeyCode::Char('j') | KeyCode::Down => {
                            // Move down: 0→1→…→6→8→9→…→(8+N-1)→7 (settings at end)
                            let ext_count = engine.ext_panels.len() as u16;
                            let max_ext = if ext_count > 0 { 7 + ext_count } else { 0 };
                            let sel = sidebar.toolbar_selected;
                            if sel < 6 {
                                sidebar.toolbar_selected = sel + 1;
                            } else if sel == 6 && ext_count > 0 {
                                sidebar.toolbar_selected = 8; // first ext panel
                            } else if sel == 6 && ext_count == 0 {
                                sidebar.toolbar_selected = 7; // settings
                            } else if sel >= 8 && sel < max_ext {
                                sidebar.toolbar_selected = sel + 1;
                            } else if sel >= 8 && sel == max_ext {
                                sidebar.toolbar_selected = 7; // settings
                            }
                            // sel == 7 (settings) → no movement (bottom)
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            // Move up: 7→max_ext→…→8→6→5→…→0
                            let ext_count = engine.ext_panels.len() as u16;
                            let max_ext = if ext_count > 0 { 7 + ext_count } else { 0 };
                            let sel = sidebar.toolbar_selected;
                            if sel == 7 && ext_count > 0 {
                                sidebar.toolbar_selected = max_ext; // settings → last ext
                            } else if sel == 7 && ext_count == 0 {
                                sidebar.toolbar_selected = 6; // settings → AI
                            } else if sel == 8 {
                                sidebar.toolbar_selected = 6; // first ext → AI
                            } else if sel > 8 {
                                sidebar.toolbar_selected = sel - 1;
                            } else {
                                sidebar.toolbar_selected = sel.saturating_sub(1);
                            }
                        }
                        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                            // Activate the selected panel
                            let panel = match sidebar.toolbar_selected {
                                0 => {
                                    engine.toggle_menu_bar();
                                    sidebar.toolbar_focused = false;
                                    needs_redraw = true;
                                    continue;
                                }
                                1 => TuiPanel::Explorer,
                                2 => TuiPanel::Search,
                                3 => TuiPanel::Debug,
                                4 => TuiPanel::Git,
                                5 => TuiPanel::Extensions,
                                6 => TuiPanel::Ai,
                                7 => TuiPanel::Settings,
                                idx if idx >= 8 => {
                                    // Extension panel activation
                                    let ext_idx = (idx - 8) as usize;
                                    let mut ext_names: Vec<_> =
                                        engine.ext_panels.keys().cloned().collect();
                                    ext_names.sort();
                                    if ext_idx < ext_names.len() {
                                        let name = ext_names[ext_idx].clone();
                                        sidebar.toolbar_focused = false;
                                        sidebar.ext_panel_name = Some(name.clone());
                                        sidebar.visible = true;
                                        sidebar.has_focus = true;
                                        engine.ext_panel_active = Some(name.clone());
                                        engine.ext_panel_has_focus = true;
                                        engine.ext_panel_selected = 0;
                                        engine.session.explorer_visible = true;
                                        let _ = engine.session.save();
                                        engine.plugin_event("panel_focus", &name);
                                    }
                                    needs_redraw = true;
                                    continue;
                                }
                                _ => {
                                    needs_redraw = true;
                                    continue;
                                }
                            };
                            sidebar.toolbar_focused = false;
                            sidebar.ext_panel_name = None;
                            engine.ext_panel_has_focus = false;
                            engine.ext_panel_active = None;
                            sidebar.active_panel = panel;
                            sidebar.visible = true;
                            sidebar.has_focus = true;
                            engine.session.explorer_visible = true;
                            let _ = engine.session.save();
                            if panel == TuiPanel::Search {
                                sidebar.search_input_mode = true;
                                sidebar.replace_input_focused = false;
                            }
                            if panel == TuiPanel::Git {
                                engine.sc_has_focus = true;
                                engine.sc_refresh();
                            }
                            if panel == TuiPanel::Debug {
                                engine.dap_sidebar_has_focus = true;
                            }
                            if panel == TuiPanel::Extensions {
                                engine.ext_sidebar_has_focus = true;
                                if engine.ext_registry.is_none() && !engine.ext_registry_fetching {
                                    engine.ext_refresh();
                                }
                            }
                            if panel == TuiPanel::Ai {
                                engine.ai_has_focus = true;
                            }
                            if panel == TuiPanel::Settings {
                                engine.settings_has_focus = true;
                            }
                        }
                        KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc => {
                            // Leave toolbar, return focus to editor
                            sidebar.toolbar_focused = false;
                        }
                        KeyCode::Char('q') => {
                            // Collapse sidebar from toolbar
                            sidebar.toolbar_focused = false;
                            sidebar.visible = false;
                            sidebar.has_focus = false;
                            engine.session.explorer_visible = false;
                            let _ = engine.session.save();
                        }
                        _ => {}
                    }
                    needs_redraw = true;
                    continue;
                }

                // ── Sidebar focused ─────────────────────────────────────────
                // Note: sidebar key handling is suppressed when fuzzy modal is open.
                if sidebar.has_focus
                    && !engine.fuzzy_open
                    && !engine.grep_open
                    && key_event.kind != KeyEventKind::Release
                {
                    let ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);

                    // ── Panel navigation shortcuts work from within sidebar too ─
                    {
                        let pk = &engine.settings.panel_keys;
                        let mods = key_event.modifiers;
                        let code = key_event.code;
                        if matches_tui_key(&pk.toggle_sidebar, code, mods) {
                            sidebar.visible = false;
                            sidebar.has_focus = false;
                            engine.session.explorer_visible = false;
                            let _ = engine.session.save();
                            needs_redraw = true;
                            continue;
                        }
                        if matches_tui_key(&pk.focus_explorer, code, mods) {
                            if sidebar.active_panel == TuiPanel::Explorer {
                                // Already in explorer — return focus to editor
                                sidebar.has_focus = false;
                            } else {
                                sidebar.active_panel = TuiPanel::Explorer;
                            }
                            needs_redraw = true;
                            continue;
                        }
                        if matches_tui_key(&pk.focus_search, code, mods) {
                            if sidebar.active_panel == TuiPanel::Search {
                                // Already in search — return focus to editor
                                sidebar.has_focus = false;
                            } else {
                                sidebar.active_panel = TuiPanel::Search;
                                sidebar.search_input_mode = true;
                                sidebar.replace_input_focused = false;
                            }
                            needs_redraw = true;
                            continue;
                        }
                        // Ctrl-W prefix: set pending state for window navigation
                        if mods.contains(KeyModifiers::CONTROL)
                            && matches!(code, KeyCode::Char('w') | KeyCode::Char('W'))
                        {
                            sidebar.pending_ctrl_w = true;
                            needs_redraw = true;
                            continue;
                        }
                    }
                    // Ctrl-W {h,l,Left,Right}: navigate between toolbar/panel/editor
                    if sidebar.pending_ctrl_w {
                        sidebar.pending_ctrl_w = false;
                        match key_event.code {
                            KeyCode::Char('h') | KeyCode::Left => {
                                // Panel → toolbar
                                sidebar.has_focus = false;
                                engine.sc_has_focus = false;
                                engine.dap_sidebar_has_focus = false;
                                engine.ext_sidebar_has_focus = false;
                                engine.ai_has_focus = false;
                                engine.settings_has_focus = false;
                                engine.ext_panel_has_focus = false;
                                sidebar.toolbar_focused = true;
                                sidebar.toolbar_selected = match sidebar.active_panel {
                                    TuiPanel::Explorer => 1,
                                    TuiPanel::Search => 2,
                                    TuiPanel::Debug => 3,
                                    TuiPanel::Git => 4,
                                    TuiPanel::Extensions => 5,
                                    TuiPanel::Ai => 6,
                                    TuiPanel::Settings => 7,
                                };
                            }
                            KeyCode::Char('l') | KeyCode::Right => {
                                // Panel → editor
                                sidebar.has_focus = false;
                                engine.sc_has_focus = false;
                                engine.dap_sidebar_has_focus = false;
                                engine.ext_sidebar_has_focus = false;
                                engine.ai_has_focus = false;
                                engine.settings_has_focus = false;
                                engine.ext_panel_has_focus = false;
                            }
                            _ => {} // Unknown Ctrl-W combo in sidebar, ignore
                        }
                        needs_redraw = true;
                        continue;
                    }

                    // ── Search panel keyboard handling ──────────────────────
                    if sidebar.active_panel == TuiPanel::Search {
                        let alt = key_event.modifiers.contains(KeyModifiers::ALT);
                        // Alt+C/W/R/H toggles work in both input and results mode
                        if alt {
                            match key_event.code {
                                KeyCode::Char('c') => {
                                    engine.toggle_project_search_case();
                                    continue;
                                }
                                KeyCode::Char('w') => {
                                    engine.toggle_project_search_whole_word();
                                    continue;
                                }
                                KeyCode::Char('r') => {
                                    engine.toggle_project_search_regex();
                                    continue;
                                }
                                KeyCode::Char('h') => {
                                    let root = sidebar.root.clone();
                                    engine.start_project_replace(root);
                                    continue;
                                }
                                _ => {}
                            }
                        }
                        match key_event.code {
                            KeyCode::Esc => {
                                sidebar.has_focus = false;
                            }
                            KeyCode::Char('b') if ctrl => {
                                sidebar.visible = false;
                                sidebar.has_focus = false;
                                engine.session.explorer_visible = false;
                                let _ = engine.session.save();
                            }
                            // Input mode: typing into the search or replace box
                            _ if sidebar.search_input_mode => match key_event.code {
                                KeyCode::Tab | KeyCode::BackTab => {
                                    sidebar.replace_input_focused = !sidebar.replace_input_focused;
                                }
                                KeyCode::Enter => {
                                    if sidebar.replace_input_focused {
                                        let root = sidebar.root.clone();
                                        engine.start_project_replace(root);
                                    } else {
                                        let root = sidebar.root.clone();
                                        engine.start_project_search(root);
                                        sidebar.search_scroll_top = 0;
                                    }
                                }
                                KeyCode::Backspace => {
                                    if sidebar.replace_input_focused {
                                        engine.project_replace_text.pop();
                                    } else {
                                        engine.project_search_query.pop();
                                    }
                                }
                                KeyCode::Char(c)
                                    if !key_event.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    if sidebar.replace_input_focused {
                                        engine.project_replace_text.push(c);
                                    } else {
                                        engine.project_search_query.push(c);
                                    }
                                }
                                _ => {}
                            },
                            // Results mode: navigating the results list
                            _ => {
                                match key_event.code {
                                    KeyCode::Char('j') | KeyCode::Down => {
                                        engine.project_search_select_next();
                                        if let Ok(size) = terminal.size() {
                                            let rh = size.height.saturating_sub(7) as usize;
                                            ensure_search_selection_visible(
                                                &engine.project_search_results,
                                                engine.project_search_selected,
                                                &mut sidebar.search_scroll_top,
                                                rh,
                                            );
                                        }
                                    }
                                    KeyCode::Char('k') | KeyCode::Up => {
                                        engine.project_search_select_prev();
                                        if let Ok(size) = terminal.size() {
                                            let rh = size.height.saturating_sub(7) as usize;
                                            ensure_search_selection_visible(
                                                &engine.project_search_results,
                                                engine.project_search_selected,
                                                &mut sidebar.search_scroll_top,
                                                rh,
                                            );
                                        }
                                    }
                                    KeyCode::Enter => {
                                        let idx = engine.project_search_selected;
                                        let result = engine
                                            .project_search_results
                                            .get(idx)
                                            .map(|m| (m.file.clone(), m.line));
                                        if let Some((file, line)) = result {
                                            engine.open_file_in_tab(&file);
                                            let win_id = engine.active_window_id();
                                            engine.set_cursor_for_window(win_id, line, 0);
                                            engine.ensure_cursor_visible();
                                            sidebar.has_focus = false;
                                        }
                                    }
                                    // h/Left: switch focus to toolbar
                                    KeyCode::Char('h') | KeyCode::Left => {
                                        sidebar.has_focus = false;
                                        sidebar.toolbar_focused = true;
                                        sidebar.toolbar_selected = 2; // Search row
                                    }
                                    // Any printable char: switch back to input mode
                                    KeyCode::Char(c)
                                        if !key_event.modifiers.contains(KeyModifiers::CONTROL) =>
                                    {
                                        sidebar.search_input_mode = true;
                                        sidebar.replace_input_focused = false;
                                        engine.project_search_query.push(c);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        needs_redraw = true;
                        continue;
                    }

                    // ── Debug panel keyboard handling ──────────────────────
                    if sidebar.active_panel == TuiPanel::Debug {
                        // h/Left: switch focus to toolbar
                        if matches!(key_event.code, KeyCode::Char('h') | KeyCode::Left)
                            && !key_event.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            sidebar.has_focus = false;
                            engine.dap_sidebar_has_focus = false;
                            sidebar.toolbar_focused = true;
                            sidebar.toolbar_selected = 3; // Debug row
                            needs_redraw = true;
                            continue;
                        }
                        // Compute section heights before key handling so
                        // ensure_visible has valid dimensions.
                        if let Ok(size) = terminal.size() {
                            let menu_h: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                            let qf_h: u16 = if engine.quickfix_open { 6 } else { 0 };
                            let debug_out_open = engine.bottom_panel_kind
                                == render::BottomPanelKind::DebugOutput
                                && !engine.dap_output_lines.is_empty();
                            let bp_h: u16 = if engine.terminal_open || debug_out_open {
                                engine.session.terminal_panel_rows + 2
                            } else {
                                0
                            };
                            let dt_h: u16 = if engine.debug_toolbar_visible { 1 } else { 0 };
                            let overhead = menu_h + qf_h + bp_h + dt_h + 2;
                            let sidebar_h = size.height.saturating_sub(overhead) as usize;
                            let content_rows = sidebar_h.saturating_sub(6);
                            let base = content_rows / 4;
                            let remainder = content_rows % 4;
                            for i in 0..4 {
                                engine.dap_sidebar_section_heights[i] =
                                    (base + if i < remainder { 1 } else { 0 }) as u16;
                            }
                        }
                        let key_name = match key_event.code {
                            KeyCode::Down => Some("Down"),
                            KeyCode::Up => Some("Up"),
                            KeyCode::Char('j') => Some("j"),
                            KeyCode::Char('k') => Some("k"),
                            KeyCode::Char('g') => Some("g"),
                            KeyCode::Char('G') => Some("G"),
                            KeyCode::Home => Some("Home"),
                            KeyCode::End => Some("End"),
                            KeyCode::PageDown => Some("PageDown"),
                            KeyCode::PageUp => Some("PageUp"),
                            KeyCode::Tab => Some("Tab"),
                            KeyCode::Enter => Some("Return"),
                            KeyCode::Char(' ') => Some(" "),
                            KeyCode::Char('x') => Some("x"),
                            KeyCode::Char('d') => Some("d"),
                            KeyCode::Char('q') => Some("q"),
                            KeyCode::Esc => Some("Escape"),
                            KeyCode::Char('b') if ctrl => {
                                sidebar.visible = false;
                                sidebar.has_focus = false;
                                engine.session.explorer_visible = false;
                                engine.dap_sidebar_has_focus = false;
                                let _ = engine.session.save();
                                None
                            }
                            _ => None,
                        };
                        if let Some(name) = key_name {
                            engine.handle_debug_sidebar_key(name, ctrl);
                            if !engine.dap_sidebar_has_focus {
                                sidebar.has_focus = false;
                            }
                        }
                        needs_redraw = true;
                        continue;
                    }

                    // ── Extension panel (plugin-provided) keyboard handling ─
                    if engine.ext_panel_has_focus && sidebar.ext_panel_name.is_some() {
                        // h/Left: switch focus to toolbar
                        if matches!(key_event.code, KeyCode::Char('h') | KeyCode::Left)
                            && !key_event.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            sidebar.has_focus = false;
                            engine.ext_panel_has_focus = false;
                            sidebar.toolbar_focused = true;
                            // Find the toolbar row for this ext panel
                            let mut ext_names: Vec<_> = engine.ext_panels.keys().cloned().collect();
                            ext_names.sort();
                            let idx = ext_names
                                .iter()
                                .position(|n| Some(n) == sidebar.ext_panel_name.as_ref())
                                .unwrap_or(0);
                            sidebar.toolbar_selected = 8 + idx as u16;
                            needs_redraw = true;
                            continue;
                        }
                        let (key_name, unicode): (&str, Option<char>) = match key_event.code {
                            KeyCode::Char('j') | KeyCode::Down => ("j", None),
                            KeyCode::Char('k') | KeyCode::Up => ("k", None),
                            KeyCode::Char('g') => ("g", None),
                            KeyCode::Char('G') => ("G", None),
                            KeyCode::Tab => ("Tab", None),
                            KeyCode::Enter => ("Return", None),
                            KeyCode::Char('q') | KeyCode::Esc => ("Escape", None),
                            KeyCode::Char(ch) => ("char", Some(ch)),
                            _ => ("", None),
                        };
                        if !key_name.is_empty() {
                            let ch = if key_name == "char" { unicode } else { None };
                            let name = if key_name == "char" {
                                ch.map(|c| c.to_string()).unwrap_or_default()
                            } else {
                                key_name.to_string()
                            };
                            engine.handle_ext_panel_key(&name, ctrl, ch);
                            if !engine.ext_panel_has_focus {
                                sidebar.has_focus = false;
                                sidebar.ext_panel_name = None;
                            }
                        }
                        needs_redraw = true;
                        continue;
                    }

                    // ── Extensions panel keyboard handling ──────────────────
                    if sidebar.active_panel == TuiPanel::Extensions {
                        // h/Left: switch focus to toolbar
                        if matches!(key_event.code, KeyCode::Char('h') | KeyCode::Left)
                            && !key_event.modifiers.contains(KeyModifiers::CONTROL)
                            && !engine.ext_sidebar_input_active
                        {
                            sidebar.has_focus = false;
                            engine.ext_sidebar_has_focus = false;
                            sidebar.toolbar_focused = true;
                            sidebar.toolbar_selected = 5; // Extensions row
                            needs_redraw = true;
                            continue;
                        }
                        let (key_name, unicode): (&str, Option<char>) = match key_event.code {
                            KeyCode::Char('j') | KeyCode::Down => ("j", None),
                            KeyCode::Char('k') | KeyCode::Up => ("k", None),
                            KeyCode::Tab => ("Tab", None),
                            KeyCode::Enter => ("Return", None),
                            KeyCode::Char('d') => ("d", None),
                            KeyCode::Char('i') => ("i", None),
                            KeyCode::Char('r') => ("r", None),
                            KeyCode::Char('/') => ("/", None),
                            KeyCode::Char('q') | KeyCode::Esc => ("Escape", None),
                            KeyCode::Backspace => ("BackSpace", None),
                            KeyCode::Char(ch) => ("char", Some(ch)),
                            _ => ("", None),
                        };
                        if !key_name.is_empty() {
                            let ch = if key_name == "char" { unicode } else { None };
                            engine.handle_ext_sidebar_key(
                                if key_name == "char" { "" } else { key_name },
                                ctrl,
                                ch,
                            );
                            if !engine.ext_sidebar_has_focus {
                                sidebar.has_focus = false;
                            }
                        }
                        needs_redraw = true;
                        continue;
                    }

                    // ── Settings panel keyboard handling ──────────────────────
                    if sidebar.active_panel == TuiPanel::Settings {
                        // h/Left: switch focus to toolbar (only when not editing/searching)
                        if matches!(key_event.code, KeyCode::Char('h') | KeyCode::Left)
                            && !key_event.modifiers.contains(KeyModifiers::CONTROL)
                            && !engine.settings_input_active
                            && engine.settings_editing.is_none()
                        {
                            // Only if the selected row is not an enum (h/Left cycles enums)
                            let flat = engine.settings_flat_list();
                            let is_enum = if engine.settings_selected < flat.len() {
                                match &flat[engine.settings_selected] {
                                    crate::core::engine::SettingsRow::CoreSetting(idx) => {
                                        matches!(
                                            crate::core::settings::SETTING_DEFS[*idx].setting_type,
                                            crate::core::settings::SettingType::Enum(_)
                                                | crate::core::settings::SettingType::DynamicEnum(
                                                    _
                                                )
                                        )
                                    }
                                    crate::core::engine::SettingsRow::ExtSetting(ext_name, key) => {
                                        engine
                                            .find_ext_setting_def(ext_name, key)
                                            .is_some_and(|d| d.r#type == "enum")
                                    }
                                    _ => false,
                                }
                            } else {
                                false
                            };
                            if !is_enum {
                                sidebar.has_focus = false;
                                engine.settings_has_focus = false;
                                sidebar.toolbar_focused = true;
                                sidebar.toolbar_selected = 7; // Settings row
                                needs_redraw = true;
                                continue;
                            }
                        }
                        // Ctrl-V paste into search input or inline edit
                        if ctrl && key_event.code == KeyCode::Char('v') {
                            if engine.settings_input_active || engine.settings_editing.is_some() {
                                let text = match engine.clipboard_read {
                                    Some(ref cb) => cb().ok(),
                                    None => None,
                                };
                                if let Some(t) = text {
                                    engine.settings_paste(&t);
                                }
                            }
                            needs_redraw = true;
                            continue;
                        }
                        let (key_name, unicode): (&str, Option<char>) = match key_event.code {
                            KeyCode::Char('j') | KeyCode::Down => ("j", None),
                            KeyCode::Char('k') | KeyCode::Up => ("k", None),
                            KeyCode::Tab => ("Tab", None),
                            KeyCode::Enter => ("Return", None),
                            KeyCode::Char(' ') => ("Space", None),
                            KeyCode::Char('l') | KeyCode::Right => ("l", None),
                            KeyCode::Char('h') | KeyCode::Left => ("h", None),
                            KeyCode::Char('/') => ("/", None),
                            KeyCode::Char('q') | KeyCode::Esc => ("Escape", None),
                            KeyCode::Backspace => ("BackSpace", None),
                            KeyCode::Char(ch) => ("char", Some(ch)),
                            _ => ("", None),
                        };
                        if !key_name.is_empty() {
                            let ch = if key_name == "char" { unicode } else { None };
                            engine.handle_settings_key(
                                if key_name == "char" { "" } else { key_name },
                                ctrl,
                                ch,
                            );
                            if !engine.settings_has_focus {
                                sidebar.has_focus = false;
                            }
                            // Keep selected item visible after j/k navigation.
                            let th = terminal.size().map(|s| s.height).unwrap_or(24);
                            let content_h = th.saturating_sub(4) as usize;
                            if content_h > 0 {
                                if engine.settings_selected
                                    >= engine.settings_scroll_top + content_h
                                {
                                    engine.settings_scroll_top =
                                        engine.settings_selected - content_h + 1;
                                } else if engine.settings_selected < engine.settings_scroll_top {
                                    engine.settings_scroll_top = engine.settings_selected;
                                }
                            }
                        }
                        needs_redraw = true;
                        continue;
                    }

                    // ── AI assistant panel keyboard handling ─────────────────
                    if sidebar.active_panel == TuiPanel::Ai {
                        // h/Left: switch focus to toolbar (only when not typing)
                        if matches!(key_event.code, KeyCode::Char('h') | KeyCode::Left)
                            && !key_event.modifiers.contains(KeyModifiers::CONTROL)
                            && !engine.ai_input_active
                        {
                            sidebar.has_focus = false;
                            engine.ai_has_focus = false;
                            sidebar.toolbar_focused = true;
                            sidebar.toolbar_selected = 6; // AI row
                            needs_redraw = true;
                            continue;
                        }
                        // Ctrl-V paste
                        if ctrl && key_event.code == KeyCode::Char('v') {
                            let text = match engine.clipboard_read {
                                Some(ref cb) => cb().ok(),
                                None => None,
                            };
                            if let Some(t) = text {
                                engine.ai_insert_text(&t);
                            }
                            needs_redraw = true;
                            continue;
                        }
                        let (key_name, unicode): (&str, Option<char>) = match key_event.code {
                            KeyCode::Down if !engine.ai_input_active => ("j", None),
                            KeyCode::Up if !engine.ai_input_active => ("k", None),
                            KeyCode::Char('j') if !engine.ai_input_active => ("j", None),
                            KeyCode::Char('k') if !engine.ai_input_active => ("k", None),
                            KeyCode::Char('G') if !engine.ai_input_active => ("G", None),
                            KeyCode::Char('g') if !engine.ai_input_active => ("g", None),
                            KeyCode::Char('i') | KeyCode::Char('a') if !engine.ai_input_active => {
                                ("i", None)
                            }
                            KeyCode::Enter => ("Return", None),
                            KeyCode::Esc => ("Escape", None),
                            KeyCode::Char('q') if !engine.ai_input_active => ("Escape", None),
                            KeyCode::Backspace => ("BackSpace", None),
                            KeyCode::Delete => ("Delete", None),
                            KeyCode::Left => ("Left", None),
                            KeyCode::Right => ("Right", None),
                            KeyCode::Home => ("Home", None),
                            KeyCode::End => ("End", None),
                            KeyCode::Char('c') if ctrl => ("c", None),
                            KeyCode::Char('a') if ctrl => {
                                ("a", None) // Ctrl-A → start of input
                            }
                            KeyCode::Char('e') if ctrl => ("e", None),
                            KeyCode::Char('k') if ctrl => ("k", None),
                            KeyCode::Char(ch) => ("char", Some(ch)),
                            _ => ("", None),
                        };
                        if !key_name.is_empty() {
                            let (mapped, uni) = if key_name == "char" {
                                ("", unicode)
                            } else {
                                (key_name, None)
                            };
                            engine.handle_ai_panel_key(mapped, ctrl, uni);
                            if !engine.ai_has_focus {
                                sidebar.has_focus = false;
                            }
                        }
                        needs_redraw = true;
                        continue;
                    }

                    // ── Source Control panel keyboard handling ──────────────
                    if sidebar.active_panel == TuiPanel::Git {
                        // h/Left: switch focus to toolbar (when not in commit input or button mode)
                        if matches!(key_event.code, KeyCode::Char('h') | KeyCode::Left)
                            && !key_event.modifiers.contains(KeyModifiers::CONTROL)
                            && !engine.sc_commit_input_active
                            && engine.sc_button_focused.is_none()
                        {
                            sidebar.has_focus = false;
                            engine.sc_has_focus = false;
                            sidebar.toolbar_focused = true;
                            sidebar.toolbar_selected = 4; // Git row
                            needs_redraw = true;
                            continue;
                        }
                        if engine.sc_commit_input_active
                            || engine.sc_branch_picker_open
                            || engine.sc_branch_create_mode
                            || engine.sc_help_open
                        {
                            // In input/popup mode, route all keys through.
                            let (key_str, unicode): (&str, Option<char>) = match key_event.code {
                                KeyCode::Enter => ("Return", None),
                                KeyCode::Esc => ("Escape", None),
                                KeyCode::Backspace => ("BackSpace", None),
                                KeyCode::Up => ("Up", None),
                                KeyCode::Down => ("Down", None),
                                KeyCode::Char(ch) => ("char", Some(ch)),
                                _ => ("", None),
                            };
                            if key_str == "char" {
                                engine.handle_sc_key("", ctrl, unicode);
                            } else if !key_str.is_empty() {
                                engine.handle_sc_key(key_str, ctrl, None);
                            }
                        } else {
                            let key_name: Option<&str> = match key_event.code {
                                KeyCode::Char('j') | KeyCode::Down => Some("j"),
                                KeyCode::Char('k') | KeyCode::Up => Some("k"),
                                KeyCode::Char('h') | KeyCode::Left => Some("h"),
                                KeyCode::Char('l') | KeyCode::Right => Some("l"),
                                KeyCode::Char('s') => Some("s"),
                                KeyCode::Char('S') => Some("S"),
                                KeyCode::Char('d') => Some("d"),
                                KeyCode::Char('D') => Some("D"),
                                KeyCode::Char('c') => Some("c"),
                                KeyCode::Char('p') => Some("p"),
                                KeyCode::Char('P') => Some("P"),
                                KeyCode::Char('f') => Some("f"),
                                KeyCode::Char('b') if !ctrl => Some("b"),
                                KeyCode::Char('B') => Some("B"),
                                KeyCode::Char('?') => Some("?"),
                                KeyCode::Tab => Some("Tab"),
                                KeyCode::Enter => Some("Return"),
                                KeyCode::Char('q') | KeyCode::Esc => Some("Escape"),
                                KeyCode::Char('r') => Some("r"),
                                KeyCode::Char('b') if ctrl => {
                                    sidebar.visible = false;
                                    sidebar.has_focus = false;
                                    engine.session.explorer_visible = false;
                                    engine.sc_has_focus = false;
                                    let _ = engine.session.save();
                                    None
                                }
                                _ => None,
                            };
                            if let Some(name) = key_name {
                                engine.handle_sc_key(name, ctrl, None);
                                if !engine.sc_has_focus {
                                    sidebar.has_focus = false;
                                }
                            }
                        }
                        needs_redraw = true;
                        continue;
                    }

                    match key_event.code {
                        // Return focus to editor
                        KeyCode::Esc => {
                            sidebar.has_focus = false;
                        }
                        KeyCode::Char('b') if ctrl => {
                            sidebar.visible = false;
                            sidebar.has_focus = false;
                            engine.session.explorer_visible = false;
                            let _ = engine.session.save();
                        }
                        // Navigate down
                        KeyCode::Char('j') | KeyCode::Down => {
                            if !sidebar.rows.is_empty() {
                                sidebar.selected =
                                    (sidebar.selected + 1).min(sidebar.rows.len() - 1);
                            }
                            if let Ok(size) = terminal.size() {
                                let h = size.height.saturating_sub(4) as usize; // tab + header + status + cmd
                                sidebar.ensure_visible(h);
                            }
                        }
                        // Navigate up
                        KeyCode::Char('k') | KeyCode::Up => {
                            sidebar.selected = sidebar.selected.saturating_sub(1);
                            if let Ok(size) = terminal.size() {
                                let h = size.height.saturating_sub(4) as usize;
                                sidebar.ensure_visible(h);
                            }
                        }
                        // Expand dir / open file
                        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                            let idx = sidebar.selected;
                            if idx < sidebar.rows.len() {
                                if sidebar.rows[idx].is_dir {
                                    sidebar.toggle_dir(idx);
                                } else {
                                    let path = sidebar.rows[idx].path.clone();
                                    engine.open_file_in_tab(&path);
                                    sidebar.has_focus = false;
                                }
                            }
                        }
                        // Collapse dir / go to parent / switch to toolbar
                        KeyCode::Char('h') | KeyCode::Left => {
                            let idx = sidebar.selected;
                            if idx < sidebar.rows.len() {
                                if sidebar.rows[idx].is_dir && sidebar.rows[idx].is_expanded {
                                    // Collapse this dir
                                    sidebar.toggle_dir(idx);
                                } else {
                                    // Move to nearest parent row (lower depth)
                                    let target_depth = sidebar.rows[idx].depth;
                                    if target_depth > 0 {
                                        let parent_idx = sidebar.rows[..idx]
                                            .iter()
                                            .rposition(|r| r.depth < target_depth);
                                        if let Some(pi) = parent_idx {
                                            sidebar.selected = pi;
                                        }
                                    } else {
                                        // At root level — switch focus to toolbar
                                        sidebar.has_focus = false;
                                        sidebar.toolbar_focused = true;
                                        sidebar.toolbar_selected = 1; // Explorer row
                                    }
                                }
                            } else {
                                // Empty explorer — switch to toolbar
                                sidebar.has_focus = false;
                                sidebar.toolbar_focused = true;
                                sidebar.toolbar_selected = 1;
                            }
                        }
                        // Explorer CRUD keys — resolved from settings
                        KeyCode::Char(c) if !ctrl => {
                            if let Some(action) = engine.settings.explorer_keys.resolve(c) {
                                match action {
                                    ExplorerAction::NewFile | ExplorerAction::NewFolder => {
                                        let target_dir = {
                                            let idx = sidebar.selected;
                                            if idx < sidebar.rows.len() {
                                                let p = &sidebar.rows[idx].path;
                                                if p.is_dir() {
                                                    p.clone()
                                                } else {
                                                    p.parent()
                                                        .unwrap_or(&sidebar.root)
                                                        .to_path_buf()
                                                }
                                            } else {
                                                sidebar.root.clone()
                                            }
                                        };
                                        // Pre-fill with target dir relative to root + /
                                        let prefill = target_dir
                                            .strip_prefix(&sidebar.root)
                                            .unwrap_or(&target_dir)
                                            .to_string_lossy()
                                            .to_string();
                                        let prefill = if prefill.is_empty() {
                                            String::new()
                                        } else {
                                            format!("{}/", prefill)
                                        };
                                        let kind = if action == ExplorerAction::NewFile {
                                            PromptKind::NewFile(sidebar.root.clone())
                                        } else {
                                            PromptKind::NewFolder(sidebar.root.clone())
                                        };
                                        let cursor = prefill.len();
                                        sidebar_prompt = Some(SidebarPrompt {
                                            kind,
                                            input: prefill,
                                            cursor,
                                        });
                                    }
                                    ExplorerAction::Delete => {
                                        let idx = sidebar.selected;
                                        if idx < sidebar.rows.len() {
                                            let path = sidebar.rows[idx].path.clone();
                                            sidebar_prompt = Some(SidebarPrompt {
                                                kind: PromptKind::DeleteConfirm(path),
                                                input: String::new(),
                                                cursor: 0,
                                            });
                                        }
                                    }
                                    ExplorerAction::Rename => {
                                        let idx = sidebar.selected;
                                        if idx < sidebar.rows.len() {
                                            let path = sidebar.rows[idx].path.clone();
                                            engine.start_explorer_rename(path);
                                        }
                                    }
                                    ExplorerAction::MoveFile => {
                                        let idx = sidebar.selected;
                                        if idx < sidebar.rows.len() {
                                            let path = sidebar.rows[idx].path.clone();
                                            // Pre-fill with full relative path from root
                                            let prefill = path
                                                .strip_prefix(&sidebar.root)
                                                .unwrap_or(&path)
                                                .to_string_lossy()
                                                .to_string();
                                            let cursor = prefill.len();
                                            sidebar_prompt = Some(SidebarPrompt {
                                                kind: PromptKind::MoveFile(path),
                                                input: prefill,
                                                cursor,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    needs_redraw = true;
                    continue;
                }

                // ── Editor focused ──────────────────────────────────────────
                if let Some((key_name, unicode, ctrl)) = translate_key(key_event, keyboard_enhanced)
                {
                    // Panel navigation — all driven by panel_keys settings
                    if key_event.kind != KeyEventKind::Release {
                        let pk = &engine.settings.panel_keys;
                        let mods = key_event.modifiers;
                        let code = key_event.code;

                        // Ctrl+T: toggle terminal (checked first, works even when terminal focused)
                        if matches_tui_key(&pk.open_terminal, code, mods) {
                            if engine.terminal_open && engine.terminal_has_focus {
                                engine.close_terminal();
                            } else if engine.terminal_open {
                                engine.terminal_has_focus = true;
                            } else {
                                // Open terminal at full terminal width; create first tab if needed
                                let cols = terminal.size().ok().map(|s| s.width).unwrap_or(80);
                                if engine.terminal_panes.is_empty() {
                                    engine
                                        .terminal_new_tab(cols, engine.session.terminal_panel_rows);
                                } else {
                                    engine.open_terminal(cols, engine.session.terminal_panel_rows);
                                }
                            }
                            needs_redraw = true;
                            continue;
                        }

                        // When terminal has focus, route all keys to PTY
                        if engine.terminal_has_focus {
                            // Alt+1–9: switch terminal tab.
                            if mods.contains(KeyModifiers::ALT) && !ctrl {
                                if let KeyCode::Char(ch) = code {
                                    if ch.is_ascii_digit() && ch != '0' {
                                        engine.terminal_switch_tab((ch as u8 - b'1') as usize);
                                        needs_redraw = true;
                                        continue;
                                    }
                                }
                            }
                            // PageUp/PageDown scroll through scrollback instead of going to PTY.
                            if matches!(code, KeyCode::PageUp) {
                                engine.terminal_scroll_up(12);
                                needs_redraw = true;
                                continue;
                            }
                            if matches!(code, KeyCode::PageDown) {
                                engine.terminal_scroll_down(12);
                                needs_redraw = true;
                                continue;
                            }
                            // Ctrl+Y: copy terminal selection to clipboard.
                            if ctrl && matches!(code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                                let text = engine.active_terminal().and_then(|t| t.selected_text());
                                if let Some(ref text) = text {
                                    if let Some(ref cb) = engine.clipboard_write {
                                        let _ = cb(text);
                                    }
                                    engine.message = "Copied".to_string();
                                }
                                needs_redraw = true;
                                continue;
                            }
                            // Ctrl+Shift+V (crossterm: Ctrl+uppercase-V): paste clipboard to PTY.
                            if ctrl && matches!(code, KeyCode::Char('V')) {
                                if let Some(ref cb) = engine.clipboard_read {
                                    if let Ok(text) = cb() {
                                        engine.terminal_write(text.as_bytes());
                                    }
                                }
                                needs_redraw = true;
                                continue;
                            }
                            // Ctrl+F: toggle terminal inline find bar.
                            if ctrl && matches!(code, KeyCode::Char('f') | KeyCode::Char('F')) {
                                if engine.terminal_find_active {
                                    engine.terminal_find_close();
                                } else {
                                    engine.terminal_find_open();
                                }
                                needs_redraw = true;
                                continue;
                            }
                            // Terminal find bar key routing (all other keys go here when active).
                            if engine.terminal_find_active {
                                match code {
                                    KeyCode::Esc => engine.terminal_find_close(),
                                    KeyCode::Enter if mods.contains(KeyModifiers::SHIFT) => {
                                        engine.terminal_find_prev()
                                    }
                                    KeyCode::Enter => engine.terminal_find_next(),
                                    KeyCode::Backspace => engine.terminal_find_backspace(),
                                    KeyCode::Char(ch) if !ctrl => engine.terminal_find_char(ch),
                                    _ => {}
                                }
                                needs_redraw = true;
                                continue;
                            }
                            // Ctrl-W in split mode: switch focus between panes.
                            if ctrl
                                && engine.terminal_split
                                && matches!(code, KeyCode::Char('w') | KeyCode::Char('W'))
                            {
                                engine.terminal_split_switch_focus();
                                needs_redraw = true;
                                continue;
                            }
                            // Any other key resets scroll (returns to live view) and forwards.
                            engine.terminal_scroll_reset();
                            let data = translate_key_to_pty(key_event);
                            if !data.is_empty() {
                                engine.terminal_write(&data);
                                needs_redraw = true;
                            }
                            continue;
                        }

                        if matches_tui_key(&pk.toggle_sidebar, code, mods) {
                            sidebar.visible = !sidebar.visible;
                            if !sidebar.visible {
                                sidebar.has_focus = false;
                            }
                            engine.session.explorer_visible = sidebar.visible;
                            let _ = engine.session.save();
                            needs_redraw = true;
                            continue;
                        }

                        if matches_tui_key(&pk.focus_explorer, code, mods) {
                            if sidebar.has_focus && sidebar.active_panel == TuiPanel::Explorer {
                                // Already in explorer — return focus to editor
                                sidebar.has_focus = false;
                            } else {
                                sidebar.visible = true;
                                sidebar.active_panel = TuiPanel::Explorer;
                                sidebar.has_focus = true;
                            }
                            needs_redraw = true;
                            continue;
                        }

                        if matches_tui_key(&pk.focus_search, code, mods) {
                            if sidebar.has_focus && sidebar.active_panel == TuiPanel::Search {
                                sidebar.has_focus = false;
                            } else {
                                sidebar.visible = true;
                                sidebar.active_panel = TuiPanel::Search;
                                sidebar.has_focus = true;
                                sidebar.search_input_mode = true;
                                sidebar.replace_input_focused = false;
                            }
                            needs_redraw = true;
                            continue;
                        }

                        if matches_tui_key(&pk.fuzzy_finder, code, mods) {
                            engine.open_fuzzy_finder();
                            needs_redraw = true;
                            continue;
                        }

                        if matches_tui_key(&pk.live_grep, code, mods) {
                            engine.open_live_grep();
                            needs_redraw = true;
                            continue;
                        }

                        if matches_tui_key(&pk.add_cursor, code, mods) {
                            engine.add_cursor_at_next_match();
                            needs_redraw = true;
                            continue;
                        }
                        if matches_tui_key(&pk.select_all_matches, code, mods) {
                            if engine.is_vscode_mode() {
                                engine.vscode_select_all_occurrences();
                            } else {
                                engine.select_all_word_occurrences();
                            }
                            needs_redraw = true;
                            continue;
                        }
                        if !pk.split_editor_right.is_empty()
                            && matches_tui_key(&pk.split_editor_right, code, mods)
                        {
                            debug_log!(
                                "split_editor_right keybinding: groups={} active={:?}",
                                engine.group_layout.leaf_count(),
                                engine.active_group
                            );
                            engine.open_editor_group(SplitDirection::Vertical);
                            debug_log!(
                                "  after split: groups={} layout={:?}",
                                engine.group_layout.leaf_count(),
                                engine.group_layout
                            );
                            needs_redraw = true;
                            continue;
                        }

                        if !pk.split_editor_down.is_empty()
                            && matches_tui_key(&pk.split_editor_down, code, mods)
                        {
                            debug_log!(
                                "split_editor_down keybinding: groups={} active={:?}",
                                engine.group_layout.leaf_count(),
                                engine.active_group
                            );
                            engine.open_editor_group(SplitDirection::Horizontal);
                            debug_log!(
                                "  after split: groups={} layout={:?}",
                                engine.group_layout.leaf_count(),
                                engine.group_layout
                            );
                            needs_redraw = true;
                            continue;
                        }
                    }

                    // Escape when menu dropdown is open: close it
                    if matches!(key_event.code, KeyCode::Esc)
                        && key_event.kind != KeyEventKind::Release
                        && engine.menu_open_idx.is_some()
                    {
                        engine.close_menu();
                        needs_redraw = true;
                        continue;
                    }

                    // When menu dropdown is open: Left/Right/Up/Down/Enter navigate menus
                    if engine.menu_open_idx.is_some() && key_event.kind != KeyEventKind::Release {
                        match key_event.code {
                            KeyCode::Left => {
                                if let Some(idx) = engine.menu_open_idx {
                                    if idx > 0 {
                                        engine.open_menu(idx - 1);
                                    } else {
                                        engine.open_menu(render::MENU_STRUCTURE.len() - 1);
                                    }
                                }
                                needs_redraw = true;
                                continue;
                            }
                            KeyCode::Right => {
                                if let Some(idx) = engine.menu_open_idx {
                                    engine.open_menu((idx + 1) % render::MENU_STRUCTURE.len());
                                }
                                needs_redraw = true;
                                continue;
                            }
                            KeyCode::Down => {
                                if let Some(open_idx) = engine.menu_open_idx {
                                    if let Some((_, _, items)) =
                                        render::MENU_STRUCTURE.get(open_idx)
                                    {
                                        let seps: Vec<bool> =
                                            items.iter().map(|i| i.separator).collect();
                                        engine.menu_move_selection(1, &seps);
                                    }
                                }
                                needs_redraw = true;
                                continue;
                            }
                            KeyCode::Up => {
                                if let Some(open_idx) = engine.menu_open_idx {
                                    if let Some((_, _, items)) =
                                        render::MENU_STRUCTURE.get(open_idx)
                                    {
                                        let seps: Vec<bool> =
                                            items.iter().map(|i| i.separator).collect();
                                        engine.menu_move_selection(-1, &seps);
                                    }
                                }
                                needs_redraw = true;
                                continue;
                            }
                            KeyCode::Enter => {
                                if let Some((menu_idx, item_idx)) =
                                    engine.menu_activate_highlighted()
                                {
                                    if let Some((_, _, items)) =
                                        render::MENU_STRUCTURE.get(menu_idx)
                                    {
                                        if let Some(item) = items.get(item_idx) {
                                            let action = item.action.to_string();
                                            if action == "open_file_dialog" {
                                                engine.close_menu();
                                                engine.open_fuzzy_finder();
                                            } else {
                                                let act = engine.menu_activate_item(
                                                    menu_idx, item_idx, &action,
                                                );
                                                if act == EngineAction::OpenTerminal {
                                                    let cols = terminal
                                                        .size()
                                                        .ok()
                                                        .map(|s| s.width)
                                                        .unwrap_or(80);
                                                    engine.terminal_new_tab(
                                                        cols,
                                                        engine.session.terminal_panel_rows,
                                                    );
                                                } else if let EngineAction::RunInTerminal(cmd) = act
                                                {
                                                    let cols = terminal
                                                        .size()
                                                        .ok()
                                                        .map(|s| s.width)
                                                        .unwrap_or(80);
                                                    engine.terminal_run_command(
                                                        &cmd,
                                                        cols,
                                                        engine.session.terminal_panel_rows,
                                                    );
                                                } else if act == EngineAction::OpenFolderDialog {
                                                    folder_picker = Some(FolderPickerState::new(
                                                        &engine.cwd.clone(),
                                                        FolderPickerMode::OpenFolder,
                                                        engine.settings.show_hidden_files,
                                                    ));
                                                } else if act == EngineAction::OpenWorkspaceDialog {
                                                    // open_workspace_from_file() already ran;
                                                    // refresh sidebar.
                                                    sidebar = TuiSidebar::new(
                                                        engine.cwd.clone(),
                                                        sidebar.visible,
                                                    );
                                                    sidebar.show_hidden_files =
                                                        engine.settings.show_hidden_files;
                                                } else if act == EngineAction::SaveWorkspaceAsDialog
                                                {
                                                    let ws_path =
                                                        engine.cwd.join(".vimcode-workspace");
                                                    engine.save_workspace_as(&ws_path);
                                                } else if act == EngineAction::OpenRecentDialog {
                                                    folder_picker =
                                                        Some(FolderPickerState::new_recent(
                                                            &engine.session.recent_workspaces,
                                                        ));
                                                } else if act == EngineAction::QuitWithUnsaved {
                                                    quit_confirm = true;
                                                } else if act == EngineAction::ToggleSidebar {
                                                    sidebar.visible = !sidebar.visible;
                                                } else if handle_action(engine, act) {
                                                    return;
                                                }
                                            } // close else { (open_file_dialog branch)
                                        }
                                    }
                                }
                                needs_redraw = true;
                                continue;
                            }
                            _ => {}
                        }
                    }

                    // Alt+letter: open menu (only when menu bar visible)
                    if key_event.modifiers.contains(KeyModifiers::ALT)
                        && key_event.kind != KeyEventKind::Release
                        && engine.menu_bar_visible
                    {
                        if let KeyCode::Char(ch) = key_event.code {
                            let ch_lower = ch.to_ascii_lowercase();
                            let menu_idx = render::MENU_STRUCTURE
                                .iter()
                                .position(|(_, alt_key, _)| *alt_key == ch_lower);
                            if let Some(idx) = menu_idx {
                                if engine.menu_open_idx == Some(idx) {
                                    engine.close_menu();
                                } else {
                                    engine.open_menu(idx);
                                }
                                needs_redraw = true;
                                continue;
                            }
                        }
                    }

                    // Alt+Left/Right: resize sidebar
                    if key_event.modifiers.contains(KeyModifiers::ALT)
                        && key_event.kind != KeyEventKind::Release
                    {
                        match key_event.code {
                            KeyCode::Left => {
                                sidebar_width = sidebar_width.saturating_sub(1).max(15);
                                needs_redraw = true;
                                continue;
                            }
                            KeyCode::Right => {
                                sidebar_width = (sidebar_width + 1).min(150);
                                needs_redraw = true;
                                continue;
                            }
                            // Shift+Alt+F: LSP format document
                            KeyCode::Char('F') => {
                                if key_event.modifiers.contains(KeyModifiers::SHIFT) {
                                    engine.lsp_format_current();
                                    needs_redraw = true;
                                    continue;
                                }
                            }
                            // Alt-M: toggle Vim ↔ VSCode editing mode
                            KeyCode::Char('m') | KeyCode::Char('M') => {
                                engine.toggle_editor_mode();
                                needs_redraw = true;
                                continue;
                            }
                            // Alt+, / Alt+. — resize editor group split
                            KeyCode::Char(',') => {
                                engine.group_resize(-0.05);
                                needs_redraw = true;
                                continue;
                            }
                            KeyCode::Char('.') => {
                                engine.group_resize(0.05);
                                needs_redraw = true;
                                continue;
                            }
                            // Alt+] / Alt+[ — cycle AI ghost text alternatives
                            KeyCode::Char(']') => {
                                if engine.mode == crate::core::Mode::Insert {
                                    engine.ai_ghost_next_alt();
                                    needs_redraw = true;
                                    continue;
                                }
                            }
                            KeyCode::Char('[') => {
                                if engine.mode == crate::core::Mode::Insert {
                                    engine.ai_ghost_prev_alt();
                                    needs_redraw = true;
                                    continue;
                                }
                            }
                            // Alt+t is handled earlier (tab switcher)
                            _ => {}
                        }
                        // VSCode mode: encode Alt+key into key_name for engine dispatch
                        if engine.is_vscode_mode()
                            && key_event.modifiers.contains(KeyModifiers::ALT)
                        {
                            let shift = key_event.modifiers.contains(KeyModifiers::SHIFT);
                            let alt_key_name = match key_event.code {
                                KeyCode::Up if shift => Some("Alt_Shift_Up"),
                                KeyCode::Down if shift => Some("Alt_Shift_Down"),
                                KeyCode::Up => Some("Alt_Up"),
                                KeyCode::Down => Some("Alt_Down"),
                                KeyCode::Char('z') | KeyCode::Char('Z') if !shift => Some("Alt_z"),
                                _ => None,
                            };
                            if let Some(name) = alt_key_name {
                                engine.handle_key(name, None, false);
                                needs_redraw = true;
                                continue;
                            }
                        }
                    }

                    // In VSCode mode, Ctrl-V pre-loads clipboard into register '+' before
                    // calling handle_key (which dispatches to vscode_paste()).
                    if ctrl && key_name == "v" && engine.is_vscode_mode() {
                        if let Some(ref cb_read) = engine.clipboard_read {
                            if let Ok(text) = cb_read() {
                                engine.registers.insert('+', (text.clone(), false));
                                engine.registers.insert('"', (text, false));
                            }
                        }
                        // Fall through to handle_key which calls vscode_paste().
                    }

                    // Ctrl+Shift+V: paste system clipboard into editor buffer.
                    // With keyboard enhancement, this event is captured by the app
                    // instead of the terminal emulator.  In Vim mode, load clipboard
                    // into registers and trigger paste; in insert mode, insert text.
                    if ctrl && key_name == "V" && !engine.is_vscode_mode() {
                        use crate::core::Mode;
                        if let Some(ref cb_read) = engine.clipboard_read {
                            if let Ok(text) = cb_read() {
                                if !text.is_empty() {
                                    engine.load_clipboard_for_paste(text);
                                    match engine.mode {
                                        Mode::Normal => {
                                            engine.handle_key("", Some('p'), false);
                                        }
                                        Mode::Visual | Mode::VisualLine | Mode::VisualBlock => {
                                            engine.handle_key("", Some('p'), false);
                                        }
                                        Mode::Insert | Mode::Replace => {
                                            // Insert clipboard text at cursor
                                            if let Some((content, _)) =
                                                engine.get_register_content('"')
                                            {
                                                let mut changed = false;
                                                for ch in content.chars() {
                                                    engine.handle_key(
                                                        &ch.to_string(),
                                                        Some(ch),
                                                        false,
                                                    );
                                                    changed = true;
                                                }
                                                if changed {
                                                    needs_redraw = true;
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        needs_redraw = true;
                        continue;
                    }

                    // Shift+F5 → stop, Shift+F11 → stepout (debug shortcuts)
                    if key_event.modifiers.contains(KeyModifiers::SHIFT)
                        && key_event.kind != KeyEventKind::Release
                    {
                        match key_event.code {
                            KeyCode::F(5) => {
                                let _ = engine.execute_command("stop");
                                needs_redraw = true;
                                continue;
                            }
                            KeyCode::F(11) => {
                                let _ = engine.execute_command("stepout");
                                needs_redraw = true;
                                continue;
                            }
                            _ => {}
                        }
                    }

                    // ── Command-line selection: Ctrl-C copies, any other key clears ──
                    {
                        use crate::core::Mode;
                        if matches!(engine.mode, Mode::Command | Mode::Search) {
                            if ctrl && matches!(unicode, Some('c') | Some('C')) {
                                // Copy selected command-line text to clipboard
                                if let Some((start, end)) = cmd_sel {
                                    let lo = start.min(end);
                                    let hi = start.max(end);
                                    // lo/hi are column indices on screen (col 0 = ':' prefix, col 1+ = buffer chars)
                                    let buf_lo = lo.saturating_sub(1);
                                    let buf_hi = hi.saturating_sub(1);
                                    let text: String = engine
                                        .command_buffer
                                        .chars()
                                        .enumerate()
                                        .filter(|(i, _)| *i >= buf_lo && *i <= buf_hi)
                                        .map(|(_, c)| c)
                                        .collect();
                                    if !text.is_empty() {
                                        if let Some(ref cb) = engine.clipboard_write {
                                            let _ = cb(&text);
                                        }
                                        engine.message = "Copied".to_string();
                                    }
                                }
                                cmd_sel = None;
                                needs_redraw = true;
                                continue;
                            }
                            // Any other key clears the selection
                            cmd_sel = None;
                        }
                    }

                    // clipboard=unnamedplus: intercept p/P to read from system clipboard.
                    // TUI translate_key() sets key_name="" for regular chars; check unicode.
                    let paste_intercepted = !ctrl
                        && matches!(unicode, Some('p') | Some('P'))
                        && intercept_paste_key(engine, unicode == Some('P'));

                    // ── Context menu keyboard intercept (TUI-side) ──────────
                    // Handle here so explorer actions (new_file etc.) can set
                    // sidebar_prompt, which the engine doesn't know about.
                    if engine.context_menu.is_some() {
                        let effective_key = if key_name.is_empty() {
                            unicode.map(|c| c.to_string()).unwrap_or_default()
                        } else {
                            key_name.clone()
                        };
                        let (consumed, action) = engine.handle_context_menu_key(&effective_key);
                        if consumed {
                            if let Some(act) = action {
                                handle_explorer_context_action(
                                    &act,
                                    engine,
                                    &sidebar,
                                    &mut sidebar_prompt,
                                    terminal.size().ok(),
                                );
                            }
                            needs_redraw = true;
                            continue;
                        }
                    }

                    let prev_tab = engine.active_group().active_tab;
                    if !paste_intercepted {
                        debug_log!(
                            "handle_key: key_name={:?} unicode={:?} ctrl={} groups={} active_group={:?}",
                            key_name,
                            unicode,
                            ctrl,
                            engine.group_layout.leaf_count(),
                            engine.active_group
                        );
                        let action = engine.handle_key(&key_name, unicode, ctrl);
                        // After any key in insert mode, reset AI completion timer.
                        if engine.mode == crate::core::Mode::Insert
                            && engine.settings.ai_completions
                        {
                            engine.ai_completion_reset_timer();
                        }
                        debug_log!(
                            "handle_key result: action={:?} groups_after={}",
                            action,
                            engine.group_layout.leaf_count()
                        );
                        // Handle OpenTerminal specially (needs terminal size info)
                        if action == EngineAction::OpenTerminal {
                            let cols = terminal.size().ok().map(|s| s.width).unwrap_or(80);
                            engine.terminal_new_tab(cols, engine.session.terminal_panel_rows);
                            needs_redraw = true;
                        } else if let EngineAction::RunInTerminal(cmd) = action {
                            let cols = terminal.size().ok().map(|s| s.width).unwrap_or(80);
                            engine.terminal_run_command(
                                &cmd,
                                cols,
                                engine.session.terminal_panel_rows,
                            );
                            needs_redraw = true;
                        } else if action == EngineAction::OpenFolderDialog {
                            folder_picker = Some(FolderPickerState::new(
                                &engine.cwd.clone(),
                                FolderPickerMode::OpenFolder,
                                engine.settings.show_hidden_files,
                            ));
                            needs_redraw = true;
                        } else if action == EngineAction::OpenWorkspaceDialog {
                            // open_workspace_from_file() already ran in the engine;
                            // just refresh the sidebar to reflect the new cwd.
                            sidebar = TuiSidebar::new(engine.cwd.clone(), sidebar.visible);
                            sidebar.show_hidden_files = engine.settings.show_hidden_files;
                            needs_redraw = true;
                        } else if action == EngineAction::SaveWorkspaceAsDialog {
                            // For TUI, save workspace to current directory immediately
                            let ws_path = engine.cwd.join(".vimcode-workspace");
                            engine.save_workspace_as(&ws_path);
                            needs_redraw = true;
                        } else if action == EngineAction::QuitWithUnsaved {
                            quit_confirm = true;
                            needs_redraw = true;
                        } else if action == EngineAction::ToggleSidebar {
                            sidebar.visible = !sidebar.visible;
                            needs_redraw = true;
                        } else if handle_action(engine, action) {
                            break;
                        }
                    }
                    // Ctrl-W h/l overflow: move focus to sidebar/toolbar
                    if let Some(direction) = engine.window_nav_overflow.take() {
                        if !direction {
                            // Left overflow (Ctrl-W h): show sidebar if autohide
                            if !sidebar.visible && engine.settings.autohide_panels {
                                sidebar.visible = true;
                            }
                            // Left overflow → sidebar panel (if visible) or toolbar
                            if sidebar.visible {
                                sidebar.has_focus = true;
                                match sidebar.active_panel {
                                    TuiPanel::Git => engine.sc_has_focus = true,
                                    TuiPanel::Debug => engine.dap_sidebar_has_focus = true,
                                    TuiPanel::Extensions => {
                                        engine.ext_sidebar_has_focus = true;
                                    }
                                    TuiPanel::Ai => engine.ai_has_focus = true,
                                    TuiPanel::Settings => {
                                        engine.settings_has_focus = true;
                                    }
                                    _ => {}
                                }
                            } else {
                                sidebar.toolbar_focused = true;
                            }
                        }
                        // Right overflow: no action (nothing to the right of editor)
                    }

                    // Auto-hide sidebar when focus returns to editor
                    if engine.settings.autohide_panels
                        && sidebar.visible
                        && !sidebar.has_focus
                        && !sidebar.toolbar_focused
                    {
                        sidebar.visible = false;
                    }

                    // Any keypress warrants a redraw (e.g. :set wrap returns None but
                    // must still trigger a re-render to show the new wrapping).
                    needs_redraw = true;
                    loop {
                        let (has_more, action) = engine.advance_macro_playback();
                        if handle_action(engine, action) {
                            return;
                        }
                        if !has_more {
                            break;
                        }
                    }
                    // Sync unnamed register → system clipboard (clipboard=unnamedplus).
                    sync_tui_clipboard(engine, &mut last_clipboard_content);
                    // Rebuild explorer tree if a file move just completed.
                    if engine.explorer_needs_refresh {
                        engine.explorer_needs_refresh = false;
                        sidebar.build_rows();
                    }
                    // Schedule yank highlight clear after 200 ms.
                    if engine.yank_highlight.is_some() {
                        yank_hl_deadline = Some(Instant::now() + Duration::from_millis(200));
                        needs_redraw = true;
                    }
                    // Reveal the active file in the sidebar when the tab changed
                    if engine.active_group().active_tab != prev_tab {
                        if let Some(path) = engine.file_path().cloned() {
                            let h = terminal
                                .size()
                                .map(|s| s.height.saturating_sub(4) as usize)
                                .unwrap_or(40);
                            sidebar.reveal_path(&path, h);
                        }
                    }
                    // Adjust fuzzy scroll to keep selected item visible
                    if engine.fuzzy_open {
                        if let Ok(size) = terminal.size() {
                            let popup_h = ((size.height as usize) * 55 / 100).max(15);
                            let visible_rows = popup_h.saturating_sub(4); // title+query+sep+border
                            if engine.fuzzy_selected < fuzzy_scroll_top {
                                fuzzy_scroll_top = engine.fuzzy_selected;
                            }
                            if engine.fuzzy_selected >= fuzzy_scroll_top + visible_rows {
                                fuzzy_scroll_top = engine.fuzzy_selected + 1 - visible_rows;
                            }
                        }
                    } else {
                        fuzzy_scroll_top = 0;
                    }
                    // Adjust grep scroll to keep selected item visible
                    if engine.grep_open {
                        if let Ok(size) = terminal.size() {
                            let popup_h = ((size.height as usize) * 65 / 100).max(18);
                            let visible_rows = popup_h.saturating_sub(4); // title+query+sep+border
                            if engine.grep_selected < grep_scroll_top {
                                grep_scroll_top = engine.grep_selected;
                            }
                            if engine.grep_selected >= grep_scroll_top + visible_rows {
                                grep_scroll_top = engine.grep_selected + 1 - visible_rows;
                            }
                        }
                    } else {
                        grep_scroll_top = 0;
                    }
                    // Adjust quickfix scroll to keep selected item visible
                    if engine.quickfix_open {
                        const QF_VISIBLE: usize = 5; // 6 rows - 1 header
                        if engine.quickfix_selected < quickfix_scroll_top {
                            quickfix_scroll_top = engine.quickfix_selected;
                        } else if engine.quickfix_selected >= quickfix_scroll_top + QF_VISIBLE {
                            quickfix_scroll_top = engine.quickfix_selected + 1 - QF_VISIBLE;
                        }
                    } else {
                        quickfix_scroll_top = 0;
                    }
                    // Sync palette scroll_top back to engine from TUI scroll state
                    if engine.palette_open {
                        if let Ok(size) = terminal.size() {
                            let popup_h = ((size.height as usize) * 60 / 100).max(16);
                            let visible_rows = popup_h.saturating_sub(4); // title+query+sep+border
                            if engine.palette_selected < engine.palette_scroll_top {
                                engine.palette_scroll_top = engine.palette_selected;
                            }
                            if engine.palette_selected >= engine.palette_scroll_top + visible_rows {
                                engine.palette_scroll_top =
                                    engine.palette_selected + 1 - visible_rows;
                            }
                        }
                    } else {
                        engine.palette_scroll_top = 0;
                    }
                }
            }
            Event::Mouse(mut mouse_event) => {
                // Coalesce consecutive drag events to avoid render-per-pixel lag
                if matches!(mouse_event.kind, MouseEventKind::Drag(_)) {
                    while ct_event::poll(Duration::ZERO).unwrap_or(false) {
                        if let Ok(Event::Mouse(next)) = ct_event::read() {
                            if matches!(next.kind, MouseEventKind::Drag(_)) {
                                mouse_event = next; // skip intermediate positions
                                continue;
                            }
                            // Non-drag event: handle the coalesced drag first, then the new event
                            let mut mouse_should_quit = false;
                            sidebar_width = handle_mouse(
                                mouse_event,
                                &mut sidebar,
                                engine,
                                &terminal.size().ok(),
                                sidebar_width,
                                &mut dragging_sidebar,
                                &mut dragging_scrollbar,
                                &mut dragging_sidebar_search,
                                &mut dragging_debug_sb,
                                &mut dragging_terminal_sb,
                                &mut debug_output_scroll,
                                &mut dragging_debug_output_sb,
                                &mut dragging_terminal_resize,
                                &mut dragging_terminal_split,
                                &mut dragging_group_divider,
                                &mut dragging_settings_sb,
                                last_layout.as_ref(),
                                &mut sidebar_prompt,
                                &mut last_click_time,
                                &mut last_click_pos,
                                &mut mouse_text_drag,
                                &mut folder_picker,
                                &mut quit_confirm,
                                &mut close_tab_confirm,
                                &mut cmd_sel,
                                &mut cmd_dragging,
                                &mut mouse_should_quit,
                                &mut explorer_drag_src,
                                &mut explorer_drag_active,
                            );
                            if mouse_should_quit {
                                return;
                            }
                            mouse_event = next;
                            break;
                        } else {
                            break; // non-mouse event; stop draining
                        }
                    }
                }
                let mut mouse_should_quit = false;
                sidebar_width = handle_mouse(
                    mouse_event,
                    &mut sidebar,
                    engine,
                    &terminal.size().ok(),
                    sidebar_width,
                    &mut dragging_sidebar,
                    &mut dragging_scrollbar,
                    &mut dragging_sidebar_search,
                    &mut dragging_debug_sb,
                    &mut dragging_terminal_sb,
                    &mut debug_output_scroll,
                    &mut dragging_debug_output_sb,
                    &mut dragging_terminal_resize,
                    &mut dragging_terminal_split,
                    &mut dragging_group_divider,
                    &mut dragging_settings_sb,
                    last_layout.as_ref(),
                    &mut sidebar_prompt,
                    &mut last_click_time,
                    &mut last_click_pos,
                    &mut mouse_text_drag,
                    &mut folder_picker,
                    &mut quit_confirm,
                    &mut close_tab_confirm,
                    &mut cmd_sel,
                    &mut cmd_dragging,
                    &mut mouse_should_quit,
                    &mut explorer_drag_src,
                    &mut explorer_drag_active,
                );
                if mouse_should_quit {
                    return;
                }
            }
            Event::Paste(text) => {
                // Bracketed paste — text delivered directly from the terminal emulator.
                // When the embedded terminal panel has focus, forward directly to the PTY.
                if engine.terminal_has_focus {
                    engine.terminal_write(text.as_bytes());
                    needs_redraw = true;
                    continue;
                }
                use crate::core::Mode;
                match engine.mode {
                    Mode::Command | Mode::Search => {
                        engine.paste_text_to_input(&text);
                    }
                    Mode::Insert => {
                        engine.paste_in_insert_mode(&text);
                    }
                    Mode::Normal | Mode::Visual => {
                        // Load into `"` register then paste after cursor.
                        if !text.is_empty() {
                            engine.registers.insert('"', (text.clone(), false));
                            engine.load_clipboard_register(text);
                            engine.handle_key("p", Some('p'), false);
                            sync_tui_clipboard(engine, &mut last_clipboard_content);
                        }
                    }
                    _ => {}
                }
            }
            Event::Resize(new_w, _new_h) => {
                // Resize the terminal PTY to match the full new terminal width.
                let term_rows = engine.session.terminal_panel_rows;
                engine.terminal_resize(new_w, term_rows);
            }
            _ => {}
        }
        needs_redraw = true;
    }
}

// ─── Explorer context menu action handler ────────────────────────────────────

/// Process explorer-specific context menu actions that need sidebar prompts.
/// Tab context menu actions (close, split, etc.) are handled directly by
/// `context_menu_confirm()` in the engine.
fn handle_explorer_context_action(
    action: &str,
    engine: &mut Engine,
    sidebar: &TuiSidebar,
    sidebar_prompt: &mut Option<SidebarPrompt>,
    terminal_size: Option<ratatui::layout::Rect>,
) {
    // Get the path from the engine's last context menu target.
    // Note: context_menu_confirm() already took the menu, so we reconstruct
    // the path from the sidebar's selected row.
    let idx = sidebar.selected;
    let (path, is_dir) = if idx < sidebar.rows.len() {
        (sidebar.rows[idx].path.clone(), sidebar.rows[idx].is_dir)
    } else {
        return;
    };

    match action {
        "new_file" | "new_folder" => {
            let target = if is_dir {
                &path
            } else {
                path.parent().unwrap_or(&sidebar.root)
            };
            let prefill = target
                .strip_prefix(&sidebar.root)
                .unwrap_or(target)
                .to_string_lossy()
                .to_string();
            let prefill = if prefill.is_empty() {
                String::new()
            } else {
                format!("{}/", prefill)
            };
            let kind = if action == "new_file" {
                PromptKind::NewFile(sidebar.root.clone())
            } else {
                PromptKind::NewFolder(sidebar.root.clone())
            };
            let cursor = prefill.len();
            *sidebar_prompt = Some(SidebarPrompt {
                kind,
                input: prefill,
                cursor,
            });
        }
        "rename" => {
            engine.start_explorer_rename(path);
        }
        "delete" => {
            *sidebar_prompt = Some(SidebarPrompt {
                kind: PromptKind::DeleteConfirm(path),
                input: String::new(),
                cursor: 0,
            });
        }
        // copy_path, copy_relative_path, reveal, open_side are handled by the engine
        "copy_path" | "copy_relative_path" | "reveal" | "open_side" => {}
        "open_terminal" => {
            let dir = if is_dir {
                path.clone()
            } else {
                path.parent().unwrap_or(&sidebar.root).to_path_buf()
            };
            let cols = terminal_size.map(|s| s.width).unwrap_or(80);
            let rows = engine.session.terminal_panel_rows;
            engine.terminal_new_tab_at(cols, rows, Some(&dir));
        }
        // select_for_diff and diff_with_selected are handled by the engine
        "select_for_diff" | "diff_with_selected" => {}
        "find_in_folder" => {
            engine.grep_open = true;
        }
        _ => {}
    }
}

// ─── Mouse handling ───────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn handle_mouse(
    ev: MouseEvent,
    sidebar: &mut TuiSidebar,
    engine: &mut Engine,
    terminal_size: &Option<ratatui::layout::Rect>,
    sidebar_width: u16,
    dragging_sidebar: &mut bool,
    dragging_scrollbar: &mut Option<ScrollDragState>,
    dragging_sidebar_search: &mut Option<SidebarScrollDrag>,
    dragging_debug_sb: &mut Option<DebugSidebarScrollDrag>,
    dragging_terminal_sb: &mut Option<(u16, u16, usize)>,
    debug_output_scroll: &mut usize,
    dragging_debug_output_sb: &mut Option<(u16, u16, usize)>,
    dragging_terminal_resize: &mut bool,
    dragging_terminal_split: &mut bool,
    dragging_group_divider: &mut Option<usize>,
    dragging_settings_sb: &mut Option<SidebarScrollDrag>,
    last_layout: Option<&render::ScreenLayout>,
    sidebar_prompt: &mut Option<SidebarPrompt>,
    last_click_time: &mut Instant,
    last_click_pos: &mut (u16, u16),
    mouse_text_drag: &mut bool,
    folder_picker: &mut Option<FolderPickerState>,
    quit_confirm: &mut bool,
    close_tab_confirm: &mut bool,
    cmd_sel: &mut Option<(usize, usize)>,
    cmd_dragging: &mut bool,
    should_quit: &mut bool,
    explorer_drag_src: &mut Option<usize>,
    explorer_drag_active: &mut Option<(usize, Option<usize>)>,
) -> u16 {
    let col = ev.column;
    let row = ev.row;
    let term_height = terminal_size.map(|s| s.height).unwrap_or(24);

    let ab_width = if engine.settings.autohide_panels && !sidebar.visible {
        0
    } else {
        ACTIVITY_BAR_WIDTH
    };
    let editor_left = ab_width
        + if sidebar.visible {
            sidebar_width + 1
        } else {
            0
        };

    // ── Dialog popup click handling ─────────────────────────────────────────────
    if engine.dialog.is_some() {
        if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
            let term_cols = terminal_size.map(|s| s.width).unwrap_or(80);
            // Recompute dialog geometry (same formula as render_dialog_popup).
            let dialog = engine.dialog.as_ref().unwrap();
            let body_max = dialog.body.iter().map(|l| l.len()).max().unwrap_or(0);
            let btn_row_len: usize = dialog
                .buttons
                .iter()
                .map(|b| render::format_button_label(&b.label, b.hotkey).len() + 4)
                .sum::<usize>()
                + 2;
            let content_width = body_max.max(dialog.title.len() + 4).max(btn_row_len);
            let width = (content_width as u16 + 4).clamp(40, term_cols.saturating_sub(4));
            let height = (3 + dialog.body.len() as u16 + 2 + 1).min(term_height.saturating_sub(4));
            let px = (term_cols.saturating_sub(width)) / 2;
            let py = (term_height.saturating_sub(height)) / 2;
            let btn_y = py + height - 2;

            if row == btn_y {
                // Walk the button positions to find which was clicked.
                let mut col_offset = px + 2;
                for (idx, btn) in dialog.buttons.iter().enumerate() {
                    let label = render::format_button_label(&btn.label, btn.hotkey);
                    let btn_w = label.len() as u16 + 4; // "  label  "
                    if col >= col_offset && col < col_offset + btn_w {
                        let action = engine.dialog_click_button(idx);
                        if engine.explorer_needs_refresh {
                            engine.explorer_needs_refresh = false;
                            sidebar.build_rows();
                        }
                        if handle_action(engine, action) {
                            *should_quit = true;
                        }
                        return sidebar_width;
                    }
                    col_offset += btn_w;
                }
            }
            // Click outside dialog — dismiss (Escape equivalent).
            if col < px || col >= px + width || row < py || row >= py + height {
                engine.dialog = None;
                engine.pending_move = None;
            }
        }
        return sidebar_width;
    }

    // ── Folder picker mouse handling ────────────────────────────────────────────
    if let Some(ref mut picker) = folder_picker {
        if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
            let term_cols = terminal_size.map(|s| s.width).unwrap_or(80);
            let term_rows = terminal_size.map(|s| s.height).unwrap_or(24);
            let popup_w = (term_cols * 3 / 5).max(50);
            let popup_h = (term_rows * 55 / 100).max(15);
            let popup_x = (term_cols.saturating_sub(popup_w)) / 2;
            let popup_y = (term_rows.saturating_sub(popup_h)) / 2;
            let results_start = popup_y + 3;
            let results_end = popup_y + popup_h - 1;

            if col >= popup_x
                && col < popup_x + popup_w
                && row >= results_start
                && row < results_end
            {
                let clicked_idx = picker.scroll_top + (row - results_start) as usize;
                if clicked_idx < picker.filtered.len() {
                    picker.selected = clicked_idx;
                }
            } else if col < popup_x
                || col >= popup_x + popup_w
                || row < popup_y
                || row >= popup_y + popup_h
            {
                // Click outside popup — dismiss
                *folder_picker = None;
            }
            return sidebar_width;
        }
    }

    // ── Sidebar separator drag (works anywhere, regardless of row) ────────────
    let sep_col = ab_width + if sidebar.visible { sidebar_width } else { 0 };
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) if sidebar.visible && col == sep_col => {
            *dragging_sidebar = true;
            return sidebar_width;
        }
        MouseEventKind::Drag(MouseButton::Left) if *dragging_sidebar => {
            let new_w = col.saturating_sub(ab_width);
            return new_w.clamp(15, 150);
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            // Explorer drag-and-drop: activate or update target row.
            if explorer_drag_src.is_some() || explorer_drag_active.is_some() {
                let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                if sidebar.visible
                    && sidebar.active_panel == TuiPanel::Explorer
                    && col >= ab_width
                    && col < ab_width + sidebar_width
                {
                    let sidebar_row = row.saturating_sub(menu_rows);
                    if sidebar_row >= 1 {
                        let tree_row =
                            (sidebar_row as usize).saturating_sub(1) + sidebar.scroll_top;
                        if tree_row < sidebar.rows.len() {
                            if let Some(src_row) = *explorer_drag_src {
                                // Only activate drag if target differs from source.
                                if tree_row != src_row {
                                    *explorer_drag_active = Some((src_row, Some(tree_row)));
                                    *explorer_drag_src = None;
                                }
                            } else if let Some((src, _)) = explorer_drag_active {
                                *explorer_drag_active = Some((*src, Some(tree_row)));
                            }
                        }
                    }
                } else if let Some((src, _)) = explorer_drag_active {
                    // Mouse dragged outside sidebar — clear target but keep active.
                    *explorer_drag_active = Some((*src, None));
                }
                if explorer_drag_active.is_some() {
                    return sidebar_width;
                }
            }
            // Command-line text selection drag
            if *cmd_dragging {
                if let Some(ref mut sel) = *cmd_sel {
                    sel.1 = col as usize;
                }
                return sidebar_width;
            }
            // Debug sidebar section scrollbar drag
            if let Some(ref drag) = *dragging_debug_sb {
                if drag.track_len > 0 && drag.total > 0 {
                    let end = drag.track_abs_start + drag.track_len - 1;
                    let clamped = row.clamp(drag.track_abs_start, end);
                    let ratio = (clamped - drag.track_abs_start) as f64 / drag.track_len as f64;
                    let max_scroll = drag.total.saturating_sub(drag.track_len as usize);
                    engine.dap_sidebar_scroll[drag.sec_idx] =
                        (ratio * max_scroll as f64).round() as usize;
                }
                return sidebar_width;
            }
            // Sidebar search-results scrollbar drag
            if let Some(ref drag) = *dragging_sidebar_search {
                if drag.track_len > 0 && drag.total > 0 {
                    let end = drag.track_abs_start + drag.track_len - 1;
                    let clamped = row.clamp(drag.track_abs_start, end);
                    let ratio = (clamped - drag.track_abs_start) as f64 / drag.track_len as f64;
                    let new_scroll = (ratio * drag.total as f64) as usize;
                    sidebar.search_scroll_top =
                        new_scroll.min(drag.total.saturating_sub(drag.track_len as usize));
                }
                return sidebar_width;
            }
            // Settings panel scrollbar drag
            if let Some(ref drag) = *dragging_settings_sb {
                if drag.track_len > 0 && drag.total > 0 {
                    let end = drag.track_abs_start + drag.track_len - 1;
                    let clamped = row.clamp(drag.track_abs_start, end);
                    let ratio = (clamped - drag.track_abs_start) as f64 / drag.track_len as f64;
                    let max_scroll = drag.total.saturating_sub(drag.track_len as usize);
                    engine.settings_scroll_top = (ratio * max_scroll as f64).round() as usize;
                }
                return sidebar_width;
            }
            // Terminal panel resize drag
            if *dragging_terminal_resize {
                let qf_h: u16 = if engine.quickfix_open { 6 } else { 0 };
                let available = term_height.saturating_sub(row + 2 + qf_h);
                let new_rows = available.saturating_sub(1).clamp(5, 30);
                engine.session.terminal_panel_rows = new_rows;
                return sidebar_width;
            }
            // Group divider drag — update ratio based on mouse position.
            if let Some(split_index) = *dragging_group_divider {
                if let Some(split) = last_layout.and_then(|l| l.editor_group_split.as_ref()) {
                    if let Some(div) = split.dividers.iter().find(|d| d.split_index == split_index)
                    {
                        let mr: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                        let editor_row = row.saturating_sub(mr);
                        let rel_col = col.saturating_sub(editor_left);
                        let mouse_pos = match div.direction {
                            crate::core::window::SplitDirection::Vertical => rel_col as f64,
                            crate::core::window::SplitDirection::Horizontal => editor_row as f64,
                        };
                        let new_ratio = (mouse_pos - div.axis_start) / div.axis_size;
                        engine
                            .group_layout
                            .set_ratio_at_index(split_index, new_ratio);
                    }
                }
                return sidebar_width;
            }
            // Terminal split divider drag — update visual column position (no PTY resize yet).
            if *dragging_terminal_split {
                let term_width = terminal_size.map(|s| s.width).unwrap_or(80);
                let sb_col = term_width.saturating_sub(1);
                let left_cols = col.clamp(5, sb_col.saturating_sub(5));
                engine.terminal_split_set_drag_cols(left_cols);
                return sidebar_width;
            }
            // Debug output panel scrollbar drag
            if let Some((track_start, track_len, total)) = *dragging_debug_output_sb {
                if track_len > 0 && total > 0 {
                    let offset_in_track = row.saturating_sub(track_start).min(track_len) as f64;
                    let ratio = offset_in_track / track_len as f64;
                    // ratio=0 (top) → max scroll (oldest); ratio=1 (bottom) → 0 (newest)
                    let max_scroll = total.saturating_sub(track_len as usize);
                    *debug_output_scroll = ((1.0 - ratio) * max_scroll as f64).round() as usize;
                }
                return sidebar_width;
            }
            // Terminal scrollbar drag
            if let Some((track_start, track_len, total)) = *dragging_terminal_sb {
                if track_len > 0 && total > 0 {
                    // Use saturating_sub + min(track_len) so ratio reaches exactly 1.0
                    // at the bottom of the track (allowing scroll_offset to reach 0).
                    let offset_in_track = row.saturating_sub(track_start).min(track_len) as f64;
                    let ratio = offset_in_track / track_len as f64;
                    // top (ratio=0) → max offset; bottom (ratio=1) → 0 (live view)
                    let new_offset = ((1.0 - ratio) * total as f64) as usize;
                    if let Some(term) = engine.active_terminal_mut() {
                        term.set_scroll_offset(new_offset);
                    }
                }
                return sidebar_width;
            }
            // Scrollbar thumb drag (vertical or horizontal)
            if let Some(ref drag) = *dragging_scrollbar {
                if drag.track_len > 0 && drag.total > 0 {
                    if drag.is_horizontal {
                        let end = drag.track_abs_start + drag.track_len - 1;
                        let clamped = col.clamp(drag.track_abs_start, end);
                        let ratio = (clamped - drag.track_abs_start) as f64 / drag.track_len as f64;
                        let new_left = (ratio * drag.total as f64) as usize;
                        engine.set_scroll_left_for_window(drag.window_id, new_left);
                    } else {
                        let end = drag.track_abs_start + drag.track_len - 1;
                        let clamped = row.clamp(drag.track_abs_start, end);
                        let ratio = (clamped - drag.track_abs_start) as f64 / drag.track_len as f64;
                        let new_top = (ratio * drag.total as f64) as usize;
                        engine.set_cursor_for_window(drag.window_id, new_top, 0);
                        engine.ensure_cursor_visible();
                        engine.sync_scroll_binds();
                    }
                }
                return sidebar_width;
            }
            // Text drag-to-select — find window under cursor and extend visual selection
            if col >= editor_left {
                if let Some(layout) = last_layout {
                    let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                    let editor_row = row.saturating_sub(menu_rows);
                    for rw in &layout.windows {
                        let wx = rw.rect.x as u16;
                        let wy = rw.rect.y as u16;
                        let ww = rw.rect.width as u16;
                        let wh = rw.rect.height as u16;
                        let gutter = rw.gutter_char_width as u16;
                        let rel_col = col - editor_left;
                        if rel_col >= wx
                            && rel_col < wx + ww
                            && editor_row >= wy
                            && editor_row < wy + wh
                        {
                            let view_row = (editor_row - wy) as usize;
                            let drag_rl = rw.lines.get(view_row);
                            let buf_line = drag_rl
                                .map(|l| l.line_idx)
                                .unwrap_or_else(|| rw.scroll_top + view_row);
                            let seg_offset = drag_rl.map(|l| l.segment_col_offset).unwrap_or(0);
                            let col_in_text = (rel_col - wx).saturating_sub(gutter) as usize
                                + rw.scroll_left
                                + seg_offset;
                            engine.mouse_drag(rw.window_id, buf_line, col_in_text);
                            *mouse_text_drag = true;
                            return sidebar_width;
                        }
                    }
                }
            }
            // Terminal drag-to-select in content rows.
            {
                let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
                let strip_rows: u16 = if engine.terminal_open {
                    engine.session.terminal_panel_rows + 1
                } else {
                    0
                };
                let term_strip_top = term_height.saturating_sub(2 + qf_rows + strip_rows);
                if engine.terminal_open
                    && strip_rows > 0
                    && row > term_strip_top
                    && row < term_strip_top + strip_rows
                {
                    let term_row = row - term_strip_top - 1;
                    if let Some(term) = engine.active_terminal_mut() {
                        if let Some(ref mut sel) = term.selection {
                            sel.end_row = term_row;
                            sel.end_col = col;
                        }
                    }
                    return sidebar_width;
                }
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            // Explorer drag-and-drop: execute move on release.
            if let Some((src_row, Some(target_row))) = explorer_drag_active.take() {
                *explorer_drag_src = None;
                if src_row < sidebar.rows.len() && target_row < sidebar.rows.len() {
                    let src_path = sidebar.rows[src_row].path.clone();
                    let target = &sidebar.rows[target_row];
                    let dest_dir = if target.is_dir {
                        target.path.clone()
                    } else {
                        target
                            .path
                            .parent()
                            .unwrap_or(std::path::Path::new("."))
                            .to_path_buf()
                    };
                    engine.confirm_move_file(&src_path, &dest_dir);
                }
                return sidebar_width;
            }
            *explorer_drag_src = None;
            *explorer_drag_active = None;
            *dragging_sidebar = false;
            *dragging_scrollbar = None;
            *dragging_sidebar_search = None;
            *dragging_debug_sb = None;
            *dragging_terminal_sb = None;
            *dragging_debug_output_sb = None;
            *dragging_settings_sb = None;
            *dragging_group_divider = None;
            *cmd_dragging = false;
            if *dragging_terminal_resize {
                *dragging_terminal_resize = false;
                let rows = engine.session.terminal_panel_rows;
                let cols = terminal_size.map(|s| s.width).unwrap_or(80);
                engine.terminal_resize(cols, rows);
                let _ = engine.session.save();
            }
            if *dragging_terminal_split {
                *dragging_terminal_split = false;
                let left_cols = engine.terminal_split_left_cols;
                if left_cols > 0 {
                    let term_width = terminal_size.map(|s| s.width).unwrap_or(80);
                    let sb_col = term_width.saturating_sub(1);
                    let right_cols = sb_col.saturating_sub(left_cols).saturating_sub(1);
                    let rows = engine.session.terminal_panel_rows;
                    engine.terminal_split_finalize_drag(left_cols, right_cols, rows);
                }
            }
            *mouse_text_drag = false;
            engine.mouse_drag_active = false;
            // Auto-copy terminal selection to clipboard on mouse-release.
            if engine.terminal_has_focus {
                let text = engine.active_terminal().and_then(|t| t.selected_text());
                if let Some(ref text) = text {
                    if let Some(ref cb) = engine.clipboard_write {
                        let _ = cb(text);
                    }
                }
            }
            return sidebar_width;
        }
        // Scroll wheel — sidebar or editor
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            // Sidebar scroll wheel
            if sidebar.visible && col >= ab_width && col < ab_width + sidebar_width {
                if sidebar.active_panel == TuiPanel::Explorer {
                    let tree_height = term_height.saturating_sub(3) as usize;
                    let total = sidebar.rows.len();
                    if total > tree_height {
                        if matches!(ev.kind, MouseEventKind::ScrollUp) {
                            sidebar.scroll_top = sidebar.scroll_top.saturating_sub(3);
                        } else {
                            sidebar.scroll_top =
                                (sidebar.scroll_top + 3).min(total.saturating_sub(tree_height));
                        }
                    }
                } else if sidebar.active_panel == TuiPanel::Search {
                    // Scroll the viewport directly; render will keep selection visible.
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        sidebar.search_scroll_top = sidebar.search_scroll_top.saturating_sub(3);
                    } else {
                        sidebar.search_scroll_top += 3; // clamped in render_search_panel
                    }
                } else if sidebar.active_panel == TuiPanel::Git {
                    // SC panel: scroll selection
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        engine.sc_selected = engine.sc_selected.saturating_sub(3);
                    } else {
                        let flat_len = engine.sc_flat_len();
                        engine.sc_selected =
                            (engine.sc_selected + 3).min(flat_len.saturating_sub(1));
                    }
                } else if sidebar.active_panel == TuiPanel::Debug {
                    use crate::core::engine::DebugSidebarSection;
                    // Determine which section the mouse is over.
                    let menu_offset = if engine.menu_bar_visible { 1u16 } else { 0 };
                    let sidebar_row = row.saturating_sub(menu_offset);
                    let sections = [
                        (DebugSidebarSection::Variables, 0usize),
                        (DebugSidebarSection::Watch, 1),
                        (DebugSidebarSection::CallStack, 2),
                        (DebugSidebarSection::Breakpoints, 3),
                    ];
                    let mut cur_row: u16 = 2;
                    let mut target_idx: Option<usize> = None;
                    for (_section, sec_idx) in &sections {
                        let sec_height = engine.dap_sidebar_section_heights[*sec_idx];
                        let section_end = cur_row + 1 + sec_height; // header + content
                        if sidebar_row >= cur_row && sidebar_row < section_end {
                            target_idx = Some(*sec_idx);
                            break;
                        }
                        cur_row = section_end;
                    }
                    if let Some(sec_idx) = target_idx {
                        let item_count = engine.dap_sidebar_section_item_count(sections[sec_idx].0);
                        let height = engine.dap_sidebar_section_heights[sec_idx] as usize;
                        let max_scroll = item_count.saturating_sub(height);
                        if matches!(ev.kind, MouseEventKind::ScrollUp) {
                            engine.dap_sidebar_scroll[sec_idx] =
                                engine.dap_sidebar_scroll[sec_idx].saturating_sub(3);
                        } else {
                            engine.dap_sidebar_scroll[sec_idx] =
                                (engine.dap_sidebar_scroll[sec_idx] + 3).min(max_scroll);
                        }
                    }
                } else if sidebar.active_panel == TuiPanel::Settings {
                    let flat = engine.settings_flat_list();
                    let content_height = term_height.saturating_sub(4) as usize; // header+search+status+cmd
                    let max_scroll = flat.len().saturating_sub(content_height);
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        engine.settings_scroll_top = engine.settings_scroll_top.saturating_sub(3);
                    } else {
                        engine.settings_scroll_top =
                            (engine.settings_scroll_top + 3).min(max_scroll);
                    }
                } else if sidebar.active_panel == TuiPanel::Extensions {
                    // Scroll selection up/down
                    let total = engine.ext_available_manifests().len();
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        engine.ext_sidebar_selected = engine.ext_sidebar_selected.saturating_sub(3);
                    } else {
                        engine.ext_sidebar_selected =
                            (engine.ext_sidebar_selected + 3).min(total.saturating_sub(1));
                    }
                }
                return sidebar_width;
            }
            // Terminal panel scroll (must check before editor scroll).
            {
                let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
                let strip_rows: u16 = if engine.terminal_open {
                    engine.session.terminal_panel_rows + 1
                } else {
                    0
                };
                let term_strip_top = term_height.saturating_sub(2 + qf_rows + strip_rows);
                if engine.terminal_open
                    && strip_rows > 0
                    && row >= term_strip_top
                    && row < term_strip_top + strip_rows
                {
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        engine.terminal_scroll_up(3);
                    } else {
                        engine.terminal_scroll_down(3);
                    }
                    return sidebar_width;
                }
            }
            // Debug output panel scroll wheel.
            {
                let debug_output_open = engine.bottom_panel_kind
                    == render::BottomPanelKind::DebugOutput
                    && !engine.dap_output_lines.is_empty();
                if debug_output_open {
                    let dt_rows: u16 = if engine.debug_toolbar_visible { 1 } else { 0 };
                    let panel_height = engine.session.terminal_panel_rows + 2;
                    let panel_y = term_height.saturating_sub(2 + dt_rows + panel_height);
                    let panel_end = term_height.saturating_sub(2 + dt_rows);
                    if row >= panel_y && row < panel_end {
                        let content_rows = engine.session.terminal_panel_rows as usize;
                        let total = engine.dap_output_lines.len();
                        let max_scroll = total.saturating_sub(content_rows);
                        if matches!(ev.kind, MouseEventKind::ScrollUp) {
                            *debug_output_scroll = (*debug_output_scroll + 3).min(max_scroll);
                        } else {
                            *debug_output_scroll = debug_output_scroll.saturating_sub(3);
                        }
                        return sidebar_width;
                    }
                }
            }

            if col >= editor_left && row + 2 < term_height {
                let rel_col = col - editor_left;
                let scroll_menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                let editor_row = row.saturating_sub(scroll_menu_rows);
                // Find which window the mouse is over; scroll that window
                let scrolled = last_layout.and_then(|layout| {
                    layout.windows.iter().find(|rw| {
                        let wx = rw.rect.x as u16;
                        let wy = rw.rect.y as u16;
                        let ww = rw.rect.width as u16;
                        let wh = rw.rect.height as u16;
                        rel_col >= wx
                            && rel_col < wx + ww
                            && editor_row >= wy
                            && editor_row < wy + wh
                    })
                });
                if let Some(rw) = scrolled {
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        engine.scroll_up_visible_for_window(rw.window_id, 3);
                    } else {
                        engine.scroll_down_visible_for_window(rw.window_id, 3);
                    }
                    engine.sync_scroll_binds();
                } else {
                    // Fallback: scroll active window
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        engine.scroll_up_visible(3);
                    } else {
                        engine.scroll_down_visible(3);
                    }
                    engine.ensure_cursor_visible();
                    engine.sync_scroll_binds();
                }
            }
            return sidebar_width;
        }
        _ => {}
    }

    // ── Right-click: open context menus ────────────────────────────────────────
    if ev.kind == MouseEventKind::Down(MouseButton::Right) {
        // Close any existing context menu first.
        engine.close_context_menu();

        let menu_rows = if engine.menu_bar_visible { 1_u16 } else { 0 };

        // Right-click on explorer sidebar → open explorer context menu
        if sidebar.visible && col >= ab_width && col < ab_width + sidebar_width {
            if sidebar.active_panel == TuiPanel::Explorer {
                let sidebar_row = row.saturating_sub(menu_rows);
                if sidebar_row >= 1 {
                    let tree_row = (sidebar_row as usize).saturating_sub(1) + sidebar.scroll_top;
                    if tree_row < sidebar.rows.len() {
                        sidebar.selected = tree_row;
                        let path = sidebar.rows[tree_row].path.clone();
                        let is_dir = sidebar.rows[tree_row].is_dir;
                        engine.open_explorer_context_menu(path, is_dir, col, row);
                    }
                }
            }
            return sidebar_width;
        }

        // Right-click on tab bar → open tab context menu
        if col >= editor_left {
            let rel_col = col - editor_left;
            if let Some(layout) = last_layout {
                if let Some(ref split) = layout.editor_group_split {
                    let click_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
                    for gtb in split.group_tab_bars.iter() {
                        let tab_bar_row =
                            menu_rows + (gtb.bounds.y as u16).saturating_sub(click_tbh);
                        let gx = gtb.bounds.x as u16;
                        let gw = gtb.bounds.width as u16;
                        if row == tab_bar_row && rel_col >= gx && rel_col < gx + gw {
                            let local_col = rel_col - gx;
                            let mut x: u16 = 0;
                            for (i, tab) in gtb.tabs.iter().enumerate() {
                                let name_w = tab.name.chars().count() as u16;
                                let tab_w = name_w + TAB_CLOSE_COLS;
                                if local_col >= x && local_col < x + tab_w {
                                    engine.open_tab_context_menu(gtb.group_id, i, col, row + 1);
                                    return sidebar_width;
                                }
                                x += tab_w;
                            }
                            break;
                        }
                    }
                } else {
                    // Single-group tab bar (row == menu_rows)
                    if row == menu_rows {
                        let mut x: u16 = 0;
                        for (i, tab) in layout.tab_bar.iter().enumerate() {
                            let name_w = tab.name.chars().count() as u16;
                            let tab_w = name_w + TAB_CLOSE_COLS;
                            if rel_col >= x && rel_col < x + tab_w {
                                engine.open_tab_context_menu(engine.active_group, i, col, row + 1);
                                return sidebar_width;
                            }
                            x += tab_w;
                        }
                    }
                }
            }
        }

        return sidebar_width;
    }

    // ── Context menu click intercept ────────────────────────────────────────────
    if engine.context_menu.is_some() && ev.kind == MouseEventKind::Down(MouseButton::Left) {
        // Check if click is inside the context menu popup
        if let Some(ref cm) = engine.context_menu {
            let sep_count = cm.items.iter().filter(|i| i.separator_after).count() as u16;
            let popup_h = cm.items.len() as u16 + sep_count + 2;
            let max_label = cm.items.iter().map(|i| i.label.len()).max().unwrap_or(4);
            let max_sc = cm.items.iter().map(|i| i.shortcut.len()).max().unwrap_or(0);
            let popup_w = (max_label + max_sc + 6).clamp(20, 50) as u16;
            let term_w = terminal_size.map(|s| s.width).unwrap_or(80);
            let px = cm.screen_x.min(term_w.saturating_sub(popup_w));
            let py = cm.screen_y.min(term_height.saturating_sub(popup_h));

            if col >= px && col < px + popup_w && row >= py && row < py + popup_h {
                // Click inside — map to item
                let inner_row = row - py;
                if inner_row >= 1 && inner_row < popup_h - 1 {
                    // Walk items + separators to find which was clicked
                    let mut visual_row: u16 = 1;
                    for (idx, item) in cm.items.iter().enumerate() {
                        if visual_row == inner_row {
                            if item.enabled {
                                engine.context_menu.as_mut().unwrap().selected = idx;
                                if let Some(act) = engine.context_menu_confirm() {
                                    handle_explorer_context_action(
                                        &act,
                                        engine,
                                        sidebar,
                                        sidebar_prompt,
                                        *terminal_size,
                                    );
                                }
                            }
                            return sidebar_width;
                        }
                        visual_row += 1;
                        if item.separator_after {
                            if visual_row == inner_row {
                                // Clicked on separator line — ignore
                                return sidebar_width;
                            }
                            visual_row += 1;
                        }
                    }
                }
                return sidebar_width;
            }
        }
        // Click outside — close menu
        engine.close_context_menu();
        // Fall through to process the click normally
    }

    // ── Context menu mouse hover ──────────────────────────────────────────────
    if engine.context_menu.is_some() && matches!(ev.kind, MouseEventKind::Moved) {
        // Compute hit item by examining the menu geometry.
        let mut hit_idx: Option<usize> = None;
        if let Some(ref cm) = engine.context_menu {
            let sep_count = cm.items.iter().filter(|i| i.separator_after).count() as u16;
            let popup_h = cm.items.len() as u16 + sep_count + 2;
            let max_label = cm.items.iter().map(|i| i.label.len()).max().unwrap_or(4);
            let max_sc = cm.items.iter().map(|i| i.shortcut.len()).max().unwrap_or(0);
            let popup_w = (max_label + max_sc + 6).clamp(20, 50) as u16;
            let term_w = terminal_size.map(|s| s.width).unwrap_or(80);
            let px = cm.screen_x.min(term_w.saturating_sub(popup_w));
            let py = cm.screen_y.min(term_height.saturating_sub(popup_h));

            if col >= px && col < px + popup_w && row >= py && row < py + popup_h {
                let inner_row = row - py;
                if inner_row >= 1 && inner_row < popup_h - 1 {
                    let mut visual_row: u16 = 1;
                    for (idx, item) in cm.items.iter().enumerate() {
                        if visual_row == inner_row && item.enabled {
                            hit_idx = Some(idx);
                            break;
                        }
                        visual_row += 1;
                        if item.separator_after {
                            visual_row += 1;
                        }
                    }
                }
            }
        }
        if let Some(idx) = hit_idx {
            if let Some(ref mut cm) = engine.context_menu {
                cm.selected = idx;
            }
        }
        return sidebar_width;
    }

    // Only process left-click presses from here on
    if ev.kind != MouseEventKind::Down(MouseButton::Left) {
        return sidebar_width;
    }

    // ── Command line click — start text selection ──────────────────────────────
    {
        use crate::core::Mode;
        if row + 1 == term_height && matches!(engine.mode, Mode::Command | Mode::Search) {
            let char_idx = col as usize;
            let buf_len = engine.command_buffer.chars().count();
            engine.command_cursor = char_idx.saturating_sub(1).min(buf_len);
            *cmd_sel = Some((char_idx, char_idx));
            *cmd_dragging = true;
            return sidebar_width;
        }
    }

    // Bottom 2 rows are status + cmd — ignore
    if row + 2 >= term_height {
        return sidebar_width;
    }

    // ── Menu bar row click ─────────────────────────────────────────────────────
    if engine.menu_bar_visible && row == 0 {
        let mut col_pos: u16 = 1; // 1-cell left pad
        for (idx, (name, _, _)) in render::MENU_STRUCTURE.iter().enumerate() {
            let item_w = name.chars().count() as u16 + 2; // space + name + space
            if col >= col_pos && col < col_pos + item_w {
                if engine.menu_open_idx == Some(idx) {
                    engine.close_menu();
                } else {
                    engine.open_menu(idx);
                }
                return sidebar_width;
            }
            col_pos += item_w;
        }
        engine.close_menu(); // click in empty area of menu bar
        return sidebar_width;
    }

    // ── Menu dropdown item click ───────────────────────────────────────────────
    if let Some(open_idx) = engine.menu_open_idx {
        if let Some((_, _, items)) = render::MENU_STRUCTURE.get(open_idx) {
            // Determine the dropdown anchor column (same formula as render_menu_dropdown)
            let mut popup_col: u16 = 1;
            for i in 0..open_idx {
                if let Some((name, _, _)) = render::MENU_STRUCTURE.get(i) {
                    popup_col += name.chars().count() as u16 + 2;
                }
            }
            let max_label = items.iter().map(|i| i.label.len()).max().unwrap_or(4);
            let max_shortcut = items.iter().map(|i| i.shortcut.len()).max().unwrap_or(0);
            let popup_width = (max_label + max_shortcut + 6).clamp(20, 50) as u16;
            let popup_x = popup_col.min(term_height.saturating_sub(popup_width));
            // Dropdown rows: border(1) + items
            let menu_bar_row: u16 = if engine.menu_bar_visible { 1 } else { 0 };
            let popup_y = menu_bar_row; // dropdown starts below menu bar
            if row > popup_y && col >= popup_x && col < popup_x + popup_width {
                let item_idx = (row - popup_y - 1) as usize;
                if item_idx < items.len() && !items[item_idx].separator && items[item_idx].enabled {
                    let action = items[item_idx].action.to_string();
                    if action == "open_file_dialog" {
                        engine.close_menu();
                        engine.open_fuzzy_finder();
                    } else {
                        let act = engine.menu_activate_item(open_idx, item_idx, &action);
                        if act == EngineAction::OpenTerminal {
                            let cols = terminal_size.map(|s| s.width).unwrap_or(80);
                            engine.terminal_new_tab(cols, engine.session.terminal_panel_rows);
                        } else if let EngineAction::RunInTerminal(cmd) = act {
                            let cols = terminal_size.map(|s| s.width).unwrap_or(80);
                            engine.terminal_run_command(
                                &cmd,
                                cols,
                                engine.session.terminal_panel_rows,
                            );
                        } else if act == EngineAction::OpenFolderDialog {
                            *folder_picker = Some(FolderPickerState::new(
                                &engine.cwd.clone(),
                                FolderPickerMode::OpenFolder,
                                engine.settings.show_hidden_files,
                            ));
                        } else if act == EngineAction::OpenWorkspaceDialog {
                            // open_workspace_from_file() already ran in the engine;
                            // refresh the sidebar to reflect the new cwd.
                            *sidebar = TuiSidebar::new(engine.cwd.clone(), sidebar.visible);
                            sidebar.show_hidden_files = engine.settings.show_hidden_files;
                        } else if act == EngineAction::SaveWorkspaceAsDialog {
                            let ws_path = engine.cwd.join(".vimcode-workspace");
                            engine.save_workspace_as(&ws_path);
                        } else if act == EngineAction::OpenRecentDialog {
                            *folder_picker = Some(FolderPickerState::new_recent(
                                &engine.session.recent_workspaces,
                            ));
                        } else if act == EngineAction::QuitWithUnsaved {
                            *quit_confirm = true;
                        } else if act == EngineAction::ToggleSidebar {
                            sidebar.visible = !sidebar.visible;
                        } else if handle_action(engine, act) {
                            *should_quit = true;
                        }
                    }
                }
                return sidebar_width;
            }
            // Click outside dropdown — close it
            engine.close_menu();
        }
    }

    // ── Debug toolbar row click ────────────────────────────────────────────────
    if engine.debug_toolbar_visible {
        let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
        let strip_rows: u16 = if engine.terminal_open {
            engine.session.terminal_panel_rows + 1
        } else {
            0
        };
        let toolbar_row = term_height.saturating_sub(3 + qf_rows + strip_rows);
        if row == toolbar_row {
            let mut col_pos: u16 = 1;
            for (idx, btn) in render::DEBUG_BUTTONS.iter().enumerate() {
                if idx == 4 {
                    col_pos += 2; // separator gap
                }
                let btn_w = (btn.icon.chars().count() + btn.key_hint.chars().count() + 4) as u16;
                if col >= col_pos && col < col_pos + btn_w {
                    let _ = engine.execute_command(btn.action);
                    return sidebar_width;
                }
                col_pos += btn_w;
            }
            return sidebar_width; // click in toolbar row, consume event
        }
    }

    // ── Debug output panel click (scrollbar) ──────────────────────────────────
    {
        let debug_output_open = engine.bottom_panel_kind == render::BottomPanelKind::DebugOutput
            && !engine.dap_output_lines.is_empty();
        if debug_output_open {
            let dt_rows: u16 = if engine.debug_toolbar_visible { 1 } else { 0 };
            let panel_height = engine.session.terminal_panel_rows + 2;
            let panel_y = term_height.saturating_sub(2 + dt_rows + panel_height);
            let panel_end = term_height.saturating_sub(2 + dt_rows);
            if row >= panel_y && row < panel_end {
                let term_width = terminal_size.map(|s| s.width).unwrap_or(80);
                let sb_col = term_width.saturating_sub(1);
                let total = engine.dap_output_lines.len();
                let content_rows = engine.session.terminal_panel_rows as usize;
                if total > content_rows && col == sb_col && row >= panel_y + 2 {
                    // Click on scrollbar track — start drag.
                    let track_start = panel_y + 2; // after tab-bar row + header row
                    let track_len = engine.session.terminal_panel_rows;
                    *dragging_debug_output_sb = Some((track_start, track_len, total));
                }
                return sidebar_width;
            }
        }
    }
    // ── Terminal panel click ───────────────────────────────────────────────────
    {
        let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
        let strip_rows: u16 = if engine.terminal_open {
            engine.session.terminal_panel_rows + 1
        } else {
            0
        };
        let term_strip_top = term_height.saturating_sub(2 + qf_rows + strip_rows);
        if engine.terminal_open
            && strip_rows > 0
            && row >= term_strip_top
            && row < term_strip_top + strip_rows
        {
            if row == term_strip_top {
                // Header row — tab switch, toolbar buttons, or resize drag.
                engine.terminal_has_focus = true;
                const TERMINAL_TAB_COLS: u16 = 4;
                let tab_count = engine.terminal_panes.len() as u16;
                let term_width = terminal_size.map(|s| s.width).unwrap_or(80);
                if tab_count > 0 && col < tab_count * TERMINAL_TAB_COLS {
                    engine.terminal_switch_tab((col / TERMINAL_TAB_COLS) as usize);
                } else if col >= term_width.saturating_sub(2) {
                    // Close icon (rightmost 2 cols)
                    engine.terminal_close_active_tab();
                } else if col >= term_width.saturating_sub(4) {
                    // Split button (2 cols left of close)
                    let full_cols = terminal_size.map(|s| s.width).unwrap_or(80);
                    let rows = engine.session.terminal_panel_rows;
                    engine.terminal_toggle_split(full_cols, rows);
                } else if col >= term_width.saturating_sub(6) {
                    // Add button (2 cols left of split)
                    let cols = terminal_size.map(|s| s.width).unwrap_or(80);
                    let rows = engine.session.terminal_panel_rows;
                    engine.terminal_new_tab(cols, rows);
                } else {
                    *dragging_terminal_resize = true;
                }
            } else {
                // Content row — focus split pane or start divider drag.
                if engine.terminal_split && engine.terminal_panes.len() >= 2 {
                    // Mirror render.rs: use drag-override if set, else actual PTY cols.
                    let div_col = if engine.terminal_split_left_cols > 0 {
                        engine.terminal_split_left_cols
                    } else {
                        engine.terminal_panes[0].cols
                    };
                    // Allow clicking within ±1 column of the divider to start a resize drag.
                    if col.abs_diff(div_col) <= 1 {
                        engine.terminal_has_focus = true;
                        *dragging_terminal_split = true;
                        return sidebar_width; // skip selection start
                    } else {
                        engine.terminal_active = if col < div_col { 0 } else { 1 };
                    }
                }
                // Check for scrollbar click first.
                let term_width = terminal_size.map(|s| s.width).unwrap_or(80);
                let sb_col = term_width.saturating_sub(1);
                engine.terminal_has_focus = true;
                if col == sb_col {
                    // Scrollbar column — start drag.
                    let track_start = term_strip_top + 1;
                    let track_len = strip_rows.saturating_sub(1); // content rows
                                                                  // Cap total to one screenful (vt100 API limit) so the drag range
                                                                  // [0, total] exactly matches what set_scroll_offset can deliver.
                    let total = engine
                        .active_terminal()
                        .map(|t| t.history.len())
                        .unwrap_or(0);
                    *dragging_terminal_sb = Some((track_start, track_len, total));
                } else {
                    // Content area — start a selection.
                    let term_row = row - term_strip_top - 1;
                    engine.terminal_scroll_reset();
                    if let Some(term) = engine.active_terminal_mut() {
                        term.selection = Some(crate::core::terminal::TermSelection {
                            start_row: term_row,
                            start_col: col,
                            end_row: term_row,
                            end_col: col,
                        });
                    }
                }
            }
            return sidebar_width;
        }
    }
    // Click landed outside the terminal panel — return focus to the editor.
    engine.terminal_has_focus = false;

    // ── Activity bar ──────────────────────────────────────────────────────────
    if col < ab_width {
        // Activity bar is in the main content area, below the menu bar row (if visible)
        // and above the debug toolbar row (if visible).
        let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
        let strip_rows: u16 = if engine.terminal_open {
            engine.session.terminal_panel_rows + 1
        } else {
            0
        };
        let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
        let dbg_rows: u16 = if engine.debug_toolbar_visible { 1 } else { 0 };
        // Activity bar starts at row `menu_rows` in absolute terminal coordinates.
        if row < menu_rows {
            return sidebar_width; // click in menu bar area, ignore
        }
        let bar_row = row - menu_rows; // row relative to activity bar start
        let bar_height =
            term_height.saturating_sub(2 + qf_rows + strip_rows + menu_rows + dbg_rows);
        let settings_row = bar_height.saturating_sub(1);
        // Row 0: hamburger (menu bar toggle)
        if bar_row == 0 {
            engine.toggle_menu_bar();
            return sidebar_width;
        }
        let target_panel = match bar_row {
            1 => Some(TuiPanel::Explorer),
            2 => Some(TuiPanel::Search),
            3 => Some(TuiPanel::Debug),
            4 => Some(TuiPanel::Git),
            5 => Some(TuiPanel::Extensions),
            6 => Some(TuiPanel::Ai),
            r if r == settings_row && settings_row >= 7 => Some(TuiPanel::Settings),
            _ => None,
        };
        // Check if click is on an extension panel icon (rows 7+)
        if target_panel.is_none() && bar_row >= 7 {
            let ext_idx = (bar_row - 7) as usize;
            let mut ext_names: Vec<_> = engine.ext_panels.keys().cloned().collect();
            ext_names.sort();
            if ext_idx < ext_names.len() {
                let name = ext_names[ext_idx].clone();
                if sidebar.ext_panel_name.as_deref() == Some(&name) && sidebar.visible {
                    sidebar.visible = false;
                    sidebar.ext_panel_name = None;
                    engine.ext_panel_has_focus = false;
                    engine.ext_panel_active = None;
                } else {
                    sidebar.ext_panel_name = Some(name.clone());
                    sidebar.visible = true;
                    sidebar.has_focus = true;
                    engine.ext_panel_active = Some(name.clone());
                    engine.ext_panel_has_focus = true;
                    engine.ext_panel_selected = 0;
                    engine.plugin_event("panel_focus", &name);
                }
                engine.session.explorer_visible = sidebar.visible;
                let _ = engine.session.save();
                return sidebar_width;
            }
        }
        if let Some(panel) = target_panel {
            // Clear extension panel state when switching to a built-in panel
            sidebar.ext_panel_name = None;
            engine.ext_panel_has_focus = false;
            engine.ext_panel_active = None;
            if sidebar.active_panel == panel && sidebar.visible {
                sidebar.visible = false;
            } else {
                sidebar.active_panel = panel;
                sidebar.visible = true;
                if panel == TuiPanel::Search {
                    sidebar.has_focus = true;
                    sidebar.search_input_mode = true;
                }
                if panel == TuiPanel::Git {
                    engine.sc_refresh();
                }
                if panel == TuiPanel::Extensions {
                    engine.ext_sidebar_has_focus = true;
                    if engine.ext_registry.is_none() && !engine.ext_registry_fetching {
                        engine.ext_refresh();
                    }
                    sidebar.has_focus = true;
                }
                if panel == TuiPanel::Ai {
                    engine.ai_has_focus = true;
                    sidebar.has_focus = true;
                }
                if panel == TuiPanel::Settings {
                    engine.settings_has_focus = true;
                    sidebar.has_focus = true;
                }
            }
            engine.session.explorer_visible = sidebar.visible;
            let _ = engine.session.save();
        }
        return sidebar_width;
    }

    // ── Sidebar panel area ────────────────────────────────────────────────────
    if sidebar.visible && col < ab_width + sidebar_width {
        // Rightmost column of the sidebar is the scrollbar column.
        let sb_col = ab_width + sidebar_width - 1;
        // Account for menu bar: when visible it occupies absolute row 0, so the
        // sidebar's logical row 0 is at absolute terminal row `menu_rows`.
        let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
        let sidebar_row = row.saturating_sub(menu_rows);

        if sidebar.active_panel == TuiPanel::Explorer {
            // tree_height = (total height - 2 status rows) - 1 header row
            let tree_height = term_height.saturating_sub(3) as usize;
            let total_rows = sidebar.rows.len();

            // Click on the scrollbar column → jump-scroll
            if col == sb_col && total_rows > tree_height && sidebar_row >= 1 {
                let rel_row = sidebar_row.saturating_sub(1) as usize;
                let ratio = rel_row as f64 / tree_height as f64;
                let new_top = (ratio * total_rows as f64) as usize;
                sidebar.scroll_top = new_top.min(total_rows.saturating_sub(tree_height));
                return sidebar_width;
            }

            if sidebar_row == 0 {
                // Header row: check if a toolbar button was clicked.
                // Toolbar is right-aligned: 5 NF icons × 3 cols = 15.
                let toolbar_start = ab_width + sidebar_width - EXPLORER_TOOLBAR_LEN;
                if col >= toolbar_start {
                    let btn = (col - toolbar_start) / 3; // 0=new-file 1=new-folder 2=delete
                    let idx = sidebar.selected;
                    let selected_is_dir = idx < sidebar.rows.len() && sidebar.rows[idx].is_dir;
                    match btn {
                        0 | 1 if idx < sidebar.rows.len() => {
                            let target = if selected_is_dir {
                                &sidebar.rows[idx].path
                            } else {
                                sidebar.rows[idx].path.parent().unwrap_or(&sidebar.root)
                            };
                            let prefill = target
                                .strip_prefix(&sidebar.root)
                                .unwrap_or(target)
                                .to_string_lossy()
                                .to_string();
                            let prefill = if prefill.is_empty() {
                                String::new()
                            } else {
                                format!("{}/", prefill)
                            };
                            let kind = if btn == 0 {
                                PromptKind::NewFile(sidebar.root.clone())
                            } else {
                                PromptKind::NewFolder(sidebar.root.clone())
                            };
                            let cursor = prefill.len();
                            *sidebar_prompt = Some(SidebarPrompt {
                                kind,
                                input: prefill,
                                cursor,
                            });
                        }
                        2 => {
                            if idx < sidebar.rows.len() {
                                let path = sidebar.rows[idx].path.clone();
                                *sidebar_prompt = Some(SidebarPrompt {
                                    kind: PromptKind::DeleteConfirm(path),
                                    input: String::new(),
                                    cursor: 0,
                                });
                            }
                        }
                        _ => {}
                    }
                }
                return sidebar_width;
            }
            let tree_row = (sidebar_row as usize).saturating_sub(1) + sidebar.scroll_top;
            if tree_row < sidebar.rows.len() {
                // Record potential drag source for DnD.
                *explorer_drag_src = Some(tree_row);
                if sidebar.rows[tree_row].is_dir {
                    sidebar.selected = tree_row;
                    sidebar.toggle_dir(tree_row);
                } else {
                    sidebar.selected = tree_row;
                    let path = sidebar.rows[tree_row].path.clone();
                    let now = Instant::now();
                    let is_double = now.duration_since(*last_click_time)
                        < Duration::from_millis(400)
                        && *last_click_pos == (col, row);
                    *last_click_time = now;
                    *last_click_pos = (col, row);
                    if is_double {
                        engine.open_file_in_tab(&path);
                    } else {
                        engine.open_file_preview(&path);
                    }
                }
            }
        } else if sidebar.active_panel == TuiPanel::Debug {
            use crate::core::engine::DebugSidebarSection;
            sidebar.has_focus = true;
            engine.dap_sidebar_has_focus = true;

            if sidebar_row == 0 {
                // Header row — no-op
            } else if sidebar_row == 1 {
                // Run/Stop button
                if engine.dap_session_active && engine.dap_stopped_thread.is_some() {
                    engine.dap_continue();
                } else if engine.dap_session_active {
                    engine.execute_command("stop");
                } else {
                    engine.execute_command("debug");
                }
            } else {
                // Walk sections using fixed-allocation layout:
                // row 2+ = [section_header(1) + content(height)]×4
                let sections = [
                    (DebugSidebarSection::Variables, 0usize),
                    (DebugSidebarSection::Watch, 1),
                    (DebugSidebarSection::CallStack, 2),
                    (DebugSidebarSection::Breakpoints, 3),
                ];
                let mut cur_row: u16 = 2;
                for (section, sec_idx) in &sections {
                    let sec_height = engine.dap_sidebar_section_heights[*sec_idx];
                    let section_header_row = cur_row;
                    let items_start = cur_row + 1;
                    let items_end = items_start + sec_height;

                    if sidebar_row == section_header_row {
                        engine.dap_sidebar_section = *section;
                        engine.dap_sidebar_selected = 0;
                        break;
                    } else if sidebar_row >= items_start && sidebar_row < items_end {
                        let item_count = engine.dap_sidebar_section_item_count(*section);
                        let height = sec_height as usize;
                        let sb_col = ab_width + sidebar_width - 1;
                        // Scrollbar click: rightmost column when items overflow.
                        if col == sb_col && item_count > height && height > 0 {
                            let rel_row = (sidebar_row - items_start) as usize;
                            let ratio = rel_row as f64 / height as f64;
                            let max_scroll = item_count.saturating_sub(height);
                            engine.dap_sidebar_scroll[*sec_idx] =
                                (ratio * max_scroll as f64) as usize;
                            engine.dap_sidebar_section = *section;
                            // Arm drag state for subsequent Drag events.
                            *dragging_debug_sb = Some(DebugSidebarScrollDrag {
                                sec_idx: *sec_idx,
                                track_abs_start: items_start + menu_rows,
                                track_len: sec_height,
                                total: item_count,
                            });
                        } else {
                            let scroll_off = engine.dap_sidebar_scroll[*sec_idx];
                            let row_offset = (sidebar_row - items_start) as usize;
                            let item_idx = scroll_off + row_offset;
                            if item_count > 0 && item_idx < item_count {
                                engine.dap_sidebar_section = *section;
                                engine.dap_sidebar_selected = item_idx;
                                engine.handle_debug_sidebar_key("Return", false);
                            }
                        }
                        break;
                    }
                    cur_row = items_end;
                }
            }
            return sidebar_width;
        } else if sidebar.active_panel == TuiPanel::Git {
            sidebar.has_focus = true;
            engine.sc_has_focus = true;

            // sidebar_row layout:
            //   0 = header, 1 = commit input, 2 = button row, 3+ = section rows
            if sidebar_row == 0 {
                // Panel header — no-op
            } else if sidebar_row == 1 {
                // Commit input row — enter commit mode
                engine.sc_commit_input_active = true;
            } else if sidebar_row == 2 {
                // Button row: Commit (~50%), Push/Pull/Sync (~17% each, icon-only).
                // Use column relative to the sidebar content area start.
                let rel_col = col.saturating_sub(ab_width);
                let commit_w = sidebar_width / 2;
                let btn_idx = if rel_col < commit_w {
                    0
                } else {
                    let icon_w = (sidebar_width - commit_w) / 3;
                    let x = rel_col - commit_w;
                    (1 + (x / icon_w.max(1))).min(3) as usize
                };
                engine.sc_activate_button(btn_idx);
            } else {
                // TUI shows a "(no changes)" hint for expanded-but-empty sections
                // (extra visual row with no flat-index entry), so empty_section_hint = true.
                if let Some((flat_idx, is_header)) =
                    engine.sc_visual_row_to_flat(sidebar_row as usize, true)
                {
                    engine.sc_selected = flat_idx;
                    if is_header {
                        engine.handle_sc_key("Tab", false, None);
                    } else {
                        // Click opens the file but keeps panel focus so s/d work immediately.
                        // (Keyboard Enter clears sc_has_focus to return to the editor.)
                        engine.handle_sc_key("Return", false, None);
                        engine.sc_has_focus = true;
                        sidebar.has_focus = true;
                    }
                }
            }
            return sidebar_width;
        } else if sidebar.active_panel == TuiPanel::Search {
            // results_height = (total height - 2 status rows) - 5 panel header rows
            let results_height = term_height.saturating_sub(7) as usize;
            let results = &engine.project_search_results;

            // Click on the scrollbar column in the results area → jump-scroll
            if col == sb_col && !results.is_empty() && sidebar_row >= 5 {
                // Count total display rows (result rows + file header rows)
                let total_display = {
                    let mut count = 0usize;
                    let mut last_file: Option<&std::path::Path> = None;
                    for m in results.iter() {
                        if last_file != Some(m.file.as_path()) {
                            last_file = Some(m.file.as_path());
                            count += 1;
                        }
                        count += 1;
                    }
                    count
                };
                if total_display > results_height {
                    let rel_row = sidebar_row.saturating_sub(5) as usize;
                    let ratio = rel_row as f64 / results_height as f64;
                    let new_scroll = (ratio * total_display as f64) as usize;
                    sidebar.search_scroll_top =
                        new_scroll.min(total_display.saturating_sub(results_height));
                    // Arm drag state so subsequent Drag events continue scrolling.
                    // track_abs_start is the absolute terminal row of the track top.
                    *dragging_sidebar_search = Some(SidebarScrollDrag {
                        track_abs_start: 5 + menu_rows,
                        track_len: results_height as u16,
                        total: total_display,
                    });
                }
                return sidebar_width;
            }

            // sidebar_rows 0-2: header + search + replace inputs — clicking enters input mode
            if sidebar_row <= 2 {
                sidebar.search_input_mode = true;
                sidebar.replace_input_focused = sidebar_row == 2;
            } else {
                sidebar.search_input_mode = false;
                sidebar.replace_input_focused = false;
                // sidebar_row 3 = toggles, 4 = status line; 5+ = results area
                // Add scroll offset so clicks map to the correct result.
                let content_row =
                    (sidebar_row as usize).saturating_sub(5) + sidebar.search_scroll_top;
                if !results.is_empty() {
                    let selected = visual_row_to_result_idx(results, content_row);
                    if let Some(idx) = selected {
                        engine.project_search_selected = idx;
                        // Open the file immediately on click
                        let result = engine
                            .project_search_results
                            .get(idx)
                            .map(|m| (m.file.clone(), m.line));
                        if let Some((file, line)) = result {
                            engine.open_file_in_tab(&file);
                            let win_id = engine.active_window_id();
                            engine.set_cursor_for_window(win_id, line, 0);
                            engine.ensure_cursor_visible();
                            sidebar.has_focus = false;
                        }
                    }
                }
            }
        } else if sidebar.active_panel == TuiPanel::Extensions {
            sidebar.has_focus = true;
            engine.ext_sidebar_has_focus = true;

            // Row layout: 0=header, 1=search, 2=INSTALLED header, 3..=items/headers
            if sidebar_row == 0 {
                // Header — no-op
            } else if sidebar_row == 1 {
                // Search box — activate search input
                engine.ext_sidebar_input_active = true;
            } else {
                let installed = engine.ext_installed_items();
                let installed_len = if engine.ext_sidebar_sections_expanded[0] {
                    installed.len()
                } else {
                    0
                };
                let installed_header_row: u16 = 2;
                let installed_display =
                    installed_len.max(if engine.ext_sidebar_sections_expanded[0] {
                        1
                    } else {
                        0
                    });
                let available_header_row = installed_header_row + 1 + installed_display as u16;

                if sidebar_row == installed_header_row {
                    engine.ext_sidebar_sections_expanded[0] =
                        !engine.ext_sidebar_sections_expanded[0];
                } else if sidebar_row > installed_header_row
                    && sidebar_row < available_header_row
                    && installed_len > 0
                {
                    let idx = (sidebar_row - installed_header_row - 1) as usize;
                    if idx < installed_len {
                        engine.ext_sidebar_selected = idx;
                        engine.handle_ext_sidebar_key("Return", false, None);
                    }
                } else if sidebar_row == available_header_row {
                    engine.ext_sidebar_sections_expanded[1] =
                        !engine.ext_sidebar_sections_expanded[1];
                } else if sidebar_row > available_header_row {
                    let avail_len = if engine.ext_sidebar_sections_expanded[1] {
                        engine.ext_available_items().len()
                    } else {
                        0
                    };
                    let avail_idx = (sidebar_row - available_header_row - 1) as usize;
                    if avail_idx < avail_len {
                        let now = Instant::now();
                        let is_double = now.duration_since(*last_click_time)
                            < Duration::from_millis(400)
                            && *last_click_pos == (col, row);
                        *last_click_time = now;
                        *last_click_pos = (col, row);
                        engine.ext_sidebar_selected = installed_len + avail_idx;
                        if is_double {
                            // Double-click installs
                            engine.handle_ext_sidebar_key("Return", false, None);
                        }
                        // Single-click just selects
                    }
                }
            }
        } else if sidebar.active_panel == TuiPanel::Settings {
            sidebar.has_focus = true;
            engine.settings_has_focus = true;

            // Row 0: header, Row 1: search input, Row 2+: scrollable content
            let content_height = term_height.saturating_sub(4) as usize; // header+search+status+cmd
            let flat_total = engine.settings_flat_list().len();

            // Scrollbar column → jump-scroll + start drag
            if col == sb_col && sidebar_row >= 2 && flat_total > content_height {
                let track_start = row - (sidebar_row - 2);
                let track_len = content_height as u16;
                let rel = (sidebar_row - 2) as f64;
                let ratio = rel / track_len as f64;
                let max_scroll = flat_total.saturating_sub(content_height);
                engine.settings_scroll_top = (ratio * max_scroll as f64).round() as usize;
                *dragging_settings_sb = Some(SidebarScrollDrag {
                    track_abs_start: track_start,
                    track_len,
                    total: flat_total,
                });
            } else if sidebar_row == 0 {
                // Header — no-op
            } else if sidebar_row == 1 {
                // Search box — activate search input
                engine.settings_input_active = true;
            } else {
                let content_row = sidebar_row.saturating_sub(2) as usize;
                let fi = engine.settings_scroll_top + content_row;
                if fi < flat_total {
                    engine.settings_selected = fi;
                    // Double-click toggles bools / expands categories
                    let now = Instant::now();
                    let is_double = now.duration_since(*last_click_time)
                        < Duration::from_millis(400)
                        && *last_click_pos == (col, row);
                    *last_click_time = now;
                    *last_click_pos = (col, row);
                    if is_double {
                        engine.handle_settings_key("Return", false, None);
                    }
                }
            }
        // Extension panel (plugin-provided) click handling
        } else if sidebar.ext_panel_name.is_some() {
            sidebar.has_focus = true;
            engine.ext_panel_has_focus = true;

            if sidebar_row == 0 {
                // Header — no-op
            } else if sidebar_row >= 1 {
                // Map sidebar_row to flat index (row 1 = flat 0 + scroll_top)
                let flat_idx = engine.ext_panel_scroll_top + (sidebar_row - 1) as usize;
                let flat_len = engine.ext_panel_flat_len();
                if flat_idx < flat_len {
                    engine.ext_panel_selected = flat_idx;
                    // Check for double-click → trigger Enter
                    let now = Instant::now();
                    let is_double = now.duration_since(*last_click_time)
                        < Duration::from_millis(400)
                        && *last_click_pos == (col, row);
                    *last_click_time = now;
                    *last_click_pos = (col, row);
                    if is_double {
                        engine.handle_ext_panel_key("Return", false, None);
                    }
                }
            }
        }
        return sidebar_width;
    }

    // ── Editor area ───────────────────────────────────────────────────────────
    sidebar.has_focus = false;
    sidebar.toolbar_focused = false;
    engine.sc_has_focus = false;
    engine.dap_sidebar_has_focus = false;
    engine.ext_sidebar_has_focus = false;
    engine.ai_has_focus = false;
    engine.settings_has_focus = false;
    engine.ext_panel_has_focus = false;
    if col < editor_left {
        return sidebar_width; // separator column
    }

    // The menu bar (if visible) occupies absolute row 0, pushing the tab bar
    // and editor content down by `menu_rows`.
    let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };

    // ── Tab bar click ──────────────────────────────────────────────────────
    // For split groups, any group's tab bar row is clickable (not just the top row).
    if let Some(layout) = last_layout {
        let rel_col = col - editor_left;

        if let Some(ref split) = layout.editor_group_split {
            // Find which group's tab bar row matches the clicked row.
            // Tab bar sits tab_bar_height rows above the group's window content.
            let click_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
            let mut matched_group = None;
            for gtb in split.group_tab_bars.iter() {
                let tab_bar_row = menu_rows + (gtb.bounds.y as u16).saturating_sub(click_tbh);
                let gx = gtb.bounds.x as u16;
                let gw = gtb.bounds.width as u16;
                if row == tab_bar_row && rel_col >= gx && rel_col < gx + gw {
                    let was_active = gtb.group_id == split.active_group;
                    matched_group = Some((
                        gtb.group_id,
                        rel_col - gx,
                        gw,
                        &gtb.tabs,
                        gtb.diff_toolbar.as_ref(),
                        was_active,
                    ));
                    break;
                }
            }
            if let Some((
                group_id,
                local_col,
                bar_width,
                group_tabs,
                diff_toolbar_ref,
                was_active,
            )) = matched_group
            {
                engine.active_group = group_id;

                let mut x: u16 = 0;
                let mut tab_matched = false;
                // Collect tab hit info from immutable borrow, then apply mutably.
                let mut hit_info: Option<(usize, bool)> = None;
                for (i, tab) in group_tabs.iter().enumerate() {
                    let name_width = tab.name.chars().count() as u16;
                    let tab_width = name_width + TAB_CLOSE_COLS;
                    if local_col >= x && local_col < x + tab_width {
                        tab_matched = true;
                        let valid = engine
                            .editor_groups
                            .get(&group_id)
                            .is_some_and(|g| i < g.tabs.len());
                        if valid {
                            let is_close = local_col >= x + name_width;
                            hit_info = Some((i, is_close));
                        }
                        break;
                    }
                    x += tab_width;
                }
                if let Some((tab_idx, is_close)) = hit_info {
                    if let Some(g) = engine.editor_groups.get_mut(&group_id) {
                        g.active_tab = tab_idx;
                    }
                    engine.active_group = group_id;
                    engine.line_annotations.clear();
                    if is_close {
                        if engine.dirty() {
                            *close_tab_confirm = true;
                        } else {
                            engine.close_tab();
                        }
                    } else {
                        engine.lsp_ensure_active_buffer();
                        if let Some(path) = engine.file_path().cloned() {
                            sidebar.reveal_path(&path, term_height.saturating_sub(4) as usize);
                        }
                    }
                }
                if !tab_matched {
                    // Calculate diff toolbar zone (label + 3 buttons).
                    let diff_total_cols = if let Some(dt) = diff_toolbar_ref {
                        let label_cols = dt
                            .change_label
                            .as_ref()
                            .map(|l| l.len() as u16 + 1)
                            .unwrap_or(0);
                        DIFF_TOOLBAR_BTN_COLS + label_cols
                    } else {
                        0
                    };
                    // Split buttons exist on active group, or all groups in diff mode.
                    let had_split = was_active || engine.is_in_diff_view();
                    let split_cols = if had_split { TAB_SPLIT_BOTH_COLS } else { 0 };
                    let split_end = bar_width;
                    let split_start = split_end.saturating_sub(split_cols);
                    let diff_end = split_start;
                    let diff_start = diff_end.saturating_sub(diff_total_cols);
                    // Hit-test diff toolbar buttons FIRST (they sit left of
                    // split buttons, so check them before split to avoid
                    // boundary overlap).
                    if diff_total_cols > 0 && local_col >= diff_start && local_col < diff_end {
                        // Hit-test diff toolbar buttons (prev, next, fold).
                        // Layout: [label][prev][next][fold].
                        let in_diff = local_col - diff_start;
                        let label_cols = diff_total_cols - DIFF_TOOLBAR_BTN_COLS;
                        let in_btns = in_diff.saturating_sub(label_cols);
                        let has_win = engine.windows.contains_key(&engine.active_window_id());
                        if in_diff < label_cols {
                            // Clicked on the label — no-op.
                        } else if in_btns < DIFF_BTN_COLS {
                            if has_win {
                                engine.jump_prev_hunk();
                            }
                        } else if in_btns < DIFF_BTN_COLS * 2 {
                            if has_win {
                                engine.jump_next_hunk();
                            }
                        } else {
                            engine.diff_toggle_hide_unchanged();
                        }
                    } else if had_split
                        && local_col >= split_start
                        && bar_width >= TAB_SPLIT_BOTH_COLS
                    {
                        // Hit-test split buttons (rightmost).
                        let in_split = local_col - split_start;
                        if in_split >= TAB_SPLIT_BTN_COLS {
                            engine.open_editor_group(SplitDirection::Horizontal);
                        } else {
                            engine.open_editor_group(SplitDirection::Vertical);
                        }
                    }
                }
                return sidebar_width;
            }
        }
        // Single group: check top tab bar row only.
        if row == menu_rows && layout.editor_group_split.is_none() {
            let editor_col_width = terminal_size
                .map(|s| s.width)
                .unwrap_or(80)
                .saturating_sub(editor_left);
            let bar_width = editor_col_width;
            let local_col = rel_col;
            let mut x: u16 = 0;
            let mut tab_matched = false;
            for (i, tab) in layout.tab_bar.iter().enumerate() {
                let name_width = tab.name.chars().count() as u16;
                let tab_width = name_width + TAB_CLOSE_COLS;
                if local_col >= x && local_col < x + tab_width {
                    tab_matched = true;
                    if i < engine.active_group().tabs.len() {
                        let close_col = x + name_width;
                        if local_col >= close_col {
                            engine.active_group_mut().active_tab = i;
                            engine.line_annotations.clear();
                            if engine.dirty() {
                                *close_tab_confirm = true;
                            } else {
                                engine.close_tab();
                            }
                        } else {
                            engine.active_group_mut().active_tab = i;
                            engine.line_annotations.clear();
                            engine.lsp_ensure_active_buffer();
                            if let Some(path) = engine.file_path().cloned() {
                                sidebar.reveal_path(&path, term_height.saturating_sub(4) as usize);
                            }
                        }
                    }
                    break;
                }
                x += tab_width;
            }
            if !tab_matched {
                let diff_total_cols = if let Some(dt) = layout.diff_toolbar.as_ref() {
                    let label_cols = dt
                        .change_label
                        .as_ref()
                        .map(|l| l.len() as u16 + 1)
                        .unwrap_or(0);
                    DIFF_TOOLBAR_BTN_COLS + label_cols
                } else {
                    0
                };
                let split_end = bar_width;
                let split_start = split_end.saturating_sub(TAB_SPLIT_BOTH_COLS);
                let diff_end = split_start;
                let diff_start = diff_end.saturating_sub(diff_total_cols);
                // Check diff toolbar FIRST to avoid boundary overlap with split buttons.
                if diff_total_cols > 0 && local_col >= diff_start && local_col < diff_end {
                    let in_diff = local_col - diff_start;
                    let label_cols = diff_total_cols - DIFF_TOOLBAR_BTN_COLS;
                    let in_btns = in_diff.saturating_sub(label_cols);
                    let has_win = engine.windows.contains_key(&engine.active_window_id());
                    if in_diff < label_cols {
                        // Clicked on label — no-op.
                    } else if in_btns < DIFF_BTN_COLS {
                        if has_win {
                            engine.jump_prev_hunk();
                        }
                    } else if in_btns < DIFF_BTN_COLS * 2 {
                        if has_win {
                            engine.jump_next_hunk();
                        }
                    } else {
                        engine.diff_toggle_hide_unchanged();
                    }
                } else if local_col >= split_start && bar_width >= TAB_SPLIT_BOTH_COLS {
                    let in_split = local_col - split_start;
                    if in_split >= TAB_SPLIT_BTN_COLS {
                        engine.open_editor_group(SplitDirection::Horizontal);
                    } else {
                        engine.open_editor_group(SplitDirection::Vertical);
                    }
                }
            }
            return sidebar_width;
        }
    }

    let rel_col = col - editor_left;
    // editor_row is 0-indexed relative to the editor content area.
    // Window rects already include the tab_bar_height offset (y >= 1),
    // so we only subtract menu_rows here (not the tab bar row).
    let editor_row = row.saturating_sub(menu_rows);

    // ── Group divider click — start drag ──────────────────────────────────────
    if let Some(layout) = last_layout {
        if let Some(ref split) = layout.editor_group_split {
            for div in &split.dividers {
                let hit = match div.direction {
                    crate::core::window::SplitDirection::Vertical => {
                        let div_col = div.position.round() as u16;
                        rel_col == div_col
                            && (editor_row as f64) >= div.cross_start
                            && (editor_row as f64) < div.cross_start + div.cross_size
                    }
                    crate::core::window::SplitDirection::Horizontal => {
                        let div_row = div.position.round() as u16;
                        editor_row == div_row
                            && (rel_col as f64) >= div.cross_start
                            && (rel_col as f64) < div.cross_start + div.cross_size
                    }
                };
                if hit {
                    *dragging_group_divider = Some(div.split_index);
                    return sidebar_width;
                }
            }
        }
    }

    if let Some(layout) = last_layout {
        for rw in &layout.windows {
            let wx = rw.rect.x as u16;
            let wy = rw.rect.y as u16;
            let ww = rw.rect.width as u16;
            let wh = rw.rect.height as u16;

            if rel_col >= wx && rel_col < wx + ww && editor_row >= wy && editor_row < wy + wh {
                let viewport_lines = wh as usize;
                let has_v_scrollbar = rw.total_lines > viewport_lines;
                let gutter = rw.gutter_char_width as u16;
                let viewport_cols = (ww as usize)
                    .saturating_sub(gutter as usize + if has_v_scrollbar { 1 } else { 0 });
                let has_h_scrollbar = rw.max_col > viewport_cols && wh > 1;

                // Vertical scrollbar click/drag-start (rightmost column)
                if has_v_scrollbar && rel_col == wx + ww - 1 {
                    // menu_rows = menu bar offset; wy already includes tab_bar_height
                    let track_abs_start = menu_rows + wy;
                    // If there's also a h-scrollbar, v-track is 1 row shorter
                    let track_len = if has_h_scrollbar {
                        wh.saturating_sub(1)
                    } else {
                        wh
                    };
                    *dragging_scrollbar = Some(ScrollDragState {
                        window_id: rw.window_id,
                        is_horizontal: false,
                        track_abs_start,
                        track_len,
                        total: rw.total_lines,
                    });
                    let track_rel_row = editor_row.saturating_sub(wy);
                    let ratio = track_rel_row as f64 / track_len as f64;
                    let new_top = (ratio * rw.total_lines as f64) as usize;
                    engine.set_cursor_for_window(rw.window_id, new_top, 0);
                    engine.ensure_cursor_visible();
                    engine.sync_scroll_binds();
                    return sidebar_width;
                }

                // Horizontal scrollbar click/drag-start (bottom row)
                if has_h_scrollbar && editor_row == wy + wh - 1 {
                    let track_x = wx + gutter;
                    let track_w = ww.saturating_sub(gutter + if has_v_scrollbar { 1 } else { 0 });
                    if rel_col >= track_x && rel_col < track_x + track_w && track_w > 0 {
                        let track_abs_start = editor_left + track_x;
                        *dragging_scrollbar = Some(ScrollDragState {
                            window_id: rw.window_id,
                            is_horizontal: true,
                            track_abs_start,
                            track_len: track_w,
                            total: rw.max_col,
                        });
                        let ratio = (rel_col - track_x) as f64 / track_w as f64;
                        let new_left = (ratio * rw.max_col as f64) as usize;
                        engine.set_scroll_left_for_window(rw.window_id, new_left);
                        return sidebar_width;
                    }
                }

                // Check gutter area
                let view_row = (editor_row - wy) as usize;
                if gutter > 0 && rel_col >= wx && rel_col < wx + gutter {
                    if let Some(rl) = rw.lines.get(view_row) {
                        let gutter_col = (rel_col - wx) as usize;
                        let bp_offset: usize = if rw.has_breakpoints { 1 } else { 0 };
                        let git_col = if rw.has_git_diff {
                            bp_offset
                        } else {
                            usize::MAX
                        };

                        if rw.has_breakpoints && gutter_col == 0 {
                            // Breakpoint column (leftmost).
                            let file = engine
                                .windows
                                .get(&rw.window_id)
                                .and_then(|w| engine.buffer_manager.get(w.buffer_id))
                                .and_then(|bs| bs.file_path.as_ref())
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let bp_line = rl.line_idx as u64 + 1;
                            engine.dap_toggle_breakpoint(&file, bp_line);
                        } else if gutter_col == git_col {
                            // Git diff column — open diff peek popup.
                            engine.active_tab_mut().active_window = rw.window_id;
                            engine.view_mut().cursor.line = rl.line_idx;
                            engine.open_diff_peek();
                        } else {
                            let has_fold_indicator =
                                rl.gutter_text.chars().any(|c| c == '+' || c == '-');
                            if has_fold_indicator {
                                engine.toggle_fold_at_line(rl.line_idx);
                            }
                        }
                    }
                    return sidebar_width;
                }
                // Text area click — fold/wrap-aware row → buffer line mapping
                let clicked_rl = rw.lines.get(view_row);
                let buf_line = clicked_rl
                    .map(|l| l.line_idx)
                    .unwrap_or_else(|| rw.scroll_top + view_row);
                // For wrapped lines, add segment_col_offset so the click
                // targets the correct column within the full buffer line.
                let seg_offset = clicked_rl.map(|l| l.segment_col_offset).unwrap_or(0);
                let col_in_text = (rel_col - wx - gutter) as usize + rw.scroll_left + seg_offset;

                // Double-click detection
                let now = Instant::now();
                let is_double = now.duration_since(*last_click_time) < Duration::from_millis(400)
                    && *last_click_pos == (col, row);
                *last_click_time = now;
                *last_click_pos = (col, row);

                if ev.modifiers.contains(KeyModifiers::CONTROL)
                    || (ev.modifiers.contains(KeyModifiers::ALT) && engine.is_vscode_mode())
                {
                    engine.add_cursor_at_pos(buf_line, col_in_text);
                } else if is_double {
                    engine.mouse_double_click(rw.window_id, buf_line, col_in_text);
                } else {
                    // Clear selection on click in VSCode mode.
                    if engine.is_vscode_mode() {
                        engine.vscode_clear_selection();
                    }
                    engine.mouse_click(rw.window_id, buf_line, col_in_text);
                }
                // Fire cursor_move hook so plugins (e.g. git-insights blame) see
                // the new cursor position after a mouse click on a buffer line.
                engine.fire_cursor_move_hook();
                return sidebar_width;
            }
        }
    }

    sidebar_width
}

// ─── Screen layout bridging ───────────────────────────────────────────────────

fn build_screen_for_tui(
    engine: &Engine,
    theme: &Theme,
    area: Rect,
    sidebar: &TuiSidebar,
    sidebar_width: u16,
) -> render::ScreenLayout {
    // Global bottom rows: status(1) + cmd(1).  The tab bar row is included in
    // content_bounds and handled by calculate_group_window_rects (tab_bar_height=1).
    // Must match draw_frame's vertical layout exactly.
    let qf_height: u16 = if engine.quickfix_open { 6 } else { 0 };
    let bottom_panel_open = engine.terminal_open || engine.bottom_panel_open;
    let term_height: u16 = if bottom_panel_open {
        engine.session.terminal_panel_rows + 2 // 1 tab bar row + 1 header row + content
    } else {
        0
    };
    let menu_height: u16 = if engine.menu_bar_visible { 1 } else { 0 };
    let dbg_height: u16 = if engine.debug_toolbar_visible { 1 } else { 0 };
    let wildmenu_height: u16 = if !engine.wildmenu_items.is_empty() {
        1
    } else {
        0
    };
    let content_rows = area
        .height
        .saturating_sub(2 + qf_height + term_height + menu_height + dbg_height + wildmenu_height); // status + cmd + panels
    let sidebar_cols = if sidebar.visible {
        sidebar_width + 1
    } else {
        0
    }; // +1 sep
    let ab_width = if engine.settings.autohide_panels && !sidebar.visible {
        0
    } else {
        ACTIVITY_BAR_WIDTH
    };
    let content_cols = area.width.saturating_sub(ab_width + sidebar_cols);
    let content_bounds = WindowRect::new(0.0, 0.0, content_cols as f64, content_rows as f64);
    let tui_tab_bar_height = if engine.settings.breadcrumbs {
        2.0
    } else {
        1.0
    };
    let (window_rects, _dividers) =
        engine.calculate_group_window_rects(content_bounds, tui_tab_bar_height);
    debug_log!(
        "build_screen: content_rows={} content_cols={} groups={} window_rects={}",
        content_rows,
        content_cols,
        engine.group_layout.leaf_count(),
        window_rects.len()
    );
    for (wid, r) in &window_rects {
        debug_log!(
            "  window {:?}: x={:.1} y={:.1} w={:.1} h={:.1}",
            wid,
            r.x,
            r.y,
            r.width,
            r.height
        );
    }
    build_screen_layout(engine, theme, &window_rects, 1.0, 1.0, true)
}

// ─── Frame rendering ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn draw_frame(
    frame: &mut ratatui::Frame,
    screen: &render::ScreenLayout,
    theme: &Theme,
    sidebar: &mut TuiSidebar,
    engine: &Engine,
    sidebar_prompt: &Option<SidebarPrompt>,
    sidebar_width: u16,
    fuzzy_scroll_top: usize,
    grep_scroll_top: usize,
    quickfix_scroll_top: usize,
    debug_output_scroll: usize,
    folder_picker: Option<&FolderPickerState>,
    quit_confirm: bool,
    close_tab_confirm: bool,
    cmd_sel: Option<(usize, usize)>,
    explorer_drop_target: Option<usize>,
) {
    let area = frame.size();

    // ── Global vertical split: [menu] / [main] / [qf?] / [tabs?] / [term?] / [dbg?] / [status] / [cmd] ──
    let qf_height: u16 = if screen.quickfix.is_some() { 6 } else { 0 };
    let terminal_open = screen.bottom_tabs.terminal.is_some();
    // Show the bottom panel when terminal is open OR when Debug Output tab is
    // active and there are lines to display (DAP diagnostic output).
    let debug_output_open = engine.bottom_panel_kind == render::BottomPanelKind::DebugOutput
        && !screen.bottom_tabs.output_lines.is_empty();
    let bottom_panel_open = terminal_open || debug_output_open;
    // 1 tab bar row + 1 header row + content rows
    let bottom_panel_height: u16 = if bottom_panel_open {
        engine.session.terminal_panel_rows + 2
    } else {
        0
    };
    let menu_bar_height: u16 = if screen.menu_bar.is_some() { 1 } else { 0 };
    let debug_toolbar_height: u16 = if screen.debug_toolbar.is_some() { 1 } else { 0 };
    let wildmenu_height: u16 = if screen.wildmenu.is_some() { 1 } else { 0 };
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(menu_bar_height),
            Constraint::Min(0),
            Constraint::Length(qf_height),
            Constraint::Length(bottom_panel_height),
            Constraint::Length(debug_toolbar_height),
            Constraint::Length(wildmenu_height),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
    let menu_bar_area = v_chunks[0];
    let main_area = v_chunks[1];
    let quickfix_area = v_chunks[2];
    let bottom_panel_area = v_chunks[3];
    let debug_toolbar_area = v_chunks[4];
    let wildmenu_area = v_chunks[5];
    let status_area = v_chunks[6];
    let cmd_area = v_chunks[7];

    // ── Horizontal split of main_area: [activity_bar] [sidebar?] [editor_col] ─
    let ab_width = if engine.settings.autohide_panels && !sidebar.visible {
        0
    } else {
        ACTIVITY_BAR_WIDTH
    };
    let sidebar_constraint = if sidebar.visible {
        Constraint::Length(sidebar_width + 1) // +1 for separator
    } else {
        Constraint::Length(0)
    };
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(ab_width),
            sidebar_constraint,
            Constraint::Min(0),
        ])
        .split(main_area);
    let activity_area = h_chunks[0];
    let sidebar_sep_area = h_chunks[1];
    let editor_col = h_chunks[2];

    // The editor column includes the tab bar row(s).  Window rects from
    // calculate_group_window_rects already have y >= 1 (tab_bar_height offset),
    // so the tab bar occupies row 0 and windows start at row 1 automatically.
    let editor_area = editor_col;

    // ── Render menu bar strip (if visible) ───────────────────────────────────
    if let Some(ref menu_data) = screen.menu_bar {
        render_menu_bar(frame.buffer_mut(), menu_bar_area, menu_data, theme);
        // Note: dropdown is rendered LAST (after all content) so it draws on top.
    }

    // ── Render activity bar ───────────────────────────────────────────────────
    render_activity_bar(
        frame.buffer_mut(),
        activity_area,
        sidebar,
        theme,
        engine.menu_bar_visible,
        engine,
    );

    // ── Render sidebar + separator ────────────────────────────────────────────
    if sidebar.visible && sidebar_sep_area.width > 1 {
        let sidebar_area = Rect {
            x: sidebar_sep_area.x,
            y: sidebar_sep_area.y,
            width: sidebar_sep_area.width - 1,
            height: sidebar_sep_area.height,
        };
        let sep_x = sidebar_sep_area.x + sidebar_sep_area.width - 1;

        render_sidebar(
            frame.buffer_mut(),
            sidebar_area,
            sidebar,
            engine,
            theme,
            explorer_drop_target,
        );
        // Note: render_sidebar / render_search_panel write back scroll_top to sidebar

        // Separator column
        let sep_fg = rc(theme.separator);
        let sep_bg = rc(theme.background);
        for y in sidebar_sep_area.y..sidebar_sep_area.y + sidebar_sep_area.height {
            set_cell(frame.buffer_mut(), sep_x, y, '│', sep_fg, sep_bg);
        }
    }

    // ── Render editor ─────────────────────────────────────────────────────────
    if let Some(ref split) = screen.editor_group_split {
        debug_log!(
            "draw_frame split: editor_area=({},{},{}x{}) groups={}",
            editor_area.x,
            editor_area.y,
            editor_area.width,
            editor_area.height,
            split.group_tab_bars.len()
        );
        for (idx, gtb) in split.group_tab_bars.iter().enumerate() {
            debug_log!(
                "  group[{}] id={:?} bounds=({:.1},{:.1},{:.1}x{:.1}) tabs={}",
                idx,
                gtb.group_id,
                gtb.bounds.x,
                gtb.bounds.y,
                gtb.bounds.width,
                gtb.bounds.height,
                gtb.tabs.len()
            );
        }
        // Render windows first so tab bars draw on top (prevents window content
        // from overwriting an adjacent group's tab bar in horizontal splits).
        render_all_windows(frame, editor_area, &screen.windows, theme);
        // Draw each group's tab bar.  Tab bar sits tab_bar_height rows above
        // the group's window content (bounds.y - tab_bar_height).
        let tui_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
        for gtb in split.group_tab_bars.iter() {
            let tab_x = gtb.bounds.x as u16 + editor_area.x;
            let tab_w = gtb.bounds.width as u16;
            let is_active = gtb.group_id == split.active_group;
            // In diff mode, show split buttons on all groups so clicking
            // an inactive group's toolbar doesn't cause a visual shift.
            let show_split = is_active || engine.is_in_diff_view();
            if tab_w > 0 {
                let bar_y = editor_area.y + (gtb.bounds.y as u16).saturating_sub(tui_tbh);
                let g_tab = Rect {
                    x: tab_x,
                    y: bar_y,
                    width: tab_w,
                    height: 1,
                };
                render_tab_bar(
                    frame.buffer_mut(),
                    g_tab,
                    &gtb.tabs,
                    theme,
                    show_split,
                    gtb.diff_toolbar.as_ref(),
                );
            }
        }
        // Draw breadcrumb bars (below each group's tab bar).
        for bc in &screen.breadcrumbs {
            if bc.segments.is_empty() {
                continue;
            }
            let bc_x = bc.bounds.x as u16 + editor_area.x;
            let bc_w = bc.bounds.width as u16;
            // Breadcrumb bar is one row above the window content (bounds.y - 1 in
            // breadcrumb coordinates, which is one row below the tab bar).
            let bc_y = editor_area.y + bc.bounds.y as u16;
            // In multi-group with breadcrumbs, bounds.y points to the breadcrumb row
            // (tab_bar_height=2 means row 0=tab, row 1=breadcrumb, row 2+=windows).
            // The breadcrumb bounds.y is window min_y, so the bc sits 1 above.
            let bc_y = bc_y.saturating_sub(1);
            if bc_w > 0 {
                let bc_rect = Rect {
                    x: bc_x,
                    y: bc_y,
                    width: bc_w,
                    height: 1,
                };
                render_breadcrumb_bar(frame.buffer_mut(), bc_rect, &bc.segments, theme);
            }
        }
        // Draw divider lines (vertical only — horizontal splits use the tab bar as divider).
        let sep_fg = rc(theme.separator);
        let sep_bg = rc(theme.background);
        for div in &split.dividers {
            if div.direction == SplitDirection::Vertical {
                let div_x = editor_area.x + div.position as u16;
                let y_start = editor_area.y + div.cross_start as u16;
                let y_end = y_start + div.cross_size as u16;
                for y in y_start..y_end {
                    if div_x < editor_area.x + editor_area.width {
                        set_cell(frame.buffer_mut(), div_x, y, '│', sep_fg, sep_bg);
                    }
                }
            }
        }
    } else {
        // Single group: tab bar at row 0 of editor_area, windows at row 1+.
        let tab_rect = Rect {
            x: editor_area.x,
            y: editor_area.y,
            width: editor_area.width,
            height: 1,
        };
        render_tab_bar(
            frame.buffer_mut(),
            tab_rect,
            &screen.tab_bar,
            theme,
            true,
            screen.diff_toolbar.as_ref(),
        );
        // Draw breadcrumb bar for the single group.
        if let Some(bc) = screen.breadcrumbs.first() {
            if !bc.segments.is_empty() {
                let bc_rect = Rect {
                    x: editor_area.x,
                    y: editor_area.y + 1,
                    width: editor_area.width,
                    height: 1,
                };
                render_breadcrumb_bar(frame.buffer_mut(), bc_rect, &bc.segments, theme);
            }
        }
        render_all_windows(frame, editor_area, &screen.windows, theme);
    }

    // ── Completion popup (rendered on top of editor) ───────────────────────
    if let Some(ref menu) = screen.completion {
        if let Some(active_win) = screen
            .windows
            .iter()
            .find(|w| w.window_id == screen.active_window_id)
        {
            if let Some((cursor_pos, _)) = &active_win.cursor {
                let gutter_w = active_win.gutter_char_width as u16;
                let win_x = editor_area.x + active_win.rect.x as u16;
                let win_y = editor_area.y + active_win.rect.y as u16;
                let raw = active_win
                    .lines
                    .get(cursor_pos.view_line)
                    .map(|l| l.raw_text.as_str())
                    .unwrap_or("");
                let vis_col = char_col_to_visual(raw, cursor_pos.col, active_win.tabstop)
                    .saturating_sub(active_win.scroll_left) as u16;
                let popup_x = win_x + gutter_w + vis_col;
                let popup_y = win_y + cursor_pos.view_line as u16 + 1;
                render_completion_popup(frame, menu, popup_x, popup_y, frame.size(), theme);
            }
        }
    }

    // ── Hover popup (rendered on top of editor) ──────────────────────────────
    if let Some(ref hover) = screen.hover {
        if let Some(active_win) = screen
            .windows
            .iter()
            .find(|w| w.window_id == screen.active_window_id)
        {
            let gutter_w = active_win.gutter_char_width as u16;
            let win_x = editor_area.x + active_win.rect.x as u16;
            let win_y = editor_area.y + active_win.rect.y as u16;
            let anchor_view = hover.anchor_line.saturating_sub(active_win.scroll_top) as u16;
            let vis_col = hover.anchor_col.saturating_sub(active_win.scroll_left) as u16;
            let popup_x = win_x + gutter_w + vis_col;
            let popup_y = win_y + anchor_view;
            render_hover_popup(frame, hover, popup_x, popup_y, frame.size(), theme);
        }
    }

    // ── Diff peek popup (inline git hunk preview) ──────────────────────────
    if let Some(ref peek) = screen.diff_peek {
        if let Some(active_win) = screen
            .windows
            .iter()
            .find(|w| w.window_id == screen.active_window_id)
        {
            let gutter_w = active_win.gutter_char_width as u16;
            let win_x = editor_area.x + active_win.rect.x as u16;
            let win_y = editor_area.y + active_win.rect.y as u16;
            let anchor_view = peek.anchor_line.saturating_sub(active_win.scroll_top) as u16;
            let popup_x = win_x + gutter_w;
            let popup_y = win_y + anchor_view + 1; // below anchor line
            render_diff_peek_popup(frame, peek, popup_x, popup_y, frame.size(), theme);
        }
    }

    // ── Signature-help popup (shown in insert mode when cursor is inside a call) ─
    if let Some(ref sig) = screen.signature_help {
        if let Some(active_win) = screen
            .windows
            .iter()
            .find(|w| w.window_id == screen.active_window_id)
        {
            let gutter_w = active_win.gutter_char_width as u16;
            let win_x = editor_area.x + active_win.rect.x as u16;
            let win_y = editor_area.y + active_win.rect.y as u16;
            let anchor_view = sig.anchor_line.saturating_sub(active_win.scroll_top) as u16;
            let vis_col = sig.anchor_col.saturating_sub(active_win.scroll_left) as u16;
            let popup_x = win_x + gutter_w + vis_col;
            let popup_y = win_y + anchor_view;
            render_signature_popup(frame, sig, popup_x, popup_y, frame.size(), theme);
        }
    }

    // ── Fuzzy file-picker modal (rendered on top of everything) ───────────────
    if let Some(ref fuzzy) = screen.fuzzy {
        render_fuzzy_popup(frame, fuzzy, area, theme, fuzzy_scroll_top);
    }

    // ── Folder / workspace picker modal ──────────────────────────────────────
    if let Some(picker) = folder_picker {
        render_folder_picker(frame, picker, area, theme);
    }

    // ── Live grep modal (rendered on top of everything) ───────────────────────
    if let Some(ref grep) = screen.live_grep {
        render_live_grep_popup(frame, grep, area, theme, grep_scroll_top);
    }

    // ── Command palette modal (rendered on top of everything) ─────────────────
    if let Some(ref palette) = screen.command_palette {
        render_command_palette_popup(frame, palette, area, theme);
    }

    // ── Tab switcher popup ───────────────────────────────────────────────────
    if let Some(ref ts) = screen.tab_switcher {
        render_tab_switcher_popup(frame.buffer_mut(), area, ts, theme);
    }

    // ── Quickfix panel (persistent bottom strip) ──────────────────────────────
    if let Some(ref qf) = screen.quickfix {
        render_quickfix_panel(
            frame.buffer_mut(),
            quickfix_area,
            qf,
            quickfix_scroll_top,
            theme,
        );
    }

    // ── Bottom panel (tab bar + terminal or debug output) ────────────────────
    if bottom_panel_area.height > 0 {
        // Tab bar (first row)
        let tab_bar_area = Rect {
            x: bottom_panel_area.x,
            y: bottom_panel_area.y,
            width: bottom_panel_area.width,
            height: 1,
        };
        let content_area = Rect {
            x: bottom_panel_area.x,
            y: bottom_panel_area.y + 1,
            width: bottom_panel_area.width,
            height: bottom_panel_area.height.saturating_sub(1),
        };
        render_bottom_panel_tabs(
            frame.buffer_mut(),
            tab_bar_area,
            engine.bottom_panel_kind.clone(),
            theme,
        );
        match engine.bottom_panel_kind {
            render::BottomPanelKind::Terminal => {
                if let Some(ref term) = screen.bottom_tabs.terminal {
                    render_terminal_panel(frame.buffer_mut(), content_area, term, theme);
                }
            }
            render::BottomPanelKind::DebugOutput => {
                render_debug_output(
                    frame.buffer_mut(),
                    content_area,
                    &screen.bottom_tabs.output_lines,
                    debug_output_scroll,
                    theme,
                );
            }
        }
    }

    // ── Debug toolbar strip (if visible) ────────────────────────────────────
    if let Some(ref toolbar) = screen.debug_toolbar {
        render_debug_toolbar(frame.buffer_mut(), debug_toolbar_area, toolbar, theme);
    }

    // ── Wildmenu bar (command Tab completion) ─────────────────────────────────
    if let Some(ref wm) = screen.wildmenu {
        render_wildmenu(frame.buffer_mut(), wildmenu_area, wm, theme);
    }

    // ── Status / command ──────────────────────────────────────────────────────
    render_status_line(
        frame.buffer_mut(),
        status_area,
        &screen.status_left,
        &screen.status_right,
        theme,
    );

    if let Some(prompt) = sidebar_prompt {
        let (prefix, input_cursor) = match &prompt.kind {
            PromptKind::NewFile(_) => ("New file: ".to_string(), prompt.cursor),
            PromptKind::NewFolder(_) => ("New folder: ".to_string(), prompt.cursor),
            PromptKind::DeleteConfirm(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                (format!("Delete '{}'? (y/n)", name), 0)
            }
            PromptKind::MoveFile(_) => ("Move to: ".to_string(), prompt.cursor),
        };
        let prompt_text = format!("{}{}", prefix, prompt.input);
        // Cursor position in rendered chars: prefix char count + cursor char count
        let cursor_char_pos = prefix.chars().count() + prompt.input[..input_cursor].chars().count();
        render_prompt_line(
            frame.buffer_mut(),
            cmd_area,
            &prompt_text,
            cursor_char_pos,
            theme,
        );
    } else {
        render_command_line(frame.buffer_mut(), cmd_area, &screen.command, theme);
        // Highlight command-line mouse selection (invert fg/bg for selected cells)
        if let Some((start, end)) = cmd_sel {
            let lo = start.min(end);
            let hi = start.max(end);
            let buf = frame.buffer_mut();
            for i in lo..=hi {
                let cx = cmd_area.x + i as u16;
                if cx < cmd_area.x + cmd_area.width {
                    let cell = buf.get_mut(cx, cmd_area.y);
                    let old_fg = cell.fg;
                    let old_bg = cell.bg;
                    cell.set_fg(old_bg).set_bg(old_fg);
                }
            }
        }
    }

    // ── Context menu popup (above status/command line) ─────────────────────
    if let Some(ref ctx_menu) = screen.context_menu {
        render_context_menu(frame.buffer_mut(), area, ctx_menu, theme);
    }

    // ── Modal dialog (highest z-order after quit confirm) ────────────────────
    if let Some(ref dialog) = screen.dialog {
        render_dialog_popup(frame.buffer_mut(), area, dialog, theme);
    }

    // ── Menu dropdown — rendered last so it draws on top of everything ────────
    if let Some(ref menu_data) = screen.menu_bar {
        if menu_data.open_menu_idx.is_some() {
            render_menu_dropdown(frame.buffer_mut(), area, menu_data, theme);
        }
    }

    // ── Quit confirm overlay — rendered on top of absolutely everything ───────
    if quit_confirm {
        render_quit_confirm_overlay(frame.buffer_mut(), area, theme);
    }

    // ── Close-tab confirm overlay ──────────────────────────────────────────────
    if close_tab_confirm {
        render_close_tab_confirm_overlay(frame.buffer_mut(), area, theme);
    }
}

// ─── Sidebar CRUD handling ────────────────────────────────────────────────────

fn handle_sidebar_prompt(
    engine: &mut Engine,
    sidebar: &mut TuiSidebar,
    kind: PromptKind,
    input: String,
    viewport_height: usize,
) {
    match kind {
        PromptKind::NewFile(target_dir) => {
            let name = input.trim();
            if !name.is_empty() {
                let path = target_dir.join(name);
                if let Err(e) = fs::write(&path, "") {
                    engine.message = format!("Error creating file: {}", e);
                } else {
                    sidebar.reveal_path(&path, viewport_height);
                    if let Err(e) = engine.open_file_with_mode(&path, OpenMode::Permanent) {
                        engine.message = e;
                    }
                }
            }
        }
        PromptKind::NewFolder(target_dir) => {
            let name = input.trim();
            if !name.is_empty() {
                let path = target_dir.join(name);
                if let Err(e) = fs::create_dir_all(&path) {
                    engine.message = format!("Error creating folder: {}", e);
                } else {
                    sidebar.reveal_path(&path, viewport_height);
                }
            }
        }
        PromptKind::DeleteConfirm(path) => {
            if input == "y" {
                let result = if path.is_dir() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                };
                if let Err(e) = result {
                    engine.message = format!("Error deleting: {}", e);
                } else {
                    sidebar.build_rows();
                }
            }
        }
        PromptKind::MoveFile(src) => {
            let dest_str = input.trim();
            if !dest_str.is_empty() {
                // Resolve destination relative to project root
                let dest = if std::path::Path::new(dest_str).is_absolute() {
                    std::path::PathBuf::from(dest_str)
                } else {
                    sidebar.root.join(dest_str)
                };
                match engine.move_file(&src, &dest) {
                    Ok(()) => {
                        // engine.move_file resolves the final path; figure out
                        // the actual destination for reveal_path.
                        let final_dest = if dest.is_dir() {
                            dest.join(src.file_name().unwrap_or_default())
                        } else {
                            dest.clone()
                        };
                        sidebar.reveal_path(&final_dest, viewport_height);
                        engine.message = format!("Moved to '{}'", final_dest.display());
                    }
                    Err(e) => {
                        engine.message = e;
                    }
                }
            }
        }
    }
}

// ─── Cell helper ──────────────────────────────────────────────────────────────

/// Set a single buffer cell, bounds-checking against the buffer's area.
fn set_cell(buf: &mut ratatui::buffer::Buffer, x: u16, y: u16, ch: char, fg: RColor, bg: RColor) {
    let area = buf.area;
    if x < area.x + area.width && y < area.y + area.height {
        buf.get_mut(x, y).set_char(ch).set_fg(fg).set_bg(bg);
    }
}

/// Set a buffer cell with a 2-wide character (e.g. Nerd Font glyph), resetting
/// the following cell so ratatui knows it's a continuation of the wide char.
fn set_cell_wide(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    ch: char,
    fg: RColor,
    bg: RColor,
) {
    let area = buf.area;
    if x < area.x + area.width && y < area.y + area.height {
        // Use set_string which correctly handles double-width characters
        // (measures width via unicode-segmentation, resets continuation cells).
        // Private Use Area glyphs (Nerd Font) report width=1 to unicode_width
        // but render as 2 columns in the terminal, so we write the glyph as a
        // string and explicitly skip the next cell to prevent ratatui's diff
        // algorithm from emitting it (which would overwrite the glyph's second
        // column).
        let mut s = String::with_capacity(4);
        s.push(ch);
        buf.get_mut(x, y).set_symbol(&s).set_fg(fg).set_bg(bg);
        if x + 1 < area.x + area.width {
            let next = buf.get_mut(x + 1, y);
            next.reset();
            next.set_skip(true);
        }
    }
}

fn set_cell_styled(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    ch: char,
    fg: RColor,
    bg: RColor,
    modifier: Modifier,
) {
    let area = buf.area;
    if x < area.x + area.width && y < area.y + area.height {
        let cell = buf.get_mut(x, y);
        cell.set_char(ch).set_fg(fg).set_bg(bg);
        cell.modifier = modifier;
    }
}

// ─── Tab bar ──────────────────────────────────────────────────────────────────

/// Close-tab × button character (shown on every tab).
const TAB_CLOSE_CHAR: char = '×'; // U+00D7 MULTIPLICATION SIGN
/// Terminal columns used by each tab's close button (the × itself + trailing space).
const TAB_CLOSE_COLS: u16 = 2;

// Split button glyphs: \u{F0932} (split-right), \u{F0931} (split-down).
/// Terminal columns occupied by each split button (1 space + 2-wide NF glyph).
const TAB_SPLIT_BTN_COLS: u16 = 3;
/// Total columns reserved for both split buttons.
const TAB_SPLIT_BOTH_COLS: u16 = TAB_SPLIT_BTN_COLS * 2;

/// Terminal columns per diff toolbar button (1 space + 1 char + 1 space).
const DIFF_BTN_COLS: u16 = 3;
/// Total columns for all three diff toolbar buttons.
const DIFF_TOOLBAR_BTN_COLS: u16 = DIFF_BTN_COLS * 3;

fn render_tab_bar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    tabs: &[render::TabInfo],
    theme: &Theme,
    show_split_btns: bool,
    diff_toolbar: Option<&render::DiffToolbarData>,
) {
    let bar_bg = rc(theme.tab_bar_bg);

    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', bar_bg, bar_bg);
    }

    // Calculate total reserved columns at the right edge.
    let diff_cols = if diff_toolbar.is_some() {
        // 3 buttons + up to 6 chars for label like "2 of 5" + 1 space
        let label_cols = diff_toolbar
            .and_then(|d| d.change_label.as_ref())
            .map(|l| l.len() as u16 + 1)
            .unwrap_or(0);
        DIFF_TOOLBAR_BTN_COLS + label_cols
    } else {
        0
    };
    let split_cols = if show_split_btns {
        TAB_SPLIT_BOTH_COLS
    } else {
        0
    };
    let reserved = diff_cols + split_cols;

    // Reserve columns at the right edge for buttons.
    let tab_end = if area.width >= reserved {
        area.x + area.width - reserved
    } else {
        area.x + area.width
    };

    let mut x = area.x;
    for tab in tabs {
        let (fg, bg) = match (tab.active, tab.preview) {
            (true, true) => (rc(theme.tab_preview_active_fg), rc(theme.tab_active_bg)),
            (true, false) => (rc(theme.tab_active_fg), rc(theme.tab_active_bg)),
            (false, true) => (rc(theme.tab_preview_inactive_fg), rc(theme.tab_bar_bg)),
            (false, false) => (rc(theme.tab_inactive_fg), rc(theme.tab_bar_bg)),
        };
        let modifier = if tab.preview {
            Modifier::ITALIC
        } else {
            Modifier::empty()
        };

        for ch in tab.name.chars() {
            if x >= tab_end {
                break;
            }
            set_cell_styled(buf, x, area.y, ch, fg, bg, modifier);
            x += 1;
        }
        // Show ● (modified dot) when dirty, × otherwise (VSCode style).
        if x < tab_end {
            let (close_ch, close_fg) = if tab.dirty {
                ('●', rc(theme.foreground))
            } else if tab.active {
                (TAB_CLOSE_CHAR, rc(theme.tab_active_fg))
            } else {
                (TAB_CLOSE_CHAR, rc(theme.separator))
            };
            set_cell(buf, x, area.y, close_ch, close_fg, bg);
            x += 1;
        }
        // Trailing separator space.
        if x < tab_end {
            set_cell(buf, x, area.y, ' ', bar_bg, bar_bg);
            x += 1;
        }
    }

    // Draw diff toolbar buttons (to the left of split buttons).
    if let Some(dt) = diff_toolbar {
        if area.width >= reserved {
            let mut bx = area.x + area.width - reserved;
            let btn_fg = rc(theme.tab_inactive_fg);
            let active_fg = rc(theme.tab_active_fg);
            // Change label (e.g. "2/5")
            if let Some(label) = &dt.change_label {
                let label_fg = rc(theme.foreground);
                set_cell(buf, bx, area.y, ' ', label_fg, bar_bg);
                bx += 1;
                for ch in label.chars() {
                    set_cell(buf, bx, area.y, ch, label_fg, bar_bg);
                    bx += 1;
                }
            }
            // Prev button (space + 2-wide NF glyph = 3 cols)
            set_cell(buf, bx, area.y, ' ', btn_fg, bar_bg);
            set_cell_wide(buf, bx + 1, area.y, '\u{F0143}', btn_fg, bar_bg);
            bx += DIFF_BTN_COLS;
            // Next button
            set_cell(buf, bx, area.y, ' ', btn_fg, bar_bg);
            set_cell_wide(buf, bx + 1, area.y, '\u{F0140}', btn_fg, bar_bg);
            bx += DIFF_BTN_COLS;
            // Fold toggle button (highlighted when active)
            let fold_fg = if dt.unchanged_hidden {
                active_fg
            } else {
                btn_fg
            };
            set_cell(buf, bx, area.y, ' ', fold_fg, bar_bg);
            set_cell_wide(buf, bx + 1, area.y, '\u{F0233}', fold_fg, bar_bg);
            bx += DIFF_BTN_COLS;
        }
    }

    // Draw split-right then split-down buttons at the right edge.
    if show_split_btns && area.width >= split_cols {
        let btn_fg = rc(theme.tab_inactive_fg);
        let mut bx = area.x + area.width - split_cols;
        // Split-right button (space + 2-wide NF glyph = 3 cols)
        set_cell(buf, bx, area.y, ' ', btn_fg, bar_bg);
        set_cell_wide(buf, bx + 1, area.y, '\u{F0932}', btn_fg, bar_bg);
        bx += TAB_SPLIT_BTN_COLS;
        // Split-down button
        set_cell(buf, bx, area.y, ' ', btn_fg, bar_bg);
        set_cell_wide(buf, bx + 1, area.y, '\u{F0931}', btn_fg, bar_bg);
    }
}

fn render_breadcrumb_bar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    segments: &[render::BreadcrumbSegment],
    theme: &Theme,
) {
    let bg = rc(theme.breadcrumb_bg);
    // Fill the row with breadcrumb bg
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', bg, bg);
    }

    let separator = " \u{203A} "; // " › "
    let mut x = area.x + 1; // small left padding

    for seg in segments {
        // Separator before all but the first
        if x > area.x + 2 {
            let sep_fg = rc(theme.breadcrumb_fg);
            for ch in separator.chars() {
                if x >= area.x + area.width {
                    return;
                }
                set_cell(buf, x, area.y, ch, sep_fg, bg);
                x += 1;
            }
        }

        // Segment label
        let fg = if seg.is_last {
            rc(theme.breadcrumb_active_fg)
        } else {
            rc(theme.breadcrumb_fg)
        };
        for ch in seg.label.chars() {
            if x >= area.x + area.width {
                return;
            }
            set_cell(buf, x, area.y, ch, fg, bg);
            x += 1;
        }
    }
}

// ─── Editor windows ───────────────────────────────────────────────────────────

fn render_all_windows(
    frame: &mut ratatui::Frame,
    editor_area: Rect,
    windows: &[RenderedWindow],
    theme: &Theme,
) {
    for window in windows {
        let win_rect = Rect {
            x: editor_area.x + window.rect.x as u16,
            y: editor_area.y + window.rect.y as u16,
            width: window.rect.width as u16,
            height: window.rect.height as u16,
        };
        render_window(frame, win_rect, window, theme);
    }
    render_separators(frame.buffer_mut(), editor_area, windows, theme);
}

fn render_completion_popup(
    frame: &mut ratatui::Frame,
    menu: &CompletionMenu,
    popup_x: u16,
    popup_y: u16,
    term_area: Rect,
    theme: &Theme,
) {
    let visible = menu.candidates.len().min(10) as u16;
    if visible == 0 {
        return;
    }
    let width = (menu.max_width as u16 + 4).max(12);

    // Clamp so popup doesn't go off the right/bottom edge
    let x = popup_x.min(term_area.width.saturating_sub(width));
    let y = popup_y.min(term_area.height.saturating_sub(visible));

    let bg_color = rc(theme.completion_bg);
    let sel_bg_color = rc(theme.completion_selected_bg);
    let fg_color = rc(theme.completion_fg);
    let border_color = rc(theme.completion_border);

    let buf = frame.buffer_mut();
    for (i, candidate) in menu.candidates.iter().enumerate().take(visible as usize) {
        let row_y = y + i as u16;
        let row_bg = if i == menu.selected_idx {
            sel_bg_color
        } else {
            bg_color
        };
        // Fill the row background
        for col in 0..width {
            let cell_x = x + col;
            if cell_x < term_area.width && row_y < term_area.height {
                let cell = buf.get_mut(cell_x, row_y);
                cell.set_bg(row_bg).set_fg(fg_color);
                // Draw border chars on leftmost/rightmost or blank fill
                let ch = if col == 0 || col == width - 1 {
                    '│'
                } else {
                    ' '
                };
                cell.set_char(ch).set_fg(border_color);
            }
        }
        // Render candidate text starting at col 1
        let display = format!(" {}", candidate);
        for (j, ch) in display.chars().enumerate() {
            let cell_x = x + 1 + j as u16;
            if cell_x + 1 < x + width && cell_x < term_area.width && row_y < term_area.height {
                let cell = buf.get_mut(cell_x, row_y);
                cell.set_char(ch).set_fg(fg_color).set_bg(row_bg);
            }
        }
    }
}

fn render_hover_popup(
    frame: &mut ratatui::Frame,
    hover: &render::HoverPopup,
    popup_x: u16,
    popup_y: u16,
    term_area: Rect,
    theme: &Theme,
) {
    let text_lines: Vec<&str> = hover.text.lines().collect();
    let num_lines = text_lines.len().min(20) as u16;
    if num_lines == 0 {
        return;
    }
    let max_len = text_lines.iter().map(|l| l.len()).max().unwrap_or(10);
    let width = (max_len as u16 + 4).max(12);

    // Place above cursor if possible, otherwise below
    let y = if popup_y > num_lines {
        popup_y - num_lines
    } else {
        popup_y + 1
    };

    // Clamp to screen bounds
    let x = popup_x.min(term_area.width.saturating_sub(width));
    let y = y.min(term_area.height.saturating_sub(num_lines));

    let bg_color = rc(theme.hover_bg);
    let fg_color = rc(theme.hover_fg);
    let border_color = rc(theme.hover_border);

    let buf = frame.buffer_mut();
    for (i, text_line) in text_lines.iter().enumerate().take(num_lines as usize) {
        let row_y = y + i as u16;
        // Fill row background
        for col in 0..width {
            let cell_x = x + col;
            if cell_x < term_area.width && row_y < term_area.height {
                let cell = buf.get_mut(cell_x, row_y);
                cell.set_bg(bg_color);
                let ch = if col == 0 || col == width - 1 {
                    '│'
                } else {
                    ' '
                };
                cell.set_char(ch).set_fg(border_color);
            }
        }
        // Render text starting at col 1
        let display = format!(" {}", text_line);
        for (j, ch) in display.chars().enumerate() {
            let cell_x = x + 1 + j as u16;
            if cell_x + 1 < x + width && cell_x < term_area.width && row_y < term_area.height {
                let cell = buf.get_mut(cell_x, row_y);
                cell.set_char(ch).set_fg(fg_color).set_bg(bg_color);
            }
        }
    }
}

fn render_diff_peek_popup(
    frame: &mut ratatui::Frame,
    peek: &render::DiffPeekPopup,
    popup_x: u16,
    popup_y: u16,
    term_area: Rect,
    theme: &Theme,
) {
    let action_bar_lines = 1_u16;
    let num_lines = (peek.hunk_lines.len() as u16 + action_bar_lines).min(30);
    if num_lines == 0 {
        return;
    }
    let max_len = peek.hunk_lines.iter().map(|l| l.len()).max().unwrap_or(10);
    let width = (max_len as u16 + 4).max(20);

    // Clamp to screen bounds.
    let x = popup_x.min(term_area.width.saturating_sub(width));
    let y = popup_y.min(term_area.height.saturating_sub(num_lines));

    let bg_color = rc(theme.hover_bg);
    let fg_color = rc(theme.hover_fg);
    let border_color = rc(theme.hover_border);
    let added_fg = rc(theme.git_added);
    let deleted_fg = rc(theme.git_deleted);

    let buf = frame.buffer_mut();

    // Draw diff lines.
    for (i, hline) in peek.hunk_lines.iter().enumerate().take(29) {
        let row_y = y + i as u16;
        if row_y >= term_area.height {
            break;
        }
        // Fill background.
        for col in 0..width {
            let cell_x = x + col;
            if cell_x < term_area.width {
                let cell = buf.get_mut(cell_x, row_y);
                cell.set_bg(bg_color);
                let ch = if col == 0 || col == width - 1 {
                    '│'
                } else {
                    ' '
                };
                cell.set_char(ch).set_fg(border_color);
            }
        }
        // Render text.
        let line_fg = if hline.starts_with('+') {
            added_fg
        } else if hline.starts_with('-') {
            deleted_fg
        } else {
            fg_color
        };
        let display = format!(" {}", hline);
        for (j, ch) in display.chars().enumerate() {
            let cell_x = x + 1 + j as u16;
            if cell_x + 1 < x + width && cell_x < term_area.width {
                buf.get_mut(cell_x, row_y)
                    .set_char(ch)
                    .set_fg(line_fg)
                    .set_bg(bg_color);
            }
        }
    }

    // Action bar at bottom.
    let action_row = y + peek.hunk_lines.len().min(29) as u16;
    if action_row < term_area.height {
        // Fill background.
        for col in 0..width {
            let cell_x = x + col;
            if cell_x < term_area.width {
                let cell = buf.get_mut(cell_x, action_row);
                cell.set_bg(bg_color);
                let ch = if col == 0 || col == width - 1 {
                    '│'
                } else {
                    ' '
                };
                cell.set_char(ch).set_fg(border_color);
            }
        }
        let labels = ["[s] Stage", "[r] Revert", "[q] Close"];
        let mut cx = x + 2;
        for label in &labels {
            for ch in label.chars() {
                if cx + 1 < x + width && cx < term_area.width {
                    buf.get_mut(cx, action_row)
                        .set_char(ch)
                        .set_fg(fg_color)
                        .set_bg(bg_color);
                }
                cx += 1;
            }
            cx += 2; // spacing between labels
        }
    }
}

fn render_signature_popup(
    frame: &mut ratatui::Frame,
    sig: &render::SignatureHelp,
    popup_x: u16,
    popup_y: u16,
    term_area: Rect,
    theme: &Theme,
) {
    let label = &sig.label;
    if label.is_empty() {
        return;
    }
    let display = format!(" {} ", label);
    let width = (display.len() as u16 + 2).max(12);

    // Place above the cursor line if possible, otherwise below.
    let y = if popup_y > 1 {
        popup_y - 1
    } else {
        popup_y + 1
    };
    let x = popup_x.min(term_area.width.saturating_sub(width));
    let y = y.min(term_area.height.saturating_sub(1));

    let bg_color = rc(theme.hover_bg);
    let fg_color = rc(theme.hover_fg);
    let kw_color = rc(theme.keyword);
    let border_color = rc(theme.hover_border);

    // Compute which char indices are in the active parameter (byte → char mapping).
    let active_char_range: Option<(usize, usize)> = sig.active_param.and_then(|idx| {
        sig.params.get(idx).map(|&(start_byte, end_byte)| {
            let char_start = label[..start_byte].chars().count() + 1; // +1 for leading space
            let char_end = label[..end_byte].chars().count() + 1;
            (char_start, char_end)
        })
    });

    let buf = frame.buffer_mut();
    // Draw background row
    for col in 0..width {
        let cell_x = x + col;
        if cell_x < term_area.width && y < term_area.height {
            let cell = buf.get_mut(cell_x, y);
            cell.set_bg(bg_color);
            let ch = if col == 0 || col == width - 1 {
                '│'
            } else {
                ' '
            };
            cell.set_char(ch).set_fg(border_color);
        }
    }
    // Draw each character of the display string with appropriate color.
    for (j, ch) in display.chars().enumerate() {
        let cell_x = x + 1 + j as u16;
        if cell_x + 1 < x + width && cell_x < term_area.width && y < term_area.height {
            let in_active = active_char_range
                .map(|(s, e)| j >= s && j < e)
                .unwrap_or(false);
            let color = if in_active { kw_color } else { fg_color };
            let cell = buf.get_mut(cell_x, y);
            cell.set_char(ch).set_fg(color).set_bg(bg_color);
        }
    }
}

fn render_fuzzy_popup(
    frame: &mut ratatui::Frame,
    fuzzy: &render::FuzzyPanel,
    term_area: Rect,
    theme: &Theme,
    scroll_top: usize,
) {
    let term_cols = term_area.width;
    let term_rows = term_area.height;

    // Size: 3/5 of terminal width (min 50), 55% of terminal rows (min 15)
    let width = (term_cols * 3 / 5).max(50);
    let height = (term_rows * 55 / 100).max(15);

    // Centered
    let x = (term_cols.saturating_sub(width)) / 2;
    let y = (term_rows.saturating_sub(height)) / 2;

    let bg_color = rc(theme.fuzzy_bg);
    let sel_bg_color = rc(theme.fuzzy_selected_bg);
    let fg_color = rc(theme.fuzzy_fg);
    let query_fg = rc(theme.fuzzy_query_fg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);

    let buf = frame.buffer_mut();

    // Row 0: top border ╭─ Find Files ── N/M ──╮
    let title_text = format!(
        " Find Files  {}/{} ",
        fuzzy.results.len(),
        fuzzy.total_files
    );
    for col in 0..width {
        let cx = x + col;
        if cx < term_area.width && y < term_area.height {
            let ch = if col == 0 {
                '╭'
            } else if col == width - 1 {
                '╮'
            } else {
                '─'
            };
            set_cell(buf, cx, y, ch, border_fg, bg_color);
        }
    }
    // Overlay title text starting at col 2
    for (i, ch) in title_text.chars().enumerate() {
        let cx = x + 2 + i as u16;
        if cx + 1 < x + width && cx < term_area.width && y < term_area.height {
            set_cell(buf, cx, y, ch, title_fg, bg_color);
        }
    }

    // Row 1: query line │ > query_ │
    let row1 = y + 1;
    if row1 < term_area.height {
        // Left border
        set_cell(buf, x, row1, '│', border_fg, bg_color);
        // Right border
        if x + width - 1 < term_area.width {
            set_cell(buf, x + width - 1, row1, '│', border_fg, bg_color);
        }
        // Fill background
        for col in 1..width - 1 {
            let cx = x + col;
            if cx < term_area.width {
                set_cell(buf, cx, row1, ' ', fg_color, bg_color);
            }
        }
        // Query text "> query"
        let query_display = format!("> {}", fuzzy.query);
        for (i, ch) in query_display.chars().enumerate() {
            let cx = x + 1 + i as u16;
            if cx + 1 < x + width && cx < term_area.width {
                set_cell(buf, cx, row1, ch, query_fg, bg_color);
            }
        }
        // Cursor block after query
        let cursor_col = x + 1 + query_display.chars().count() as u16;
        if cursor_col + 1 < x + width && cursor_col < term_area.width {
            set_cell(buf, cursor_col, row1, '▌', query_fg, bg_color);
        }
    }

    // Row 2: separator ├───────┤
    let row2 = y + 2;
    if row2 < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '├'
                } else if col == width - 1 {
                    '┤'
                } else {
                    '─'
                };
                set_cell(buf, cx, row2, ch, border_fg, bg_color);
            }
        }
    }

    // Result rows: rows 3..height-1
    let results_start = y + 3;
    let results_end = y + height - 1;
    let visible_rows = (results_end.saturating_sub(results_start)) as usize;

    for row_idx in 0..visible_rows {
        let result_idx = scroll_top + row_idx;
        let ry = results_start + row_idx as u16;
        if ry >= results_end || ry >= term_area.height {
            break;
        }
        // Left/right border
        set_cell(buf, x, ry, '│', border_fg, bg_color);
        if x + width - 1 < term_area.width {
            set_cell(buf, x + width - 1, ry, '│', border_fg, bg_color);
        }
        // Fill row background
        let is_selected = result_idx == fuzzy.selected_idx;
        let row_bg = if is_selected { sel_bg_color } else { bg_color };
        for col in 1..width - 1 {
            let cx = x + col;
            if cx < term_area.width {
                set_cell(buf, cx, ry, ' ', fg_color, row_bg);
            }
        }
        // Result text
        if let Some(display) = fuzzy.results.get(result_idx) {
            let prefix = if is_selected { "▶ " } else { "  " };
            let row_text = format!("{}{}", prefix, display);
            for (j, ch) in row_text.chars().enumerate() {
                let cx = x + 1 + j as u16;
                if cx + 1 < x + width && cx < term_area.width {
                    set_cell(buf, cx, ry, ch, fg_color, row_bg);
                }
            }
        }
    }

    // Bottom border ╰───────╯
    let bottom = y + height - 1;
    if bottom < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '╰'
                } else if col == width - 1 {
                    '╯'
                } else {
                    '─'
                };
                set_cell(buf, cx, bottom, ch, border_fg, bg_color);
            }
        }
    }
}

fn render_folder_picker(
    frame: &mut ratatui::Frame,
    picker: &FolderPickerState,
    term_area: Rect,
    theme: &Theme,
) {
    let term_cols = term_area.width;
    let term_rows = term_area.height;

    // Same proportions as the fuzzy popup
    let width = (term_cols * 3 / 5).max(50);
    let height = (term_rows * 55 / 100).max(15);
    let x = (term_cols.saturating_sub(width)) / 2;
    let y = (term_rows.saturating_sub(height)) / 2;

    let bg_color = rc(theme.fuzzy_bg);
    let sel_bg_color = rc(theme.fuzzy_selected_bg);
    let fg_color = rc(theme.fuzzy_fg);
    let query_fg = rc(theme.fuzzy_query_fg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);

    let buf = frame.buffer_mut();

    // Title varies by mode; for folder modes show the current root for orientation
    let root_display = if picker.mode != FolderPickerMode::OpenRecent {
        let r = picker.root.to_string_lossy();
        // Truncate from left if too long
        let max = (width as usize).saturating_sub(30).max(10);
        if r.len() > max {
            format!("…{}", &r[r.len() - max..])
        } else {
            r.into_owned()
        }
    } else {
        String::new()
    };
    let title_text = match picker.mode {
        FolderPickerMode::OpenFolder => format!(
            " Open Folder {}  {}/{} ",
            root_display,
            picker.filtered.len(),
            picker.all_entries.len()
        ),
        FolderPickerMode::OpenRecent => format!(" Open Recent  {} ", picker.filtered.len()),
    };

    // Row 0: top border ╭─ Title ── N/M ──╮
    for col in 0..width {
        let cx = x + col;
        if cx < term_area.width && y < term_area.height {
            let ch = if col == 0 {
                '╭'
            } else if col == width - 1 {
                '╮'
            } else {
                '─'
            };
            set_cell(buf, cx, y, ch, border_fg, bg_color);
        }
    }
    for (i, ch) in title_text.chars().enumerate() {
        let cx = x + 2 + i as u16;
        if cx + 1 < x + width && cx < term_area.width && y < term_area.height {
            set_cell(buf, cx, y, ch, title_fg, bg_color);
        }
    }

    // Row 1: query line │ > query_ │
    let row1 = y + 1;
    if row1 < term_area.height {
        set_cell(buf, x, row1, '│', border_fg, bg_color);
        if x + width - 1 < term_area.width {
            set_cell(buf, x + width - 1, row1, '│', border_fg, bg_color);
        }
        for col in 1..width - 1 {
            let cx = x + col;
            if cx < term_area.width {
                set_cell(buf, cx, row1, ' ', fg_color, bg_color);
            }
        }
        let query_display = format!("> {}", picker.query);
        for (i, ch) in query_display.chars().enumerate() {
            let cx = x + 1 + i as u16;
            if cx + 1 < x + width && cx < term_area.width {
                set_cell(buf, cx, row1, ch, query_fg, bg_color);
            }
        }
        let cursor_col = x + 1 + query_display.chars().count() as u16;
        if cursor_col + 1 < x + width && cursor_col < term_area.width {
            set_cell(buf, cursor_col, row1, '▌', query_fg, bg_color);
        }
    }

    // Row 2: separator ├───────┤
    let row2 = y + 2;
    if row2 < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '├'
                } else if col == width - 1 {
                    '┤'
                } else {
                    '─'
                };
                set_cell(buf, cx, row2, ch, border_fg, bg_color);
            }
        }
    }

    // Result rows
    let results_start = y + 3;
    let results_end = y + height - 1;
    let visible_rows = (results_end.saturating_sub(results_start)) as usize;

    for row_idx in 0..visible_rows {
        let result_idx = picker.scroll_top + row_idx;
        let ry = results_start + row_idx as u16;
        if ry >= results_end || ry >= term_area.height {
            break;
        }
        set_cell(buf, x, ry, '│', border_fg, bg_color);
        if x + width - 1 < term_area.width {
            set_cell(buf, x + width - 1, ry, '│', border_fg, bg_color);
        }
        let is_selected = result_idx == picker.selected;
        let row_bg = if is_selected { sel_bg_color } else { bg_color };
        for col in 1..width - 1 {
            let cx = x + col;
            if cx < term_area.width {
                set_cell(buf, cx, ry, ' ', fg_color, row_bg);
            }
        }
        if let Some(entry) = picker.filtered.get(result_idx) {
            // Show workspace files differently with a marker
            let display = entry.to_string_lossy();
            let is_workspace = entry
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == ".vimcode-workspace")
                .unwrap_or(false);
            let prefix = if is_selected { "▶ " } else { "  " };
            let marker = if is_workspace { "⚙ " } else { "📁 " };
            let row_text = format!("{}{}{}", prefix, marker, display);
            for (j, ch) in row_text.chars().enumerate() {
                let cx = x + 1 + j as u16;
                if cx + 1 < x + width && cx < term_area.width {
                    set_cell(buf, cx, ry, ch, fg_color, row_bg);
                }
            }
        }
    }

    // Bottom border ╰───────╯
    let bottom = y + height - 1;
    if bottom < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '╰'
                } else if col == width - 1 {
                    '╯'
                } else {
                    '─'
                };
                set_cell(buf, cx, bottom, ch, border_fg, bg_color);
            }
        }
    }
}

fn render_live_grep_popup(
    frame: &mut ratatui::Frame,
    grep: &render::LiveGrepPanel,
    term_area: Rect,
    theme: &Theme,
    scroll_top: usize,
) {
    let term_cols = term_area.width;
    let term_rows = term_area.height;

    // Size: 4/5 of terminal width (min 60), 65% of terminal rows (min 18)
    let width = (term_cols * 4 / 5).max(60);
    let height = (term_rows * 65 / 100).max(18);

    // Centered
    let x = (term_cols.saturating_sub(width)) / 2;
    let y = (term_rows.saturating_sub(height)) / 2;

    let bg_color = rc(theme.fuzzy_bg);
    let sel_bg_color = rc(theme.fuzzy_selected_bg);
    let fg_color = rc(theme.fuzzy_fg);
    let query_fg = rc(theme.fuzzy_query_fg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);

    let buf = frame.buffer_mut();

    // Row 0: top border ╭─ Live Grep ── N matches ──╮
    let title_text = format!(" Live Grep  {} matches ", grep.total_matches);
    for col in 0..width {
        let cx = x + col;
        if cx < term_area.width && y < term_area.height {
            let ch = if col == 0 {
                '╭'
            } else if col == width - 1 {
                '╮'
            } else {
                '─'
            };
            set_cell(buf, cx, y, ch, border_fg, bg_color);
        }
    }
    // Overlay title text starting at col 2
    for (i, ch) in title_text.chars().enumerate() {
        let cx = x + 2 + i as u16;
        if cx + 1 < x + width && cx < term_area.width && y < term_area.height {
            set_cell(buf, cx, y, ch, title_fg, bg_color);
        }
    }

    // Row 1: query line │ > query_ │
    let row1 = y + 1;
    if row1 < term_area.height {
        set_cell(buf, x, row1, '│', border_fg, bg_color);
        if x + width - 1 < term_area.width {
            set_cell(buf, x + width - 1, row1, '│', border_fg, bg_color);
        }
        for col in 1..width - 1 {
            let cx = x + col;
            if cx < term_area.width {
                set_cell(buf, cx, row1, ' ', fg_color, bg_color);
            }
        }
        let query_display = format!("> {}", grep.query);
        for (i, ch) in query_display.chars().enumerate() {
            let cx = x + 1 + i as u16;
            if cx + 1 < x + width && cx < term_area.width {
                set_cell(buf, cx, row1, ch, query_fg, bg_color);
            }
        }
        let cursor_col = x + 1 + query_display.chars().count() as u16;
        if cursor_col + 1 < x + width && cursor_col < term_area.width {
            set_cell(buf, cursor_col, row1, '▌', query_fg, bg_color);
        }
    }

    // Row 2: separator ├───────────────┬───────────────────────────────┤
    let row2 = y + 2;
    // Left pane width: 35% of popup (in columns)
    let left_w = (width as usize * 35 / 100) as u16;
    if row2 < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '├'
                } else if col == width - 1 {
                    '┤'
                } else if col == left_w {
                    '┬'
                } else {
                    '─'
                };
                set_cell(buf, cx, row2, ch, border_fg, bg_color);
            }
        }
    }

    // Result rows: rows 3..height-1
    let results_start = y + 3;
    let results_end = y + height - 1;
    let visible_rows = (results_end.saturating_sub(results_start)) as usize;

    for row_idx in 0..visible_rows {
        let result_idx = scroll_top + row_idx;
        let ry = results_start + row_idx as u16;
        if ry >= results_end || ry >= term_area.height {
            break;
        }

        // Left border
        set_cell(buf, x, ry, '│', border_fg, bg_color);
        // Vertical separator between panes
        if x + left_w < term_area.width {
            set_cell(buf, x + left_w, ry, '│', border_fg, bg_color);
        }
        // Right border
        if x + width - 1 < term_area.width {
            set_cell(buf, x + width - 1, ry, '│', border_fg, bg_color);
        }

        // Fill left pane background
        let is_selected = result_idx == grep.selected_idx;
        let left_bg = if is_selected { sel_bg_color } else { bg_color };
        for col in 1..left_w {
            let cx = x + col;
            if cx < term_area.width {
                set_cell(buf, cx, ry, ' ', fg_color, left_bg);
            }
        }
        // Fill right pane background
        for col in (left_w + 1)..(width - 1) {
            let cx = x + col;
            if cx < term_area.width {
                set_cell(buf, cx, ry, ' ', fg_color, bg_color);
            }
        }

        // Left pane: result text
        if let Some(display) = grep.results.get(result_idx) {
            let prefix = if is_selected { "▶" } else { " " };
            let row_text = format!("{}{}", prefix, display);
            let left_inner = left_w.saturating_sub(1) as usize; // inner cols
            for (j, ch) in row_text.chars().enumerate().take(left_inner) {
                let cx = x + 1 + j as u16;
                if cx < x + left_w && cx < term_area.width {
                    set_cell(buf, cx, ry, ch, fg_color, left_bg);
                }
            }
        }

        // Right pane: preview line for this row_idx
        if let Some((lineno, text, is_match)) = grep.preview_lines.get(row_idx) {
            let preview_text = format!("{:4}: {}", lineno, text);
            let preview_fg = if *is_match { title_fg } else { fg_color };
            let right_start = x + left_w + 1;
            let right_inner = (width - left_w - 2) as usize;
            for (j, ch) in preview_text.chars().enumerate().take(right_inner) {
                let cx = right_start + j as u16;
                if cx + 1 < x + width && cx < term_area.width {
                    set_cell(buf, cx, ry, ch, preview_fg, bg_color);
                }
            }
        }
    }

    // Bottom border ╰───────────────┴───────────────────────────────╯
    let bottom = y + height - 1;
    if bottom < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '╰'
                } else if col == width - 1 {
                    '╯'
                } else if col == left_w {
                    '┴'
                } else {
                    '─'
                };
                set_cell(buf, cx, bottom, ch, border_fg, bg_color);
            }
        }
    }
}

fn render_command_palette_popup(
    frame: &mut ratatui::Frame,
    palette: &render::CommandPalettePanel,
    term_area: Rect,
    theme: &Theme,
) {
    let term_cols = term_area.width;
    let term_rows = term_area.height;

    // Size: 55% of terminal width (min 55), 60% of terminal rows (min 16)
    let width = (term_cols * 55 / 100).max(55);
    let height = (term_rows * 60 / 100).max(16);

    // Centered
    let x = (term_cols.saturating_sub(width)) / 2;
    let y = (term_rows.saturating_sub(height)) / 2;

    let bg_color = rc(theme.fuzzy_bg);
    let sel_bg_color = rc(theme.fuzzy_selected_bg);
    let fg_color = rc(theme.fuzzy_fg);
    let query_fg = rc(theme.fuzzy_query_fg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);

    let buf = frame.buffer_mut();

    // Row 0: top border ╭─ Command Palette ── N/M ──╮
    let title_text = format!(" Command Palette  {} ", palette.items.len());
    for col in 0..width {
        let cx = x + col;
        if cx < term_area.width && y < term_area.height {
            let ch = if col == 0 {
                '╭'
            } else if col == width - 1 {
                '╮'
            } else {
                '─'
            };
            set_cell(buf, cx, y, ch, border_fg, bg_color);
        }
    }
    // Overlay title text starting at col 2
    for (i, ch) in title_text.chars().enumerate() {
        let cx = x + 2 + i as u16;
        if cx + 1 < x + width && cx < term_area.width && y < term_area.height {
            set_cell(buf, cx, y, ch, title_fg, bg_color);
        }
    }

    // Row 1: query line │ > query▌ │
    let row1 = y + 1;
    if row1 < term_area.height {
        set_cell(buf, x, row1, '│', border_fg, bg_color);
        if x + width - 1 < term_area.width {
            set_cell(buf, x + width - 1, row1, '│', border_fg, bg_color);
        }
        for col in 1..width - 1 {
            let cx = x + col;
            if cx < term_area.width {
                set_cell(buf, cx, row1, ' ', fg_color, bg_color);
            }
        }
        let query_display = format!("> {}", palette.query);
        for (i, ch) in query_display.chars().enumerate() {
            let cx = x + 1 + i as u16;
            if cx + 1 < x + width && cx < term_area.width {
                set_cell(buf, cx, row1, ch, query_fg, bg_color);
            }
        }
        let cursor_col = x + 1 + query_display.chars().count() as u16;
        if cursor_col + 1 < x + width && cursor_col < term_area.width {
            set_cell(buf, cursor_col, row1, '▌', query_fg, bg_color);
        }
    }

    // Row 2: separator ├────────────────────────────────────────────────┤
    let row2 = y + 2;
    if row2 < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '├'
                } else if col == width - 1 {
                    '┤'
                } else {
                    '─'
                };
                set_cell(buf, cx, row2, ch, border_fg, bg_color);
            }
        }
    }

    // Result rows (title+query+sep = 3 rows, bottom border = 1 row)
    let inner_rows = height.saturating_sub(4) as usize;
    let total_items = palette.items.len();
    // Scrollbar: use the last inner column when content overflows
    let has_scrollbar = total_items > inner_rows;
    // Inner content width: strip left │, right │, and scrollbar column if present
    let inner_w = width.saturating_sub(2 + if has_scrollbar { 1 } else { 0 }) as usize;
    let visible_count = inner_rows.min(total_items.saturating_sub(palette.scroll_top));

    for i in 0..visible_count {
        let display_idx = palette.scroll_top + i;
        let ry = row2 + 1 + i as u16;
        if ry >= y + height - 1 || ry >= term_area.height {
            break;
        }

        let is_selected = display_idx == palette.selected_idx;

        // Fill row background
        let row_bg = if is_selected { sel_bg_color } else { bg_color };
        set_cell(buf, x, ry, '│', border_fg, bg_color);
        if x + width - 1 < term_area.width {
            set_cell(buf, x + width - 1, ry, '│', border_fg, bg_color);
        }
        // Fill inner content columns (excluding scrollbar column)
        let content_end = width - 1 - if has_scrollbar { 1 } else { 0 };
        for col in 1..content_end {
            let cx = x + col;
            if cx < term_area.width {
                set_cell(buf, cx, ry, ' ', fg_color, row_bg);
            }
        }

        let (label, shortcut) = &palette.items[display_idx];
        let prefix = if is_selected { "▶ " } else { "  " };
        let label_text = format!("{}{}", prefix, label);

        // Draw label (left-aligned)
        for (j, ch) in label_text.chars().enumerate() {
            let cx = x + 1 + j as u16;
            let limit = x + 1 + content_end - 1;
            if cx < limit && cx < term_area.width {
                set_cell(buf, cx, ry, ch, fg_color, row_bg);
            }
        }

        // Draw shortcut (right-aligned within content area, dimmed)
        if !shortcut.is_empty() {
            let sc_with_pad = format!("{}  ", shortcut);
            let sc_len = sc_with_pad.chars().count();
            let sc_start = (inner_w + 1).saturating_sub(sc_len);
            for (j, ch) in sc_with_pad.chars().enumerate() {
                let cx = x + sc_start as u16 + j as u16;
                let limit = x + 1 + content_end - 1;
                if cx < limit && cx < term_area.width {
                    set_cell(buf, cx, ry, ch, border_fg, row_bg);
                }
            }
        }
    }

    // Scrollbar column (between content and right border)
    if has_scrollbar && inner_rows > 0 {
        let sb_col = x + width - 2; // column to the left of right border
        let track_start = (row2 + 1) as usize;
        let track_len = inner_rows;
        let thumb_size = ((inner_rows * inner_rows) / total_items).max(1);
        let max_scroll = total_items.saturating_sub(inner_rows);
        let thumb_offset = if max_scroll > 0 {
            (palette.scroll_top * (track_len.saturating_sub(thumb_size))) / max_scroll
        } else {
            0
        };

        for row_off in 0..track_len {
            let ry = (track_start + row_off) as u16;
            if ry >= y + height - 1 || ry >= term_area.height {
                break;
            }
            let in_thumb = row_off >= thumb_offset && row_off < thumb_offset + thumb_size;
            let sb_char = if in_thumb { '█' } else { '░' };
            if sb_col < term_area.width {
                set_cell(buf, sb_col, ry, sb_char, border_fg, bg_color);
            }
        }
    }

    // Bottom border ╰────────────────────────────────────────────────────╯
    let bottom = y + height - 1;
    if bottom < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '╰'
                } else if col == width - 1 {
                    '╯'
                } else {
                    '─'
                };
                set_cell(buf, cx, bottom, ch, border_fg, bg_color);
            }
        }
    }
}

fn render_tab_switcher_popup(
    buf: &mut ratatui::buffer::Buffer,
    term_area: Rect,
    ts: &render::TabSwitcherPanel,
    theme: &Theme,
) {
    if ts.items.is_empty() {
        return;
    }
    let item_count = ts.items.len();
    // Size: 45% width (min 40, max 80), height = items + 2 (borders)
    let width = (term_area.width * 45 / 100).clamp(40, 80);
    let max_visible = (term_area.height as usize).saturating_sub(4).min(20);
    let visible = item_count.min(max_visible);
    let height = visible as u16 + 2; // top + bottom border

    // Centered
    let x = (term_area.width.saturating_sub(width)) / 2;
    let y = (term_area.height.saturating_sub(height)) / 2;

    let bg = rc(theme.fuzzy_bg);
    let fg = rc(theme.fuzzy_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);

    // Top border
    if y < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '╭'
                } else if col == width - 1 {
                    '╮'
                } else {
                    '─'
                };
                set_cell(buf, cx, y, ch, border_fg, bg);
            }
        }
        // Title overlay
        let title = " Open Tabs ";
        for (i, ch) in title.chars().enumerate() {
            let cx = x + 2 + i as u16;
            if cx + 1 < x + width && cx < term_area.width {
                set_cell(buf, cx, y, ch, title_fg, bg);
            }
        }
    }

    // Scroll offset so selected item is always visible
    let scroll = if ts.selected_idx >= visible {
        ts.selected_idx - visible + 1
    } else {
        0
    };

    // Items
    let inner_w = (width - 2) as usize;
    for i in 0..visible {
        let item_idx = scroll + i;
        if item_idx >= item_count {
            break;
        }
        let ry = y + 1 + i as u16;
        if ry >= term_area.height {
            break;
        }
        let is_selected = item_idx == ts.selected_idx;
        let row_bg = if is_selected { sel_bg } else { bg };

        // Clear row
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 || col == width - 1 {
                    '│'
                } else {
                    ' '
                };
                let c = if col == 0 || col == width - 1 {
                    border_fg
                } else {
                    fg
                };
                set_cell(
                    buf,
                    cx,
                    ry,
                    ch,
                    c,
                    if col == 0 || col == width - 1 {
                        bg
                    } else {
                        row_bg
                    },
                );
            }
        }

        let (name, path, dirty) = &ts.items[item_idx];
        let dirty_mark = if *dirty { " ●" } else { "" };
        let prefix = if is_selected { "▶ " } else { "  " };
        let label = format!("{}{}{}", prefix, name, dirty_mark);

        // Draw label
        for (j, ch) in label.chars().enumerate() {
            let cx = x + 1 + j as u16;
            if cx + 1 < x + width && cx < term_area.width {
                set_cell(buf, cx, ry, ch, fg, row_bg);
            }
        }

        // Draw path right-aligned (dimmed)
        if !path.is_empty() && inner_w > label.chars().count() + 4 {
            let available = inner_w - label.chars().count() - 2;
            let display_path = if path.len() > available {
                &path[path.len() - available..]
            } else {
                path.as_str()
            };
            let path_start = inner_w - display_path.len();
            for (j, ch) in display_path.chars().enumerate() {
                let cx = x + 1 + (path_start + j) as u16;
                if cx + 1 < x + width && cx < term_area.width {
                    set_cell(buf, cx, ry, ch, border_fg, row_bg);
                }
            }
        }
    }

    // Bottom border
    let bottom = y + height - 1;
    if bottom < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '╰'
                } else if col == width - 1 {
                    '╯'
                } else {
                    '─'
                };
                set_cell(buf, cx, bottom, ch, border_fg, bg);
            }
        }
    }
}

fn render_quit_confirm_overlay(buf: &mut ratatui::buffer::Buffer, term_area: Rect, theme: &Theme) {
    // Lines of content (title, blank, message, blank, 3 options, blank, bottom)
    let lines: &[(&str, bool)] = &[
        ("  You have unsaved changes.", false),
        ("", false),
        ("  [S]   Save All & Quit", true),
        ("  [Q]   Quit Without Saving", true),
        ("  [Esc] Cancel", true),
    ];
    let title = " Unsaved Changes ";
    let width: u16 = 42;
    // top border + blank + content rows + blank + bottom border
    let height: u16 = 2 + 1 + lines.len() as u16 + 1;

    let x = (term_area.width.saturating_sub(width)) / 2;
    let y = (term_area.height.saturating_sub(height)) / 2;

    let bg_color = rc(theme.fuzzy_bg);
    let fg_color = rc(theme.fuzzy_fg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);
    let key_fg = rc(theme.fuzzy_query_fg);

    // Top border row ╭─ Unsaved Changes ─╮
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 {
            '╭'
        } else if col == width - 1 {
            '╮'
        } else {
            '─'
        };
        set_cell(buf, cx, y, ch, border_fg, bg_color);
    }
    // Overlay title
    for (i, ch) in title.chars().enumerate() {
        let cx = x + 2 + i as u16;
        if cx + 1 < x + width {
            set_cell(buf, cx, y, ch, title_fg, bg_color);
        }
    }

    // Blank row after title
    let blank_row = y + 1;
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 || col == width - 1 {
            '│'
        } else {
            ' '
        };
        let fg = if col == 0 || col == width - 1 {
            border_fg
        } else {
            fg_color
        };
        set_cell(buf, cx, blank_row, ch, fg, bg_color);
    }

    // Content rows
    for (row_i, (text, is_key_row)) in lines.iter().enumerate() {
        let ry = y + 2 + row_i as u16;
        for col in 0..width {
            let cx = x + col;
            let ch = if col == 0 || col == width - 1 {
                '│'
            } else {
                ' '
            };
            let fg = if col == 0 || col == width - 1 {
                border_fg
            } else {
                fg_color
            };
            set_cell(buf, cx, ry, ch, fg, bg_color);
        }
        let row_fg = if *is_key_row { key_fg } else { fg_color };
        for (j, ch) in text.chars().enumerate() {
            let cx = x + 1 + j as u16;
            if cx + 1 < x + width {
                set_cell(buf, cx, ry, ch, row_fg, bg_color);
            }
        }
    }

    // Blank row before bottom border
    let pre_bottom = y + 2 + lines.len() as u16;
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 || col == width - 1 {
            '│'
        } else {
            ' '
        };
        let fg = if col == 0 || col == width - 1 {
            border_fg
        } else {
            fg_color
        };
        set_cell(buf, cx, pre_bottom, ch, fg, bg_color);
    }

    // Bottom border ╰──────╯
    let bottom = y + height - 1;
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 {
            '╰'
        } else if col == width - 1 {
            '╯'
        } else {
            '─'
        };
        set_cell(buf, cx, bottom, ch, border_fg, bg_color);
    }
}

fn render_close_tab_confirm_overlay(
    buf: &mut ratatui::buffer::Buffer,
    term_area: Rect,
    theme: &Theme,
) {
    let lines: &[(&str, bool)] = &[
        ("  This file has unsaved changes.", false),
        ("", false),
        ("  [S]   Save & Close Tab", true),
        ("  [D]   Discard & Close Tab", true),
        ("  [Esc] Cancel", true),
    ];
    let title = " Unsaved Changes ";
    let width: u16 = 42;
    let height: u16 = 2 + 1 + lines.len() as u16 + 1;

    let x = (term_area.width.saturating_sub(width)) / 2;
    let y = (term_area.height.saturating_sub(height)) / 2;

    let bg_color = rc(theme.fuzzy_bg);
    let fg_color = rc(theme.fuzzy_fg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);
    let key_fg = rc(theme.fuzzy_query_fg);

    // Top border row
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 {
            '╭'
        } else if col == width - 1 {
            '╮'
        } else {
            '─'
        };
        set_cell(buf, cx, y, ch, border_fg, bg_color);
    }
    for (i, ch) in title.chars().enumerate() {
        let cx = x + 2 + i as u16;
        if cx + 1 < x + width {
            set_cell(buf, cx, y, ch, title_fg, bg_color);
        }
    }

    // Blank row after title
    let blank_row = y + 1;
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 || col == width - 1 {
            '│'
        } else {
            ' '
        };
        let fg = if col == 0 || col == width - 1 {
            border_fg
        } else {
            fg_color
        };
        set_cell(buf, cx, blank_row, ch, fg, bg_color);
    }

    // Content rows
    for (row_i, (text, is_key_row)) in lines.iter().enumerate() {
        let ry = y + 2 + row_i as u16;
        for col in 0..width {
            let cx = x + col;
            let ch = if col == 0 || col == width - 1 {
                '│'
            } else {
                ' '
            };
            let fg = if col == 0 || col == width - 1 {
                border_fg
            } else {
                fg_color
            };
            set_cell(buf, cx, ry, ch, fg, bg_color);
        }
        let row_fg = if *is_key_row { key_fg } else { fg_color };
        for (j, ch) in text.chars().enumerate() {
            let cx = x + 1 + j as u16;
            if cx + 1 < x + width {
                set_cell(buf, cx, ry, ch, row_fg, bg_color);
            }
        }
    }

    // Blank row before bottom border
    let pre_bottom = y + 2 + lines.len() as u16;
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 || col == width - 1 {
            '│'
        } else {
            ' '
        };
        let fg = if col == 0 || col == width - 1 {
            border_fg
        } else {
            fg_color
        };
        set_cell(buf, cx, pre_bottom, ch, fg, bg_color);
    }

    // Bottom border
    let bottom = y + height - 1;
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 {
            '╰'
        } else if col == width - 1 {
            '╯'
        } else {
            '─'
        };
        set_cell(buf, cx, bottom, ch, border_fg, bg_color);
    }
}

fn render_dialog_popup(
    buf: &mut ratatui::buffer::Buffer,
    term_area: Rect,
    dialog: &render::DialogPanel,
    theme: &Theme,
) {
    let bg = rc(theme.fuzzy_bg);
    let fg = rc(theme.fuzzy_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);

    // Compute dimensions: widest line of body or title, at least 40.
    let body_max = dialog.body.iter().map(|l| l.len()).max().unwrap_or(0);
    let btn_row_len: usize = dialog
        .buttons
        .iter()
        .map(|(lbl, _)| lbl.len() + 4) // "  [label]  "
        .sum::<usize>()
        + 2;
    let content_width = body_max.max(dialog.title.len() + 4).max(btn_row_len);
    let width = (content_width as u16 + 4).clamp(40, term_area.width.saturating_sub(4));
    // Height: top border + title + blank + body lines + blank + button row + bottom border.
    let height = (3 + dialog.body.len() as u16 + 2 + 1).min(term_area.height.saturating_sub(4));

    let x = (term_area.width.saturating_sub(width)) / 2;
    let y = (term_area.height.saturating_sub(height)) / 2;

    // Clear background.
    for row in y..y + height {
        for col in x..x + width {
            if col < term_area.width && row < term_area.height {
                set_cell(buf, col, row, ' ', fg, bg);
            }
        }
    }

    // Top border.
    for col in 0..width {
        let cx = x + col;
        if cx < term_area.width {
            let ch = if col == 0 {
                '╭'
            } else if col == width - 1 {
                '╮'
            } else {
                '─'
            };
            set_cell(buf, cx, y, ch, border_fg, bg);
        }
    }
    // Title overlay.
    let title = format!(" {} ", dialog.title);
    for (i, ch) in title.chars().enumerate() {
        let cx = x + 2 + i as u16;
        if cx + 1 < x + width && cx < term_area.width {
            set_cell(buf, cx, y, ch, title_fg, bg);
        }
    }

    // Left/right borders for content rows.
    for row in (y + 1)..(y + height - 1) {
        if row < term_area.height {
            if x < term_area.width {
                set_cell(buf, x, row, '│', border_fg, bg);
            }
            let rx = x + width - 1;
            if rx < term_area.width {
                set_cell(buf, rx, row, '│', border_fg, bg);
            }
        }
    }

    // Body lines.
    let body_y = y + 2;
    for (i, line) in dialog.body.iter().enumerate() {
        let row = body_y + i as u16;
        if row >= y + height - 2 {
            break;
        }
        for (j, ch) in line.chars().enumerate() {
            let cx = x + 2 + j as u16;
            if cx + 1 < x + width && cx < term_area.width && row < term_area.height {
                set_cell(buf, cx, row, ch, fg, bg);
            }
        }
    }

    // Button row — last content row before bottom border.
    let btn_y = y + height - 2;
    if btn_y < term_area.height {
        let mut col_offset = 2u16;
        for (label, is_selected) in &dialog.buttons {
            let btn_text = format!("  {}  ", label);
            let btn_bg = if *is_selected { sel_bg } else { bg };
            for ch in btn_text.chars() {
                let cx = x + col_offset;
                if cx + 1 < x + width && cx < term_area.width {
                    set_cell(buf, cx, btn_y, ch, fg, btn_bg);
                }
                col_offset += 1;
            }
        }
    }

    // Bottom border.
    let bottom = y + height - 1;
    if bottom < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '╰'
                } else if col == width - 1 {
                    '╯'
                } else {
                    '─'
                };
                set_cell(buf, cx, bottom, ch, border_fg, bg);
            }
        }
    }
}

fn render_window(frame: &mut ratatui::Frame, area: Rect, window: &RenderedWindow, theme: &Theme) {
    let window_bg = rc(if window.show_active_bg {
        theme.active_background
    } else {
        theme.background
    });
    let default_fg = rc(theme.foreground);
    let gutter_w = window.gutter_char_width as u16;
    let viewport_lines = area.height as usize;
    let has_scrollbar = window.total_lines > viewport_lines && area.width > gutter_w + 1;
    let viewport_cols =
        (area.width as usize).saturating_sub(gutter_w as usize + if has_scrollbar { 1 } else { 0 });
    let has_h_scrollbar = window.max_col > viewport_cols && area.height > 1;

    // Fill background
    for row in 0..area.height {
        for col in 0..area.width {
            set_cell(
                frame.buffer_mut(),
                area.x + col,
                area.y + row,
                ' ',
                default_fg,
                window_bg,
            );
        }
    }

    for (row_idx, line) in window.lines.iter().enumerate() {
        let screen_y = area.y + row_idx as u16;
        if screen_y >= area.y + area.height {
            break;
        }

        // Diff / DAP stopped-line background.
        let line_bg = if line.is_dap_current {
            rc(theme.dap_stopped_bg)
        } else {
            match line.diff_status {
                Some(DiffLine::Added) => rc(theme.diff_added_bg),
                Some(DiffLine::Removed) => rc(theme.diff_removed_bg),
                Some(DiffLine::Padding) => rc(theme.diff_padding_bg),
                _ => window_bg,
            }
        };
        if line_bg != window_bg {
            for col in 0..area.width {
                set_cell(
                    frame.buffer_mut(),
                    area.x + col,
                    screen_y,
                    ' ',
                    default_fg,
                    line_bg,
                );
            }
        }

        // Gutter
        if gutter_w > 0 {
            let line_num_fg = rc(if line.is_current_line {
                theme.line_number_active_fg
            } else {
                theme.line_number_fg
            });
            // The bp column offset: 1 when has_breakpoints, else 0.
            // The git column offset: bp_offset + 1 when has_git_diff, else bp_offset.
            let bp_offset = if window.has_breakpoints { 1 } else { 0 };
            let git_offset = if window.has_git_diff {
                bp_offset + 1
            } else {
                bp_offset
            };
            for (i, ch) in line.gutter_text.chars().enumerate() {
                let gx = area.x + i as u16;
                if gx >= area.x + gutter_w {
                    break;
                }
                let fg = if window.has_breakpoints && i == 0 {
                    // Breakpoint column: red when active, dimmed otherwise.
                    if line.is_dap_current || line.is_breakpoint {
                        rc(theme.diagnostic_error)
                    } else {
                        line_num_fg
                    }
                } else if window.has_git_diff && i == bp_offset {
                    // Git column.
                    rc(match line.git_diff {
                        Some(GitLineStatus::Added) => theme.git_added,
                        Some(GitLineStatus::Modified) => theme.git_modified,
                        Some(GitLineStatus::Deleted) => theme.git_deleted,
                        None => theme.line_number_fg,
                    })
                } else {
                    let _ = git_offset; // suppress unused-variable warning
                    line_num_fg
                };
                set_cell(frame.buffer_mut(), gx, screen_y, ch, fg, line_bg);
            }
            // Diagnostic gutter icon (overwrite leftmost gutter char)
            if let Some(severity) = window.diagnostic_gutter.get(&line.line_idx) {
                let (diag_ch, diag_color) = match severity {
                    DiagnosticSeverity::Error => ('E', rc(theme.diagnostic_error)),
                    DiagnosticSeverity::Warning => ('W', rc(theme.diagnostic_warning)),
                    DiagnosticSeverity::Information => ('I', rc(theme.diagnostic_info)),
                    DiagnosticSeverity::Hint => ('H', rc(theme.diagnostic_hint)),
                };
                set_cell(
                    frame.buffer_mut(),
                    area.x,
                    screen_y,
                    diag_ch,
                    diag_color,
                    line_bg,
                );
            }
        }

        // Text (narrowed by 1 when scrollbar is shown)
        let text_area_x = area.x + gutter_w;
        let text_width = area
            .width
            .saturating_sub(gutter_w)
            .saturating_sub(if has_scrollbar { 1 } else { 0 });
        render_text_line(
            frame.buffer_mut(),
            text_area_x,
            screen_y,
            text_width,
            line,
            window.scroll_left,
            theme,
            line_bg,
            window.tabstop,
        );

        // Indent guides: draw │ at guide columns where the cell is a space
        if !line.indent_guides.is_empty() {
            let guide_fg = rc(theme.indent_guide_fg);
            let active_fg = rc(theme.indent_guide_active_fg);
            for &guide_col in &line.indent_guides {
                if guide_col < window.scroll_left {
                    continue;
                }
                let vis_col = (guide_col - window.scroll_left) as u16;
                if vis_col >= text_width {
                    break;
                }
                let cx = text_area_x + vis_col;
                if cx < area.x + area.width && screen_y < area.y + area.height {
                    let cell = frame.buffer_mut().get_mut(cx, screen_y);
                    // Only draw guide if the cell is a space (don't overwrite text)
                    if cell.symbol() == " " {
                        let is_active = window.active_indent_col == Some(guide_col);
                        let fg = if is_active { active_fg } else { guide_fg };
                        cell.set_char('│');
                        cell.set_fg(fg);
                    }
                }
            }
        }

        // Ghost continuation lines — draw full line in ghost colour.
        if line.is_ghost_continuation {
            if let Some(ghost) = &line.ghost_suffix {
                let ghost_fg = rc(theme.ghost_text_fg);
                for (i, ch) in ghost.chars().enumerate() {
                    let gx = text_area_x + i as u16;
                    if gx >= text_area_x + text_width {
                        break;
                    }
                    set_cell(frame.buffer_mut(), gx, screen_y, ch, ghost_fg, line_bg);
                }
            }
        }

        // Diagnostic underlines (UNDERLINED modifier on diagnostic spans)
        for dm in &line.diagnostics {
            let diag_fg = rc(match dm.severity {
                DiagnosticSeverity::Error => theme.diagnostic_error,
                DiagnosticSeverity::Warning => theme.diagnostic_warning,
                DiagnosticSeverity::Information => theme.diagnostic_info,
                DiagnosticSeverity::Hint => theme.diagnostic_hint,
            });
            for col in dm.start_col..dm.end_col {
                if col < window.scroll_left {
                    continue;
                }
                let vis_col = (col - window.scroll_left) as u16;
                if vis_col >= text_width {
                    break;
                }
                let cx = text_area_x + vis_col;
                if cx < area.x + area.width && screen_y < area.y + area.height {
                    let cell = frame.buffer_mut().get_mut(cx, screen_y);
                    cell.set_fg(diag_fg);
                    cell.modifier |= Modifier::UNDERLINED;
                }
            }
        }

        // Spell error underlines
        let spell_fg = rc(theme.spell_error);
        for sm in &line.spell_errors {
            for col in sm.start_col..sm.end_col {
                if col < window.scroll_left {
                    continue;
                }
                let vis_col = (col - window.scroll_left) as u16;
                if vis_col >= text_width {
                    break;
                }
                let cx = text_area_x + vis_col;
                if cx < area.x + area.width && screen_y < area.y + area.height {
                    let cell = frame.buffer_mut().get_mut(cx, screen_y);
                    cell.set_fg(spell_fg);
                    cell.modifier |= Modifier::UNDERLINED;
                }
            }
        }

        // Bracket match highlighting
        let bracket_bg = rc(theme.bracket_match_bg);
        for &(view_line, col) in &window.bracket_match_positions {
            if view_line == row_idx {
                let vis = char_col_to_visual(&line.raw_text, col, window.tabstop);
                if vis < window.scroll_left {
                    continue;
                }
                let vis_col = (vis - window.scroll_left) as u16;
                if vis_col >= text_width {
                    continue;
                }
                let cx = text_area_x + vis_col;
                if cx < area.x + area.width && screen_y < area.y + area.height {
                    let cell = frame.buffer_mut().get_mut(cx, screen_y);
                    cell.set_bg(bracket_bg);
                }
            }
        }
    }

    // Selection overlay
    if let Some(sel) = &window.selection {
        render_selection(
            frame.buffer_mut(),
            area,
            window,
            sel,
            window_bg,
            theme.selection,
            rc(theme.foreground),
        );
    }

    // Extra selections (Ctrl+D multi-cursor word highlights)
    for esel in &window.extra_selections {
        render_selection(
            frame.buffer_mut(),
            area,
            window,
            esel,
            window_bg,
            theme.selection,
            rc(theme.foreground),
        );
    }

    // Yank highlight overlay (brief flash after yank)
    if let Some(yh) = &window.yank_highlight {
        render_selection(
            frame.buffer_mut(),
            area,
            window,
            yh,
            window_bg,
            theme.yank_highlight_bg,
            rc(theme.foreground),
        );
    }

    // Vertical scrollbar
    if has_scrollbar {
        render_scrollbar(
            frame.buffer_mut(),
            area,
            window.scroll_top,
            window.total_lines,
            viewport_lines,
            has_h_scrollbar,
            theme,
        );
    }

    // Horizontal scrollbar
    if has_h_scrollbar {
        render_h_scrollbar(
            frame.buffer_mut(),
            area,
            window.scroll_left,
            window.max_col,
            viewport_cols,
            gutter_w,
            has_scrollbar,
            theme,
        );
    }

    // Cursor
    if let Some((cursor_pos, cursor_shape)) = &window.cursor {
        let cursor_screen_y = area.y + cursor_pos.view_line as u16;
        let raw = window
            .lines
            .get(cursor_pos.view_line)
            .map(|l| l.raw_text.as_str())
            .unwrap_or("");
        let vis_col = char_col_to_visual(raw, cursor_pos.col, window.tabstop)
            .saturating_sub(window.scroll_left) as u16;
        let cursor_screen_x = area.x + gutter_w + vis_col;

        let buf = frame.buffer_mut();
        let buf_area = buf.area;

        match cursor_shape {
            CursorShape::Block => {
                if cursor_screen_x < buf_area.x + buf_area.width
                    && cursor_screen_y < buf_area.y + buf_area.height
                {
                    let cell = buf.get_mut(cursor_screen_x, cursor_screen_y);
                    let old_fg = cell.fg;
                    let old_bg = cell.bg;
                    cell.set_fg(old_bg).set_bg(old_fg);
                }
            }
            CursorShape::Bar | CursorShape::Underline => {
                frame.set_cursor(cursor_screen_x, cursor_screen_y);
            }
        }
    }

    // AI ghost text — draw after cursor at cursor position in muted colour.
    if let Some((cursor_pos, _)) = &window.cursor {
        if let Some(rl) = window.lines.get(cursor_pos.view_line) {
            if let Some(ghost) = &rl.ghost_suffix {
                let ghost_screen_y = area.y + cursor_pos.view_line as u16;
                let vis_col = char_col_to_visual(&rl.raw_text, cursor_pos.col, window.tabstop)
                    .saturating_sub(window.scroll_left) as u16;
                let ghost_start_x = area.x + gutter_w + vis_col;
                let ghost_fg = rc(theme.ghost_text_fg);
                let buf = frame.buffer_mut();
                for (i, ch) in ghost.chars().enumerate() {
                    let gx = ghost_start_x + i as u16;
                    if gx >= area.x + area.width {
                        break;
                    }
                    set_cell(buf, gx, ghost_screen_y, ch, ghost_fg, window_bg);
                }
            }
        }
    }

    // Secondary cursors (multi-cursor) — render with cursor color background.
    let cursor_color = ratatui::style::Color::Rgb(theme.cursor.r, theme.cursor.g, theme.cursor.b);
    let has_extra_sels = !window.extra_selections.is_empty();
    for extra_pos in &window.extra_cursors {
        let sy = area.y + extra_pos.view_line as u16;
        // When Ctrl+D selections are active, show cursor at col+1 (right of selection)
        let col = if has_extra_sels {
            extra_pos.col + 1
        } else {
            extra_pos.col
        };
        let raw = window
            .lines
            .get(extra_pos.view_line)
            .map(|l| l.raw_text.as_str())
            .unwrap_or("");
        let vis_col =
            char_col_to_visual(raw, col, window.tabstop).saturating_sub(window.scroll_left) as u16;
        let sx = area.x + gutter_w + vis_col;
        let buf = frame.buffer_mut();
        if sx < buf.area.x + buf.area.width && sy < buf.area.y + buf.area.height {
            let cell = buf.get_mut(sx, sy);
            cell.set_bg(cursor_color).set_fg(ratatui::style::Color::Rgb(
                theme.background.r,
                theme.background.g,
                theme.background.b,
            ));
        }
    }
}

fn render_scrollbar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    scroll_top: usize,
    total_lines: usize,
    viewport_lines: usize,
    // When true, leave the last row for the horizontal scrollbar (don't draw there)
    has_h_scrollbar: bool,
    theme: &Theme,
) {
    if area.height == 0 || total_lines == 0 {
        return;
    }
    let track_fg = rc(theme.separator);
    let thumb_fg = RColor::Rgb(128, 128, 128);
    let sb_bg = rc(theme.background);
    // Track height: reserve last row for h-scrollbar if present
    let track_h = if has_h_scrollbar {
        area.height.saturating_sub(1)
    } else {
        area.height
    };
    if track_h == 0 {
        return;
    }
    let h = track_h as f64;
    let thumb_size = ((viewport_lines as f64 / total_lines as f64) * h)
        .ceil()
        .max(1.0) as u16;
    let thumb_top = ((scroll_top as f64 / total_lines as f64) * h).floor() as u16;
    let sb_x = area.x + area.width - 1;
    for dy in 0..track_h {
        let y = area.y + dy;
        let in_thumb = dy >= thumb_top && dy < thumb_top + thumb_size;
        let ch = if in_thumb { '█' } else { '░' };
        let fg = if in_thumb { thumb_fg } else { track_fg };
        set_cell(buf, sb_x, y, ch, fg, sb_bg);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_h_scrollbar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    scroll_left: usize,
    max_col: usize,
    viewport_cols: usize,
    gutter_w: u16,
    has_v_scrollbar: bool,
    theme: &Theme,
) {
    if area.height == 0 || max_col == 0 || viewport_cols == 0 {
        return;
    }
    let thumb_fg = RColor::Rgb(128, 128, 128);
    let sb_bg = rc(theme.background);
    let corner_fg = rc(theme.separator);

    let sb_y = area.y + area.height - 1;
    let track_x = area.x + gutter_w;
    // Leave the rightmost cell for the v-scrollbar corner / separator
    let track_w = area
        .width
        .saturating_sub(gutter_w + if has_v_scrollbar { 1 } else { 0 });
    if track_w == 0 {
        return;
    }

    let w = track_w as f64;
    let thumb_size = ((viewport_cols as f64 / max_col as f64) * w)
        .ceil()
        .max(1.0) as u16;
    let thumb_left = ((scroll_left as f64 / max_col as f64) * w).floor() as u16;

    for dx in 0..track_w {
        let x = track_x + dx;
        let in_thumb = dx >= thumb_left && dx < thumb_left + thumb_size;
        let ch = if in_thumb { '▄' } else { ' ' };
        let fg = if in_thumb { thumb_fg } else { sb_bg };
        set_cell(buf, x, sb_y, ch, fg, sb_bg);
    }
    // Corner cell (intersection of v-scrollbar column and h-scrollbar row)
    if has_v_scrollbar {
        set_cell(buf, area.x + area.width - 1, sb_y, '┘', corner_fg, sb_bg);
    }
}

/// Convert a character-index column to a visual column, expanding tabs.
fn char_col_to_visual(raw_text: &str, char_col: usize, tabstop: usize) -> usize {
    let tabstop = tabstop.max(1);
    let mut vis = 0usize;
    for (i, ch) in raw_text.chars().enumerate() {
        if ch == '\n' || ch == '\r' {
            break;
        }
        if i >= char_col {
            break;
        }
        if ch == '\t' {
            vis = ((vis / tabstop) + 1) * tabstop;
        } else {
            vis += 1;
        }
    }
    vis
}

#[allow(clippy::too_many_arguments)]
fn render_text_line(
    buf: &mut ratatui::buffer::Buffer,
    x_start: u16,
    y: u16,
    max_width: u16,
    line: &RenderedLine,
    scroll_left: usize,
    theme: &Theme,
    window_bg: RColor,
    tabstop: usize,
) {
    let raw = &line.raw_text;
    let chars: Vec<char> = raw.chars().filter(|&c| c != '\n' && c != '\r').collect();

    let mut char_fgs: Vec<Color> = vec![theme.foreground; chars.len()];
    let mut char_bgs: Vec<Option<Color>> = vec![None; chars.len()];
    let mut char_mods: Vec<Modifier> = vec![Modifier::empty(); chars.len()];

    for span in &line.spans {
        let start = byte_to_char_idx(raw, span.start_byte);
        let end = byte_to_char_idx(raw, span.end_byte).min(chars.len());
        for i in start..end {
            char_fgs[i] = span.style.fg;
            char_bgs[i] = span.style.bg;
            let mut m = Modifier::empty();
            if span.style.bold {
                m |= Modifier::BOLD;
            }
            if span.style.italic {
                m |= Modifier::ITALIC;
            }
            char_mods[i] = m;
        }
    }

    // Expand characters to visual columns, handling tabs.
    // Each entry: (visual_col, char_idx) for non-tab chars, or multiple
    // space entries for a single tab.
    let tabstop = tabstop.max(1);
    let mut vis_col: usize = 0;
    // Build a flat list of (visual_column, char_to_draw, char_index_for_style)
    let mut cells: Vec<(usize, char, usize)> = Vec::with_capacity(chars.len());
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '\t' {
            let next_stop = ((vis_col / tabstop) + 1) * tabstop;
            while vis_col < next_stop {
                cells.push((vis_col, ' ', i));
                vis_col += 1;
            }
        } else {
            cells.push((vis_col, ch, i));
            vis_col += 1;
        }
    }
    let total_vis_cols = vis_col;

    for &(vcol, ch, ci) in &cells {
        if vcol < scroll_left {
            continue;
        }
        let col = (vcol - scroll_left) as u16;
        if col >= max_width {
            break;
        }
        let fg = rc(char_fgs[ci]);
        let bg = char_bgs[ci].map(rc).unwrap_or(window_bg);
        if char_mods[ci].is_empty() {
            set_cell(buf, x_start + col, y, ch, fg, bg);
        } else {
            set_cell_styled(buf, x_start + col, y, ch, fg, bg, char_mods[ci]);
        }
    }

    // Inline annotation / virtual text (e.g. git blame)
    if let Some(ann) = &line.annotation {
        let visible_cols = total_vis_cols.saturating_sub(scroll_left);
        let ann_start = x_start + visible_cols.min(max_width as usize) as u16;
        let ann_fg = rc(theme.annotation_fg);
        for (i, ch) in ann.chars().enumerate() {
            let col = ann_start + i as u16;
            if col >= x_start + max_width {
                break;
            }
            set_cell(buf, col, y, ch, ann_fg, window_bg);
        }
    }
}

fn render_selection(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    window: &RenderedWindow,
    sel: &render::SelectionRange,
    window_bg: RColor,
    color: render::Color,
    default_fg: RColor,
) {
    let sel_bg = rc(color);
    let gutter_w = window.gutter_char_width as u16;
    let text_area_x = area.x + gutter_w;
    let text_width = area.width.saturating_sub(gutter_w) as usize;

    for (row_idx, line) in window.lines.iter().enumerate() {
        let buffer_line = window.scroll_top + row_idx;
        if buffer_line < sel.start_line || buffer_line > sel.end_line {
            continue;
        }
        let screen_y = area.y + row_idx as u16;

        let col_start = match sel.kind {
            SelectionKind::Line => 0,
            SelectionKind::Char => {
                if buffer_line == sel.start_line {
                    sel.start_col
                } else {
                    0
                }
            }
            SelectionKind::Block => sel.start_col,
        };
        let col_end = match sel.kind {
            SelectionKind::Line => usize::MAX,
            SelectionKind::Char => {
                if buffer_line == sel.end_line {
                    sel.end_col + 1
                } else {
                    usize::MAX
                }
            }
            SelectionKind::Block => sel.end_col + 1,
        };

        let char_count = line.raw_text.chars().filter(|&c| c != '\n').count().max(1);
        let effective_end = col_end.min(char_count);

        // Convert char-index column range to visual columns accounting for tabs.
        let vis_start = char_col_to_visual(&line.raw_text, col_start, window.tabstop);
        let vis_end = char_col_to_visual(&line.raw_text, effective_end, window.tabstop);

        for vis in vis_start..vis_end {
            if vis < window.scroll_left {
                continue;
            }
            let screen_col = (vis - window.scroll_left) as u16;
            if screen_col >= text_width as u16 {
                break;
            }
            let sx = text_area_x + screen_col;
            let buf_area = buf.area;
            if sx < buf_area.x + buf_area.width && screen_y < buf_area.y + buf_area.height {
                let cell = buf.get_mut(sx, screen_y);
                let old_fg = cell.fg;
                cell.set_bg(sel_bg);
                // Keep text visible against selection background
                if old_fg == window_bg {
                    cell.set_fg(default_fg);
                }
            }
        }
    }
}

fn render_separators(
    buf: &mut ratatui::buffer::Buffer,
    editor_area: Rect,
    windows: &[RenderedWindow],
    theme: &Theme,
) {
    if windows.len() <= 1 {
        return;
    }
    let sep_fg = rc(theme.separator);
    let thumb_fg = RColor::Rgb(128, 128, 128);
    let track_fg = sep_fg;
    let sep_bg = rc(theme.background);

    for i in 0..windows.len() {
        for j in (i + 1)..windows.len() {
            let a = &windows[i];
            let b = &windows[j];

            // Vertical separator: window a is the left pane, b is the right pane.
            // The separator is drawn in the last column of a. We draw scrollbar
            // chars there so the user can see and interact with a's scroll position.
            // Also require vertical overlap — windows from different groups may
            // share an x edge but not overlap in y (e.g. 2×2 grid).
            let v_overlap =
                a.rect.y.max(b.rect.y) < (a.rect.y + a.rect.height).min(b.rect.y + b.rect.height);
            if (a.rect.x + a.rect.width - b.rect.x).abs() < 1.0 && v_overlap {
                let sep_x = editor_area.x + (a.rect.x + a.rect.width) as u16;
                let y_start = editor_area.y + a.rect.y.max(b.rect.y) as u16;
                let y_end =
                    editor_area.y + (a.rect.y + a.rect.height).min(b.rect.y + b.rect.height) as u16;
                let track_h = y_end.saturating_sub(y_start) as usize;
                let viewport_lines = a.rect.height as usize;
                let has_scroll = a.total_lines > viewport_lines && track_h > 0;

                let (thumb_top, thumb_size) = if has_scroll {
                    let h = track_h as f64;
                    let size = ((viewport_lines as f64 / a.total_lines as f64) * h)
                        .ceil()
                        .max(1.0) as usize;
                    let top = ((a.scroll_top as f64 / a.total_lines as f64) * h).floor() as usize;
                    (top, size)
                } else {
                    (0, track_h)
                };

                for dy in 0..y_end.saturating_sub(y_start) {
                    let y = y_start + dy;
                    let (ch, fg) = if has_scroll {
                        let in_thumb =
                            (dy as usize) >= thumb_top && (dy as usize) < thumb_top + thumb_size;
                        if in_thumb {
                            ('█', thumb_fg)
                        } else {
                            ('░', track_fg)
                        }
                    } else {
                        ('│', sep_fg)
                    };
                    set_cell(buf, sep_x.saturating_sub(1), y, ch, fg, sep_bg);
                }
            }

            // Horizontal separator — also require horizontal overlap.
            let h_overlap =
                a.rect.x.max(b.rect.x) < (a.rect.x + a.rect.width).min(b.rect.x + b.rect.width);
            if (a.rect.y + a.rect.height - b.rect.y).abs() < 1.0 && h_overlap {
                let sep_y = editor_area.y + (a.rect.y + a.rect.height) as u16;
                let x_start = editor_area.x + a.rect.x.max(b.rect.x) as u16;
                let x_end =
                    editor_area.x + (a.rect.x + a.rect.width).min(b.rect.x + b.rect.width) as u16;
                for x in x_start..x_end.max(x_start) {
                    set_cell(buf, x, sep_y.saturating_sub(1), '─', sep_fg, sep_bg);
                }
            }
        }
    }
}

// ─── Activity bar ─────────────────────────────────────────────────────────────

// ─── Menu bar rendering ───────────────────────────────────────────────────────────────────

fn render_menu_bar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    data: &render::MenuBarData,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let bar_bg = rc(theme.status_bg);
    let bar_fg = rc(theme.status_fg);
    let y = area.y;

    // Fill background
    for x in area.x..area.x + area.width {
        set_cell(buf, x, y, ' ', bar_fg, bar_bg);
    }

    // Menu labels (no hamburger here — it lives in the activity bar below)
    let mut col = area.x + 1; // one-cell left pad

    for (idx, (name, _, _)) in render::MENU_STRUCTURE.iter().enumerate() {
        let is_open = data.open_menu_idx == Some(idx);
        let (fg, bg) = if is_open {
            (bar_bg, bar_fg) // reversed for open
        } else {
            (bar_fg, bar_bg)
        };
        // Space before name
        if col < area.x + area.width {
            set_cell(buf, col, y, ' ', bar_fg, bar_bg);
            col += 1;
        }
        // Name chars
        for ch in name.chars() {
            if col >= area.x + area.width {
                break;
            }
            set_cell(buf, col, y, ch, fg, bg);
            col += 1;
        }
        // Space after name
        if col < area.x + area.width {
            set_cell(buf, col, y, ' ', fg, bar_bg);
            col += 1;
        }
    }

    // Title text drawn right-aligned (dimmed)
    if !data.title.is_empty() {
        let title_chars: Vec<char> = data.title.chars().collect();
        let title_len = title_chars.len() as u16;
        let right_margin = 1u16;
        if area.width > title_len + right_margin {
            let title_start = area.x + area.width - title_len - right_margin;
            if title_start > col {
                let dim_fg = rc(theme.line_number_fg);
                for (i, ch) in title_chars.iter().enumerate() {
                    let tx = title_start + i as u16;
                    if tx < area.x + area.width {
                        set_cell(buf, tx, y, *ch, dim_fg, bar_bg);
                    }
                }
            }
        }
    }
}

fn render_menu_dropdown(
    buf: &mut ratatui::buffer::Buffer,
    full_area: Rect,
    data: &render::MenuBarData,
    theme: &Theme,
) {
    let Some(midx) = data.open_menu_idx else {
        return;
    };
    if data.open_items.is_empty() {
        return;
    }

    let popup_bg = rc(theme.tab_bar_bg);
    let popup_fg = rc(theme.foreground);
    let sep_fg = rc(theme.line_number_fg);
    let shortcut_fg = rc(theme.line_number_fg);

    let total_rows = data.open_items.len() as u16 + 2; // border top/bottom
    let max_label = data
        .open_items
        .iter()
        .map(|i| i.label.len())
        .max()
        .unwrap_or(4);
    let max_shortcut = data
        .open_items
        .iter()
        .map(|i| {
            if data.is_vscode_mode && !i.vscode_shortcut.is_empty() {
                i.vscode_shortcut.len()
            } else {
                i.shortcut.len()
            }
        })
        .max()
        .unwrap_or(0);
    let popup_width = (max_label + max_shortcut + 6).clamp(20, 50) as u16;
    let anchor_col = data.open_menu_col + full_area.x;
    let popup_x = anchor_col.min(full_area.x + full_area.width.saturating_sub(popup_width));
    // Dropdown appears just below the menu bar row (y=1)
    let popup_y = full_area.y + 1;
    let popup_height = total_rows.min(full_area.height.saturating_sub(popup_y));

    // Draw border + background
    for dy in 0..popup_height {
        for dx in 0..popup_width {
            let x = popup_x + dx;
            let y = popup_y + dy;
            if x >= full_area.x + full_area.width || y >= full_area.y + full_area.height {
                continue;
            }
            let ch = if dy == 0 {
                if dx == 0 {
                    '\u{250c}'
                } else if dx == popup_width - 1 {
                    '\u{2510}'
                } else {
                    '\u{2500}'
                }
            } else if dy == popup_height - 1 {
                if dx == 0 {
                    '\u{2514}'
                } else if dx == popup_width - 1 {
                    '\u{2518}'
                } else {
                    '\u{2500}'
                }
            } else if dx == 0 || dx == popup_width - 1 {
                '\u{2502}'
            } else {
                ' '
            };
            set_cell(buf, x, y, ch, popup_fg, popup_bg);
        }
    }

    // Draw items
    let mut row: u16 = popup_y + 1;
    for (item_idx, item) in data.open_items.iter().enumerate() {
        if row >= popup_y + popup_height - 1 {
            break;
        }
        let is_highlighted = data.highlighted_item_idx == Some(item_idx);
        let (item_fg, item_bg) = if is_highlighted {
            (popup_bg, popup_fg) // invert for highlighted row
        } else {
            (popup_fg, popup_bg)
        };
        if item.separator {
            // Separator line (never highlighted)
            for dx in 1..popup_width - 1 {
                let x = popup_x + dx;
                if x < full_area.x + full_area.width {
                    set_cell(buf, x, row, '\u{2500}', sep_fg, popup_bg);
                }
            }
        } else {
            // Fill highlighted row background
            if is_highlighted {
                for dx in 1..popup_width - 1 {
                    let x = popup_x + dx;
                    if x < full_area.x + full_area.width {
                        set_cell(buf, x, row, ' ', item_fg, item_bg);
                    }
                }
            }
            // Label
            let label_x = popup_x + 2;
            for (i, ch) in item.label.chars().enumerate() {
                let x = label_x + i as u16;
                if x >= popup_x + popup_width - 1 {
                    break;
                }
                set_cell(buf, x, row, ch, item_fg, item_bg);
            }
            // Right-aligned shortcut (use VSCode variant when in VSCode mode)
            let sc = if data.is_vscode_mode && !item.vscode_shortcut.is_empty() {
                item.vscode_shortcut
            } else {
                item.shortcut
            };
            if !sc.is_empty() {
                let sc_fg = if is_highlighted { item_fg } else { shortcut_fg };
                let sc_len = sc.len() as u16;
                let sc_x = popup_x + popup_width - 1 - sc_len - 1;
                for (i, ch) in sc.chars().enumerate() {
                    let x = sc_x + i as u16;
                    if x < full_area.x + full_area.width {
                        set_cell(buf, x, row, ch, sc_fg, item_bg);
                    }
                }
            }
        }
        row += 1;
    }
    let _ = midx; // suppress unused warning
}

// ─── Context menu popup rendering ───────────────────────────────────────────────────────

fn render_context_menu(
    buf: &mut ratatui::buffer::Buffer,
    full_area: Rect,
    data: &render::ContextMenuPanel,
    theme: &Theme,
) {
    if data.items.is_empty() {
        return;
    }

    let popup_bg = rc(theme.tab_bar_bg);
    let popup_fg = rc(theme.foreground);
    let sep_fg = rc(theme.line_number_fg);
    let shortcut_fg = rc(theme.line_number_fg);
    let disabled_fg = rc(theme.line_number_fg);

    // Count visual rows: items + separator lines after items that have separator_after
    let separator_count = data.items.iter().filter(|i| i.separator_after).count() as u16;
    let total_rows = data.items.len() as u16 + separator_count + 2; // +2 for borders

    let max_label = data.items.iter().map(|i| i.label.len()).max().unwrap_or(4);
    let max_shortcut = data
        .items
        .iter()
        .map(|i| i.shortcut.len())
        .max()
        .unwrap_or(0);
    let popup_width = (max_label + max_shortcut + 6).clamp(20, 50) as u16;

    // Clamp position to stay within terminal
    let popup_x = data
        .screen_col
        .min(full_area.x + full_area.width.saturating_sub(popup_width));
    let popup_y = data
        .screen_row
        .min(full_area.y + full_area.height.saturating_sub(total_rows));
    let popup_height = total_rows.min(full_area.y + full_area.height - popup_y);

    // Draw border + background
    for dy in 0..popup_height {
        for dx in 0..popup_width {
            let x = popup_x + dx;
            let y = popup_y + dy;
            if x >= full_area.x + full_area.width || y >= full_area.y + full_area.height {
                continue;
            }
            let ch = if dy == 0 {
                if dx == 0 {
                    '\u{250c}'
                } else if dx == popup_width - 1 {
                    '\u{2510}'
                } else {
                    '\u{2500}'
                }
            } else if dy == popup_height - 1 {
                if dx == 0 {
                    '\u{2514}'
                } else if dx == popup_width - 1 {
                    '\u{2518}'
                } else {
                    '\u{2500}'
                }
            } else if dx == 0 || dx == popup_width - 1 {
                '\u{2502}'
            } else {
                ' '
            };
            set_cell(buf, x, y, ch, popup_fg, popup_bg);
        }
    }

    // Draw items
    let mut row: u16 = popup_y + 1;
    for (item_idx, item) in data.items.iter().enumerate() {
        if row >= popup_y + popup_height - 1 {
            break;
        }
        let is_selected = item_idx == data.selected_idx;
        let (item_fg, item_bg) = if is_selected && item.enabled {
            (popup_bg, popup_fg) // invert for selected row
        } else if !item.enabled {
            (disabled_fg, popup_bg)
        } else {
            (popup_fg, popup_bg)
        };

        // Fill row background
        if is_selected && item.enabled {
            for dx in 1..popup_width - 1 {
                let x = popup_x + dx;
                if x < full_area.x + full_area.width {
                    set_cell(buf, x, row, ' ', item_fg, item_bg);
                }
            }
        }
        // Label
        let label_x = popup_x + 2;
        for (i, ch) in item.label.chars().enumerate() {
            let x = label_x + i as u16;
            if x >= popup_x + popup_width - 1 {
                break;
            }
            set_cell(buf, x, row, ch, item_fg, item_bg);
        }
        // Right-aligned shortcut
        if !item.shortcut.is_empty() {
            let sc_fg = if is_selected && item.enabled {
                item_fg
            } else {
                shortcut_fg
            };
            let sc_len = item.shortcut.len() as u16;
            let sc_x = popup_x + popup_width - 1 - sc_len - 1;
            for (i, ch) in item.shortcut.chars().enumerate() {
                let x = sc_x + i as u16;
                if x < full_area.x + full_area.width {
                    set_cell(buf, x, row, ch, sc_fg, item_bg);
                }
            }
        }
        row += 1;

        // Draw separator after this item if needed
        if item.separator_after && row < popup_y + popup_height - 1 {
            for dx in 1..popup_width - 1 {
                let x = popup_x + dx;
                if x < full_area.x + full_area.width {
                    set_cell(buf, x, row, '\u{2500}', sep_fg, popup_bg);
                }
            }
            row += 1;
        }
    }
}

// ─── Debug toolbar rendering ────────────────────────────────────────────────────────────

fn render_debug_toolbar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    toolbar: &render::DebugToolbarData,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let bar_bg = rc(theme.status_bg);
    let bar_fg = rc(theme.status_fg);
    let dim_fg = rc(theme.line_number_fg);
    let y = area.y;

    // Fill background
    for x in area.x..area.x + area.width {
        set_cell(buf, x, y, ' ', bar_fg, bar_bg);
    }

    let mut col = area.x + 1;
    for (idx, btn) in toolbar.buttons.iter().enumerate() {
        // Separator between index 3 and 4
        if idx == 4 {
            if col < area.x + area.width {
                set_cell(buf, col, y, '\u{2502}', dim_fg, bar_bg);
                col += 1;
            }
            if col < area.x + area.width {
                set_cell(buf, col, y, ' ', bar_fg, bar_bg);
                col += 1;
            }
        }
        let fg = if toolbar.session_active {
            bar_fg
        } else {
            dim_fg
        };
        // Icon
        for ch in btn.icon.chars() {
            if col >= area.x + area.width {
                break;
            }
            set_cell(buf, col, y, ch, fg, bar_bg);
            col += 1;
        }
        // Key hint in parens
        if col < area.x + area.width {
            set_cell(buf, col, y, '(', dim_fg, bar_bg);
            col += 1;
        }
        for ch in btn.key_hint.chars() {
            if col >= area.x + area.width {
                break;
            }
            set_cell(buf, col, y, ch, dim_fg, bar_bg);
            col += 1;
        }
        if col < area.x + area.width {
            set_cell(buf, col, y, ')', dim_fg, bar_bg);
            col += 1;
        }
        // Space separator
        if col < area.x + area.width {
            set_cell(buf, col, y, ' ', bar_fg, bar_bg);
            col += 1;
        }
    }
}

fn render_activity_bar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    sidebar: &TuiSidebar,
    theme: &Theme,
    _menu_bar_visible: bool,
    engine: &Engine,
) {
    let bar_bg = rc(theme.tab_bar_bg);
    // All icons rendered in off-white for readability; active indicated by left accent bar.
    let icon_fg = RColor::Rgb(200, 200, 210);
    let accent_fg = rc(theme.cursor); // left-edge accent bar for active panel
    let toolbar_sel_bg = rc(theme.cursor); // highlight for toolbar-focused selection

    // Fill entire activity bar background
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', icon_fg, bar_bg);
        }
    }

    // Row 0: Hamburger icon (menu bar toggle)
    if area.height >= 1 {
        let y = area.y;
        let is_kbd_sel = sidebar.toolbar_focused && sidebar.toolbar_selected == 0;
        let row_bg = if is_kbd_sel { toolbar_sel_bg } else { bar_bg };
        let fg = icon_fg;
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, row_bg);
        }
        if area.width >= 3 {
            set_cell(buf, area.x + 1, y, '\u{f035c}', fg, row_bg); // hamburger
        }
    }

    // Top buttons: Explorer (1), Search (2), Debug (3), Git (4), Extensions (5), AI (6)
    let top_buttons: &[(u16, TuiPanel, char)] = &[
        (1, TuiPanel::Explorer, '\u{f07c}'),   // nf-fa-folder_open
        (2, TuiPanel::Search, '\u{f002}'),     // nf-fa-search
        (3, TuiPanel::Debug, '\u{f188}'),      // nf-fa-bug
        (4, TuiPanel::Git, '\u{e702}'),        // nf-dev-git_branch
        (5, TuiPanel::Extensions, '\u{eae6}'), // nf-cod-extensions
        (6, TuiPanel::Ai, '\u{f0e5}'),         // nf-fa-comment (AI chat)
    ];

    for &(row_off, panel, icon) in top_buttons {
        let y = area.y + row_off;
        if y >= area.y + area.height {
            break;
        }
        let is_active = sidebar.visible && sidebar.active_panel == panel;
        let is_kbd_sel = sidebar.toolbar_focused && sidebar.toolbar_selected == row_off;
        let row_bg = if is_kbd_sel { toolbar_sel_bg } else { bar_bg };
        let fg = icon_fg;
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, row_bg);
        }
        if area.width >= 3 {
            set_cell(buf, area.x + 1, y, icon, fg, row_bg);
        }
        if is_active && !is_kbd_sel {
            // Left accent bar for active panel
            set_cell(buf, area.x, y, '▎', accent_fg, bar_bg);
        }
    }

    // Extension panel icons (after the fixed 6 panels, starting at row 7)
    {
        let mut ext_panels: Vec<_> = engine.ext_panels.values().collect();
        ext_panels.sort_by(|a, b| a.name.cmp(&b.name));
        for (i, panel) in ext_panels.iter().enumerate() {
            let row_off = 7 + i as u16;
            let y = area.y + row_off;
            if y >= area.y + area.height.saturating_sub(1) {
                break; // leave room for settings at bottom
            }
            let is_active =
                sidebar.ext_panel_name.as_deref() == Some(&panel.name) && sidebar.visible;
            let toolbar_idx = 8 + i as u16; // 0=hamburger, 1-6=panels, 7=settings, 8+=ext
            let is_kbd_sel = sidebar.toolbar_focused && sidebar.toolbar_selected == toolbar_idx;
            let row_bg = if is_kbd_sel { toolbar_sel_bg } else { bar_bg };
            let fg = icon_fg;
            for x in area.x..area.x + area.width {
                set_cell(buf, x, y, ' ', fg, row_bg);
            }
            if area.width >= 3 {
                set_cell(buf, area.x + 1, y, panel.icon, fg, row_bg);
            }
            if is_active && !is_kbd_sel {
                set_cell(buf, area.x, y, '▎', accent_fg, bar_bg);
            }
        }
    }

    // Settings button pinned to the bottom row (like VSCode)
    if area.height >= 1 {
        let y = area.y + area.height - 1;
        let is_active = sidebar.visible && sidebar.active_panel == TuiPanel::Settings;
        let is_kbd_sel = sidebar.toolbar_focused && sidebar.toolbar_selected == 7;
        let row_bg = if is_kbd_sel { toolbar_sel_bg } else { bar_bg };
        let fg = icon_fg;
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, row_bg);
        }
        if area.width >= 3 {
            set_cell(buf, area.x + 1, y, '\u{f013}', fg, row_bg); // nf-fa-cog
        }
        if is_active && !is_kbd_sel {
            set_cell(buf, area.x, y, '▎', accent_fg, bar_bg);
        }
    }
}

// ─── Sidebar rendering ────────────────────────────────────────────────────────

fn render_sidebar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    sidebar: &mut TuiSidebar,
    engine: &Engine,
    theme: &Theme,
    explorer_drop_target: Option<usize>,
) {
    let header_fg = rc(theme.status_fg);
    let header_bg = rc(theme.status_bg);
    let default_fg = rc(theme.foreground);
    let row_bg = rc(theme.tab_bar_bg);
    let active_file_fg = rc(theme.keyword);
    let sel_bg = if sidebar.has_focus {
        rc(theme.sidebar_sel_bg)
    } else {
        rc(theme.sidebar_sel_bg_inactive)
    };
    let sel_fg = default_fg;

    // Extension panel (plugin-provided)
    if sidebar.ext_panel_name.is_some() {
        render_ext_panel(buf, area, engine, theme);
        return;
    }

    // Settings panel
    if sidebar.active_panel == TuiPanel::Settings {
        render_settings_panel(buf, area, theme, engine);
        return;
    }

    // Search panel
    if sidebar.active_panel == TuiPanel::Search {
        render_search_panel(buf, area, sidebar, engine, theme);
        return;
    }

    // Debug panel
    if sidebar.active_panel == TuiPanel::Debug {
        render_debug_sidebar(buf, area, engine, theme);
        return;
    }

    // Source Control panel
    if sidebar.active_panel == TuiPanel::Git {
        render_source_control(buf, area, engine, theme);
        return;
    }

    // Extensions panel
    if sidebar.active_panel == TuiPanel::Extensions {
        render_ext_sidebar(buf, area, engine, theme);
        return;
    }

    // AI assistant panel
    if sidebar.active_panel == TuiPanel::Ai {
        render_ai_sidebar(buf, area, engine, theme);
        return;
    }

    // Collect open buffer paths for highlighting active files
    let open_paths: Vec<PathBuf> = engine
        .buffer_manager
        .list()
        .into_iter()
        .filter_map(|id| {
            engine
                .buffer_manager
                .get(id)
                .and_then(|s| s.file_path.as_ref())
                .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
        })
        .collect();

    // ── Background fill — covers empty space below tree rows ────────────
    if area.height == 0 {
        return;
    }
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', default_fg, row_bg);
        }
    }

    let header_y = area.y;
    // Fill header
    for x in area.x..area.x + area.width {
        set_cell(buf, x, header_y, ' ', header_fg, header_bg);
    }
    // " EXPLORER" label
    let label = " EXPLORER";
    let mut x = area.x;
    for ch in label.chars() {
        if x >= area.x + area.width {
            break;
        }
        set_cell(buf, x, header_y, ch, header_fg, header_bg);
        x += 1;
    }
    // Toolbar buttons (right-aligned, Nerd Font icons):
    //   new-file  new-folder  delete  refresh  explorer-mode
    // Each icon occupies 2 terminal cols (Nerd Font) + 1 space = 3 cols per button.
    // EXPLORER_TOOLBAR_LEN = 9 (3 NF icons × 3 cols each).
    // When a file (not folder) is selected, new-file/new-folder icons are dimmed.
    let selected_is_dir = {
        let idx = sidebar.selected;
        idx < sidebar.rows.len() && sidebar.rows[idx].is_dir
    };
    let dim_fg = rc(theme.line_number_fg); // dimmed color for unavailable buttons
    let icons: &[(char, bool, ratatui::style::Color)] = &[
        (
            '\u{f15b}',
            selected_is_dir,
            if selected_is_dir { header_fg } else { dim_fg },
        ), // new file
        (
            '\u{f07b}',
            selected_is_dir,
            if selected_is_dir { header_fg } else { dim_fg },
        ), // new folder
        ('\u{f1f8}', true, header_fg), // delete
    ];
    let toolbar_len = EXPLORER_TOOLBAR_LEN;
    if toolbar_len < area.width {
        let mut tx = area.x + area.width - toolbar_len;
        for &(icon, _enabled, fg) in icons {
            set_cell(buf, tx, header_y, icon, fg, header_bg);
            tx += 2; // icon is 2-cols wide (Nerd Font)
            set_cell(buf, tx, header_y, ' ', header_fg, header_bg);
            tx += 1;
        }
    }

    // ── Tree rows ────────────────────────────────────────────────────────
    let tree_height = area.height.saturating_sub(1) as usize;
    let visible_rows = sidebar
        .rows
        .iter()
        .enumerate()
        .skip(sidebar.scroll_top)
        .take(tree_height);

    for (i, (row_idx, row)) in visible_rows.enumerate() {
        let screen_y = area.y + 1 + i as u16;
        if screen_y >= area.y + area.height {
            break;
        }

        // Fill row background
        for x in area.x..area.x + area.width {
            set_cell(buf, x, screen_y, ' ', default_fg, row_bg);
        }

        // Determine colours
        let is_selected = row_idx == sidebar.selected;
        let is_drop_target = explorer_drop_target == Some(row_idx);
        let canonical_path = row.path.canonicalize().unwrap_or_else(|_| row.path.clone());
        let is_active = open_paths.contains(&canonical_path);

        let drop_bg = rc(render::Color {
            r: 40,
            g: 60,
            b: 80,
        }); // muted blue highlight
        let (fg, bg) = if is_drop_target {
            (sel_fg, drop_bg)
        } else if is_selected {
            (sel_fg, sel_bg)
        } else if is_active {
            (active_file_fg, row_bg)
        } else {
            (default_fg, row_bg)
        };

        // Build row string: indent + chevron/icon + name
        let indent = "  ".repeat(row.depth);
        let prefix = if row.is_dir {
            if row.is_expanded {
                "\u{25be} " // ▾
            } else {
                "\u{25b8} " // ▸
            }
        } else {
            let ext = row.path.extension().and_then(|e| e.to_str()).unwrap_or("");
            // We format as "  {icon} " — two spaces, icon, space
            // Rendered char-by-char below
            let _ = ext; // used in the render step
            "  "
        };

        let mut x = area.x;
        // Indent
        for ch in indent.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, screen_y, ch, fg, bg);
            x += 1;
        }
        // Prefix (chevron or spaces)
        for ch in prefix.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, screen_y, ch, fg, bg);
            x += 1;
        }
        // File icon (only for files)
        if !row.is_dir {
            let ext = row.path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let icon = crate::icons::file_icon(ext);
            for ch in icon.chars() {
                if x >= area.x + area.width {
                    break;
                }
                set_cell(buf, x, screen_y, ch, fg, bg);
                x += 1;
            }
            // Space after icon
            if x < area.x + area.width {
                set_cell(buf, x, screen_y, ' ', fg, bg);
                x += 1;
            }
        }
        // Name — or inline rename input when active on this row
        let is_renaming = engine
            .explorer_rename
            .as_ref()
            .is_some_and(|r| r.path == row.path);
        if is_renaming {
            let rename = engine.explorer_rename.as_ref().unwrap();
            let input_bg = rc(theme.background);
            let input_fg = rc(theme.foreground);
            let input_start_x = x;
            // Render the input text
            for (byte_idx, ch) in rename.input.char_indices() {
                if x >= area.x + area.width {
                    break;
                }
                let is_cursor = byte_idx == rename.cursor;
                let cell_fg = if is_cursor { input_bg } else { input_fg };
                let cell_bg = if is_cursor { input_fg } else { input_bg };
                set_cell(buf, x, screen_y, ch, cell_fg, cell_bg);
                x += 1;
            }
            // Cursor at end of input (append position)
            if rename.cursor >= rename.input.len() && x < area.x + area.width {
                set_cell(buf, x, screen_y, ' ', input_bg, input_fg);
                x += 1;
            }
            // Fill remaining width with input background
            while x < area.x + area.width {
                set_cell(buf, x, screen_y, ' ', input_fg, input_bg);
                x += 1;
            }
            let _ = input_start_x;
        } else {
            for ch in row.name.chars() {
                if x >= area.x + area.width {
                    break;
                }
                set_cell(buf, x, screen_y, ch, fg, bg);
                x += 1;
            }
        }
    }

    // Vertical scrollbar (rightmost column, tree rows only — not header)
    let total_rows = sidebar.rows.len();
    let visible_rows_count = tree_height;
    if total_rows > visible_rows_count && area.width >= 2 {
        let track_fg = rc(theme.separator);
        let thumb_fg = rc(theme.status_fg);
        let sb_bg = rc(theme.tab_bar_bg);
        let track_h = visible_rows_count as f64;
        let thumb_size = ((visible_rows_count as f64 / total_rows as f64) * track_h)
            .ceil()
            .max(1.0) as u16;
        let thumb_top = ((sidebar.scroll_top as f64 / total_rows as f64) * track_h).floor() as u16;
        let sb_x = area.x + area.width - 1;
        for dy in 0..visible_rows_count as u16 {
            let y = area.y + 1 + dy; // +1 for header row
            if y >= area.y + area.height {
                break;
            }
            let in_thumb = dy >= thumb_top && dy < thumb_top + thumb_size;
            let ch = if in_thumb { '█' } else { '░' };
            let fg = if in_thumb { thumb_fg } else { track_fg };
            set_cell(buf, sb_x, y, ch, fg, sb_bg);
        }
    }
}

/// Render the settings panel — shows current key settings and the file path.
fn render_settings_panel(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    theme: &Theme,
    engine: &Engine,
) {
    use crate::core::settings::{setting_categories, SettingType, SETTING_DEFS};

    let header_fg = rc(theme.status_fg);
    let header_bg = rc(theme.status_bg);
    let fg = rc(theme.foreground);
    let bg = rc(theme.tab_bar_bg);
    let dim_fg = rc(theme.line_number_fg);
    let key_fg = rc(theme.keyword);
    let sel_bg = if engine.settings_has_focus {
        rc(theme.sidebar_sel_bg)
    } else {
        rc(theme.sidebar_sel_bg_inactive)
    };
    let cat_fg = rc(theme.keyword);

    if area.height == 0 {
        return;
    }

    // Fill background
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, bg);
        }
    }

    // Row 0: Header " SETTINGS"
    let header_y = area.y;
    for x in area.x..area.x + area.width {
        set_cell(buf, x, header_y, ' ', header_fg, header_bg);
    }
    let mut x = area.x;
    for ch in " SETTINGS".chars() {
        if x >= area.x + area.width {
            break;
        }
        set_cell(buf, x, header_y, ch, header_fg, header_bg);
        x += 1;
    }

    // Row 1: Search input
    let search_y = area.y + 1;
    if search_y < area.y + area.height {
        let search_bg = if engine.settings_input_active {
            rc(theme.sidebar_sel_bg)
        } else {
            bg
        };
        for x in area.x..area.x + area.width {
            set_cell(buf, x, search_y, ' ', fg, search_bg);
        }
        let mut x = area.x;
        set_cell(buf, x, search_y, ' ', dim_fg, search_bg);
        x += 1;
        set_cell(buf, x, search_y, '/', dim_fg, search_bg);
        x += 1;
        set_cell(buf, x, search_y, ' ', dim_fg, search_bg);
        x += 1;
        for ch in engine.settings_query.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, search_y, ch, fg, search_bg);
            x += 1;
        }
        if engine.settings_input_active && x < area.x + area.width {
            set_cell(buf, x, search_y, '█', fg, search_bg);
        }
    }

    // Rows 2+: scrollable form content
    let content_start = area.y + 2;
    let content_height = area.height.saturating_sub(2) as usize;
    if content_height == 0 {
        return;
    }

    let flat = engine.settings_flat_list();
    let cats = setting_categories();
    let total = flat.len();

    // Scrollbar column is the rightmost
    let sb_col = area.x + area.width - 1;
    let content_width = area.width.saturating_sub(1); // leave room for scrollbar

    let scroll = engine.settings_scroll_top;

    for vi in 0..content_height {
        let fi = scroll + vi;
        let y = content_start + vi as u16;
        if fi >= total {
            break;
        }

        use crate::core::engine::SettingsRow;
        let row = &flat[fi];
        let is_selected = fi == engine.settings_selected && engine.settings_has_focus;
        let row_bg = if is_selected { sel_bg } else { bg };

        // Fill row background
        for x in area.x..area.x + content_width {
            set_cell(buf, x, y, ' ', fg, row_bg);
        }

        let right_edge = area.x + content_width;

        match row {
            SettingsRow::CoreCategory(cat_idx) => {
                let collapsed = *cat_idx < engine.settings_collapsed.len()
                    && engine.settings_collapsed[*cat_idx];
                let arrow = if collapsed { '▶' } else { '▼' };
                let cat_name = if *cat_idx < cats.len() {
                    cats[*cat_idx]
                } else {
                    "?"
                };
                let mut x = area.x + 1;
                set_cell(buf, x, y, arrow, cat_fg, row_bg);
                x += 2;
                for ch in cat_name.chars() {
                    if x >= area.x + content_width {
                        break;
                    }
                    set_cell(buf, x, y, ch, cat_fg, row_bg);
                    x += 1;
                }
            }
            SettingsRow::ExtCategory(name) => {
                let collapsed = engine
                    .ext_settings_collapsed
                    .get(name)
                    .copied()
                    .unwrap_or(false);
                let arrow = if collapsed { '▶' } else { '▼' };
                // Use display_name if available, otherwise capitalize name
                let display = engine
                    .ext_available_manifests()
                    .into_iter()
                    .find(|m| &m.name == name)
                    .map(|m| m.display_name.clone())
                    .unwrap_or_else(|| name.clone());
                let mut x = area.x + 1;
                set_cell(buf, x, y, arrow, cat_fg, row_bg);
                x += 2;
                for ch in display.chars() {
                    if x >= area.x + content_width {
                        break;
                    }
                    set_cell(buf, x, y, ch, cat_fg, row_bg);
                    x += 1;
                }
            }
            SettingsRow::CoreSetting(idx) => {
                let def = &SETTING_DEFS[*idx];
                let mut x = area.x + 3;
                for ch in def.label.chars() {
                    if x >= area.x + content_width {
                        break;
                    }
                    set_cell(buf, x, y, ch, fg, row_bg);
                    x += 1;
                }

                let editing_this = engine.settings_editing == Some(*idx);

                match &def.setting_type {
                    SettingType::Bool => {
                        let val = engine.settings.get_value_str(def.key);
                        let display = if val == "true" { "[✓]" } else { "[ ]" };
                        let val_len = 3u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx;
                        for ch in display.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, key_fg, row_bg);
                            cx += 1;
                        }
                    }
                    SettingType::Integer { .. } => {
                        let display = if editing_this {
                            format!("{}█", engine.settings_edit_buf)
                        } else {
                            engine.settings.get_value_str(def.key)
                        };
                        let val_len = display.chars().count() as u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx.max(x);
                        for ch in display.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, key_fg, row_bg);
                            cx += 1;
                        }
                    }
                    SettingType::Enum(_) | SettingType::DynamicEnum(_) => {
                        let val = engine.settings.get_value_str(def.key);
                        let display = format!("{val} ▸");
                        let val_len = display.chars().count() as u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx.max(x);
                        for ch in display.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, key_fg, row_bg);
                            cx += 1;
                        }
                    }
                    SettingType::StringVal => {
                        let display = if editing_this {
                            format!("{}█", engine.settings_edit_buf)
                        } else {
                            let val = engine.settings.get_value_str(def.key);
                            if val.is_empty() {
                                "(empty)".to_string()
                            } else {
                                val
                            }
                        };
                        let max_val_width = content_width.saturating_sub(x - area.x + 2) as usize;
                        let truncated: String = display.chars().take(max_val_width).collect();
                        let val_len = truncated.chars().count() as u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx.max(x);
                        let val_fg = if editing_this { fg } else { dim_fg };
                        for ch in truncated.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, val_fg, row_bg);
                            cx += 1;
                        }
                    }
                    SettingType::BufferEditor => {
                        let display = match def.key {
                            "keymaps" => {
                                format!("{} defined ▸", engine.settings.keymaps.len())
                            }
                            "extension_registries" => {
                                format!(
                                    "{} configured ▸",
                                    engine.settings.extension_registries.len()
                                )
                            }
                            _ => "▸".to_string(),
                        };
                        let val_len = display.chars().count() as u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx.max(x);
                        for ch in display.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, key_fg, row_bg);
                            cx += 1;
                        }
                    }
                }
            }
            SettingsRow::ExtSetting(ext_name, ext_key) => {
                // Extension setting — render like core settings
                let def = engine.find_ext_setting_def(ext_name, ext_key);
                let label = def.as_ref().map(|d| d.label.as_str()).unwrap_or(ext_key);
                let mut x = area.x + 3;
                for ch in label.chars() {
                    if x >= area.x + content_width {
                        break;
                    }
                    set_cell(buf, x, y, ch, fg, row_bg);
                    x += 1;
                }

                let editing_this = engine
                    .ext_settings_editing
                    .as_ref()
                    .is_some_and(|(en, ek)| en == ext_name && ek == ext_key);
                let val = engine.get_ext_setting(ext_name, ext_key);
                let typ = def.as_ref().map(|d| d.r#type.as_str()).unwrap_or("string");

                match typ {
                    "bool" => {
                        let display = if val == "true" { "[✓]" } else { "[ ]" };
                        let val_len = 3u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx;
                        for ch in display.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, key_fg, row_bg);
                            cx += 1;
                        }
                    }
                    "enum" => {
                        let display = format!("{val} ▸");
                        let val_len = display.chars().count() as u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx.max(x);
                        for ch in display.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, key_fg, row_bg);
                            cx += 1;
                        }
                    }
                    _ => {
                        // string/integer
                        let display = if editing_this {
                            format!("{}█", engine.settings_edit_buf)
                        } else if val.is_empty() {
                            "(empty)".to_string()
                        } else {
                            val
                        };
                        let max_val_width = content_width.saturating_sub(x - area.x + 2) as usize;
                        let truncated: String = display.chars().take(max_val_width).collect();
                        let val_len = truncated.chars().count() as u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx.max(x);
                        let val_fg = if editing_this { fg } else { dim_fg };
                        for ch in truncated.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, val_fg, row_bg);
                            cx += 1;
                        }
                    }
                }
            }
        }
    }

    // Scrollbar
    if total > content_height && content_height > 0 {
        let track_len = content_height;
        let thumb_len = (content_height * content_height / total).max(1);
        let thumb_start = scroll * track_len / total;
        for i in 0..track_len {
            let y = content_start + i as u16;
            let ch = if i >= thumb_start && i < thumb_start + thumb_len {
                '█'
            } else {
                '░'
            };
            set_cell(buf, sb_col, y, ch, dim_fg, bg);
        }
    }
}

/// Return the visual display row (0-based, including file-header rows) for a result index.
fn result_idx_to_display_row(results: &[crate::core::ProjectMatch], target_idx: usize) -> usize {
    let mut row = 0usize;
    let mut last_file: Option<&std::path::Path> = None;
    for (idx, m) in results.iter().enumerate() {
        if last_file != Some(m.file.as_path()) {
            last_file = Some(m.file.as_path());
            row += 1; // file-header row
        }
        if idx == target_idx {
            return row;
        }
        row += 1;
    }
    0
}

/// Adjust `search_scroll_top` so that `selected_idx` is within the viewport.
/// Call this after changing the selection via keyboard — not during render.
fn ensure_search_selection_visible(
    results: &[crate::core::ProjectMatch],
    selected_idx: usize,
    scroll_top: &mut usize,
    results_height: usize,
) {
    if results.is_empty() || results_height == 0 {
        return;
    }
    let display_row = result_idx_to_display_row(results, selected_idx);
    if display_row < *scroll_top {
        *scroll_top = display_row;
    } else if display_row >= *scroll_top + results_height {
        *scroll_top = display_row + 1 - results_height;
    }
}

/// Map a visual row index (0-based from top of results area) to a `project_search_results` index.
///
/// The results area interleaves file-header rows (not selectable) with result rows.
/// Returns `None` if the row falls on a file header.
fn visual_row_to_result_idx(
    results: &[crate::core::ProjectMatch],
    visual_row: usize,
) -> Option<usize> {
    let mut row = 0usize;
    let mut last_file: Option<&std::path::Path> = None;
    for (idx, m) in results.iter().enumerate() {
        if last_file != Some(m.file.as_path()) {
            last_file = Some(m.file.as_path());
            if row == visual_row {
                return None; // file header row
            }
            row += 1;
        }
        if row == visual_row {
            return Some(idx);
        }
        row += 1;
    }
    None
}

/// Render the project search panel.
fn render_search_panel(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    sidebar: &mut TuiSidebar,
    engine: &Engine,
    theme: &Theme,
) {
    let header_fg = rc(theme.status_fg);
    let header_bg = rc(theme.status_bg);
    let fg = rc(theme.foreground);
    let bg = rc(theme.tab_bar_bg);
    let dim_fg = rc(theme.line_number_fg);
    let sel_fg = bg;
    let sel_bg = fg;
    let file_header_fg = rc(theme.keyword);

    if area.height == 0 {
        return;
    }

    // Fill background
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, bg);
        }
    }

    // Row 0: panel header " SEARCH"
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', header_fg, header_bg);
    }
    let mut x = area.x;
    for ch in " SEARCH".chars() {
        if x >= area.x + area.width {
            break;
        }
        set_cell(buf, x, area.y, ch, header_fg, header_bg);
        x += 1;
    }

    if area.height < 2 {
        return;
    }

    // Row 1: search input box  "[ query___ ]"
    let input_y = area.y + 1;
    let query = &engine.project_search_query;
    let input_bg = rc(theme.active_background);
    let input_fg = fg;
    // Draw bracket prefix
    set_cell(buf, area.x, input_y, '[', dim_fg, bg);
    let end_bracket_x = if area.width > 1 {
        area.x + area.width - 1
    } else {
        area.x
    };
    set_cell(buf, end_bracket_x, input_y, ']', dim_fg, bg);
    // Fill input background
    for x in (area.x + 1)..end_bracket_x {
        set_cell(buf, x, input_y, ' ', input_fg, input_bg);
    }
    // Render query text
    let mut x = area.x + 1;
    for ch in query.chars() {
        if x >= end_bracket_x {
            break;
        }
        set_cell(buf, x, input_y, ch, input_fg, input_bg);
        x += 1;
    }
    // Cursor blinking indicator: show │ at cursor position when in input mode
    if sidebar.search_input_mode && !sidebar.replace_input_focused && x < end_bracket_x {
        set_cell(buf, x, input_y, '\u{258f}', rc(theme.cursor), input_bg); // ▏
    }

    if area.height < 3 {
        return;
    }

    // Row 2: replace input box  "[ replace_ ]"
    let replace_y = area.y + 2;
    let replace_text = &engine.project_replace_text;
    let replace_bg = if sidebar.replace_input_focused && sidebar.search_input_mode {
        input_bg
    } else {
        rc(theme.tab_bar_bg) // dimmer when unfocused
    };
    set_cell(buf, area.x, replace_y, '[', dim_fg, bg);
    let rep_end_x = if area.width > 1 {
        area.x + area.width - 1
    } else {
        area.x
    };
    set_cell(buf, rep_end_x, replace_y, ']', dim_fg, bg);
    for x in (area.x + 1)..rep_end_x {
        set_cell(buf, x, replace_y, ' ', input_fg, replace_bg);
    }
    // Placeholder or actual text
    if replace_text.is_empty() && !(sidebar.replace_input_focused && sidebar.search_input_mode) {
        let placeholder = "Replace…";
        let mut x = area.x + 1;
        for ch in placeholder.chars() {
            if x >= rep_end_x {
                break;
            }
            set_cell(buf, x, replace_y, ch, dim_fg, replace_bg);
            x += 1;
        }
    } else {
        let mut x = area.x + 1;
        for ch in replace_text.chars() {
            if x >= rep_end_x {
                break;
            }
            set_cell(buf, x, replace_y, ch, input_fg, replace_bg);
            x += 1;
        }
        if sidebar.replace_input_focused && sidebar.search_input_mode && x < rep_end_x {
            set_cell(buf, x, replace_y, '\u{258f}', rc(theme.cursor), replace_bg);
        }
    }

    if area.height < 4 {
        return;
    }

    // Row 3: toggle indicators (Aa / Ab| / .* ) + hint
    let toggle_y = area.y + 3;
    for x in area.x..area.x + area.width {
        set_cell(buf, x, toggle_y, ' ', dim_fg, bg);
    }
    {
        let opts = &engine.project_search_options;
        let active_fg = rc(theme.keyword);
        let mut tx = area.x;

        // Helper: render a label with active/inactive coloring
        let draw_toggle =
            |buf: &mut ratatui::buffer::Buffer, label: &str, active: bool, x: &mut u16| {
                let color = if active { active_fg } else { dim_fg };
                for ch in label.chars() {
                    if *x >= area.x + area.width {
                        break;
                    }
                    set_cell(buf, *x, toggle_y, ch, color, bg);
                    *x += 1;
                }
                // Space separator
                if *x < area.x + area.width {
                    set_cell(buf, *x, toggle_y, ' ', dim_fg, bg);
                    *x += 1;
                }
            };

        draw_toggle(buf, "Aa", opts.case_sensitive, &mut tx);
        draw_toggle(buf, "Ab|", opts.whole_word, &mut tx);
        draw_toggle(buf, ".*", opts.use_regex, &mut tx);

        // Hint text
        let hint = "Alt+C/W/R/H";
        if tx + 1 < area.x + area.width {
            // Small gap
            tx += 1;
            for ch in hint.chars() {
                if tx >= area.x + area.width {
                    break;
                }
                set_cell(buf, tx, toggle_y, ch, dim_fg, bg);
                tx += 1;
            }
        }
    }

    if area.height < 5 {
        return;
    }

    // Row 4: status / hint line
    let status_y = area.y + 4;
    let status_text = if engine.project_search_results.is_empty() {
        if query.is_empty() {
            " Type to search, Enter to run"
        } else {
            &engine.message
        }
    } else {
        &engine.message
    };
    // We borrow status_text potentially as &engine.message which is a &str reference,
    // so we just render it directly.
    let mut x = area.x;
    for ch in status_text.chars() {
        if x >= area.x + area.width {
            break;
        }
        set_cell(buf, x, status_y, ch, dim_fg, bg);
        x += 1;
    }

    if area.height < 6 {
        return;
    }

    // Rows 5+: results
    let results = &engine.project_search_results;
    if results.is_empty() {
        return;
    }

    let results_start_y = area.y + 5;
    let results_height = area.height.saturating_sub(5) as usize;

    // Build the flat display list (file headers + result rows)
    struct DisplayRow {
        text: String,
        is_header: bool,
        result_idx: Option<usize>,
    }

    let mut display_rows: Vec<DisplayRow> = Vec::new();
    let root = &sidebar.root;
    let mut last_file: Option<&std::path::Path> = None;

    for (idx, m) in results.iter().enumerate() {
        if last_file != Some(m.file.as_path()) {
            last_file = Some(m.file.as_path());
            let rel = m.file.strip_prefix(root).unwrap_or(&m.file);
            display_rows.push(DisplayRow {
                text: rel.display().to_string(),
                is_header: true,
                result_idx: None,
            });
        }
        let snippet = format!("  {}: {}", m.line + 1, m.line_text.trim());
        display_rows.push(DisplayRow {
            text: snippet,
            is_header: false,
            result_idx: Some(idx),
        });
    }

    let total_display = display_rows.len();
    let max_scroll = total_display.saturating_sub(results_height);

    // Viewport scrolls freely — only clamped to valid range.
    // Selection-tracking happens in the keyboard / poll handlers, not here.
    let scroll_top = sidebar.search_scroll_top.min(max_scroll);
    sidebar.search_scroll_top = scroll_top;

    for (i, dr) in display_rows
        .iter()
        .skip(scroll_top)
        .take(results_height)
        .enumerate()
    {
        let screen_y = results_start_y + i as u16;
        if screen_y >= area.y + area.height {
            break;
        }

        // Fill row background first
        for x in area.x..area.x + area.width {
            set_cell(buf, x, screen_y, ' ', fg, bg);
        }

        let is_selected = !dr.is_header
            && dr.result_idx == Some(engine.project_search_selected)
            && !sidebar.search_input_mode;

        let (row_fg, row_bg) = if is_selected {
            (sel_fg, sel_bg)
        } else if dr.is_header {
            (file_header_fg, bg)
        } else {
            (fg, bg)
        };

        // Re-fill with correct bg for selected rows
        if is_selected || dr.is_header {
            for x in area.x..area.x + area.width {
                set_cell(buf, x, screen_y, ' ', row_fg, row_bg);
            }
        }

        let mut x = area.x;
        for ch in dr.text.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, screen_y, ch, row_fg, row_bg);
            x += 1;
        }
    }

    // Vertical scrollbar for results area
    let total_display = display_rows.len();
    if total_display > results_height && area.width >= 2 {
        let track_fg = rc(theme.separator);
        let thumb_fg = rc(theme.status_fg);
        let sb_bg = bg;
        let track_h = results_height as f64;
        let thumb_size = ((results_height as f64 / total_display as f64) * track_h)
            .ceil()
            .max(1.0) as u16;
        let thumb_top = ((scroll_top as f64 / total_display as f64) * track_h).floor() as u16;
        let sb_x = area.x + area.width - 1;
        for dy in 0..results_height as u16 {
            let y = results_start_y + dy;
            if y >= area.y + area.height {
                break;
            }
            let in_thumb = dy >= thumb_top && dy < thumb_top + thumb_size;
            let ch = if in_thumb { '█' } else { '░' };
            let fg_color = if in_thumb { thumb_fg } else { track_fg };
            set_cell(buf, sb_x, y, ch, fg_color, sb_bg);
        }
    }
}

/// Render a one-line prompt in the command area (used for sidebar CRUD input).
fn render_prompt_line(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    text: &str,
    cursor_char_pos: usize,
    theme: &Theme,
) {
    let fg = rc(theme.command_fg);
    let bg = rc(theme.command_bg);
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', fg, bg);
    }
    let mut x = area.x;
    let mut char_idx = 0;
    let mut cursor_x = None;
    for ch in text.chars() {
        if x >= area.x + area.width {
            break;
        }
        if char_idx == cursor_char_pos {
            cursor_x = Some(x);
        }
        set_cell(buf, x, area.y, ch, fg, bg);
        x += 1;
        char_idx += 1;
    }
    // If cursor is at the end (past all chars)
    if cursor_x.is_none() && char_idx == cursor_char_pos {
        cursor_x = Some(x);
    }
    // Show cursor (inverted colors)
    if let Some(cx) = cursor_x {
        if cx < area.x + area.width {
            let cell = buf.get_mut(cx, area.y);
            let old_fg = cell.fg;
            let old_bg = cell.bg;
            cell.set_fg(old_bg).set_bg(old_fg);
        }
    }
}

// ─── Wildmenu (command Tab completion bar) ───────────────────────────────────

fn render_wildmenu(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    wm: &WildmenuData,
    theme: &Theme,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let bg = rc(theme.wildmenu_bg);
    let fg = rc(theme.wildmenu_fg);
    let sel_bg = rc(theme.wildmenu_sel_bg);
    let sel_fg = rc(theme.wildmenu_sel_fg);

    // Fill background
    for x in area.x..area.x + area.width {
        let cell = buf.get_mut(x, area.y);
        cell.set_char(' ').set_fg(fg).set_bg(bg);
    }

    // Draw items separated by spaces
    let mut col = area.x;
    for (i, item) in wm.items.iter().enumerate() {
        if col >= area.x + area.width {
            break;
        }
        let is_selected = wm.selected == Some(i);
        let item_fg = if is_selected { sel_fg } else { fg };
        let item_bg = if is_selected { sel_bg } else { bg };

        // Leading space
        if col < area.x + area.width {
            buf.get_mut(col, area.y)
                .set_char(' ')
                .set_fg(item_fg)
                .set_bg(item_bg);
            col += 1;
        }

        for ch in item.chars() {
            if col >= area.x + area.width {
                break;
            }
            buf.get_mut(col, area.y)
                .set_char(ch)
                .set_fg(item_fg)
                .set_bg(item_bg);
            col += 1;
        }

        // Trailing space for selected item padding
        if is_selected && col < area.x + area.width {
            buf.get_mut(col, area.y)
                .set_char(' ')
                .set_fg(item_fg)
                .set_bg(item_bg);
            col += 1;
        }
    }
}

// ─── Status / command line ────────────────────────────────────────────────────

fn render_status_line(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    left: &str,
    right: &str,
    theme: &Theme,
) {
    let fg = rc(theme.status_fg);
    let bg = rc(theme.status_bg);

    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', fg, bg);
    }

    let right_chars: Vec<char> = right.chars().collect();
    let right_len = right_chars.len() as u16;
    let right_start = if right_len <= area.width {
        area.x + area.width - right_len
    } else {
        area.x + area.width
    };

    // Draw left text, stopping 1 col before right text to avoid overlap.
    let left_limit = if right_start > area.x {
        right_start - 1
    } else {
        area.x
    };
    let mut x = area.x;
    for ch in left.chars() {
        if x >= left_limit {
            break;
        }
        set_cell(buf, x, area.y, ch, fg, bg);
        x += 1;
    }

    // Draw right text, right-aligned.
    if right_len <= area.width {
        let mut rx = right_start;
        for &ch in &right_chars {
            if rx >= area.x + area.width {
                break;
            }
            set_cell(buf, rx, area.y, ch, fg, bg);
            rx += 1;
        }
    }
}

fn render_command_line(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    command: &render::CommandLineData,
    theme: &Theme,
) {
    let fg = rc(theme.command_fg);
    let bg = rc(theme.command_bg);

    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', fg, bg);
    }

    if command.right_align {
        let chars: Vec<char> = command.text.chars().collect();
        let len = chars.len() as u16;
        if len <= area.width {
            let mut x = area.x + area.width - len;
            for &ch in &chars {
                if x >= area.x + area.width {
                    break;
                }
                set_cell(buf, x, area.y, ch, fg, bg);
                x += 1;
            }
        }
    } else {
        let mut x = area.x;
        for ch in command.text.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, area.y, ch, fg, bg);
            x += 1;
        }
    }

    // Command-line cursor (inverted block at insertion point)
    if command.show_cursor {
        let cursor_col = command.cursor_anchor_text.chars().count() as u16;
        let cx = area.x + cursor_col.min(area.width.saturating_sub(1));
        let buf_area = buf.area;
        if cx < buf_area.x + buf_area.width {
            let cell = buf.get_mut(cx, area.y);
            let old_fg = cell.fg;
            let old_bg = cell.bg;
            cell.set_fg(old_bg).set_bg(old_fg);
        }
    }
}

// ─── Input translation ────────────────────────────────────────────────────────

fn translate_key(event: KeyEvent, keyboard_enhanced: bool) -> Option<(String, Option<char>, bool)> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
    let shift = event.modifiers.contains(KeyModifiers::SHIFT);
    match event.code {
        KeyCode::Char(c) => {
            let lower = c.to_ascii_lowercase();
            let (key_name, unicode) = if ctrl {
                // Engine dispatches Ctrl combos via key_name (e.g. "d" for Ctrl-D).
                // Space is a named key; use "space" to match GTK and the engine's convention.
                // Ctrl+Shift+X: the char arrives as uppercase (or SHIFT flag is set); keep
                // uppercase so the engine can distinguish Ctrl+P from Ctrl+Shift+P ("P").
                // Some special chars use GTK-style names to match GTK backend conventions.
                let name = if lower == ' ' {
                    "space".to_string()
                } else if lower == '\\' || (!keyboard_enhanced && lower == '4') {
                    // Ctrl+\ sends byte 0x1C; without keyboard enhancement crossterm decodes
                    // 0x1C as KeyCode::Char('4')+CONTROL (formula: 0x1C-0x1C+'4'='4').
                    // Map both to "backslash" so Ctrl+\ works in all terminals.
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
                ("".to_string(), Some(c))
            };
            Some((key_name, unicode, ctrl))
        }
        KeyCode::Esc => Some(("Escape".to_string(), None, false)),
        KeyCode::Enter if shift && ctrl => Some(("Shift_Return".to_string(), None, true)),
        KeyCode::Enter if ctrl => Some(("Return".to_string(), None, true)),
        KeyCode::Enter => Some(("Return".to_string(), None, false)),
        KeyCode::Backspace => Some(("BackSpace".to_string(), None, false)),
        KeyCode::Delete => Some(("Delete".to_string(), None, false)),
        KeyCode::Tab => Some(("Tab".to_string(), None, ctrl)),
        KeyCode::BackTab => Some(("ISO_Left_Tab".to_string(), None, ctrl)),
        // Shift+Arrow (no ctrl): emit as "Shift_X" for VSCode selection extension.
        KeyCode::Up if shift && !ctrl => Some(("Shift_Up".to_string(), None, false)),
        KeyCode::Down if shift && !ctrl => Some(("Shift_Down".to_string(), None, false)),
        KeyCode::Left if shift && !ctrl => Some(("Shift_Left".to_string(), None, false)),
        KeyCode::Right if shift && !ctrl => Some(("Shift_Right".to_string(), None, false)),
        KeyCode::Home if shift => Some(("Shift_Home".to_string(), None, false)),
        KeyCode::End if shift => Some(("Shift_End".to_string(), None, false)),
        // Ctrl+Shift+Arrow: emit as "Shift_X" with ctrl=true for word-level selection.
        KeyCode::Left if shift && ctrl => Some(("Shift_Left".to_string(), None, true)),
        KeyCode::Right if shift && ctrl => Some(("Shift_Right".to_string(), None, true)),
        KeyCode::Up => Some(("Up".to_string(), None, false)),
        KeyCode::Down => Some(("Down".to_string(), None, false)),
        KeyCode::Left => Some(("Left".to_string(), None, ctrl)),
        KeyCode::Right => Some(("Right".to_string(), None, ctrl)),
        KeyCode::Home => Some(("Home".to_string(), None, ctrl)),
        KeyCode::End => Some(("End".to_string(), None, ctrl)),
        KeyCode::PageUp => Some(("Page_Up".to_string(), None, false)),
        KeyCode::PageDown => Some(("Page_Down".to_string(), None, false)),
        KeyCode::F(n) => Some((format!("F{}", n), None, false)),
        _ => None,
    }
}

// ─── Terminal PTY key translation ────────────────────────────────────────────

/// Translate a crossterm key event to PTY input bytes.
/// Returns an empty vec for keys with no PTY mapping.
fn translate_key_to_pty(event: KeyEvent) -> Vec<u8> {
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
    match event.code {
        KeyCode::Char(c) if ctrl => {
            let b = c.to_ascii_lowercase() as u8;
            if b.is_ascii() {
                vec![b & 0x1f]
            } else {
                vec![]
            }
        }
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Backspace => b"\x7f".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::Esc => b"\x1b".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::F(1) => b"\x1bOP".to_vec(),
        KeyCode::F(2) => b"\x1bOQ".to_vec(),
        KeyCode::F(3) => b"\x1bOR".to_vec(),
        KeyCode::F(4) => b"\x1bOS".to_vec(),
        KeyCode::F(5) => b"\x1b[15~".to_vec(),
        KeyCode::F(6) => b"\x1b[17~".to_vec(),
        KeyCode::F(7) => b"\x1b[18~".to_vec(),
        KeyCode::F(8) => b"\x1b[19~".to_vec(),
        KeyCode::F(9) => b"\x1b[20~".to_vec(),
        KeyCode::F(10) => b"\x1b[21~".to_vec(),
        KeyCode::F(11) => b"\x1b[23~".to_vec(),
        KeyCode::F(12) => b"\x1b[24~".to_vec(),
        _ => vec![],
    }
}

// ─── Engine action handling ───────────────────────────────────────────────────

fn handle_action(engine: &mut Engine, action: EngineAction) -> bool {
    match action {
        EngineAction::Quit | EngineAction::SaveQuit => {
            engine.cleanup_all_swaps();
            engine.lsp_shutdown();
            save_session(engine);
            true
        }
        EngineAction::OpenFile(path) => {
            if let Err(e) = engine.open_file_with_mode(&path, OpenMode::Permanent) {
                engine.message = e;
            }
            false
        }
        EngineAction::OpenTerminal | EngineAction::RunInTerminal(_) => false, // TUI handles terminal open in main event loop
        EngineAction::OpenFolderDialog
        | EngineAction::OpenWorkspaceDialog
        | EngineAction::SaveWorkspaceAsDialog
        | EngineAction::OpenRecentDialog => false, // handled by caller
        EngineAction::QuitWithUnsaved => false, // handled by caller (shows quit confirm overlay)
        EngineAction::ToggleSidebar => false,   // handled by caller (has access to sidebar state)
        EngineAction::QuitWithError => {
            engine.cleanup_all_swaps();
            engine.lsp_shutdown();
            save_session(engine);
            std::process::exit(1);
        }
        EngineAction::None | EngineAction::Error => false,
    }
}

fn save_session(engine: &mut Engine) {
    let buffer_id = engine.active_buffer_id();
    if let Some(path) = engine
        .buffer_manager
        .get(buffer_id)
        .and_then(|s| s.file_path.as_deref())
        .map(|p| p.to_path_buf())
    {
        let view = engine.active_window().view.clone();
        engine.session.save_file_position(
            &path,
            view.cursor.line,
            view.cursor.col,
            view.scroll_top,
        );
    }
    engine.collect_session_open_files();
    if let Some(ref root) = engine.workspace_root.clone() {
        engine.save_session_for_workspace(root);
    }
    let _ = engine.session.save();
}

// ─── Source Control sidebar panel ────────────────────────────────────────────

/// Render the Source Control sidebar: header strip, Staged Changes, Changes, Worktrees.
fn render_source_control(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let hdr_fg = rc(theme.status_fg);
    let hdr_bg = rc(theme.status_bg);
    // Clear the entire area first to prevent stale content from previous renders.
    {
        let clear_fg = rc(theme.foreground);
        let clear_bg = rc(theme.tab_bar_bg);
        for cy in area.y..area.y + area.height {
            for cx in area.x..area.x + area.width {
                set_cell(buf, cx, cy, ' ', clear_fg, clear_bg);
            }
        }
    }
    let item_fg = rc(theme.foreground);
    let dim_fg = rc(theme.line_number_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let row_bg = rc(theme.tab_bar_bg);
    let add_fg = RColor::Rgb(90, 180, 90);
    let del_fg = RColor::Rgb(220, 70, 60);
    let mod_fg = RColor::Rgb(220, 180, 80);

    // Build SC data from engine state via the render abstraction.
    let screen = render::build_screen_layout(engine, theme, &[], 1.0, 1.0, true);
    let Some(ref sc) = screen.source_control else {
        return;
    };

    // Reserve bottom row for hint bar when focused.
    let area = if sc.has_focus && area.height > 2 {
        let hint_y = area.y + area.height - 1;
        let hint_text = " Press '?' for help";
        for cx in area.x..area.x + area.width {
            set_cell(buf, cx, hint_y, ' ', dim_fg, hdr_bg);
        }
        for (i, ch) in hint_text.chars().enumerate().take(area.width as usize) {
            set_cell(buf, area.x + i as u16, hint_y, ch, dim_fg, hdr_bg);
        }
        Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height - 1,
        }
    } else {
        area
    };

    // ── Row 0: header "SOURCE CONTROL" ──────────────────────────────────────
    let branch_info = if sc.ahead > 0 || sc.behind > 0 {
        format!(
            "  \u{e702} SOURCE CONTROL  {}  \u{2191}{} \u{2193}{}",
            sc.branch, sc.ahead, sc.behind
        )
    } else {
        format!("  \u{e702} SOURCE CONTROL  {}", sc.branch)
    };
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', hdr_fg, hdr_bg);
    }
    for (i, ch) in branch_info.chars().enumerate().take(area.width as usize) {
        set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
    }

    if area.height < 2 {
        return;
    }

    // ── Row 1: commit input row ───────────────────────────────────────────────
    {
        let commit_y = area.y + 1;
        let inp_bg = if sc.commit_input_active {
            sel_bg
        } else {
            row_bg
        };
        for x in area.x..area.x + area.width {
            set_cell(buf, x, commit_y, ' ', item_fg, inp_bg);
        }
        let prompt = if sc.commit_input_active {
            format!(" \u{f044}  {}|", sc.commit_message)
        } else if sc.commit_message.is_empty() {
            " \u{f044}  Message (press c)".to_string()
        } else {
            format!(" \u{f044}  {}", sc.commit_message)
        };
        let prompt_fg = if sc.commit_input_active {
            item_fg
        } else {
            dim_fg
        };
        for (i, ch) in prompt.chars().enumerate().take(area.width as usize) {
            set_cell(buf, area.x + i as u16, commit_y, ch, prompt_fg, inp_bg);
        }
    }

    if area.height < 3 {
        return;
    }

    // ── Row 2: action buttons ────────────────────────────────────────────────
    {
        // Commit gets ~50% of the width (with label text).
        // Push / Pull / Sync get equal shares of the remaining width, icon only.
        let btn_y = area.y + 2;
        let commit_w = (area.width / 2).max(1);
        let remain = area.width.saturating_sub(commit_w);
        let icon_w = (remain / 3).max(1);

        // (x_offset_from_area_x, segment_width, display_text, button_index)
        let buttons: [(u16, u16, &str, usize); 4] = [
            (0, commit_w, " \u{e729} Commit", 0),
            (commit_w, icon_w, " \u{f093}", 1),
            (commit_w + icon_w, icon_w, " \u{f019}", 2),
            (
                commit_w + icon_w * 2,
                area.width.saturating_sub(commit_w + icon_w * 2),
                " \u{f021}",
                3,
            ),
        ];
        for (x_off, seg_w, text, btn_idx) in &buttons {
            let bx = area.x + x_off;
            let seg_end = if *btn_idx == 3 {
                area.x + area.width
            } else {
                (bx + seg_w).min(area.x + area.width)
            };
            let is_focused = sc.button_focused == Some(*btn_idx);
            let (fg, bg) = if is_focused {
                (hdr_bg, hdr_fg) // inverted = highlighted
            } else {
                (hdr_fg, row_bg)
            };
            for px in bx..seg_end {
                set_cell(buf, px, btn_y, ' ', fg, bg);
            }
            for (j, ch) in text.chars().enumerate() {
                let cx = bx + j as u16;
                if cx < seg_end {
                    set_cell(buf, cx, btn_y, ch, fg, bg);
                }
            }
        }
    }

    if area.height < 4 {
        return;
    }

    // Sections: staged, unstaged, and optionally worktrees (only when linked worktrees exist).
    #[allow(clippy::type_complexity)]
    let sections: [(
        &str,
        &[render::ScFileItem],
        Option<&[render::ScWorktreeItem]>,
        usize,
    ); 3] = [
        ("\u{f055} STAGED CHANGES", &sc.staged, None, 0),
        ("\u{f02b} CHANGES", &sc.unstaged, None, 1),
        ("\u{e702} WORKTREES", &[], Some(&sc.worktrees), 2),
    ];
    // Only show WORKTREES section when there are linked worktrees (>1 total).
    let show_worktrees = sc.worktrees.len() > 1;

    let mut row_y = area.y + 3; // start after header + commit row + button row
    let max_y = area.y + area.height;
    let mut flat_row: usize = 0; // tracks position in flat selection space

    for (section_label, file_items, wt_items, sec_idx) in &sections {
        // Skip the WORKTREES section unless there are linked worktrees.
        if *sec_idx == 2 && !show_worktrees {
            continue;
        }
        if row_y >= max_y {
            break;
        }
        let is_expanded = sc.sections_expanded[*sec_idx];
        let expand_icon = if is_expanded { '\u{25bc}' } else { '\u{25b6}' }; // ▼ / ▶

        // Section header row
        let is_hdr_selected = sc.has_focus && sc.selected == flat_row;
        let (hdr_r_fg, hdr_r_bg) = if is_hdr_selected {
            (hdr_fg, sel_bg)
        } else {
            (hdr_fg, hdr_bg)
        };
        for x in area.x..area.x + area.width {
            set_cell(buf, x, row_y, ' ', hdr_r_fg, hdr_r_bg);
        }
        // Expand icon + label
        let hdr_text = format!(" {} {}", expand_icon, section_label);
        // Item count badge
        let badge_text = {
            let count = if *sec_idx == 2 {
                wt_items.map(|v| v.len()).unwrap_or(0)
            } else {
                file_items.len()
            };
            if count > 0 {
                format!(" ({})", count)
            } else {
                String::new()
            }
        };
        let full_hdr = format!("{}{}", hdr_text, badge_text);
        for (i, ch) in full_hdr.chars().enumerate().take(area.width as usize) {
            set_cell(buf, area.x + i as u16, row_y, ch, hdr_r_fg, hdr_r_bg);
        }
        row_y += 1;
        flat_row += 1;

        if !is_expanded {
            continue;
        }

        // Items
        let item_count = if *sec_idx == 2 {
            wt_items.map(|v| v.len()).unwrap_or(0)
        } else {
            file_items.len()
        };

        // Determine if we need a scrollbar.
        // For simplicity we don't scroll SC sections (they're usually small).
        // Reserve rightmost col for scrollbar if needed.
        let need_sb = item_count > (max_y.saturating_sub(row_y)) as usize;
        let text_w = if need_sb {
            (area.width as usize).saturating_sub(1)
        } else {
            area.width as usize
        };

        if item_count == 0 {
            // "(no changes)" hint
            if row_y < max_y {
                let hint = "  (no changes)";
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, row_y, ' ', dim_fg, row_bg);
                }
                for (i, ch) in hint.chars().enumerate().take(area.width as usize) {
                    set_cell(buf, area.x + i as u16, row_y, ch, dim_fg, row_bg);
                }
                row_y += 1;
            }
        } else if *sec_idx == 2 {
            // Worktrees section
            if let Some(wts) = wt_items {
                for wt in *wts {
                    if row_y >= max_y {
                        break;
                    }
                    let is_selected = sc.has_focus && sc.selected == flat_row;
                    let (row_fg, r_bg) = if is_selected {
                        (item_fg, sel_bg)
                    } else {
                        (item_fg, row_bg)
                    };
                    for x in area.x..area.x + area.width {
                        set_cell(buf, x, row_y, ' ', row_fg, r_bg);
                    }
                    let check = if wt.is_current { '\u{2713}' } else { ' ' }; // ✓
                    let main_marker = if wt.is_main { " [main]" } else { "" };
                    let text = format!("  {} {} {}{}", check, wt.branch, wt.path, main_marker);
                    for (i, ch) in text.chars().enumerate().take(text_w) {
                        set_cell(buf, area.x + i as u16, row_y, ch, row_fg, r_bg);
                    }
                    row_y += 1;
                    flat_row += 1;
                }
            }
        } else {
            // File items (staged or unstaged)
            for fi in *file_items {
                if row_y >= max_y {
                    break;
                }
                let is_selected = sc.has_focus && sc.selected == flat_row;
                let r_bg = if is_selected { sel_bg } else { row_bg };
                let status_color = match fi.status_char {
                    'A' => add_fg,
                    'D' => del_fg,
                    _ => mod_fg,
                };
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, row_y, ' ', dim_fg, r_bg);
                }
                // Status char colored, then path dimmed
                let prefix = format!("  {} ", fi.status_char);
                for (i, ch) in prefix.chars().enumerate().take(text_w) {
                    let ch_fg = if ch == fi.status_char {
                        status_color
                    } else {
                        dim_fg
                    };
                    set_cell(buf, area.x + i as u16, row_y, ch, ch_fg, r_bg);
                }
                let path_start = prefix.chars().count();
                let path_color = if is_selected { item_fg } else { dim_fg };
                for (i, ch) in fi.path.chars().enumerate() {
                    let col = path_start + i;
                    if col >= text_w {
                        break;
                    }
                    set_cell(buf, area.x + col as u16, row_y, ch, path_color, r_bg);
                }
                row_y += 1;
                flat_row += 1;
            }
        }
    }

    // ── Log section (RECENT COMMITS) ─────────────────────────────────────────
    if row_y < max_y {
        let is_expanded = sc.sections_expanded[3];
        let expand_icon = if is_expanded { '\u{25bc}' } else { '\u{25b6}' };

        // Section header
        let is_hdr_selected = sc.has_focus && sc.selected == flat_row;
        let (log_hdr_fg, log_hdr_bg) = if is_hdr_selected {
            (hdr_fg, sel_bg)
        } else {
            (hdr_fg, hdr_bg)
        };
        for x in area.x..area.x + area.width {
            set_cell(buf, x, row_y, ' ', log_hdr_fg, log_hdr_bg);
        }
        let count_badge = if !sc.log.is_empty() {
            format!(" ({})", sc.log.len())
        } else {
            String::new()
        };
        let hdr_text = format!(" {} \u{f417} RECENT COMMITS{}", expand_icon, count_badge);
        for (i, ch) in hdr_text.chars().enumerate().take(area.width as usize) {
            set_cell(buf, area.x + i as u16, row_y, ch, log_hdr_fg, log_hdr_bg);
        }
        row_y += 1;
        flat_row += 1;

        if is_expanded && row_y < max_y {
            if sc.log.is_empty() {
                // "(no commits)" hint
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, row_y, ' ', dim_fg, row_bg);
                }
                let hint = "  (no commits)";
                for (i, ch) in hint.chars().enumerate().take(area.width as usize) {
                    set_cell(buf, area.x + i as u16, row_y, ch, dim_fg, row_bg);
                }
                row_y += 1;
            } else {
                for entry in &sc.log {
                    if row_y >= max_y {
                        break;
                    }
                    let is_selected = sc.has_focus && sc.selected == flat_row;
                    let r_bg = if is_selected { sel_bg } else { row_bg };
                    for x in area.x..area.x + area.width {
                        set_cell(buf, x, row_y, ' ', dim_fg, r_bg);
                    }
                    // Hash in dim color, message in item_fg
                    let hash_text = format!("  {} ", entry.hash);
                    let hash_w = hash_text.chars().count();
                    for (i, ch) in hash_text.chars().enumerate().take(area.width as usize) {
                        set_cell(buf, area.x + i as u16, row_y, ch, dim_fg, r_bg);
                    }
                    let msg_fg = if is_selected { item_fg } else { dim_fg };
                    for (i, ch) in entry.message.chars().enumerate() {
                        let col = hash_w + i;
                        if col >= area.width as usize {
                            break;
                        }
                        set_cell(buf, area.x + col as u16, row_y, ch, msg_fg, r_bg);
                    }
                    row_y += 1;
                    flat_row += 1;
                }
            }
        }
    }
    let _ = (row_y, flat_row); // consumed by rendering loop

    // ── Branch picker / create popup ─────────────────────────────────────────
    if let Some(ref bp) = sc.branch_picker {
        let popup_bg = rc(theme.completion_bg);
        let popup_fg = rc(theme.completion_fg);
        let popup_border = rc(theme.completion_border);
        let popup_sel = rc(theme.completion_selected_bg);
        let popup_w = area.width.saturating_sub(2).min(40);
        let popup_h = if bp.create_mode {
            3u16
        } else {
            area.height.saturating_sub(4).min(15)
        };
        let popup_x = area.x + (area.width.saturating_sub(popup_w)) / 2;
        let popup_y = area.y + 2;
        // Clear popup area
        for y in popup_y..popup_y + popup_h {
            for x in popup_x..popup_x + popup_w {
                set_cell(buf, x, y, ' ', popup_fg, popup_bg);
            }
        }
        // Top border
        if popup_w >= 2 {
            set_cell(buf, popup_x, popup_y, '┌', popup_border, popup_bg);
            set_cell(
                buf,
                popup_x + popup_w - 1,
                popup_y,
                '┐',
                popup_border,
                popup_bg,
            );
            for x in popup_x + 1..popup_x + popup_w - 1 {
                set_cell(buf, x, popup_y, '─', popup_border, popup_bg);
            }
            let title = if bp.create_mode {
                " New Branch "
            } else {
                " Switch Branch "
            };
            let title_x = popup_x + 1;
            for (i, ch) in title.chars().enumerate() {
                let x = title_x + i as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, popup_y, ch, popup_border, popup_bg);
                }
            }
        }
        if bp.create_mode {
            let iy = popup_y + 1;
            let label = "Name: ";
            for (i, ch) in label.chars().enumerate() {
                let x = popup_x + 1 + i as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, iy, ch, dim_fg, popup_bg);
                }
            }
            let input_x = popup_x + 1 + label.len() as u16;
            for (i, ch) in bp.create_input.chars().enumerate() {
                let x = input_x + i as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, iy, ch, popup_fg, popup_bg);
                }
            }
            let cx = input_x + bp.create_input.len() as u16;
            if cx < popup_x + popup_w - 1 {
                set_cell(buf, cx, iy, '▏', popup_fg, popup_bg);
            }
            let by = popup_y + popup_h - 1;
            set_cell(buf, popup_x, by, '└', popup_border, popup_bg);
            set_cell(buf, popup_x + popup_w - 1, by, '┘', popup_border, popup_bg);
            for x in popup_x + 1..popup_x + popup_w - 1 {
                set_cell(buf, x, by, '─', popup_border, popup_bg);
            }
        } else {
            let iy = popup_y + 1;
            let prefix = " \u{f002} ";
            for (i, ch) in prefix.chars().enumerate() {
                let x = popup_x + i as u16;
                if x < popup_x + popup_w {
                    set_cell(buf, x, iy, ch, dim_fg, popup_bg);
                }
            }
            let qx = popup_x + prefix.chars().count() as u16;
            for (i, ch) in bp.query.chars().enumerate() {
                let x = qx + i as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, iy, ch, popup_fg, popup_bg);
                }
            }
            let list_y = popup_y + 2;
            let list_h = popup_h.saturating_sub(3) as usize;
            let scroll_off = if bp.selected >= list_h {
                bp.selected - list_h + 1
            } else {
                0
            };
            for (vi, (name, is_current)) in
                bp.results.iter().skip(scroll_off).take(list_h).enumerate()
            {
                let y = list_y + vi as u16;
                let is_sel = vi + scroll_off == bp.selected;
                let bg = if is_sel { popup_sel } else { popup_bg };
                for x in popup_x..popup_x + popup_w {
                    set_cell(buf, x, y, ' ', popup_fg, bg);
                }
                let marker = if *is_current { "● " } else { "  " };
                let display = format!("{marker}{name}");
                for (i, ch) in display.chars().enumerate() {
                    let x = popup_x + 1 + i as u16;
                    if x < popup_x + popup_w - 1 {
                        set_cell(buf, x, y, ch, popup_fg, bg);
                    }
                }
            }
            let by = popup_y + popup_h - 1;
            if by >= list_y {
                set_cell(buf, popup_x, by, '└', popup_border, popup_bg);
                set_cell(buf, popup_x + popup_w - 1, by, '┘', popup_border, popup_bg);
                for x in popup_x + 1..popup_x + popup_w - 1 {
                    set_cell(buf, x, by, '─', popup_border, popup_bg);
                }
            }
        }
        // Side borders
        for y in popup_y + 1..popup_y + popup_h.saturating_sub(1) {
            set_cell(buf, popup_x, y, '│', popup_border, popup_bg);
            if popup_x + popup_w > 0 {
                set_cell(buf, popup_x + popup_w - 1, y, '│', popup_border, popup_bg);
            }
        }
    }

    // ── Help dialog ──────────────────────────────────────────────────────────
    if sc.help_open {
        let popup_bg = rc(theme.completion_bg);
        let popup_fg = rc(theme.completion_fg);
        let popup_border = rc(theme.completion_border);
        let bindings: &[(&str, &str)] = &[
            ("j/k", "Navigate"),
            ("s", "Stage / unstage"),
            ("S", "Stage all"),
            ("d", "Discard file"),
            ("D", "Discard all unstaged"),
            ("c", "Commit message"),
            ("b", "Switch branch"),
            ("B", "Create branch"),
            ("p", "Push"),
            ("P", "Pull"),
            ("f", "Fetch"),
            ("r", "Refresh"),
            ("Tab", "Expand / collapse"),
            ("Enter", "Open file"),
            ("q/Esc", "Close panel"),
        ];
        let popup_w = area.width.saturating_sub(2).min(36);
        let popup_h = (bindings.len() as u16 + 3).min(area.height.saturating_sub(2));
        let popup_x = area.x + (area.width.saturating_sub(popup_w)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(popup_h)) / 2;
        for y in popup_y..popup_y + popup_h {
            for x in popup_x..popup_x + popup_w {
                set_cell(buf, x, y, ' ', popup_fg, popup_bg);
            }
        }
        set_cell(buf, popup_x, popup_y, '┌', popup_border, popup_bg);
        set_cell(
            buf,
            popup_x + popup_w - 1,
            popup_y,
            '┐',
            popup_border,
            popup_bg,
        );
        for x in popup_x + 1..popup_x + popup_w - 1 {
            set_cell(buf, x, popup_y, '─', popup_border, popup_bg);
        }
        let title = " Keybindings ";
        let tx = popup_x + (popup_w.saturating_sub(title.len() as u16)) / 2;
        for (i, ch) in title.chars().enumerate() {
            let x = tx + i as u16;
            if x > popup_x && x < popup_x + popup_w - 1 {
                set_cell(buf, x, popup_y, ch, popup_border, popup_bg);
            }
        }
        // Close hint
        let close_x = popup_x + popup_w - 2;
        if close_x > popup_x {
            set_cell(buf, close_x, popup_y, 'x', popup_border, popup_bg);
        }
        let key_fg = rc(theme.function);
        for (i, (key, desc)) in bindings.iter().enumerate() {
            let y = popup_y + 1 + i as u16;
            if y >= popup_y + popup_h - 1 {
                break;
            }
            for (j, ch) in key.chars().enumerate() {
                let x = popup_x + 2 + j as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, y, ch, key_fg, popup_bg);
                }
            }
            let desc_x = popup_x + 12;
            for (j, ch) in desc.chars().enumerate() {
                let x = desc_x + j as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, y, ch, popup_fg, popup_bg);
                }
            }
        }
        let by = popup_y + popup_h - 1;
        set_cell(buf, popup_x, by, '└', popup_border, popup_bg);
        set_cell(buf, popup_x + popup_w - 1, by, '┘', popup_border, popup_bg);
        for x in popup_x + 1..popup_x + popup_w - 1 {
            set_cell(buf, x, by, '─', popup_border, popup_bg);
        }
        for y in popup_y + 1..popup_y + popup_h - 1 {
            set_cell(buf, popup_x, y, '│', popup_border, popup_bg);
            set_cell(buf, popup_x + popup_w - 1, y, '│', popup_border, popup_bg);
        }
    }
}

// ─── Extension panel (plugin-provided) ───────────────────────────────────────

/// Render an extension-provided sidebar panel.
fn render_ext_panel(buf: &mut ratatui::buffer::Buffer, area: Rect, engine: &Engine, theme: &Theme) {
    use crate::core::plugin::ExtPanelStyle;

    if area.height == 0 {
        return;
    }
    let screen = render::build_screen_layout(engine, theme, &[], 1.0, 1.0, true);
    let Some(ref panel) = screen.ext_panel else {
        return;
    };

    let hdr_fg = rc(theme.status_fg);
    let hdr_bg = rc(theme.status_bg);
    let item_fg = rc(theme.foreground);
    let dim_fg = rc(theme.line_number_fg);
    let accent_fg = rc(theme.keyword);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let row_bg = rc(theme.tab_bar_bg);

    // Clear area
    for cy in area.y..area.y + area.height {
        for cx in area.x..area.x + area.width {
            set_cell(buf, cx, cy, ' ', item_fg, row_bg);
        }
    }

    // Row 0: header
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', hdr_fg, hdr_bg);
    }
    let title = format!("  {}", panel.title);
    for (i, ch) in title.chars().enumerate().take(area.width as usize) {
        set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
    }

    if area.height < 2 {
        return;
    }

    // Build flat list of rows
    let content_area_height = (area.height - 1) as usize;
    let mut flat_rows: Vec<(String, String, bool, bool)> = Vec::new(); // (text, hint, is_header, is_selected)
    let mut flat_idx = 0usize;
    for section in &panel.sections {
        let is_sel = flat_idx == panel.selected;
        let arrow = if section.expanded { "▼" } else { "▶" };
        flat_rows.push((
            format!(" {} {}", arrow, section.name),
            String::new(),
            true,
            is_sel,
        ));
        flat_idx += 1;
        if section.expanded {
            for item in &section.items {
                let is_sel = flat_idx == panel.selected;
                let indent = "  ".repeat(item.indent as usize + 1);
                let icon_part = if item.icon.is_empty() {
                    String::new()
                } else {
                    format!("{} ", item.icon)
                };
                let fg_marker = match item.style {
                    ExtPanelStyle::Header => 'H',
                    ExtPanelStyle::Dim => 'D',
                    ExtPanelStyle::Accent => 'A',
                    ExtPanelStyle::Normal => 'N',
                };
                flat_rows.push((
                    format!("{}{}{}", indent, icon_part, item.text),
                    format!("{}|{}", fg_marker, item.hint),
                    false,
                    is_sel,
                ));
                flat_idx += 1;
            }
        }
    }

    // Apply scroll
    let scroll = panel.scroll_top;
    let visible_rows = &flat_rows[scroll.min(flat_rows.len())..];

    for (ri, (text, hint_raw, is_header, is_sel)) in
        visible_rows.iter().enumerate().take(content_area_height)
    {
        let y = area.y + 1 + ri as u16;
        let bg = if *is_sel && panel.has_focus {
            sel_bg
        } else {
            row_bg
        };
        let fg = if *is_header {
            hdr_fg
        } else if hint_raw.starts_with('D') {
            dim_fg
        } else if hint_raw.starts_with('A') {
            accent_fg
        } else {
            item_fg
        };

        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, bg);
        }

        let w = area.width as usize;
        for (i, ch) in text.chars().enumerate().take(w) {
            set_cell(buf, area.x + i as u16, y, ch, fg, bg);
        }

        // Right-aligned hint (skip the style marker char and pipe)
        let hint = if hint_raw.len() > 2 {
            &hint_raw[2..]
        } else {
            ""
        };
        if !hint.is_empty() {
            let hint_len = hint.chars().count();
            let start = w.saturating_sub(hint_len + 1);
            for (i, ch) in hint.chars().enumerate() {
                let x = area.x + (start + i) as u16;
                if x < area.x + area.width {
                    set_cell(buf, x, y, ch, dim_fg, bg);
                }
            }
        }
    }

    // Scrollbar
    let total = flat_rows.len();
    if total > content_area_height && content_area_height > 0 {
        let sb_x = area.x + area.width - 1;
        let track_h = content_area_height;
        let thumb_h = (track_h * content_area_height / total).max(1);
        let thumb_top = scroll * track_h / total;
        for i in 0..track_h {
            let y = area.y + 1 + i as u16;
            let ch = if i >= thumb_top && i < thumb_top + thumb_h {
                '█'
            } else {
                '░'
            };
            set_cell(buf, sb_x, y, ch, dim_fg, row_bg);
        }
    }
}

// ─── Extensions sidebar panel ─────────────────────────────────────────────────

/// Render the Extensions sidebar panel.
fn render_ext_sidebar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let screen = render::build_screen_layout(engine, theme, &[], 1.0, 1.0, true);
    let Some(ref ext) = screen.ext_sidebar else {
        return;
    };

    let header_fg = rc(theme.status_fg);
    let header_bg = rc(theme.status_bg);
    let sec_bg = ratatui::style::Color::Rgb(
        (theme.status_bg.r as f64 * 0.85) as u8,
        (theme.status_bg.g as f64 * 0.85) as u8,
        (theme.status_bg.b as f64 * 0.85) as u8,
    );
    let default_fg = rc(theme.foreground);
    let dim_fg = rc(theme.line_number_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let panel_bg = rc(theme.completion_bg);

    // Helper: fill one row then write text chars
    let write_row =
        |buf: &mut ratatui::buffer::Buffer, y: u16, text: &str, fg: RColor, bg: RColor| {
            for x in area.x..area.x + area.width {
                set_cell(buf, x, y, ' ', fg, bg);
            }
            for (i, ch) in text.chars().enumerate().take(area.width as usize) {
                set_cell(buf, area.x + i as u16, y, ch, fg, bg);
            }
        };

    let mut y = area.y;

    // ── Row 0: header ────────────────────────────────────────────────────────
    if y < area.y + area.height {
        let hdr = if ext.fetching {
            " \u{eb85} EXTENSIONS  (fetching…)".to_string()
        } else {
            " \u{eb85} EXTENSIONS".to_string()
        };
        write_row(buf, y, &hdr, header_fg, header_bg);
        y += 1;
    }

    // ── Row 1: search box ─────────────────────────────────────────────────────
    if y < area.y + area.height {
        let search_bg = if ext.input_active { sel_bg } else { panel_bg };
        let search_fg = if ext.input_active || !ext.query.is_empty() {
            default_fg
        } else {
            dim_fg
        };
        let search_text = if ext.input_active {
            format!(" \u{f002} {}|", ext.query)
        } else if ext.query.is_empty() {
            " \u{f002} Search extensions (press /)".to_string()
        } else {
            format!(" \u{f002} {}", ext.query)
        };
        write_row(buf, y, &search_text, search_fg, search_bg);
        y += 1;
    }

    // ── INSTALLED section ─────────────────────────────────────────────────────
    let installed_count = ext.items_installed.len();
    if y < area.y + area.height {
        let arrow = if ext.sections_expanded[0] {
            '▼'
        } else {
            '▶'
        };
        let sec_hdr = format!("  {} INSTALLED ({})", arrow, installed_count);
        write_row(buf, y, &sec_hdr, dim_fg, sec_bg);
        y += 1;
    }

    if ext.sections_expanded[0] {
        for (idx, item) in ext.items_installed.iter().enumerate() {
            if y >= area.y + area.height {
                break;
            }
            let is_sel = ext.has_focus && ext.selected == idx;
            let (fg, bg) = if is_sel {
                (panel_bg, default_fg)
            } else {
                (default_fg, panel_bg)
            };
            let label = if item.update_available {
                format!("  ● {} \u{2191}", item.display_name) // ↑ update indicator
            } else {
                format!("  ● {}", item.display_name)
            };
            write_row(buf, y, &label, fg, bg);
            // Right-aligned hint
            let hint = if item.update_available {
                "[u] update"
            } else {
                "[d] remove"
            };
            let hint_start = area.x + area.width.saturating_sub(hint.len() as u16 + 1);
            for (i, ch) in hint.chars().enumerate() {
                let cx = hint_start + i as u16;
                if cx < area.x + area.width {
                    set_cell(buf, cx, y, ch, dim_fg, bg);
                }
            }
            y += 1;
        }
        if installed_count == 0 && y < area.y + area.height {
            write_row(buf, y, "    (none installed)", dim_fg, panel_bg);
            y += 1;
        }
    }

    // ── AVAILABLE section ─────────────────────────────────────────────────────
    let available_count = ext.items_available.len();
    if y < area.y + area.height {
        let arrow = if ext.sections_expanded[1] {
            '▼'
        } else {
            '▶'
        };
        let sec_hdr = format!("  {} AVAILABLE ({})", arrow, available_count);
        write_row(buf, y, &sec_hdr, dim_fg, sec_bg);
        y += 1;
    }

    if ext.sections_expanded[1] {
        for (idx, item) in ext.items_available.iter().enumerate() {
            if y >= area.y + area.height {
                break;
            }
            let flat_idx = installed_count + idx;
            let is_sel = ext.has_focus && ext.selected == flat_idx;
            let (fg, bg) = if is_sel {
                (panel_bg, default_fg)
            } else {
                (default_fg, panel_bg)
            };
            write_row(buf, y, &format!("  ○ {}", item.display_name), fg, bg);
            // Right-aligned hint
            let hint = "[i] install";
            let hint_start = area.x + area.width.saturating_sub(hint.len() as u16 + 1);
            for (i, ch) in hint.chars().enumerate() {
                let cx = hint_start + i as u16;
                if cx < area.x + area.width {
                    set_cell(buf, cx, y, ch, dim_fg, bg);
                }
            }
            y += 1;
        }
        if available_count == 0 && y < area.y + area.height {
            let msg = if ext.fetching {
                "    Fetching registry…"
            } else {
                "    (all installed)"
            };
            write_row(buf, y, msg, dim_fg, panel_bg);
            y += 1;
        }
    }

    // Fill remainder with panel_bg
    while y < area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', dim_fg, panel_bg);
        }
        y += 1;
    }

    let _ = sel_bg;
}

// ─── AI assistant sidebar panel ───────────────────────────────────────────────

/// Render the AI assistant sidebar panel.
fn render_ai_sidebar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let screen = render::build_screen_layout(engine, theme, &[], 1.0, 1.0, true);
    let Some(ref ai) = screen.ai_panel else {
        return;
    };

    let header_fg = rc(theme.status_fg);
    let header_bg = rc(theme.status_bg);
    let default_fg = rc(theme.foreground);
    let dim_fg = rc(theme.line_number_fg);
    let panel_bg = rc(theme.completion_bg);
    let user_fg = rc(theme.keyword);
    let asst_fg = rc(theme.string_lit);
    let input_bg = rc(theme.fuzzy_selected_bg);

    let write_row =
        |buf: &mut ratatui::buffer::Buffer, y: u16, text: &str, fg: RColor, bg: RColor| {
            for x in area.x..area.x + area.width {
                set_cell(buf, x, y, ' ', fg, bg);
            }
            for (i, ch) in text.chars().enumerate().take(area.width as usize) {
                set_cell(buf, area.x + i as u16, y, ch, fg, bg);
            }
        };

    let mut y = area.y;

    // ── Row 0: header ─────────────────────────────────────────────────────────
    if y < area.y + area.height {
        let hdr = if ai.streaming {
            " \u{f0e5} AI ASSISTANT  (thinking…)"
        } else {
            " \u{f0e5} AI ASSISTANT"
        };
        write_row(buf, y, hdr, header_fg, header_bg);
        y += 1;
    }

    // ── Compute input height (grows with content) ─────────────────────────────
    let pfx_len = 3usize; // " > " / "   "
    let content_w = (area.width as usize).saturating_sub(pfx_len).max(1);
    let input_chars: Vec<char> = ai.input.chars().collect();
    let input_line_count = {
        let raw = if input_chars.is_empty() {
            1
        } else {
            input_chars.len().div_ceil(content_w)
        };
        // cap so messages keep at least 3 rows
        raw.min((area.height as usize).saturating_sub(5).max(1))
    };
    // +1 for separator row
    let input_rows = input_line_count as u16 + 1;
    let msg_area_height = area.height.saturating_sub(1 + input_rows); // 1 = header

    // ── Message history ───────────────────────────────────────────────────────
    let scroll = ai.scroll_top;
    let wrap_w = content_w.saturating_sub(1).max(10); // slightly narrower for "  " indent
    let mut all_rows: Vec<(String, RColor)> = Vec::new();
    for msg in &ai.messages {
        let is_user = msg.role == "user";
        let role_label = if is_user { "You:" } else { "AI:" };
        let role_fg = if is_user { user_fg } else { asst_fg };
        all_rows.push((role_label.to_string(), role_fg));
        for line in msg.content.lines() {
            if line.is_empty() {
                all_rows.push(("  ".to_string(), default_fg));
                continue;
            }
            let chars: Vec<char> = line.chars().collect();
            let mut pos = 0;
            while pos < chars.len() {
                let end = (pos + wrap_w).min(chars.len());
                let chunk: String = chars[pos..end].iter().collect();
                all_rows.push((format!("  {}", chunk), default_fg));
                pos = end;
            }
        }
        all_rows.push((" ".to_string(), panel_bg)); // blank separator
    }

    let total = all_rows.len();
    let start = scroll.min(total.saturating_sub(msg_area_height as usize));
    for (i, (text, fg)) in all_rows.iter().enumerate().skip(start) {
        if y >= area.y + 1 + msg_area_height {
            break;
        }
        write_row(buf, y, text, *fg, panel_bg);
        y += 1;
        let _ = i;
    }

    // Fill remaining message area
    while y < area.y + 1 + msg_area_height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', dim_fg, panel_bg);
        }
        y += 1;
    }

    // ── Separator ─────────────────────────────────────────────────────────────
    if y < area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, '─', dim_fg, header_bg);
        }
        y += 1;
    }

    // ── Input area (multi-line, grows with content) ────────────────────────────
    let (inp_bg, inp_fg) = if ai.input_active {
        (input_bg, default_fg)
    } else {
        (panel_bg, dim_fg)
    };
    let cursor = ai.input_cursor.min(input_chars.len());
    let cursor_line = if content_w > 0 { cursor / content_w } else { 0 };
    let cursor_col = if content_w > 0 {
        cursor % content_w
    } else {
        cursor
    };

    if ai.input_active || !ai.input.is_empty() {
        // Split input into visual chunks
        let chunks: Vec<&[char]> = if input_chars.is_empty() {
            vec![&[][..]]
        } else {
            input_chars.chunks(content_w).collect()
        };
        for (line_idx, chunk) in chunks.iter().enumerate().take(input_line_count) {
            if y >= area.y + area.height {
                break;
            }
            // Fill background
            for x in area.x..area.x + area.width {
                set_cell(buf, x, y, ' ', inp_fg, inp_bg);
            }
            // Prefix: " > " on first line, "   " on continuations
            let pfx = if line_idx == 0 { " > " } else { "   " };
            for (i, ch) in pfx.chars().enumerate() {
                set_cell(buf, area.x + i as u16, y, ch, inp_fg, inp_bg);
            }
            // Content
            for (i, &ch) in chunk.iter().enumerate() {
                set_cell(
                    buf,
                    area.x + pfx_len as u16 + i as u16,
                    y,
                    ch,
                    inp_fg,
                    inp_bg,
                );
            }
            // Cursor (inverted cell on the cursor line)
            if ai.input_active && line_idx == cursor_line {
                let cx = area.x + pfx_len as u16 + cursor_col as u16;
                if cx < area.x + area.width {
                    let cursor_ch = input_chars.get(cursor).copied().unwrap_or(' ');
                    set_cell(buf, cx, y, cursor_ch, inp_bg, inp_fg);
                }
            }
            y += 1;
        }
    } else {
        // Placeholder when input is empty and not active
        if y < area.y + area.height {
            for x in area.x..area.x + area.width {
                set_cell(buf, x, y, ' ', inp_fg, inp_bg);
            }
            let placeholder = if ai.streaming {
                " (waiting for response…)"
            } else {
                " Press i to type…"
            };
            for (i, ch) in placeholder.chars().enumerate().take(area.width as usize) {
                set_cell(buf, area.x + i as u16, y, ch, inp_fg, inp_bg);
            }
        }
    }
}

// ─── Debug sidebar panel ──────────────────────────────────────────────────────

/// Render the debug sidebar: header + run button + 4 sections (Variables, Watch, Call Stack, Breakpoints).
fn render_debug_sidebar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    use render::DebugSidebarSection;
    if area.height == 0 {
        return;
    }
    let hdr_fg = rc(theme.status_fg);
    let hdr_bg = rc(theme.status_bg);
    let item_fg = rc(theme.line_number_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let act_fg = rc(theme.tab_active_fg);
    let row_bg = rc(theme.tab_bar_bg);

    // ── Row 0: header strip ──────────────────────────────────────────────────
    let cfg_name = engine
        .dap_launch_configs
        .get(engine.dap_selected_launch_config)
        .map(|c| c.name.as_str())
        .unwrap_or("no config");
    let header_text = format!("  \u{f188} DEBUG  |  {cfg_name}");
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', hdr_fg, hdr_bg);
    }
    for (i, ch) in header_text.chars().enumerate().take(area.width as usize) {
        set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
    }

    if area.height < 2 {
        return;
    }

    // ── Row 1: Run / Stop button ─────────────────────────────────────────────
    let btn_y = area.y + 1;
    let (btn_label, btn_fg) = if engine.dap_session_active && engine.dap_stopped_thread.is_some() {
        ("\u{f04b}  Continue", rc(Color::from_rgb(97, 186, 115)))
    } else if engine.dap_session_active {
        ("\u{f04d}  Stop", rc(Color::from_rgb(220, 70, 56)))
    } else {
        (
            "\u{f04b}  Start Debugging",
            rc(Color::from_rgb(97, 186, 115)),
        )
    };
    for x in area.x..area.x + area.width {
        set_cell(buf, x, btn_y, ' ', btn_fg, hdr_bg);
    }
    for (i, ch) in btn_label.chars().enumerate().take(area.width as usize) {
        set_cell(buf, area.x + i as u16, btn_y, ch, btn_fg, hdr_bg);
    }

    // ── Sections with fixed-height allocation + per-section scrolling ──────
    // Build minimal screen layout to get debug_sidebar data
    let screen = render::build_screen_layout(engine, theme, &[], 1.0, 1.0, true);
    let sidebar = &screen.debug_sidebar;

    let sections: [(
        &str,
        &[render::DebugSidebarItem],
        DebugSidebarSection,
        usize,
    ); 4] = [
        (
            "\u{f6a9} VARIABLES",
            &sidebar.variables,
            DebugSidebarSection::Variables,
            0,
        ),
        (
            "\u{f06e} WATCH",
            &sidebar.watch,
            DebugSidebarSection::Watch,
            1,
        ),
        (
            "\u{f020e} CALL STACK",
            &sidebar.frames,
            DebugSidebarSection::CallStack,
            2,
        ),
        (
            "\u{f111} BREAKPOINTS",
            &sidebar.breakpoints,
            DebugSidebarSection::Breakpoints,
            3,
        ),
    ];

    // Available rows after header(1) + button(1) = 2 overhead rows.
    // Each section has 1 header row, so 4 section headers = 4 rows.
    // Content rows = available - 4 section headers.
    let available = (area.height as usize).saturating_sub(2);
    let section_header_rows = 4;
    let content_rows = available.saturating_sub(section_header_rows);

    // Compute per-section content heights (equal share; remainder to first).
    let mut heights = [0u16; 4];
    if content_rows > 0 {
        let base = content_rows / 4;
        let remainder = content_rows % 4;
        for (i, h) in heights.iter_mut().enumerate() {
            *h = (base + if i < remainder { 1 } else { 0 }) as u16;
        }
    }
    // Store back into engine for ensure_visible calculations.
    // (We can't mutate engine directly here since it's borrowed, but the heights
    // are also stored on the sidebar data for reference.)

    let track_fg = rc(theme.separator);
    let thumb_fg = RColor::Rgb(128, 128, 128);
    let sb_bg = rc(theme.background);

    let mut row_y = area.y + 2;
    let max_y = area.y + area.height;

    for (section_label, items, section_kind, sec_idx) in &sections {
        if row_y >= max_y {
            break;
        }
        // Section header
        let is_active = sidebar.active_section == *section_kind;
        let sect_fg = if is_active { act_fg } else { hdr_fg };
        for x in area.x..area.x + area.width {
            set_cell(buf, x, row_y, ' ', sect_fg, hdr_bg);
        }
        for (i, ch) in section_label.chars().enumerate().take(area.width as usize) {
            set_cell(buf, area.x + i as u16, row_y, ch, sect_fg, hdr_bg);
        }
        row_y += 1;

        let sec_height = heights[*sec_idx] as usize;
        let scroll_off = sidebar.scroll_offsets[*sec_idx];
        let total_items = items.len().max(1); // at least 1 for "(empty)" hint

        // Render items within the allocated height
        for row_offset in 0..sec_height {
            if row_y >= max_y {
                break;
            }
            let item_idx = scroll_off + row_offset;
            if items.is_empty() && row_offset == 0 {
                // Empty hint
                let hint = if engine.dap_session_active {
                    "  (empty)"
                } else {
                    "  (not running)"
                };
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, row_y, ' ', item_fg, row_bg);
                }
                for (i, ch) in hint.chars().enumerate().take(area.width as usize) {
                    set_cell(buf, area.x + i as u16, row_y, ch, item_fg, row_bg);
                }
            } else if item_idx < items.len() {
                let item = &items[item_idx];
                let (fg, bg) = if item.is_selected {
                    (hdr_fg, sel_bg)
                } else {
                    (item_fg, row_bg)
                };
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, row_y, ' ', fg, bg);
                }
                let indent = item.indent as usize * 2;
                let text = format!("{:indent$}{}", "", item.text, indent = indent);
                // Leave rightmost column for scrollbar if needed
                let max_text_w = if items.len() > sec_height {
                    (area.width as usize).saturating_sub(1)
                } else {
                    area.width as usize
                };
                for (i, ch) in text.chars().enumerate().take(max_text_w) {
                    set_cell(buf, area.x + i as u16, row_y, ch, fg, bg);
                }
            } else {
                // Past end of items — blank row
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, row_y, ' ', item_fg, row_bg);
                }
            }
            row_y += 1;
        }

        // Draw scrollbar in the rightmost column if items exceed visible height
        if items.len() > sec_height && sec_height > 0 && area.width > 1 {
            let sb_x = area.x + area.width - 1;
            let sb_start_y = row_y - sec_height as u16;
            let thumb_size = ((sec_height * sec_height) / total_items).max(1);
            let thumb_pos = if total_items <= sec_height {
                0
            } else {
                (scroll_off * sec_height) / (total_items - sec_height)
            };
            let thumb_pos = thumb_pos.min(sec_height.saturating_sub(thumb_size));
            for r in 0..sec_height {
                let in_thumb = r >= thumb_pos && r < thumb_pos + thumb_size;
                let ch = if in_thumb { '█' } else { '░' };
                let fg = if in_thumb { thumb_fg } else { track_fg };
                let sy = sb_start_y + r as u16;
                if sy < max_y {
                    set_cell(buf, sb_x, sy, ch, fg, sb_bg);
                }
            }
        }
    }
}

/// Render the bottom panel tab bar (Terminal | Debug Output).
fn render_bottom_panel_tabs(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    active: render::BottomPanelKind,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let tab_bg = rc(theme.tab_bar_bg);
    let active_fg = rc(theme.tab_active_fg);
    let inactive_fg = rc(theme.tab_inactive_fg);

    // Fill background
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', inactive_fg, tab_bg);
    }

    let tabs = [
        ("  Terminal  ", render::BottomPanelKind::Terminal),
        ("  Debug Output  ", render::BottomPanelKind::DebugOutput),
    ];
    let mut cur_x = area.x;
    for (label, kind) in &tabs {
        let fg = if *kind == active {
            active_fg
        } else {
            inactive_fg
        };
        for (i, ch) in label.chars().enumerate() {
            let x = cur_x + i as u16;
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, area.y, ch, fg, tab_bg);
        }
        cur_x += label.len() as u16;
        if cur_x >= area.x + area.width {
            break;
        }
    }
}

/// Render the debug output tab content with a scrollbar.
/// `scroll` = 0 shows the newest lines (bottom); larger values scroll toward older lines.
#[allow(clippy::too_many_arguments)]
fn render_debug_output(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    output_lines: &[String],
    scroll: usize,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let hdr_fg = rc(theme.status_fg);
    let hdr_bg = rc(theme.status_bg);
    let item_fg = rc(theme.foreground);
    let row_bg = rc(theme.tab_bar_bg);
    let sb_active = RColor::Rgb(128, 128, 128);
    let sb_track = rc(theme.separator);

    // Header row
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', hdr_fg, hdr_bg);
    }
    let hdr_text = " DEBUG OUTPUT";
    for (i, ch) in hdr_text.chars().enumerate().take(area.width as usize) {
        set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
    }

    let content_rows = area.height.saturating_sub(1) as usize;
    let total = output_lines.len();
    let max_scroll = total.saturating_sub(content_rows);
    let scroll = scroll.min(max_scroll);
    let show_sb = total > content_rows;
    // Index of the first visible line (0 = oldest).
    // scroll=0 → show lines [max_scroll..total]; scroll=max_scroll → show [0..content_rows].
    let start_idx = max_scroll.saturating_sub(scroll);
    let text_width = if show_sb {
        area.width.saturating_sub(1) as usize
    } else {
        area.width as usize
    };
    let sb_x = area.x + area.width - 1;

    // Content rows
    for row in 0..content_rows {
        let ry = area.y + 1 + row as u16;
        if ry >= area.y + area.height {
            break;
        }
        for x in area.x..area.x + text_width as u16 {
            set_cell(buf, x, ry, ' ', item_fg, row_bg);
        }
        if let Some(line_text) = output_lines.get(start_idx + row) {
            let text = format!("  {line_text}");
            for (i, ch) in text.chars().enumerate().take(text_width) {
                set_cell(buf, area.x + i as u16, ry, ch, item_fg, row_bg);
            }
        }
    }

    // Scrollbar
    if show_sb {
        let thumb_size = (content_rows * content_rows)
            .div_ceil(total)
            .max(1)
            .min(content_rows);
        let available = content_rows.saturating_sub(thumb_size);
        // scroll=0 → thumb at bottom; scroll=max_scroll → thumb at top
        let thumb_top = if max_scroll > 0 {
            (available as f64 * (max_scroll - scroll) as f64 / max_scroll as f64).round() as usize
        } else {
            0
        };
        for i in 0..content_rows {
            let sy = area.y + 1 + i as u16;
            let ch = if i >= thumb_top && i < thumb_top + thumb_size {
                '█'
            } else {
                '░'
            };
            let fg = if i >= thumb_top && i < thumb_top + thumb_size {
                sb_active
            } else {
                sb_track
            };
            set_cell(buf, sb_x, sy, ch, fg, row_bg);
        }
    }
}

// ─── Quickfix panel ───────────────────────────────────────────────────────────

fn render_quickfix_panel(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    qf: &render::QuickfixPanel,
    scroll_top: usize,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let hdr_fg = rc(theme.status_fg);
    let hdr_bg = rc(theme.status_bg);
    let item_fg = rc(theme.fuzzy_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let row_bg = rc(theme.background);

    // Header row
    let focus_mark = if qf.has_focus { " [FOCUS]" } else { "" };
    let header = format!(" QUICKFIX ({} items){}", qf.total_items, focus_mark);
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', hdr_fg, hdr_bg);
    }
    for (i, ch) in header.chars().enumerate().take(area.width as usize) {
        set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
    }

    // Result rows
    let visible_rows = area.height.saturating_sub(1) as usize;
    for row_idx in 0..visible_rows {
        let item_idx = scroll_top + row_idx;
        let ry = area.y + 1 + row_idx as u16;
        if ry >= area.y + area.height {
            break;
        }
        let is_selected = item_idx == qf.selected_idx;
        let bg = if is_selected { sel_bg } else { row_bg };
        // Clear the row
        for x in area.x..area.x + area.width {
            set_cell(buf, x, ry, ' ', item_fg, bg);
        }
        if item_idx < qf.items.len() {
            let prefix = if is_selected { "▶ " } else { "  " };
            let text = format!("{}{}", prefix, qf.items[item_idx]);
            for (i, ch) in text.chars().enumerate().take(area.width as usize) {
                set_cell(buf, area.x + i as u16, ry, ch, item_fg, bg);
            }
        }
    }
}

// ─── Terminal panel ───────────────────────────────────────────────────────────

/// Nerd Font icons for the terminal toolbar.
const NF_TERMINAL_CLOSE: &str = "󰅖"; // nf-md-close_box
const NF_TERMINAL_SPLIT: &str = "󰤼"; // nf-md-view_split_vertical

fn render_terminal_panel(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    panel: &render::TerminalPanel,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let hdr_fg = RColor::Rgb(theme.status_fg.r, theme.status_fg.g, theme.status_fg.b);
    let hdr_bg = RColor::Rgb(theme.status_bg.r, theme.status_bg.g, theme.status_bg.b);

    // ── Toolbar row ──────────────────────────────────────────────────────────
    // Clear toolbar background
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', hdr_fg, hdr_bg);
    }

    if panel.find_active {
        // Find bar mode: show query and match count in toolbar
        let match_info = if panel.find_match_count == 0 {
            if panel.find_query.is_empty() {
                String::new()
            } else {
                " (no matches)".to_string()
            }
        } else {
            format!(
                " ({}/{})",
                panel.find_selected_idx + 1,
                panel.find_match_count
            )
        };
        let find_str = format!(" FIND: {}█{}", panel.find_query, match_info);
        let max_chars = area.width.saturating_sub(3) as usize;
        for (i, ch) in find_str.chars().enumerate().take(max_chars) {
            set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
        }
        // Close icon right-aligned
        for (i, ch) in NF_TERMINAL_CLOSE.chars().enumerate() {
            let x = area.x + area.width.saturating_sub(1 + i as u16);
            set_cell(buf, x, area.y, ch, hdr_fg, hdr_bg);
        }
    } else {
        // Tab strip — each tab is exactly 4 chars: "[N] "
        const TERMINAL_TAB_COLS: u16 = 4;
        let mut cursor_x = area.x;
        for i in 0..panel.tab_count {
            let label: Vec<char> = format!("[{}] ", i + 1).chars().collect();
            let (tab_fg, tab_bg) = if i == panel.active_tab {
                (hdr_bg, hdr_fg) // inverted for active tab
            } else {
                (hdr_fg, hdr_bg)
            };
            for (j, &ch) in label.iter().enumerate().take(TERMINAL_TAB_COLS as usize) {
                let x = cursor_x + j as u16;
                if x >= area.x + area.width {
                    break;
                }
                set_cell(buf, x, area.y, ch, tab_fg, tab_bg);
            }
            cursor_x += TERMINAL_TAB_COLS;
            if cursor_x >= area.x + area.width {
                break;
            }
        }

        // If no tabs yet, show minimal title
        if panel.tab_count == 0 {
            for (i, ch) in " TERMINAL".chars().enumerate().take(area.width as usize) {
                set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
            }
        }

        // Right-aligned icons: + ⊞ ×
        let icons = format!("+ {} {}", NF_TERMINAL_SPLIT, NF_TERMINAL_CLOSE);
        let icon_chars: Vec<char> = icons.chars().collect();
        let icon_start = area.width.saturating_sub(icon_chars.len() as u16 + 1);
        for (i, &ch) in icon_chars.iter().enumerate() {
            set_cell(
                buf,
                area.x + icon_start + i as u16,
                area.y,
                ch,
                hdr_fg,
                hdr_bg,
            );
        }
    }

    // ── Scrollbar geometry ────────────────────────────────────────────────────
    let content_rows = area.height.saturating_sub(1) as usize;
    let sb_col = area.x + area.width.saturating_sub(1);
    // Compute thumb range (row indices into the content area).
    let total = panel.scrollback_rows + content_rows;
    let (thumb_start, thumb_end) = if panel.scrollback_rows == 0 || area.width < 2 {
        (0, content_rows) // no scrollback → full bar
    } else {
        let thumb_h = ((content_rows * content_rows) / total).max(1);
        let max_off = panel.scrollback_rows;
        // scroll_offset=0 → thumb at bottom (live view); max_off → thumb at top.
        let max_top = content_rows.saturating_sub(thumb_h);
        let thumb_top = {
            let frac = 1.0 - (panel.scroll_offset as f64 / max_off as f64).min(1.0);
            (frac * max_top as f64) as usize
        };
        (thumb_top, (thumb_top + thumb_h).min(content_rows))
    };

    // ── Split view: left pane | divider | right pane ─────────────────────────
    if let Some(ref left_rows) = panel.split_left_rows {
        let half_w = panel.split_left_cols; // left-pane column count (may reflect drag state)
        let div_col = area.x + half_w;

        for row_idx in 0..content_rows {
            let screen_row = area.y + 1 + row_idx as u16;
            if screen_row >= area.y + area.height {
                break;
            }
            let term_bg = RColor::Rgb(30, 30, 30);

            // Clear both halves.
            for x in area.x..area.x + area.width.saturating_sub(1) {
                set_cell(buf, x, screen_row, ' ', hdr_fg, term_bg);
            }

            // Left pane cells.
            render_terminal_pane_cells(buf, left_rows, area.x, screen_row, half_w, row_idx);

            // Divider column.
            let div_fg = rc(theme.separator);
            set_cell(buf, div_col, screen_row, '│', div_fg, term_bg);

            // Right pane cells.
            render_terminal_pane_cells(buf, &panel.rows, div_col + 1, screen_row, half_w, row_idx);

            // Scrollbar in the last column.
            let (sb_char, sb_fg) = if row_idx >= thumb_start && row_idx < thumb_end {
                ('█', RColor::Rgb(128, 128, 128))
            } else {
                ('░', rc(theme.separator))
            };
            set_cell(
                buf,
                sb_col,
                screen_row,
                sb_char,
                sb_fg,
                rc(theme.background),
            );
        }

        return;
    }

    // ── Normal single-pane content rows ──────────────────────────────────────
    let cell_width = area.width.saturating_sub(1); // leave last col for scrollbar
    for row_idx in 0..content_rows {
        let screen_row = area.y + 1 + row_idx as u16;
        if screen_row >= area.y + area.height {
            break;
        }
        let term_bg_default = RColor::Rgb(30, 30, 30);
        // Clear row with terminal default background (excluding scrollbar col).
        for x in area.x..area.x + cell_width {
            set_cell(buf, x, screen_row, ' ', hdr_fg, term_bg_default);
        }

        render_terminal_pane_cells(buf, &panel.rows, area.x, screen_row, cell_width, row_idx);

        // Scrollbar column — same colors as the editor scrollbar.
        let (sb_char, sb_fg) = if row_idx >= thumb_start && row_idx < thumb_end {
            ('█', RColor::Rgb(128, 128, 128))
        } else {
            ('░', rc(theme.separator))
        };
        set_cell(
            buf,
            sb_col,
            screen_row,
            sb_char,
            sb_fg,
            rc(theme.background),
        );
    }
}

/// Render one row of terminal pane cells into a ratatui buffer.
fn render_terminal_pane_cells(
    buf: &mut ratatui::buffer::Buffer,
    rows: &[Vec<render::TerminalCell>],
    start_x: u16,
    screen_row: u16,
    max_cols: u16,
    row_idx: usize,
) {
    if row_idx >= rows.len() {
        return;
    }
    let row = &rows[row_idx];
    for (col_idx, cell) in row.iter().enumerate() {
        let x = start_x + col_idx as u16;
        if x >= start_x + max_cols {
            break;
        }
        let fg = RColor::Rgb(cell.fg.0, cell.fg.1, cell.fg.2);
        let bg = RColor::Rgb(cell.bg.0, cell.bg.1, cell.bg.2);
        let (draw_fg, draw_bg) = if cell.is_cursor || cell.selected {
            (bg, fg)
        } else if cell.is_find_active {
            (RColor::Rgb(0, 0, 0), RColor::Rgb(255, 165, 0))
        } else if cell.is_find_match {
            (RColor::Rgb(255, 220, 0), RColor::Rgb(80, 65, 0))
        } else {
            (fg, bg)
        };
        let ch = if cell.ch == '\0' { ' ' } else { cell.ch };
        let mut modifier = Modifier::empty();
        if cell.bold {
            modifier |= Modifier::BOLD;
        }
        if cell.italic {
            modifier |= Modifier::ITALIC;
        }
        if cell.underline {
            modifier |= Modifier::UNDERLINED;
        }
        if modifier.is_empty() {
            set_cell(buf, x, screen_row, ch, draw_fg, draw_bg);
        } else {
            set_cell_styled(buf, x, screen_row, ch, draw_fg, draw_bg, modifier);
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Convert a `render::Color` to a ratatui `Color::Rgb`.
#[inline]
fn rc(c: Color) -> RColor {
    RColor::Rgb(c.r, c.g, c.b)
}

/// Return the character index that corresponds to a byte offset in a UTF-8
/// string. Returns the total char count if `byte_offset` is past the end.
fn byte_to_char_idx(text: &str, byte_offset: usize) -> usize {
    let clamped = byte_offset.min(text.len());
    // Walk back from clamped to find a char boundary (avoids unstable
    // `floor_char_boundary` which older Rust toolchains lack).
    let mut safe = clamped;
    while safe > 0 && !text.is_char_boundary(safe) {
        safe -= 1;
    }
    text[..safe].chars().count()
}
