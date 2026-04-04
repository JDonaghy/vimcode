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

    /// Returns true if the tab bar should be hidden for this group
    /// (hide_single_tab is on, there's only one editor group, and it has at most one tab).
    /// In multi-group mode, tab bars are always shown so users can distinguish groups.
    pub fn is_tab_bar_hidden(&self, group_id: GroupId) -> bool {
        self.settings.hide_single_tab
            && self.group_layout.leaf_count() <= 1
            && self
                .editor_groups
                .get(&group_id)
                .is_some_and(|g| g.tabs.len() <= 1)
    }

    /// Adjust group rects in-place: for groups whose tab bar is hidden,
    /// expand the content area upward by `tab_row_height` (one row of tabs).
    pub fn adjust_group_rects_for_hidden_tabs(
        &self,
        rects: &mut [(GroupId, WindowRect)],
        full_tab_bar_height: f64,
    ) {
        if !self.settings.hide_single_tab || self.group_layout.leaf_count() > 1 {
            return;
        }
        // The tab row is one unit; breadcrumbs (if any) is the rest.
        let tab_row_h = if self.settings.breadcrumbs {
            full_tab_bar_height / 2.0
        } else {
            full_tab_bar_height
        };
        for (gid, rect) in rects.iter_mut() {
            if self
                .editor_groups
                .get(gid)
                .is_some_and(|g| g.tabs.len() <= 1)
            {
                rect.y -= tab_row_h;
                rect.height += tab_row_h;
            }
        }
    }

    // =======================================================================
    // Accessors for active window/buffer (facade for backward compatibility)
    // =======================================================================

    /// Repair inconsistent state where `active_tab().active_window` points
    /// to a WindowId that no longer exists in `self.windows`.  This can
    /// happen after certain tab/group close sequences.  The method finds
    /// a valid window from the current tab's layout, or creates a fresh
    /// scratch window as a last resort.
    pub(crate) fn repair_active_window(&mut self) {
        let wid = self.active_tab().active_window;
        if self.windows.contains_key(&wid) {
            return; // already valid
        }

        // Try to find another valid window in the current tab's layout.
        let layout_wids = self.active_tab().layout.window_ids();
        for candidate in &layout_wids {
            if self.windows.contains_key(candidate) {
                self.active_tab_mut().active_window = *candidate;
                return;
            }
        }

        // No valid windows in this tab at all — create a scratch window.
        let buf_id = self.buffer_manager.create();
        let new_wid = crate::core::window::WindowId(self.next_window_id);
        self.next_window_id += 1;
        let window = crate::core::window::Window::new(new_wid, buf_id);
        self.windows.insert(new_wid, window);
        let tab = self.active_tab_mut();
        tab.layout = crate::core::window::WindowLayout::leaf(new_wid);
        tab.active_window = new_wid;
    }

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
        let id = self.active_window_id();
        self.windows.get(&id).unwrap_or_else(|| {
            panic!(
                "BUG: active_window WindowId({}) not in windows map (map has {} entries). \
                 Please report this at https://github.com/anthropics/claude-code/issues",
                id.0,
                self.windows.len()
            )
        })
    }

    pub fn active_window_mut(&mut self) -> &mut Window {
        // Self-heal: if the active window ID is stale, repair before accessing.
        self.repair_active_window();
        let id = self.active_window_id();
        self.windows
            .get_mut(&id)
            .expect("repair_active_window should have fixed this")
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

        // Propagate git statuses up to parent directories so that a folder
        // shows modified/added color when any descendant file has that status.
        // Priority: M > D > R > A > ?
        fn git_priority(c: char) -> u8 {
            match c {
                'M' => 5,
                'D' => 4,
                'R' => 3,
                'A' => 2,
                '?' => 1,
                _ => 0,
            }
        }
        let git_file_paths: Vec<PathBuf> = git_statuses.keys().cloned().collect();
        for file_path in &git_file_paths {
            let status = git_statuses[file_path];
            let mut ancestor = file_path.parent();
            while let Some(dir) = ancestor {
                let entry = git_statuses.entry(dir.to_path_buf()).or_insert(status);
                if git_priority(status) > git_priority(*entry) {
                    *entry = status;
                }
                ancestor = dir.parent();
                if dir == self.cwd {
                    break;
                }
            }
        }

        // Propagate diagnostic counts up to parent directories so that a
        // folder shows error/warning color when any descendant file has issues.
        let file_paths: Vec<PathBuf> = diag_counts.keys().cloned().collect();
        for file_path in &file_paths {
            let (errors, warnings) = diag_counts[file_path];
            if errors == 0 && warnings == 0 {
                continue;
            }
            let mut ancestor = file_path.parent();
            while let Some(dir) = ancestor {
                let entry = diag_counts.entry(dir.to_path_buf()).or_insert((0, 0));
                entry.0 += errors;
                entry.1 += warnings;
                ancestor = dir.parent();
                // Stop at the cwd to avoid propagating to unrelated dirs.
                if dir == self.cwd {
                    break;
                }
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

    // =======================================================================
    // Sidebar focus helpers
    // =======================================================================

    /// Returns true if any sidebar panel currently has keyboard focus.
    #[allow(dead_code)]
    pub fn sidebar_has_focus(&self) -> bool {
        self.explorer_has_focus
            || self.search_has_focus
            || self.sc_has_focus
            || self.dap_sidebar_has_focus
            || self.ext_sidebar_has_focus
            || self.ai_has_focus
            || self.settings_has_focus
            || self.ext_panel_has_focus
    }

    /// Clear all sidebar panel focus flags at once.
    pub fn clear_sidebar_focus(&mut self) {
        self.explorer_has_focus = false;
        self.search_has_focus = false;
        self.sc_has_focus = false;
        self.dap_sidebar_has_focus = false;
        self.ext_sidebar_has_focus = false;
        self.ai_has_focus = false;
        self.settings_has_focus = false;
        self.ext_panel_has_focus = false;
    }
}
