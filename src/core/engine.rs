use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use super::buffer::{Buffer, BufferId};
use super::buffer_manager::{BufferManager, BufferState};
use super::git;
use super::session::SessionState;
use super::settings::Settings;
use super::tab::{Tab, TabId};
use super::view::View;
use super::window::{SplitDirection, Window, WindowId, WindowLayout, WindowRect};
use super::{Cursor, Mode};

/// Actions returned from `handle_key` that the UI layer must act on.
/// This keeps GTK/platform concerns out of the core engine.
#[derive(Debug, PartialEq)]
pub enum EngineAction {
    None,
    Quit,
    SaveQuit,
    OpenFile(PathBuf),
    /// Display an error to the user (engine already set self.message)
    Error,
}

/// How a file should be opened: as a temporary preview or permanent buffer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpenMode {
    /// Preview mode: replaces the current window's buffer temporarily.
    /// Used internally; sidebar clicks use `open_file_in_tab` instead.
    #[allow(dead_code)]
    Preview,
    Permanent,
}

/// Maximum depth for macro recursion to prevent infinite loops.
const MAX_MACRO_RECURSION: usize = 100;

/// Direction of the last search operation
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchDirection {
    Forward,  // Last search was '/'
    Backward, // Last search was '?'
}

/// Represents a change operation that can be repeated with `.`
#[derive(Debug, Clone)]
struct Change {
    /// Type of operation
    op: ChangeOp,
    /// Text inserted (for insert operations)
    text: String,
    /// Count used with the operation
    count: usize,
    /// Motion used with operator (for d/c with motions)
    motion: Option<Motion>,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum ChangeOp {
    Insert,
    Delete,
    Change,
    Substitute,
    SubstituteLine,
    DeleteToEnd,
    ChangeToEnd,
    Replace,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordForward,
    WordBackward,
    WordEnd,
    WordBackwardEnd,
    LineStart,
    LineEnd,
    DeleteLine,
    CharFind(char, char), // (motion_type, target_char)
    ParagraphForward,
    ParagraphBackward,
    MatchingBracket,
    TextObject(char, char), // (modifier, object) - e.g., ('i', 'w')
}

pub struct Engine {
    // --- Multi-buffer/window state ---
    pub buffer_manager: BufferManager,
    pub windows: HashMap<WindowId, Window>,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    next_window_id: usize,
    next_tab_id: usize,

    // --- Preview mode ---
    /// The buffer currently in preview mode (at most one at a time).
    pub preview_buffer_id: Option<BufferId>,

    // --- Global state (not per-window) ---
    pub mode: Mode,
    /// Accumulates typed characters in Command/Search mode.
    pub command_buffer: String,
    /// Status message shown in the command line area (e.g. "written", errors).
    pub message: String,
    /// Current search query (from last `/` or `?` search).
    pub search_query: String,
    /// Char-offset pairs (start, end) for all search matches in active buffer.
    pub search_matches: Vec<(usize, usize)>,
    /// Index into `search_matches` for the current match.
    pub search_index: Option<usize>,
    /// Direction of the last search operation.
    pub search_direction: SearchDirection,
    /// Cursor position when search mode was entered (for incremental search)
    search_start_cursor: Option<Cursor>,

    // --- Find/Replace state ---
    /// Replacement text for current operation
    #[allow(dead_code)] // Reserved for future UI state tracking
    pub replace_text: String,
    /// Replace flags: 'g' (global), 'c' (confirm), 'i' (case-insensitive)
    #[allow(dead_code)] // Reserved for future UI state tracking
    pub replace_flags: String,

    /// Pending key for multi-key sequences (e.g. 'g' for gg, 'd' for dd).
    pub pending_key: Option<char>,

    // --- Registers (yank/delete storage) ---
    /// Named registers: 'a'-'z' plus '"' (unnamed default). Value is (content, is_linewise).
    pub registers: HashMap<char, (String, bool)>,
    /// Currently selected register for next yank/delete/paste (set by "x prefix).
    pub selected_register: Option<char>,

    // --- Marks ---
    /// Marks per buffer: BufferId -> (mark_char -> Cursor position)
    /// Supports 'a'-'z' for file-local marks
    pub marks: HashMap<BufferId, HashMap<char, Cursor>>,

    // --- Visual mode state ---
    /// Visual mode anchor point (where visual selection started).
    pub visual_anchor: Option<Cursor>,

    // --- Count state ---
    /// Accumulated count for commands (e.g., 5j, 3dd). None means no count entered yet.
    pub count: Option<usize>,

    // --- Character find state ---
    /// Last character find motion: (motion_type, target_char)
    /// motion_type: 'f', 'F', 't', 'T'
    pub last_find: Option<(char, char)>,

    // --- Operator state ---
    /// Pending operator waiting for a motion (e.g., 'd' for dw, 'c' for cw).
    pub pending_operator: Option<char>,

    // --- Text object state ---
    /// Pending text object modifier: 'i' (inner) or 'a' (around)
    pub pending_text_object: Option<char>,

    // --- Repeat state ---
    /// Last change operation for repeat (.)
    last_change: Option<Change>,
    /// Text accumulated during insert mode for repeat
    insert_text_buffer: String,

    // --- Settings ---
    /// Editor settings (line numbers, etc.)
    pub settings: Settings,

    // --- Session state (history, window geometry, etc.) ---
    /// Session state persisted across restarts
    pub session: SessionState,

    /// Current position in command history (None = typing new command)
    pub command_history_index: Option<usize>,

    /// Temporary buffer for current typing when cycling history
    pub command_typing_buffer: String,

    /// Whether Ctrl-R reverse history search is active
    pub history_search_active: bool,

    /// The search string typed during Ctrl-R history search
    pub history_search_query: String,

    /// The index into command history where the current match was found
    pub history_search_index: Option<usize>,

    /// Current position in search history
    pub search_history_index: Option<usize>,

    /// Temporary buffer for search typing
    pub search_typing_buffer: String,

    // --- Macro recording state ---
    /// Which register is recording (None if not recording).
    pub macro_recording: Option<char>,
    /// Accumulated keystrokes during recording.
    pub recording_buffer: Vec<char>,

    // --- Macro playback state ---
    /// Keys to inject for playback.
    pub macro_playback_queue: VecDeque<char>,
    /// Last macro played (for @@).
    pub last_macro_register: Option<char>,
    /// Prevent infinite recursion.
    pub macro_recursion_depth: usize,

    // --- Git integration ---
    /// Current git branch name (None if not in a git repo or git not available).
    pub git_branch: Option<String>,

    // --- Scroll binding ---
    /// Pairs of windows whose scroll_top should stay in sync (e.g. :Gblame).
    /// Each pair is (primary_window_id, secondary_window_id).
    scroll_bind_pairs: Vec<(WindowId, WindowId)>,

    // --- Completion state ---
    /// Current completion candidates (populated on first Ctrl-N/P).
    pub completion_candidates: Vec<String>,
    /// Index of the currently selected candidate, or None when inactive.
    pub completion_idx: Option<usize>,
    /// Buffer column where the prefix that triggered completion starts.
    pub completion_start_col: usize,
}

impl Engine {
    pub fn new() -> Self {
        let mut buffer_manager = BufferManager::new();
        let buffer_id = buffer_manager.create();

        let window_id = WindowId(1);
        let window = Window::new(window_id, buffer_id);
        let mut windows = HashMap::new();
        windows.insert(window_id, window);

        let tab = Tab::new(TabId(1), window_id);

        Self {
            buffer_manager,
            windows,
            tabs: vec![tab],
            active_tab: 0,
            next_window_id: 2,
            next_tab_id: 2,
            preview_buffer_id: None,
            mode: Mode::Normal,
            command_buffer: String::new(),
            message: String::new(),
            search_query: String::new(),
            search_matches: Vec::new(),
            search_index: None,
            search_direction: SearchDirection::Forward,
            search_start_cursor: None,
            replace_text: String::new(),
            replace_flags: String::new(),
            pending_key: None,
            registers: HashMap::new(),
            selected_register: None,
            marks: HashMap::new(),
            visual_anchor: None,
            count: None,
            last_find: None,
            pending_operator: None,
            pending_text_object: None,
            last_change: None,
            insert_text_buffer: String::new(),
            settings: {
                // Ensure settings.json exists with defaults
                Settings::ensure_exists().ok();
                Settings::load()
            },
            session: SessionState::load(),
            command_history_index: None,
            command_typing_buffer: String::new(),
            history_search_active: false,
            history_search_query: String::new(),
            history_search_index: None,
            search_history_index: None,
            search_typing_buffer: String::new(),
            macro_recording: None,
            recording_buffer: Vec::new(),
            macro_playback_queue: VecDeque::new(),
            last_macro_register: None,
            macro_recursion_depth: 0,
            git_branch: {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                git::current_branch(&cwd)
            },
            scroll_bind_pairs: Vec::new(),
            completion_candidates: Vec::new(),
            completion_idx: None,
            completion_start_col: 0,
        }
    }

    /// Create an engine with a file loaded (or empty buffer for new file).
    pub fn open(path: &Path) -> Self {
        let mut engine = Self::new();

        // Replace the default empty buffer with the file
        let old_buffer_id = engine.active_buffer_id();
        let _ = engine.buffer_manager.delete(old_buffer_id, true);

        match engine.buffer_manager.open_file(path) {
            Ok(buffer_id) => {
                // Update the window to point to the new buffer
                if let Some(window) = engine.windows.get_mut(&engine.active_window_id()) {
                    window.buffer_id = buffer_id;
                }
                // Restore saved cursor/scroll position from previous session
                let view = engine.restore_file_position(buffer_id);
                if let Some(window) = engine.windows.get_mut(&engine.active_window_id()) {
                    window.view = view;
                }
                engine.refresh_git_diff(buffer_id);
                if !path.exists() {
                    engine.message = format!("\"{}\" [New File]", path.display());
                }
            }
            Err(e) => {
                engine.message = format!("Error reading {}: {}", path.display(), e);
                // Create a new empty buffer since we deleted the old one
                let buffer_id = engine.buffer_manager.create();
                if let Some(window) = engine.windows.get_mut(&engine.active_window_id()) {
                    window.buffer_id = buffer_id;
                }
            }
        }

        engine
    }

    // =======================================================================
    // Accessors for active window/buffer (facade for backward compatibility)
    // =======================================================================

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab]
    }

    pub fn active_window_id(&self) -> WindowId {
        self.active_tab().active_window
    }

    pub fn active_window(&self) -> &Window {
        self.windows.get(&self.active_window_id()).unwrap()
    }

    pub fn active_window_mut(&mut self) -> &mut Window {
        let id = self.active_window_id();
        self.windows.get_mut(&id).unwrap()
    }

    pub fn active_buffer_id(&self) -> BufferId {
        self.active_window().buffer_id
    }

    pub fn active_buffer_state(&self) -> &BufferState {
        self.buffer_manager.get(self.active_buffer_id()).unwrap()
    }

    pub fn active_buffer_state_mut(&mut self) -> &mut BufferState {
        let id = self.active_buffer_id();
        self.buffer_manager.get_mut(id).unwrap()
    }

    /// Get the buffer for the active window.
    pub fn buffer(&self) -> &Buffer {
        &self.active_buffer_state().buffer
    }

    /// Get a mutable reference to the buffer for the active window.
    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.active_buffer_state_mut().buffer
    }

    /// Get the view for the active window.
    pub fn view(&self) -> &View {
        &self.active_window().view
    }

    /// Get a mutable reference to the view for the active window.
    pub fn view_mut(&mut self) -> &mut View {
        &mut self.active_window_mut().view
    }

    /// Get cursor position (facade for tests and compatibility).
    pub fn cursor(&self) -> &Cursor {
        &self.view().cursor
    }

    /// Get the file path for the active buffer.
    pub fn file_path(&self) -> Option<&PathBuf> {
        self.active_buffer_state().file_path.as_ref()
    }

    /// Check if the active buffer has unsaved changes.
    pub fn dirty(&self) -> bool {
        self.active_buffer_state().dirty
    }

    /// Set the dirty flag for the active buffer.
    pub fn set_dirty(&mut self, dirty: bool) {
        self.active_buffer_state_mut().dirty = dirty;
    }

    /// Get the syntax highlights for the active buffer.
    #[allow(dead_code)]
    pub fn highlights(&self) -> &[(usize, usize, String)] {
        &self.active_buffer_state().highlights
    }

    /// Get scroll_top for the active window.
    #[allow(dead_code)]
    pub fn scroll_top(&self) -> usize {
        self.view().scroll_top
    }

    /// Set scroll_top for the active window.
    #[allow(dead_code)]
    pub fn set_scroll_top(&mut self, scroll_top: usize) {
        self.view_mut().scroll_top = scroll_top;
    }

    /// Get viewport_lines for the active window.
    pub fn viewport_lines(&self) -> usize {
        self.view().viewport_lines
    }

    /// Set viewport_lines for the active window.
    pub fn set_viewport_lines(&mut self, lines: usize) {
        self.view_mut().viewport_lines = lines;
    }

    /// Get scroll_left for the active window.
    #[allow(dead_code)]
    pub fn scroll_left(&self) -> usize {
        self.view().scroll_left
    }

    /// Set scroll_left for the active window.
    #[allow(dead_code)]
    pub fn set_scroll_left(&mut self, scroll_left: usize) {
        self.view_mut().scroll_left = scroll_left;
    }

    /// Get viewport_cols for the active window.
    #[allow(dead_code)]
    pub fn viewport_cols(&self) -> usize {
        self.view().viewport_cols
    }

    /// Set viewport_cols for the active window.
    #[allow(dead_code)]
    pub fn set_viewport_cols(&mut self, cols: usize) {
        self.view_mut().viewport_cols = cols;
    }

    /// Set viewport dimensions for a specific window (used by TUI for per-pane sizing).
    pub fn set_viewport_for_window(&mut self, window_id: WindowId, lines: usize, cols: usize) {
        if let Some(window) = self.windows.get_mut(&window_id) {
            window.view.viewport_lines = lines;
            window.view.viewport_cols = cols;
        }
    }

    /// Set scroll_top for a specific window without changing the active window.
    pub fn set_scroll_top_for_window(&mut self, window_id: WindowId, scroll_top: usize) {
        if let Some(window) = self.windows.get_mut(&window_id) {
            window.view.scroll_top = scroll_top;
        }
    }

    /// Set scroll_left for a specific window without changing the active window.
    pub fn set_scroll_left_for_window(&mut self, window_id: WindowId, scroll_left: usize) {
        if let Some(window) = self.windows.get_mut(&window_id) {
            window.view.scroll_left = scroll_left;
        }
    }

    // =======================================================================
    // Buffer operations
    // =======================================================================

    pub fn update_syntax(&mut self) {
        self.active_buffer_state_mut().update_syntax();
    }

    // =======================================================================
    // Undo/Redo operations
    // =======================================================================

    /// Start a new undo group for the active buffer.
    pub fn start_undo_group(&mut self) {
        let cursor = *self.cursor();
        // Save line state before modification (for U command)
        self.save_line_for_undo();
        self.active_buffer_state_mut().start_undo_group(cursor);
    }

    /// Finish the current undo group for the active buffer.
    pub fn finish_undo_group(&mut self) {
        self.active_buffer_state_mut().finish_undo_group();
    }

    /// Insert text with undo recording.
    pub fn insert_with_undo(&mut self, pos: usize, text: &str) {
        self.active_buffer_state_mut().record_insert(pos, text);
        self.buffer_mut().insert(pos, text);
    }

    /// Delete a range with undo recording.
    pub fn delete_with_undo(&mut self, start: usize, end: usize) {
        // Capture the text being deleted before deleting
        let deleted_text: String = self.buffer().content.slice(start..end).chars().collect();
        self.active_buffer_state_mut()
            .record_delete(start, &deleted_text);
        self.buffer_mut().delete_range(start, end);
    }

    /// Perform undo on the active buffer. Returns true if undo was performed.
    pub fn undo(&mut self) -> bool {
        if let Some(cursor) = self.active_buffer_state_mut().undo() {
            self.view_mut().cursor = cursor;
            self.clamp_cursor_col();
            // Clear dirty flag if we've undone all changes
            if !self.active_buffer_state().can_undo() {
                self.set_dirty(false);
            }
            true
        } else {
            self.message = "Already at oldest change".to_string();
            false
        }
    }

    /// Perform redo on the active buffer. Returns true if redo was performed.
    pub fn redo(&mut self) -> bool {
        if let Some(cursor) = self.active_buffer_state_mut().redo() {
            self.view_mut().cursor = cursor;
            self.clamp_cursor_col();
            true
        } else {
            self.message = "Already at newest change".to_string();
            false
        }
    }

    /// Check if undo is available.
    #[allow(dead_code)]
    pub fn can_undo(&self) -> bool {
        self.active_buffer_state().can_undo()
    }

    /// Check if redo is available.
    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        self.active_buffer_state().can_redo()
    }

    /// Undo all changes on the current line (U command). Returns true if undo was performed.
    pub fn undo_line(&mut self) -> bool {
        let current_line = self.view().cursor.line;
        let cursor = self.view().cursor;

        if let Some(restored_cursor) = self
            .active_buffer_state_mut()
            .undo_line(current_line, cursor)
        {
            self.view_mut().cursor = restored_cursor;
            self.clamp_cursor_col();
            self.message = "Line restored".to_string();
            true
        } else {
            self.message = "No changes to undo on this line".to_string();
            false
        }
    }

    /// Save the current line state before modification (for U command)
    pub fn save_line_for_undo(&mut self) {
        let current_line = self.view().cursor.line;
        self.active_buffer_state_mut()
            .save_line_for_undo(current_line);
    }

    /// Save the active buffer to its file.
    pub fn save(&mut self) -> Result<(), String> {
        // Promote preview on save
        let active_id = self.active_buffer_id();
        if self.preview_buffer_id == Some(active_id) {
            self.promote_preview(active_id);
        }
        let state = self.active_buffer_state_mut();
        if let Some(ref path) = state.file_path.clone() {
            match state.save() {
                Ok(line_count) => {
                    self.message = format!("\"{}\" {}L written", path.display(), line_count);
                    // Refresh git diff after save
                    let id = self.active_buffer_id();
                    self.refresh_git_diff(id);
                    Ok(())
                }
                Err(e) => {
                    self.message = format!("Error writing {}: {}", path.display(), e);
                    Err(self.message.clone())
                }
            }
        } else {
            self.message = "No file name".to_string();
            Err(self.message.clone())
        }
    }

    // =======================================================================
    // Git integration
    // =======================================================================

    /// Refresh git diff markers for the given buffer.
    fn refresh_git_diff(&mut self, buffer_id: BufferId) {
        if let Some(path) = self
            .buffer_manager
            .get(buffer_id)
            .and_then(|s| s.file_path.clone())
        {
            let diff = git::compute_file_diff(&path);
            if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
                state.git_diff = diff;
            }
        }
    }

    /// Open the git diff for the current file in a vertical split.
    fn cmd_git_diff(&mut self) -> EngineAction {
        let path = match self.file_path().map(|p| p.to_path_buf()) {
            Some(p) => p,
            None => {
                self.message = "No file".to_string();
                return EngineAction::Error;
            }
        };
        match git::file_diff_text(&path) {
            Some(text) => {
                let buf_id = self.buffer_manager.create();
                if let Some(state) = self.buffer_manager.get_mut(buf_id) {
                    state.buffer.content = ropey::Rope::from_str(&text);
                }
                // Split vertically; the new window (now active) shares the original buffer.
                // Redirect it to the diff buffer without touching the original.
                self.split_window(SplitDirection::Vertical, None);
                self.active_window_mut().buffer_id = buf_id;
                self.message = format!("Git diff: {}", path.display());
                EngineAction::None
            }
            None => {
                self.message = "No changes (clean working tree)".to_string();
                EngineAction::None
            }
        }
    }

    /// Helper: resolve the git repo dir from either the current file's directory or cwd.
    fn git_dir(&self) -> PathBuf {
        self.file_path()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    /// Open `git status` output in a new read-only buffer (vertical split).
    fn cmd_git_status(&mut self) -> EngineAction {
        let dir = self.git_dir();
        match git::status_text(&dir) {
            Some(text) => {
                let buf_id = self.buffer_manager.create();
                if let Some(state) = self.buffer_manager.get_mut(buf_id) {
                    state.buffer.content = ropey::Rope::from_str(&text);
                }
                self.split_window(SplitDirection::Vertical, None);
                self.active_window_mut().buffer_id = buf_id;
                self.message = "Git status".to_string();
                EngineAction::None
            }
            None => {
                self.message = "Not a git repository".to_string();
                EngineAction::Error
            }
        }
    }

    /// Stage the current file (`:Gadd`) or all changes (`:Gadd!`).
    fn cmd_git_add(&mut self, all: bool) -> EngineAction {
        let dir = self.git_dir();
        let result = if all {
            git::stage_all(&dir)
        } else {
            match self.file_path().map(|p| p.to_path_buf()) {
                Some(path) => git::stage_file(&path),
                None => Err("No file to stage".to_string()),
            }
        };
        match result {
            Ok(()) => {
                // Refresh git diff markers for all open buffers.
                let ids: Vec<_> = self.buffer_manager.list();
                for id in ids {
                    self.refresh_git_diff(id);
                }
                // Update branch (in case this was first commit)
                self.git_branch = git::current_branch(&dir);
                let label = if all { "all files" } else { "current file" };
                self.message = format!("Staged {}", label);
                EngineAction::None
            }
            Err(e) => {
                self.message = e;
                EngineAction::Error
            }
        }
    }

    /// Commit staged changes with the given message (`:Gcommit <msg>`).
    fn cmd_git_commit(&mut self, message: &str) -> EngineAction {
        let dir = self.git_dir();
        match git::commit(&dir, message) {
            Ok(summary) => {
                // Update branch name (in case first commit on new branch).
                self.git_branch = git::current_branch(&dir);
                // Refresh diffs (committed changes are no longer "modified").
                let ids: Vec<_> = self.buffer_manager.list();
                for id in ids {
                    self.refresh_git_diff(id);
                }
                self.message = summary;
                EngineAction::None
            }
            Err(e) => {
                self.message = e;
                EngineAction::Error
            }
        }
    }

    /// Push current branch to remote (`:Gpush`).
    fn cmd_git_push(&mut self) -> EngineAction {
        let dir = self.git_dir();
        match git::push(&dir) {
            Ok(summary) => {
                self.message = if summary.is_empty() {
                    "Pushed".to_string()
                } else {
                    summary
                };
                EngineAction::None
            }
            Err(e) => {
                self.message = e;
                EngineAction::Error
            }
        }
    }

    /// Open `git blame` for the current file in a vertical split.
    fn cmd_git_blame(&mut self) -> EngineAction {
        let path = match self.file_path().map(|p| p.to_path_buf()) {
            Some(p) => p,
            None => {
                self.message = "No file".to_string();
                return EngineAction::Error;
            }
        };
        match git::blame_text(&path) {
            Some(text) => {
                let buf_id = self.buffer_manager.create();
                if let Some(state) = self.buffer_manager.get_mut(buf_id) {
                    state.buffer.content = ropey::Rope::from_str(&text);
                }
                let source_win = self.active_window_id();
                self.split_window(SplitDirection::Vertical, None);
                let blame_win = self.active_window_id();
                self.active_window_mut().buffer_id = buf_id;
                self.scroll_bind_pairs.push((source_win, blame_win));
                self.message = format!("Git blame: {}", path.display());
                EngineAction::None
            }
            None => {
                self.message =
                    "No blame info (file not committed or not in a git repo)".to_string();
                EngineAction::Error
            }
        }
    }

    // =======================================================================
    // Preview mode
    // =======================================================================

    /// Promote a preview buffer to permanent.
    pub fn promote_preview(&mut self, buffer_id: BufferId) {
        if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
            state.preview = false;
        }
        if self.preview_buffer_id == Some(buffer_id) {
            self.preview_buffer_id = None;
        }
    }

    /// Open a file in the current window with the given mode.
    ///
    /// - `Preview`: Replaces any existing preview buffer. The tab shows italic/dimmed.
    /// - `Permanent`: Opens the file as a normal, persistent buffer.
    ///
    /// If the file is already open as a permanent buffer, just switches to it regardless of mode.
    pub fn open_file_with_mode(&mut self, path: &Path, mode: OpenMode) -> Result<(), String> {
        // Check which buffers exist before opening (to detect reuse vs creation)
        let existing_ids: Vec<_> = self.buffer_manager.list();

        let buffer_id = self
            .buffer_manager
            .open_file(path)
            .map_err(|e| format!("Error: {}", e))?;

        let already_existed = existing_ids.contains(&buffer_id);
        let is_already_permanent = already_existed
            && self
                .buffer_manager
                .get(buffer_id)
                .is_some_and(|s| !s.preview);

        // If buffer already exists as permanent, just switch to it
        if is_already_permanent && self.preview_buffer_id != Some(buffer_id) {
            let current = self.active_buffer_id();
            if current != buffer_id {
                self.buffer_manager.alternate_buffer = Some(current);
            }
            self.switch_window_buffer(buffer_id);
            self.message = format!("\"{}\"", path.display());
            return Ok(());
        }

        match mode {
            OpenMode::Preview => {
                // Close old preview if it's a different buffer
                if let Some(old_preview) = self.preview_buffer_id {
                    if old_preview != buffer_id {
                        // Only close if no other window shows it
                        let _ = self.delete_buffer(old_preview, true);
                    }
                }
                // Mark as preview
                if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
                    state.preview = true;
                }
                self.preview_buffer_id = Some(buffer_id);
            }
            OpenMode::Permanent => {
                // If it was a preview, promote it
                if self.preview_buffer_id == Some(buffer_id) {
                    self.promote_preview(buffer_id);
                }
            }
        }

        let current = self.active_buffer_id();
        if current != buffer_id {
            self.buffer_manager.alternate_buffer = Some(current);
        }
        self.switch_window_buffer(buffer_id);
        self.refresh_git_diff(buffer_id);
        self.message = format!("\"{}\"", path.display());
        Ok(())
    }

    // =======================================================================
    // Window operations
    // =======================================================================

    /// Create a new window ID.
    fn new_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        id
    }

    /// Create a new tab ID.
    fn new_tab_id(&mut self) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        id
    }

    /// Split the active window in the given direction.
    pub fn split_window(&mut self, direction: SplitDirection, file_path: Option<&Path>) {
        let current_buffer_id = self.active_buffer_id();
        let current_window_id = self.active_window_id();

        // Determine which buffer the new window should show
        let new_buffer_id = if let Some(path) = file_path {
            match self.buffer_manager.open_file(path) {
                Ok(id) => id,
                Err(e) => {
                    self.message = format!("Error: {}", e);
                    return;
                }
            }
        } else {
            // Same buffer as current window
            current_buffer_id
        };

        // Create new window
        let new_window_id = self.new_window_id();
        let mut new_window = Window::new(new_window_id, new_buffer_id);

        // Copy view state if same buffer
        if new_buffer_id == current_buffer_id {
            new_window.view = self.active_window().view.clone();
        }

        self.windows.insert(new_window_id, new_window);

        // Update layout
        let tab = self.active_tab_mut();
        tab.layout
            .split_at(current_window_id, direction, new_window_id, false);
        tab.active_window = new_window_id;

        self.message = String::new();
    }

    /// Close the active window. Returns true if the window was closed.
    pub fn close_window(&mut self) -> bool {
        let tab = &self.tabs[self.active_tab];

        // Can't close the last window in the last tab
        if tab.layout.is_single_window() && self.tabs.len() == 1 {
            self.message = "Cannot close last window".to_string();
            return false;
        }

        let window_id = tab.active_window;

        // If this is the last window in the tab, close the tab
        if tab.layout.is_single_window() {
            return self.close_tab();
        }

        // Remove window from layout
        let tab = self.active_tab_mut();
        if let Some(new_layout) = tab.layout.remove(window_id) {
            tab.layout = new_layout;
            // Set new active window
            if let Some(new_active) = tab.layout.window_ids().first().copied() {
                tab.active_window = new_active;
            }
        }

        // Remove window from windows map and any scroll-bind pairs that referenced it.
        self.windows.remove(&window_id);
        self.scroll_bind_pairs
            .retain(|&(a, b)| a != window_id && b != window_id);

        true
    }

    /// Close all windows except the active one in the current tab.
    pub fn close_other_windows(&mut self) {
        let active_window_id = self.active_window_id();
        let tab = self.active_tab_mut();

        // Get all window IDs except active
        let windows_to_close: Vec<WindowId> = tab
            .layout
            .window_ids()
            .into_iter()
            .filter(|&id| id != active_window_id)
            .collect();

        // Reset layout to single window
        tab.layout = WindowLayout::leaf(active_window_id);

        // Remove closed windows and any scroll-bind pairs referencing them.
        for id in windows_to_close {
            self.windows.remove(&id);
            self.scroll_bind_pairs.retain(|&(a, b)| a != id && b != id);
        }

        self.message = String::new();
    }

    /// Move focus to the next window in the current tab.
    pub fn focus_next_window(&mut self) {
        self.active_tab_mut().cycle_next_window();
    }

    /// Move focus to the previous window in the current tab.
    pub fn focus_prev_window(&mut self) {
        self.active_tab_mut().cycle_prev_window();
    }

    /// Move focus to a window in the given direction.
    pub fn focus_window_direction(&mut self, _direction: SplitDirection, forward: bool) {
        // For now, just cycle - proper directional navigation requires geometry
        if forward {
            self.focus_next_window();
        } else {
            self.focus_prev_window();
        }
    }

    /// Get the layout rectangles for the current tab.
    pub fn calculate_window_rects(&self, bounds: WindowRect) -> Vec<(WindowId, WindowRect)> {
        self.active_tab().layout.calculate_rects(bounds)
    }

    /// Set cursor position for a specific window and make it active.
    /// Clamps line and col to valid buffer positions.
    pub fn set_cursor_for_window(&mut self, window_id: WindowId, line: usize, col: usize) {
        // Make the window active
        if self.windows.contains_key(&window_id) {
            self.active_tab_mut().active_window = window_id;

            // Get buffer and clamp line
            let buffer = self.buffer();
            let max_line = buffer.content.len_lines().saturating_sub(1);
            let clamped_line = line.min(max_line);

            // Get max col for this line (excludes newline)
            let max_col = self.get_max_cursor_col(clamped_line);
            let clamped_col = col.min(max_col);

            // Set cursor position
            let view = self.view_mut();
            view.cursor.line = clamped_line;
            view.cursor.col = clamped_col;
        }
    }

    // =======================================================================
    // Tab operations
    // =======================================================================

    /// Create a new tab with an optional file.
    pub fn new_tab(&mut self, file_path: Option<&Path>) {
        let buffer_id = if let Some(path) = file_path {
            match self.buffer_manager.open_file(path) {
                Ok(id) => id,
                Err(e) => {
                    self.message = format!("Error: {}", e);
                    return;
                }
            }
        } else {
            self.buffer_manager.create()
        };

        let window_id = self.new_window_id();
        let window = Window::new(window_id, buffer_id);
        self.windows.insert(window_id, window);

        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;

        self.message = String::new();
    }

    /// Close the current tab. Returns true if closed.
    pub fn close_tab(&mut self) -> bool {
        if self.tabs.len() <= 1 {
            self.message = "Cannot close last tab".to_string();
            return false;
        }

        // Remove all windows in this tab
        let tab = &self.tabs[self.active_tab];
        for window_id in tab.window_ids() {
            self.windows.remove(&window_id);
        }

        self.tabs.remove(self.active_tab);

        // Adjust active tab index
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }

        true
    }

    /// Switch to the next tab.
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
        }
    }

    /// Switch to the previous tab.
    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = if self.active_tab == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab - 1
            };
        }
    }

    /// Switch to a specific tab (0-indexed).
    #[allow(dead_code)]
    pub fn goto_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
        }
    }

    /// Open a file from the explorer: switch to an existing tab that shows it,
    /// or create a new tab when no tab currently displays it.
    ///
    /// This is the correct handler for sidebar file clicks — it never replaces
    /// the current tab's contents.
    pub fn open_file_in_tab(&mut self, path: &Path) {
        let buffer_id = match self.buffer_manager.open_file(path) {
            Ok(id) => id,
            Err(e) => {
                self.message = format!("Error: {}", e);
                return;
            }
        };

        // If this buffer is the current preview, just promote it in-place.
        if self.preview_buffer_id == Some(buffer_id) {
            self.promote_preview(buffer_id);
            self.refresh_git_diff(buffer_id);
            self.message = format!("\"{}\"", path.display());
            return;
        }

        // Switch to any existing tab whose active window already shows this buffer.
        for (tab_idx, tab) in self.tabs.iter().enumerate() {
            if let Some(win) = self.windows.get(&tab.active_window) {
                if win.buffer_id == buffer_id {
                    self.active_tab = tab_idx;
                    self.refresh_git_diff(buffer_id);
                    self.message = format!("\"{}\"", path.display());
                    return;
                }
            }
        }

        // No existing tab shows this file — open it in a new tab.
        let window_id = self.new_window_id();
        let window = Window::new(window_id, buffer_id);
        self.windows.insert(window_id, window);

        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;

        // Restore saved cursor/scroll position.
        let view = self.restore_file_position(buffer_id);
        if let Some(w) = self.windows.get_mut(&window_id) {
            w.view = view;
        }

        self.refresh_git_diff(buffer_id);
        self.message = format!("\"{}\"", path.display());
    }

    /// Open a file from the sidebar via single-click (preview mode).
    ///
    /// Behaviour mirrors VSCode:
    /// - If the file is already shown in any tab, just switch to that tab.
    /// - If there is an existing preview tab, replace it with this file.
    /// - Otherwise open a new preview tab.
    ///
    /// A preview buffer is marked italic/dimmed and is replaced by the next
    /// single-click. Double-clicking (or editing/saving) promotes it to
    /// permanent.
    pub fn open_file_preview(&mut self, path: &Path) {
        let buffer_id = match self.buffer_manager.open_file(path) {
            Ok(id) => id,
            Err(e) => {
                self.message = format!("Error: {}", e);
                return;
            }
        };

        // Already shown in any tab? Just switch to it (permanent or current preview).
        for (tab_idx, tab) in self.tabs.iter().enumerate() {
            if let Some(win) = self.windows.get(&tab.active_window) {
                if win.buffer_id == buffer_id {
                    self.active_tab = tab_idx;
                    self.refresh_git_diff(buffer_id);
                    self.message = format!("\"{}\"", path.display());
                    return;
                }
            }
        }

        // Find the existing preview tab, if any.
        let mut preview_slot: Option<(usize, WindowId, BufferId)> = None;
        if let Some(preview_buf_id) = self.preview_buffer_id {
            for (idx, tab) in self.tabs.iter().enumerate() {
                if let Some(win) = self.windows.get(&tab.active_window) {
                    if win.buffer_id == preview_buf_id {
                        preview_slot = Some((idx, tab.active_window, preview_buf_id));
                        break;
                    }
                }
            }
        }

        if let Some((tab_idx, win_id, old_buf_id)) = preview_slot {
            // Reuse the existing preview tab: close old preview buffer and
            // point the window at the new one.
            let _ = self.delete_buffer(old_buf_id, true);
            if let Some(w) = self.windows.get_mut(&win_id) {
                w.buffer_id = buffer_id;
            }
            if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
                state.preview = true;
            }
            self.preview_buffer_id = Some(buffer_id);
            self.active_tab = tab_idx;
            let view = self.restore_file_position(buffer_id);
            if let Some(w) = self.windows.get_mut(&win_id) {
                w.view = view;
            }
        } else {
            // No preview tab yet — open a new one.
            let window_id = self.new_window_id();
            let window = Window::new(window_id, buffer_id);
            self.windows.insert(window_id, window);
            let tab_id = self.new_tab_id();
            let tab = Tab::new(tab_id, window_id);
            self.tabs.push(tab);
            self.active_tab = self.tabs.len() - 1;
            if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
                state.preview = true;
            }
            self.preview_buffer_id = Some(buffer_id);
            let view = self.restore_file_position(buffer_id);
            if let Some(w) = self.windows.get_mut(&window_id) {
                w.view = view;
            }
        }

        self.refresh_git_diff(buffer_id);
        self.message = format!("\"{}\"", path.display());
    }

    // =======================================================================
    // Buffer navigation
    // =======================================================================

    /// Switch the current window to the next buffer.
    pub fn next_buffer(&mut self) {
        let current = self.active_buffer_id();
        if let Some(next) = self.buffer_manager.next_buffer(current) {
            self.buffer_manager.alternate_buffer = Some(current);
            self.switch_window_buffer(next);
        }
    }

    /// Switch the current window to the previous buffer.
    pub fn prev_buffer(&mut self) {
        let current = self.active_buffer_id();
        if let Some(prev) = self.buffer_manager.prev_buffer(current) {
            self.buffer_manager.alternate_buffer = Some(current);
            self.switch_window_buffer(prev);
        }
    }

    /// Switch the current window to the alternate buffer.
    pub fn alternate_buffer(&mut self) {
        if let Some(alt) = self.buffer_manager.alternate_buffer {
            let current = self.active_buffer_id();
            self.buffer_manager.alternate_buffer = Some(current);
            self.switch_window_buffer(alt);
        } else {
            self.message = "No alternate buffer".to_string();
        }
    }

    /// Switch the current window to a buffer by number (1-indexed).
    pub fn goto_buffer(&mut self, num: usize) {
        if let Some(id) = self.buffer_manager.get_by_number(num) {
            let current = self.active_buffer_id();
            if id != current {
                self.buffer_manager.alternate_buffer = Some(current);
                self.switch_window_buffer(id);
            }
        } else {
            self.message = format!("Buffer {} does not exist", num);
        }
    }

    /// Switch the current window to a different buffer.
    fn switch_window_buffer(&mut self, buffer_id: BufferId) {
        if self.buffer_manager.get(buffer_id).is_none() {
            return;
        }

        // Save current buffer's cursor/scroll position before switching
        let current_id = self.active_window().buffer_id;
        if current_id != buffer_id {
            if let Some(path) = self
                .buffer_manager
                .get(current_id)
                .and_then(|s| s.file_path.as_deref())
                .map(|p| p.to_path_buf())
            {
                let view = &self.active_window().view;
                self.session.save_file_position(
                    &path,
                    view.cursor.line,
                    view.cursor.col,
                    view.scroll_top,
                );
            }
        }

        // Switch to the new buffer
        self.active_window_mut().buffer_id = buffer_id;

        // Restore saved position, clamped to actual buffer bounds
        let new_view = self.restore_file_position(buffer_id);
        self.active_window_mut().view = new_view;

        self.search_matches.clear();
        self.search_index = None;
    }

    /// Build a View restoring the saved position for a buffer, or return View::new().
    fn restore_file_position(&self, buffer_id: BufferId) -> View {
        let path = match self
            .buffer_manager
            .get(buffer_id)
            .and_then(|s| s.file_path.as_deref())
            .map(|p| p.to_path_buf())
        {
            Some(p) => p,
            None => return View::new(),
        };

        let pos = match self.session.get_file_position(&path) {
            Some(p) => p,
            None => return View::new(),
        };

        let buf = self.buffer_manager.get(buffer_id).unwrap();
        let max_line = buf.buffer.len_lines().saturating_sub(1);
        let line = pos.line.min(max_line);
        let line_len = buf.buffer.line_len_chars(line);
        let max_col = line_len.saturating_sub(1);
        let col = pos.col.min(max_col);
        let scroll_top = pos.scroll_top.min(max_line);

        View {
            cursor: Cursor { line, col },
            scroll_top,
            ..View::new()
        }
    }

    /// Return the absolute paths of buffers currently shown in at least one window.
    /// Orphaned buffers (closed via :q but not yet freed) are intentionally excluded so
    /// that files the user explicitly closed are not restored on the next startup.
    pub fn open_file_paths(&self) -> Vec<std::path::PathBuf> {
        let in_window: std::collections::HashSet<BufferId> =
            self.windows.values().map(|w| w.buffer_id).collect();
        self.buffer_manager
            .list()
            .into_iter()
            .filter(|id| in_window.contains(id))
            .filter_map(|id| {
                self.buffer_manager
                    .get(id)
                    .and_then(|s| s.file_path.clone())
            })
            .collect()
    }

    /// Snapshot the current open-file list and active file into session state, ready for saving.
    pub fn collect_session_open_files(&mut self) {
        self.session.open_files = self.open_file_paths();
        self.session.active_file = self
            .buffer_manager
            .get(self.active_buffer_id())
            .and_then(|s| s.file_path.clone());
    }

    /// Restore open files from session state (called at startup when no CLI file is given).
    /// Each file gets its own tab; the previously-active file's tab is focused.
    /// Skips files that no longer exist. Removes the initial empty scratch buffer.
    pub fn restore_session_files(&mut self) {
        let paths = self.session.open_files.clone();
        let active = self.session.active_file.clone();

        if paths.is_empty() {
            return;
        }

        let initial_id = self.active_buffer_id();
        let mut any_opened = false;
        let mut first = true;

        for path in &paths {
            if !path.exists() {
                continue;
            }
            if first {
                // Reuse the initial window for the first file.
                if self.open_file_with_mode(path, OpenMode::Permanent).is_ok() {
                    any_opened = true;
                    first = false;
                }
            } else {
                // Each subsequent file gets its own tab.
                self.new_tab(Some(path));
                let buf_id = self.active_buffer_id();
                let view = self.restore_file_position(buf_id);
                let win_id = self.active_tab().active_window;
                if let Some(window) = self.windows.get_mut(&win_id) {
                    window.view = view;
                }
                any_opened = true;
            }
        }

        if !any_opened {
            return;
        }

        // Remove the initial empty scratch buffer now that real files are open.
        let _ = self.delete_buffer(initial_id, true);

        // Switch focus to the tab showing the previously-active file.
        if let Some(ref ap) = active {
            if let Ok(canonical_ap) = ap.canonicalize() {
                let tab_idx = self.tabs.iter().position(|t| {
                    self.windows
                        .get(&t.active_window)
                        .and_then(|w| self.buffer_manager.get(w.buffer_id))
                        .and_then(|s| s.file_path.as_ref())
                        .and_then(|p| p.canonicalize().ok())
                        .map_or(false, |p| p == canonical_ap)
                });
                if let Some(idx) = tab_idx {
                    self.active_tab = idx;
                }
            }
        }
    }

    /// Delete a buffer. Returns error if buffer is shown in any window or is dirty.
    pub fn delete_buffer(&mut self, id: BufferId, force: bool) -> Result<(), String> {
        // Check if buffer is shown in any window
        let in_use: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, w)| w.buffer_id == id)
            .map(|(wid, _)| *wid)
            .collect();

        if !in_use.is_empty() && self.buffer_manager.len() > 1 {
            // Switch those windows to another buffer
            let alt = self
                .buffer_manager
                .list()
                .into_iter()
                .find(|&bid| bid != id);

            if let Some(alt_id) = alt {
                for wid in in_use {
                    if let Some(window) = self.windows.get_mut(&wid) {
                        window.buffer_id = alt_id;
                        window.view = View::new();
                    }
                }
            }
        }

        // Clear preview tracking if deleting the preview buffer
        if self.preview_buffer_id == Some(id) {
            self.preview_buffer_id = None;
        }

        self.buffer_manager.delete(id, force)
    }

    /// Get the list of buffers for :ls display.
    pub fn list_buffers(&self) -> String {
        let active = self.active_buffer_id();
        let alternate = self.buffer_manager.alternate_buffer;

        let mut lines = Vec::new();
        for (i, id) in self.buffer_manager.list().iter().enumerate() {
            let state = self.buffer_manager.get(*id).unwrap();
            let num = i + 1;
            let active_flag = if *id == active { "%a" } else { "  " };
            let alt_flag = if Some(*id) == alternate { "#" } else { " " };
            let dirty_flag = if state.dirty { "+" } else { " " };
            let name = state.display_name();
            let preview_flag = if state.preview { " [Preview]" } else { "" };
            lines.push(format!(
                "{:3} {}{}{} \"{}\"{}",
                num, active_flag, alt_flag, dirty_flag, name, preview_flag
            ));
        }
        lines.join("\n")
    }

    // =======================================================================
    // Cursor helpers (delegating to buffer/view)
    // =======================================================================

    fn get_max_cursor_col(&self, line_idx: usize) -> usize {
        let buffer = self.buffer();
        let len = buffer.line_len_chars(line_idx);
        if len == 0 {
            return 0;
        }

        let line = buffer.content.line(line_idx);
        let ends_with_newline = line.chars().last() == Some('\n');

        if ends_with_newline {
            if len > 1 {
                len - 2
            } else {
                0
            }
        } else {
            len.saturating_sub(1)
        }
    }

    fn clamp_cursor_col(&mut self) {
        let line = self.view().cursor.line;
        let max_col = self.get_max_cursor_col(line);
        let view = self.view_mut();
        if view.cursor.col > max_col {
            view.cursor.col = max_col;
        }
    }

    /// Ensure the cursor is visible within the viewport, adjusting scroll_top.
    pub fn ensure_cursor_visible(&mut self) {
        self.view_mut().ensure_cursor_visible();
    }

    /// Synchronise the scroll_top of scroll-bound window pairs.
    /// Called after every key that may move the cursor or scroll, and also
    /// after direct scroll_top mutations (e.g. scrollbar drag).
    pub fn sync_scroll_binds(&mut self) {
        if self.scroll_bind_pairs.is_empty() {
            return;
        }
        let active_id = self.active_window_id();
        let active_scroll = self.active_window().view.scroll_top;
        let pairs = self.scroll_bind_pairs.clone();
        for (a, b) in pairs {
            let partner = if a == active_id {
                Some(b)
            } else if b == active_id {
                Some(a)
            } else {
                None
            };
            if let Some(pid) = partner {
                if let Some(w) = self.windows.get_mut(&pid) {
                    w.view.scroll_top = active_scroll;
                }
            }
        }
    }

    // =======================================================================
    // Key handling
    // =======================================================================

    /// Process a key event and return an action the UI should perform.
    pub fn handle_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
    ) -> EngineAction {
        // Clear message on any keypress (unless we're in command/search mode)
        if self.mode != Mode::Command && self.mode != Mode::Search {
            self.message.clear();
        }

        // Record keystroke if macro recording is active
        // Skip recording the 'q' that stops recording
        if self.macro_recording.is_some() {
            let is_stop_q =
                self.mode == Mode::Normal && unicode == Some('q') && self.pending_key.is_none();

            if !is_stop_q {
                // Encode the keystroke for recording
                let encoded = self.encode_key_for_macro(key_name, unicode, ctrl);
                for ch in encoded.chars() {
                    self.recording_buffer.push(ch);
                }
            }
        }

        // Ctrl-S: save in any mode (does not change mode).
        if ctrl && key_name == "s" {
            if let Err(e) = self.save() {
                self.message = format!("Save failed: {}", e);
            }
            return EngineAction::None;
        }

        let mut changed = false;
        let mut action = EngineAction::None;

        match self.mode {
            Mode::Normal => {
                action = self.handle_normal_key(key_name, unicode, ctrl, &mut changed);
            }
            Mode::Insert => {
                self.handle_insert_key(key_name, unicode, ctrl, &mut changed);
            }
            Mode::Command => {
                action = self.handle_command_key(key_name, unicode, ctrl);
            }
            Mode::Search => {
                self.handle_search_key(key_name, unicode);
            }
            Mode::Visual | Mode::VisualLine | Mode::VisualBlock => {
                action = self.handle_visual_key(key_name, unicode, ctrl, &mut changed);
            }
        }

        if changed {
            self.set_dirty(true);
            self.update_syntax();
            // Auto-promote preview buffer on text modification
            let active_id = self.active_buffer_id();
            if self.preview_buffer_id == Some(active_id) {
                self.promote_preview(active_id);
            }
        }

        self.ensure_cursor_visible();
        self.sync_scroll_binds();
        action
    }

    /// Decode a sequence from the macro playback queue.
    /// Returns (key_name, unicode, ctrl) tuple and the number of characters consumed.
    fn decode_macro_sequence(&mut self) -> Option<(String, Option<char>, bool, usize)> {
        if self.macro_playback_queue.is_empty() {
            return None;
        }

        let first_char = *self.macro_playback_queue.front().unwrap();

        // Check for angle-bracket notation (e.g., <Left>, <C-D>)
        if first_char == '<' {
            // Collect characters until we find '>'
            let mut sequence = String::new();
            let temp_queue: Vec<char> = self.macro_playback_queue.iter().copied().collect();

            for (i, &ch) in temp_queue.iter().enumerate() {
                sequence.push(ch);
                if ch == '>' {
                    // Found complete sequence
                    let len = i + 1;

                    // Parse the sequence
                    if let Some((key_name, unicode, ctrl)) = self.parse_key_sequence(&sequence) {
                        return Some((key_name, unicode, ctrl, len));
                    } else {
                        // Invalid sequence, treat '<' as literal
                        return Some(("".to_string(), Some('<'), false, 1));
                    }
                }
            }

            // No closing '>', treat '<' as literal
            return Some(("".to_string(), Some('<'), false, 1));
        }

        // Handle ESC
        if first_char == '\x1b' {
            return Some(("Escape".to_string(), None, false, 1));
        }

        // Regular character
        Some(("".to_string(), Some(first_char), false, 1))
    }

    /// Parse a key sequence like "<Left>", "<C-D>", "<CR>", etc.
    fn parse_key_sequence(&self, seq: &str) -> Option<(String, Option<char>, bool)> {
        if !seq.starts_with('<') || !seq.ends_with('>') {
            return None;
        }

        let inner = &seq[1..seq.len() - 1];

        // Check for Ctrl combinations: <C-X>
        if inner.starts_with("C-") && inner.len() == 3 {
            let ch = inner.chars().nth(2).unwrap().to_lowercase().next().unwrap();
            return Some((ch.to_string(), Some(ch), true));
        }

        // Special keys
        match inner {
            "CR" => Some(("Return".to_string(), None, false)),
            "BS" => Some(("BackSpace".to_string(), None, false)),
            "Del" => Some(("Delete".to_string(), None, false)),
            "Left" => Some(("Left".to_string(), None, false)),
            "Right" => Some(("Right".to_string(), None, false)),
            "Up" => Some(("Up".to_string(), None, false)),
            "Down" => Some(("Down".to_string(), None, false)),
            "Home" => Some(("Home".to_string(), None, false)),
            "End" => Some(("End".to_string(), None, false)),
            "PageUp" => Some(("Page_Up".to_string(), None, false)),
            "PageDown" => Some(("Page_Down".to_string(), None, false)),
            _ => None,
        }
    }

    /// Advance macro playback by processing the next keystroke in the queue.
    /// Returns true if there are more keys to process.
    pub fn advance_macro_playback(&mut self) -> (bool, EngineAction) {
        // Decode the next key sequence
        if let Some((key_name, unicode, ctrl, consume_count)) = self.decode_macro_sequence() {
            // Remove consumed characters from queue
            for _ in 0..consume_count {
                self.macro_playback_queue.pop_front();
            }

            self.macro_recursion_depth += 1;
            let action = self.handle_key(&key_name, unicode, ctrl);
            self.macro_recursion_depth -= 1;

            // Check if we hit recursion limit
            if self.macro_recursion_depth >= MAX_MACRO_RECURSION {
                self.macro_playback_queue.clear();
                self.message = "Macro recursion limit reached".to_string();
                return (false, EngineAction::Error);
            }

            (!self.macro_playback_queue.is_empty(), action)
        } else {
            (false, EngineAction::None)
        }
    }

    fn handle_normal_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        changed: &mut bool,
    ) -> EngineAction {
        // Handle Ctrl combinations first
        if ctrl {
            match key_name {
                "d" => {
                    // Half-page down
                    let count = self.take_count();
                    let half = self.viewport_lines() / 2;
                    let scroll_amount = half * count;
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    self.view_mut().cursor.line =
                        (self.view().cursor.line + scroll_amount).min(max_line);
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "u" => {
                    // Ctrl-U: Half-page up
                    let count = self.take_count();
                    let half = self.viewport_lines() / 2;
                    let scroll_amount = half * count;
                    self.view_mut().cursor.line =
                        self.view().cursor.line.saturating_sub(scroll_amount);
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "r" => {
                    // Ctrl-R: Redo
                    self.redo();
                    return EngineAction::None;
                }
                "f" => {
                    // Full page down
                    let count = self.take_count();
                    let viewport = self.viewport_lines();
                    let scroll_amount = viewport * count;
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    self.view_mut().cursor.line =
                        (self.view().cursor.line + scroll_amount).min(max_line);
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "b" => {
                    // Full page up
                    let count = self.take_count();
                    let viewport = self.viewport_lines();
                    let scroll_amount = viewport * count;
                    self.view_mut().cursor.line =
                        self.view().cursor.line.saturating_sub(scroll_amount);
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "w" => {
                    // Ctrl-W prefix for window commands
                    self.pending_key = Some('\x17'); // Ctrl-W marker
                    return EngineAction::None;
                }
                "v" => {
                    // Ctrl-V: Enter visual block mode
                    self.mode = Mode::VisualBlock;
                    self.visual_anchor = Some(self.view().cursor);
                    return EngineAction::None;
                }
                _ => {}
            }
        }

        // Handle pending multi-key sequences (gg, dd, Ctrl-W x, gt, r, f, t, m, etc.)
        // MUST come before count accumulation: pending keys like 'r' expect the next
        // character verbatim, including digits (e.g. r1 replaces with '1', not a count).
        if let Some(pending) = self.pending_key.take() {
            return self.handle_pending_key(pending, key_name, unicode, changed);
        }

        // Handle count accumulation (digits 1-9, and 0 when count already exists)
        if let Some(ch) = unicode {
            match ch {
                '1'..='9' => {
                    let digit = ch.to_digit(10).unwrap() as usize;
                    let new_count = self.count.unwrap_or(0) * 10 + digit;
                    if new_count > 10_000 {
                        self.count = Some(10_000);
                        self.message = "Count limited to 10,000".to_string();
                    } else {
                        self.count = Some(new_count);
                    }
                    return EngineAction::None;
                }
                '0' => {
                    if self.count.is_some() {
                        // Accumulate: 10, 20, 100, etc.
                        let new_count = self.count.unwrap() * 10;
                        if new_count > 10_000 {
                            self.count = Some(10_000);
                            self.message = "Count limited to 10,000".to_string();
                        } else {
                            self.count = Some(new_count);
                        }
                        return EngineAction::None;
                    }
                    // Fall through to handle '0' as "go to column 0" below
                }
                _ => {}
            }
        }

        // Handle pending operator + motion (dw, cw, etc.)
        if let Some(op) = self.pending_operator.take() {
            return self.handle_operator_motion(op, key_name, unicode, changed);
        }

        // In normal mode, check the unicode char for vim keys
        match unicode {
            Some('h') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_left();
                }
            }
            Some('j') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_down();
                }
            }
            Some('k') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_up();
                }
            }
            Some('l') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_right();
                }
            }
            Some('i') => {
                self.start_undo_group();
                self.insert_text_buffer.clear();
                self.mode = Mode::Insert;
                self.count = None; // Clear count when entering insert mode
            }
            Some('a') => {
                self.start_undo_group();
                self.insert_text_buffer.clear();
                let max_col = self.get_max_cursor_col(self.view().cursor.line);
                if self.view().cursor.col < max_col {
                    self.view_mut().cursor.col += 1;
                } else {
                    let line = self.view().cursor.line;
                    let insert_max = self.get_line_len_for_insert(line);
                    self.view_mut().cursor.col = insert_max;
                }
                self.mode = Mode::Insert;
                self.count = None; // Clear count when entering insert mode
            }
            Some('A') => {
                self.start_undo_group();
                self.insert_text_buffer.clear();
                let line = self.view().cursor.line;
                self.view_mut().cursor.col = self.get_line_len_for_insert(line);
                self.mode = Mode::Insert;
                self.count = None; // Clear count when entering insert mode
            }
            Some('I') => {
                self.start_undo_group();
                self.insert_text_buffer.clear();
                let line = self.view().cursor.line;
                let line_start = self.buffer().line_to_char(line);
                let line_len = self.buffer().line_len_chars(line);
                let mut col = 0;
                for i in 0..line_len {
                    let ch = self.buffer().content.char(line_start + i);
                    if ch != ' ' && ch != '\t' {
                        break;
                    }
                    col = i + 1;
                }
                self.view_mut().cursor.col = col;
                self.mode = Mode::Insert;
                self.count = None; // Clear count when entering insert mode
            }
            Some('o') => {
                let count = self.take_count();
                self.start_undo_group();
                let line = self.view().cursor.line;
                let indent = if self.settings.auto_indent {
                    self.get_line_indent_str(line)
                } else {
                    String::new()
                };
                let indent_len = indent.len();
                let line_end =
                    self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
                let line_content = self.buffer().content.line(line);
                let insert_pos = if self.buffer().line_len_chars(line) > 0 {
                    if line_content.chars().last() == Some('\n') {
                        line_end - 1
                    } else {
                        line_end
                    }
                } else {
                    line_end
                };
                // Insert newlines (with indent on the first new line for count==1).
                // For count > 1 only the first new line gets the indent; the rest are blank.
                let text = if count == 1 {
                    format!("\n{}", indent)
                } else {
                    format!("\n{}{}", indent, "\n".repeat(count - 1))
                };
                self.insert_with_undo(insert_pos, &text);
                self.insert_text_buffer.clear();
                self.view_mut().cursor.line += 1;
                self.view_mut().cursor.col = indent_len;
                self.mode = Mode::Insert;
                self.count = None; // Clear count when entering insert mode
                *changed = true;
            }
            Some('O') => {
                let count = self.take_count();
                self.start_undo_group();
                let line = self.view().cursor.line;
                let indent = if self.settings.auto_indent {
                    self.get_line_indent_str(line)
                } else {
                    String::new()
                };
                let indent_len = indent.len();
                let line_start = self.buffer().line_to_char(line);
                // Insert indent + newlines above current line.
                let text = if count == 1 {
                    format!("{}\n", indent)
                } else {
                    format!("{}\n{}", indent, "\n".repeat(count - 1))
                };
                self.insert_with_undo(line_start, &text);
                self.insert_text_buffer.clear();
                self.view_mut().cursor.col = indent_len;
                self.mode = Mode::Insert;
                self.count = None; // Clear count when entering insert mode
                *changed = true;
            }
            Some('0') => self.view_mut().cursor.col = 0,
            Some('$') => {
                let line = self.view().cursor.line;
                self.view_mut().cursor.col = self.get_max_cursor_col(line);
            }
            Some('x') => {
                let count = self.take_count();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let max_col = self.get_max_cursor_col(line);
                if max_col > 0 || self.buffer().line_len_chars(line) > 0 {
                    let char_idx = self.buffer().line_to_char(line) + col;
                    // Calculate how many chars we can actually delete
                    let line_end =
                        self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
                    let available = line_end - char_idx;
                    let to_delete = count.min(available);

                    if to_delete > 0 && char_idx < self.buffer().len_chars() {
                        // Save deleted chars to register (characterwise)
                        let deleted_chars: String = self
                            .buffer()
                            .content
                            .slice(char_idx..char_idx + to_delete)
                            .chars()
                            .collect();
                        let reg = self.active_register();
                        self.set_register(reg, deleted_chars, false);
                        self.clear_selected_register();

                        self.start_undo_group();
                        self.delete_with_undo(char_idx, char_idx + to_delete);
                        self.finish_undo_group();
                        self.clamp_cursor_col();
                        *changed = true;

                        // Record for repeat
                        self.last_change = Some(Change {
                            op: ChangeOp::Delete,
                            text: String::new(),
                            count,
                            motion: Some(Motion::Right),
                        });
                    }
                }
            }
            Some('w') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_word_forward();
                }
            }
            Some('b') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_word_backward();
                }
            }
            Some('e') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_word_end();
                }
            }
            Some('f') => {
                self.pending_key = Some('f');
            }
            Some('F') => {
                self.pending_key = Some('F');
            }
            Some('t') => {
                self.pending_key = Some('t');
            }
            Some('T') => {
                self.pending_key = Some('T');
            }
            Some('r') => {
                self.pending_key = Some('r');
            }
            Some(';') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.repeat_find(false);
                }
            }
            Some(',') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.repeat_find(true);
                }
            }
            Some('{') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_paragraph_backward();
                }
            }
            Some('}') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_paragraph_forward();
                }
            }
            Some('d') => {
                // 'd' can be both operator (dw) and motion (dd)
                // Set as pending_operator first
                self.pending_operator = Some('d');
            }
            Some('D') => {
                let count = self.take_count();
                self.start_undo_group();
                // D with count deletes from cursor to end of line, then (count-1) full lines below
                self.delete_to_end_of_line_with_count(count, changed);
                self.finish_undo_group();
            }
            Some('c') => {
                // 'c' operator (change) - delete then enter insert mode
                self.pending_operator = Some('c');
            }
            Some('C') => {
                // C: delete from cursor to end of line, enter insert mode
                let count = self.take_count();
                self.start_undo_group();
                self.delete_to_end_of_line_with_count(count, changed);
                self.insert_text_buffer.clear();
                self.mode = Mode::Insert;
                self.count = None;
                // Don't finish_undo_group here - let insert mode do it
            }
            Some('s') => {
                // s: substitute char (delete char under cursor, enter insert mode)
                let count = self.take_count();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let max_col = self.get_max_cursor_col(line);
                if max_col > 0 || self.buffer().line_len_chars(line) > 0 {
                    let char_idx = self.buffer().line_to_char(line) + col;
                    let line_end =
                        self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
                    let available = line_end - char_idx;
                    let to_delete = count.min(available);

                    if to_delete > 0 && char_idx < self.buffer().len_chars() {
                        let deleted_chars: String = self
                            .buffer()
                            .content
                            .slice(char_idx..char_idx + to_delete)
                            .chars()
                            .collect();
                        let reg = self.active_register();
                        self.set_register(reg, deleted_chars, false);
                        self.clear_selected_register();

                        self.start_undo_group();
                        self.delete_with_undo(char_idx, char_idx + to_delete);
                        *changed = true;
                    } else {
                        self.start_undo_group();
                    }
                } else {
                    self.start_undo_group();
                }
                self.insert_text_buffer.clear();
                self.mode = Mode::Insert;
                self.count = None;
            }
            Some('S') => {
                // S: substitute line (delete entire line content, enter insert mode)
                let count = self.take_count();
                let start_line = self.view().cursor.line;
                let _end_line = (start_line + count).min(self.buffer().len_lines());

                self.start_undo_group();

                // Delete content of lines but keep one line structure
                for i in 0..count {
                    let line_idx = start_line + i;
                    if line_idx >= self.buffer().len_lines() {
                        break;
                    }

                    let line_start = self.buffer().line_to_char(line_idx);
                    let line_len = self.buffer().line_len_chars(line_idx);
                    let line_content = self.buffer().content.line(line_idx);

                    // Calculate what to delete (exclude trailing newline)
                    let delete_end = if line_content.chars().last() == Some('\n') && line_len > 0 {
                        line_start + line_len - 1
                    } else {
                        line_start + line_len
                    };

                    if line_start < delete_end {
                        let deleted: String = self
                            .buffer()
                            .content
                            .slice(line_start..delete_end)
                            .chars()
                            .collect();
                        let reg = self.active_register();
                        self.set_register(reg, deleted, false);
                        self.clear_selected_register();

                        self.delete_with_undo(line_start, delete_end);
                        *changed = true;
                        break; // After first deletion, line indices change
                    }
                }

                self.view_mut().cursor.col = 0;
                self.insert_text_buffer.clear();
                self.mode = Mode::Insert;
                self.count = None;
            }
            Some('g') => {
                self.pending_key = Some('g');
            }
            Some('z') => {
                // Fold commands: za, zo, zc, zR
                self.pending_key = Some('z');
                self.message = "z: a=toggle  c=close  o=open  R=open all".to_string();
            }
            Some('m') => {
                // Set mark: m{a-z}
                self.pending_key = Some('m');
            }
            Some('\'') => {
                // Jump to mark line: '{a-z}
                self.pending_key = Some('\'');
            }
            Some('`') => {
                // Jump to exact mark position: `{a-z}
                self.pending_key = Some('`');
            }
            Some('G') => {
                if self.peek_count().is_some() {
                    // Count provided: go to line N (1-indexed)
                    let count = self.take_count();
                    let target_line = (count - 1).min(self.buffer().len_lines().saturating_sub(1));
                    self.view_mut().cursor.line = target_line;
                } else {
                    // No count: go to last line
                    let last = self.buffer().len_lines().saturating_sub(1);
                    self.view_mut().cursor.line = last;
                }
                self.clamp_cursor_col();
            }
            Some('u') => {
                self.undo();
            }
            Some('U') => {
                *changed = self.undo_line();
            }
            Some('.') => {
                // Repeat last change
                let count = self.take_count();
                self.repeat_last_change(count, changed);
            }
            Some('y') => {
                self.pending_key = Some('y');
            }
            Some('Y') => {
                let count = self.take_count();
                self.yank_lines(count);
            }
            Some('p') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.paste_after(changed);
                }
            }
            Some('P') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.paste_before(changed);
                }
            }
            Some('q') => {
                // If already recording, stop recording
                if self.macro_recording.is_some() {
                    self.stop_macro_recording();
                    return EngineAction::None;
                }

                // Otherwise, start pending key for register selection
                self.pending_key = Some('q');
            }
            Some('@') => {
                // Start pending key for register selection (@ + register)
                // @@ is handled in handle_pending_key
                self.pending_key = Some('@');
            }
            Some('"') => {
                self.pending_key = Some('"');
            }
            Some('n') => {
                let count = self.take_count();
                for _ in 0..count {
                    match self.search_direction {
                        SearchDirection::Forward => self.search_next(),
                        SearchDirection::Backward => self.search_prev(),
                    }
                }
            }
            Some('N') => {
                let count = self.take_count();
                for _ in 0..count {
                    match self.search_direction {
                        SearchDirection::Forward => self.search_prev(),
                        SearchDirection::Backward => self.search_next(),
                    }
                }
            }
            Some('v') => {
                self.mode = Mode::Visual;
                self.visual_anchor = Some(self.view().cursor);
            }
            Some('V') => {
                self.mode = Mode::VisualLine;
                self.visual_anchor = Some(self.view().cursor);
            }
            Some('%') => {
                self.move_to_matching_bracket();
            }
            Some(':') => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
                self.count = None; // Clear count when entering command mode
            }
            Some('/') => {
                self.mode = Mode::Search;
                self.command_buffer.clear();
                self.search_direction = SearchDirection::Forward;
                self.search_start_cursor = Some(self.view().cursor);
                self.count = None; // Clear count when entering search mode
            }
            Some('?') => {
                self.mode = Mode::Search;
                self.command_buffer.clear();
                self.search_direction = SearchDirection::Backward;
                self.search_start_cursor = Some(self.view().cursor);
                self.count = None; // Clear count when entering search mode
            }
            _ => match key_name {
                "Escape" => {
                    // Clear count and pending key in normal mode
                    self.count = None;
                    self.pending_key = None;
                }
                "Left" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_left();
                    }
                }
                "Down" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_down();
                    }
                }
                "Up" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_up();
                    }
                }
                "Right" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_right();
                    }
                }
                "Home" => self.view_mut().cursor.col = 0,
                "End" => {
                    let line = self.view().cursor.line;
                    self.view_mut().cursor.col = self.get_max_cursor_col(line);
                }
                _ => {}
            },
        }
        EngineAction::None
    }

    fn handle_pending_key(
        &mut self,
        pending: char,
        key_name: &str,
        unicode: Option<char>,
        changed: &mut bool,
    ) -> EngineAction {
        match pending {
            'g' => match unicode {
                Some('g') => {
                    if self.peek_count().is_some() {
                        // Count provided: go to line N (1-indexed)
                        let count = self.take_count();
                        let target_line =
                            (count - 1).min(self.buffer().len_lines().saturating_sub(1));
                        self.view_mut().cursor.line = target_line;
                    } else {
                        // No count: go to first line
                        self.view_mut().cursor.line = 0;
                    }
                    self.view_mut().cursor.col = 0;
                }
                Some('e') => {
                    // ge: backward to end of word
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_word_end_backward();
                    }
                }
                Some('t') => {
                    self.next_tab();
                }
                Some('T') => {
                    self.prev_tab();
                }
                _ => {}
            },
            'd' => {
                // This should not be reached - 'd' is now handled as pending_operator
                // But keep for backward compatibility during transition
                if unicode == Some('d') {
                    let count = self.take_count();
                    self.start_undo_group();
                    self.delete_lines(count, changed);
                    self.finish_undo_group();
                }
            }
            'y' => {
                if unicode == Some('y') {
                    let count = self.take_count();
                    self.yank_lines(count);
                } else if unicode == Some('i') || unicode == Some('a') {
                    // Text object yank: yi", ya(, etc.
                    self.pending_text_object = unicode;
                    self.pending_operator = Some('y'); // Set y as the operator
                } else {
                    // Invalid - clear pending
                }
            }
            '"' => {
                // Register selection: "x sets selected_register for next operation
                if let Some(ch) = unicode {
                    if ch.is_ascii_lowercase() || ch == '"' {
                        self.selected_register = Some(ch);
                    }
                }
            }
            'q' => {
                // Macro recording: q<register>
                if let Some(ch) = unicode {
                    if ch.is_ascii_lowercase() {
                        self.start_macro_recording(ch);
                    } else {
                        self.message = "Invalid register for macro".to_string();
                    }
                }
            }
            '@' => {
                // Macro playback: @<register> or @@
                if let Some(ch) = unicode {
                    if ch == '@' {
                        // @@ - repeat last macro
                        if let Some(last_reg) = self.last_macro_register {
                            let count = self.take_count();
                            let _ = self.play_macro_with_count(last_reg, count);
                        } else {
                            self.message = "No previous macro".to_string();
                        }
                    } else if ch.is_ascii_lowercase() {
                        let count = self.take_count();
                        let _ = self.play_macro_with_count(ch, count);
                    } else {
                        self.message = "Invalid register for macro playback".to_string();
                    }
                }
            }
            'f' | 'F' | 't' | 'T' => {
                // Character find motions
                if let Some(target) = unicode {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.find_char(pending, target);
                    }
                    // Remember this find for ; and , repeat
                    self.last_find = Some((pending, target));
                }
            }
            'r' => {
                // Replace character: r followed by a character replaces char under cursor
                if let Some(replacement) = unicode {
                    let count = self.take_count();
                    self.start_undo_group();
                    self.replace_chars(replacement, count, changed);
                    self.finish_undo_group();

                    // Record for repeat (.)
                    self.last_change = Some(Change {
                        op: ChangeOp::Replace,
                        text: replacement.to_string(),
                        count,
                        motion: None,
                    });
                }
            }
            '\x17' => {
                // Ctrl-W prefix
                match unicode {
                    Some('h') | Some('H') => {
                        self.focus_window_direction(SplitDirection::Vertical, false)
                    }
                    Some('j') | Some('J') => {
                        self.focus_window_direction(SplitDirection::Horizontal, true)
                    }
                    Some('k') | Some('K') => {
                        self.focus_window_direction(SplitDirection::Horizontal, false)
                    }
                    Some('l') | Some('L') => {
                        self.focus_window_direction(SplitDirection::Vertical, true)
                    }
                    Some('w') | Some('W') => self.focus_next_window(),
                    Some('c') | Some('C') => {
                        self.close_window();
                    }
                    Some('o') | Some('O') => self.close_other_windows(),
                    Some('s') | Some('S') => self.split_window(SplitDirection::Horizontal, None),
                    Some('v') | Some('V') => self.split_window(SplitDirection::Vertical, None),
                    _ => {
                        // Also handle by key_name for special keys
                        match key_name {
                            "Left" => self.focus_window_direction(SplitDirection::Vertical, false),
                            "Down" => self.focus_window_direction(SplitDirection::Horizontal, true),
                            "Up" => self.focus_window_direction(SplitDirection::Horizontal, false),
                            "Right" => self.focus_window_direction(SplitDirection::Vertical, true),
                            _ => {}
                        }
                    }
                }
            }
            'm' => {
                // Set mark: m{a-z}
                if let Some(ch) = unicode {
                    if ch.is_ascii_lowercase() {
                        let buffer_id = self.active_window().buffer_id;
                        let cursor = self.view().cursor;
                        self.marks.entry(buffer_id).or_default().insert(ch, cursor);
                        self.message = format!("Mark '{}' set", ch);
                    } else {
                        self.message = "Only lowercase marks (a-z) are supported".to_string();
                    }
                }
            }
            '\'' => {
                // Jump to mark line: '{a-z}
                if let Some(ch) = unicode {
                    if ch.is_ascii_lowercase() {
                        let buffer_id = self.active_window().buffer_id;
                        if let Some(buffer_marks) = self.marks.get(&buffer_id) {
                            if let Some(mark_cursor) = buffer_marks.get(&ch) {
                                self.view_mut().cursor.line = mark_cursor.line;
                                self.view_mut().cursor.col = 0;
                                self.clamp_cursor_col();
                            } else {
                                self.message = format!("Mark '{}' not set", ch);
                            }
                        } else {
                            self.message = format!("Mark '{}' not set", ch);
                        }
                    } else {
                        self.message = "Only lowercase marks (a-z) are supported".to_string();
                    }
                }
            }
            '`' => {
                // Jump to exact mark position: `{a-z}
                if let Some(ch) = unicode {
                    if ch.is_ascii_lowercase() {
                        let buffer_id = self.active_window().buffer_id;
                        if let Some(buffer_marks) = self.marks.get(&buffer_id) {
                            if let Some(mark_cursor) = buffer_marks.get(&ch) {
                                self.view_mut().cursor = *mark_cursor;
                                self.clamp_cursor_col();
                            } else {
                                self.message = format!("Mark `{}` not set", ch);
                            }
                        } else {
                            self.message = format!("Mark `{}` not set", ch);
                        }
                    } else {
                        self.message = "Only lowercase marks (a-z) are supported".to_string();
                    }
                }
            }
            'z' => {
                // Fold commands
                match unicode {
                    Some('a') => self.cmd_fold_toggle(),
                    Some('o') => self.cmd_fold_open(),
                    Some('c') => self.cmd_fold_close(),
                    Some('R') => self.view_mut().open_all_folds(),
                    _ => {}
                }
            }
            _ => {}
        }
        EngineAction::None
    }

    fn handle_operator_motion(
        &mut self,
        operator: char,
        _key_name: &str,
        unicode: Option<char>,
        changed: &mut bool,
    ) -> EngineAction {
        // Check if we're waiting for a text object type (after 'i' or 'a')
        if let Some(modifier) = self.pending_text_object.take() {
            if let Some(obj_type) = unicode {
                self.apply_operator_text_object(operator, modifier, obj_type, changed);
            }
            return EngineAction::None;
        }

        // Check if the next character is a text object modifier ('i' or 'a')
        if unicode == Some('i') || unicode == Some('a') {
            self.pending_text_object = unicode;
            self.pending_operator = Some(operator); // Put the operator back!
            return EngineAction::None;
        }

        // Handle operator + motion combinations (dw, cw, db, cb, de, ce, etc.)
        match unicode {
            Some('d') if operator == 'd' => {
                // dd: delete line
                let count = self.take_count();
                self.start_undo_group();
                self.delete_lines(count, changed);
                self.finish_undo_group();

                // Record for repeat
                self.last_change = Some(Change {
                    op: ChangeOp::Delete,
                    text: String::new(),
                    count,
                    motion: Some(Motion::DeleteLine),
                });
            }
            Some('c') if operator == 'c' => {
                // cc: change line (like S)
                let count = self.take_count();
                let start_line = self.view().cursor.line;

                self.start_undo_group();

                // Delete content of lines
                for i in 0..count {
                    let line_idx = start_line + i;
                    if line_idx >= self.buffer().len_lines() {
                        break;
                    }

                    let line_start = self.buffer().line_to_char(line_idx);
                    let line_len = self.buffer().line_len_chars(line_idx);
                    let line_content = self.buffer().content.line(line_idx);

                    let delete_end = if line_content.chars().last() == Some('\n') && line_len > 0 {
                        line_start + line_len - 1
                    } else {
                        line_start + line_len
                    };

                    if line_start < delete_end {
                        let deleted: String = self
                            .buffer()
                            .content
                            .slice(line_start..delete_end)
                            .chars()
                            .collect();
                        let reg = self.active_register();
                        self.set_register(reg, deleted, false);
                        self.clear_selected_register();

                        self.delete_with_undo(line_start, delete_end);
                        *changed = true;
                        break;
                    }
                }

                self.view_mut().cursor.col = 0;
                self.insert_text_buffer.clear();
                self.mode = Mode::Insert;
                self.count = None;
            }
            Some('w') => {
                // dw/cw: delete/change to start of next word
                // Special case: cw behaves like ce (Vim compatibility)
                let count = self.take_count();
                if operator == 'c' {
                    self.apply_operator_with_motion(operator, 'e', count, changed);
                } else {
                    self.apply_operator_with_motion(operator, 'w', count, changed);
                }
            }
            Some('b') => {
                // db/cb: delete/change back to start of word
                let count = self.take_count();
                self.apply_operator_with_motion(operator, 'b', count, changed);
            }
            Some('e') => {
                // de/ce: delete/change to end of word
                let count = self.take_count();
                self.apply_operator_with_motion(operator, 'e', count, changed);
            }
            Some('%') => {
                // d%/c%: delete/change to matching bracket
                self.apply_operator_bracket_motion(operator, changed);
            }
            _ => {
                // Invalid motion - cancel operator
                self.count = None;
            }
        }
        EngineAction::None
    }

    fn apply_operator_with_motion(
        &mut self,
        operator: char,
        motion: char,
        count: usize,
        changed: &mut bool,
    ) {
        // Save cursor position
        let start_cursor = self.view().cursor;
        let start_pos = self.buffer().line_to_char(start_cursor.line) + start_cursor.col;

        // Execute motion to find end position
        for _ in 0..count {
            match motion {
                'w' => self.move_word_forward(),
                'b' => self.move_word_backward(),
                'e' => self.move_word_end(),
                _ => return,
            }
        }

        let end_cursor = self.view().cursor;
        let end_pos = self.buffer().line_to_char(end_cursor.line) + end_cursor.col;

        // Restore cursor to start position
        self.view_mut().cursor = start_cursor;

        // Determine range to delete
        let (delete_start, delete_end) = match start_pos.cmp(&end_pos) {
            std::cmp::Ordering::Less => {
                // Forward motion: delete from start to end (inclusive for 'e', exclusive for 'w')
                if motion == 'e' {
                    // 'e' moves to end of word, so include that character
                    (start_pos, (end_pos + 1).min(self.buffer().len_chars()))
                } else {
                    // 'w' moves to start of next word, already at correct position
                    (start_pos, end_pos)
                }
            }
            std::cmp::Ordering::Greater => {
                // Backward motion (db): delete from end to start
                (end_pos, start_pos)
            }
            std::cmp::Ordering::Equal => {
                // No movement
                return;
            }
        };

        if delete_start >= delete_end {
            return;
        }

        // Save deleted text to register
        let deleted_text: String = self
            .buffer()
            .content
            .slice(delete_start..delete_end)
            .chars()
            .collect();
        let reg = self.active_register();
        self.set_register(reg, deleted_text, false);
        self.clear_selected_register();

        // Perform deletion
        self.start_undo_group();
        self.delete_with_undo(delete_start, delete_end);

        // For backward motion, move cursor to start of deletion
        if start_pos > end_pos {
            self.view_mut().cursor = end_cursor;
        }

        *changed = true;

        // If operator is 'c', enter insert mode
        if operator == 'c' {
            self.mode = Mode::Insert;
            self.count = None;
            self.clamp_cursor_col_insert(); // Use insert-mode clamping
                                            // Don't finish_undo_group - let insert mode do it
        } else {
            self.clamp_cursor_col(); // Use normal-mode clamping
            self.finish_undo_group();
        }
    }

    fn apply_operator_bracket_motion(&mut self, operator: char, changed: &mut bool) {
        let start_line = self.view().cursor.line;
        let start_col = self.view().cursor.col;
        let start_pos = self.buffer().line_to_char(start_line) + start_col;

        if start_pos >= self.buffer().len_chars() {
            return;
        }

        let current_char = self.buffer().content.char(start_pos);

        // Find matching bracket and determine search parameters
        let (is_opening, open_char, close_char) = match current_char {
            '(' => (true, '(', ')'),
            ')' => (false, '(', ')'),
            '{' => (true, '{', '}'),
            '}' => (false, '{', '}'),
            '[' => (true, '[', ']'),
            ']' => (false, '[', ']'),
            _ => {
                // Not on a bracket - cancel operation
                return;
            }
        };

        // Find the matching bracket position
        if let Some(match_pos) =
            self.find_matching_bracket(start_pos, open_char, close_char, is_opening)
        {
            // Determine range to delete (inclusive of both brackets)
            let (delete_start, delete_end) = if is_opening {
                (start_pos, match_pos + 1)
            } else {
                (match_pos, start_pos + 1)
            };

            // Save deleted text to register
            let deleted_text: String = self
                .buffer()
                .content
                .slice(delete_start..delete_end)
                .chars()
                .collect();
            let reg = self.active_register();
            self.set_register(reg, deleted_text, false);
            self.clear_selected_register();

            // Perform deletion
            self.start_undo_group();
            self.delete_with_undo(delete_start, delete_end);

            // Move cursor to start of deletion
            let new_line = self.buffer().content.char_to_line(delete_start);
            let line_start = self.buffer().line_to_char(new_line);
            self.view_mut().cursor.line = new_line;
            self.view_mut().cursor.col = delete_start - line_start;

            self.clamp_cursor_col();
            *changed = true;

            // If operator is 'c', enter insert mode
            if operator == 'c' {
                self.mode = Mode::Insert;
                self.count = None;
                // Don't finish_undo_group - let insert mode do it
            } else {
                self.finish_undo_group();
            }
        }
    }

    fn handle_insert_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        changed: &mut bool,
    ) {
        // ── Ctrl-N / Ctrl-P: word completion ─────────────────────────────────
        if ctrl && (key_name == "n" || key_name == "p") {
            let next = key_name == "n";
            if self.completion_idx.is_none() {
                let (prefix, start_col) = self.completion_prefix_at_cursor();
                let candidates = self.word_completions_for_prefix(&prefix);
                if candidates.is_empty() {
                    self.message = "No completions".to_string();
                    return;
                }
                self.completion_start_col = start_col;
                self.completion_candidates = candidates;
                let idx = if next {
                    0
                } else {
                    self.completion_candidates.len() - 1
                };
                self.completion_idx = Some(idx);
                self.apply_completion_candidate(idx);
            } else {
                let len = self.completion_candidates.len();
                let cur = self.completion_idx.unwrap();
                let new_idx = if next {
                    (cur + 1) % len
                } else {
                    (cur + len - 1) % len
                };
                self.completion_idx = Some(new_idx);
                self.apply_completion_candidate(new_idx);
            }
            *changed = true;
            return;
        }

        // Clear completion state on any non-completion key.
        if self.completion_idx.is_some() {
            self.completion_candidates.clear();
            self.completion_idx = None;
        }

        match key_name {
            "Escape" => {
                self.finish_undo_group();
                // Record the insert operation for repeat
                if !self.insert_text_buffer.is_empty() {
                    self.last_change = Some(Change {
                        op: ChangeOp::Insert,
                        text: self.insert_text_buffer.clone(),
                        count: 1,
                        motion: None,
                    });
                }
                self.mode = Mode::Normal;
                self.clamp_cursor_col();
            }
            "BackSpace" => {
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;
                if col > 0 {
                    self.delete_with_undo(char_idx - 1, char_idx);
                    self.view_mut().cursor.col -= 1;
                    *changed = true;
                } else if line > 0 {
                    let prev_line_len = self.buffer().line_len_chars(line - 1);
                    let new_col = if prev_line_len > 0 {
                        prev_line_len - 1
                    } else {
                        0
                    };
                    self.delete_with_undo(char_idx - 1, char_idx);
                    self.view_mut().cursor.line -= 1;
                    self.view_mut().cursor.col = new_col;
                    *changed = true;
                }
            }
            "Delete" => {
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;
                if char_idx < self.buffer().len_chars() {
                    self.delete_with_undo(char_idx, char_idx + 1);
                    *changed = true;
                }
            }
            "Return" => {
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;
                let indent = if self.settings.auto_indent {
                    self.get_line_indent_str(line)
                } else {
                    String::new()
                };
                let indent_len = indent.len();
                let text = format!("\n{}", indent);
                self.insert_with_undo(char_idx, &text);
                self.insert_text_buffer.push('\n');
                self.view_mut().cursor.line += 1;
                self.view_mut().cursor.col = indent_len;
                *changed = true;
            }
            "Tab" => {
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;
                if self.settings.expand_tab {
                    let n = self.settings.tabstop as usize;
                    let spaces = " ".repeat(n);
                    self.insert_with_undo(char_idx, &spaces);
                    self.insert_text_buffer.push_str(&spaces);
                    self.view_mut().cursor.col += n;
                } else {
                    self.insert_with_undo(char_idx, "\t");
                    self.insert_text_buffer.push('\t');
                    self.view_mut().cursor.col += 1;
                }
                *changed = true;
            }
            "Left" => self.move_left(),
            "Right" => self.move_right_insert(),
            "Up" => {
                if self.view().cursor.line > 0 {
                    self.view_mut().cursor.line -= 1;
                    self.clamp_cursor_col_insert();
                }
            }
            "Down" => {
                let max_line = self.buffer().len_lines().saturating_sub(1);
                if self.view().cursor.line < max_line {
                    self.view_mut().cursor.line += 1;
                    self.clamp_cursor_col_insert();
                }
            }
            "Home" => self.view_mut().cursor.col = 0,
            "End" => {
                let line = self.view().cursor.line;
                self.view_mut().cursor.col = self.get_line_len_for_insert(line);
            }
            _ => {
                if let Some(ch) = unicode {
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let char_idx = self.buffer().line_to_char(line) + col;
                    let mut buf = [0u8; 4];
                    let s = ch.encode_utf8(&mut buf);
                    self.insert_with_undo(char_idx, s);
                    self.insert_text_buffer.push(ch);
                    self.view_mut().cursor.col += 1;
                    *changed = true;
                }
            }
        }
    }

    fn handle_command_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
    ) -> EngineAction {
        // --- Ctrl-R: activate / cycle reverse history search ---
        if ctrl && key_name == "r" {
            if self.session.command_history.is_empty() {
                return EngineAction::None;
            }
            if !self.history_search_active {
                // Enter history search: save current command buffer
                self.history_search_active = true;
                self.history_search_query = String::new();
                self.history_search_index = None;
                self.command_typing_buffer = self.command_buffer.clone();
            }
            // Find next (older) match from current index
            self.history_search_step(true);
            return EngineAction::None;
        }

        // --- Ctrl-G: cancel history search ---
        if ctrl && key_name == "g" && self.history_search_active {
            self.history_search_active = false;
            self.history_search_query.clear();
            self.history_search_index = None;
            self.command_buffer = self.command_typing_buffer.clone();
            self.command_typing_buffer.clear();
            return EngineAction::None;
        }

        match key_name {
            "Escape" => {
                if self.history_search_active {
                    // Cancel history search, restore original buffer
                    self.history_search_active = false;
                    self.history_search_query.clear();
                    self.history_search_index = None;
                    self.command_buffer = self.command_typing_buffer.clone();
                    self.command_typing_buffer.clear();
                } else {
                    self.mode = Mode::Normal;
                    self.command_buffer.clear();
                    self.command_history_index = None;
                    self.command_typing_buffer.clear();
                }
                EngineAction::None
            }
            "Return" => {
                self.mode = Mode::Normal;
                // If in history search, the matched command is already in command_buffer
                self.history_search_active = false;
                self.history_search_query.clear();
                self.history_search_index = None;

                let cmd = self.command_buffer.clone();
                self.command_buffer.clear();
                self.session.add_command(&cmd);
                self.command_history_index = None;
                self.command_typing_buffer.clear();
                let _ = self.session.save();
                self.execute_command(&cmd)
            }
            "Up" => {
                // Exit history search first
                self.history_search_active = false;
                self.history_search_query.clear();
                self.history_search_index = None;

                if self.session.command_history.is_empty() {
                    return EngineAction::None;
                }
                if self.command_history_index.is_none() {
                    self.command_typing_buffer = self.command_buffer.clone();
                    self.command_history_index = Some(self.session.command_history.len() - 1);
                } else if let Some(idx) = self.command_history_index {
                    if idx > 0 {
                        self.command_history_index = Some(idx - 1);
                    }
                }
                if let Some(idx) = self.command_history_index {
                    if let Some(cmd) = self.session.command_history.get(idx) {
                        self.command_buffer = cmd.clone();
                    }
                }
                EngineAction::None
            }
            "Down" => {
                // Exit history search first
                self.history_search_active = false;
                self.history_search_query.clear();
                self.history_search_index = None;

                if self.command_history_index.is_none() {
                    return EngineAction::None;
                }
                let idx = self.command_history_index.unwrap();
                if idx + 1 >= self.session.command_history.len() {
                    self.command_buffer = self.command_typing_buffer.clone();
                    self.command_history_index = None;
                } else {
                    self.command_history_index = Some(idx + 1);
                    if let Some(cmd) = self.session.command_history.get(idx + 1) {
                        self.command_buffer = cmd.clone();
                    }
                }
                EngineAction::None
            }
            "Tab" => {
                // Exit history search, then complete
                self.history_search_active = false;
                self.history_search_query.clear();
                self.history_search_index = None;

                let completions = self.complete_command(&self.command_buffer);
                if completions.is_empty() {
                    return EngineAction::None;
                } else if completions.len() == 1 {
                    self.command_buffer = completions[0].clone();
                } else {
                    let common = Self::find_common_prefix(&completions);
                    if common.len() > self.command_buffer.len() {
                        self.command_buffer = common;
                    } else {
                        self.message = format!("Completions: {}", completions.join(", "));
                    }
                }
                EngineAction::None
            }
            "BackSpace" => {
                if self.history_search_active {
                    // Remove last char from search query and re-search
                    self.history_search_query.pop();
                    self.history_search_index = None; // restart from most recent
                    self.history_search_step(false);
                } else {
                    self.command_history_index = None;
                    self.command_typing_buffer.clear();
                    self.command_buffer.pop();
                    if self.command_buffer.is_empty() {
                        self.mode = Mode::Normal;
                    }
                }
                EngineAction::None
            }
            _ => {
                if self.history_search_active {
                    // Append char to search query and find match
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            self.history_search_query.push(ch);
                            self.history_search_index = None; // restart from most recent
                            self.history_search_step(false);
                        }
                    }
                } else {
                    self.command_history_index = None;
                    self.command_typing_buffer.clear();
                    if let Some(ch) = unicode {
                        self.command_buffer.push(ch);
                    }
                }
                EngineAction::None
            }
        }
    }

    /// Find a history match for the current `history_search_query`.
    /// If `next` is true, start searching one step older than `history_search_index`.
    /// Updates `command_buffer` with the match, or shows "no match" message.
    fn history_search_step(&mut self, next: bool) {
        let query = self.history_search_query.clone();
        let history = &self.session.command_history;
        if history.is_empty() {
            return;
        }

        // Determine start index: search from end (most recent) backwards
        let start = if next {
            // Step one older than current match
            match self.history_search_index {
                Some(0) => {
                    self.message = "(reverse-i-search): no more matches".to_string();
                    return;
                }
                Some(idx) => idx - 1,
                None => history.len() - 1,
            }
        } else {
            history.len() - 1
        };

        // Search backwards from start
        let found = (0..=start)
            .rev()
            .find(|&i| history[i].contains(query.as_str()));

        match found {
            Some(idx) => {
                self.history_search_index = Some(idx);
                self.command_buffer = history[idx].clone();
                self.message.clear();
            }
            None => {
                self.message = format!("(reverse-i-search): no match for '{}'", query);
            }
        }
    }

    fn handle_search_key(&mut self, key_name: &str, unicode: Option<char>) {
        match key_name {
            "Escape" => {
                self.mode = Mode::Normal;
                self.command_buffer.clear();
                self.search_history_index = None;
                self.search_typing_buffer.clear();

                // Restore cursor to original position (incremental search)
                if let Some(start_cursor) = self.search_start_cursor.take() {
                    self.view_mut().cursor = start_cursor;
                    // Clear search matches and query
                    self.search_matches.clear();
                    self.search_index = None;
                    self.search_query.clear();
                }
            }
            "Return" => {
                self.mode = Mode::Normal;
                let query = self.command_buffer.clone();
                self.command_buffer.clear();
                self.search_start_cursor = None; // Clear saved cursor position

                // Add to search history
                if !query.is_empty() {
                    self.session.add_search(&query);
                    self.search_history_index = None;
                    self.search_typing_buffer.clear();

                    // Save session state
                    let _ = self.session.save();

                    self.search_query = query;
                    self.run_search();
                    // If incremental search is enabled, cursor is already at the correct match
                    // Otherwise, jump to first match in the appropriate direction
                    if !self.settings.incremental_search {
                        match self.search_direction {
                            SearchDirection::Forward => self.search_next(),
                            SearchDirection::Backward => self.search_prev(),
                        }
                    }
                }
            }
            "Up" => {
                // Cycle to previous search
                if self.session.search_history.is_empty() {
                    return;
                }

                // First Up press: save current typing
                if self.search_history_index.is_none() {
                    self.search_typing_buffer = self.command_buffer.clone();
                    self.search_history_index = Some(self.session.search_history.len() - 1);
                } else if let Some(idx) = self.search_history_index {
                    if idx > 0 {
                        self.search_history_index = Some(idx - 1);
                    }
                }

                // Load history entry
                if let Some(idx) = self.search_history_index {
                    if let Some(query) = self.session.search_history.get(idx) {
                        self.command_buffer = query.clone();
                    }
                }
            }
            "Down" => {
                // Cycle to next search (or back to typing buffer)
                if self.search_history_index.is_none() {
                    return;
                }

                let idx = self.search_history_index.unwrap();
                if idx + 1 >= self.session.search_history.len() {
                    // Reached end, restore typing buffer
                    self.command_buffer = self.search_typing_buffer.clone();
                    self.search_history_index = None;
                } else {
                    self.search_history_index = Some(idx + 1);
                    if let Some(query) = self.session.search_history.get(idx + 1) {
                        self.command_buffer = query.clone();
                    }
                }
            }
            "BackSpace" => {
                // Reset history navigation when editing
                self.search_history_index = None;
                self.search_typing_buffer.clear();

                self.command_buffer.pop();
                if self.command_buffer.is_empty() {
                    self.mode = Mode::Normal;
                    // Restore cursor to original position
                    if let Some(start_cursor) = self.search_start_cursor.take() {
                        self.view_mut().cursor = start_cursor;
                        self.search_matches.clear();
                        self.search_index = None;
                        self.search_query.clear();
                    }
                } else if self.settings.incremental_search {
                    // Incremental search: update search as user types
                    self.perform_incremental_search();
                }
            }
            _ => {
                // Reset history navigation when typing
                self.search_history_index = None;
                self.search_typing_buffer.clear();

                if let Some(ch) = unicode {
                    self.command_buffer.push(ch);
                    // Incremental search: update search as user types
                    if self.settings.incremental_search {
                        self.perform_incremental_search();
                    }
                }
            }
        }
    }

    fn handle_visual_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        changed: &mut bool,
    ) -> EngineAction {
        // Handle Escape to exit visual mode
        if key_name == "Escape" {
            self.mode = Mode::Normal;
            self.visual_anchor = None;
            self.count = None; // Clear count on mode exit
            return EngineAction::None;
        }

        // Handle digit accumulation for count (same logic as normal mode)
        if let Some(ch) = unicode {
            if ch.is_ascii_digit() {
                let digit = ch.to_digit(10).unwrap() as usize;
                // Special case: '0' alone should NOT start count accumulation (reserved for column 0)
                // But '0' after a digit (like "10") should accumulate
                if digit == 0 && self.count.is_none() {
                    // Let '0' be handled as a motion command (go to column 0)
                } else {
                    // Accumulate digit into count
                    let current = self.count.unwrap_or(0);
                    let new_count = current * 10 + digit;
                    if new_count > 10000 {
                        self.message = "Count limited to 10,000".to_string();
                        self.count = Some(10000);
                    } else {
                        self.count = Some(new_count);
                    }
                    return EngineAction::None;
                }
            }
        }

        // Handle Ctrl-V for visual block mode switching
        if ctrl && key_name == "v" {
            if self.mode == Mode::VisualBlock {
                // Exit to normal mode
                self.mode = Mode::Normal;
                self.visual_anchor = None;
                self.count = None;
            } else {
                // Switch to VisualBlock mode, preserve anchor
                self.mode = Mode::VisualBlock;
            }
            return EngineAction::None;
        }

        // Handle mode switching: v toggles to Visual, V toggles to VisualLine
        if let Some(ch) = unicode {
            match ch {
                'v' => {
                    if self.mode == Mode::Visual {
                        // Exit to normal mode
                        self.mode = Mode::Normal;
                        self.visual_anchor = None;
                        self.count = None; // Clear count on mode exit
                    } else {
                        // Switch to Visual mode, preserve anchor
                        self.mode = Mode::Visual;
                    }
                    return EngineAction::None;
                }
                'V' => {
                    if self.mode == Mode::VisualLine {
                        // Exit to normal mode
                        self.mode = Mode::Normal;
                        self.visual_anchor = None;
                        self.count = None; // Clear count on mode exit
                    } else {
                        // Switch to VisualLine mode, preserve anchor
                        self.mode = Mode::VisualLine;
                    }
                    return EngineAction::None;
                }
                _ => {}
            }
        }

        // Handle text objects (iw, aw, i", a(, etc.) - set pending key
        if let Some(ch) = unicode {
            if ch == 'i' || ch == 'a' {
                self.pending_key = Some(ch);
                return EngineAction::None;
            }
        }

        // Handle operators: d (delete), y (yank), c (change), u (lowercase), U (uppercase)
        // Note: count is NOT applied to visual operators - they operate on the selection
        if let Some(ch) = unicode {
            match ch {
                'd' => {
                    self.count = None; // Clear count (not used for visual operators)
                    self.delete_visual_selection(changed);
                    return EngineAction::None;
                }
                'y' => {
                    self.count = None; // Clear count (not used for visual operators)
                    self.yank_visual_selection();
                    return EngineAction::None;
                }
                'c' => {
                    self.count = None; // Clear count (not used for visual operators)
                    self.change_visual_selection(changed);
                    return EngineAction::None;
                }
                'u' => {
                    self.count = None; // Clear count (not used for visual operators)
                    self.lowercase_visual_selection(changed);
                    return EngineAction::None;
                }
                'U' => {
                    self.count = None; // Clear count (not used for visual operators)
                    self.uppercase_visual_selection(changed);
                    return EngineAction::None;
                }
                ':' => {
                    self.mode = Mode::Command;
                    self.command_buffer = "'<,'>".to_string(); // Auto-populate visual range
                    self.count = None;
                    return EngineAction::None;
                }
                _ => {}
            }
        }

        // Handle navigation keys (extend selection)
        // These use the same movement logic as normal mode
        if ctrl {
            match key_name {
                "d" => {
                    let count = self.take_count();
                    let half = self.viewport_lines() / 2;
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    self.view_mut().cursor.line =
                        (self.view().cursor.line + half * count).min(max_line);
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "u" => {
                    let count = self.take_count();
                    let half = self.viewport_lines() / 2;
                    self.view_mut().cursor.line =
                        self.view().cursor.line.saturating_sub(half * count);
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "f" => {
                    let count = self.take_count();
                    let viewport = self.viewport_lines();
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    self.view_mut().cursor.line =
                        (self.view().cursor.line + viewport * count).min(max_line);
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "b" => {
                    let count = self.take_count();
                    let viewport = self.viewport_lines();
                    self.view_mut().cursor.line =
                        self.view().cursor.line.saturating_sub(viewport * count);
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                _ => {}
            }
        }

        // Handle multi-key sequences (gg, {, }, text objects)
        if let Some(pending) = self.pending_key.take() {
            if pending == 'i' || pending == 'a' {
                // Text object selection
                if let Some(obj_type) = unicode {
                    let cursor = self.view().cursor;
                    let cursor_pos = self.buffer().line_to_char(cursor.line) + cursor.col;

                    if let Some((start_pos, end_pos)) =
                        self.find_text_object_range(pending, obj_type, cursor_pos)
                    {
                        // Set visual selection to the text object range
                        let start_line = self.buffer().content.char_to_line(start_pos);
                        let start_line_char = self.buffer().line_to_char(start_line);
                        let start_col = start_pos - start_line_char;

                        let end_line = self
                            .buffer()
                            .content
                            .char_to_line(end_pos.saturating_sub(1).max(start_pos));
                        let end_line_char = self.buffer().line_to_char(end_line);
                        let end_col = (end_pos - 1).saturating_sub(end_line_char);

                        self.visual_anchor = Some(Cursor {
                            line: start_line,
                            col: start_col,
                        });
                        self.view_mut().cursor.line = end_line;
                        self.view_mut().cursor.col = end_col;

                        // Switch to character visual mode for text objects
                        self.mode = Mode::Visual;
                    }
                }
                return EngineAction::None;
            } else if pending == 'g' && unicode == Some('g') {
                // gg in visual mode: with count, go to line N; without count, go to first line
                if let Some(count) = self.peek_count() {
                    self.count = None; // Consume count
                    let target_line = count.saturating_sub(1); // 1-indexed to 0-indexed
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    self.view_mut().cursor.line = target_line.min(max_line);
                } else {
                    self.view_mut().cursor.line = 0;
                }
                self.view_mut().cursor.col = 0;
                return EngineAction::None;
            }
        }

        // Single-key navigation
        match unicode {
            Some('h') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_left();
                }
            }
            Some('j') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_down();
                }
            }
            Some('k') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_up();
                }
            }
            Some('l') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_right();
                }
            }
            Some('w') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_word_forward();
                }
            }
            Some('b') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_word_backward();
                }
            }
            Some('e') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_word_end();
                }
            }
            Some('0') => self.view_mut().cursor.col = 0,
            Some('$') => {
                let line = self.view().cursor.line;
                self.view_mut().cursor.col = self.get_max_cursor_col(line);
            }
            Some('g') => {
                self.pending_key = Some('g');
            }
            Some('G') => {
                let last_line = self.buffer().len_lines().saturating_sub(1);
                self.view_mut().cursor.line = last_line;
                self.clamp_cursor_col();
            }
            Some('{') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_paragraph_backward();
                }
            }
            Some('}') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_paragraph_forward();
                }
            }
            _ => match key_name {
                "Left" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_left();
                    }
                }
                "Down" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_down();
                    }
                }
                "Up" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_up();
                    }
                }
                "Right" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_right();
                    }
                }
                "Home" => self.view_mut().cursor.col = 0,
                "End" => {
                    let line = self.view().cursor.line;
                    self.view_mut().cursor.col = self.get_max_cursor_col(line);
                }
                _ => {}
            },
        }

        EngineAction::None
    }

    // =======================================================================
    // Visual mode helpers
    // =======================================================================

    /// Get normalized visual selection range (start, end).
    /// Start is always before or equal to end.
    fn get_visual_selection_range(&self) -> Option<(Cursor, Cursor)> {
        let anchor = self.visual_anchor?;
        let cursor = self.view().cursor;

        // Normalize so start <= end
        let (start, end) = if anchor.line < cursor.line
            || (anchor.line == cursor.line && anchor.col <= cursor.col)
        {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        };

        Some((start, end))
    }

    /// Extract the text from the visual selection.
    /// Returns (text, is_linewise).
    fn get_visual_selection_text(&self) -> Option<(String, bool)> {
        let (start, end) = self.get_visual_selection_range()?;

        match self.mode {
            Mode::VisualLine => {
                // Line mode: extract full lines from start.line to end.line (inclusive)
                let start_char = self.buffer().line_to_char(start.line);
                let end_line = end.line;
                let end_char = if end_line + 1 < self.buffer().len_lines() {
                    self.buffer().line_to_char(end_line + 1)
                } else {
                    self.buffer().len_chars()
                };

                let text = self
                    .buffer()
                    .content
                    .slice(start_char..end_char)
                    .to_string();

                // Ensure it ends with newline for linewise
                let text = if text.ends_with('\n') {
                    text
                } else {
                    format!("{}\n", text)
                };

                Some((text, true))
            }
            Mode::Visual => {
                // Character mode: extract from start to end (inclusive)
                let start_char = self.buffer().line_to_char(start.line) + start.col;
                let end_char = self.buffer().line_to_char(end.line) + end.col;

                // Include the character at the end position (Vim-like inclusive)
                let end_char_inclusive = (end_char + 1).min(self.buffer().len_chars());

                let text = self
                    .buffer()
                    .content
                    .slice(start_char..end_char_inclusive)
                    .to_string();

                Some((text, false))
            }
            Mode::VisualBlock => {
                // Block mode: extract rectangular region
                // Use anchor and cursor columns directly for block selection
                let anchor = self.visual_anchor?;
                let cursor = self.view().cursor;
                let start_col = anchor.col.min(cursor.col);
                let end_col = anchor.col.max(cursor.col);

                let mut lines = Vec::new();

                for line_idx in start.line..=end.line {
                    if let Some(line) = self.buffer().content.lines().nth(line_idx) {
                        let line_str = line.to_string();
                        let line_chars: Vec<char> = line_str.chars().collect();

                        // Extract the block portion of this line
                        let block_start = start_col.min(line_chars.len());
                        let block_end = (end_col + 1).min(line_chars.len());

                        let block_text: String = if block_start < line_chars.len() {
                            line_chars[block_start..block_end].iter().collect()
                        } else {
                            // Line is too short, just use empty string
                            String::new()
                        };

                        lines.push(block_text);
                    }
                }

                let text = lines.join("\n");
                Some((text, false))
            }
            _ => None,
        }
    }

    fn yank_visual_selection(&mut self) {
        if let Some((text, is_linewise)) = self.get_visual_selection_text() {
            // Store in selected register (or unnamed register)
            let reg = self.selected_register.unwrap_or('"');
            self.registers.insert(reg, (text.clone(), is_linewise));

            // Also store in unnamed register if we used a named one
            if reg != '"' {
                self.registers.insert('"', (text, is_linewise));
            }

            self.selected_register = None;
            self.message = format!("{} yanked", if is_linewise { "Line(s)" } else { "Text" });
        }

        // Exit visual mode
        self.mode = Mode::Normal;
        self.visual_anchor = None;
    }

    fn delete_visual_selection(&mut self, changed: &mut bool) {
        if let Some((text, is_linewise)) = self.get_visual_selection_text() {
            // Store in register
            let reg = self.selected_register.unwrap_or('"');
            self.registers.insert(reg, (text.clone(), is_linewise));
            if reg != '"' {
                self.registers.insert('"', (text, is_linewise));
            }
            self.selected_register = None;

            // Delete the selection
            let (start, end) = self.get_visual_selection_range().unwrap();

            self.start_undo_group();

            match self.mode {
                Mode::VisualLine => {
                    // Delete full lines
                    let start_char = self.buffer().line_to_char(start.line);
                    let end_char = if end.line + 1 < self.buffer().len_lines() {
                        self.buffer().line_to_char(end.line + 1)
                    } else {
                        self.buffer().len_chars()
                    };

                    self.delete_with_undo(start_char, end_char);

                    // Position cursor at start of line
                    self.view_mut().cursor.line = start.line;
                    self.view_mut().cursor.col = 0;
                }
                Mode::Visual => {
                    // Delete characters
                    let start_char = self.buffer().line_to_char(start.line) + start.col;
                    let end_char = self.buffer().line_to_char(end.line) + end.col + 1;

                    self.delete_with_undo(start_char, end_char.min(self.buffer().len_chars()));

                    // Position cursor at start
                    self.view_mut().cursor = start;
                }
                Mode::VisualBlock => {
                    // Delete rectangular block (work backwards to avoid offset issues)
                    // Use anchor and cursor columns directly for block selection
                    let anchor = self.visual_anchor.unwrap();
                    let cursor = self.view().cursor;
                    let start_col = anchor.col.min(cursor.col);
                    let end_col = anchor.col.max(cursor.col);

                    for line_idx in (start.line..=end.line).rev() {
                        let line_start_char = self.buffer().line_to_char(line_idx);
                        if let Some(line) = self.buffer().content.lines().nth(line_idx) {
                            let line_str = line.to_string();
                            let line_len = line_str.chars().count();

                            // Only delete if the line is long enough to have characters in the block
                            if start_col < line_len {
                                let block_end = (end_col + 1).min(line_len);
                                let del_start = line_start_char + start_col;
                                let del_end = line_start_char + block_end;
                                self.delete_with_undo(del_start, del_end);
                            }
                        }
                    }

                    // Position cursor at start of block
                    self.view_mut().cursor.line = start.line;
                    self.view_mut().cursor.col = start_col;
                }
                _ => {}
            }

            self.finish_undo_group();
            *changed = true;
            self.clamp_cursor_col();
        }

        // Exit visual mode
        self.mode = Mode::Normal;
        self.visual_anchor = None;
    }

    fn change_visual_selection(&mut self, changed: &mut bool) {
        // Change is like delete, but then enter insert mode
        self.delete_visual_selection(changed);

        // The delete already finished the undo group and set mode to Normal
        // Now start a new undo group for the insert mode typing
        self.start_undo_group();
        self.insert_text_buffer.clear();
        self.mode = Mode::Insert;
    }

    fn lowercase_visual_selection(&mut self, changed: &mut bool) {
        self.transform_visual_selection(|s| s.to_lowercase(), changed);
    }

    fn uppercase_visual_selection(&mut self, changed: &mut bool) {
        self.transform_visual_selection(|s| s.to_uppercase(), changed);
    }

    fn transform_visual_selection<F>(&mut self, transform: F, changed: &mut bool)
    where
        F: Fn(&str) -> String,
    {
        let (start, end) = match self.get_visual_selection_range() {
            Some(range) => range,
            None => return,
        };

        self.start_undo_group();

        match self.mode {
            Mode::VisualLine => {
                // Transform full lines
                for line_idx in start.line..=end.line {
                    if let Some(line) = self.buffer().content.lines().nth(line_idx) {
                        let line_str = line.to_string();
                        let transformed = transform(&line_str);

                        // Replace the line
                        let line_start_char = self.buffer().line_to_char(line_idx);
                        let line_end_char = line_start_char + line_str.chars().count();
                        self.delete_with_undo(line_start_char, line_end_char);
                        self.insert_with_undo(line_start_char, &transformed);
                    }
                }

                // Position cursor at start of first line
                self.view_mut().cursor.line = start.line;
                self.view_mut().cursor.col = 0;
            }
            Mode::Visual => {
                // Transform character selection
                if let Some((text, _)) = self.get_visual_selection_text() {
                    let transformed = transform(&text);

                    let start_char = self.buffer().line_to_char(start.line) + start.col;
                    let end_char = self.buffer().line_to_char(end.line) + end.col + 1;

                    self.delete_with_undo(start_char, end_char.min(self.buffer().len_chars()));
                    self.insert_with_undo(start_char, &transformed);

                    // Position cursor at start
                    self.view_mut().cursor = start;
                }
            }
            Mode::VisualBlock => {
                // Transform rectangular block (work backwards to maintain positions)
                let anchor = self.visual_anchor.unwrap();
                let cursor = self.view().cursor;
                let start_col = anchor.col.min(cursor.col);
                let end_col = anchor.col.max(cursor.col);

                for line_idx in (start.line..=end.line).rev() {
                    let line_start_char = self.buffer().line_to_char(line_idx);
                    if let Some(line) = self.buffer().content.lines().nth(line_idx) {
                        let line_str = line.to_string();
                        let line_chars: Vec<char> = line_str.chars().collect();

                        // Extract and transform the block portion
                        if start_col < line_chars.len() {
                            let block_end = (end_col + 1).min(line_chars.len());
                            let block_text: String =
                                line_chars[start_col..block_end].iter().collect();
                            let transformed = transform(&block_text);

                            let del_start = line_start_char + start_col;
                            let del_end = line_start_char + block_end;
                            self.delete_with_undo(del_start, del_end);
                            self.insert_with_undo(del_start, &transformed);
                        }
                    }
                }

                // Position cursor at start of block
                self.view_mut().cursor.line = start.line;
                self.view_mut().cursor.col = start_col;
            }
            _ => {}
        }

        self.finish_undo_group();
        *changed = true;
        self.clamp_cursor_col();

        // Exit visual mode
        self.mode = Mode::Normal;
        self.visual_anchor = None;
    }

    // =======================================================================
    // Repeat command (.)
    // =======================================================================

    fn repeat_last_change(&mut self, repeat_count: usize, changed: &mut bool) {
        let change = match &self.last_change {
            Some(c) => c.clone(),
            None => return, // No change to repeat
        };

        let final_count = if repeat_count > 1 {
            repeat_count
        } else {
            change.count
        };

        match change.op {
            ChangeOp::Insert => {
                // Repeat insert: insert the same text at current position
                self.start_undo_group();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;

                // Insert the text final_count times
                let repeated_text = change.text.repeat(final_count);
                self.insert_with_undo(char_idx, &repeated_text);

                // Update cursor position based on inserted text
                let newlines = repeated_text.matches('\n').count();
                if newlines > 0 {
                    self.view_mut().cursor.line += newlines;
                    // Find column after last newline
                    if let Some(last_nl) = repeated_text.rfind('\n') {
                        self.view_mut().cursor.col = repeated_text[last_nl + 1..].chars().count();
                    }
                } else {
                    self.view_mut().cursor.col += repeated_text.chars().count();
                }
                self.finish_undo_group();
                *changed = true;
            }
            ChangeOp::Delete => {
                // Repeat delete with motion
                if let Some(motion) = &change.motion {
                    for _ in 0..final_count {
                        self.start_undo_group();
                        match motion {
                            Motion::Right => {
                                // Delete character(s) at cursor (like x)
                                let line = self.view().cursor.line;
                                let col = self.view().cursor.col;
                                let char_idx = self.buffer().line_to_char(line) + col;
                                let line_end = self.buffer().line_to_char(line)
                                    + self.buffer().line_len_chars(line);
                                let available = line_end - char_idx;
                                let to_delete = change.count.min(available);

                                if to_delete > 0 && char_idx < self.buffer().len_chars() {
                                    let deleted_chars: String = self
                                        .buffer()
                                        .content
                                        .slice(char_idx..char_idx + to_delete)
                                        .chars()
                                        .collect();
                                    let reg = self.active_register();
                                    self.set_register(reg, deleted_chars, false);
                                    self.clear_selected_register();
                                    self.delete_with_undo(char_idx, char_idx + to_delete);
                                    self.clamp_cursor_col();
                                    *changed = true;
                                }
                            }
                            Motion::DeleteLine => {
                                // Repeat dd
                                self.delete_lines(change.count, changed);
                            }
                            _ => {}
                        }
                        self.finish_undo_group();
                    }
                }
            }
            ChangeOp::Change => {
                // Repeat change operation - for now just handle simple cases
                // More complex handling would go here
            }
            ChangeOp::Substitute => {
                // Repeat s command
                for _ in 0..final_count {
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let max_col = self.get_max_cursor_col(line);
                    if max_col > 0 || self.buffer().line_len_chars(line) > 0 {
                        let char_idx = self.buffer().line_to_char(line) + col;
                        let line_end =
                            self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
                        let available = line_end - char_idx;
                        let to_delete = change.count.min(available);

                        self.start_undo_group();
                        if to_delete > 0 && char_idx < self.buffer().len_chars() {
                            self.delete_with_undo(char_idx, char_idx + to_delete);
                            *changed = true;
                        }

                        // Insert the recorded text
                        if !change.text.is_empty() {
                            self.insert_with_undo(char_idx, &change.text);
                            *changed = true;
                        }
                        self.finish_undo_group();
                    }
                }
            }
            ChangeOp::SubstituteLine | ChangeOp::DeleteToEnd | ChangeOp::ChangeToEnd => {
                // Handle other operations
            }
            ChangeOp::Replace => {
                // Repeat r command
                if let Some(replacement_char) = change.text.chars().next() {
                    for _ in 0..final_count {
                        self.start_undo_group();
                        self.replace_chars(replacement_char, change.count, changed);
                        self.finish_undo_group();
                    }
                }
            }
        }
    }

    /// Available commands for auto-completion
    fn available_commands() -> &'static [&'static str] {
        &[
            "w",
            "q",
            "q!",
            "wq",
            "wq!",
            "wa",
            "qa",
            "qa!",
            "e ",
            "e!",
            "enew",
            "bn",
            "bp",
            "bd",
            "b#",
            "ls",
            "split",
            "vsplit",
            "tabnew",
            "tabnext",
            "tabprev",
            "tabclose",
            "s/",
            "%s/",
            "config reload",
        ]
    }

    /// Find completions for partial command
    fn complete_command(&self, partial: &str) -> Vec<String> {
        if partial.is_empty() {
            return Vec::new();
        }

        Self::available_commands()
            .iter()
            .filter(|cmd| cmd.starts_with(partial))
            .map(|s| s.to_string())
            .collect()
    }

    /// Find common prefix of strings
    fn find_common_prefix(strings: &[String]) -> String {
        if strings.is_empty() {
            return String::new();
        }

        let first = &strings[0];
        let mut common = String::new();

        for (i, ch) in first.chars().enumerate() {
            if strings.iter().all(|s| s.chars().nth(i) == Some(ch)) {
                common.push(ch);
            } else {
                break;
            }
        }

        common
    }

    fn execute_command(&mut self, cmd: &str) -> EngineAction {
        let cmd = cmd.trim();

        // Handle :Gdiff / :Gd
        if cmd == "Gdiff" || cmd == "Gd" {
            return self.cmd_git_diff();
        }

        // Handle :Gstatus / :Gs
        if cmd == "Gstatus" || cmd == "Gs" {
            return self.cmd_git_status();
        }

        // Handle :Gadd[!] — stage current file or all
        if cmd == "Gadd" || cmd == "Ga" {
            return self.cmd_git_add(false);
        }
        if cmd == "Gadd!" || cmd == "Ga!" {
            return self.cmd_git_add(true);
        }

        // Handle :Gcommit <message> / :Gc <message>
        if let Some(msg) = cmd
            .strip_prefix("Gcommit ")
            .or_else(|| cmd.strip_prefix("Gc "))
        {
            return self.cmd_git_commit(msg.trim());
        }
        if cmd == "Gcommit" || cmd == "Gc" {
            self.message = "Usage: Gcommit <message>".to_string();
            return EngineAction::Error;
        }

        // Handle :Gpush / :Gp
        if cmd == "Gpush" || cmd == "Gp" {
            return self.cmd_git_push();
        }

        // Handle :Gblame / :Gb
        if cmd == "Gblame" || cmd == "Gb" {
            return self.cmd_git_blame();
        }

        // Handle :e <filename>
        if let Some(filename) = cmd.strip_prefix("e ") {
            let filename = filename.trim();
            if filename.is_empty() {
                self.message = "No file name".to_string();
                return EngineAction::Error;
            }
            return EngineAction::OpenFile(PathBuf::from(filename));
        }

        // Handle :b <buffer>
        if let Some(arg) = cmd.strip_prefix("b ") {
            let arg = arg.trim();
            if let Ok(num) = arg.parse::<usize>() {
                self.goto_buffer(num);
            } else if let Some(id) = self.buffer_manager.find_by_path(arg) {
                let current = self.active_buffer_id();
                if id != current {
                    self.buffer_manager.alternate_buffer = Some(current);
                    self.switch_window_buffer(id);
                }
            } else {
                self.message = format!("No matching buffer for {}", arg);
            }
            return EngineAction::None;
        }

        // Handle :bd[!] [N]
        if cmd == "bd" || cmd.starts_with("bd ") || cmd == "bd!" || cmd.starts_with("bd! ") {
            let force = cmd.contains('!');
            let arg = cmd.trim_start_matches("bd").trim_start_matches('!').trim();

            let id = if arg.is_empty() {
                self.active_buffer_id()
            } else if let Ok(num) = arg.parse::<usize>() {
                if let Some(id) = self.buffer_manager.get_by_number(num) {
                    id
                } else {
                    self.message = format!("Buffer {} does not exist", num);
                    return EngineAction::Error;
                }
            } else {
                self.message = format!("Invalid buffer: {}", arg);
                return EngineAction::Error;
            };

            match self.delete_buffer(id, force) {
                Ok(()) => {
                    self.message = "Buffer deleted".to_string();
                }
                Err(e) => {
                    self.message = e;
                    return EngineAction::Error;
                }
            }
            return EngineAction::None;
        }

        // Handle :split / :sp [file]
        if cmd == "split" || cmd == "sp" || cmd.starts_with("split ") || cmd.starts_with("sp ") {
            let file = cmd
                .strip_prefix("split")
                .or_else(|| cmd.strip_prefix("sp"))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            self.split_window(SplitDirection::Horizontal, file.map(Path::new));
            return EngineAction::None;
        }

        // Handle :vsplit / :vsp [file]
        if cmd == "vsplit" || cmd == "vsp" || cmd.starts_with("vsplit ") || cmd.starts_with("vsp ")
        {
            let file = cmd
                .strip_prefix("vsplit")
                .or_else(|| cmd.strip_prefix("vsp"))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            self.split_window(SplitDirection::Vertical, file.map(Path::new));
            return EngineAction::None;
        }

        // Handle :close / :clo
        if cmd == "close" || cmd == "clo" {
            self.close_window();
            return EngineAction::None;
        }

        // Handle :only / :on
        if cmd == "only" || cmd == "on" {
            self.close_other_windows();
            return EngineAction::None;
        }

        // Handle :tabnew / :tabedit [file]
        if cmd == "tabnew"
            || cmd == "tabe"
            || cmd.starts_with("tabnew ")
            || cmd.starts_with("tabe ")
        {
            let file = cmd
                .strip_prefix("tabnew")
                .or_else(|| cmd.strip_prefix("tabe"))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            self.new_tab(file.map(Path::new));
            return EngineAction::None;
        }

        // Handle :tabclose / :tabc
        if cmd == "tabclose" || cmd == "tabc" {
            self.close_tab();
            return EngineAction::None;
        }

        // Handle :tabnext / :tabn
        if cmd == "tabnext" || cmd == "tabn" {
            self.next_tab();
            return EngineAction::None;
        }

        // Handle :tabprev / :tabp
        if cmd == "tabprev" || cmd == "tabp" || cmd == "tabprevious" {
            self.prev_tab();
            return EngineAction::None;
        }

        // Handle :set [option]
        if cmd == "set" {
            self.message = self.settings.display_all();
            return EngineAction::None;
        }
        if let Some(args) = cmd.strip_prefix("set ") {
            match self.settings.parse_set_option(args.trim()) {
                Ok(msg) => {
                    if let Err(e) = self.settings.save() {
                        self.message = format!("Setting changed but failed to save: {e}");
                    } else {
                        self.message = msg;
                    }
                }
                Err(e) => {
                    self.message = e;
                    return EngineAction::Error;
                }
            }
            return EngineAction::None;
        }

        // Handle :config reload
        if cmd == "config reload" {
            match Settings::load_with_validation() {
                Ok(new_settings) => {
                    self.settings = new_settings;
                    self.message = "Settings reloaded successfully".to_string();
                }
                Err(e) => {
                    // Preserve current settings on error
                    self.message = format!("Error reloading settings: {}", e);
                }
            }
            return EngineAction::None;
        }

        // Handle :ls / :buffers
        if cmd == "ls" || cmd == "buffers" {
            self.message = self.list_buffers();
            return EngineAction::None;
        }

        // Handle :bn / :bnext
        if cmd == "bn" || cmd == "bnext" {
            self.next_buffer();
            return EngineAction::None;
        }

        // Handle :bp / :bprev / :bprevious
        if cmd == "bp" || cmd == "bprev" || cmd == "bprevious" {
            self.prev_buffer();
            return EngineAction::None;
        }

        // Handle :b# (alternate buffer)
        if cmd == "b#" {
            self.alternate_buffer();
            return EngineAction::None;
        }

        // Substitute command: :s/pattern/replacement/flags or :%s/...
        if cmd.starts_with("s/") || cmd.starts_with("%s/") || cmd.starts_with("'<,'>s/") {
            return self.execute_substitute_command(cmd);
        }

        // Handle :N (jump to line number)
        if let Ok(line_num) = cmd.parse::<usize>() {
            let target = if line_num > 0 { line_num - 1 } else { 0 };
            let max = self.buffer().len_lines().saturating_sub(1);
            self.view_mut().cursor.line = target.min(max);
            self.view_mut().cursor.col = 0;
            self.clamp_cursor_col();
            return EngineAction::None;
        }

        match cmd {
            "w" => {
                let _ = self.save();
                EngineAction::None
            }
            "q" => {
                // Block if the current buffer has unsaved changes.
                if self.dirty() {
                    self.message = "No write since last change (add ! to override)".to_string();
                    return EngineAction::Error;
                }
                // If this is the very last window in the very last tab: quit the app.
                let is_last = self.tabs.len() == 1 && self.active_tab().layout.is_single_window();
                if is_last {
                    return EngineAction::Quit;
                }
                // Otherwise close the current window (and the tab if it's the last
                // window in it).  Drop the buffer if nothing else shows it so that
                // collect_session_open_files() (which filters by window-visible buffers)
                // correctly excludes explicitly-closed files from the next session.
                let buf_id = self.active_buffer_id();
                self.close_window();
                if !self.windows.values().any(|w| w.buffer_id == buf_id) {
                    let _ = self.buffer_manager.delete(buf_id, true);
                }
                EngineAction::None
            }
            "q!" => {
                // If this is the very last window in the very last tab: force-quit.
                let is_last = self.tabs.len() == 1 && self.active_tab().layout.is_single_window();
                if is_last {
                    return EngineAction::Quit;
                }
                // Force-close without checking dirty flag.
                let buf_id = self.active_buffer_id();
                self.close_window();
                if !self.windows.values().any(|w| w.buffer_id == buf_id) {
                    let _ = self.buffer_manager.delete(buf_id, true);
                }
                EngineAction::None
            }
            "qa" => {
                // Quit all: block if any buffer is dirty.
                let has_dirty = self
                    .buffer_manager
                    .list()
                    .iter()
                    .any(|id| self.buffer_manager.get(*id).map_or(false, |s| s.dirty));
                if has_dirty {
                    self.message = "No write since last change (add ! to override)".to_string();
                    EngineAction::Error
                } else {
                    EngineAction::Quit
                }
            }
            "qa!" => EngineAction::Quit,
            "wq" | "x" => {
                if self.save().is_ok() {
                    EngineAction::SaveQuit
                } else {
                    EngineAction::Error
                }
            }
            _ => {
                self.message = format!("Not an editor command: {}", cmd);
                EngineAction::Error
            }
        }
    }

    fn execute_substitute_command(&mut self, cmd: &str) -> EngineAction {
        // Parse: [range]s/pattern/replacement/[flags]
        // Supported ranges: none (current line), % (all lines), '<,'> (visual selection)

        // Determine if this is :%s (all lines) or :s (current line/visual selection)
        let (range_str, rest) = if cmd.starts_with("%s/") {
            ("%", &cmd[2..]) // Skip "%s"
        } else if cmd.starts_with("s/") {
            ("", &cmd[1..]) // Skip "s"
        } else if cmd.starts_with("'<,'>s/") {
            // Visual selection range (set when entering command mode from visual)
            ("'<,'>", &cmd[6..]) // Skip "'<,'>s"
        } else {
            self.message = "Invalid substitute command".to_string();
            return EngineAction::Error;
        };

        // Parse /pattern/replacement/flags
        // rest is like "/foo/baz/" or "/foo/baz/g"
        // Splitting by '/' gives: ["", "foo", "baz", ""] or ["", "foo", "baz", "g"]
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() < 3 {
            self.message = "Usage: :s/pattern/replacement/[flags]".to_string();
            return EngineAction::Error;
        }

        let pattern = parts[1];
        let replacement = parts.get(2).unwrap_or(&"");
        let flags = parts.get(3).unwrap_or(&"");

        // Determine line range
        let range = if range_str == "%" {
            // All lines
            let last = self.buffer().len_lines().saturating_sub(1);
            Some((0, last))
        } else if range_str == "'<,'>" {
            // Visual selection (if we have one)
            if let Some((start, end)) = self.get_visual_selection_range() {
                Some((start.line, end.line))
            } else {
                self.message = "No visual selection".to_string();
                return EngineAction::Error;
            }
        } else {
            // Current line only
            None
        };

        // Execute replacement
        match self.replace_in_range(range, pattern, replacement, flags) {
            Ok(count) => {
                self.message = format!(
                    "{} substitution{}",
                    count,
                    if count == 1 { "" } else { "s" }
                );
                EngineAction::None
            }
            Err(e) => {
                self.message = e;
                EngineAction::Error
            }
        }
    }

    // --- Search ---

    pub fn run_search(&mut self) {
        self.search_matches.clear();
        self.search_index = None;

        if self.search_query.is_empty() {
            return;
        }

        let text = self.buffer().to_string();
        let query = &self.search_query;
        let mut byte_pos = 0;
        while let Some(found) = text[byte_pos..].find(query) {
            let start_byte = byte_pos + found;
            let end_byte = start_byte + query.len();
            let start_char = self.buffer().content.byte_to_char(start_byte);
            let end_char = self.buffer().content.byte_to_char(end_byte);
            self.search_matches.push((start_char, end_char));
            byte_pos = start_byte + 1;
        }

        if self.search_matches.is_empty() {
            self.message = format!("Pattern not found: {}", self.search_query);
        }
    }

    pub fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            if !self.search_query.is_empty() {
                self.message = format!("Pattern not found: {}", self.search_query);
            }
            return;
        }

        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let cursor_char = self.buffer().line_to_char(line) + col;

        let next = self
            .search_matches
            .iter()
            .position(|(start, _)| *start > cursor_char);
        let idx = next.unwrap_or(0);

        self.search_index = Some(idx);
        self.jump_to_search_match(idx);
    }

    pub fn search_prev(&mut self) {
        if self.search_matches.is_empty() {
            if !self.search_query.is_empty() {
                self.message = format!("Pattern not found: {}", self.search_query);
            }
            return;
        }

        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let cursor_char = self.buffer().line_to_char(line) + col;

        let prev = self
            .search_matches
            .iter()
            .rposition(|(start, _)| *start < cursor_char);
        let idx = prev.unwrap_or(self.search_matches.len() - 1);

        self.search_index = Some(idx);
        self.jump_to_search_match(idx);
    }

    fn jump_to_search_match(&mut self, idx: usize) {
        if let Some(&(start_char, _)) = self.search_matches.get(idx) {
            let line = self.buffer().content.char_to_line(start_char);
            let line_start = self.buffer().line_to_char(line);
            let col = start_char - line_start;
            self.view_mut().cursor.line = line;
            self.view_mut().cursor.col = col;
            self.message = format!("match {} of {}", idx + 1, self.search_matches.len());
        }
    }

    /// Perform incremental search as user types
    fn perform_incremental_search(&mut self) {
        // Update search query from command buffer
        self.search_query = self.command_buffer.clone();

        if self.search_query.is_empty() {
            // Restore to start position if search is empty
            if let Some(start_cursor) = self.search_start_cursor {
                self.view_mut().cursor = start_cursor;
            }
            self.search_matches.clear();
            self.search_index = None;
            self.message.clear();
            return;
        }

        // Run the search
        self.run_search();

        // Jump to the first match from the start position
        if !self.search_matches.is_empty() {
            // Get the starting cursor position
            let start_cursor = self.search_start_cursor.unwrap_or(self.view().cursor);
            let start_char = self.buffer().line_to_char(start_cursor.line) + start_cursor.col;

            // Find the appropriate match based on search direction
            let idx = match self.search_direction {
                SearchDirection::Forward => {
                    // Find first match at or after start position
                    self.search_matches
                        .iter()
                        .position(|(start, _)| *start >= start_char)
                        .unwrap_or(0)
                }
                SearchDirection::Backward => {
                    // Find last match strictly before start position
                    self.search_matches
                        .iter()
                        .rposition(|(start, _)| *start < start_char)
                        .unwrap_or(self.search_matches.len() - 1)
                }
            };

            self.search_index = Some(idx);
            self.jump_to_search_match(idx);
        } else {
            // No matches, restore to start position
            if let Some(start_cursor) = self.search_start_cursor {
                self.view_mut().cursor = start_cursor;
            }
        }
    }

    // --- Find/Replace methods ---

    /// Replace text in a given range
    /// range: None = current line, Some((start_line, end_line)) = line range
    /// pattern: string to find (will use simple substring matching for now)
    /// replacement: string to replace with
    /// flags: "g" (all), "c" (confirm), "i" (case-insensitive)
    /// Returns: (num_replacements, modified_text_preview)
    pub fn replace_in_range(
        &mut self,
        range: Option<(usize, usize)>,
        pattern: &str,
        replacement: &str,
        flags: &str,
    ) -> Result<usize, String> {
        if pattern.is_empty() {
            return Err("Pattern cannot be empty".to_string());
        }

        let global = flags.contains('g');
        let _confirm = flags.contains('c'); // For Phase 2
        let case_insensitive = flags.contains('i');

        // Determine line range
        let (start_line, end_line) = match range {
            Some((s, e)) => (s, e),
            None => {
                let current = self.view().cursor.line;
                (current, current)
            }
        };

        let mut replacements = 0;
        self.start_undo_group();

        // Process each line in range
        for line_num in start_line..=end_line {
            if line_num >= self.buffer().len_lines() {
                break;
            }

            let line_start_char = self.buffer().line_to_char(line_num);
            let line_len = self.buffer().line_len_chars(line_num);
            let line_text: String = self
                .buffer()
                .content
                .slice(line_start_char..line_start_char + line_len)
                .chars()
                .collect();

            // Find and replace in this line
            let new_line = if global {
                self.replace_all_in_string(&line_text, pattern, replacement, case_insensitive)
            } else {
                self.replace_first_in_string(&line_text, pattern, replacement, case_insensitive)
            };

            if new_line != line_text {
                // Delete old line content and insert new
                self.delete_with_undo(line_start_char, line_start_char + line_len);
                self.insert_with_undo(line_start_char, &new_line);
                replacements += 1;
            }
        }

        self.finish_undo_group();
        Ok(replacements)
    }

    /// Helper: Replace all occurrences in a string
    fn replace_all_in_string(
        &self,
        text: &str,
        pattern: &str,
        replacement: &str,
        case_insensitive: bool,
    ) -> String {
        if case_insensitive {
            // Case-insensitive: convert to lowercase for comparison
            let pattern_lower = pattern.to_lowercase();
            let text_lower = text.to_lowercase();

            let mut result = String::new();
            let mut last_pos = 0;

            while let Some(pos) = text_lower[last_pos..].find(&pattern_lower) {
                let absolute_pos = last_pos + pos;
                result.push_str(&text[last_pos..absolute_pos]);
                result.push_str(replacement);
                last_pos = absolute_pos + pattern.len();
            }
            result.push_str(&text[last_pos..]);
            result
        } else {
            text.replace(pattern, replacement)
        }
    }

    /// Helper: Replace first occurrence in a string
    fn replace_first_in_string(
        &self,
        text: &str,
        pattern: &str,
        replacement: &str,
        case_insensitive: bool,
    ) -> String {
        if case_insensitive {
            let pattern_lower = pattern.to_lowercase();
            let text_lower = text.to_lowercase();

            if let Some(pos) = text_lower.find(&pattern_lower) {
                let mut result = String::new();
                result.push_str(&text[..pos]);
                result.push_str(replacement);
                result.push_str(&text[pos + pattern.len()..]);
                result
            } else {
                text.to_string()
            }
        } else if let Some(pos) = text.find(pattern) {
            let mut result = String::new();
            result.push_str(&text[..pos]);
            result.push_str(replacement);
            result.push_str(&text[pos + pattern.len()..]);
            result
        } else {
            text.to_string()
        }
    }

    // --- Word motions ---

    fn move_word_forward(&mut self) {
        let total_chars = self.buffer().len_chars();
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos >= total_chars {
            return;
        }

        let first = self.buffer().content.char(pos);
        if is_word_char(first) {
            while pos < total_chars && is_word_char(self.buffer().content.char(pos)) {
                pos += 1;
            }
        } else if !first.is_whitespace() {
            while pos < total_chars {
                let ch = self.buffer().content.char(pos);
                if is_word_char(ch) || ch.is_whitespace() {
                    break;
                }
                pos += 1;
            }
        }

        while pos < total_chars && self.buffer().content.char(pos).is_whitespace() {
            pos += 1;
        }

        if pos >= total_chars {
            pos = total_chars.saturating_sub(1);
        }

        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    fn move_word_backward(&mut self) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos == 0 {
            return;
        }
        pos -= 1;

        while pos > 0 && self.buffer().content.char(pos).is_whitespace() {
            pos -= 1;
        }

        let ch = self.buffer().content.char(pos);
        if is_word_char(ch) {
            while pos > 0 && is_word_char(self.buffer().content.char(pos - 1)) {
                pos -= 1;
            }
        } else {
            while pos > 0 {
                let prev = self.buffer().content.char(pos - 1);
                if is_word_char(prev) || prev.is_whitespace() {
                    break;
                }
                pos -= 1;
            }
        }

        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    fn move_word_end(&mut self) {
        let total_chars = self.buffer().len_chars();
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos >= total_chars {
            return;
        }

        let current_char = self.buffer().content.char(pos);

        // Check if we're already at the end of a word
        let at_word_end = if pos + 1 < total_chars {
            let next_char = self.buffer().content.char(pos + 1);
            (is_word_char(current_char) && !is_word_char(next_char))
                || (!is_word_char(current_char)
                    && !current_char.is_whitespace()
                    && (is_word_char(next_char) || next_char.is_whitespace()))
        } else {
            false
        };

        // If at end of word, move to next word; otherwise move within current word
        if at_word_end || current_char.is_whitespace() {
            // Skip past current position
            pos += 1;
            // Skip whitespace
            while pos < total_chars && self.buffer().content.char(pos).is_whitespace() {
                pos += 1;
            }
        } else {
            // We're in the middle of a word, find its end
            // Don't increment pos here - stay on current character
        }

        if pos >= total_chars {
            pos = total_chars - 1;
        }

        let ch = self.buffer().content.char(pos);
        if is_word_char(ch) {
            while pos + 1 < total_chars && is_word_char(self.buffer().content.char(pos + 1)) {
                pos += 1;
            }
        } else if !ch.is_whitespace() {
            while pos + 1 < total_chars {
                let next = self.buffer().content.char(pos + 1);
                if is_word_char(next) || next.is_whitespace() {
                    break;
                }
                pos += 1;
            }
        }

        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    fn move_word_end_backward(&mut self) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos == 0 {
            return;
        }

        // Move back one character first
        pos -= 1;

        // Skip whitespace backward
        while pos > 0 && self.buffer().content.char(pos).is_whitespace() {
            pos -= 1;
        }

        // If we're at position 0 and it's whitespace, stop
        if pos == 0 {
            if self.buffer().content.char(pos).is_whitespace() {
                return;
            }
            // At position 0 and it's not whitespace, this is the end of the first word
            let new_line = self.buffer().content.char_to_line(pos);
            let line_start = self.buffer().line_to_char(new_line);
            self.view_mut().cursor.line = new_line;
            self.view_mut().cursor.col = pos - line_start;
            return;
        }

        // Now we're on a non-whitespace char - find the start of this word
        let ch = self.buffer().content.char(pos);
        if is_word_char(ch) {
            // Move to start of word
            while pos > 0 && is_word_char(self.buffer().content.char(pos - 1)) {
                pos -= 1;
            }
        } else {
            // Non-word punctuation
            while pos > 0 {
                let prev = self.buffer().content.char(pos - 1);
                if is_word_char(prev) || prev.is_whitespace() {
                    break;
                }
                pos -= 1;
            }
        }

        // Now pos is at the start of a word, go back to find the end of the previous word
        if pos == 0 {
            // Already at start of buffer
            return;
        }

        pos -= 1;

        // Skip whitespace backward
        while pos > 0 && self.buffer().content.char(pos).is_whitespace() {
            pos -= 1;
        }

        // Now we're at the end of the previous word
        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    // --- Paragraph motions ---

    fn move_paragraph_forward(&mut self) {
        let total_lines = self.buffer().len_lines();
        let mut line = self.view().cursor.line;

        // Move forward at least one line to find the next empty line
        if line + 1 >= total_lines {
            // Already at or past last line, don't move
            return;
        }
        line += 1;

        // Search for the next empty line
        while line < total_lines && !self.is_line_empty(line) {
            line += 1;
        }

        // If we found an empty line, move there
        if line < total_lines {
            self.view_mut().cursor.line = line;
            // Move to end of line (column 0 for empty lines)
            self.view_mut().cursor.col = self.get_line_len_for_insert(line);
        }
        // Otherwise stay at current position (EOF case)
    }

    fn move_paragraph_backward(&mut self) {
        let mut line = self.view().cursor.line;

        // Already at top, don't move
        if line == 0 {
            return;
        }
        line -= 1;

        // Search backward for an empty line
        while line > 0 && !self.is_line_empty(line) {
            line -= 1;
        }

        // Move to the found empty line (or line 0 if that's where we stopped)
        self.view_mut().cursor.line = line;
        // Move to end of line (column 0 for empty lines)
        self.view_mut().cursor.col = self.get_line_len_for_insert(line);
    }

    /// Returns true if the line is empty or contains only whitespace.
    fn is_line_empty(&self, line: usize) -> bool {
        if line >= self.buffer().len_lines() {
            return false;
        }

        let line_len = self.buffer().line_len_chars(line);

        // Line with no characters or just newline
        if line_len == 0 || line_len == 1 {
            return true;
        }

        // Check if all characters are whitespace
        let line_start = self.buffer().line_to_char(line);
        for i in 0..line_len {
            let ch = self.buffer().content.char(line_start + i);
            if ch != '\n' && !ch.is_whitespace() {
                return false;
            }
        }

        true
    }

    // --- Character find motions (f, F, t, T, ;, ,) ---

    /// Find a character on the current line.
    /// motion_type: 'f' (forward inclusive), 'F' (backward inclusive),
    ///              't' (forward till/exclusive), 'T' (backward till/exclusive)
    fn find_char(&mut self, motion_type: char, target: char) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_start = self.buffer().line_to_char(line);
        let line_len = self.buffer().line_len_chars(line);

        match motion_type {
            'f' => {
                // Find forward (inclusive): search right of cursor
                for i in (col + 1)..line_len {
                    let ch = self.buffer().content.char(line_start + i);
                    if ch == target && ch != '\n' {
                        self.view_mut().cursor.col = i;
                        return;
                    }
                }
            }
            'F' => {
                // Find backward (inclusive): search left of cursor
                if col > 0 {
                    for i in (0..col).rev() {
                        let ch = self.buffer().content.char(line_start + i);
                        if ch == target {
                            self.view_mut().cursor.col = i;
                            return;
                        }
                    }
                }
            }
            't' => {
                // Till forward (exclusive): stop before target
                for i in (col + 1)..line_len {
                    let ch = self.buffer().content.char(line_start + i);
                    if ch == target && ch != '\n' {
                        if i > 0 {
                            self.view_mut().cursor.col = i - 1;
                        }
                        return;
                    }
                }
            }
            'T' => {
                // Till backward (exclusive): stop after target
                if col > 0 {
                    for i in (0..col).rev() {
                        let ch = self.buffer().content.char(line_start + i);
                        if ch == target {
                            self.view_mut().cursor.col = i + 1;
                            return;
                        }
                    }
                }
            }
            _ => {}
        }
        // Character not found - cursor doesn't move (Vim behavior)
    }

    /// Repeat the last character find motion.
    /// If reverse is true, search in the opposite direction.
    fn repeat_find(&mut self, reverse: bool) {
        if let Some((motion_type, target)) = self.last_find {
            let actual_motion = if reverse {
                // Reverse the direction
                match motion_type {
                    'f' => 'F',
                    'F' => 'f',
                    't' => 'T',
                    'T' => 't',
                    _ => motion_type,
                }
            } else {
                motion_type
            };
            self.find_char(actual_motion, target);
        }
    }

    // --- Bracket matching (%) ---

    fn move_to_matching_bracket(&mut self) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let char_pos = self.buffer().line_to_char(line) + col;

        if char_pos >= self.buffer().len_chars() {
            return;
        }

        let current_char = self.buffer().content.char(char_pos);

        // Check if current character is a bracket and determine search parameters
        let (is_opening, open_char, close_char) = match current_char {
            '(' => (true, '(', ')'),
            ')' => (false, '(', ')'),
            '{' => (true, '{', '}'),
            '}' => (false, '{', '}'),
            '[' => (true, '[', ']'),
            ']' => (false, '[', ']'),
            _ => {
                // Not on a bracket, search forward on current line for next bracket
                self.search_forward_for_bracket();
                return;
            }
        };

        // Find matching bracket
        if let Some(match_pos) =
            self.find_matching_bracket(char_pos, open_char, close_char, is_opening)
        {
            let new_line = self.buffer().content.char_to_line(match_pos);
            let line_start = self.buffer().line_to_char(new_line);
            self.view_mut().cursor.line = new_line;
            self.view_mut().cursor.col = match_pos - line_start;
        }
    }

    fn search_forward_for_bracket(&mut self) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_start = self.buffer().line_to_char(line);
        let line_len = self.buffer().line_len_chars(line);

        // Search forward from cursor position for any bracket
        for i in col..line_len {
            let pos = line_start + i;
            if pos >= self.buffer().len_chars() {
                return;
            }
            let ch = self.buffer().content.char(pos);
            match ch {
                '(' | ')' | '{' | '}' | '[' | ']' => {
                    self.view_mut().cursor.col = i;
                    // Now move to matching bracket
                    self.move_to_matching_bracket();
                    return;
                }
                '\n' => return, // Don't go past end of line
                _ => {}
            }
        }
    }

    fn find_matching_bracket(
        &self,
        start_pos: usize,
        open_char: char,
        close_char: char,
        is_opening: bool,
    ) -> Option<usize> {
        let total_chars = self.buffer().len_chars();
        let mut depth = 1;

        if is_opening {
            // Search forward
            let mut pos = start_pos + 1;
            while pos < total_chars {
                let ch = self.buffer().content.char(pos);
                if ch == open_char {
                    depth += 1;
                } else if ch == close_char {
                    depth -= 1;
                    if depth == 0 {
                        return Some(pos);
                    }
                }
                pos += 1;
            }
        } else {
            // Search backward
            if start_pos == 0 {
                return None;
            }
            let mut pos = start_pos - 1;
            loop {
                let ch = self.buffer().content.char(pos);
                if ch == open_char {
                    depth -= 1;
                    if depth == 0 {
                        return Some(pos);
                    }
                } else if ch == close_char {
                    depth += 1;
                }
                if pos == 0 {
                    break;
                }
                pos -= 1;
            }
        }

        None
    }

    /// Find the range for a text object.
    /// Returns (start_pos, end_pos) if found, None otherwise.
    fn find_text_object_range(
        &self,
        modifier: char,
        obj_type: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        match obj_type {
            'w' => self.find_word_object(modifier, cursor_pos),
            '"' => self.find_quote_object(modifier, '"', cursor_pos),
            '\'' => self.find_quote_object(modifier, '\'', cursor_pos),
            '(' | ')' => self.find_bracket_object(modifier, '(', ')', cursor_pos),
            '{' | '}' => self.find_bracket_object(modifier, '{', '}', cursor_pos),
            '[' | ']' => self.find_bracket_object(modifier, '[', ']', cursor_pos),
            _ => None,
        }
    }

    /// Find word text object range (iw/aw)
    fn find_word_object(&self, modifier: char, cursor_pos: usize) -> Option<(usize, usize)> {
        let total_chars = self.buffer().len_chars();
        if cursor_pos >= total_chars {
            return None;
        }

        let char_at_cursor = self.buffer().content.char(cursor_pos);

        // If on whitespace and modifier is 'i', no match
        if modifier == 'i' && (char_at_cursor.is_whitespace() && char_at_cursor != '\n') {
            return None;
        }

        // Find word boundaries
        let mut start = cursor_pos;
        let mut end = cursor_pos;

        // Expand backward to start of word
        while start > 0 {
            let ch = self.buffer().content.char(start - 1);
            if ch.is_whitespace() || !is_word_char(ch) {
                break;
            }
            start -= 1;
        }

        // Expand forward to end of word
        while end < total_chars {
            let ch = self.buffer().content.char(end);
            if ch.is_whitespace() || !is_word_char(ch) {
                break;
            }
            end += 1;
        }

        // For 'aw', include trailing whitespace
        if modifier == 'a' {
            while end < total_chars {
                let ch = self.buffer().content.char(end);
                if !ch.is_whitespace() || ch == '\n' {
                    break;
                }
                end += 1;
            }
        }

        if start < end {
            Some((start, end))
        } else {
            None
        }
    }

    /// Find quote text object range (i"/a")
    fn find_quote_object(
        &self,
        modifier: char,
        quote_char: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        let total_chars = self.buffer().len_chars();
        if cursor_pos >= total_chars {
            return None;
        }

        // Get current line bounds to search within
        let cursor_line = self.buffer().content.char_to_line(cursor_pos);
        let line_start = self.buffer().line_to_char(cursor_line);
        let line_len = self.buffer().line_len_chars(cursor_line);
        let line_end = line_start + line_len;

        // Find opening quote (search backward from cursor)
        let mut open_pos = None;
        let mut pos = cursor_pos;
        while pos >= line_start {
            let ch = self.buffer().content.char(pos);
            if ch == quote_char {
                // Check if it's escaped
                if pos == line_start || self.buffer().content.char(pos - 1) != '\\' {
                    open_pos = Some(pos);
                    break;
                }
            }
            if pos == line_start {
                break;
            }
            pos -= 1;
        }

        let open_pos = open_pos?;

        // Find closing quote (search forward from opening)
        let mut close_pos = None;
        let mut pos = open_pos + 1;
        while pos < line_end {
            let ch = self.buffer().content.char(pos);
            if ch == quote_char {
                // Check if it's escaped
                if self.buffer().content.char(pos - 1) != '\\' {
                    close_pos = Some(pos);
                    break;
                }
            }
            pos += 1;
        }

        let close_pos = close_pos?;

        // Return range based on modifier
        if modifier == 'i' {
            // Inner: exclude quotes
            if open_pos < close_pos {
                Some((open_pos + 1, close_pos))
            } else {
                None
            }
        } else {
            // Around: include quotes
            Some((open_pos, close_pos + 1))
        }
    }

    /// Find bracket text object range (i(/a()
    fn find_bracket_object(
        &self,
        modifier: char,
        open_char: char,
        close_char: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        let total_chars = self.buffer().len_chars();
        if cursor_pos >= total_chars {
            return None;
        }

        // Find the nearest enclosing bracket pair
        let mut open_pos = None;
        let mut depth = 0;

        // Search backward for opening bracket
        let mut pos = cursor_pos;
        loop {
            let ch = self.buffer().content.char(pos);
            if ch == close_char {
                depth += 1;
            } else if ch == open_char {
                if depth == 0 {
                    open_pos = Some(pos);
                    break;
                } else {
                    depth -= 1;
                }
            }
            if pos == 0 {
                break;
            }
            pos -= 1;
        }

        let open_pos = open_pos?;

        // Find matching closing bracket
        let close_pos = self.find_matching_bracket(open_pos, open_char, close_char, true)?;

        // Return range based on modifier
        if modifier == 'i' {
            // Inner: exclude brackets
            if open_pos < close_pos {
                Some((open_pos + 1, close_pos))
            } else {
                None
            }
        } else {
            // Around: include brackets
            Some((open_pos, close_pos + 1))
        }
    }

    /// Apply an operator to a text object
    fn apply_operator_text_object(
        &mut self,
        operator: char,
        modifier: char,
        obj_type: char,
        changed: &mut bool,
    ) {
        let cursor = self.view().cursor;
        let cursor_pos = self.buffer().line_to_char(cursor.line) + cursor.col;

        // Find text object range
        let range = match self.find_text_object_range(modifier, obj_type, cursor_pos) {
            Some(r) => r,
            None => return, // No matching text object found
        };

        let (start_pos, end_pos) = range;
        if start_pos >= end_pos {
            return;
        }

        // Get text content
        let text_content: String = self
            .buffer()
            .content
            .slice(start_pos..end_pos)
            .chars()
            .collect();

        let reg = self.active_register();
        self.set_register(reg, text_content, false);
        self.clear_selected_register();

        // Perform operation based on operator type
        match operator {
            'y' => {
                // Yank only - don't delete, don't change cursor
                // No undo group needed for yank
            }
            'd' | 'c' => {
                // Delete or change
                self.start_undo_group();
                self.delete_with_undo(start_pos, end_pos);

                // Move cursor to start of deletion
                let new_line = self.buffer().content.char_to_line(start_pos);
                let line_start = self.buffer().line_to_char(new_line);
                let new_col = start_pos - line_start;
                self.view_mut().cursor.line = new_line;
                self.view_mut().cursor.col = new_col;

                *changed = true;

                // If operator is 'c', enter insert mode
                if operator == 'c' {
                    self.mode = Mode::Insert;
                    self.count = None;
                    // Don't finish_undo_group - let insert mode do it
                    // Don't clamp cursor - insert mode allows cursor at end of line
                } else {
                    self.clamp_cursor_col();
                    self.finish_undo_group();
                }
            }
            _ => {
                // Unknown operator - do nothing
            }
        }
    }

    // --- Line operations ---

    #[allow(dead_code)]
    fn delete_current_line(&mut self, changed: &mut bool) {
        self.delete_lines(1, changed);
    }

    /// Delete count lines starting from current line
    fn delete_lines(&mut self, count: usize, changed: &mut bool) {
        let num_lines = self.buffer().len_lines();
        if num_lines == 0 {
            return;
        }

        let start_line = self.view().cursor.line;
        let end_line = (start_line + count).min(num_lines);
        let actual_count = end_line - start_line;

        if actual_count == 0 {
            return;
        }

        let line_start = self.buffer().line_to_char(start_line);
        let line_end = if end_line < num_lines {
            self.buffer().line_to_char(end_line)
        } else {
            self.buffer().len_chars()
        };

        // Save deleted lines to register (linewise)
        let deleted_content: String = self
            .buffer()
            .content
            .slice(line_start..line_end)
            .chars()
            .collect();

        // Ensure linewise content ends with newline
        let deleted_content = if deleted_content.ends_with('\n') {
            deleted_content
        } else {
            format!("{}\n", deleted_content)
        };
        let reg = self.active_register();
        self.set_register(reg, deleted_content, true);
        self.clear_selected_register();

        // Determine what to delete
        let (delete_start, delete_end) = if end_line < num_lines {
            // Delete lines including their newlines
            (line_start, line_end)
        } else {
            // Deleting to end of buffer
            if start_line > 0 {
                // Delete the newline before the first line being deleted
                (line_start - 1, line_end)
            } else {
                (line_start, line_end)
            }
        };

        self.delete_with_undo(delete_start, delete_end);
        *changed = true;

        let new_num_lines = self.buffer().len_lines();
        if self.view().cursor.line >= new_num_lines && new_num_lines > 0 {
            self.view_mut().cursor.line = new_num_lines - 1;
        }
        self.view_mut().cursor.col = 0;
        self.clamp_cursor_col();
    }

    #[allow(dead_code)]
    fn delete_to_end_of_line(&mut self, changed: &mut bool) {
        self.delete_to_end_of_line_with_count(1, changed);
    }

    fn delete_to_end_of_line_with_count(&mut self, count: usize, changed: &mut bool) {
        let start_line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let char_idx = self.buffer().line_to_char(start_line) + col;

        if count == 1 {
            // Single D: delete to end of current line, excluding newline
            let line_content = self.buffer().content.line(start_line);
            let line_start = self.buffer().line_to_char(start_line);
            let line_end = line_start + line_content.len_chars();

            let delete_end = if line_content.chars().last() == Some('\n') {
                line_end - 1
            } else {
                line_end
            };

            if char_idx < delete_end {
                let deleted_content: String = self
                    .buffer()
                    .content
                    .slice(char_idx..delete_end)
                    .chars()
                    .collect();
                let reg = self.active_register();
                self.set_register(reg, deleted_content, false);
                self.clear_selected_register();

                self.delete_with_undo(char_idx, delete_end);
                self.clamp_cursor_col();
                *changed = true;
            }
        } else {
            // Multiple D: delete to end of current line (excluding newline) + (count-1) full lines below
            let total_lines = self.buffer().len_lines();
            let line_content = self.buffer().content.line(start_line);
            let line_start = self.buffer().line_to_char(start_line);
            let line_end = line_start + line_content.len_chars();

            // End of current line excluding newline
            let first_part_end = if line_content.chars().last() == Some('\n') {
                line_end - 1
            } else {
                line_end
            };

            // Build the content to delete (for register)
            let to_eol: String = self
                .buffer()
                .content
                .slice(char_idx..first_part_end)
                .chars()
                .collect();

            let mut deleted_content = to_eol;
            deleted_content.push('\n');

            // Add (count-1) full lines
            if count > 1 {
                let last_line = (start_line + count - 1).min(total_lines - 1);
                let lines_start = line_end; // After newline of current line
                let lines_end = if last_line + 1 < total_lines {
                    self.buffer().line_to_char(last_line + 1)
                } else {
                    self.buffer().len_chars()
                };

                let full_lines: String = self
                    .buffer()
                    .content
                    .slice(lines_start..lines_end)
                    .chars()
                    .collect();
                deleted_content.push_str(&full_lines);
            }

            let reg = self.active_register();
            self.set_register(reg, deleted_content, false);
            self.clear_selected_register();

            // Perform the actual deletion: from char_idx to first_part_end
            self.delete_with_undo(char_idx, first_part_end);

            // Now delete the (count-1) full lines that follow
            if count > 1 {
                // After deleting to EOL, the cursor position hasn't moved
                // The newline is at char_idx, and we want to delete starting from char_idx + 1
                let lines_to_delete = count - 1;
                let delete_from = char_idx + 1; // Start after the newline

                // Calculate how many chars to delete
                let remaining_lines = self.buffer().len_lines() - start_line - 1;
                let actual_lines_to_delete = lines_to_delete.min(remaining_lines);

                if actual_lines_to_delete > 0 {
                    let delete_to =
                        if start_line + 1 + actual_lines_to_delete < self.buffer().len_lines() {
                            self.buffer()
                                .line_to_char(start_line + 1 + actual_lines_to_delete)
                        } else {
                            self.buffer().len_chars()
                        };

                    if delete_from < delete_to {
                        self.delete_with_undo(delete_from, delete_to);
                    }
                }
            }

            self.clamp_cursor_col();
            *changed = true;
        }
    }

    fn move_left(&mut self) {
        if self.view().cursor.col > 0 {
            self.view_mut().cursor.col -= 1;
        }
    }

    fn move_down(&mut self) {
        let max_line = self.buffer().len_lines().saturating_sub(1);
        let mut next = self.view().cursor.line;
        loop {
            if next >= max_line {
                return;
            }
            next += 1;
            if !self.view().is_line_hidden(next) {
                break;
            }
        }
        self.view_mut().cursor.line = next;
        self.clamp_cursor_col();
    }

    fn move_up(&mut self) {
        let mut prev = self.view().cursor.line;
        loop {
            if prev == 0 {
                return;
            }
            prev -= 1;
            if !self.view().is_line_hidden(prev) {
                break;
            }
        }
        self.view_mut().cursor.line = prev;
        self.clamp_cursor_col();
    }

    // ── Indent / completion helpers ───────────────────────────────────────────

    /// Return the leading whitespace string (spaces/tabs) of the given buffer line.
    fn get_line_indent_str(&self, line_idx: usize) -> String {
        let total = self.buffer().len_lines();
        if line_idx >= total {
            return String::new();
        }
        self.buffer()
            .content
            .line(line_idx)
            .chars()
            .take_while(|&c| c == ' ' || c == '\t')
            .collect()
    }

    /// True for word characters: [a-zA-Z0-9_].
    fn is_word_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    /// Walk left from cursor to find the current word prefix.
    /// Returns `(prefix, start_col)` where `start_col` is the column index
    /// where the prefix begins.
    fn completion_prefix_at_cursor(&self) -> (String, usize) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let chars: Vec<char> = self.buffer().content.line(line).chars().collect();
        let mut start = col;
        while start > 0 && Self::is_word_char(chars[start - 1]) {
            start -= 1;
        }
        let prefix: String = chars[start..col].iter().collect();
        (prefix, start)
    }

    /// Collect all words in the current buffer that start with `prefix`,
    /// deduplicated, sorted, excluding an exact match of `prefix` itself.
    fn word_completions_for_prefix(&self, prefix: &str) -> Vec<String> {
        let mut set: std::collections::HashSet<String> = Default::default();
        for line_idx in 0..self.buffer().len_lines() {
            let text: String = self.buffer().content.line(line_idx).chars().collect();
            let chars: Vec<char> = text.chars().collect();
            let len = chars.len();
            let mut i = 0usize;
            while i < len {
                if Self::is_word_char(chars[i]) {
                    let start = i;
                    while i < len && Self::is_word_char(chars[i]) {
                        i += 1;
                    }
                    let word: String = chars[start..i].iter().collect();
                    if word.starts_with(prefix) && word != prefix {
                        set.insert(word);
                    }
                } else {
                    i += 1;
                }
            }
        }
        let mut v: Vec<String> = set.into_iter().collect();
        v.sort();
        v
    }

    /// Delete the previously inserted candidate (or prefix), insert the new
    /// candidate at `completion_start_col`, and update the cursor column.
    fn apply_completion_candidate(&mut self, idx: usize) {
        let line = self.view().cursor.line;
        let prev_end = self.view().cursor.col;
        let start = self.completion_start_col;
        let line_char = self.buffer().line_to_char(line);
        if prev_end > start {
            self.delete_with_undo(line_char + start, line_char + prev_end);
        }
        let candidate = self.completion_candidates[idx].clone();
        self.insert_with_undo(line_char + start, &candidate);
        self.view_mut().cursor.col = start + candidate.len();
    }

    // ── Fold helpers ──────────────────────────────────────────────────────────

    /// Count leading whitespace characters (spaces = 1, tabs = tab_width).
    fn line_indent(&self, line_idx: usize) -> usize {
        let total = self.buffer().len_lines();
        if line_idx >= total {
            return 0;
        }
        let line = self.buffer().content.line(line_idx);
        let tab_width = 4usize;
        let mut indent = 0usize;
        for ch in line.chars() {
            match ch {
                ' ' => indent += 1,
                '\t' => indent += tab_width,
                _ => break,
            }
        }
        indent
    }

    /// Detect the fold range starting at `start_line` using indentation heuristics.
    /// Returns `Some((start, end))` when at least one following line has strictly
    /// greater indentation. Returns `None` for blank/empty trailing sections.
    fn detect_fold_range(&self, start_line: usize) -> Option<(usize, usize)> {
        let total = self.buffer().len_lines();
        if start_line + 1 >= total {
            return None;
        }
        let base_indent = self.line_indent(start_line);
        let mut end = start_line;
        for idx in (start_line + 1)..total {
            let line = self.buffer().content.line(idx);
            let text: String = line.chars().collect();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                // blank lines are included in fold body
                end = idx;
                continue;
            }
            if self.line_indent(idx) > base_indent {
                end = idx;
            } else {
                break;
            }
        }
        if end > start_line {
            Some((start_line, end))
        } else {
            None
        }
    }

    /// Toggle the fold at `line_idx` regardless of cursor position.
    /// Used by click handlers when the user clicks the fold indicator.
    pub fn toggle_fold_at_line(&mut self, line_idx: usize) {
        if self.view().fold_at(line_idx).is_some() {
            self.view_mut().open_fold(line_idx);
        } else {
            let saved = self.view().cursor.line;
            self.view_mut().cursor.line = line_idx;
            self.cmd_fold_close();
            self.view_mut().cursor.line = saved;
        }
    }

    fn cmd_fold_toggle(&mut self) {
        let line = self.view().cursor.line;
        if self.view().fold_at(line).is_some() {
            self.view_mut().open_fold(line);
        } else {
            self.cmd_fold_close();
        }
    }

    fn cmd_fold_close(&mut self) {
        let line = self.view().cursor.line;
        if let Some((start, end)) = self.detect_fold_range(line) {
            self.view_mut().close_fold(start, end);
            // If cursor ended up inside the fold, move it to the header.
            if self.view().is_line_hidden(self.view().cursor.line) {
                self.view_mut().cursor.line = start;
                self.clamp_cursor_col();
            }
        }
    }

    fn cmd_fold_open(&mut self) {
        let line = self.view().cursor.line;
        self.view_mut().open_fold(line);
    }

    fn move_right(&mut self) {
        let line = self.view().cursor.line;
        let max_valid_col = self.get_max_cursor_col(line);
        if self.view().cursor.col < max_valid_col {
            self.view_mut().cursor.col += 1;
        }
    }

    fn move_right_insert(&mut self) {
        let line = self.view().cursor.line;
        let max = self.get_line_len_for_insert(line);
        if self.view().cursor.col < max {
            self.view_mut().cursor.col += 1;
        }
    }

    fn get_line_len_for_insert(&self, line_idx: usize) -> usize {
        let len = self.buffer().line_len_chars(line_idx);
        if len == 0 {
            return 0;
        }
        let line = self.buffer().content.line(line_idx);
        if line.chars().last() == Some('\n') {
            len - 1
        } else {
            len
        }
    }

    fn clamp_cursor_col_insert(&mut self) {
        let line = self.view().cursor.line;
        let max = self.get_line_len_for_insert(line);
        if self.view().cursor.col > max {
            self.view_mut().cursor.col = max;
        }
    }

    // --- Register operations ---

    /// Returns the active register name (selected or default '"').
    fn active_register(&self) -> char {
        self.selected_register.unwrap_or('"')
    }

    /// Sets a register's content. `is_linewise` affects paste behavior.
    fn set_register(&mut self, reg: char, content: String, is_linewise: bool) {
        self.registers.insert(reg, (content.clone(), is_linewise));
        // Also copy to unnamed register if using a named register
        if reg != '"' {
            self.registers.insert('"', (content, is_linewise));
        }
    }

    /// Gets a register's content and linewise flag.
    fn get_register(&self, reg: char) -> Option<&(String, bool)> {
        self.registers.get(&reg)
    }

    /// Clears the selected register after an operation.
    fn clear_selected_register(&mut self) {
        self.selected_register = None;
    }

    // --- Macro operations ---

    /// Encode a keystroke for macro recording using Vim-style notation.
    /// Returns a string representation that can be decoded during playback.
    fn encode_key_for_macro(&self, key_name: &str, unicode: Option<char>, ctrl: bool) -> String {
        // Handle Ctrl combinations
        if ctrl {
            if let Some(ch) = unicode {
                // Ctrl-D, Ctrl-U, etc.
                return format!("<C-{}>", ch.to_uppercase());
            }
        }

        // Handle special keys (no unicode)
        if unicode.is_none() {
            match key_name {
                "Escape" => return "\x1b".to_string(),
                "Return" => return "<CR>".to_string(),
                "BackSpace" => return "<BS>".to_string(),
                "Delete" => return "<Del>".to_string(),
                "Left" => return "<Left>".to_string(),
                "Right" => return "<Right>".to_string(),
                "Up" => return "<Up>".to_string(),
                "Down" => return "<Down>".to_string(),
                "Home" => return "<Home>".to_string(),
                "End" => return "<End>".to_string(),
                "Page_Up" => return "<PageUp>".to_string(),
                "Page_Down" => return "<PageDown>".to_string(),
                _ => return String::new(), // Unknown key, don't record
            }
        }

        // Regular character
        if let Some(ch) = unicode {
            ch.to_string()
        } else {
            String::new()
        }
    }

    /// Start recording a macro into the specified register.
    fn start_macro_recording(&mut self, register: char) {
        self.macro_recording = Some(register);
        self.recording_buffer.clear();
        self.message = format!("Recording macro into register '{}'", register);
    }

    /// Stop recording and save the macro to the register.
    fn stop_macro_recording(&mut self) {
        if let Some(reg) = self.macro_recording {
            // Convert recording_buffer to string
            let macro_content: String = self.recording_buffer.iter().collect();

            // Store in register (not linewise)
            self.set_register(reg, macro_content, false);

            self.message = format!("Macro recorded into register '{}'", reg);
            self.macro_recording = None;
            self.recording_buffer.clear();
        }
    }

    /// Play a macro from the specified register.
    fn play_macro(&mut self, register: char) -> Result<(), String> {
        // Check recursion depth
        if self.macro_recursion_depth >= MAX_MACRO_RECURSION {
            return Err("Macro recursion too deep".to_string());
        }

        // Get macro content from register (clone it to avoid borrow issues)
        let content = if let Some((content, _)) = self.get_register(register) {
            content.clone()
        } else {
            self.message = format!("Register '{}' is empty", register);
            return Ok(());
        };

        if content.is_empty() {
            self.message = format!("Register '{}' is empty", register);
            return Ok(());
        }

        // Remember last macro for @@
        self.last_macro_register = Some(register);

        // Add keys to playback queue
        for ch in content.chars() {
            self.macro_playback_queue.push_back(ch);
        }

        self.message = format!("Playing macro from register '{}'", register);
        Ok(())
    }

    /// Play a macro with a count prefix.
    fn play_macro_with_count(&mut self, register: char, count: usize) -> Result<(), String> {
        for _ in 0..count {
            self.play_macro(register)?;
        }
        Ok(())
    }

    /// Takes and consumes the count, returning it (or 1 if no count was entered).
    /// This clears the count field.
    #[allow(dead_code)] // Will be used in Step 2 for motion commands
    pub fn take_count(&mut self) -> usize {
        self.count.take().unwrap_or(1)
    }

    /// Peeks at the current count without consuming it. Used for UI display.
    pub fn peek_count(&self) -> Option<usize> {
        self.count
    }

    /// Yank the current line into the active register (linewise).
    #[allow(dead_code)]
    fn yank_current_line(&mut self) {
        let line = self.view().cursor.line;
        let line_start = self.buffer().line_to_char(line);
        let line_len = self.buffer().line_len_chars(line);
        let content: String = self
            .buffer()
            .content
            .slice(line_start..line_start + line_len)
            .chars()
            .collect();

        // Ensure linewise content ends with newline
        let content = if content.ends_with('\n') {
            content
        } else {
            format!("{}\n", content)
        };

        let reg = self.active_register();
        self.set_register(reg, content, true);
        self.clear_selected_register();
        self.message = "1 line yanked".to_string();
    }

    /// Replace count characters with the replacement character
    fn replace_chars(&mut self, replacement: char, count: usize, changed: &mut bool) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let char_idx = self.buffer().line_to_char(line) + col;

        // Calculate how many chars we can replace on this line (not crossing newline)
        let line_end = self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
        let available = line_end.saturating_sub(char_idx);

        // Don't count the newline character at the end of line
        let line_content = self.buffer().content.line(line);
        let available = if line_content.chars().last() == Some('\n') {
            available.saturating_sub(1)
        } else {
            available
        };

        let to_replace = count.min(available);

        if to_replace > 0 && char_idx < self.buffer().len_chars() {
            // Build the replacement string
            let replacement_str: String = std::iter::repeat_n(replacement, to_replace).collect();

            // Delete the old characters and insert the new ones
            self.delete_with_undo(char_idx, char_idx + to_replace);
            self.insert_with_undo(char_idx, &replacement_str);

            // Keep cursor at the start position (Vim behavior)
            self.view_mut().cursor.col = col;
            self.clamp_cursor_col();
            *changed = true;
        }
    }

    /// Yank count lines starting from current line
    fn yank_lines(&mut self, count: usize) {
        let start_line = self.view().cursor.line;
        let total_lines = self.buffer().len_lines();
        let end_line = (start_line + count).min(total_lines);
        let actual_count = end_line - start_line;

        if actual_count == 0 {
            return;
        }

        let start_char = self.buffer().line_to_char(start_line);
        let end_char = if end_line < total_lines {
            self.buffer().line_to_char(end_line)
        } else {
            self.buffer().len_chars()
        };

        let content: String = self
            .buffer()
            .content
            .slice(start_char..end_char)
            .chars()
            .collect();

        // Ensure linewise content ends with newline
        let content = if content.ends_with('\n') {
            content
        } else {
            format!("{}\n", content)
        };

        let reg = self.active_register();
        self.set_register(reg, content, true);
        self.clear_selected_register();

        let msg = if actual_count == 1 {
            "1 line yanked".to_string()
        } else {
            format!("{} lines yanked", actual_count)
        };
        self.message = msg;
    }

    /// Paste after cursor (p). Linewise pastes below current line.
    fn paste_after(&mut self, changed: &mut bool) {
        let reg = self.active_register();
        let (content, is_linewise) = match self.get_register(reg) {
            Some((c, l)) => (c.clone(), *l),
            None => {
                self.clear_selected_register();
                return;
            }
        };

        self.start_undo_group();

        if is_linewise {
            // Paste below current line
            let line = self.view().cursor.line;
            let line_end = self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
            // If current line doesn't end with newline, we need to add one
            let line_content = self.buffer().content.line(line);
            if line_content.chars().last() == Some('\n') {
                self.insert_with_undo(line_end, &content);
            } else {
                // Insert newline + content
                let content_with_newline = format!("\n{}", content);
                self.insert_with_undo(line_end, &content_with_newline);
            };
            // Move cursor to first non-blank of new line
            self.view_mut().cursor.line += 1;
            self.view_mut().cursor.col = 0;
        } else {
            // Paste after cursor position
            let line = self.view().cursor.line;
            let col = self.view().cursor.col;
            let char_idx = self.buffer().line_to_char(line) + col;
            // Insert after current char (if line not empty)
            let insert_pos = if self.buffer().line_len_chars(line) > 0 {
                char_idx + 1
            } else {
                char_idx
            };
            self.insert_with_undo(insert_pos, &content);
            // Move cursor to end of pasted text (last char)
            let paste_len = content.chars().count();
            if paste_len > 0 {
                self.view_mut().cursor.col = col + paste_len;
            }
        }

        self.finish_undo_group();
        self.clear_selected_register();
        *changed = true;
    }

    /// Paste before cursor (P). Linewise pastes above current line.
    fn paste_before(&mut self, changed: &mut bool) {
        let reg = self.active_register();
        let (content, is_linewise) = match self.get_register(reg) {
            Some((c, l)) => (c.clone(), *l),
            None => {
                self.clear_selected_register();
                return;
            }
        };

        self.start_undo_group();

        if is_linewise {
            // Paste above current line
            let line = self.view().cursor.line;
            let line_start = self.buffer().line_to_char(line);
            self.insert_with_undo(line_start, &content);
            // Cursor stays on same line number (which is now the pasted line)
            self.view_mut().cursor.col = 0;
        } else {
            // Paste before cursor position
            let line = self.view().cursor.line;
            let col = self.view().cursor.col;
            let char_idx = self.buffer().line_to_char(line) + col;
            self.insert_with_undo(char_idx, &content);
            // Cursor moves to end of pasted text
            let paste_len = content.chars().count();
            if paste_len > 0 {
                self.view_mut().cursor.col = col + paste_len - 1;
            }
        }

        self.finish_undo_group();
        self.clear_selected_register();
        *changed = true;
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LineNumberMode;

    fn press_char(engine: &mut Engine, ch: char) {
        engine.handle_key(&ch.to_string(), Some(ch), false);
    }

    fn press_special(engine: &mut Engine, name: &str) {
        engine.handle_key(name, None, false);
    }

    fn press_ctrl(engine: &mut Engine, ch: char) {
        engine.handle_key(&ch.to_string(), Some(ch), true);
    }

    #[test]
    fn test_normal_movement() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "Hello");

        press_char(&mut engine, 'l');
        assert_eq!(engine.view().cursor.col, 1);

        press_char(&mut engine, 'h');
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_bounds_checking() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "Hi\nThere");

        press_char(&mut engine, 'l');
        press_char(&mut engine, 'l');
        press_char(&mut engine, 'l');
        assert!(
            engine.view().cursor.col <= 1,
            "Cursor col went too far right"
        );

        press_char(&mut engine, 'j');
        assert_eq!(engine.view().cursor.line, 1);

        press_char(&mut engine, 'j');
        assert_eq!(
            engine.view().cursor.line,
            1,
            "Cursor line went past last line"
        );
    }

    #[test]
    fn test_column_clamping() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "Long line\nShort");

        for _ in 0..10 {
            press_char(&mut engine, 'l');
        }

        press_char(&mut engine, 'j');
        assert!(
            engine.view().cursor.col <= 4,
            "Cursor col not clamped on short line"
        );
    }

    #[test]
    fn test_arrow_keys() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "AB\nCD");

        press_special(&mut engine, "Right");
        assert_eq!(engine.view().cursor.col, 1);

        press_special(&mut engine, "Down");
        assert_eq!(engine.view().cursor.line, 1);

        press_special(&mut engine, "Up");
        assert_eq!(engine.view().cursor.line, 0);

        press_special(&mut engine, "Left");
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_insert_mode_typing() {
        let mut engine = Engine::new();
        press_char(&mut engine, 'i');
        assert_eq!(engine.mode, Mode::Insert);

        press_char(&mut engine, 'H');
        press_char(&mut engine, 'i');
        press_char(&mut engine, '!');
        assert_eq!(engine.buffer().to_string(), "Hi!");
        assert_eq!(engine.view().cursor.col, 3);

        press_special(&mut engine, "Escape");
        assert_eq!(engine.mode, Mode::Normal);
    }

    #[test]
    fn test_insert_special_chars() {
        let mut engine = Engine::new();
        press_char(&mut engine, 'i');

        for ch in "fn main() { println!(\"hello\"); }".chars() {
            press_char(&mut engine, ch);
        }
        assert_eq!(
            engine.buffer().to_string(),
            "fn main() { println!(\"hello\"); }"
        );
    }

    #[test]
    fn test_insert_tab() {
        let mut engine = Engine::new();
        press_char(&mut engine, 'i');
        press_special(&mut engine, "Tab");
        assert_eq!(engine.buffer().to_string(), "    ");
        assert_eq!(engine.view().cursor.col, 4);
    }

    #[test]
    fn test_backspace_joins_lines() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "AB\nCD");
        engine.update_syntax();

        press_char(&mut engine, 'j');
        press_char(&mut engine, 'i');
        assert_eq!(engine.view().cursor.line, 1);
        assert_eq!(engine.view().cursor.col, 0);

        press_special(&mut engine, "BackSpace");
        assert_eq!(engine.view().cursor.line, 0);
        assert_eq!(engine.buffer().to_string(), "ABCD");
    }

    #[test]
    fn test_delete_key() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ABC");
        engine.update_syntax();

        press_char(&mut engine, 'i');
        press_special(&mut engine, "Delete");
        assert_eq!(engine.buffer().to_string(), "BC");
    }

    #[test]
    fn test_normal_x_deletes_char() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ABC");
        engine.update_syntax();

        press_char(&mut engine, 'x');
        assert_eq!(engine.buffer().to_string(), "BC");
    }

    #[test]
    fn test_normal_o_opens_line_below() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "AB\nCD");
        engine.update_syntax();

        press_char(&mut engine, 'o');
        assert_eq!(engine.mode, Mode::Insert);
        assert_eq!(engine.view().cursor.line, 1);
        assert_eq!(engine.view().cursor.col, 0);
        assert_eq!(engine.buffer().to_string(), "AB\n\nCD");
    }

    fn type_command(engine: &mut Engine, cmd: &str) {
        press_char(engine, ':');
        assert_eq!(engine.mode, Mode::Command);
        for ch in cmd.chars() {
            engine.handle_key(&ch.to_string(), Some(ch), false);
        }
        press_special(engine, "Return");
    }

    #[test]
    fn test_command_mode_enter_exit() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "Hello");

        press_char(&mut engine, ':');
        assert_eq!(engine.mode, Mode::Command);
        assert!(engine.command_buffer.is_empty());

        press_special(&mut engine, "Escape");
        assert_eq!(engine.mode, Mode::Normal);
    }

    #[test]
    fn test_command_quit_clean() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "Hello");
        engine.set_dirty(false);

        press_char(&mut engine, ':');
        press_char(&mut engine, 'q');
        let action = engine.handle_key("Return", None, false);
        assert_eq!(action, EngineAction::Quit);
    }

    #[test]
    fn test_command_quit_dirty_blocked() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "Hello");
        engine.set_dirty(true);

        type_command(&mut engine, "q");
        assert!(engine.message.contains("No write since last change"));
    }

    #[test]
    fn test_command_force_quit() {
        let mut engine = Engine::new();
        engine.set_dirty(true);

        press_char(&mut engine, ':');
        for ch in "q!".chars() {
            engine.handle_key(&ch.to_string(), Some(ch), false);
        }
        let action = engine.handle_key("Return", None, false);
        assert_eq!(action, EngineAction::Quit);
    }

    #[test]
    fn test_command_unknown() {
        let mut engine = Engine::new();
        type_command(&mut engine, "notacommand");
        assert!(engine.message.contains("Not an editor command"));
    }

    #[test]
    fn test_q_closes_tab_when_multiple_tabs() {
        let mut engine = Engine::new();
        // Tab 0 — first file
        engine.buffer_mut().insert(0, "first");
        engine.set_dirty(false);
        let first_id = engine.active_buffer_id();
        // Tab 1 — second file
        engine.new_tab(None);
        engine.buffer_mut().insert(0, "second");
        engine.set_dirty(false);
        assert_eq!(engine.tabs.len(), 2);
        assert_eq!(engine.buffer_manager.len(), 2);
        // :q closes the active tab, not the whole app
        let action = type_command_action(&mut engine, "q");
        assert_eq!(action, EngineAction::None);
        assert_eq!(engine.tabs.len(), 1, "tab should be closed");
        // The closed buffer is freed; session restore excludes it via window-filter.
        assert_eq!(engine.buffer_manager.len(), 1);
        assert!(engine.buffer_manager.get(first_id).is_some());
    }

    #[test]
    fn test_q_quits_when_single_buffer_clean() {
        let mut engine = Engine::new();
        engine.set_dirty(false);
        let action = type_command_action(&mut engine, "q");
        assert_eq!(action, EngineAction::Quit);
    }

    #[test]
    fn test_q_blocks_when_single_buffer_dirty() {
        let mut engine = Engine::new();
        engine.set_dirty(true);
        type_command(&mut engine, "q");
        assert!(engine.message.contains("No write since last change"));
    }

    #[test]
    fn test_q_bang_closes_dirty_tab_when_multiple() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "first");
        engine.set_dirty(false);
        engine.new_tab(None);
        engine.buffer_mut().insert(0, "second");
        engine.set_dirty(true); // dirty but force-close with q!
        assert_eq!(engine.tabs.len(), 2);
        let action = type_command_action(&mut engine, "q!");
        assert_eq!(action, EngineAction::None);
        assert_eq!(engine.tabs.len(), 1, "tab should be closed");
        assert_eq!(engine.buffer_manager.len(), 1);
    }

    #[test]
    fn test_q_bang_quits_when_single_buffer() {
        let mut engine = Engine::new();
        engine.set_dirty(true);
        let action = type_command_action(&mut engine, "q!");
        assert_eq!(action, EngineAction::Quit);
    }

    #[test]
    fn test_qa_quits_when_all_clean() {
        let mut engine = Engine::new();
        engine.set_dirty(false);
        let action = type_command_action(&mut engine, "qa");
        assert_eq!(action, EngineAction::Quit);
    }

    #[test]
    fn test_qa_blocks_when_any_dirty() {
        let mut engine = Engine::new();
        engine.set_dirty(true);
        type_command(&mut engine, "qa");
        assert!(engine.message.contains("No write since last change"));
    }

    #[test]
    fn test_qa_bang_force_quits() {
        let mut engine = Engine::new();
        engine.set_dirty(true);
        let action = type_command_action(&mut engine, "qa!");
        assert_eq!(action, EngineAction::Quit);
    }

    #[test]
    fn test_restore_session_files_opens_separate_tabs() {
        let dir = std::env::temp_dir();
        let p1 = dir.join("vimcode_restore_a.txt");
        let p2 = dir.join("vimcode_restore_b.txt");
        let p3 = dir.join("vimcode_restore_c.txt");
        std::fs::write(&p1, "aaa").unwrap();
        std::fs::write(&p2, "bbb").unwrap();
        std::fs::write(&p3, "ccc").unwrap();

        let mut engine = Engine::new();
        engine.session.open_files = vec![p1.clone(), p2.clone(), p3.clone()];
        engine.session.active_file = Some(p2.clone());
        engine.restore_session_files();

        // Three files → three tabs.
        assert_eq!(engine.tabs.len(), 3, "each file should get its own tab");
        // Three buffers in manager (no scratch buffer).
        assert_eq!(engine.buffer_manager.len(), 3);
        // Active tab should be the one showing p2.
        let active_buf = engine.active_buffer_id();
        let active_path = engine
            .buffer_manager
            .get(active_buf)
            .and_then(|s| s.file_path.clone());
        assert_eq!(active_path.as_deref(), Some(p2.as_path()));

        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
        let _ = std::fs::remove_file(&p3);
    }

    #[test]
    fn test_ctrl_s_saves_in_normal_mode() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("vimcode_test_ctrl_s.txt");
        std::fs::write(&path, "original").unwrap();
        let mut engine = Engine::open(&path);
        // Edit the buffer (direct insert to simulate typing)
        engine.buffer_mut().insert(0, "new ");
        engine.set_dirty(true);
        // Ctrl-S in normal mode
        let action = engine.handle_key("s", Some('s'), true);
        assert_eq!(action, EngineAction::None);
        // File should be saved (not dirty)
        assert!(!engine.dirty());
        let _ = std::fs::remove_file(&path);
    }

    /// Helper: type a command and return its EngineAction.
    fn type_command_action(engine: &mut Engine, cmd: &str) -> EngineAction {
        press_char(engine, ':');
        for ch in cmd.chars() {
            engine.handle_key(&ch.to_string(), Some(ch), false);
        }
        engine.handle_key("Return", None, false)
    }

    #[test]
    fn test_history_search_basic() {
        let mut engine = Engine::new();
        engine.session.add_command("write");
        engine.session.add_command("quit");
        engine.session.add_command("wall");

        // Enter command mode, then Ctrl-R
        press_char(&mut engine, ':');
        press_ctrl(&mut engine, 'r');

        assert!(engine.history_search_active);
        // Most recent match with empty query: "wall"
        assert_eq!(engine.command_buffer, "wall");
    }

    #[test]
    fn test_history_search_typing_filters() {
        let mut engine = Engine::new();
        engine.session.add_command("write");
        engine.session.add_command("quit");
        engine.session.add_command("wall");

        press_char(&mut engine, ':');
        press_ctrl(&mut engine, 'r');

        // Type "w" - should match most recent command containing "w": "wall"
        engine.handle_key("w", Some('w'), false);
        assert_eq!(engine.history_search_query, "w");
        assert_eq!(engine.command_buffer, "wall");

        // Type "r" -> "wr" - should match "write"
        engine.handle_key("r", Some('r'), false);
        assert_eq!(engine.history_search_query, "wr");
        assert_eq!(engine.command_buffer, "write");
    }

    #[test]
    fn test_history_search_ctrl_r_cycles() {
        let mut engine = Engine::new();
        engine.session.add_command("write");
        engine.session.add_command("wquit");
        engine.session.add_command("wall");

        press_char(&mut engine, ':');
        press_ctrl(&mut engine, 'r');
        engine.handle_key("w", Some('w'), false);

        // First match: "wall" (most recent with "w")
        assert_eq!(engine.command_buffer, "wall");

        // Ctrl-R again: next older match "wquit"
        press_ctrl(&mut engine, 'r');
        assert_eq!(engine.command_buffer, "wquit");

        // Ctrl-R again: next older match "write"
        press_ctrl(&mut engine, 'r');
        assert_eq!(engine.command_buffer, "write");
    }

    #[test]
    fn test_history_search_escape_cancels() {
        let mut engine = Engine::new();
        engine.session.add_command("write");
        engine.session.add_command("quit");

        press_char(&mut engine, ':');
        engine.handle_key("w", Some('w'), false); // type "w" normally
        press_ctrl(&mut engine, 'r');

        assert!(engine.history_search_active);

        // Escape should cancel and restore original buffer ("w")
        press_special(&mut engine, "Escape");
        assert!(!engine.history_search_active);
        assert_eq!(engine.command_buffer, "w");
        // Mode is still Command (Escape from search returns to command line)
        assert_eq!(engine.mode, Mode::Command);
    }

    #[test]
    fn test_history_search_enter_accepts() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello\nworld\nfoo");
        engine.session.add_command("3");

        press_char(&mut engine, ':');
        press_ctrl(&mut engine, 'r');

        // Found "3" (only history entry)
        assert_eq!(engine.command_buffer, "3");

        // Enter executes it
        press_special(&mut engine, "Return");
        assert!(!engine.history_search_active);
        assert_eq!(engine.mode, Mode::Normal);
        assert_eq!(engine.view().cursor.line, 2); // jumped to line 3
    }

    #[test]
    fn test_history_search_backspace_narrows() {
        let mut engine = Engine::new();
        engine.session.add_command("write");
        engine.session.add_command("wall");

        press_char(&mut engine, ':');
        press_ctrl(&mut engine, 'r');
        engine.handle_key("r", Some('r'), false); // query = "r", matches "write"
        assert_eq!(engine.command_buffer, "write");

        // Backspace removes "r" -> query = "", matches "wall" (most recent)
        press_special(&mut engine, "BackSpace");
        assert_eq!(engine.history_search_query, "");
        assert_eq!(engine.command_buffer, "wall");
    }

    #[test]
    fn test_command_line_number_jump() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "line1\nline2\nline3\nline4\nline5");

        type_command(&mut engine, "3");
        assert_eq!(engine.view().cursor.line, 2);
    }

    #[test]
    fn test_command_save() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("vimcode_test_save");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_save.txt");

        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"original").unwrap();
        }

        let mut engine = Engine::open(&path);
        assert_eq!(engine.buffer().to_string(), "original");

        engine.buffer_mut().insert(0, "new ");
        engine.set_dirty(true);
        type_command(&mut engine, "w");
        assert!(!engine.dirty());
        assert!(engine.message.contains("written"));

        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(saved, "new original");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn test_dirty_flag() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "Hello");
        assert!(!engine.dirty());

        press_char(&mut engine, 'i');
        press_char(&mut engine, 'X');
        assert!(engine.dirty());
    }

    #[test]
    fn test_search_basic() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo bar foo baz foo");

        press_char(&mut engine, '/');
        assert_eq!(engine.mode, Mode::Search);

        for ch in "foo".chars() {
            engine.handle_key(&ch.to_string(), Some(ch), false);
        }
        press_special(&mut engine, "Return");

        assert_eq!(engine.mode, Mode::Normal);
        assert_eq!(engine.search_query, "foo");
        assert_eq!(engine.search_matches.len(), 3);
        assert!(engine.message.contains("match"));
    }

    #[test]
    fn test_search_not_found() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");

        press_char(&mut engine, '/');
        for ch in "zzz".chars() {
            engine.handle_key(&ch.to_string(), Some(ch), false);
        }
        press_special(&mut engine, "Return");

        assert!(engine.search_matches.is_empty());
        assert!(engine.message.contains("Pattern not found"));
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_search_n_and_N() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "aXa\naXa\naXa");

        press_char(&mut engine, '/');
        engine.handle_key("X", Some('X'), false);
        press_special(&mut engine, "Return");

        assert_eq!(engine.search_matches.len(), 3);
        let first_line = engine.view().cursor.line;
        let first_col = engine.view().cursor.col;

        press_char(&mut engine, 'n');
        assert!(
            engine.view().cursor.line > first_line
                || (engine.view().cursor.line == first_line
                    && engine.view().cursor.col > first_col)
                || engine.search_matches.len() == 1,
            "n should advance to next match"
        );

        press_char(&mut engine, 'N');
    }

    #[test]
    fn test_search_escape_cancels() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello");

        press_char(&mut engine, '/');
        assert_eq!(engine.mode, Mode::Search);
        press_special(&mut engine, "Escape");
        assert_eq!(engine.mode, Mode::Normal);
        assert!(engine.search_query.is_empty());
    }

    #[test]
    fn test_incremental_search_forward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo bar baz foo");
        engine.update_syntax();

        // Start at beginning
        assert_eq!(engine.view().cursor.line, 0);
        assert_eq!(engine.view().cursor.col, 0);

        // Enter search mode
        press_char(&mut engine, '/');
        assert_eq!(engine.mode, Mode::Search);

        // Type 'f' - should jump to first 'foo'
        press_char(&mut engine, 'f');
        assert_eq!(engine.view().cursor.col, 0); // Already at first 'f'

        // Type 'o' - should still be at 'foo'
        press_char(&mut engine, 'o');
        assert_eq!(engine.view().cursor.col, 0);

        // Type 'o' - complete 'foo'
        press_char(&mut engine, 'o');
        assert_eq!(engine.view().cursor.col, 0);
        assert_eq!(engine.search_matches.len(), 2);

        // Press Enter to confirm
        press_special(&mut engine, "Return");
        assert_eq!(engine.mode, Mode::Normal);
    }

    #[test]
    fn test_incremental_search_backward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo bar baz foo");
        engine.update_syntax();

        // Move to end of line
        for _ in 0..15 {
            press_char(&mut engine, 'l');
        }
        let start_col = engine.view().cursor.col;

        // Enter reverse search mode
        press_char(&mut engine, '?');

        // Type 'foo' - should jump to last 'foo' before cursor
        press_char(&mut engine, 'f');
        press_char(&mut engine, 'o');
        press_char(&mut engine, 'o');

        // Should have jumped to the second 'foo' (at col 12)
        assert!(engine.view().cursor.col < start_col);
        assert_eq!(engine.view().cursor.col, 12);
    }

    #[test]
    fn test_incremental_search_escape_restores_cursor() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world test");
        engine.update_syntax();

        // Move to col 6 (start of 'world')
        for _ in 0..6 {
            press_char(&mut engine, 'l');
        }
        assert_eq!(engine.view().cursor.col, 6);

        // Start search
        press_char(&mut engine, '/');

        // Type 'test' - cursor should jump to 'test'
        for ch in "test".chars() {
            press_char(&mut engine, ch);
        }
        assert_eq!(engine.view().cursor.col, 12);

        // Escape - should restore to original position (col 6)
        press_special(&mut engine, "Escape");
        assert_eq!(engine.mode, Mode::Normal);
        assert_eq!(engine.view().cursor.col, 6);
    }

    #[test]
    fn test_incremental_search_backspace() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo food fool");
        engine.update_syntax();

        // Start search
        press_char(&mut engine, '/');

        // Type 'fool' - should jump to 'fool'
        for ch in "fool".chars() {
            press_char(&mut engine, ch);
        }
        assert_eq!(engine.view().cursor.col, 9);

        // Backspace to 'foo' - should update to first 'foo'
        press_special(&mut engine, "BackSpace");
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_incremental_search_no_match() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Move to col 5
        for _ in 0..5 {
            press_char(&mut engine, 'l');
        }
        assert_eq!(engine.view().cursor.col, 5);

        // Start search
        press_char(&mut engine, '/');

        // Type pattern that doesn't exist
        for ch in "xyz".chars() {
            press_char(&mut engine, ch);
        }

        // Cursor should stay at original position
        assert_eq!(engine.view().cursor.col, 5);
        assert!(engine.message.contains("not found"));
    }

    #[test]
    fn test_reverse_search_basic() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo bar foo baz foo");

        // Enter reverse search mode with '?'
        press_char(&mut engine, '?');
        assert_eq!(engine.mode, Mode::Search);

        // Type search pattern
        for ch in "foo".chars() {
            engine.handle_key(&ch.to_string(), Some(ch), false);
        }
        press_special(&mut engine, "Return");

        assert_eq!(engine.mode, Mode::Normal);
        assert_eq!(engine.search_query, "foo");
        assert_eq!(engine.search_matches.len(), 3);
    }

    #[test]
    fn test_reverse_search_n_goes_backward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1 X\nline2 X\nline3 X");

        // Move to line 3
        engine.view_mut().cursor.line = 2;
        engine.view_mut().cursor.col = 6;

        // Reverse search for 'X'
        press_char(&mut engine, '?');
        engine.handle_key("X", Some('X'), false);
        press_special(&mut engine, "Return");

        assert_eq!(engine.search_matches.len(), 3);

        // After '?', 'n' should go to previous match (backward)
        let start_line = engine.view().cursor.line;
        press_char(&mut engine, 'n');

        // Should move to an earlier line or same line with earlier column
        assert!(
            engine.view().cursor.line < start_line
                || (engine.view().cursor.line == start_line && engine.view().cursor.col < 6),
            "n after ? should go backward"
        );
    }

    #[test]
    fn test_reverse_search_n_goes_forward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1 X\nline2 X\nline3 X");

        // Move to line 2, after the last match
        engine.view_mut().cursor.line = 2;
        engine.view_mut().cursor.col = 7;

        // Reverse search for 'X' - should find the match on line 2
        press_char(&mut engine, '?');
        engine.handle_key("X", Some('X'), false);
        press_special(&mut engine, "Return");

        assert_eq!(engine.search_matches.len(), 3);
        assert_eq!(engine.view().cursor.line, 2);
        assert_eq!(engine.view().cursor.col, 6);

        // After '?', 'N' should go to next match (forward), wrapping to line 0
        press_char(&mut engine, 'N');
        assert_eq!(engine.view().cursor.line, 0, "N after ? should go forward");
    }

    #[test]
    fn test_forward_then_reverse_search() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "line1 X\nline2 X\nline3 X\nline4 X");

        // Start at line 1
        engine.view_mut().cursor.line = 1;
        engine.view_mut().cursor.col = 0;

        // Forward search with '/' - should find X on line 1
        press_char(&mut engine, '/');
        engine.handle_key("X", Some('X'), false);
        press_special(&mut engine, "Return");
        assert_eq!(engine.search_matches.len(), 4);
        assert_eq!(engine.view().cursor.line, 1);

        // 'n' should go forward to line 2
        press_char(&mut engine, 'n');
        assert_eq!(engine.view().cursor.line, 2, "n after / should go forward");

        // Now do a reverse search with '?' - should find X on line 1 (previous match)
        press_char(&mut engine, '?');
        engine.handle_key("X", Some('X'), false);
        press_special(&mut engine, "Return");
        assert_eq!(engine.view().cursor.line, 1);

        // 'n' should now go backward to line 0
        press_char(&mut engine, 'n');
        assert_eq!(engine.view().cursor.line, 0, "n after ? should go backward");
    }

    #[test]
    fn test_word_forward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world foo");

        press_char(&mut engine, 'w');
        assert_eq!(engine.view().cursor.col, 6);

        press_char(&mut engine, 'w');
        assert_eq!(engine.view().cursor.col, 12);
    }

    #[test]
    fn test_word_backward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world foo");

        press_char(&mut engine, '$');

        press_char(&mut engine, 'b');
        assert_eq!(engine.view().cursor.col, 12);

        press_char(&mut engine, 'b');
        assert_eq!(engine.view().cursor.col, 6);
    }

    #[test]
    fn test_word_end() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");

        press_char(&mut engine, 'e');
        assert_eq!(engine.view().cursor.col, 4);

        press_char(&mut engine, 'e');
        assert_eq!(engine.view().cursor.col, 10);
    }

    #[test]
    fn test_paragraph_forward_basic() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "text1\ntext2\n\ntext3");
        // Cursor at line 0 (text1)

        press_char(&mut engine, '}');
        assert_eq!(engine.view().cursor.line, 2); // Empty line
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_paragraph_backward_basic() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "text1\n\ntext2\ntext3");
        engine.view_mut().cursor.line = 3;

        press_char(&mut engine, '{');
        assert_eq!(engine.view().cursor.line, 1); // Empty line
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_paragraph_forward_from_empty_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "text1\n\ntext2\n\ntext3");
        engine.view_mut().cursor.line = 1; // First empty line

        press_char(&mut engine, '}');
        assert_eq!(engine.view().cursor.line, 3); // Next empty line
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_paragraph_backward_from_empty_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "text1\n\ntext2\n\ntext3");
        engine.view_mut().cursor.line = 3; // Second empty line

        press_char(&mut engine, '{');
        assert_eq!(engine.view().cursor.line, 1); // First empty line
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_paragraph_forward_at_eof() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "text1\ntext2\ntext3");
        engine.view_mut().cursor.line = 2; // Last line

        press_char(&mut engine, '}');
        assert_eq!(engine.view().cursor.line, 2); // Stays at last line
    }

    #[test]
    fn test_paragraph_backward_at_bof() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "text1\ntext2\ntext3");
        // Cursor at line 0

        press_char(&mut engine, '{');
        assert_eq!(engine.view().cursor.line, 0); // Stays at line 0
    }

    #[test]
    fn test_paragraph_whitespace_lines() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "text1\n  \t  \ntext2");

        press_char(&mut engine, '}');
        assert_eq!(engine.view().cursor.line, 1); // Whitespace line
        assert_eq!(engine.view().cursor.col, 5); // End of whitespace line
    }

    #[test]
    fn test_paragraph_forward_multiple() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "a\n\nb\n\nc\n\nd");

        press_char(&mut engine, '}');
        assert_eq!(engine.view().cursor.line, 1);

        press_char(&mut engine, '}');
        assert_eq!(engine.view().cursor.line, 3);

        press_char(&mut engine, '}');
        assert_eq!(engine.view().cursor.line, 5);
    }

    #[test]
    fn test_paragraph_backward_multiple() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "a\n\nb\n\nc\n\nd");
        engine.view_mut().cursor.line = 6;

        press_char(&mut engine, '{');
        assert_eq!(engine.view().cursor.line, 5);

        press_char(&mut engine, '{');
        assert_eq!(engine.view().cursor.line, 3);

        press_char(&mut engine, '{');
        assert_eq!(engine.view().cursor.line, 1);
    }

    #[test]
    fn test_paragraph_consecutive_empty_lines() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "text\n\n\n\nmore");

        press_char(&mut engine, '}');
        assert_eq!(engine.view().cursor.line, 1); // First empty

        press_char(&mut engine, '}');
        assert_eq!(engine.view().cursor.line, 2); // Second empty

        press_char(&mut engine, '}');
        assert_eq!(engine.view().cursor.line, 3); // Third empty
    }

    #[test]
    fn test_gg_goes_to_top() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3\nline4");
        engine.view_mut().cursor.line = 3;

        press_char(&mut engine, 'g');
        press_char(&mut engine, 'g');
        assert_eq!(engine.view().cursor.line, 0);
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_G_goes_to_bottom() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3\nline4");

        press_char(&mut engine, 'G');
        assert_eq!(engine.view().cursor.line, 3);
    }

    #[test]
    fn test_dd_deletes_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3");

        press_char(&mut engine, 'd');
        press_char(&mut engine, 'd');
        assert_eq!(engine.buffer().to_string(), "line2\nline3");
        assert_eq!(engine.view().cursor.line, 0);
    }

    #[test]
    fn test_dd_deletes_middle_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "aaa\nbbb\nccc");

        press_char(&mut engine, 'j');
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'd');
        assert_eq!(engine.buffer().to_string(), "aaa\nccc");
        assert_eq!(engine.view().cursor.line, 1);
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_D_deletes_to_end_of_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world\nline2");

        press_char(&mut engine, 'l');
        press_char(&mut engine, 'l');
        press_char(&mut engine, 'l');
        press_char(&mut engine, 'l');
        press_char(&mut engine, 'l');
        press_char(&mut engine, 'D');
        assert_eq!(engine.buffer().to_string(), "hello\nline2");
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_A_appends_at_end() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello\nworld");

        press_char(&mut engine, 'A');
        assert_eq!(engine.mode, Mode::Insert);
        let line_insert_len = engine.get_line_len_for_insert(0);
        assert_eq!(engine.view().cursor.col, line_insert_len);
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_I_inserts_at_first_nonwhitespace() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "    hello");

        press_char(&mut engine, 'I');
        assert_eq!(engine.mode, Mode::Insert);
        assert_eq!(engine.view().cursor.col, 4);
    }

    #[test]
    fn test_ensure_cursor_visible() {
        let mut engine = Engine::new();
        let mut text = String::new();
        for i in 0..100 {
            text.push_str(&format!("line {}\n", i));
        }
        engine.buffer_mut().insert(0, &text);
        engine.set_viewport_lines(20);

        engine.view_mut().cursor.line = 50;
        engine.ensure_cursor_visible();
        assert!(engine.scroll_top() <= 50);
        assert!(engine.scroll_top() + engine.viewport_lines() > 50);
    }

    #[test]
    fn test_ctrl_d_half_page_down() {
        let mut engine = Engine::new();
        let mut text = String::new();
        for i in 0..100 {
            text.push_str(&format!("line {}\n", i));
        }
        engine.buffer_mut().insert(0, &text);
        engine.set_viewport_lines(20);

        engine.handle_key("d", Some('d'), true);
        assert_eq!(engine.view().cursor.line, 10);
    }

    #[test]
    fn test_ctrl_u_half_page_up() {
        let mut engine = Engine::new();
        let mut text = String::new();
        for i in 0..100 {
            text.push_str(&format!("line {}\n", i));
        }
        engine.buffer_mut().insert(0, &text);
        engine.set_viewport_lines(20);
        engine.view_mut().cursor.line = 50;

        engine.handle_key("u", Some('u'), true);
        assert_eq!(engine.view().cursor.line, 40);
    }

    #[test]
    fn test_open_nonexistent_file() {
        let path = std::path::PathBuf::from("/tmp/vimcode_nonexistent_12345.txt");
        let engine = Engine::open(&path);
        assert!(engine.buffer().to_string().is_empty());
        assert!(engine.message.contains("[New File]"));
        assert_eq!(engine.file_path(), Some(&path));
    }

    #[test]
    fn test_open_existing_file() {
        use std::io::Write;
        let path = std::env::temp_dir().join("vimcode_test_open.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"test content").unwrap();
        }

        let engine = Engine::open(&path);
        assert_eq!(engine.buffer().to_string(), "test content");
        assert!(!engine.dirty());

        let _ = std::fs::remove_file(&path);
    }

    // --- New tests for multi-buffer/window/tab ---

    #[test]
    fn test_split_window() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello");

        assert_eq!(engine.windows.len(), 1);
        assert_eq!(engine.active_tab().window_ids().len(), 1);

        engine.split_window(SplitDirection::Vertical, None);

        assert_eq!(engine.windows.len(), 2);
        assert_eq!(engine.active_tab().window_ids().len(), 2);
    }

    #[test]
    fn test_close_window() {
        let mut engine = Engine::new();
        engine.split_window(SplitDirection::Vertical, None);
        assert_eq!(engine.windows.len(), 2);

        engine.close_window();
        assert_eq!(engine.windows.len(), 1);
    }

    #[test]
    fn test_window_cycling() {
        let mut engine = Engine::new();
        engine.split_window(SplitDirection::Vertical, None);

        let first_window = engine.active_window_id();
        engine.focus_next_window();
        let second_window = engine.active_window_id();
        assert_ne!(first_window, second_window);

        engine.focus_next_window();
        assert_eq!(engine.active_window_id(), first_window);
    }

    #[test]
    fn test_new_tab() {
        let mut engine = Engine::new();
        assert_eq!(engine.tabs.len(), 1);

        engine.new_tab(None);
        assert_eq!(engine.tabs.len(), 2);
        assert_eq!(engine.active_tab, 1);
    }

    #[test]
    fn test_tab_navigation() {
        let mut engine = Engine::new();
        engine.new_tab(None);
        engine.new_tab(None);
        assert_eq!(engine.tabs.len(), 3);
        assert_eq!(engine.active_tab, 2);

        engine.prev_tab();
        assert_eq!(engine.active_tab, 1);

        engine.next_tab();
        assert_eq!(engine.active_tab, 2);

        engine.goto_tab(0);
        assert_eq!(engine.active_tab, 0);
    }

    #[test]
    fn test_buffer_navigation() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "buffer 1");

        // Open a new file (creates second buffer)
        let path = std::env::temp_dir().join("vimcode_test_buf2.txt");
        std::fs::write(&path, "buffer 2").unwrap();

        engine.split_window(SplitDirection::Vertical, Some(&path));

        let buf2_id = engine.active_buffer_id();
        assert_eq!(engine.buffer().to_string(), "buffer 2");

        engine.prev_buffer();
        assert_ne!(engine.active_buffer_id(), buf2_id);

        engine.next_buffer();
        assert_eq!(engine.active_buffer_id(), buf2_id);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_list_buffers() {
        let engine = Engine::new();
        let listing = engine.list_buffers();
        assert!(listing.contains("[No Name]"));
    }

    #[test]
    fn test_ctrl_w_commands() {
        let mut engine = Engine::new();

        // Ctrl-W s should split horizontally
        press_ctrl(&mut engine, 'w');
        press_char(&mut engine, 's');
        assert_eq!(engine.windows.len(), 2);

        // Ctrl-W v should split vertically
        press_ctrl(&mut engine, 'w');
        press_char(&mut engine, 'v');
        assert_eq!(engine.windows.len(), 3);

        // Ctrl-W w should cycle
        let before = engine.active_window_id();
        press_ctrl(&mut engine, 'w');
        press_char(&mut engine, 'w');
        assert_ne!(engine.active_window_id(), before);

        // Ctrl-W c should close
        press_ctrl(&mut engine, 'w');
        press_char(&mut engine, 'c');
        assert_eq!(engine.windows.len(), 2);
    }

    #[test]
    fn test_gt_gT_tab_navigation() {
        let mut engine = Engine::new();
        engine.new_tab(None);
        engine.new_tab(None);
        engine.goto_tab(0);

        // gt should go to next tab
        press_char(&mut engine, 'g');
        press_char(&mut engine, 't');
        assert_eq!(engine.active_tab, 1);

        // gT should go to previous tab
        press_char(&mut engine, 'g');
        press_char(&mut engine, 'T');
        assert_eq!(engine.active_tab, 0);
    }

    // --- Undo/Redo tests ---

    #[test]
    fn test_undo_insert_mode_typing() {
        let mut engine = Engine::new();

        // Type "hello" in insert mode
        press_char(&mut engine, 'i');
        for ch in "hello".chars() {
            press_char(&mut engine, ch);
        }
        press_special(&mut engine, "Escape");

        assert_eq!(engine.buffer().to_string(), "hello");

        // Undo should remove entire "hello" (single undo group for insert session)
        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "");
    }

    #[test]
    fn test_undo_x_delete() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ABC");
        engine.update_syntax();

        // Delete 'A' with x
        press_char(&mut engine, 'x');
        assert_eq!(engine.buffer().to_string(), "BC");

        // Undo should restore 'A'
        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "ABC");
    }

    #[test]
    fn test_undo_dd_delete_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3");
        engine.update_syntax();

        // Delete first line with dd
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'd');
        assert_eq!(engine.buffer().to_string(), "line2\nline3");

        // Undo should restore the line
        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "line1\nline2\nline3");
    }

    #[test]
    fn test_undo_D_delete_to_eol() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world\nline2");
        engine.update_syntax();

        // Move to 'w' and delete to end of line
        for _ in 0..6 {
            press_char(&mut engine, 'l');
        }
        press_char(&mut engine, 'D');
        assert_eq!(engine.buffer().to_string(), "hello \nline2");

        // Undo should restore "world"
        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "hello world\nline2");
    }

    #[test]
    fn test_undo_o_open_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2");
        engine.update_syntax();

        // Open line below and type "new"
        press_char(&mut engine, 'o');
        for ch in "new".chars() {
            press_char(&mut engine, ch);
        }
        press_special(&mut engine, "Escape");

        assert_eq!(engine.buffer().to_string(), "line1\nnew\nline2");

        // Undo should remove the new line and text
        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "line1\nline2");
    }

    #[test]
    fn test_redo_after_undo() {
        let mut engine = Engine::new();

        // Type "hello"
        press_char(&mut engine, 'i');
        for ch in "hello".chars() {
            press_char(&mut engine, ch);
        }
        press_special(&mut engine, "Escape");

        assert_eq!(engine.buffer().to_string(), "hello");

        // Undo
        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "");

        // Redo with Ctrl-r
        press_ctrl(&mut engine, 'r');
        assert_eq!(engine.buffer().to_string(), "hello");
    }

    #[test]
    fn test_redo_cleared_on_new_edit() {
        let mut engine = Engine::new();

        // Type "hello"
        press_char(&mut engine, 'i');
        for ch in "hello".chars() {
            press_char(&mut engine, ch);
        }
        press_special(&mut engine, "Escape");

        // Undo
        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "");

        // New edit (type "world")
        press_char(&mut engine, 'i');
        for ch in "world".chars() {
            press_char(&mut engine, ch);
        }
        press_special(&mut engine, "Escape");

        // Redo should do nothing (redo stack was cleared)
        press_ctrl(&mut engine, 'r');
        assert_eq!(engine.buffer().to_string(), "world");
        assert!(engine.message.contains("Already at newest"));
    }

    #[test]
    fn test_multiple_undos() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ABC");
        engine.update_syntax();

        // Delete three chars one by one
        press_char(&mut engine, 'x'); // removes A
        press_char(&mut engine, 'x'); // removes B
        press_char(&mut engine, 'x'); // removes C

        assert_eq!(engine.buffer().to_string(), "");

        // Three undos should restore ABC
        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "C");

        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "BC");

        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "ABC");
    }

    #[test]
    fn test_undo_at_empty_stack() {
        let mut engine = Engine::new();

        // Try to undo with nothing to undo
        press_char(&mut engine, 'u');
        assert!(engine.message.contains("Already at oldest"));
    }

    #[test]
    fn test_undo_cursor_position_restored() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Move to column 6 ('w') and delete with x
        for _ in 0..6 {
            press_char(&mut engine, 'l');
        }
        assert_eq!(engine.view().cursor.col, 6);

        press_char(&mut engine, 'x'); // delete 'w'
        assert_eq!(engine.buffer().to_string(), "hello orld");

        // Undo should restore cursor to column 6
        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "hello world");
        assert_eq!(engine.view().cursor.col, 6);
    }

    #[test]
    fn test_undo_line_basic() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Make some changes to the line
        press_char(&mut engine, 'x'); // delete 'h' -> "ello world"
        press_char(&mut engine, 'x'); // delete 'e' -> "llo world"

        assert_eq!(engine.buffer().to_string(), "llo world");

        // Undo line with U
        press_char(&mut engine, 'U');

        assert_eq!(engine.buffer().to_string(), "hello world");
        assert_eq!(engine.view().cursor.line, 0);
    }

    #[test]
    fn test_undo_line_multiple_operations() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test");
        engine.update_syntax();

        // Multiple operations on the line
        press_char(&mut engine, 'A'); // append mode
        engine.handle_key("1", Some('1'), false);
        engine.handle_key("2", Some('2'), false);
        engine.handle_key("3", Some('3'), false);
        press_special(&mut engine, "Escape");

        assert_eq!(engine.buffer().to_string(), "test123");

        // Delete some chars
        press_char(&mut engine, 'x'); // delete '3'
        press_char(&mut engine, 'x'); // delete '2'

        assert_eq!(engine.buffer().to_string(), "test1");

        // U should restore original line
        press_char(&mut engine, 'U');

        assert_eq!(engine.buffer().to_string(), "test");
    }

    #[test]
    fn test_undo_line_no_changes() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello");
        engine.update_syntax();

        // Try U without making any changes
        press_char(&mut engine, 'U');

        // Should show message but not crash
        assert_eq!(engine.buffer().to_string(), "hello");
    }

    #[test]
    fn test_undo_line_multiline() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3");
        engine.update_syntax();

        // Modify line 1
        press_char(&mut engine, 'x'); // delete 'l' -> "ine1"
        assert_eq!(engine.buffer().to_string(), "ine1\nline2\nline3");

        // Move to line 2
        press_char(&mut engine, 'j');
        assert_eq!(engine.view().cursor.line, 1);

        // Modify line 2
        press_char(&mut engine, 'x'); // delete 'l' -> "ine2"
        assert_eq!(engine.buffer().to_string(), "ine1\nine2\nline3");

        // U should only restore line 2
        press_char(&mut engine, 'U');
        assert_eq!(engine.buffer().to_string(), "ine1\nline2\nline3");

        // Move back to line 1 - U won't work because we moved away
        press_char(&mut engine, 'k');
        press_char(&mut engine, 'U');
        // Line 1 stays modified because we moved away from it
        assert_eq!(engine.buffer().to_string(), "ine1\nline2\nline3");
    }

    #[test]
    fn test_undo_line_is_undoable() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello");
        engine.update_syntax();

        // Make a change
        press_char(&mut engine, 'x'); // "ello"
        assert_eq!(engine.buffer().to_string(), "ello");

        // U to restore
        press_char(&mut engine, 'U');
        assert_eq!(engine.buffer().to_string(), "hello");

        // Regular undo should undo the U operation
        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "ello");
    }

    // --- Yank/Paste/Register Tests ---

    #[test]
    fn test_yank_line_yy() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3");
        engine.update_syntax();

        // Yank first line with yy
        press_char(&mut engine, 'y');
        press_char(&mut engine, 'y');

        // Check register content
        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "line1\n");
        assert!(is_linewise);
        assert!(engine.message.contains("yanked"));
    }

    #[test]
    fn test_yank_line_Y() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "first\nsecond");
        engine.update_syntax();

        press_char(&mut engine, 'j'); // move to line 2
        press_char(&mut engine, 'Y');

        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "second\n");
        assert!(is_linewise);
    }

    #[test]
    fn test_paste_after_linewise() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2");
        engine.update_syntax();

        // Yank line1
        press_char(&mut engine, 'y');
        press_char(&mut engine, 'y');

        // Paste after (p) - should insert below current line
        press_char(&mut engine, 'p');

        assert_eq!(engine.buffer().to_string(), "line1\nline1\nline2");
        assert_eq!(engine.view().cursor.line, 1); // cursor on pasted line
    }

    #[test]
    fn test_paste_before_linewise() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2");
        engine.update_syntax();

        press_char(&mut engine, 'j'); // move to line2
        press_char(&mut engine, 'y');
        press_char(&mut engine, 'y'); // yank line2

        press_char(&mut engine, 'k'); // back to line1
        press_char(&mut engine, 'P'); // paste before

        assert_eq!(engine.buffer().to_string(), "line2\nline1\nline2");
        assert_eq!(engine.view().cursor.line, 0);
    }

    #[test]
    fn test_delete_x_fills_register() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ABC");
        engine.update_syntax();

        press_char(&mut engine, 'x'); // delete 'A'

        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "A");
        assert!(!is_linewise);
    }

    #[test]
    fn test_delete_dd_fills_register() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "first\nsecond\nthird");
        engine.update_syntax();

        press_char(&mut engine, 'j'); // move to "second"
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'd'); // delete line

        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "second\n");
        assert!(is_linewise);
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_delete_D_fills_register() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        press_char(&mut engine, 'l');
        press_char(&mut engine, 'l'); // cursor on 'l'
        press_char(&mut engine, 'D'); // delete to end

        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "llo world");
        assert!(!is_linewise);
    }

    #[test]
    fn test_named_register_yank() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test line");
        engine.update_syntax();

        // Use "a register
        press_char(&mut engine, '"');
        press_char(&mut engine, 'a');
        press_char(&mut engine, 'y');
        press_char(&mut engine, 'y');

        // Check 'a' register has content
        let (content, _) = engine.registers.get(&'a').unwrap();
        assert_eq!(content, "test line\n");

        // Unnamed register should also have it
        let (content2, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(content2, "test line\n");
    }

    #[test]
    fn test_named_register_paste() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "AAA\nBBB");
        engine.update_syntax();

        // Yank to "a
        press_char(&mut engine, '"');
        press_char(&mut engine, 'a');
        press_char(&mut engine, 'y');
        press_char(&mut engine, 'y');

        // Move down and yank to "b
        press_char(&mut engine, 'j');
        press_char(&mut engine, '"');
        press_char(&mut engine, 'b');
        press_char(&mut engine, 'y');
        press_char(&mut engine, 'y');

        // Now paste from "a
        press_char(&mut engine, '"');
        press_char(&mut engine, 'a');
        press_char(&mut engine, 'p');

        assert!(engine.buffer().to_string().contains("AAA"));
    }

    #[test]
    fn test_delete_and_paste_workflow() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3");
        engine.update_syntax();

        // Delete line2 with dd
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'd');

        assert_eq!(engine.buffer().to_string(), "line1\nline3");

        // Paste it back
        press_char(&mut engine, 'p');

        assert_eq!(engine.buffer().to_string(), "line1\nline3\nline2\n");
    }

    #[test]
    fn test_x_delete_and_paste() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ABCD");
        engine.update_syntax();

        press_char(&mut engine, 'x'); // delete 'A'
        press_char(&mut engine, 'l');
        press_char(&mut engine, 'l'); // cursor after 'D'
        press_char(&mut engine, 'p'); // paste after

        assert_eq!(engine.buffer().to_string(), "BCDA");
    }

    #[test]
    fn test_replace_char_basic() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello");

        // Replace 'h' with 'j'
        press_char(&mut engine, 'r');
        press_char(&mut engine, 'j');

        assert_eq!(engine.buffer().to_string(), "jello");
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_replace_char_with_count() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello");

        // Replace 3 chars with 'x': "xxxlo"
        press_char(&mut engine, '3');
        press_char(&mut engine, 'r');
        press_char(&mut engine, 'x');

        assert_eq!(engine.buffer().to_string(), "xxxlo");
        // Cursor should stay at starting position
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_replace_char_at_line_end() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test");

        // Move to last char
        engine.view_mut().cursor.col = 3;

        // Replace 't' with 'x'
        press_char(&mut engine, 'r');
        press_char(&mut engine, 'x');

        assert_eq!(engine.buffer().to_string(), "tesx");
    }

    #[test]
    fn test_replace_char_doesnt_cross_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hi\nbye");

        // Move to 'i' (last char of first line)
        engine.view_mut().cursor.col = 1;

        // Try to replace 3 chars - should only replace 'i' (not crossing newline)
        press_char(&mut engine, '3');
        press_char(&mut engine, 'r');
        press_char(&mut engine, 'x');

        assert_eq!(engine.buffer().to_string(), "hx\nbye");
    }

    #[test]
    fn test_replace_char_with_space() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello");

        // Replace 'h' with space
        press_char(&mut engine, 'r');
        press_char(&mut engine, ' ');

        assert_eq!(engine.buffer().to_string(), " ello");
    }

    #[test]
    fn test_replace_char_with_digit() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello");

        // r followed by a digit should replace with that digit, not treat it as count
        press_char(&mut engine, 'r');
        press_char(&mut engine, '1');

        assert_eq!(engine.buffer().to_string(), "1ello");
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_replace_char_repeat() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello");

        // Replace 'h' with 'j'
        press_char(&mut engine, 'r');
        press_char(&mut engine, 'j');
        assert_eq!(engine.buffer().to_string(), "jello");

        // Move forward and repeat
        press_char(&mut engine, 'l');
        press_char(&mut engine, '.');

        assert_eq!(engine.buffer().to_string(), "jjllo");
    }

    #[test]
    fn test_replace_char_multicount_repeat() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");

        // Replace 2 chars with 'x'
        press_char(&mut engine, '2');
        press_char(&mut engine, 'r');
        press_char(&mut engine, 'x');
        assert_eq!(engine.buffer().to_string(), "xxllo world");

        // Move forward and repeat (should replace 2 chars again)
        engine.view_mut().cursor.col = 6;
        press_char(&mut engine, '.');

        assert_eq!(engine.buffer().to_string(), "xxllo xxrld");
    }

    #[test]
    fn test_paste_empty_register() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test");
        engine.update_syntax();

        // Try to paste from empty register - should do nothing
        press_char(&mut engine, 'p');

        assert_eq!(engine.buffer().to_string(), "test");
    }

    #[test]
    fn test_yank_last_line_no_newline() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "first\nlast");
        engine.update_syntax();

        press_char(&mut engine, 'j'); // move to "last" (no trailing newline)
        press_char(&mut engine, 'y');
        press_char(&mut engine, 'y');

        // Should still be linewise with newline added
        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "last\n");
        assert!(is_linewise);
    }

    // --- Visual Mode Tests ---

    #[test]
    fn test_enter_visual_mode() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Enter visual mode with v
        press_char(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::Visual);
        assert!(engine.visual_anchor.is_some());
        assert_eq!(engine.visual_anchor.unwrap().line, 0);
        assert_eq!(engine.visual_anchor.unwrap().col, 0);
    }

    #[test]
    fn test_enter_visual_line_mode() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2");
        engine.update_syntax();

        // Enter visual line mode with V
        press_char(&mut engine, 'V');
        assert_eq!(engine.mode, Mode::VisualLine);
        assert!(engine.visual_anchor.is_some());
    }

    #[test]
    fn test_visual_mode_escape_exits() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test");
        engine.update_syntax();

        press_char(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::Visual);

        press_special(&mut engine, "Escape");
        assert_eq!(engine.mode, Mode::Normal);
        assert!(engine.visual_anchor.is_none());
    }

    #[test]
    fn test_visual_yank_forward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Select "hello" (5 chars)
        press_char(&mut engine, 'v');
        for _ in 0..4 {
            press_char(&mut engine, 'l');
        }

        // Yank
        press_char(&mut engine, 'y');

        // Check register
        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "hello");
        assert!(!is_linewise);

        // Should be back in normal mode
        assert_eq!(engine.mode, Mode::Normal);
        assert!(engine.visual_anchor.is_none());
    }

    #[test]
    fn test_visual_yank_backward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Move to 'w' (position 6)
        for _ in 0..6 {
            press_char(&mut engine, 'l');
        }

        // Select backward to 'h'
        press_char(&mut engine, 'v');
        for _ in 0..6 {
            press_char(&mut engine, 'h');
        }

        // Yank
        press_char(&mut engine, 'y');

        // Should yank "hello " (anchor at 6, cursor at 0, inclusive)
        let (content, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "hello w");
    }

    #[test]
    fn test_visual_delete() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Select "hello"
        press_char(&mut engine, 'v');
        for _ in 0..4 {
            press_char(&mut engine, 'l');
        }

        // Delete
        press_char(&mut engine, 'd');

        assert_eq!(engine.buffer().to_string(), " world");
        assert_eq!(engine.mode, Mode::Normal);

        // Check register
        let (content, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "hello");
    }

    #[test]
    fn test_visual_line_yank() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3");
        engine.update_syntax();

        // Select 2 lines
        press_char(&mut engine, 'V');
        press_char(&mut engine, 'j');

        // Yank
        press_char(&mut engine, 'y');

        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "line1\nline2\n");
        assert!(is_linewise);
    }

    #[test]
    fn test_visual_line_delete() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3");
        engine.update_syntax();

        // Select middle line
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'V');

        // Delete
        press_char(&mut engine, 'd');

        assert_eq!(engine.buffer().to_string(), "line1\nline3");
        assert_eq!(engine.view().cursor.line, 1); // cursor at start of next line
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_visual_change() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Select "hello"
        press_char(&mut engine, 'v');
        for _ in 0..4 {
            press_char(&mut engine, 'l');
        }

        // Change (should delete and enter insert mode)
        press_char(&mut engine, 'c');

        assert_eq!(engine.buffer().to_string(), " world");
        assert_eq!(engine.mode, Mode::Insert);
        assert_eq!(engine.view().cursor.col, 0);

        // Type replacement
        for ch in "hi".chars() {
            press_char(&mut engine, ch);
        }
        press_special(&mut engine, "Escape");

        assert_eq!(engine.buffer().to_string(), "hi world");
    }

    #[test]
    fn test_visual_line_change() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3");
        engine.update_syntax();

        press_char(&mut engine, 'V');
        press_char(&mut engine, 'c');

        assert_eq!(engine.buffer().to_string(), "line2\nline3");
        assert_eq!(engine.mode, Mode::Insert);
    }

    #[test]
    fn test_visual_mode_navigation() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        press_char(&mut engine, 'v');
        assert_eq!(engine.view().cursor.col, 0);

        // Move right extends selection
        press_char(&mut engine, 'l');
        assert_eq!(engine.view().cursor.col, 1);
        assert_eq!(engine.mode, Mode::Visual); // still in visual mode

        press_char(&mut engine, 'l');
        press_char(&mut engine, 'l');
        assert_eq!(engine.view().cursor.col, 3);
    }

    #[test]
    fn test_visual_mode_switching() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2");
        engine.update_syntax();

        // Start in character visual
        press_char(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::Visual);

        // Switch to line visual
        press_char(&mut engine, 'V');
        assert_eq!(engine.mode, Mode::VisualLine);
        assert!(engine.visual_anchor.is_some()); // anchor preserved

        // Press V again to exit
        press_char(&mut engine, 'V');
        assert_eq!(engine.mode, Mode::Normal);
        assert!(engine.visual_anchor.is_none());
    }

    #[test]
    fn test_visual_mode_toggle_with_v() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test");
        engine.update_syntax();

        // Enter visual mode
        press_char(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::Visual);

        // Press v again to exit
        press_char(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::Normal);
    }

    #[test]
    fn test_visual_multiline_selection() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3");
        engine.update_syntax();

        // Select from beginning of line1 to middle of line2
        press_char(&mut engine, 'v');
        press_char(&mut engine, 'j'); // move to line 2
        for _ in 0..2 {
            press_char(&mut engine, 'l'); // move right 2 chars
        }

        press_char(&mut engine, 'y');

        let (content, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "line1\nlin");
    }

    #[test]
    fn test_visual_with_named_register() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Select text and yank to register 'a'
        press_char(&mut engine, '"');
        press_char(&mut engine, 'a');
        press_char(&mut engine, 'v');
        for _ in 0..4 {
            press_char(&mut engine, 'l');
        }
        press_char(&mut engine, 'y');

        // Check register 'a'
        let (content, _) = engine.registers.get(&'a').unwrap();
        assert_eq!(content, "hello");

        // Also in unnamed register
        let (content, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "hello");
    }

    #[test]
    fn test_visual_word_motion() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world foo bar");
        engine.update_syntax();

        // Select with word motion
        press_char(&mut engine, 'v');
        press_char(&mut engine, 'w'); // cursor moves to 'w' (start of "world")
        press_char(&mut engine, 'w'); // cursor moves to 'f' (start of "foo")

        press_char(&mut engine, 'y');

        // Visual mode is inclusive, so we get from 'h' to 'f' inclusive
        let (content, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "hello world f");
    }

    #[test]
    fn test_visual_line_multiple_lines() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "a\nb\nc\nd\ne");
        engine.update_syntax();

        // Move to line 2 (b)
        press_char(&mut engine, 'j');

        // Select 3 lines (b, c, d)
        press_char(&mut engine, 'V');
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'j');

        press_char(&mut engine, 'd');

        assert_eq!(engine.buffer().to_string(), "a\ne");
        assert_eq!(engine.view().cursor.line, 1);
    }

    // ===================================================================
    // Count infrastructure tests (Step 1)
    // ===================================================================

    #[test]
    fn test_count_accumulation() {
        let mut engine = Engine::new();
        press_char(&mut engine, '1');
        assert_eq!(engine.peek_count(), Some(1));
        press_char(&mut engine, '2');
        assert_eq!(engine.peek_count(), Some(12));
        press_char(&mut engine, '3');
        assert_eq!(engine.peek_count(), Some(123));
        assert_eq!(engine.take_count(), 123);
        assert_eq!(engine.peek_count(), None);
    }

    #[test]
    fn test_zero_goes_to_line_start() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "Hello world");
        engine.view_mut().cursor.col = 5;
        assert_eq!(engine.view().cursor.col, 5);

        press_char(&mut engine, '0');
        assert_eq!(engine.view().cursor.col, 0);
        assert_eq!(engine.peek_count(), None);
    }

    #[test]
    fn test_count_with_zero() {
        let mut engine = Engine::new();
        press_char(&mut engine, '1');
        assert_eq!(engine.peek_count(), Some(1));
        press_char(&mut engine, '0');
        assert_eq!(engine.peek_count(), Some(10));

        // take_count() should return 10 and clear
        assert_eq!(engine.take_count(), 10);
        assert_eq!(engine.peek_count(), None);
    }

    #[test]
    fn test_count_max_limit() {
        let mut engine = Engine::new();
        // Type 99999 to exceed 10,000 limit
        for ch in ['9', '9', '9', '9', '9'] {
            press_char(&mut engine, ch);
        }
        assert_eq!(engine.peek_count(), Some(10_000));
        assert!(engine.message.contains("limit") || engine.message.contains("10,000"));
    }

    #[test]
    fn test_count_display() {
        let mut engine = Engine::new();
        press_char(&mut engine, '5');

        // peek_count should not consume
        assert_eq!(engine.peek_count(), Some(5));
        assert_eq!(engine.peek_count(), Some(5));
        assert_eq!(engine.peek_count(), Some(5));

        // take_count should consume
        assert_eq!(engine.take_count(), 5);
        assert_eq!(engine.peek_count(), None);
    }

    #[test]
    fn test_count_cleared_on_escape() {
        let mut engine = Engine::new();
        press_char(&mut engine, '5');
        assert_eq!(engine.peek_count(), Some(5));

        press_special(&mut engine, "Escape");
        assert_eq!(engine.peek_count(), None);
    }

    // --- Count-based motion tests (Step 2) ---

    #[test]
    fn test_count_hjkl_motions() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "ABCDEFGH\nIJKLMNOP\nQRSTUVWX\nYZ");
        engine.update_syntax();

        // Test 5l - move right 5 times
        press_char(&mut engine, '5');
        press_char(&mut engine, 'l');
        assert_eq!(engine.view().cursor.col, 5);
        assert_eq!(engine.peek_count(), None); // count consumed

        // Test 2j - move down 2 times
        press_char(&mut engine, '2');
        press_char(&mut engine, 'j');
        assert_eq!(engine.view().cursor.line, 2);

        // Test 3h - move left 3 times
        press_char(&mut engine, '3');
        press_char(&mut engine, 'h');
        assert_eq!(engine.view().cursor.col, 2);

        // Test 1k - move up 1 time
        press_char(&mut engine, '1');
        press_char(&mut engine, 'k');
        assert_eq!(engine.view().cursor.line, 1);
    }

    #[test]
    fn test_count_arrow_keys() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "ABCDEFGH\nIJKLMNOP\nQRSTUVWX");
        engine.update_syntax();

        // Test 3 Right
        press_char(&mut engine, '3');
        press_special(&mut engine, "Right");
        assert_eq!(engine.view().cursor.col, 3);

        // Test 2 Down
        press_char(&mut engine, '2');
        press_special(&mut engine, "Down");
        assert_eq!(engine.view().cursor.line, 2);

        // Test 2 Up
        press_char(&mut engine, '2');
        press_special(&mut engine, "Up");
        assert_eq!(engine.view().cursor.line, 0);

        // Test 2 Left
        press_char(&mut engine, '2');
        press_special(&mut engine, "Left");
        assert_eq!(engine.view().cursor.col, 1);
    }

    #[test]
    fn test_count_word_motions() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "one two three four five six seven");
        engine.update_syntax();

        // Test 3w - move forward 3 words
        press_char(&mut engine, '3');
        press_char(&mut engine, 'w');
        // Should be at start of "four"
        assert_eq!(engine.view().cursor.col, 14);

        // Test 2b - move backward 2 words
        press_char(&mut engine, '2');
        press_char(&mut engine, 'b');
        // Should be at start of "two"
        assert_eq!(engine.view().cursor.col, 4);

        // Test 2e - move to end of 2nd word from here
        press_char(&mut engine, '2');
        press_char(&mut engine, 'e');
        // Should be at end of "three"
        assert_eq!(engine.view().cursor.col, 12);
    }

    #[test]
    fn test_count_paragraph_motions() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "para1\npara1\n\npara2\npara2\n\npara3\n\npara4");
        engine.update_syntax();
        // Line 0: para1
        // Line 1: para1
        // Line 2: empty
        // Line 3: para2
        // Line 4: para2
        // Line 5: empty
        // Line 6: para3
        // Line 7: empty
        // Line 8: para4

        // Test 2} - move forward 2 empty lines
        press_char(&mut engine, '2');
        press_char(&mut engine, '}');
        assert_eq!(engine.view().cursor.line, 5);

        // Test 1{ - move backward 1 empty line
        press_char(&mut engine, '1');
        press_char(&mut engine, '{');
        assert_eq!(engine.view().cursor.line, 2);

        // Test 2{ - move backward 2 empty lines (but there's only line 0 before)
        press_char(&mut engine, '2');
        press_char(&mut engine, '{');
        assert_eq!(engine.view().cursor.line, 0);
    }

    #[test]
    fn test_count_scroll_commands() {
        let mut engine = Engine::new();
        // Create a buffer with 100 lines
        let mut text = String::new();
        for i in 0..100 {
            text.push_str(&format!("Line {}\n", i));
        }
        engine.buffer_mut().insert(0, &text);
        engine.update_syntax();
        engine.set_viewport_lines(20); // Simulate 20 lines visible

        // Test 2 Ctrl-D (2 half-pages down = 20 lines)
        press_char(&mut engine, '2');
        press_ctrl(&mut engine, 'd');
        assert_eq!(engine.view().cursor.line, 20);

        // Test 1 Ctrl-U (1 half-page up = 10 lines)
        press_char(&mut engine, '1');
        press_ctrl(&mut engine, 'u');
        assert_eq!(engine.view().cursor.line, 10);

        // Test 3 Ctrl-F (3 full pages down = 60 lines)
        press_char(&mut engine, '3');
        press_ctrl(&mut engine, 'f');
        assert_eq!(engine.view().cursor.line, 70);

        // Test 2 Ctrl-B (2 full pages up = 40 lines)
        press_char(&mut engine, '2');
        press_ctrl(&mut engine, 'b');
        assert_eq!(engine.view().cursor.line, 30);
    }

    #[test]
    fn test_count_motion_bounds_checking() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ABC\nDEF");
        engine.update_syntax();

        // Test 100l - should stop at line end
        press_char(&mut engine, '1');
        press_char(&mut engine, '0');
        press_char(&mut engine, '0');
        press_char(&mut engine, 'l');
        assert!(engine.view().cursor.col <= 2);

        // Test 100j - should stop at last line
        press_char(&mut engine, '1');
        press_char(&mut engine, '0');
        press_char(&mut engine, '0');
        press_char(&mut engine, 'j');
        assert_eq!(engine.view().cursor.line, 1);
    }

    #[test]
    fn test_count_large_values() {
        let mut engine = Engine::new();
        // Create text with many words
        let text = "a b c d e f g h i j k l m n o p q r s t u v w x y z";
        engine.buffer_mut().insert(0, text);
        engine.update_syntax();

        // Test 10w - move forward 10 words
        press_char(&mut engine, '1');
        press_char(&mut engine, '0');
        press_char(&mut engine, 'w');
        // Should be at 'k' (10th word from start)
        assert_eq!(engine.view().cursor.col, 20);
    }

    // --- Count-based line operation tests (Step 3) ---

    #[test]
    fn test_count_x_delete_chars() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ABCDEFGH");
        engine.update_syntax();

        // Test 3x - delete 3 characters
        press_char(&mut engine, '3');
        press_char(&mut engine, 'x');
        assert_eq!(engine.buffer().to_string(), "DEFGH");

        // Check register contains deleted chars
        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "ABC");
        assert!(!is_linewise);
    }

    #[test]
    fn test_count_x_bounds() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ABC");
        engine.update_syntax();

        // Test 100x - should only delete 3 chars (all available)
        press_char(&mut engine, '1');
        press_char(&mut engine, '0');
        press_char(&mut engine, '0');
        press_char(&mut engine, 'x');
        assert_eq!(engine.buffer().to_string(), "");
    }

    #[test]
    fn test_count_dd_delete_lines() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "line1\nline2\nline3\nline4\nline5");
        engine.update_syntax();

        // Test 3dd - delete 3 lines
        press_char(&mut engine, '3');
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'd');
        assert_eq!(engine.buffer().to_string(), "line4\nline5");

        // Check register contains all 3 lines
        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "line1\nline2\nline3\n");
        assert!(is_linewise);
    }

    #[test]
    fn test_count_yy_yank_lines() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "alpha\nbeta\ngamma\ndelta");
        engine.update_syntax();

        // Test 2yy - yank 2 lines
        press_char(&mut engine, '2');
        press_char(&mut engine, 'y');
        press_char(&mut engine, 'y');

        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "alpha\nbeta\n");
        assert!(is_linewise);
        assert!(engine.message.contains("2 lines yanked"));

        // Buffer should be unchanged
        assert_eq!(engine.buffer().to_string(), "alpha\nbeta\ngamma\ndelta");
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_count_Y_yank_lines() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "one\ntwo\nthree\nfour");
        engine.update_syntax();

        // Test 3Y - yank 3 lines
        press_char(&mut engine, '3');
        press_char(&mut engine, 'Y');

        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "one\ntwo\nthree\n");
        assert!(is_linewise);
        assert!(engine.message.contains("3 lines yanked"));
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_count_D_delete_to_eol() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "ABCDEFGH\nIJKLMNOP\nQRSTUVWX\nYZ");
        engine.update_syntax();

        // Move to column 2 of first line
        press_char(&mut engine, 'l');
        press_char(&mut engine, 'l');
        assert_eq!(engine.view().cursor.col, 2);

        // Test 2D - delete to end of line + 1 more full line
        press_char(&mut engine, '2');
        press_char(&mut engine, 'D');

        // Should delete "CDEFGH\nIJKLMNOP\n" (to EOL + next line)
        assert_eq!(engine.buffer().to_string(), "AB\nQRSTUVWX\nYZ");

        let (content, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "CDEFGH\nIJKLMNOP\n");
    }

    #[test]
    fn test_count_dd_last_lines() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3");
        engine.update_syntax();

        // Move to line 2 (0-indexed: line 1)
        press_char(&mut engine, 'j');

        // Test 5dd - delete more lines than available (should delete 2 lines)
        press_char(&mut engine, '5');
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'd');

        assert_eq!(engine.buffer().to_string(), "line1");
    }

    #[test]
    fn test_count_yy_last_lines() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "A\nB\nC");
        engine.update_syntax();

        // Move to line B
        press_char(&mut engine, 'j');

        // Test 10yy - yank more than available (should yank 2 lines: B and C)
        press_char(&mut engine, '1');
        press_char(&mut engine, '0');
        press_char(&mut engine, 'y');
        press_char(&mut engine, 'y');

        let (content, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "B\nC\n");
    }

    // Step 4 tests: Special commands and mode changes

    #[test]
    fn test_count_G_goto_line() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "line1\nline2\nline3\nline4\nline5");
        engine.update_syntax();

        // Start at line 0
        assert_eq!(engine.view().cursor.line, 0);

        // Test 3G - go to line 3 (1-indexed, so line index 2)
        press_char(&mut engine, '3');
        press_char(&mut engine, 'G');

        assert_eq!(engine.view().cursor.line, 2);

        // Test G with no count - go to last line
        press_char(&mut engine, 'G');
        assert_eq!(engine.view().cursor.line, 4);

        // Test 1G - go to first line
        press_char(&mut engine, '1');
        press_char(&mut engine, 'G');
        assert_eq!(engine.view().cursor.line, 0);
    }

    #[test]
    fn test_count_gg_goto_line() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "line1\nline2\nline3\nline4\nline5");
        engine.update_syntax();

        // Move to last line
        press_char(&mut engine, 'G');
        assert_eq!(engine.view().cursor.line, 4);

        // Test 2gg - go to line 2 (1-indexed, so line index 1)
        press_char(&mut engine, '2');
        press_char(&mut engine, 'g');
        press_char(&mut engine, 'g');

        assert_eq!(engine.view().cursor.line, 1);

        // Test gg with no count - go to first line
        press_char(&mut engine, 'G');
        press_char(&mut engine, 'g');
        press_char(&mut engine, 'g');
        assert_eq!(engine.view().cursor.line, 0);
    }

    #[test]
    fn test_count_paste() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello");
        engine.update_syntax();

        // Yank "hello"
        press_char(&mut engine, 'y');
        press_char(&mut engine, 'y');

        // Move to next line (insert blank line)
        press_char(&mut engine, 'o');
        press_special(&mut engine, "Escape");

        // Test 3p - paste 3 times
        press_char(&mut engine, '3');
        press_char(&mut engine, 'p');

        // Should have: hello\n + 3 copies of "hello\n"
        let text = engine.buffer().to_string();
        assert_eq!(text, "hello\n\nhello\nhello\nhello\n");
    }

    #[test]
    fn test_count_search_next() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "x\nx\nx\nx\nx");
        engine.update_syntax();

        // Search for "x" - should find 5 matches (one per line)
        press_char(&mut engine, '/');
        press_char(&mut engine, 'x');
        press_special(&mut engine, "Return");

        // After search from line 0, we jump to first match after cursor (line 1, since line 0 col 0 has 'x' but search looks AFTER cursor)
        // Actually, search should jump to line 0 if that's the first match
        // Let me check: cursor starts at 0,0. Search for 'x' finds match at 0,0
        // But search_next looks for matches > cursor position
        // So it finds line 1 as first match > position 0
        let first_line = engine.view().cursor.line;
        assert_eq!(engine.search_matches.len(), 5);

        // Test 3n - should move forward 3 more times
        press_char(&mut engine, '3');
        press_char(&mut engine, 'n');

        // Should have moved forward 3 times from first_line
        assert_eq!(engine.view().cursor.line, first_line + 3);

        // Test 2N - should move backward 2 times
        press_char(&mut engine, '2');
        press_char(&mut engine, 'N');

        // Should be back 2 lines
        assert_eq!(engine.view().cursor.line, first_line + 1);
    }

    #[test]
    fn test_count_cleared_on_insert_mode() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello");
        engine.update_syntax();

        // Set count to 5
        press_char(&mut engine, '5');
        assert_eq!(engine.peek_count(), Some(5));

        // Enter insert mode with 'i'
        press_char(&mut engine, 'i');
        assert_eq!(engine.peek_count(), None);

        // Exit insert mode
        press_special(&mut engine, "Escape");

        // Set count again
        press_char(&mut engine, '3');
        assert_eq!(engine.peek_count(), Some(3));

        // Enter insert mode with 'a'
        press_char(&mut engine, 'a');
        assert_eq!(engine.peek_count(), None);

        // Exit and test 'A'
        press_special(&mut engine, "Escape");
        press_char(&mut engine, '7');
        press_char(&mut engine, 'A');
        assert_eq!(engine.peek_count(), None);

        // Exit and test 'I'
        press_special(&mut engine, "Escape");
        press_char(&mut engine, '9');
        press_char(&mut engine, 'I');
        assert_eq!(engine.peek_count(), None);
    }

    #[test]
    fn test_count_cleared_on_mode_changes() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Test visual mode PRESERVES count (for use with motions)
        press_char(&mut engine, '5');
        assert_eq!(engine.peek_count(), Some(5));
        press_char(&mut engine, 'v');
        assert_eq!(engine.peek_count(), Some(5)); // Count preserved
        press_special(&mut engine, "Escape"); // Escape clears count

        // Test visual line mode PRESERVES count (for use with motions)
        press_char(&mut engine, '3');
        assert_eq!(engine.peek_count(), Some(3));
        press_char(&mut engine, 'V');
        assert_eq!(engine.peek_count(), Some(3)); // Count preserved
        press_special(&mut engine, "Escape"); // Escape clears count

        // Test command mode clears count
        press_char(&mut engine, '7');
        assert_eq!(engine.peek_count(), Some(7));
        press_char(&mut engine, ':');
        assert_eq!(engine.peek_count(), None);
        press_special(&mut engine, "Escape");

        // Test search mode clears count
        press_char(&mut engine, '9');
        assert_eq!(engine.peek_count(), Some(9));
        press_char(&mut engine, '/');
        assert_eq!(engine.peek_count(), None);
        press_special(&mut engine, "Escape");
    }

    #[test]
    fn test_count_visual_motion() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(
            0,
            "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8",
        );
        engine.update_syntax();

        // Start at line 0
        assert_eq!(engine.view().cursor.line, 0);

        // Enter visual mode
        press_char(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::Visual);

        // Test 5j - should extend selection 5 lines down
        press_char(&mut engine, '5');
        press_char(&mut engine, 'j');
        assert_eq!(engine.view().cursor.line, 5);
        assert_eq!(engine.mode, Mode::Visual); // Should still be in visual mode

        // Test 2k - should move up 2 lines
        press_char(&mut engine, '2');
        press_char(&mut engine, 'k');
        assert_eq!(engine.view().cursor.line, 3);

        // Exit visual mode
        press_special(&mut engine, "Escape");
        assert_eq!(engine.mode, Mode::Normal);
    }

    #[test]
    fn test_count_visual_word() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "one two three four five six seven eight");
        engine.update_syntax();

        // Start at beginning
        assert_eq!(engine.view().cursor, Cursor { line: 0, col: 0 });

        // Enter visual mode
        press_char(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::Visual);

        // Test 3w - should extend by 3 words
        press_char(&mut engine, '3');
        press_char(&mut engine, 'w');

        // After 3 word-forwards from position 0, we should be at "four"
        // one(0) -> two(4) -> three(8) -> four(14)
        assert_eq!(engine.view().cursor.col, 14);

        // Test 2b - should move back 2 words
        press_char(&mut engine, '2');
        press_char(&mut engine, 'b');

        // four(14) -> three(8) -> two(4)
        assert_eq!(engine.view().cursor.col, 4);

        // Exit visual mode
        press_special(&mut engine, "Escape");
        assert_eq!(engine.mode, Mode::Normal);
    }

    #[test]
    fn test_count_visual_line_mode() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7");
        engine.update_syntax();

        // Start at line 0
        assert_eq!(engine.view().cursor.line, 0);

        // Enter visual line mode
        press_char(&mut engine, 'V');
        assert_eq!(engine.mode, Mode::VisualLine);

        // Test 3j - should extend selection 3 lines down
        press_char(&mut engine, '3');
        press_char(&mut engine, 'j');
        assert_eq!(engine.view().cursor.line, 3);
        assert_eq!(engine.mode, Mode::VisualLine);

        // Yank the selection
        press_char(&mut engine, 'y');
        assert_eq!(engine.mode, Mode::Normal);

        // Should have yanked 4 lines (lines 0-3)
        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert!(is_linewise);
        assert!(content.contains("line 1"));
        assert!(content.contains("line 4"));
    }

    #[test]
    fn test_count_not_applied_to_visual_operators() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "line 1\nline 2\nline 3\nline 4\nline 5");
        engine.update_syntax();

        // Start at line 0
        assert_eq!(engine.view().cursor.line, 0);

        // Enter visual mode
        press_char(&mut engine, 'v');

        // Move down 2 lines to create selection
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'j');
        assert_eq!(engine.view().cursor.line, 2);

        // Now type "3" then "d" - should delete the selection ONCE, not 3 times
        press_char(&mut engine, '3');
        assert_eq!(engine.peek_count(), Some(3));

        press_char(&mut engine, 'd');

        // Should be back in normal mode
        assert_eq!(engine.mode, Mode::Normal);

        // Count should be cleared (not applied to operator)
        assert_eq!(engine.peek_count(), None);

        // Buffer should have deleted lines 0-2 (3 lines), leaving lines 3-4
        let text = engine.buffer().to_string();
        assert!(text.contains("line 4"));
        assert!(text.contains("line 5"));
        assert!(!text.contains("line 1"));
        assert!(!text.contains("line 2"));
        assert!(!text.contains("line 3"));
    }

    #[test]
    fn test_config_reload() {
        use std::fs;
        use std::path::PathBuf;

        // Get config file path
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let config_path = PathBuf::from(&home)
            .join(".config")
            .join("vimcode")
            .join("settings.json");

        // Save original settings
        let original_settings = fs::read_to_string(&config_path).ok();

        // Create config directory
        if let Some(parent) = config_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Test 1: Successful reload with valid JSON
        let test_settings = r#"{"line_numbers":"Absolute"}"#;
        fs::write(&config_path, test_settings).unwrap();

        let mut engine = Engine::new();
        engine.execute_command("config reload");

        assert_eq!(engine.settings.line_numbers, LineNumberMode::Absolute);
        assert_eq!(engine.message, "Settings reloaded successfully");

        // Test 2: Failed reload with invalid JSON
        fs::write(&config_path, "{ invalid json }").unwrap();
        let initial_settings = engine.settings.line_numbers;

        engine.execute_command("config reload");

        // Settings should be unchanged
        assert_eq!(engine.settings.line_numbers, initial_settings);
        assert!(engine.message.contains("Error reloading settings"));

        // Test 3: Failed reload with missing file
        let _ = fs::remove_file(&config_path);

        engine.execute_command("config reload");

        // Settings should still be unchanged
        assert_eq!(engine.settings.line_numbers, initial_settings);
        assert!(engine.message.contains("Error reloading settings"));

        // Restore original settings or clean up
        if let Some(original) = original_settings {
            fs::write(&config_path, original).unwrap();
        }
    }

    // --- Character find motion tests ---

    #[test]
    fn test_find_char_forward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abcdef");
        // Cursor at column 0, find 'd'
        press_char(&mut engine, 'f');
        press_char(&mut engine, 'd');
        assert_eq!(engine.view().cursor.col, 3);
    }

    #[test]
    fn test_find_char_forward_not_found() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abcdef");
        press_char(&mut engine, 'f');
        press_char(&mut engine, 'z');
        // Cursor should not move
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_find_char_backward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abcdef");
        // Move to column 5
        for _ in 0..5 {
            press_char(&mut engine, 'l');
        }
        assert_eq!(engine.view().cursor.col, 5);
        // Find 'b' backward
        press_char(&mut engine, 'F');
        press_char(&mut engine, 'b');
        assert_eq!(engine.view().cursor.col, 1);
    }

    #[test]
    fn test_till_char_forward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abcdef");
        // Cursor at column 0, till 'd' (stop before)
        press_char(&mut engine, 't');
        press_char(&mut engine, 'd');
        assert_eq!(engine.view().cursor.col, 2);
    }

    #[test]
    fn test_till_char_backward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abcdef");
        // Move to column 5
        for _ in 0..5 {
            press_char(&mut engine, 'l');
        }
        // Till 'b' backward (stop after)
        press_char(&mut engine, 'T');
        press_char(&mut engine, 'b');
        assert_eq!(engine.view().cursor.col, 2);
    }

    #[test]
    fn test_find_with_count() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ababab");
        // Find 2nd 'b'
        press_char(&mut engine, '2');
        press_char(&mut engine, 'f');
        press_char(&mut engine, 'b');
        assert_eq!(engine.view().cursor.col, 3);
    }

    #[test]
    fn test_repeat_find_forward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ababab");
        // Find first 'b'
        press_char(&mut engine, 'f');
        press_char(&mut engine, 'b');
        assert_eq!(engine.view().cursor.col, 1);
        // Repeat to find next 'b'
        press_char(&mut engine, ';');
        assert_eq!(engine.view().cursor.col, 3);
        // Repeat again
        press_char(&mut engine, ';');
        assert_eq!(engine.view().cursor.col, 5);
    }

    #[test]
    fn test_repeat_find_backward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ababab");
        // Move to end
        for _ in 0..5 {
            press_char(&mut engine, 'l');
        }
        // Find 'a' backward
        press_char(&mut engine, 'F');
        press_char(&mut engine, 'a');
        assert_eq!(engine.view().cursor.col, 4);
        // Repeat backward
        press_char(&mut engine, ';');
        assert_eq!(engine.view().cursor.col, 2);
    }

    #[test]
    fn test_repeat_find_reverse() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ababab");
        // Find 'b' forward
        press_char(&mut engine, 'f');
        press_char(&mut engine, 'b');
        assert_eq!(engine.view().cursor.col, 1);
        // Reverse direction (go back to 'b' at col 1, but we're already there)
        // So it should not find anything before col 1
        let prev_col = engine.view().cursor.col;
        press_char(&mut engine, ',');
        // Should stay at same position (no 'b' before col 1)
        assert_eq!(engine.view().cursor.col, prev_col);
    }

    #[test]
    fn test_find_does_not_cross_lines() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc\nxyz");
        // Cursor at line 0, col 0
        // Try to find 'x' (which is on next line)
        press_char(&mut engine, 'f');
        press_char(&mut engine, 'x');
        // Should not move (find is within-line only)
        assert_eq!(engine.view().cursor.line, 0);
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_repeat_with_count() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ababab");
        // Find first 'b'
        press_char(&mut engine, 'f');
        press_char(&mut engine, 'b');
        assert_eq!(engine.view().cursor.col, 1);
        // Repeat twice with count
        press_char(&mut engine, '2');
        press_char(&mut engine, ';');
        assert_eq!(engine.view().cursor.col, 5);
    }

    // --- Tests for delete/change operators (Step 2) ---

    #[test]
    fn test_dw_delete_word() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world foo bar");
        engine.update_syntax();
        assert_eq!(engine.view().cursor, Cursor { line: 0, col: 0 });

        // dw should delete "hello "
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'w');

        assert_eq!(engine.buffer().to_string(), "world foo bar");
        assert_eq!(engine.view().cursor, Cursor { line: 0, col: 0 });

        // Check register
        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "hello ");
        assert!(!is_linewise);
    }

    #[test]
    fn test_db_delete_backward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world foo");
        engine.update_syntax();

        // Move to space after "world" (before "foo")
        // "hello world foo" -> cols: h=0, e=1, ..., d=10, ' '=11, f=12
        engine.view_mut().cursor.col = 12;

        // db from 'f' should delete backward to start of word
        // It will go back to col 6 ('w'), so it deletes "world "
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'b');

        assert_eq!(engine.buffer().to_string(), "hello foo");
        assert_eq!(engine.view().cursor.col, 6);
    }

    #[test]
    fn test_de_delete_to_end() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // de from start should delete "hello"
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'e');

        assert_eq!(engine.buffer().to_string(), " world");
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_cw_change_word() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // cw behaves like ce (Vim compatibility) - deletes "hello" only
        press_char(&mut engine, 'c');
        press_char(&mut engine, 'w');

        assert_eq!(engine.buffer().to_string(), " world");
        assert_eq!(engine.mode, Mode::Insert);
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_cb_change_backward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Move to 'w' in "world"
        engine.view_mut().cursor.col = 6;

        // cb from 'w' should go back to start of previous word ('h')
        // So it deletes "hello " and leaves "world"
        press_char(&mut engine, 'c');
        press_char(&mut engine, 'b');

        assert_eq!(engine.buffer().to_string(), "world");
        assert_eq!(engine.mode, Mode::Insert);
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_ce_change_to_end() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // ce should delete "hello" and enter insert mode
        press_char(&mut engine, 'c');
        press_char(&mut engine, 'e');

        assert_eq!(engine.buffer().to_string(), " world");
        assert_eq!(engine.mode, Mode::Insert);
    }

    #[test]
    fn test_dw_with_count() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "one two three four");
        engine.update_syntax();

        // 2dw should delete "one two "
        press_char(&mut engine, '2');
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'w');

        assert_eq!(engine.buffer().to_string(), "three four");
    }

    #[test]
    fn test_cw_with_count() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "one two three");
        engine.update_syntax();

        // 2cw behaves like 2ce - deletes "one two" (not the trailing space)
        press_char(&mut engine, '2');
        press_char(&mut engine, 'c');
        press_char(&mut engine, 'w');

        assert_eq!(engine.buffer().to_string(), " three");
        assert_eq!(engine.mode, Mode::Insert);
    }

    #[test]
    fn test_cw_at_end_of_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello\nworld");
        engine.update_syntax();

        // cw at "hello" should NOT delete the newline
        press_char(&mut engine, 'c');
        press_char(&mut engine, 'w');

        assert_eq!(engine.buffer().to_string(), "\nworld");
        assert_eq!(engine.mode, Mode::Insert);
        assert_eq!(engine.view().cursor.line, 0);
    }

    #[test]
    fn test_cw_on_last_word() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc def");
        engine.update_syntax();

        // Move to 'd' in "def"
        engine.view_mut().cursor.col = 4;

        // cw should delete "def", leaving "abc " (with trailing space)
        press_char(&mut engine, 'c');
        press_char(&mut engine, 'w');

        assert_eq!(
            engine.buffer().to_string(),
            "abc ",
            "cw on last word should preserve preceding space"
        );
        assert_eq!(engine.mode, Mode::Insert);
    }

    #[test]
    fn test_ce_on_last_word() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc def");
        engine.update_syntax();

        // Move to 'd' in "def"
        engine.view_mut().cursor.col = 4;

        // ce should delete "def", leaving "abc " (with trailing space)
        press_char(&mut engine, 'c');
        press_char(&mut engine, 'e');

        assert_eq!(
            engine.buffer().to_string(),
            "abc ",
            "ce on last word should preserve preceding space"
        );
        assert_eq!(engine.mode, Mode::Insert);
    }

    #[test]
    fn test_s_substitute_char() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello");
        engine.update_syntax();

        // s should delete 'h' and enter insert mode
        press_char(&mut engine, 's');

        assert_eq!(engine.buffer().to_string(), "ello");
        assert_eq!(engine.mode, Mode::Insert);
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_s_with_count() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello");
        engine.update_syntax();

        // 3s should delete "hel" and enter insert mode
        press_char(&mut engine, '3');
        press_char(&mut engine, 's');

        assert_eq!(engine.buffer().to_string(), "lo");
        assert_eq!(engine.mode, Mode::Insert);
    }

    #[test]
    fn test_S_substitute_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Move cursor to middle
        engine.view_mut().cursor.col = 6;

        // S should delete entire line content and enter insert mode
        press_char(&mut engine, 'S');

        assert_eq!(engine.buffer().to_string(), "");
        assert_eq!(engine.mode, Mode::Insert);
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_C_change_to_eol() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Move to 'w'
        engine.view_mut().cursor.col = 6;

        // C should delete "world" and enter insert mode
        press_char(&mut engine, 'C');

        // After deleting "world", cursor stays at col 6
        // But the line is now "hello " (length 6), so cursor should clamp to col 5
        assert_eq!(engine.buffer().to_string(), "hello ");
        assert_eq!(engine.mode, Mode::Insert);
        // In insert mode, cursor can be at end of line
        assert!(engine.view().cursor.col >= 5);
    }

    #[test]
    fn test_dd_still_works() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3");
        engine.update_syntax();

        // dd should still work
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'd');

        assert_eq!(engine.buffer().to_string(), "line2\nline3");
    }

    #[test]
    fn test_cc_change_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // cc should delete line content and enter insert mode
        press_char(&mut engine, 'c');
        press_char(&mut engine, 'c');

        assert_eq!(engine.buffer().to_string(), "");
        assert_eq!(engine.mode, Mode::Insert);
    }

    #[test]
    fn test_operators_with_registers() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // "adw should delete into register 'a'
        press_char(&mut engine, '"');
        press_char(&mut engine, 'a');
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'w');

        let (content, _) = engine.registers.get(&'a').unwrap();
        assert_eq!(content, "hello ");
    }

    #[test]
    fn test_operators_undo_redo() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // dw
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'w');
        assert_eq!(engine.buffer().to_string(), "world");

        // Undo
        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "hello world");

        // Redo
        press_ctrl(&mut engine, 'r');
        assert_eq!(engine.buffer().to_string(), "world");
    }

    // --- Tests for ge motion ---

    #[test]
    fn test_ge_basic() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world test");
        engine.update_syntax();

        // Start at end of first word: "hello world test"
        //                                    ^
        engine.view_mut().cursor.col = 4;

        // ge should move to end of "hello" (already there, so go back to previous word end)
        // But since we're already at end of word, should go to previous
        press_char(&mut engine, 'g');
        press_char(&mut engine, 'e');

        // Should stay at position or move (depending on implementation)
        // Let's test from middle of word instead
    }

    #[test]
    fn test_ge_from_middle_of_word() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world test");
        engine.update_syntax();

        // Start in middle of "world": "hello world test"
        //                                      ^
        engine.view_mut().cursor.col = 8;

        // ge should move to end of "hello"
        press_char(&mut engine, 'g');
        press_char(&mut engine, 'e');

        assert_eq!(engine.view().cursor.col, 4); // End of "hello"
    }

    #[test]
    fn test_ge_with_count() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "one two three four");
        engine.update_syntax();

        // Start at "four": "one two three four"
        //                                 ^
        engine.view_mut().cursor.col = 14;

        // 2ge should move back 2 word ends: "three" -> "two" -> "one"
        press_char(&mut engine, '2');
        press_char(&mut engine, 'g');
        press_char(&mut engine, 'e');

        assert_eq!(engine.view().cursor.col, 2); // End of "one"
    }

    #[test]
    fn test_ge_at_start() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Start at beginning
        engine.view_mut().cursor.col = 0;

        // ge at start should not move
        press_char(&mut engine, 'g');
        press_char(&mut engine, 'e');

        assert_eq!(engine.view().cursor.col, 0);
    }

    // --- Tests for % motion ---

    #[test]
    fn test_percent_parentheses() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo(bar)baz");
        engine.update_syntax();

        // Start on opening paren: "foo(bar)baz"
        //                             ^
        engine.view_mut().cursor.col = 3;

        // % should jump to closing paren
        press_char(&mut engine, '%');

        assert_eq!(engine.view().cursor.col, 7); // Closing paren
    }

    #[test]
    fn test_percent_braces() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "if { x }");
        engine.update_syntax();

        // Start on opening brace: "if { x }"
        //                             ^
        engine.view_mut().cursor.col = 3;

        // % should jump to closing brace
        press_char(&mut engine, '%');

        assert_eq!(engine.view().cursor.col, 7); // Closing brace
    }

    #[test]
    fn test_percent_brackets() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "arr[0]");
        engine.update_syntax();

        // Start on opening bracket: "arr[0]"
        //                                ^
        engine.view_mut().cursor.col = 3;

        // % should jump to closing bracket
        press_char(&mut engine, '%');

        assert_eq!(engine.view().cursor.col, 5); // Closing bracket
    }

    #[test]
    fn test_percent_closing_to_opening() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "(abc)");
        engine.update_syntax();

        // Start on closing paren: "(abc)"
        //                             ^
        engine.view_mut().cursor.col = 4;

        // % should jump to opening paren
        press_char(&mut engine, '%');

        assert_eq!(engine.view().cursor.col, 0); // Opening paren
    }

    #[test]
    fn test_percent_nested() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "((a))");
        engine.update_syntax();

        // Start on first opening paren: "((a))"
        //                                 ^
        engine.view_mut().cursor.col = 0;

        // % should jump to matching closing paren (outermost)
        press_char(&mut engine, '%');

        assert_eq!(engine.view().cursor.col, 4); // Outermost closing paren
    }

    #[test]
    fn test_percent_not_on_bracket_searches_forward() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo(bar)");
        engine.update_syntax();

        // Start before opening paren: "foo(bar)"
        //                              ^
        engine.view_mut().cursor.col = 0;

        // % should search forward for next bracket and jump to match
        press_char(&mut engine, '%');

        assert_eq!(engine.view().cursor.col, 7); // Closing paren
    }

    #[test]
    fn test_d_percent() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo(bar)baz");
        engine.update_syntax();

        // Start on opening paren: "foo(bar)baz"
        //                             ^
        engine.view_mut().cursor.col = 3;

        // d% should delete from ( to ) inclusive
        press_char(&mut engine, 'd');
        press_char(&mut engine, '%');

        assert_eq!(engine.buffer().to_string(), "foobaz");
        assert_eq!(engine.view().cursor.col, 3);
    }

    #[test]
    fn test_c_percent() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo{bar}baz");
        engine.update_syntax();

        // Start on opening brace: "foo{bar}baz"
        //                             ^
        engine.view_mut().cursor.col = 3;

        // c% should delete from { to } and enter insert mode
        press_char(&mut engine, 'c');
        press_char(&mut engine, '%');

        assert_eq!(engine.buffer().to_string(), "foobaz");
        assert_eq!(engine.mode, Mode::Insert);
        assert_eq!(engine.view().cursor.col, 3);
    }

    // --- Text Object Tests ---

    #[test]
    fn test_diw_inner_word() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo bar baz");
        engine.update_syntax();

        // Position on "bar": "foo bar baz"
        //                         ^
        engine.view_mut().cursor.col = 5;

        press_char(&mut engine, 'd');
        press_char(&mut engine, 'i');
        press_char(&mut engine, 'w');

        assert_eq!(engine.buffer().to_string(), "foo  baz");
        assert_eq!(engine.view().cursor.col, 4);
    }

    #[test]
    fn test_daw_around_word() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo bar baz");
        engine.update_syntax();

        // Position on "bar": "foo bar baz"
        //                         ^
        engine.view_mut().cursor.col = 5;

        press_char(&mut engine, 'd');
        press_char(&mut engine, 'a');
        press_char(&mut engine, 'w');

        assert_eq!(engine.buffer().to_string(), "foo baz");
        assert_eq!(engine.view().cursor.col, 4);
    }

    #[test]
    fn test_ciw_change_word() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Position on "world"
        engine.view_mut().cursor.col = 6;

        press_char(&mut engine, 'c');
        press_char(&mut engine, 'i');
        press_char(&mut engine, 'w');

        assert_eq!(engine.buffer().to_string(), "hello ");
        assert_eq!(engine.mode, Mode::Insert);
        assert_eq!(engine.view().cursor.col, 6);
    }

    #[test]
    fn test_yiw_yank_word() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "one two three");
        engine.update_syntax();

        // Position on "two"
        engine.view_mut().cursor.col = 4;

        press_char(&mut engine, 'y');
        press_char(&mut engine, 'i');
        press_char(&mut engine, 'w');

        // Check register contains "two"
        let (content, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "two");

        // Buffer should be unchanged
        assert_eq!(engine.buffer().to_string(), "one two three");
    }

    #[test]
    fn test_di_quote_double() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, r#"foo "hello world" bar"#);
        engine.update_syntax();

        // Position inside quotes: foo "hello world" bar
        //                                  ^
        engine.view_mut().cursor.col = 10;

        press_char(&mut engine, 'd');
        press_char(&mut engine, 'i');
        press_char(&mut engine, '"');

        assert_eq!(engine.buffer().to_string(), r#"foo "" bar"#);
        assert_eq!(engine.view().cursor.col, 5);
    }

    #[test]
    fn test_da_quote_double() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, r#"foo "hello world" bar"#);
        engine.update_syntax();

        // Position inside quotes
        engine.view_mut().cursor.col = 10;

        press_char(&mut engine, 'd');
        press_char(&mut engine, 'a');
        press_char(&mut engine, '"');

        assert_eq!(engine.buffer().to_string(), "foo  bar");
        assert_eq!(engine.view().cursor.col, 4);
    }

    #[test]
    fn test_di_quote_single() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo 'test' bar");
        engine.update_syntax();

        // Position inside quotes
        engine.view_mut().cursor.col = 6;

        press_char(&mut engine, 'd');
        press_char(&mut engine, 'i');
        press_char(&mut engine, '\'');

        assert_eq!(engine.buffer().to_string(), "foo '' bar");
        assert_eq!(engine.view().cursor.col, 5);
    }

    #[test]
    fn test_di_paren() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo(bar)baz");
        engine.update_syntax();

        // Position inside parens
        engine.view_mut().cursor.col = 5;

        press_char(&mut engine, 'd');
        press_char(&mut engine, 'i');
        press_char(&mut engine, '(');

        assert_eq!(engine.buffer().to_string(), "foo()baz");
        assert_eq!(engine.view().cursor.col, 4);
    }

    #[test]
    fn test_da_paren() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo(bar)baz");
        engine.update_syntax();

        // Position inside parens
        engine.view_mut().cursor.col = 5;

        press_char(&mut engine, 'd');
        press_char(&mut engine, 'a');
        press_char(&mut engine, ')');

        assert_eq!(engine.buffer().to_string(), "foobaz");
        assert_eq!(engine.view().cursor.col, 3);
    }

    #[test]
    fn test_di_brace() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "fn main() {code}");
        engine.update_syntax();

        // Position inside braces
        engine.view_mut().cursor.col = 12;

        press_char(&mut engine, 'd');
        press_char(&mut engine, 'i');
        press_char(&mut engine, '{');

        assert_eq!(engine.buffer().to_string(), "fn main() {}");
        assert_eq!(engine.view().cursor.col, 11);
    }

    #[test]
    fn test_da_brace() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test{content}end");
        engine.update_syntax();

        // Position inside braces
        engine.view_mut().cursor.col = 6;

        press_char(&mut engine, 'd');
        press_char(&mut engine, 'a');
        press_char(&mut engine, '}');

        assert_eq!(engine.buffer().to_string(), "testend");
        assert_eq!(engine.view().cursor.col, 4);
    }

    #[test]
    fn test_di_bracket() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "array[index]end");
        engine.update_syntax();

        // Position inside brackets
        engine.view_mut().cursor.col = 7;

        press_char(&mut engine, 'd');
        press_char(&mut engine, 'i');
        press_char(&mut engine, '[');

        assert_eq!(engine.buffer().to_string(), "array[]end");
        assert_eq!(engine.view().cursor.col, 6);
    }

    #[test]
    fn test_da_bracket() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "array[index]end");
        engine.update_syntax();

        // Position inside brackets
        engine.view_mut().cursor.col = 7;

        press_char(&mut engine, 'd');
        press_char(&mut engine, 'a');
        press_char(&mut engine, ']');

        assert_eq!(engine.buffer().to_string(), "arrayend");
        assert_eq!(engine.view().cursor.col, 5);
    }

    #[test]
    fn test_ciw_at_start_of_word() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Position at start of "world"
        engine.view_mut().cursor.col = 6;

        press_char(&mut engine, 'c');
        press_char(&mut engine, 'i');
        press_char(&mut engine, 'w');

        assert_eq!(engine.buffer().to_string(), "hello ");
        assert_eq!(engine.mode, Mode::Insert);
    }

    #[test]
    fn test_text_object_nested_parens() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "outer(inner(x))end");
        engine.update_syntax();

        // Position in inner parens: outer(inner(x))end
        //                                     ^
        engine.view_mut().cursor.col = 12;

        press_char(&mut engine, 'd');
        press_char(&mut engine, 'i');
        press_char(&mut engine, '(');

        assert_eq!(engine.buffer().to_string(), "outer(inner())end");
    }

    #[test]
    fn test_visual_iw() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "one two three");
        engine.update_syntax();

        // Position on "two"
        engine.view_mut().cursor.col = 4;

        // Enter visual mode and select iw
        press_char(&mut engine, 'v');
        press_char(&mut engine, 'i');
        press_char(&mut engine, 'w');

        assert_eq!(engine.mode, Mode::Visual);
        assert_eq!(engine.visual_anchor.unwrap().col, 4);
        assert_eq!(engine.view().cursor.col, 6);

        // Delete the selection
        press_char(&mut engine, 'd');
        assert_eq!(engine.buffer().to_string(), "one  three");
        assert_eq!(engine.mode, Mode::Normal);
    }

    #[test]
    fn test_visual_aw() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "one two three");
        engine.update_syntax();

        // Position on "two"
        engine.view_mut().cursor.col = 4;

        // Enter visual mode and select aw
        press_char(&mut engine, 'v');
        press_char(&mut engine, 'a');
        press_char(&mut engine, 'w');

        assert_eq!(engine.mode, Mode::Visual);

        // Yank the selection
        press_char(&mut engine, 'y');
        let (content, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "two ");
    }

    #[test]
    fn test_visual_i_quote() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, r#"say "hello" now"#);
        engine.update_syntax();

        // Position inside quotes
        engine.view_mut().cursor.col = 6;

        press_char(&mut engine, 'v');
        press_char(&mut engine, 'i');
        press_char(&mut engine, '"');

        assert_eq!(engine.mode, Mode::Visual);

        // Delete selection
        press_char(&mut engine, 'd');
        assert_eq!(engine.buffer().to_string(), r#"say "" now"#);
    }

    // =======================================================================
    // Repeat command (.) tests
    // =======================================================================

    // TODO: Fix cursor positioning after insert operations
    #[test]
    #[ignore]
    fn test_repeat_insert() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3");
        engine.update_syntax();

        // Insert text on first line
        press_char(&mut engine, 'i');
        assert_eq!(engine.mode, Mode::Insert);
        press_char(&mut engine, 'X');
        press_char(&mut engine, 'Y');
        press_special(&mut engine, "Escape");
        assert_eq!(engine.mode, Mode::Normal);
        assert_eq!(engine.buffer().to_string(), "XYline1\nline2\nline3");

        // Move to second line and repeat
        press_char(&mut engine, 'j');
        press_char(&mut engine, '.');
        assert_eq!(engine.buffer().to_string(), "XYline1\nXYline2\nline3");
        assert_eq!(engine.view().cursor.line, 1);
        assert_eq!(engine.view().cursor.col, 2);
    }

    // TODO: Fix multi-count delete repeat
    #[test]
    #[ignore]
    fn test_repeat_delete_x() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ABCDEF\nGHIJKL");
        engine.update_syntax();

        // Delete 2 chars with 2x
        press_char(&mut engine, '2');
        press_char(&mut engine, 'x');
        assert_eq!(engine.buffer().to_string(), "CDEF\nGHIJKL");

        // Move to second line and repeat
        press_char(&mut engine, 'j');
        press_char(&mut engine, '.');
        assert_eq!(engine.buffer().to_string(), "CDEF\nIJKL");
    }

    #[test]
    fn test_repeat_delete_dd() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3\nline4");
        engine.update_syntax();

        // Delete one line
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'd');
        assert_eq!(engine.buffer().to_string(), "line2\nline3\nline4");

        // Repeat delete
        press_char(&mut engine, '.');
        assert_eq!(engine.buffer().to_string(), "line3\nline4");
    }

    // TODO: Fix cursor positioning for repeat with count
    #[test]
    #[ignore]
    fn test_repeat_insert_with_count() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc\ndef\nghi");
        engine.update_syntax();

        // Insert 'X' once
        press_char(&mut engine, 'i');
        press_char(&mut engine, 'X');
        press_special(&mut engine, "Escape");
        assert_eq!(engine.buffer().to_string(), "Xabc\ndef\nghi");

        // Repeat 3 times on next line
        press_char(&mut engine, 'j');
        press_char(&mut engine, '3');
        press_char(&mut engine, '.');
        assert_eq!(engine.buffer().to_string(), "Xabc\nXXXdef\nghi");
    }

    // TODO: Fix cursor positioning with newline repeats
    #[test]
    #[ignore]
    fn test_repeat_insert_with_newline() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "first");
        engine.update_syntax();

        // Insert with newline
        press_char(&mut engine, 'a');
        press_special(&mut engine, "Return");
        press_char(&mut engine, 'X');
        press_special(&mut engine, "Escape");
        assert_eq!(engine.buffer().to_string(), "first\nX");

        // Move to start and repeat
        engine.view_mut().cursor.line = 0;
        engine.view_mut().cursor.col = 0;
        press_char(&mut engine, '.');
        assert_eq!(engine.buffer().to_string(), "\nXfirst\nX");
    }

    // TODO: Implement substitute repeat
    #[test]
    #[ignore]
    fn test_repeat_substitute_s() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello\nworld");
        engine.update_syntax();

        // Substitute first char with 'X'
        press_char(&mut engine, 's');
        press_char(&mut engine, 'X');
        press_special(&mut engine, "Escape");
        assert_eq!(engine.buffer().to_string(), "Xello\nworld");

        // Move to second line and repeat
        press_char(&mut engine, 'j');
        press_char(&mut engine, '.');
        assert_eq!(engine.buffer().to_string(), "Xello\nXorld");
    }

    // TODO: Implement substitute repeat with count
    #[test]
    #[ignore]
    fn test_repeat_substitute_2s() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abcdef\nghijkl");
        engine.update_syntax();

        // Substitute 2 chars with 'XY'
        press_char(&mut engine, '2');
        press_char(&mut engine, 's');
        press_char(&mut engine, 'X');
        press_char(&mut engine, 'Y');
        press_special(&mut engine, "Escape");
        assert_eq!(engine.buffer().to_string(), "XYcdef\nghijkl");

        // Move to second line and repeat
        press_char(&mut engine, 'j');
        press_char(&mut engine, '.');
        assert_eq!(engine.buffer().to_string(), "XYcdef\nXYijkl");
    }

    #[test]
    fn test_repeat_append() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "one\ntwo");
        engine.update_syntax();

        // Append text
        press_char(&mut engine, 'a');
        press_char(&mut engine, '!');
        press_special(&mut engine, "Escape");
        assert_eq!(engine.buffer().to_string(), "o!ne\ntwo");

        // Move to second line start and repeat (inserts at current position)
        press_char(&mut engine, 'j');
        engine.view_mut().cursor.col = 0; // Ensure we're at column 0
        press_char(&mut engine, '.');
        assert_eq!(engine.buffer().to_string(), "o!ne\n!two");
    }

    #[test]
    fn test_repeat_open_line_o() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "alpha\nbeta");
        engine.update_syntax();

        // Open line below and insert
        press_char(&mut engine, 'o');
        press_char(&mut engine, 'N');
        press_char(&mut engine, 'E');
        press_char(&mut engine, 'W');
        press_special(&mut engine, "Escape");
        assert_eq!(engine.buffer().to_string(), "alpha\nNEW\nbeta");

        // Repeat inserts the text "NEW" at current position (not a full 'o' command)
        // Move to last line and repeat
        press_char(&mut engine, 'j');
        engine.view_mut().cursor.col = 0;
        press_char(&mut engine, '.');
        assert_eq!(engine.buffer().to_string(), "alpha\nNEW\nNEWbeta");
    }

    #[test]
    fn test_repeat_before_any_change() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test");
        engine.update_syntax();

        // Try to repeat when no change has been made
        press_char(&mut engine, '.');
        // Should be no-op
        assert_eq!(engine.buffer().to_string(), "test");
    }

    // TODO: Fix count preservation in repeat
    #[test]
    #[ignore]
    fn test_repeat_preserves_count() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ABCDEFGH\nIJKLMNOP");
        engine.update_syntax();

        // Delete 3 chars
        press_char(&mut engine, '3');
        press_char(&mut engine, 'x');
        assert_eq!(engine.buffer().to_string(), "DEFGH\nIJKLMNOP");

        // Repeat on second line (should delete 3 again)
        press_char(&mut engine, 'j');
        press_char(&mut engine, '.');
        assert_eq!(engine.buffer().to_string(), "DEFGH\nLMNOP");
    }

    // TODO: Fix dd repeat with count
    #[test]
    #[ignore]
    fn test_repeat_dd_multiple_lines() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "a\nb\nc\nd\ne\nf");
        engine.update_syntax();

        // Delete 2 lines
        press_char(&mut engine, '2');
        press_char(&mut engine, 'd');
        press_char(&mut engine, 'd');
        assert_eq!(engine.buffer().to_string(), "c\nd\ne\nf");

        // Repeat (should delete 2 more lines)
        press_char(&mut engine, '.');
        assert_eq!(engine.buffer().to_string(), "e\nf");
    }

    // =======================================================================
    // Mouse click tests
    // =======================================================================

    #[test]
    fn test_mouse_click_sets_cursor() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line0\nline1\nline2\nline3");
        engine.update_syntax();

        // Get the active window ID
        let window_id = engine.active_window_id();

        // Click to move cursor to line 2, col 3
        engine.set_cursor_for_window(window_id, 2, 3);
        assert_eq!(engine.cursor().line, 2);
        assert_eq!(engine.cursor().col, 3);
    }

    #[test]
    fn test_mouse_click_clamps_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line0\nline1\nline2");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click beyond last line (should clamp to line 2)
        engine.set_cursor_for_window(window_id, 10, 0);
        assert_eq!(engine.cursor().line, 2);
        assert_eq!(engine.cursor().col, 0);
    }

    #[test]
    fn test_mouse_click_clamps_col() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "short\nline1\nline2");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click beyond line length (should clamp to 4, last char of "short")
        engine.set_cursor_for_window(window_id, 0, 100);
        assert_eq!(engine.cursor().line, 0);
        assert_eq!(engine.cursor().col, 4); // "short" has 5 chars, max cursor pos is 4
    }

    #[test]
    fn test_mouse_click_switches_window_in_split() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "buffer1\nline1");
        engine.update_syntax();

        // Create a split
        engine.split_window(SplitDirection::Horizontal, None);

        // Modify second buffer
        let len = engine.buffer().len_chars();
        engine.buffer_mut().delete_range(0, len);
        engine.buffer_mut().insert(0, "buffer2\nline2");
        engine.update_syntax();

        // Get both window IDs
        let all_windows: Vec<WindowId> = engine.windows.keys().copied().collect();
        assert_eq!(all_windows.len(), 2);
        let window1 = all_windows[0];
        let window2 = all_windows[1];

        // Make window1 active first
        engine.set_cursor_for_window(window1, 0, 0);
        assert_eq!(engine.active_window_id(), window1);

        // Click in window2 should switch to it
        engine.set_cursor_for_window(window2, 0, 3);
        assert_eq!(engine.active_window_id(), window2);
        assert_eq!(engine.cursor().line, 0);
        assert_eq!(engine.cursor().col, 3);
    }

    #[test]
    fn test_mouse_click_empty_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line0\n\nline2");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click on empty line (line 1)
        engine.set_cursor_for_window(window_id, 1, 5);
        assert_eq!(engine.cursor().line, 1);
        assert_eq!(engine.cursor().col, 0); // Should clamp to 0 for empty line
    }

    #[test]
    fn test_mouse_click_single_window() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc\ndef\nghi");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click to line 1, col 2
        engine.set_cursor_for_window(window_id, 1, 2);
        assert_eq!(engine.cursor().line, 1);
        assert_eq!(engine.cursor().col, 2);

        // Verify we're still in normal mode
        assert_eq!(engine.mode, Mode::Normal);
    }

    #[test]
    fn test_mouse_click_preserves_mode() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line0\nline1\nline2");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Enter insert mode
        press_char(&mut engine, 'i');
        assert_eq!(engine.mode, Mode::Insert);

        // Click should move cursor but mode is handled by UI layer
        // The engine method itself doesn't change mode
        engine.set_cursor_for_window(window_id, 2, 1);
        assert_eq!(engine.cursor().line, 2);
        assert_eq!(engine.cursor().col, 1);
    }

    #[test]
    fn test_mouse_click_invalid_window_id() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line0\nline1");
        engine.update_syntax();

        let old_cursor = *engine.cursor();
        let old_window = engine.active_window_id();

        // Click with invalid window ID (should do nothing)
        engine.set_cursor_for_window(WindowId(9999), 1, 1);

        // Cursor and active window should be unchanged
        assert_eq!(*engine.cursor(), old_cursor);
        assert_eq!(engine.active_window_id(), old_window);
    }

    #[test]
    fn test_mouse_click_at_exact_line_end() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello\nworld");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click at column 5 of "hello" (length is 5, so max cursor pos is 4)
        engine.set_cursor_for_window(window_id, 0, 5);
        assert_eq!(engine.cursor().line, 0);
        assert_eq!(engine.cursor().col, 4); // Clamped to last valid position
    }

    #[test]
    fn test_mouse_click_way_past_last_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "a\nb\nc");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click at line 1000 (way past the 3 lines we have)
        engine.set_cursor_for_window(window_id, 1000, 0);
        assert_eq!(engine.cursor().line, 2); // Clamped to last line
        assert_eq!(engine.cursor().col, 0);
    }

    #[test]
    fn test_mouse_click_on_line_with_tabs() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "\thello\t world");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click at column 0 (before tab)
        engine.set_cursor_for_window(window_id, 0, 0);
        assert_eq!(engine.cursor().col, 0);

        // Click at column 1 (on the tab character itself)
        engine.set_cursor_for_window(window_id, 0, 1);
        assert_eq!(engine.cursor().col, 1);

        // Click at column 6 (in "hello", after tab)
        engine.set_cursor_for_window(window_id, 0, 6);
        assert_eq!(engine.cursor().col, 6);
    }

    #[test]
    fn test_mouse_click_on_unicode_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "Hello 世界 World");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click at various positions
        engine.set_cursor_for_window(window_id, 0, 0);
        assert_eq!(engine.cursor().col, 0);

        engine.set_cursor_for_window(window_id, 0, 6);
        assert_eq!(engine.cursor().col, 6); // First unicode char position

        engine.set_cursor_for_window(window_id, 0, 7);
        assert_eq!(engine.cursor().col, 7); // Second unicode char position
    }

    #[test]
    fn test_mouse_click_at_column_zero() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc\ndef\nghi");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click at column 0 on various lines
        engine.set_cursor_for_window(window_id, 0, 0);
        assert_eq!(engine.cursor().line, 0);
        assert_eq!(engine.cursor().col, 0);

        engine.set_cursor_for_window(window_id, 1, 0);
        assert_eq!(engine.cursor().line, 1);
        assert_eq!(engine.cursor().col, 0);

        engine.set_cursor_for_window(window_id, 2, 0);
        assert_eq!(engine.cursor().line, 2);
        assert_eq!(engine.cursor().col, 0);
    }

    #[test]
    fn test_mouse_click_very_large_column() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "short");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click at column 99999 on a short line
        engine.set_cursor_for_window(window_id, 0, 99999);
        assert_eq!(engine.cursor().line, 0);
        assert_eq!(engine.cursor().col, 4); // Clamped to "short".len() - 1
    }

    #[test]
    fn test_mouse_click_on_very_long_line() {
        let mut engine = Engine::new();
        let long_line = "x".repeat(1000);
        engine.buffer_mut().insert(0, &long_line);
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click at various positions on long line
        engine.set_cursor_for_window(window_id, 0, 0);
        assert_eq!(engine.cursor().col, 0);

        engine.set_cursor_for_window(window_id, 0, 500);
        assert_eq!(engine.cursor().col, 500);

        engine.set_cursor_for_window(window_id, 0, 999);
        assert_eq!(engine.cursor().col, 999);

        // Past the end should clamp to 999 (last valid position)
        engine.set_cursor_for_window(window_id, 0, 1000);
        assert_eq!(engine.cursor().col, 999);
    }

    #[test]
    fn test_mouse_click_mixed_tabs_and_spaces() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "\t  hello  \tworld");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click at start (tab)
        engine.set_cursor_for_window(window_id, 0, 0);
        assert_eq!(engine.cursor().col, 0);

        // Click in middle (after spaces)
        engine.set_cursor_for_window(window_id, 0, 5);
        assert_eq!(engine.cursor().col, 5);

        // Click near end
        engine.set_cursor_for_window(window_id, 0, 15);
        assert_eq!(engine.cursor().col, 15);
    }

    #[test]
    fn test_mouse_click_on_last_character_of_file() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nend");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click on the 'd' in "end" (line 2, col 2)
        engine.set_cursor_for_window(window_id, 2, 2);
        assert_eq!(engine.cursor().line, 2);
        assert_eq!(engine.cursor().col, 2);
    }

    #[test]
    fn test_mouse_click_single_character_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "a\nb\nc");
        engine.update_syntax();

        let window_id = engine.active_window_id();

        // Click on single character lines
        engine.set_cursor_for_window(window_id, 0, 0);
        assert_eq!(engine.cursor().line, 0);
        assert_eq!(engine.cursor().col, 0);

        // Click past the single character
        engine.set_cursor_for_window(window_id, 1, 5);
        assert_eq!(engine.cursor().line, 1);
        assert_eq!(engine.cursor().col, 0); // Clamped to 0 (last valid pos of "b")
    }

    // --- Preview mode tests ---

    #[test]
    fn test_preview_open_marks_buffer() {
        use std::io::Write;
        let path = std::env::temp_dir().join("vimcode_test_preview1.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"preview").unwrap();
        }

        let mut engine = Engine::new();
        engine
            .open_file_with_mode(&path, OpenMode::Preview)
            .unwrap();

        let bid = engine.active_buffer_id();
        assert!(engine.buffer_manager.get(bid).unwrap().preview);
        assert_eq!(engine.preview_buffer_id, Some(bid));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_permanent_open_not_preview() {
        use std::io::Write;
        let path = std::env::temp_dir().join("vimcode_test_preview2.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"permanent").unwrap();
        }

        let mut engine = Engine::new();
        engine
            .open_file_with_mode(&path, OpenMode::Permanent)
            .unwrap();

        let bid = engine.active_buffer_id();
        assert!(!engine.buffer_manager.get(bid).unwrap().preview);
        assert_eq!(engine.preview_buffer_id, None);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_preview_replaced_by_new_preview() {
        use std::io::Write;
        let path1 = std::env::temp_dir().join("vimcode_test_preview3a.txt");
        let path2 = std::env::temp_dir().join("vimcode_test_preview3b.txt");
        {
            let mut f = std::fs::File::create(&path1).unwrap();
            f.write_all(b"file1").unwrap();
        }
        {
            let mut f = std::fs::File::create(&path2).unwrap();
            f.write_all(b"file2").unwrap();
        }

        let mut engine = Engine::new();
        engine
            .open_file_with_mode(&path1, OpenMode::Preview)
            .unwrap();
        let bid1 = engine.active_buffer_id();

        engine
            .open_file_with_mode(&path2, OpenMode::Preview)
            .unwrap();
        let bid2 = engine.active_buffer_id();

        // Old preview should be deleted
        assert!(engine.buffer_manager.get(bid1).is_none());
        // New preview should be active
        assert!(engine.buffer_manager.get(bid2).unwrap().preview);
        assert_eq!(engine.preview_buffer_id, Some(bid2));

        let _ = std::fs::remove_file(&path1);
        let _ = std::fs::remove_file(&path2);
    }

    #[test]
    fn test_double_click_promotes_preview() {
        use std::io::Write;
        let path = std::env::temp_dir().join("vimcode_test_preview4.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"promote").unwrap();
        }

        let mut engine = Engine::new();
        // Single-click: preview
        engine
            .open_file_with_mode(&path, OpenMode::Preview)
            .unwrap();
        let bid = engine.active_buffer_id();
        assert!(engine.buffer_manager.get(bid).unwrap().preview);

        // Double-click: permanent
        engine
            .open_file_with_mode(&path, OpenMode::Permanent)
            .unwrap();
        assert!(!engine.buffer_manager.get(bid).unwrap().preview);
        assert_eq!(engine.preview_buffer_id, None);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_edit_promotes_preview() {
        use std::io::Write;
        let path = std::env::temp_dir().join("vimcode_test_preview5.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"editme").unwrap();
        }

        let mut engine = Engine::new();
        engine
            .open_file_with_mode(&path, OpenMode::Preview)
            .unwrap();
        let bid = engine.active_buffer_id();
        assert!(engine.buffer_manager.get(bid).unwrap().preview);

        // Enter insert mode and type a character
        press_char(&mut engine, 'i');
        press_char(&mut engine, 'x');
        press_special(&mut engine, "Escape");

        // Should be promoted
        assert!(!engine.buffer_manager.get(bid).unwrap().preview);
        assert_eq!(engine.preview_buffer_id, None);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_save_promotes_preview() {
        use std::io::Write;
        let path = std::env::temp_dir().join("vimcode_test_preview6.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"saveme").unwrap();
        }

        let mut engine = Engine::new();
        engine
            .open_file_with_mode(&path, OpenMode::Preview)
            .unwrap();
        let bid = engine.active_buffer_id();
        assert!(engine.buffer_manager.get(bid).unwrap().preview);

        // Save
        let _ = engine.save();

        assert!(!engine.buffer_manager.get(bid).unwrap().preview);
        assert_eq!(engine.preview_buffer_id, None);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_ls_shows_preview_flag() {
        use std::io::Write;
        let path = std::env::temp_dir().join("vimcode_test_preview7.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"ls").unwrap();
        }

        let mut engine = Engine::new();
        engine
            .open_file_with_mode(&path, OpenMode::Preview)
            .unwrap();

        let listing = engine.list_buffers();
        assert!(listing.contains("[Preview]"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_already_permanent_ignores_preview_mode() {
        use std::io::Write;
        let path = std::env::temp_dir().join("vimcode_test_preview8.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"perm").unwrap();
        }

        let mut engine = Engine::new();
        // Open as permanent first
        engine
            .open_file_with_mode(&path, OpenMode::Permanent)
            .unwrap();
        let bid = engine.active_buffer_id();

        // Trying to preview the same file should NOT mark it as preview
        engine
            .open_file_with_mode(&path, OpenMode::Preview)
            .unwrap();
        assert!(!engine.buffer_manager.get(bid).unwrap().preview);
        assert_eq!(engine.preview_buffer_id, None);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_delete_preview_clears_tracking() {
        use std::io::Write;
        let path = std::env::temp_dir().join("vimcode_test_preview9.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"del").unwrap();
        }

        let mut engine = Engine::new();
        engine
            .open_file_with_mode(&path, OpenMode::Preview)
            .unwrap();
        let bid = engine.active_buffer_id();
        assert_eq!(engine.preview_buffer_id, Some(bid));

        let _ = engine.delete_buffer(bid, true);
        assert_eq!(engine.preview_buffer_id, None);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_preview_never_dirty_and_preview() {
        use std::io::Write;
        let path = std::env::temp_dir().join("vimcode_test_preview10.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"dirtytest").unwrap();
        }

        let mut engine = Engine::new();
        engine
            .open_file_with_mode(&path, OpenMode::Preview)
            .unwrap();
        let bid = engine.active_buffer_id();

        // Type to make dirty — should auto-promote
        press_char(&mut engine, 'i');
        press_char(&mut engine, 'z');
        press_special(&mut engine, "Escape");

        let state = engine.buffer_manager.get(bid).unwrap();
        // Should be dirty but NOT preview (promoted)
        assert!(state.dirty);
        assert!(!state.preview);

        let _ = std::fs::remove_file(&path);
    }

    // =======================================================================
    // open_file_preview (single-click sidebar) Tests
    // =======================================================================

    #[test]
    fn test_open_file_preview_creates_preview_tab() {
        use std::io::Write;
        let path = std::env::temp_dir().join("vimcode_test_sidebar_preview1.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"hello").unwrap();
        }

        let mut engine = Engine::new();
        engine.open_file_preview(&path);

        let bid = engine.active_buffer_id();
        let state = engine.buffer_manager.get(bid).unwrap();
        assert!(state.preview, "single-click should open as preview");
        assert_eq!(engine.preview_buffer_id, Some(bid));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_open_file_preview_replaced_by_second_single_click() {
        use std::io::Write;
        let path1 = std::env::temp_dir().join("vimcode_test_sidebar_preview2a.txt");
        let path2 = std::env::temp_dir().join("vimcode_test_sidebar_preview2b.txt");
        {
            let mut f = std::fs::File::create(&path1).unwrap();
            f.write_all(b"file1").unwrap();
            let mut f = std::fs::File::create(&path2).unwrap();
            f.write_all(b"file2").unwrap();
        }

        let mut engine = Engine::new();
        engine.open_file_preview(&path1);
        let bid1 = engine.active_buffer_id();

        engine.open_file_preview(&path2);
        let bid2 = engine.active_buffer_id();

        // The first preview buffer should be gone; only the second remains.
        assert!(
            engine.buffer_manager.get(bid1).is_none(),
            "old preview buffer deleted"
        );
        assert!(
            engine.buffer_manager.get(bid2).unwrap().preview,
            "new buffer is preview"
        );
        assert_eq!(engine.preview_buffer_id, Some(bid2));
        // Tab count should not have grown (reused the preview slot).
        assert_eq!(
            engine.tabs.len(),
            2,
            "still only 2 tabs (initial + 1 preview)"
        );

        let _ = std::fs::remove_file(&path1);
        let _ = std::fs::remove_file(&path2);
    }

    #[test]
    fn test_open_file_preview_double_click_promotes() {
        use std::io::Write;
        let path = std::env::temp_dir().join("vimcode_test_sidebar_preview3.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"hello").unwrap();
        }

        let mut engine = Engine::new();
        engine.open_file_preview(&path);
        let bid = engine.active_buffer_id();
        assert!(engine.buffer_manager.get(bid).unwrap().preview);

        // Double-click: open_file_in_tab promotes the preview in-place.
        engine.open_file_in_tab(&path);
        assert!(
            !engine.buffer_manager.get(bid).unwrap().preview,
            "promoted to permanent"
        );
        assert_eq!(engine.preview_buffer_id, None);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_open_file_preview_permanent_file_just_switches() {
        use std::io::Write;
        let path1 = std::env::temp_dir().join("vimcode_test_sidebar_preview4a.txt");
        let path2 = std::env::temp_dir().join("vimcode_test_sidebar_preview4b.txt");
        {
            let mut f = std::fs::File::create(&path1).unwrap();
            f.write_all(b"file1").unwrap();
            let mut f = std::fs::File::create(&path2).unwrap();
            f.write_all(b"file2").unwrap();
        }

        let mut engine = Engine::new();
        // Open file1 permanently in a second tab.
        engine.open_file_in_tab(&path1);
        let permanent_tab_idx = engine.active_tab;
        let bid1 = engine.active_buffer_id();

        // Single-click file1 — should just switch back to it, not make it a preview.
        engine.open_file_preview(&path1);
        assert_eq!(
            engine.active_tab, permanent_tab_idx,
            "switched to existing tab"
        );
        assert!(
            !engine.buffer_manager.get(bid1).unwrap().preview,
            "file stays permanent"
        );
        assert_eq!(engine.preview_buffer_id, None);

        let _ = std::fs::remove_file(&path1);
        let _ = std::fs::remove_file(&path2);
    }

    // =======================================================================
    // Visual Block Mode Tests
    // =======================================================================

    #[test]
    fn test_visual_block_mode_entry() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test");
        engine.update_syntax();

        // Enter visual block mode with Ctrl-V
        press_ctrl(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::VisualBlock);
        assert!(engine.visual_anchor.is_some());
        assert_eq!(engine.visual_anchor.unwrap().line, 0);
        assert_eq!(engine.visual_anchor.unwrap().col, 0);
    }

    #[test]
    fn test_visual_block_mode_escape_exits() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test");
        engine.update_syntax();

        press_ctrl(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::VisualBlock);

        press_special(&mut engine, "Escape");
        assert_eq!(engine.mode, Mode::Normal);
        assert!(engine.visual_anchor.is_none());
    }

    #[test]
    fn test_visual_block_mode_switching() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test");
        engine.update_syntax();

        // Start in visual block
        press_ctrl(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::VisualBlock);

        // Switch to character visual with v
        press_char(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::Visual);
        assert!(engine.visual_anchor.is_some()); // anchor preserved

        // Switch to line visual with V
        press_char(&mut engine, 'V');
        assert_eq!(engine.mode, Mode::VisualLine);

        // Switch back to block visual with Ctrl-V
        press_ctrl(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::VisualBlock);

        // Ctrl-V again to exit
        press_ctrl(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::Normal);
    }

    #[test]
    fn test_visual_block_yank() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc\ndef\nghi");
        engine.update_syntax();

        // Enter visual block mode
        press_ctrl(&mut engine, 'v');

        // Select 2x2 block: "ab", "de"
        press_char(&mut engine, 'l'); // col 1
        press_char(&mut engine, 'j'); // line 1

        // Yank
        press_char(&mut engine, 'y');

        // Check register - should have "ab\nde"
        let (content, is_linewise) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "ab\nde");
        assert!(!is_linewise);

        // Should be back in normal mode
        assert_eq!(engine.mode, Mode::Normal);
        assert!(engine.visual_anchor.is_none());
    }

    #[test]
    fn test_visual_block_delete() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc\ndef\nghi");
        engine.update_syntax();

        // Enter visual block mode
        press_ctrl(&mut engine, 'v');

        // Select 2x2 block: "ab", "de"
        press_char(&mut engine, 'l'); // col 1
        press_char(&mut engine, 'j'); // line 1

        // Delete
        press_char(&mut engine, 'd');

        // Check buffer - should be "c\nf\nghi"
        let text = engine.buffer().to_string();
        assert_eq!(text, "c\nf\nghi");

        // Check register
        let (content, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "ab\nde");

        // Should be back in normal mode at start of block
        assert_eq!(engine.mode, Mode::Normal);
        assert_eq!(engine.view().cursor.line, 0);
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_visual_block_simple_delete() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc\ndef\nghi");
        engine.update_syntax();

        // Start at (0, 1) - character 'b'
        press_char(&mut engine, 'l');

        // Enter visual block
        press_ctrl(&mut engine, 'v');

        // Select 2x2 block: move right once, down once
        // This should select cols 1-2 on lines 0-1
        press_char(&mut engine, 'l'); // Now at col 2
        press_char(&mut engine, 'j'); // Now at line 1

        // Delete
        press_char(&mut engine, 'd');

        // Should have deleted "bc" from line 0 and "ef" from line 1
        // Result: "a\nd\nghi"
        let text = engine.buffer().to_string();
        assert_eq!(text, "a\nd\nghi");
    }

    #[test]
    fn test_visual_block_cursor_positions() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abcdef");
        engine.update_syntax();

        // Start at col 0
        assert_eq!(engine.view().cursor.col, 0);

        // Move to col 1
        press_char(&mut engine, 'l');
        assert_eq!(engine.view().cursor.col, 1);

        // Enter visual block
        press_ctrl(&mut engine, 'v');
        assert_eq!(engine.visual_anchor.unwrap().col, 1);

        // Move right once more
        press_char(&mut engine, 'l');
        assert_eq!(engine.view().cursor.col, 2);

        // Check anchor and cursor
        assert_eq!(engine.visual_anchor.unwrap().col, 1);
        assert_eq!(engine.view().cursor.col, 2);
    }

    #[test]
    fn test_visual_block_yank_simple() {
        // Note: Visual block with uneven line lengths is simplified
        // Full Vim behavior with "virtual columns" is a future enhancement
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abcdef\nghijkl");
        engine.update_syntax();

        // Start at col 1 (character 'b')
        press_char(&mut engine, 'l');
        press_ctrl(&mut engine, 'v');

        // Select cols 1-2 on 2 lines
        press_char(&mut engine, 'l'); // Now at col 2 (character 'c')
        press_char(&mut engine, 'j'); // Move down to line 1

        // Yank
        press_char(&mut engine, 'y');

        // Check register - should have "bc\nhi"
        let (content, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "bc\nhi");
    }

    #[test]
    fn test_visual_block_delete_uniform_lines() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abcdef\nghijkl\nmnopqr");
        engine.update_syntax();

        // Start at col 1 (character 'b')
        press_char(&mut engine, 'l');
        press_ctrl(&mut engine, 'v');

        // Select cols 1-2 on 3 lines
        press_char(&mut engine, 'l'); // Now at col 2 (character 'c')
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'j'); // line 2

        // Delete
        press_char(&mut engine, 'd');

        // Check buffer - should have deleted "bc", "hi", "no"
        let text = engine.buffer().to_string();
        assert_eq!(text, "adef\ngjkl\nmpqr");
    }

    #[test]
    fn test_visual_block_change() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc\ndef\nghi");
        engine.update_syntax();

        // Enter visual block mode
        press_ctrl(&mut engine, 'v');

        // Select 2x2 block
        press_char(&mut engine, 'l');
        press_char(&mut engine, 'j');

        // Change
        press_char(&mut engine, 'c');

        // Should be in insert mode
        assert_eq!(engine.mode, Mode::Insert);

        // Buffer should have block deleted
        let text = engine.buffer().to_string();
        assert_eq!(text, "c\nf\nghi");
    }

    #[test]
    fn test_visual_block_navigation() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3\nline4");
        engine.update_syntax();

        // Enter visual block mode
        press_ctrl(&mut engine, 'v');
        assert_eq!(engine.mode, Mode::VisualBlock);

        // Move right extends block horizontally
        press_char(&mut engine, 'l');
        assert_eq!(engine.view().cursor.col, 1);
        press_char(&mut engine, 'l');
        assert_eq!(engine.view().cursor.col, 2);

        // Move down extends block vertically
        press_char(&mut engine, 'j');
        assert_eq!(engine.view().cursor.line, 1);
        press_char(&mut engine, 'j');
        assert_eq!(engine.view().cursor.line, 2);

        // Still in visual block mode
        assert_eq!(engine.mode, Mode::VisualBlock);
    }

    #[test]
    fn test_visual_block_yank_single_column() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc\ndef\nghi");
        engine.update_syntax();

        // Enter visual block mode
        press_ctrl(&mut engine, 'v');

        // Select single column, 3 lines (just move down, don't move right)
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'j');

        // Yank
        press_char(&mut engine, 'y');

        // Check register - should have "a\nd\ng" (first character of each line)
        let (content, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(content, "a\nd\ng");
    }

    #[test]
    fn test_visual_block_with_count() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc\ndef\nghi\njkl");
        engine.update_syntax();

        // Enter visual block mode
        press_ctrl(&mut engine, 'v');

        // Use count to move: 2j should move down 2 lines
        press_char(&mut engine, '2');
        press_char(&mut engine, 'j');
        assert_eq!(engine.view().cursor.line, 2);
        assert_eq!(engine.mode, Mode::VisualBlock);

        // Use count to move right: 2l
        press_char(&mut engine, '2');
        press_char(&mut engine, 'l');
        assert_eq!(engine.view().cursor.col, 2);
    }

    // ========================================================================
    // Visual Mode Case Change Tests
    // ========================================================================

    #[test]
    fn test_visual_lowercase() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "HELLO World");
        engine.update_syntax();

        // Select "HELLO"
        press_char(&mut engine, 'v');
        for _ in 0..4 {
            press_char(&mut engine, 'l');
        }

        // Lowercase
        press_char(&mut engine, 'u');

        assert_eq!(engine.buffer().to_string(), "hello World");
        assert_eq!(engine.mode, Mode::Normal);
        assert_eq!(engine.view().cursor.line, 0);
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_visual_uppercase() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello WORLD");
        engine.update_syntax();

        // Select "hello"
        press_char(&mut engine, 'v');
        for _ in 0..4 {
            press_char(&mut engine, 'l');
        }

        // Uppercase
        press_char(&mut engine, 'U');

        assert_eq!(engine.buffer().to_string(), "HELLO WORLD");
        assert_eq!(engine.mode, Mode::Normal);
    }

    #[test]
    fn test_visual_line_lowercase() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "FIRST Line\nSECOND Line\nthird");
        engine.update_syntax();

        // Select first two lines
        press_char(&mut engine, 'V');
        press_char(&mut engine, 'j');

        // Lowercase
        press_char(&mut engine, 'u');

        assert_eq!(
            engine.buffer().to_string(),
            "first line\nsecond line\nthird"
        );
        assert_eq!(engine.mode, Mode::Normal);
        assert_eq!(engine.view().cursor.line, 0);
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_visual_line_uppercase() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "first\nsecond\nthird");
        engine.update_syntax();

        // Select middle line
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'V');

        // Uppercase
        press_char(&mut engine, 'U');

        assert_eq!(engine.buffer().to_string(), "first\nSECOND\nthird");
        assert_eq!(engine.mode, Mode::Normal);
        assert_eq!(engine.view().cursor.line, 1);
    }

    #[test]
    fn test_visual_block_lowercase() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "ABC\nDEF\nGHI");
        engine.update_syntax();

        // Enter visual block mode
        press_ctrl(&mut engine, 'v');

        // Select 2x2 block (AB, DE)
        press_char(&mut engine, 'l');
        press_char(&mut engine, 'j');

        // Lowercase
        press_char(&mut engine, 'u');

        assert_eq!(engine.buffer().to_string(), "abC\ndeF\nGHI");
        assert_eq!(engine.mode, Mode::Normal);
        assert_eq!(engine.view().cursor.line, 0);
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_visual_block_uppercase() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc\ndef\nghi");
        engine.update_syntax();

        // Move to column 1
        press_char(&mut engine, 'l');

        // Enter visual block mode
        press_ctrl(&mut engine, 'v');

        // Select 2x3 block (bc, ef, hi)
        press_char(&mut engine, 'l');
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'j');

        // Uppercase
        press_char(&mut engine, 'U');

        assert_eq!(engine.buffer().to_string(), "aBC\ndEF\ngHI");
        assert_eq!(engine.mode, Mode::Normal);
        assert_eq!(engine.view().cursor.line, 0);
        assert_eq!(engine.view().cursor.col, 1);
    }

    #[test]
    fn test_visual_case_change_with_undo() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.update_syntax();

        // Select and uppercase "hello"
        press_char(&mut engine, 'v');
        for _ in 0..4 {
            press_char(&mut engine, 'l');
        }
        press_char(&mut engine, 'U');

        assert_eq!(engine.buffer().to_string(), "HELLO world");

        // Undo
        press_char(&mut engine, 'u');
        assert_eq!(engine.buffer().to_string(), "hello world");
    }

    #[test]
    fn test_visual_case_mixed_content() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "Hello123WORLD!");
        engine.update_syntax();

        // Select all
        press_char(&mut engine, 'v');
        press_char(&mut engine, '$');

        // Lowercase
        press_char(&mut engine, 'u');

        assert_eq!(engine.buffer().to_string(), "hello123world!");
    }

    // ========================================================================
    // Marks Tests
    // ========================================================================

    #[test]
    fn test_mark_set_and_jump_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\nline3\nline4");
        engine.update_syntax();

        // Go to line 2
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'j');
        assert_eq!(engine.view().cursor.line, 2);

        // Set mark 'a'
        press_char(&mut engine, 'm');
        press_char(&mut engine, 'a');
        assert!(engine.message.contains("Mark 'a' set"));

        // Move to line 0
        press_char(&mut engine, 'g');
        press_char(&mut engine, 'g');
        assert_eq!(engine.view().cursor.line, 0);

        // Jump to mark 'a' line
        press_char(&mut engine, '\'');
        press_char(&mut engine, 'a');
        assert_eq!(engine.view().cursor.line, 2);
        assert_eq!(engine.view().cursor.col, 0); // ' jumps to start of line
    }

    #[test]
    fn test_mark_jump_exact_position() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world\nfoo bar baz");
        engine.update_syntax();

        // Move to line 1, col 4
        press_char(&mut engine, 'j');
        for _ in 0..4 {
            press_char(&mut engine, 'l');
        }
        assert_eq!(engine.view().cursor.line, 1);
        assert_eq!(engine.view().cursor.col, 4);

        // Set mark 'b'
        press_char(&mut engine, 'm');
        press_char(&mut engine, 'b');

        // Move to line 0, col 0
        press_char(&mut engine, 'g');
        press_char(&mut engine, 'g');
        assert_eq!(engine.view().cursor.line, 0);
        assert_eq!(engine.view().cursor.col, 0);

        // Jump to exact mark position with backtick
        press_char(&mut engine, '`');
        press_char(&mut engine, 'b');
        assert_eq!(engine.view().cursor.line, 1);
        assert_eq!(engine.view().cursor.col, 4);
    }

    #[test]
    fn test_mark_not_set() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test");
        engine.update_syntax();

        // Try to jump to mark that doesn't exist
        press_char(&mut engine, '\'');
        press_char(&mut engine, 'x');
        assert!(engine.message.contains("Mark 'x' not set"));
    }

    #[test]
    fn test_mark_multiple_marks() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "a\nb\nc\nd\ne");
        engine.update_syntax();

        // Set mark 'a' at line 1
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'm');
        press_char(&mut engine, 'a');

        // Set mark 'b' at line 3
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'm');
        press_char(&mut engine, 'b');

        // Jump to mark 'a'
        press_char(&mut engine, '\'');
        press_char(&mut engine, 'a');
        assert_eq!(engine.view().cursor.line, 1);

        // Jump to mark 'b'
        press_char(&mut engine, '\'');
        press_char(&mut engine, 'b');
        assert_eq!(engine.view().cursor.line, 3);
    }

    #[test]
    fn test_mark_overwrite() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "a\nb\nc");
        engine.update_syntax();

        // Set mark 'a' at line 0
        press_char(&mut engine, 'm');
        press_char(&mut engine, 'a');

        // Move to line 2 and overwrite mark 'a'
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'm');
        press_char(&mut engine, 'a');

        // Jump to mark 'a' should go to line 2
        press_char(&mut engine, 'g');
        press_char(&mut engine, 'g');
        press_char(&mut engine, '\'');
        press_char(&mut engine, 'a');
        assert_eq!(engine.view().cursor.line, 2);
    }

    #[test]
    fn test_mark_per_buffer() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "buffer1 line1\nbuffer1 line2");
        engine.update_syntax();

        // Set mark 'a' in first buffer
        press_char(&mut engine, 'j');
        press_char(&mut engine, 'm');
        press_char(&mut engine, 'a');

        // Create second buffer
        let buffer2_id = engine.buffer_manager.create();
        engine
            .buffer_manager
            .get_mut(buffer2_id)
            .unwrap()
            .buffer
            .insert(0, "buffer2 line1\nbuffer2 line2");

        // Switch to second buffer
        let window_id = engine.active_window().id;
        engine.windows.get_mut(&window_id).unwrap().buffer_id = buffer2_id;

        // Mark 'a' shouldn't exist in buffer 2
        press_char(&mut engine, '\'');
        press_char(&mut engine, 'a');
        assert!(engine.message.contains("Mark 'a' not set"));
    }

    // ========================================================================
    // Macro Tests
    // ========================================================================

    #[test]
    fn test_macro_basic_recording() {
        let mut engine = Engine::new();

        // Start recording into register 'a'
        press_char(&mut engine, 'q');
        press_char(&mut engine, 'a');
        assert_eq!(engine.macro_recording, Some('a'));
        assert!(engine.message.contains("Recording"));

        // Record some keystrokes
        press_char(&mut engine, 'i'); // Enter insert mode
        press_char(&mut engine, 'h');
        press_char(&mut engine, 'i');
        press_special(&mut engine, "Escape"); // ESC
        press_char(&mut engine, 'l');

        // Stop recording
        press_char(&mut engine, 'q');
        assert_eq!(engine.macro_recording, None);
        assert!(engine.message.contains("recorded"));

        // Verify macro content in register
        let (content, _) = engine.registers.get(&'a').unwrap();
        // Should contain "ihi<ESC>l" but ESC is unicode 0x1b
        assert!(content.contains("hi"));
        assert_eq!(content.len(), 5); // i, h, i, ESC, l
    }

    #[test]
    fn test_macro_playback() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "line1\nline2\n");

        // Manually set up a macro in register 'a' (skip recording for simplicity)
        // Macro: A!<ESC> (append "!" to end of line, then ESC)
        engine.set_register('a', "A!\x1b".to_string(), false);

        // Play macro
        press_char(&mut engine, '@');
        press_char(&mut engine, 'a');

        // Process playback queue
        while !engine.macro_playback_queue.is_empty() {
            let _ = engine.advance_macro_playback();
        }

        // Verify result
        assert_eq!(engine.buffer().to_string(), "line1!\nline2\n");
    }

    #[test]
    fn test_macro_repeat_last() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test\n");

        // Set up macro with ESC to return to normal mode
        engine.set_register('b', "A.\x1b".to_string(), false);

        // Play it once
        press_char(&mut engine, '@');
        press_char(&mut engine, 'b');
        while !engine.macro_playback_queue.is_empty() {
            let _ = engine.advance_macro_playback();
        }

        assert_eq!(engine.buffer().to_string(), "test.\n");

        // Play it again with @@
        press_char(&mut engine, '@');
        press_char(&mut engine, '@');
        while !engine.macro_playback_queue.is_empty() {
            let _ = engine.advance_macro_playback();
        }

        assert_eq!(engine.buffer().to_string(), "test..\n");
    }

    #[test]
    fn test_macro_with_count() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "x\n");

        // Macro: A!<ESC> (append "!" and return to normal mode)
        engine.set_register('c', "A!\x1b".to_string(), false);

        // Play 3 times: 3@c
        press_char(&mut engine, '3');
        press_char(&mut engine, '@');
        press_char(&mut engine, 'c');

        while !engine.macro_playback_queue.is_empty() {
            let _ = engine.advance_macro_playback();
        }

        assert_eq!(engine.buffer().to_string(), "x!!!\n");
    }

    #[test]
    fn test_macro_recursion_limit() {
        let mut engine = Engine::new();

        // Create recursive macro: @a calls @a
        engine.set_register('a', "@a".to_string(), false);

        // Try to play it
        press_char(&mut engine, '@');
        press_char(&mut engine, 'a');

        // Should hit recursion limit
        for _ in 0..MAX_MACRO_RECURSION + 10 {
            if engine.macro_playback_queue.is_empty() {
                break;
            }
            let (has_more, _) = engine.advance_macro_playback();
            if !has_more {
                break;
            }
        }

        // Engine should still be functional
        assert!(engine.macro_recursion_depth <= MAX_MACRO_RECURSION);
    }

    #[test]
    fn test_macro_empty_register() {
        let mut engine = Engine::new();

        // Try to play from empty register
        press_char(&mut engine, '@');
        press_char(&mut engine, 'z');

        assert!(engine.message.contains("empty"));
    }

    #[test]
    fn test_macro_stop_on_error() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "short\n");

        // Macro that tries to move right 100 times
        engine.set_register('d', "100l".to_string(), false);

        press_char(&mut engine, '@');
        press_char(&mut engine, 'd');

        // Playback should stop when hitting buffer boundary
        let mut iterations = 0;
        while !engine.macro_playback_queue.is_empty() && iterations < 200 {
            let _ = engine.advance_macro_playback();
            iterations += 1;
        }

        // Should be at end of line, not crashed
        assert!(engine.cursor().col <= 4); // At or before the newline
    }

    #[test]
    fn test_macro_recording_saves_to_register() {
        let mut engine = Engine::new();

        // Record a simple macro
        press_char(&mut engine, 'q');
        press_char(&mut engine, 'm');
        press_char(&mut engine, 'i');
        press_char(&mut engine, 'x');
        press_special(&mut engine, "Escape"); // Must ESC before stopping recording
        press_char(&mut engine, 'q');

        // Verify it's in register 'm'
        let (content, _) = engine.registers.get(&'m').unwrap();
        assert_eq!(content, "ix\x1b"); // i, x, ESC

        // Also should be in unnamed register
        let (unnamed_content, _) = engine.registers.get(&'"').unwrap();
        assert_eq!(unnamed_content, "ix\x1b");
    }

    #[test]
    fn test_macro_records_navigation_keys() {
        let mut engine = Engine::new();

        // Start recording
        press_char(&mut engine, 'q');
        press_char(&mut engine, 'n');

        // Record some navigation
        press_char(&mut engine, 'l'); // Move right (unicode)
        press_char(&mut engine, 'j'); // Move down (unicode)
        press_special(&mut engine, "Left"); // Arrow key (no unicode)
        press_special(&mut engine, "Up"); // Arrow key (no unicode)

        // Stop recording
        press_char(&mut engine, 'q');

        // Verify it's recorded with proper encoding
        let (content, _) = engine.registers.get(&'n').unwrap();
        assert_eq!(content, "lj<Left><Up>");
    }

    #[test]
    fn test_macro_records_ctrl_keys() {
        let mut engine = Engine::new();

        // Start recording
        press_char(&mut engine, 'q');
        press_char(&mut engine, 'c');

        // Record some Ctrl combinations
        press_ctrl(&mut engine, 'd'); // Ctrl-D
        press_ctrl(&mut engine, 'u'); // Ctrl-U

        // Stop recording
        press_char(&mut engine, 'q');

        // Verify it's recorded with proper encoding
        let (content, _) = engine.registers.get(&'c').unwrap();
        assert_eq!(content, "<C-D><C-U>");
    }

    #[test]
    fn test_macro_playback_with_arrow_keys() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc\ndef\nghi");

        // Macro: move right twice, then move down
        engine.set_register('a', "ll<Down>".to_string(), false);

        // Start at (0, 0)
        assert_eq!(engine.cursor().line, 0);
        assert_eq!(engine.cursor().col, 0);

        // Play macro
        press_char(&mut engine, '@');
        press_char(&mut engine, 'a');
        while !engine.macro_playback_queue.is_empty() {
            let _ = engine.advance_macro_playback();
        }

        // Should be at (1, 2) - line 1, col 2
        assert_eq!(engine.cursor().line, 1);
        assert_eq!(engine.cursor().col, 2);
    }

    #[test]
    fn test_macro_playback_with_ctrl_keys() {
        let mut engine = Engine::new();
        // Create a buffer with many lines
        let mut content = String::new();
        for i in 0..50 {
            content.push_str(&format!("line {}\n", i));
        }
        engine.buffer_mut().insert(0, &content);

        // Macro: Ctrl-D (half page down)
        engine.set_register('d', "<C-D>".to_string(), false);

        let initial_line = engine.cursor().line;

        // Play macro
        press_char(&mut engine, '@');
        press_char(&mut engine, 'd');
        while !engine.macro_playback_queue.is_empty() {
            let _ = engine.advance_macro_playback();
        }

        // Should have moved down (exact amount depends on viewport, but should be > 0)
        assert!(engine.cursor().line > initial_line);
    }

    #[test]
    fn test_macro_records_insert_mode_with_enter() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "test");

        // Start recording
        press_char(&mut engine, 'q');
        press_char(&mut engine, 'r');

        // Enter insert mode, type text, press enter, type more, ESC
        press_char(&mut engine, 'A'); // Append
        press_char(&mut engine, '!');
        press_special(&mut engine, "Return"); // New line
        press_char(&mut engine, 'n');
        press_char(&mut engine, 'e');
        press_char(&mut engine, 'w');
        press_special(&mut engine, "Escape");

        // Stop recording
        press_char(&mut engine, 'q');

        // Verify the macro content includes <CR>
        let (content, _) = engine.registers.get(&'r').unwrap();
        assert_eq!(content, "A!<CR>new\x1b");
    }

    #[test]
    fn test_macro_comprehensive() {
        let mut engine = Engine::new();
        // Create a buffer with multiple lines
        engine
            .buffer_mut()
            .insert(0, "line one\nline two\nline three");

        // Record a complex macro that uses:
        // - Navigation (j, l, arrow keys)
        // - Insert mode
        // - Special keys (Return, ESC)
        // - Ctrl keys

        // Macro: j (down), $$ (end of line), A (append), ! (type), ESC, Ctrl-D
        engine.set_register('z', "j$A!\x1b<C-D>".to_string(), false);

        // Start at (0, 0)
        assert_eq!(engine.cursor().line, 0);

        // Play the macro
        press_char(&mut engine, '@');
        press_char(&mut engine, 'z');
        while !engine.macro_playback_queue.is_empty() {
            let _ = engine.advance_macro_playback();
        }

        // Should have:
        // - Moved down to line 1
        // - Moved to end of line
        // - Appended "!"
        // - Returned to normal mode
        // - Scrolled down with Ctrl-D

        // Check that "!" was appended to line 1
        let line1_content: String = engine.buffer().content.line(1).chars().collect();
        assert!(line1_content.contains("line two!"));
    }

    #[test]
    fn test_replace_current_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world\nhello again\n");

        // Replace "hello" with "hi" on current line only (no g flag)
        let result = engine.replace_in_range(None, "hello", "hi", "");
        assert_eq!(result.unwrap(), 1);
        assert_eq!(engine.buffer().to_string(), "hi world\nhello again\n");
    }

    #[test]
    fn test_replace_all_lines() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world\nhello again\n");

        // Replace all "hello" with "hi" across both lines
        let result = engine.replace_in_range(Some((0, 1)), "hello", "hi", "g");
        assert_eq!(result.unwrap(), 2);
        assert_eq!(engine.buffer().to_string(), "hi world\nhi again\n");
    }

    #[test]
    fn test_replace_case_insensitive() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "Hello HELLO hello\n");

        // Replace all case variations
        let result = engine.replace_in_range(None, "hello", "hi", "gi");
        assert_eq!(result.unwrap(), 1); // Replaces all in one line
        assert_eq!(engine.buffer().to_string(), "hi hi hi\n");
    }

    #[test]
    fn test_substitute_command_current_line() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo bar foo\n");

        engine.execute_command("s/foo/baz/");
        assert_eq!(engine.buffer().to_string(), "baz bar foo\n"); // Only first
    }

    #[test]
    fn test_substitute_command_global() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo bar foo\n");

        engine.execute_command("s/foo/baz/g");
        assert_eq!(engine.buffer().to_string(), "baz bar baz\n"); // All on line
    }

    #[test]
    fn test_substitute_command_all_lines() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo\nbar foo\nfoo\n");

        engine.execute_command("%s/foo/baz/g");
        assert_eq!(engine.buffer().to_string(), "baz\nbar baz\nbaz\n");
    }

    #[test]
    fn test_substitute_visual_range() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foo\nbar\nbaz\n");

        // Simulate visual selection on lines 0-1
        engine.mode = Mode::VisualLine;
        engine.visual_anchor = Some(Cursor { line: 0, col: 0 });
        engine.view_mut().cursor = Cursor { line: 1, col: 0 };

        engine.execute_command("'<,'>s/bar/qux/");
        // Should only affect line 1, not lines 0 or 2
        assert_eq!(engine.buffer().to_string(), "foo\nqux\nbaz\n");
    }

    #[test]
    fn test_substitute_undo() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world\n");

        // Do a substitution
        engine.execute_command("s/hello/goodbye/");
        assert_eq!(engine.buffer().to_string(), "goodbye world\n");

        // Undo should restore original text completely
        engine.undo();
        assert_eq!(engine.buffer().to_string(), "hello world\n");

        // Redo should apply the substitution again
        engine.redo();
        assert_eq!(engine.buffer().to_string(), "goodbye world\n");
    }

    #[test]
    fn test_substitute_multiple_lines_undo() {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "vi is great\nvi is powerful\nvi rocks\n");

        // Replace all occurrences across all lines
        engine.execute_command("%s/vi/vim/gi");
        assert_eq!(
            engine.buffer().to_string(),
            "vim is great\nvim is powerful\nvim rocks\n"
        );

        // Undo should restore all original text
        engine.undo();
        assert_eq!(
            engine.buffer().to_string(),
            "vi is great\nvi is powerful\nvi rocks\n"
        );
    }

    #[test]
    fn test_cw_cursor_position_after_last_word() {
        // Verify cursor is positioned AFTER the space when using cw on last word
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc def");
        engine.update_syntax();

        // Move to 'd' in "def"
        engine.view_mut().cursor.col = 4;

        // cw should delete "def" and position cursor after the space for insertion
        press_char(&mut engine, 'c');
        press_char(&mut engine, 'w');

        assert_eq!(
            engine.buffer().to_string(),
            "abc ",
            "cw should leave 'abc '"
        );
        assert_eq!(engine.mode, Mode::Insert, "should be in insert mode");
        assert_eq!(
            engine.view().cursor.col,
            4,
            "cursor should be after the space (col 4)"
        );
    }

    #[test]
    fn test_ce_cursor_position_after_last_word() {
        // Verify cursor is positioned AFTER the space when using ce on last word
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abc def");
        engine.update_syntax();

        // Move to 'd' in "def"
        engine.view_mut().cursor.col = 4;

        // ce should delete "def" and position cursor after the space for insertion
        press_char(&mut engine, 'c');
        press_char(&mut engine, 'e');

        assert_eq!(
            engine.buffer().to_string(),
            "abc ",
            "ce should leave 'abc '"
        );
        assert_eq!(engine.mode, Mode::Insert, "should be in insert mode");
        assert_eq!(
            engine.view().cursor.col,
            4,
            "cursor should be after the space (col 4)"
        );
    }

    // ── Fold tests ────────────────────────────────────────────────────────────

    fn make_indented_engine() -> Engine {
        let mut engine = Engine::new();
        // 5-line buffer: line 0 is the header, lines 1-3 are indented, line 4 is peer
        engine.buffer_mut().insert(
            0,
            "fn foo() {\n    let x = 1;\n    let y = 2;\n    x + y\n}\n",
        );
        engine
    }

    #[test]
    fn test_fold_close_detects_range() {
        let mut engine = make_indented_engine();
        // Cursor at line 0 ("fn foo() {")
        engine.view_mut().cursor.line = 0;
        let range = engine.detect_fold_range(0);
        assert!(range.is_some(), "should detect fold range under fn");
        let (start, end) = range.unwrap();
        assert_eq!(start, 0);
        assert!(end >= 3, "end should include indented body");
    }

    #[test]
    fn test_fold_close_and_open() {
        let mut engine = make_indented_engine();
        engine.view_mut().cursor.line = 0;

        // zc — close fold
        press_char(&mut engine, 'z');
        press_char(&mut engine, 'c');
        assert!(
            engine.view().fold_at(0).is_some(),
            "fold should exist after zc"
        );

        // zo — open fold
        press_char(&mut engine, 'z');
        press_char(&mut engine, 'o');
        assert!(
            engine.view().fold_at(0).is_none(),
            "fold should be removed after zo"
        );
    }

    #[test]
    fn test_fold_toggle_za() {
        let mut engine = make_indented_engine();
        engine.view_mut().cursor.line = 0;

        // First za closes the fold
        press_char(&mut engine, 'z');
        press_char(&mut engine, 'a');
        assert!(engine.view().fold_at(0).is_some(), "first za should close");

        // Second za opens it
        press_char(&mut engine, 'z');
        press_char(&mut engine, 'a');
        assert!(engine.view().fold_at(0).is_none(), "second za should open");
    }

    #[test]
    fn test_fold_open_all_zr() {
        let mut engine = make_indented_engine();
        engine.view_mut().cursor.line = 0;

        press_char(&mut engine, 'z');
        press_char(&mut engine, 'c');
        assert!(!engine.view().folds.is_empty(), "should have a fold");

        press_char(&mut engine, 'z');
        press_char(&mut engine, 'R');
        assert!(engine.view().folds.is_empty(), "zR should clear all folds");
    }

    #[test]
    fn test_fold_navigation_skips_hidden_lines() {
        let mut engine = make_indented_engine();
        engine.view_mut().cursor.line = 0;

        // Close the fold (lines 1-3 become hidden)
        press_char(&mut engine, 'z');
        press_char(&mut engine, 'c');

        // j from line 0 should skip to line 4 (first visible line after fold)
        press_char(&mut engine, 'j');
        assert_eq!(
            engine.view().cursor.line,
            4,
            "j should skip hidden fold lines"
        );

        // k from line 4 should go back to line 0 (fold header)
        press_char(&mut engine, 'k');
        assert_eq!(
            engine.view().cursor.line,
            0,
            "k should skip hidden fold lines"
        );
    }

    #[test]
    fn test_fold_cursor_clamp_on_close() {
        let mut engine = make_indented_engine();
        // Put cursor inside what will become the fold body
        engine.view_mut().cursor.line = 2;

        // Close fold from line 0 — but cursor is on line 2, which is inside.
        // The fold command detects range from cursor (line 2) not header.
        // So we place cursor at 0 and close, then move cursor inside and close again.

        // Close from line 0
        engine.view_mut().cursor.line = 0;
        press_char(&mut engine, 'z');
        press_char(&mut engine, 'c');

        // Cursor should still be on line 0 (the fold header)
        assert_eq!(
            engine.view().cursor.line,
            0,
            "cursor should stay at fold header after zc"
        );
    }

    // ── Auto-indent tests ─────────────────────────────────────────────────────

    #[test]
    fn test_auto_indent_enter() {
        let mut engine = Engine::new();
        engine.settings.auto_indent = true;
        // Buffer has one indented line
        engine.buffer_mut().insert(0, "    hello");
        // Move cursor to end of line and press Enter
        press_char(&mut engine, 'A'); // Append mode at end of line
        press_special(&mut engine, "Return");
        // New line should have same indent
        assert_eq!(engine.view().cursor.line, 1);
        assert_eq!(engine.view().cursor.col, 4);
        let line1: String = engine.buffer().content.line(1).chars().collect();
        assert!(
            line1.starts_with("    "),
            "new line should start with 4 spaces"
        );
    }

    #[test]
    fn test_auto_indent_no_indent() {
        let mut engine = Engine::new();
        engine.settings.auto_indent = true;
        engine.buffer_mut().insert(0, "hello");
        press_char(&mut engine, 'A');
        press_special(&mut engine, "Return");
        // Line with no indent should produce no indent on new line
        assert_eq!(engine.view().cursor.line, 1);
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_auto_indent_disabled() {
        let mut engine = Engine::new();
        engine.settings.auto_indent = false;
        engine.buffer_mut().insert(0, "    hello");
        press_char(&mut engine, 'A');
        press_special(&mut engine, "Return");
        // With auto_indent off, new line should have col 0
        assert_eq!(engine.view().cursor.line, 1);
        assert_eq!(engine.view().cursor.col, 0);
    }

    #[test]
    fn test_auto_indent_o() {
        let mut engine = Engine::new();
        engine.settings.auto_indent = true;
        engine.buffer_mut().insert(0, "    fn foo() {");
        // 'o' opens a new line below with same indent
        press_special(&mut engine, "Escape"); // ensure normal mode
        press_char(&mut engine, 'o');
        assert_eq!(engine.view().cursor.line, 1);
        assert_eq!(engine.view().cursor.col, 4);
        assert_eq!(engine.mode, Mode::Insert);
    }

    #[test]
    fn test_auto_indent_capital_o() {
        let mut engine = Engine::new();
        engine.settings.auto_indent = true;
        // Put cursor on line 1 (which is indented)
        engine.buffer_mut().insert(0, "fn foo() {\n    body\n}");
        press_char(&mut engine, 'j'); // move to "    body"
        press_special(&mut engine, "Escape");
        press_char(&mut engine, 'O');
        // New line above "    body" should have same indent (4 spaces)
        assert_eq!(engine.view().cursor.line, 1);
        assert_eq!(engine.view().cursor.col, 4);
        assert_eq!(engine.mode, Mode::Insert);
    }

    // ── Completion tests ──────────────────────────────────────────────────────

    #[test]
    fn test_completion_basic() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foobar\nfoo");
        // Position cursor at end of "foo" on line 1
        press_char(&mut engine, 'G'); // last line
        press_char(&mut engine, 'A'); // Append at end — now in insert mode at col 3
                                      // Ctrl-N should complete to "foobar"
        press_ctrl(&mut engine, 'n');
        let line1: String = engine.buffer().content.line(1).chars().collect();
        assert!(
            line1.starts_with("foobar"),
            "Ctrl-N should insert foobar, got: {}",
            line1
        );
        assert_eq!(engine.completion_idx, Some(0));
    }

    #[test]
    fn test_completion_cycle_next() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foobar foobaz football\nfoo");
        press_char(&mut engine, 'G');
        press_char(&mut engine, 'A');
        // First Ctrl-N selects first candidate
        press_ctrl(&mut engine, 'n');
        let first_idx = engine.completion_idx.unwrap();
        // Second Ctrl-N moves to next
        press_ctrl(&mut engine, 'n');
        let second_idx = engine.completion_idx.unwrap();
        assert_ne!(first_idx, second_idx, "Ctrl-N should cycle candidates");
    }

    #[test]
    fn test_completion_cycle_prev() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foobar foobaz football\nfoo");
        press_char(&mut engine, 'G');
        press_char(&mut engine, 'A');
        // Ctrl-P starts from last candidate
        press_ctrl(&mut engine, 'p');
        let total = engine.completion_candidates.len();
        assert_eq!(engine.completion_idx, Some(total - 1));
    }

    #[test]
    fn test_completion_clear_on_other_key() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "foobar\nfoo");
        press_char(&mut engine, 'G');
        press_char(&mut engine, 'A');
        press_ctrl(&mut engine, 'n');
        assert!(engine.completion_idx.is_some());
        // Any regular key clears completion state
        press_char(&mut engine, 'x');
        assert!(engine.completion_idx.is_none());
        assert!(engine.completion_candidates.is_empty());
    }

    // ── :set command (engine-level) ───────────────────────────────────────────

    #[test]
    fn test_set_number_via_command() {
        let mut engine = Engine::new();
        engine.settings.line_numbers = crate::core::settings::LineNumberMode::None;
        // Use parse_set_option directly to avoid writing to disk in unit tests
        engine.settings.parse_set_option("number").unwrap();
        assert_eq!(
            engine.settings.line_numbers,
            crate::core::settings::LineNumberMode::Absolute
        );
    }

    #[test]
    fn test_set_relativenumber_after_number_gives_hybrid() {
        let mut engine = Engine::new();
        engine.settings.line_numbers = crate::core::settings::LineNumberMode::Absolute;
        engine.settings.parse_set_option("relativenumber").unwrap();
        assert_eq!(
            engine.settings.line_numbers,
            crate::core::settings::LineNumberMode::Hybrid
        );
    }

    #[test]
    fn test_set_expandtab_false_tab_inserts_tab_char() {
        let mut engine = Engine::new();
        engine.settings.expand_tab = false;
        press_char(&mut engine, 'i');
        press_special(&mut engine, "Tab");
        press_special(&mut engine, "Escape");
        let text: String = engine.buffer().content.chars().collect();
        assert!(text.starts_with('\t'), "expected tab char, got: {:?}", text);
    }

    #[test]
    fn test_set_expandtab_true_tab_inserts_spaces() {
        let mut engine = Engine::new();
        engine.settings.expand_tab = true;
        engine.settings.tabstop = 2;
        press_char(&mut engine, 'i');
        press_special(&mut engine, "Tab");
        press_special(&mut engine, "Escape");
        let text: String = engine.buffer().content.chars().collect();
        assert!(text.starts_with("  "), "expected 2 spaces, got: {:?}", text);
        assert!(!text.starts_with('\t'));
    }

    #[test]
    fn test_set_unknown_option_sets_error_message() {
        let mut engine = Engine::new();
        let result = engine.settings.parse_set_option("badoption");
        assert!(result.is_err());
    }

    #[test]
    fn test_set_display_all() {
        let engine = Engine::new();
        let display = engine.settings.display_all();
        assert!(!display.is_empty());
        assert!(display.contains("ts="));
        assert!(display.contains("sw="));
    }
}
