use super::*;

impl Engine {
    // =======================================================================
    // Editor group accessors
    // =======================================================================

    pub fn active_group(&self) -> &EditorGroup {
        self.editor_groups.get(&self.active_group).unwrap()
    }

    pub fn active_group_mut(&mut self) -> &mut EditorGroup {
        self.editor_groups.get_mut(&self.active_group).unwrap()
    }

    /// Allocate a new unique GroupId.
    pub(crate) fn new_group_id(&mut self) -> GroupId {
        let id = GroupId(self.next_group_id);
        self.next_group_id += 1;
        id
    }

    // =======================================================================
    // Accessors for active window/buffer (facade for backward compatibility)
    // =======================================================================

    pub fn active_tab(&self) -> &Tab {
        self.active_group().active_tab()
    }

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        self.active_group_mut().active_tab_mut()
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

    /// Get just the filename (without directory) of the active buffer, if any.
    pub fn active_buffer_name(&self) -> Option<String> {
        self.file_path()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    }

    /// Check if the active buffer has unsaved changes.
    pub fn dirty(&self) -> bool {
        self.active_buffer_state().dirty
    }

    /// True if ANY open buffer has unsaved changes.
    pub fn has_any_unsaved(&self) -> bool {
        self.buffer_manager
            .list()
            .into_iter()
            .any(|id| self.buffer_manager.get(id).is_some_and(|s| s.dirty))
    }

    /// Compute explorer tree indicators: git status + deduplicated diagnostic counts.
    /// Returns (git_statuses, diag_counts) where:
    /// - git_statuses: canonical path → git status char (M, A, D, R, ?)
    /// - diag_counts: canonical path → (error_lines, warning_lines) deduplicated by line number
    pub fn explorer_indicators(
        &self,
    ) -> (HashMap<PathBuf, char>, HashMap<PathBuf, (usize, usize)>) {
        use crate::core::lsp::DiagnosticSeverity;
        use std::collections::HashSet;

        // Build git status map
        let mut git_statuses: HashMap<PathBuf, char> = HashMap::new();
        let repo_root = git::find_repo_root(&self.cwd);
        if let Some(ref root) = repo_root {
            for fs in &self.sc_file_statuses {
                let kind = fs.unstaged.or(fs.staged);
                if let Some(k) = kind {
                    let abs = root.join(&fs.path);
                    let canon = abs.canonicalize().unwrap_or(abs);
                    git_statuses.insert(canon, k.label());
                }
            }
        }

        // Collect ignored error sources from installed extensions' LSP configs.
        // E.g. rust extension declares ignore_error_sources = ["rust-analyzer"]
        // because its internal analysis produces false-positive errors.
        let manifests = self.ext_available_manifests();
        let ignored_error_sources: HashSet<&str> = manifests
            .iter()
            .flat_map(|m| m.lsp.ignore_error_sources.iter().map(|s| s.as_str()))
            .collect();

        // Count diagnostics for explorer indicators, deduplicating by (code, message).
        // Skip error-severity diagnostics from ignored sources (configured per-extension).
        // Warnings from all sources are still counted.
        let mut diag_counts: HashMap<PathBuf, (usize, usize)> = HashMap::new();
        for (path, diagnostics) in &self.lsp_diagnostics {
            let mut error_keys = HashSet::new();
            let mut warning_keys = HashSet::new();
            for d in diagnostics {
                let key = (d.code.clone().unwrap_or_default(), d.message.clone());
                match d.severity {
                    DiagnosticSeverity::Error => {
                        let dominated = d
                            .source
                            .as_deref()
                            .is_some_and(|s| ignored_error_sources.contains(s));
                        if !dominated {
                            error_keys.insert(key);
                        }
                    }
                    DiagnosticSeverity::Warning => {
                        warning_keys.insert(key);
                    }
                    _ => {}
                }
            }
            if !error_keys.is_empty() || !warning_keys.is_empty() {
                let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
                diag_counts.insert(canon, (error_keys.len(), warning_keys.len()));
            }
        }

        (git_statuses, diag_counts)
    }

    /// Save every dirty buffer that has a known file path.
    /// Returns the number of buffers successfully saved.
    pub fn save_all_dirty(&mut self) -> usize {
        let dirty_ids: Vec<_> = self
            .buffer_manager
            .list()
            .into_iter()
            .filter(|&id| {
                self.buffer_manager
                    .get(id)
                    .is_some_and(|s| s.dirty && s.file_path.is_some())
            })
            .collect();
        let mut saved = 0;
        for id in dirty_ids {
            if let Some(state) = self.buffer_manager.get_mut(id) {
                if state.save().is_ok() {
                    saved += 1;
                }
            }
        }
        saved
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

    /// Set scroll_top for the active window, snapping out of fold bodies.
    #[allow(dead_code)]
    pub fn set_scroll_top(&mut self, scroll_top: usize) {
        let snapped = Self::snap_scroll_top(&self.view().folds, scroll_top);
        self.view_mut().scroll_top = snapped;
    }

    /// Scroll the active window down by `count` visible lines (fold-aware).
    pub fn scroll_down_visible(&mut self, count: usize) {
        let max_line = self.buffer().len_lines().saturating_sub(1);
        let st = self.view().scroll_top;
        let new_top = self.view().next_visible_line(st, count, max_line);
        self.view_mut().scroll_top = new_top;
    }

    /// Scroll the active window up by `count` visible lines (fold-aware).
    pub fn scroll_up_visible(&mut self, count: usize) {
        let st = self.view().scroll_top;
        let new_top = self.view().prev_visible_line(st, count);
        self.view_mut().scroll_top = new_top;
    }

    /// Scroll a specific window down by `count` visible lines (fold-aware).
    pub fn scroll_down_visible_for_window(&mut self, window_id: WindowId, count: usize) {
        if let Some(window) = self.windows.get(&window_id) {
            let buf_id = window.buffer_id;
            let max_line = self
                .buffer_manager
                .get(buf_id)
                .map(|bs| bs.buffer.len_lines().saturating_sub(1))
                .unwrap_or(0);
            let new_top = window
                .view
                .next_visible_line(window.view.scroll_top, count, max_line);
            self.windows.get_mut(&window_id).unwrap().view.scroll_top = new_top;
        }
    }

    /// Scroll a specific window up by `count` visible lines (fold-aware).
    pub fn scroll_up_visible_for_window(&mut self, window_id: WindowId, count: usize) {
        if let Some(window) = self.windows.get(&window_id) {
            let new_top = window.view.prev_visible_line(window.view.scroll_top, count);
            self.windows.get_mut(&window_id).unwrap().view.scroll_top = new_top;
        }
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

    /// Set scroll_top for a specific window without changing the active window,
    /// snapping out of fold bodies.
    #[allow(dead_code)]
    pub fn set_scroll_top_for_window(&mut self, window_id: WindowId, scroll_top: usize) {
        if let Some(window) = self.windows.get_mut(&window_id) {
            window.view.scroll_top = Self::snap_scroll_top(&window.view.folds, scroll_top);
        }
    }

    /// If `line` falls inside a fold body, snap to the fold header (start).
    pub(crate) fn snap_scroll_top(folds: &[FoldRegion], line: usize) -> usize {
        for f in folds {
            if line > f.start && line <= f.end {
                return f.start;
            }
        }
        line
    }

    /// Set scroll_left for a specific window without changing the active window.
    pub fn set_scroll_left_for_window(&mut self, window_id: WindowId, scroll_left: usize) {
        if let Some(window) = self.windows.get_mut(&window_id) {
            window.view.scroll_left = scroll_left;
        }
    }
}
