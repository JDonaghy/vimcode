use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::buffer::{Buffer, BufferId};
use super::buffer_manager::{BufferManager, BufferState};
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

pub struct Engine {
    // --- Multi-buffer/window state ---
    pub buffer_manager: BufferManager,
    pub windows: HashMap<WindowId, Window>,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    next_window_id: usize,
    next_tab_id: usize,

    // --- Global state (not per-window) ---
    pub mode: Mode,
    /// Accumulates typed characters in Command/Search mode.
    pub command_buffer: String,
    /// Status message shown in the command line area (e.g. "written", errors).
    pub message: String,
    /// Current search query (from last `/` search).
    pub search_query: String,
    /// Char-offset pairs (start, end) for all search matches in active buffer.
    pub search_matches: Vec<(usize, usize)>,
    /// Index into `search_matches` for the current match.
    pub search_index: Option<usize>,
    /// Pending key for multi-key sequences (e.g. 'g' for gg, 'd' for dd).
    pub pending_key: Option<char>,
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
            mode: Mode::Normal,
            command_buffer: String::new(),
            message: String::new(),
            search_query: String::new(),
            search_matches: Vec::new(),
            search_index: None,
            pending_key: None,
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

    // =======================================================================
    // Buffer operations
    // =======================================================================

    pub fn update_syntax(&mut self) {
        self.active_buffer_state_mut().update_syntax();
    }

    /// Save the active buffer to its file.
    pub fn save(&mut self) -> Result<(), String> {
        let state = self.active_buffer_state_mut();
        if let Some(ref path) = state.file_path.clone() {
            match state.save() {
                Ok(line_count) => {
                    self.message = format!("\"{}\" {}L written", path.display(), line_count);
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

        // Remove window from windows map
        self.windows.remove(&window_id);

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

        // Remove closed windows
        for id in windows_to_close {
            self.windows.remove(&id);
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
        if self.buffer_manager.get(buffer_id).is_some() {
            self.active_window_mut().buffer_id = buffer_id;
            self.active_window_mut().view = View::new(); // Reset view
            self.search_matches.clear();
            self.search_index = None;
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
            lines.push(format!(
                "{:3} {}{}{} \"{}\"",
                num, active_flag, alt_flag, dirty_flag, name
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

        let mut changed = false;
        let mut action = EngineAction::None;

        match self.mode {
            Mode::Normal => {
                action = self.handle_normal_key(key_name, unicode, ctrl, &mut changed);
            }
            Mode::Insert => {
                self.handle_insert_key(key_name, unicode, &mut changed);
            }
            Mode::Command => {
                action = self.handle_command_key(key_name, unicode);
            }
            Mode::Search => {
                self.handle_search_key(key_name, unicode);
            }
        }

        if changed {
            self.set_dirty(true);
            self.update_syntax();
        }

        self.ensure_cursor_visible();
        action
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
                    let half = self.viewport_lines() / 2;
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    self.view_mut().cursor.line = (self.view().cursor.line + half).min(max_line);
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "u" => {
                    // Half-page up
                    let half = self.viewport_lines() / 2;
                    self.view_mut().cursor.line = self.view().cursor.line.saturating_sub(half);
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "f" => {
                    // Full page down
                    let viewport = self.viewport_lines();
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    self.view_mut().cursor.line =
                        (self.view().cursor.line + viewport).min(max_line);
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "b" => {
                    // Full page up
                    let viewport = self.viewport_lines();
                    self.view_mut().cursor.line = self.view().cursor.line.saturating_sub(viewport);
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "w" => {
                    // Ctrl-W prefix for window commands
                    self.pending_key = Some('\x17'); // Ctrl-W marker
                    return EngineAction::None;
                }
                _ => {}
            }
        }

        // Handle pending multi-key sequences (gg, dd, Ctrl-W x, gt)
        if let Some(pending) = self.pending_key.take() {
            return self.handle_pending_key(pending, key_name, unicode, changed);
        }

        // In normal mode, check the unicode char for vim keys
        match unicode {
            Some('h') => self.move_left(),
            Some('j') => self.move_down(),
            Some('k') => self.move_up(),
            Some('l') => self.move_right(),
            Some('i') => self.mode = Mode::Insert,
            Some('a') => {
                let max_col = self.get_max_cursor_col(self.view().cursor.line);
                if self.view().cursor.col < max_col {
                    self.view_mut().cursor.col += 1;
                } else {
                    let line = self.view().cursor.line;
                    let insert_max = self.get_line_len_for_insert(line);
                    self.view_mut().cursor.col = insert_max;
                }
                self.mode = Mode::Insert;
            }
            Some('A') => {
                let line = self.view().cursor.line;
                self.view_mut().cursor.col = self.get_line_len_for_insert(line);
                self.mode = Mode::Insert;
            }
            Some('I') => {
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
            }
            Some('o') => {
                let line = self.view().cursor.line;
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
                self.buffer_mut().insert(insert_pos, "\n");
                self.view_mut().cursor.line += 1;
                self.view_mut().cursor.col = 0;
                self.mode = Mode::Insert;
                *changed = true;
            }
            Some('O') => {
                let line = self.view().cursor.line;
                let line_start = self.buffer().line_to_char(line);
                self.buffer_mut().insert(line_start, "\n");
                self.view_mut().cursor.col = 0;
                self.mode = Mode::Insert;
                *changed = true;
            }
            Some('0') => self.view_mut().cursor.col = 0,
            Some('$') => {
                let line = self.view().cursor.line;
                self.view_mut().cursor.col = self.get_max_cursor_col(line);
            }
            Some('x') => {
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let max_col = self.get_max_cursor_col(line);
                if max_col > 0 || self.buffer().line_len_chars(line) > 0 {
                    let char_idx = self.buffer().line_to_char(line) + col;
                    if char_idx < self.buffer().len_chars() {
                        self.buffer_mut().delete_range(char_idx, char_idx + 1);
                        self.clamp_cursor_col();
                        *changed = true;
                    }
                }
            }
            Some('w') => self.move_word_forward(),
            Some('b') => self.move_word_backward(),
            Some('e') => self.move_word_end(),
            Some('d') => {
                self.pending_key = Some('d');
            }
            Some('D') => {
                self.delete_to_end_of_line(changed);
            }
            Some('g') => {
                self.pending_key = Some('g');
            }
            Some('G') => {
                let last = self.buffer().len_lines().saturating_sub(1);
                self.view_mut().cursor.line = last;
                self.clamp_cursor_col();
            }
            Some('n') => self.search_next(),
            Some('N') => self.search_prev(),
            Some(':') => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
            }
            Some('/') => {
                self.mode = Mode::Search;
                self.command_buffer.clear();
            }
            _ => match key_name {
                "Left" => self.move_left(),
                "Down" => self.move_down(),
                "Up" => self.move_up(),
                "Right" => self.move_right(),
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
                    self.view_mut().cursor.line = 0;
                    self.view_mut().cursor.col = 0;
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
                if unicode == Some('d') {
                    self.delete_current_line(changed);
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
            _ => {}
        }
        EngineAction::None
    }

    fn handle_insert_key(&mut self, key_name: &str, unicode: Option<char>, changed: &mut bool) {
        match key_name {
            "Escape" => {
                self.mode = Mode::Normal;
                self.clamp_cursor_col();
            }
            "BackSpace" => {
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;
                if col > 0 {
                    self.buffer_mut().delete_range(char_idx - 1, char_idx);
                    self.view_mut().cursor.col -= 1;
                    *changed = true;
                } else if line > 0 {
                    let prev_line_len = self.buffer().line_len_chars(line - 1);
                    let new_col = if prev_line_len > 0 {
                        prev_line_len - 1
                    } else {
                        0
                    };
                    self.buffer_mut().delete_range(char_idx - 1, char_idx);
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
                    self.buffer_mut().delete_range(char_idx, char_idx + 1);
                    *changed = true;
                }
            }
            "Return" => {
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;
                self.buffer_mut().insert(char_idx, "\n");
                self.view_mut().cursor.line += 1;
                self.view_mut().cursor.col = 0;
                *changed = true;
            }
            "Tab" => {
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;
                self.buffer_mut().insert(char_idx, "    ");
                self.view_mut().cursor.col += 4;
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
                    self.buffer_mut().insert(char_idx, s);
                    self.view_mut().cursor.col += 1;
                    *changed = true;
                }
            }
        }
    }

    fn handle_command_key(&mut self, key_name: &str, unicode: Option<char>) -> EngineAction {
        match key_name {
            "Escape" => {
                self.mode = Mode::Normal;
                self.command_buffer.clear();
                EngineAction::None
            }
            "Return" => {
                self.mode = Mode::Normal;
                let cmd = self.command_buffer.clone();
                self.command_buffer.clear();
                self.execute_command(&cmd)
            }
            "BackSpace" => {
                self.command_buffer.pop();
                if self.command_buffer.is_empty() {
                    self.mode = Mode::Normal;
                }
                EngineAction::None
            }
            _ => {
                if let Some(ch) = unicode {
                    self.command_buffer.push(ch);
                }
                EngineAction::None
            }
        }
    }

    fn handle_search_key(&mut self, key_name: &str, unicode: Option<char>) {
        match key_name {
            "Escape" => {
                self.mode = Mode::Normal;
                self.command_buffer.clear();
            }
            "Return" => {
                self.mode = Mode::Normal;
                let query = self.command_buffer.clone();
                self.command_buffer.clear();
                if !query.is_empty() {
                    self.search_query = query;
                    self.run_search();
                    self.search_next();
                }
            }
            "BackSpace" => {
                self.command_buffer.pop();
                if self.command_buffer.is_empty() {
                    self.mode = Mode::Normal;
                }
            }
            _ => {
                if let Some(ch) = unicode {
                    self.command_buffer.push(ch);
                }
            }
        }
    }

    fn execute_command(&mut self, cmd: &str) -> EngineAction {
        let cmd = cmd.trim();

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
                if self.dirty() {
                    self.message = "No write since last change (add ! to override)".to_string();
                    EngineAction::Error
                } else {
                    EngineAction::Quit
                }
            }
            "q!" => EngineAction::Quit,
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

    // --- Search ---

    fn run_search(&mut self) {
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

    fn search_next(&mut self) {
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

    fn search_prev(&mut self) {
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

        if pos + 1 >= total_chars {
            return;
        }
        pos += 1;

        while pos < total_chars && self.buffer().content.char(pos).is_whitespace() {
            pos += 1;
        }

        let ch = self.buffer().content.char(pos.min(total_chars - 1));
        if is_word_char(ch) {
            while pos + 1 < total_chars && is_word_char(self.buffer().content.char(pos + 1)) {
                pos += 1;
            }
        } else {
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

    // --- Line operations ---

    fn delete_current_line(&mut self, changed: &mut bool) {
        let num_lines = self.buffer().len_lines();
        if num_lines == 0 {
            return;
        }

        let line = self.view().cursor.line;
        let line_start = self.buffer().line_to_char(line);
        let line_char_len = self.buffer().line_len_chars(line);

        if line_char_len == 0 && num_lines <= 1 {
            return;
        }

        let line_content = self.buffer().content.line(line);
        let ends_with_newline = line_content.chars().last() == Some('\n');

        let (delete_start, delete_end) = if ends_with_newline {
            (line_start, line_start + line_char_len)
        } else if line > 0 {
            (line_start - 1, line_start + line_char_len)
        } else {
            (line_start, line_start + line_char_len)
        };

        self.buffer_mut().delete_range(delete_start, delete_end);
        *changed = true;

        let new_num_lines = self.buffer().len_lines();
        if self.view().cursor.line >= new_num_lines && new_num_lines > 0 {
            self.view_mut().cursor.line = new_num_lines - 1;
        }
        self.view_mut().cursor.col = 0;
        self.clamp_cursor_col();
    }

    fn delete_to_end_of_line(&mut self, changed: &mut bool) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let char_idx = self.buffer().line_to_char(line) + col;
        let line_content = self.buffer().content.line(line);
        let line_start = self.buffer().line_to_char(line);
        let line_end = line_start + line_content.len_chars();

        let delete_end = if line_content.chars().last() == Some('\n') {
            line_end - 1
        } else {
            line_end
        };

        if char_idx < delete_end {
            self.buffer_mut().delete_range(char_idx, delete_end);
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
        if self.view().cursor.line < max_line {
            self.view_mut().cursor.line += 1;
            self.clamp_cursor_col();
        }
    }

    fn move_up(&mut self) {
        if self.view().cursor.line > 0 {
            self.view_mut().cursor.line -= 1;
            self.clamp_cursor_col();
        }
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
        let mut engine = Engine::new();
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
}
