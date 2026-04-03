//! TUI (terminal UI) entry point for VimCode.
//!
//! Activated with the `--tui` CLI flag. Uses ratatui + crossterm to render
//! the same `ScreenLayout` produced by `render::build_screen_layout` that the
//! GTK backend consumes — just rendered to a terminal instead of a Cairo
//! surface.
//!
//! **No GTK/Cairo/Pango imports here.** All editor logic comes from `core`.
//! All rendering data comes from `render`.
#![allow(unused_assignments)]

use std::collections::HashSet;
use std::fs;
use std::io::{self, Stdout, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

mod mouse;
mod panels;
mod render_impl;

#[allow(unused_imports)]
use mouse::*;
#[allow(unused_imports)]
use panels::*;
#[allow(unused_imports)]
use render_impl::*;

// ─── Debug logging ────────────────────────────────────────────────────────────

/// Global debug log file handle, set once at startup via `--debug <path>`.
static DEBUG_LOG: std::sync::OnceLock<Mutex<std::fs::File>> = std::sync::OnceLock::new();

/// Initialise the debug log.  Call once before the event loop starts.
fn init_debug_log(path: &str) {
    match std::fs::File::create(path) {
        Ok(f) => {
            let _ = DEBUG_LOG.set(Mutex::new(f));
            // Also enable LSP debug logging (read by the reader thread in lsp.rs).
            std::env::set_var("VIMCODE_LSP_DEBUG", "1");
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
use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color as RColor, Modifier};
use ratatui::Terminal;

use crate::core::engine::{DiffLine, EngineAction};
use crate::core::lsp::DiagnosticSeverity;
use crate::core::settings::ExplorerAction;
use crate::core::window::{GroupId, SplitDirection};
use crate::core::{Engine, GitLineStatus, Mode, OpenMode, WindowRect};
use crate::icons;
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

/// Sync the engine's `explorer_has_focus` and `search_has_focus` fields from the
/// TUI-local sidebar state.  Called after any key/mouse event that may change focus.
fn sync_sidebar_focus(sidebar: &TuiSidebar, engine: &mut Engine) {
    let in_fixed_panel = sidebar.has_focus && sidebar.ext_panel_name.is_none();
    engine.explorer_has_focus = in_fixed_panel && sidebar.active_panel == TuiPanel::Explorer;
    engine.search_has_focus = in_fixed_panel && sidebar.active_panel == TuiPanel::Search;
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

/// Check if a binary exists on PATH.
fn has_binary(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Find the first available clipboard write command (program + args).
fn find_clipboard_write_cmd() -> Option<(&'static str, &'static [&'static str])> {
    let candidates: &[(&str, &[&str])] = &[
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
        ("wl-copy", &[]),
        #[cfg(target_os = "macos")]
        ("pbcopy", &[]),
    ];
    for &(prog, args) in candidates {
        if has_binary(prog) {
            return Some((prog, args));
        }
    }
    None
}

/// Find the first available clipboard read command (program + args).
fn find_clipboard_read_cmd() -> Option<(&'static str, &'static [&'static str])> {
    let candidates: &[(&str, &[&str])] = &[
        ("xclip", &["-selection", "clipboard", "-o"]),
        ("xsel", &["--clipboard", "--output"]),
        ("wl-paste", &[]),
        #[cfg(target_os = "macos")]
        ("pbpaste", &[]),
    ];
    for &(prog, args) in candidates {
        if has_binary(prog) {
            return Some((prog, args));
        }
    }
    None
}

/// Set up system clipboard callbacks on the engine.
///
/// Spawns xclip/xsel/wl-copy/wl-paste/pbcopy/pbpaste directly rather than
/// using copypasta_ext, which has a bug where it doesn't close the child's
/// stdin pipe before calling wait() — causing xclip to exit with status 1
/// under crossterm raw mode.
fn setup_tui_clipboard(engine: &mut Engine) {
    // Ensure DISPLAY is set for xclip/xsel — TUI sessions (e.g. tmux, SSH)
    // may not inherit it even when an X server is running on :0.
    if std::env::var("DISPLAY").unwrap_or_default().is_empty() {
        unsafe { std::env::set_var("DISPLAY", ":0") };
    }
    if let Some((prog, args)) = find_clipboard_read_cmd() {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        engine.clipboard_read = Some(Box::new(move || {
            let _guard = suppress_stderr();
            let output = std::process::Command::new(prog)
                .args(&args)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .output()
                .map_err(|e| format!("clipboard read: {e}"))?;
            if !output.status.success() {
                return Err(format!("{} exited with status {}", prog, output.status));
            }
            String::from_utf8(output.stdout).map_err(|e| format!("clipboard: {e}"))
        }));
    }

    if let Some((prog, args)) = find_clipboard_write_cmd() {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        engine.clipboard_write = Some(Box::new(move |text: &str| {
            let _guard = suppress_stderr();
            let mut child = std::process::Command::new(prog)
                .args(&args)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| format!("clipboard write: {e}"))?;
            // Write text then DROP stdin to send EOF — critical for xclip.
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(text.as_bytes());
                // stdin dropped here, pipe closed, xclip sees EOF
            }
            let status = child.wait().map_err(|e| format!("clipboard: {e}"))?;
            if !status.success() {
                return Err(format!("{} exited with status {}", prog, status));
            }
            Ok(())
        }));
    }

    if engine.clipboard_write.is_none() && engine.clipboard_read.is_none() {
        engine.message = "Clipboard unavailable — install xclip or xsel".to_string();
    }
}

/// Copy text to the system clipboard and show a status message.
fn tui_copy_to_clipboard(text: &str, engine: &mut Engine) {
    if let Some(ref cb) = engine.clipboard_write {
        if cb(text).is_ok() {
            engine.message = format!("Copied: {}", text);
            return;
        }
    }
    engine.message = format!("Link: {} (clipboard unavailable)", text);
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
    icons::set_nerd_fonts(engine.settings.use_nerd_fonts);
    engine.plugin_init();
    if let Some(path) = file_path {
        // CLI argument: open only the specified file/directory, skip session restore
        if path.is_dir() {
            debug_log!("Opening directory from CLI: {:?}", path);
            engine.open_folder(&path);
        } else {
            // Load file into the initial window (reuses the scratch buffer's tab).
            debug_log!("Opening file from CLI: {:?}", path);
            let _ = engine.open_file_with_mode(&path, crate::core::engine::OpenMode::Permanent);
        }
    } else {
        engine.restore_session_files();
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
            // Emergency: flush swap files for all dirty buffers before anything else.
            crate::core::swap::run_emergency_flush();

            if let Some(path) = crate::core::swap::write_crash_log(info) {
                // Also mirror to the debug log when --debug is active.
                debug_log!("Crash log written to {}", path.display());
            }
            prev_hook(info);
        }));
    }

    // Register engine pointer for emergency swap flush from the panic hook.
    // SAFETY: `engine` lives on the stack until process exit; the pointer is
    // only dereferenced during panic recovery on the same thread.
    unsafe {
        crate::core::swap::register_emergency_engine(&engine as *const _);
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.clear().expect("clear terminal");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        event_loop(&mut terminal, &mut engine, keyboard_enhanced);
    }));

    restore_terminal(&mut terminal, keyboard_enhanced);

    if let Err(e) = result {
        // Emergency: flush swap files for all dirty buffers before exiting.
        // This preserves unsaved work that would otherwise be lost.
        engine.emergency_swap_flush();

        // Extract the panic message before aborting — resume_unwind would call
        // abort() on Linux (via the default panic handler), producing a core dump.
        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            format!("VimCode internal error: {s}")
        } else if let Some(s) = e.downcast_ref::<String>() {
            format!("VimCode internal error: {s}")
        } else {
            "VimCode internal error (unknown panic payload)".to_string()
        };
        let crash_path = crate::core::swap::crash_log_path();
        eprintln!("{msg}");
        eprintln!("Unsaved buffers written to swap files for recovery.");
        eprintln!("Crash details written to {}", crash_path.display());
        eprintln!("Please report this at https://github.com/JDonaghy/vimcode/issues");
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

    // Mutable sidebar width (default SIDEBAR_WIDTH, clamped 15..60)
    let mut sidebar_width: u16 = SIDEBAR_WIDTH;
    // Folder picker modal state (None = closed)
    let mut folder_picker: Option<FolderPickerState> = None;
    // Scroll offset for the quickfix panel (fuzzy/grep scroll is handled by unified picker)
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
    // Non-None while user is dragging a sidebar scrollbar that has no dedicated drag state.
    // Used for explorer and ext panel scrollbars to prevent text selection leaking.
    let mut dragging_generic_sb: Option<SidebarScrollDrag> = None;
    // True while user drags the terminal header row to resize the panel.
    let mut dragging_terminal_resize: bool = false;
    // True while user drags the terminal split divider left/right.
    let mut dragging_terminal_split: bool = false;
    // Non-None while user is dragging a group divider (stores split_index).
    let mut dragging_group_divider: Option<usize> = None;
    // True while user is drag-selecting text inside the editor hover popup.
    let mut hover_selecting: bool = false;
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
    // Tab drag-and-drop: position where mouse-down occurred on a tab (potential drag start).
    let mut tab_drag_start: Option<(u16, u16)> = None;
    // True while a tab drag is actively in progress.
    let mut tab_dragging: bool = false;

    // Track unnamed register content so we only write to clipboard on changes.
    let mut last_clipboard_content: Option<String> = None;
    // True when the quit-confirm overlay is shown (unsaved changes on exit).
    let mut quit_confirm = false;
    // True when the close-tab-confirm overlay is shown (unsaved changes on tab close).
    let mut close_tab_confirm = false;

    let mut needs_redraw = true;
    // Track whether a large overlay popup was visible last frame so we can
    // force a full redraw when it disappears (prevents stale characters from
    // the popup lingering due to ratatui's incremental diff).
    let mut had_popup_overlay = false;
    // Link hit rects from the hover popup render: (x, y, w, h, url).
    let mut hover_link_rects: Vec<(u16, u16, u16, u16, String)> = Vec::new();
    // Bounding rect of the panel hover popup (x, y, w, h) — used to suppress dismiss on mouse-over.
    let mut hover_popup_rect: Option<(u16, u16, u16, u16)> = None;
    // Bounding rect of the editor hover popup (x, y, w, h) — for scroll wheel + click + dismiss.
    let mut editor_hover_popup_rect: Option<(u16, u16, u16, u16)> = None;
    // Link hit rects from the editor hover popup: (x, y, w, h, url).
    let mut editor_hover_link_rects: Vec<(u16, u16, u16, u16, String)> = Vec::new();
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

    // Reveal the active file in the explorer sidebar at startup (session restore).
    if let Some(path) = engine.file_path().cloned() {
        let h = terminal
            .size()
            .map(|s| s.height.saturating_sub(4) as usize)
            .unwrap_or(40);
        sidebar.reveal_path(&path, h);
    }

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
            // Compute how many rows the tab bar + breadcrumbs consume.
            let tab_bar_rows: u16 = {
                let has_single_tab = engine.active_group().tabs.len() <= 1;
                if engine.settings.hide_single_tab && has_single_tab {
                    if engine.settings.breadcrumbs {
                        1
                    } else {
                        0
                    }
                } else if engine.settings.breadcrumbs {
                    2
                } else {
                    1
                }
            };
            engine.set_viewport_lines(content_rows.saturating_sub(tab_bar_rows).max(1) as usize);
            engine.set_viewport_cols(content_cols.max(1) as usize);
        }

        if needs_redraw && last_draw.elapsed() >= min_frame {
            // Keep engine focus flags in sync with TUI sidebar state before rendering.
            sync_sidebar_focus(&sidebar, engine);
            let redraw_t0 = std::time::Instant::now();
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

            // Detect when a large overlay popup (picker, folder picker, dialog)
            // was visible last frame but isn't now.  Force a full redraw so
            // ratatui's incremental diff doesn't leave stale popup characters
            // in the editor area.
            let has_popup = screen
                .map(|s| s.picker.is_some())
                .unwrap_or(false)
                || folder_picker.is_some();
            if had_popup_overlay && !has_popup {
                terminal.clear().ok();
            }
            had_popup_overlay = has_popup;

            let mut tab_visible_counts: Vec<(crate::core::window::GroupId, usize)> = Vec::new();
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
                            sidebar_width,
                            quickfix_scroll_top,
                            debug_output_scroll,
                            folder_picker.as_ref(),
                            quit_confirm,
                            close_tab_confirm,
                            cmd_sel,
                            drop_target,
                            &mut hover_link_rects,
                            &mut hover_popup_rect,
                            &mut editor_hover_popup_rect,
                            &mut editor_hover_link_rects,
                            &mut tab_visible_counts,
                        );
                    }
                })
                .expect("draw frame");
            // Report available tab bar width (in columns) back to the engine
            // so that ensure_active_tab_visible() can compute how many tabs fit.
            for (gid, width_cols) in &tab_visible_counts {
                engine.set_tab_visible_count(*gid, *width_cols);
            }
            // After updating counts (e.g. after a terminal resize), re-check
            // that every group's active tab is still visible.
            engine.ensure_all_groups_tabs_visible();

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

            let redraw_ms = redraw_t0.elapsed();
            if redraw_ms.as_millis() > 16 {
                debug_log!("PERF redraw: {:.1}ms", redraw_ms.as_secs_f64() * 1000.0);
            }
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
            // Flush debounced cursor_move hook (plugin events + code action requests).
            if engine.flush_cursor_move_hook() {
                needs_redraw = true;
            }
            let idle_t0 = std::time::Instant::now();
            // Flush LSP didChange (may block briefly on pipe write for large buffers).
            engine.lsp_flush_changes();
            let lsp_flush_ms = idle_t0.elapsed().as_secs_f64() * 1000.0;
            let poll_t0 = std::time::Instant::now();
            if engine.poll_lsp() {
                needs_redraw = true;
            }
            let lsp_poll_ms = poll_t0.elapsed().as_secs_f64() * 1000.0;
            if lsp_flush_ms > 5.0 || lsp_poll_ms > 5.0 {
                debug_log!(
                    "PERF idle: lsp_flush={:.1}ms lsp_poll={:.1}ms",
                    lsp_flush_ms,
                    lsp_poll_ms
                );
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
                if sidebar.active_panel == TuiPanel::Git
                    || sidebar.active_panel == TuiPanel::Explorer
                {
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
            // Poll for completed SC diff background request.
            if engine.poll_sc_diff() {
                needs_redraw = true;
            }
            if engine.poll_ai() {
                needs_redraw = true;
            }
            // Poll for completed async shell tasks (plugin background commands).
            if engine.poll_async_shells() {
                needs_redraw = true;
            }
            // Check for panel reveal request from plugins.
            if let Some(panel_name) = engine.ext_panel_focus_pending.take() {
                sidebar.ext_panel_name = Some(panel_name);
                sidebar.visible = true;
                sidebar.has_focus = true;
                needs_redraw = true;
            }
            // Poll panel hover dwell timer (shows popup after brief mouse hover).
            if engine.poll_panel_hover() {
                needs_redraw = true;
            }
            // Poll editor hover dwell / delayed dismiss timers.
            if engine.poll_editor_hover() {
                needs_redraw = true;
            }
            // Poll async blame results.
            if engine.poll_blame() {
                needs_redraw = true;
            }
            // Tick AI inline completion debounce counter each event-loop frame.
            if engine.tick_ai_completion() {
                needs_redraw = true;
            }
            // Debounced syntax refresh during insert mode — after 150ms of no
            // keystrokes, re-parse + re-extract highlights so stale byte offsets
            // don't cause wrong colors near edited regions.
            if engine.tick_syntax_debounce() {
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

                // ── Modal dialog intercepts ALL keys ──────────────────────
                if engine.dialog.is_some() {
                    if let Some((key_name, unicode, ctrl)) =
                        translate_key(key_event, keyboard_enhanced)
                    {
                        let action = engine.handle_key(&key_name, unicode, ctrl);
                        if action == EngineAction::Quit {
                            return;
                        }
                    } else if key_event.kind != KeyEventKind::Release {
                        // translate_key doesn't map Tab — handle it directly.
                        match key_event.code {
                            KeyCode::Tab => {
                                engine.handle_key("Tab", None, false);
                            }
                            KeyCode::BackTab => {
                                engine.handle_key("Shift_Tab", None, false);
                            }
                            _ => {}
                        }
                    }
                    needs_redraw = true;
                    continue;
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

                // ── Inline new file/folder in explorer ───────────────────────
                if engine.explorer_new_entry.is_some() {
                    if let Some((key_name, unicode, ctrl)) =
                        translate_key(key_event, keyboard_enhanced)
                    {
                        engine.handle_explorer_new_entry_key(&key_name, unicode, ctrl);
                        if engine.explorer_needs_refresh {
                            sidebar.build_rows();
                            engine.explorer_needs_refresh = false;
                        }
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
                                    if let Some(fp) = engine.file_path().cloned() {
                                        let h = terminal
                                            .size()
                                            .map(|s| s.height.saturating_sub(4) as usize)
                                            .unwrap_or(40);
                                        sidebar.reveal_path(&fp, h);
                                    }
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
                                    // Reveal the active file from the restored session
                                    if let Some(path) = engine.file_path().cloned() {
                                        let h = terminal
                                            .size()
                                            .map(|s| s.height.saturating_sub(4) as usize)
                                            .unwrap_or(40);
                                        sidebar.reveal_path(&path, h);
                                    }
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
                    && !engine.picker_open
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
                            if panel == TuiPanel::Explorer {
                                engine.explorer_has_focus = true;
                            }
                            if panel == TuiPanel::Search {
                                engine.search_has_focus = true;
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
                // Note: sidebar key handling is suppressed when a picker modal is
                // open and when terminal has focus (e.g. "Press Enter to close..."
                // after extension install).
                if sidebar.has_focus
                    && !engine.picker_open
                    && !engine.terminal_has_focus
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
                                engine.clear_sidebar_focus();
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
                                engine.clear_sidebar_focus();
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
                                KeyCode::Char('v')
                                    if key_event.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    if let Some(text) = Engine::clipboard_paste() {
                                        let line = text.lines().next().unwrap_or("");
                                        for c in line.chars() {
                                            if !c.is_control() {
                                                if sidebar.replace_input_focused {
                                                    engine.project_replace_text.push(c);
                                                } else {
                                                    engine.project_search_query.push(c);
                                                }
                                            }
                                        }
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
                        // When the input field is active, pass characters as
                        // input text instead of navigation commands.
                        if engine.ext_panel_input_active {
                            let (ikey, ich): (&str, Option<char>) = match key_event.code {
                                KeyCode::Esc => ("Escape", None),
                                KeyCode::Enter => ("Return", None),
                                KeyCode::Backspace => ("BackSpace", None),
                                KeyCode::Char(ch) => ("char", Some(ch)),
                                _ => ("", None),
                            };
                            if !ikey.is_empty() {
                                let name = if ikey == "char" {
                                    ich.map(|c| c.to_string()).unwrap_or_default()
                                } else {
                                    ikey.to_string()
                                };
                                engine.handle_ext_panel_input_key(&name, ctrl, ich);
                            }
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
                                KeyCode::Delete => ("Delete", None),
                                KeyCode::Up => ("Up", None),
                                KeyCode::Down => ("Down", None),
                                KeyCode::Left => ("Left", None),
                                KeyCode::Right => ("Right", None),
                                KeyCode::Home => ("Home", None),
                                KeyCode::End => ("End", None),
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
                                if name == "Return" {
                                    // Open tab immediately, diff arrives
                                    // asynchronously via poll_sc_diff.
                                    let done = engine.sc_open_selected_async();
                                    if done && !engine.sc_has_focus {
                                        sidebar.has_focus = false;
                                    }
                                } else {
                                    engine.handle_sc_key(name, ctrl, None);
                                    if !engine.sc_has_focus {
                                        sidebar.has_focus = false;
                                    }
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
                                        // Expand the target dir so the new entry row is visible
                                        sidebar.expanded.insert(target_dir.clone());
                                        sidebar.build_rows();
                                        if action == ExplorerAction::NewFile {
                                            engine.start_explorer_new_file(target_dir);
                                        } else {
                                            engine.start_explorer_new_folder(target_dir);
                                        }
                                    }
                                    ExplorerAction::Delete => {
                                        let idx = sidebar.selected;
                                        if idx < sidebar.rows.len() {
                                            let path = sidebar.rows[idx].path.clone();
                                            engine.confirm_delete_file(&path);
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
                                            let root = sidebar.root.clone();
                                            engine.start_move_file_dialog(&path, &root);
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

                        // Ctrl+L: force full screen redraw (clears rendering artifacts)
                        if ctrl && matches!(code, KeyCode::Char('l') | KeyCode::Char('L')) {
                            terminal.clear().ok();
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
                                // Poll PTY output immediately so held keys (e.g.
                                // backspace) show feedback each frame instead of
                                // batching until the key is released.
                                engine.poll_terminal();
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
                            sync_sidebar_focus(&sidebar, engine);
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
                            sync_sidebar_focus(&sidebar, engine);
                            needs_redraw = true;
                            continue;
                        }

                        if matches_tui_key(&pk.fuzzy_finder, code, mods) {
                            engine.open_picker(crate::core::engine::PickerSource::Files);
                            needs_redraw = true;
                            continue;
                        }

                        if matches_tui_key(&pk.live_grep, code, mods) {
                            engine.open_picker(crate::core::engine::PickerSource::Grep);
                            needs_redraw = true;
                            continue;
                        }

                        if matches_tui_key(&pk.command_palette, code, mods) {
                            engine.open_picker(crate::core::engine::PickerSource::Commands);
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
                        if matches_tui_key(&pk.nav_back, code, mods) {
                            engine.tab_nav_back();
                            needs_redraw = true;
                            continue;
                        }
                        if matches_tui_key(&pk.nav_forward, code, mods) {
                            engine.tab_nav_forward();
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
                                                engine.open_picker(
                                                    crate::core::engine::PickerSource::Files,
                                                );
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
                        if ctrl && matches!(unicode, Some('c') | Some('C')) && cmd_sel.is_some() {
                            debug_log!(
                                "CMD_SEL Ctrl+C: cmd_sel={:?} msg_len={}",
                                cmd_sel,
                                engine.message.len()
                            );
                            if let Some((start, end)) = cmd_sel {
                                let lo = start.min(end);
                                let hi = start.max(end);
                                // Determine the source text for the selection.
                                let source = if matches!(engine.mode, Mode::Command | Mode::Search)
                                {
                                    // col 0 = ':' prefix, col 1+ = buffer chars
                                    let buf_lo = lo.saturating_sub(1);
                                    let buf_hi = hi.saturating_sub(1);
                                    engine
                                        .command_buffer
                                        .chars()
                                        .enumerate()
                                        .filter(|(i, _)| *i >= buf_lo && *i <= buf_hi)
                                        .map(|(_, c)| c)
                                        .collect::<String>()
                                } else {
                                    // Normal mode message line — no prefix offset
                                    engine
                                        .message
                                        .chars()
                                        .enumerate()
                                        .filter(|(i, _)| *i >= lo && *i <= hi)
                                        .map(|(_, c)| c)
                                        .collect::<String>()
                                };
                                if !source.is_empty() {
                                    tui_copy_to_clipboard(&source, engine);
                                }
                            }
                            cmd_sel = None;
                            needs_redraw = true;
                            continue;
                        }
                        if matches!(engine.mode, Mode::Command | Mode::Search) {
                            // Any other key clears the selection
                            cmd_sel = None;
                        } else if cmd_sel.is_some() {
                            // In normal mode, any non-Ctrl-C key clears message selection
                            cmd_sel = None;
                        }
                    }

                    // clipboard=unnamedplus: intercept p/P to read from system clipboard.
                    // TUI translate_key() sets key_name="" for regular chars; check unicode.
                    let paste_intercepted = !ctrl
                        && matches!(unicode, Some('p') | Some('P'))
                        && intercept_paste_key(engine, unicode == Some('P'));

                    // ── Context menu keyboard intercept (TUI-side) ──────────
                    // Handle here so explorer actions (new_file etc.) can be
                    // dispatched to the engine's dialog system.
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
                        let _key_t0 = std::time::Instant::now();
                        let action = engine.handle_key(&key_name, unicode, ctrl);
                        let key_elapsed = _key_t0.elapsed();
                        // After any key in insert mode, reset AI completion timer.
                        if engine.mode == crate::core::Mode::Insert
                            && engine.settings.ai_completions
                        {
                            engine.ai_completion_reset_timer();
                        }
                        debug_log!(
                            "handle_key result: action={:?} groups_after={} elapsed={:.1}ms",
                            action,
                            engine.group_layout.leaf_count(),
                            key_elapsed.as_secs_f64() * 1000.0,
                        );
                        if let Some(perf) = engine.perf_log.take() {
                            debug_log!("  {}", perf);
                        }
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
                                    TuiPanel::Explorer => {
                                        engine.explorer_has_focus = true;
                                    }
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
                                &mut dragging_generic_sb,
                                last_layout.as_ref(),
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
                                &mut tab_drag_start,
                                &mut tab_dragging,
                                &hover_link_rects,
                                hover_popup_rect,
                                editor_hover_popup_rect,
                                &editor_hover_link_rects,
                                &mut hover_selecting,
                            );
                            sync_sidebar_focus(&sidebar, engine);
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
                let hover_before = engine.sc_button_hovered;
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
                    &mut dragging_generic_sb,
                    last_layout.as_ref(),
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
                    &mut tab_drag_start,
                    &mut tab_dragging,
                    &hover_link_rects,
                    hover_popup_rect,
                    editor_hover_popup_rect,
                    &editor_hover_link_rects,
                    &mut hover_selecting,
                );
                sync_sidebar_focus(&sidebar, engine);
                if mouse_should_quit {
                    return;
                }
                if engine.sc_button_hovered != hover_before {
                    needs_redraw = true;
                }
                // Poll editor hover dwell after mouse events so the timer
                // can fire even when continuous mouse events prevent idle polling.
                if engine.poll_editor_hover() {
                    needs_redraw = true;
                }
                if engine.poll_blame() {
                    needs_redraw = true;
                }
            }
            Event::Paste(text) => {
                // Bracketed paste — text delivered directly from the terminal emulator
                // (e.g. Ctrl+V in Windows Terminal / WSL).  Check all active input
                // contexts before falling through to the mode-based dispatch, because
                // many input fields (picker, sidebar search, SC commit, etc.) keep the
                // engine in Normal mode while the TUI routes keys locally.
                let first_line = text.lines().next().unwrap_or("");

                // Terminal PTY — forward raw bytes.
                if engine.terminal_has_focus {
                    if engine.terminal_find_active {
                        // Paste into the terminal find bar, not the PTY.
                        for ch in first_line.chars() {
                            if !ch.is_control() {
                                engine.terminal_find_char(ch);
                            }
                        }
                    } else {
                        engine.terminal_write(text.as_bytes());
                    }
                    needs_redraw = true;
                    continue;
                }

                // Unified picker (fuzzy finder / live grep / command palette).
                if engine.picker_open {
                    for c in first_line.chars() {
                        if !c.is_control() {
                            engine.picker_query.push(c);
                        }
                    }
                    engine.picker_selected = 0;
                    engine.picker_scroll_top = 0;
                    engine.picker_filter();
                    engine.picker_load_preview();
                    needs_redraw = true;
                    continue;
                }

                // Sidebar search / replace input.
                if sidebar.has_focus
                    && sidebar.active_panel == TuiPanel::Search
                    && sidebar.search_input_mode
                {
                    for c in first_line.chars() {
                        if !c.is_control() {
                            if sidebar.replace_input_focused {
                                engine.project_replace_text.push(c);
                            } else {
                                engine.project_search_query.push(c);
                            }
                        }
                    }
                    needs_redraw = true;
                    continue;
                }

                // Source control commit message input.
                if engine.sc_commit_input_active {
                    for c in first_line.chars() {
                        if !c.is_control() {
                            engine.sc_commit_message.push(c);
                        }
                    }
                    needs_redraw = true;
                    continue;
                }

                // Extension sidebar search input.
                if engine.ext_sidebar_input_active {
                    for c in first_line.chars() {
                        if !c.is_control() {
                            engine.ext_sidebar_query.push(c);
                        }
                    }
                    needs_redraw = true;
                    continue;
                }

                // AI chat input.
                if engine.ai_has_focus && engine.ai_input_active {
                    for c in first_line.chars() {
                        if !c.is_control() {
                            engine.ai_input.push(c);
                        }
                    }
                    needs_redraw = true;
                    continue;
                }

                // Standard mode-based dispatch (editor-level paste).
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
                // Force ratatui to do a full redraw.  Terminal emulators reflow
                // screen content on resize, which can leave the physical display
                // out of sync with ratatui's previous-frame buffer.  Clearing
                // resets both buffers so the next draw emits every cell.
                terminal.clear().ok();
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
    terminal_size: Option<Size>,
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
                path.clone()
            } else {
                path.parent().unwrap_or(&sidebar.root).to_path_buf()
            };
            if action == "new_file" {
                engine.start_explorer_new_file(target);
            } else {
                engine.start_explorer_new_folder(target);
            }
        }
        "rename" => {
            engine.start_explorer_rename(path);
        }
        "delete" => {
            engine.confirm_delete_file(&path);
        }
        // copy_path, copy_relative_path, reveal, open_side, open_side_vsplit handled by engine
        "copy_path" | "copy_relative_path" | "reveal" | "open_side" | "open_side_vsplit" => {}
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
            engine.open_picker(crate::core::engine::PickerSource::Grep);
        }
        _ => {}
    }
}

fn set_cell(buf: &mut ratatui::buffer::Buffer, x: u16, y: u16, ch: char, fg: RColor, bg: RColor) {
    let area = buf.area;
    if x < area.x + area.width && y < area.y + area.height {
        buf[(x, y)].set_char(ch).set_fg(fg).set_bg(bg);
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
        buf[(x, y)].set_symbol(&s).set_fg(fg).set_bg(bg);
        if x + 1 < area.x + area.width {
            let next = &mut buf[(x + 1, y)];
            next.reset();
            next.set_skip(true);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn set_cell_styled(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    ch: char,
    fg: RColor,
    bg: RColor,
    modifier: Modifier,
    underline_color: Option<RColor>,
) {
    let area = buf.area;
    if x < area.x + area.width && y < area.y + area.height {
        let cell = &mut buf[(x, y)];
        cell.set_char(ch).set_fg(fg).set_bg(bg);
        cell.modifier = modifier;
        if let Some(ul) = underline_color {
            cell.underline_color = ul;
        }
    }
}

// ─── Tab bar ──────────────────────────────────────────────────────────────────
// Tab/diff constants are defined in render_impl.rs and re-exported via `use render_impl::*;`.

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
                ("".to_string(), Some(resolved))
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
        EngineAction::OpenUrl(url) => {
            #[cfg(target_os = "macos")]
            let cmd = "open";
            #[cfg(not(target_os = "macos"))]
            let cmd = "xdg-open";
            if crate::core::engine::is_safe_url(&url) {
                std::process::Command::new(cmd)
                    .arg(&url)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .ok();
            }
            false
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

// ─── Color / index helpers ───────────────────────────────────────────────────

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
