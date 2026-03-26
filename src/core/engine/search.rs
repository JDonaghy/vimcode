use super::*;

impl Engine {
    // =======================================================================
    // Cursor helpers (delegating to buffer/view)
    // =======================================================================

    pub(crate) fn get_max_cursor_col(&self, line_idx: usize) -> usize {
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

    pub fn clamp_cursor_col(&mut self) {
        let line = self.view().cursor.line;
        let max_col = self.get_max_cursor_col(line);
        let view = self.view_mut();
        if view.cursor.col > max_col {
            view.cursor.col = max_col;
        }
    }

    /// Ensure the cursor is visible within the viewport, adjusting scroll_top.
    pub fn ensure_cursor_visible(&mut self) {
        if self.settings.wrap && self.view().viewport_cols > 0 {
            self.ensure_cursor_visible_wrap();
        } else {
            let scrolloff = self.settings.scrolloff;
            if scrolloff == 0 {
                self.view_mut().ensure_cursor_visible();
            } else {
                let cursor_line = self.view().cursor.line;
                let viewport_lines = self.view().viewport_lines;
                let scroll_top = self.view().scroll_top;
                if cursor_line < scroll_top + scrolloff {
                    self.view_mut().scroll_top = cursor_line.saturating_sub(scrolloff);
                } else if viewport_lines > 0
                    && cursor_line + scrolloff + 1 > scroll_top + viewport_lines
                {
                    self.view_mut().scroll_top = cursor_line + scrolloff + 1 - viewport_lines;
                }
            }
        }
    }

    /// Wrap-aware scroll-to-cursor. Counts visual rows (accounting for
    /// soft-wrapped buffer lines) to determine when to adjust `scroll_top`.
    pub(crate) fn ensure_cursor_visible_wrap(&mut self) {
        let viewport_cols = self.view().viewport_cols;
        let viewport_lines = self.view().viewport_lines;
        let cursor_line = self.view().cursor.line;
        let cursor_col = self.view().cursor.col;
        let scroll_top = self.view().scroll_top;
        let total_lines = self.buffer().len_lines();

        // Scroll up if cursor is above the viewport.
        if cursor_line < scroll_top {
            self.view_mut().scroll_top = cursor_line;
            return;
        }

        if viewport_lines == 0 {
            return;
        }

        // Count visual rows from scroll_top up to and including the cursor's
        // visual row within cursor_line.
        let mut visual_rows: usize = 0;
        for r in scroll_top..=cursor_line {
            let line_len = self.buffer().content.line(r).len_chars().saturating_sub(1);
            if r < cursor_line {
                visual_rows += engine_visual_rows_for_line(line_len, viewport_cols);
            } else {
                // Partial count: only up to the cursor's visual segment.
                visual_rows += cursor_col / viewport_cols + 1;
            }
        }

        // If cursor fits within the viewport, nothing to adjust.
        if visual_rows <= viewport_lines {
            return;
        }

        // Cursor is below the viewport — walk backwards from cursor_line
        // to find the new scroll_top that makes the cursor visible.
        let cursor_visual_row_within_line = cursor_col / viewport_cols;
        // Rows the cursor's line contributes, up to and including cursor segment.
        let mut rows_used = cursor_visual_row_within_line + 1;
        let mut new_scroll_top = cursor_line;
        if rows_used < viewport_lines && cursor_line > 0 {
            for r in (0..cursor_line).rev() {
                let line_len = self.buffer().content.line(r).len_chars().saturating_sub(1);
                let vrows = engine_visual_rows_for_line(line_len, viewport_cols);
                if rows_used + vrows > viewport_lines {
                    break;
                }
                rows_used += vrows;
                new_scroll_top = r;
            }
        }
        // Clamp scroll_top to valid range.
        new_scroll_top = new_scroll_top.min(total_lines.saturating_sub(1));
        self.view_mut().scroll_top = new_scroll_top;
    }

    /// Move cursor down by one visual row (within the same wrapped line if
    /// possible, otherwise to the next buffer line).  Used by `gj`.
    pub(crate) fn move_visual_down(&mut self) {
        let vp = self.view().viewport_cols.max(1);
        let cursor_line = self.view().cursor.line;
        let cursor_col = self.view().cursor.col;
        let total_lines = self.buffer().len_lines();
        let line_len = self
            .buffer()
            .content
            .line(cursor_line)
            .len_chars()
            .saturating_sub(1);
        let visual_col = cursor_col % vp;

        if cursor_col + vp < line_len {
            // Advance within the same buffer line (to the next wrapped segment).
            self.view_mut().cursor.col = cursor_col + vp;
        } else if cursor_line + 1 < total_lines {
            // Move to the next buffer line, keeping the same visual column offset.
            let next_len = self
                .buffer()
                .content
                .line(cursor_line + 1)
                .len_chars()
                .saturating_sub(1);
            self.view_mut().cursor.line = cursor_line + 1;
            self.view_mut().cursor.col = visual_col.min(next_len.saturating_sub(1));
        }
        self.clamp_cursor_col();
        self.ensure_cursor_visible();
    }

    /// Move cursor up by one visual row (within the same wrapped line if
    /// possible, otherwise to the previous buffer line).  Used by `gk`.
    pub(crate) fn move_visual_up(&mut self) {
        let vp = self.view().viewport_cols.max(1);
        let cursor_line = self.view().cursor.line;
        let cursor_col = self.view().cursor.col;
        let visual_col = cursor_col % vp;

        if cursor_col >= vp {
            // Move up within the same buffer line (to the previous wrapped segment).
            self.view_mut().cursor.col = cursor_col - vp;
        } else if cursor_line > 0 {
            // Move to the previous buffer line's last visual segment.
            let prev_len = self
                .buffer()
                .content
                .line(cursor_line - 1)
                .len_chars()
                .saturating_sub(1);
            let last_seg_start = (prev_len / vp) * vp;
            let target_col = (last_seg_start + visual_col).min(prev_len.saturating_sub(1));
            self.view_mut().cursor.line = cursor_line - 1;
            self.view_mut().cursor.col = target_col;
        }
        self.clamp_cursor_col();
        self.ensure_cursor_visible();
    }

    /// Synchronise the scroll_top of scroll-bound window pairs.
    /// Called after every key that may move the cursor or scroll, and also
    /// after direct scroll_top mutations (e.g. scrollbar drag).
    pub fn sync_scroll_binds(&mut self) {
        if self.scroll_bind_pairs.is_empty() {
            return;
        }
        let active_id = self.active_window_id();
        let active_win = &self.windows[&active_id];
        let active_scroll = active_win.view.scroll_top;
        let active_buf_id = active_win.buffer_id;
        let active_lines = self
            .buffer_manager
            .get(active_buf_id)
            .map(|s| s.buffer.len_lines())
            .unwrap_or(1)
            .max(1);

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
                let partner_buf_id = self.windows.get(&pid).map(|w| w.buffer_id);
                let is_md_pair = partner_buf_id.is_some_and(|pb| {
                    self.md_preview_links.contains_key(&pb)
                        || self.md_preview_links.contains_key(&active_buf_id)
                });
                if let Some(w) = self.windows.get_mut(&pid) {
                    if is_md_pair {
                        // Proportional scroll: map source position to preview position.
                        let partner_lines = partner_buf_id
                            .and_then(|id| self.buffer_manager.get(id))
                            .map(|s| s.buffer.len_lines())
                            .unwrap_or(1)
                            .max(1);
                        let ratio = active_scroll as f64 / active_lines as f64;
                        w.view.scroll_top = ((ratio * partner_lines as f64).round() as usize)
                            .min(partner_lines.saturating_sub(1));
                    } else if let (Some(active_aligned), Some(partner_aligned)) = (
                        self.diff_aligned.get(&active_id),
                        self.diff_aligned.get(&pid),
                    ) {
                        // Aligned diff scroll: map active scroll_top through
                        // aligned sequences so both sides stay in visual lockstep
                        // even when one side has large padding blocks.
                        let target_idx = active_aligned
                            .iter()
                            .position(|e| e.source_line == Some(active_scroll))
                            .unwrap_or_else(|| {
                                // Fallback: find nearest aligned entry at or after active_scroll.
                                active_aligned
                                    .iter()
                                    .position(|e| e.source_line.is_some_and(|l| l >= active_scroll))
                                    .unwrap_or(active_aligned.len().saturating_sub(1))
                            });
                        // Map that aligned index to the partner's buffer line.
                        let partner_line = if target_idx < partner_aligned.len() {
                            partner_aligned[target_idx..]
                                .iter()
                                .find_map(|e| e.source_line)
                                .or_else(|| {
                                    partner_aligned[..target_idx]
                                        .iter()
                                        .rev()
                                        .find_map(|e| e.source_line)
                                })
                                .unwrap_or(0)
                        } else {
                            partner_aligned
                                .last()
                                .and_then(|e| e.source_line)
                                .unwrap_or(0)
                        };
                        w.view.scroll_top = partner_line;
                    } else {
                        w.view.scroll_top = active_scroll;
                    }
                }
            }
        }
    }

    // =======================================================================
    // Project search
    // =======================================================================

    /// Run a project-wide search synchronously (blocks until complete).
    ///
    /// Prefer `start_project_search` + `poll_project_search` for UI use.
    /// Used directly in tests.
    #[allow(dead_code)]
    pub fn run_project_search(&mut self, root: &Path) {
        let query = self.project_search_query.clone();
        if query.is_empty() {
            self.project_search_results.clear();
            self.project_search_selected = 0;
            self.message = "Search query is empty".to_string();
            return;
        }
        let opts = self.project_search_options.clone();
        match project_search::search_in_project(root, &query, &opts) {
            Ok(results) => self.apply_search_results(results, &query),
            Err(e) => {
                self.project_search_results.clear();
                self.project_search_selected = 0;
                self.message = format!("Invalid regex: {}", e.0);
            }
        }
    }

    /// Spawn a background thread to search `root` for `self.project_search_query`.
    ///
    /// Call `poll_project_search` on each UI tick to collect results.
    pub fn start_project_search(&mut self, root: PathBuf) {
        let query = self.project_search_query.clone();
        if query.is_empty() {
            self.project_search_results.clear();
            self.project_search_selected = 0;
            self.message = "Search query is empty".to_string();
            return;
        }
        self.project_search_running = true;
        self.message = format!("Searching for \"{}\"…", query);
        let opts = self.project_search_options.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.project_search_receiver = Some(rx);
        std::thread::spawn(move || {
            let result = project_search::search_in_project(&root, &query, &opts);
            let _ = tx.send(result);
        });
    }

    /// Check whether the background search thread has finished and, if so, store results.
    ///
    /// Returns `true` when new results have just arrived (UI should redraw).
    pub fn poll_project_search(&mut self) -> bool {
        let result = match self.project_search_receiver {
            Some(ref rx) => match rx.try_recv() {
                Ok(r) => r,
                Err(_) => return false,
            },
            None => return false,
        };
        let query = self.project_search_query.clone();
        self.project_search_receiver = None;
        self.project_search_running = false;
        match result {
            Ok(results) => self.apply_search_results(results, &query),
            Err(e) => {
                self.project_search_results.clear();
                self.project_search_selected = 0;
                self.message = format!("Invalid regex: {}", e.0);
            }
        }
        true
    }

    /// Store search results and update the status message. Called by both sync and async paths.
    pub(crate) fn apply_search_results(&mut self, results: Vec<ProjectMatch>, query: &str) {
        let capped = results.len() >= 10_000;
        if results.is_empty() {
            self.message = format!("No results for \"{}\"", query);
        } else {
            let file_count = {
                let mut files: Vec<&std::path::Path> =
                    results.iter().map(|m| m.file.as_path()).collect();
                files.sort();
                files.dedup();
                files.len()
            };
            self.message = format!(
                "{} match{} in {} file{}{}",
                results.len(),
                if results.len() == 1 { "" } else { "es" },
                file_count,
                if file_count == 1 { "" } else { "s" },
                if capped { " (capped at 10000)" } else { "" }
            );
        }
        self.project_search_results = results;
        self.project_search_selected = 0;
    }

    /// Toggle case-sensitive project search.
    pub fn toggle_project_search_case(&mut self) {
        self.project_search_options.case_sensitive = !self.project_search_options.case_sensitive;
    }

    /// Toggle whole-word project search.
    pub fn toggle_project_search_whole_word(&mut self) {
        self.project_search_options.whole_word = !self.project_search_options.whole_word;
    }

    /// Toggle regex project search.
    pub fn toggle_project_search_regex(&mut self) {
        self.project_search_options.use_regex = !self.project_search_options.use_regex;
    }

    /// Move the project search selection down by one, clamped to the last result.
    pub fn project_search_select_next(&mut self) {
        if !self.project_search_results.is_empty() {
            self.project_search_selected =
                (self.project_search_selected + 1).min(self.project_search_results.len() - 1);
        }
    }

    /// Move the project search selection up by one, clamped to 0.
    pub fn project_search_select_prev(&mut self) {
        self.project_search_selected = self.project_search_selected.saturating_sub(1);
    }

    // =======================================================================
    // Project replace
    // =======================================================================

    /// Collect canonical paths of all dirty (unsaved) buffers.
    pub(crate) fn dirty_buffer_paths(&self) -> std::collections::HashSet<PathBuf> {
        let mut paths = std::collections::HashSet::new();
        for id in self.buffer_manager.list() {
            if let Some(state) = self.buffer_manager.get(id) {
                if state.dirty {
                    if let Some(ref p) = state.file_path {
                        let canonical = p.canonicalize().unwrap_or_else(|_| p.clone());
                        paths.insert(canonical);
                    }
                }
            }
        }
        paths
    }

    /// Run a project-wide replace synchronously (blocks until complete).
    /// Used directly in tests.
    #[allow(dead_code)]
    pub fn run_project_replace(&mut self, root: &Path) {
        let query = self.project_search_query.clone();
        let replacement = self.project_replace_text.clone();
        if query.is_empty() {
            self.message = "Search query is empty".to_string();
            return;
        }
        let opts = self.project_search_options.clone();
        let skip = self.dirty_buffer_paths();
        match project_search::replace_in_project(root, &query, &replacement, &opts, &skip) {
            Ok(rr) => self.apply_replace_result(rr),
            Err(e) => {
                self.message = format!("Invalid regex: {}", e.0);
            }
        }
    }

    /// Spawn a background thread to replace across `root`.
    ///
    /// Call `poll_project_replace` on each UI tick to collect results.
    pub fn start_project_replace(&mut self, root: PathBuf) {
        let query = self.project_search_query.clone();
        let replacement = self.project_replace_text.clone();
        if query.is_empty() {
            self.message = "Search query is empty".to_string();
            return;
        }
        self.project_replace_running = true;
        self.message = format!("Replacing \"{}\" → \"{}\"…", query, replacement);
        let opts = self.project_search_options.clone();
        let skip = self.dirty_buffer_paths();
        let (tx, rx) = std::sync::mpsc::channel();
        self.project_replace_receiver = Some(rx);
        std::thread::spawn(move || {
            let result =
                project_search::replace_in_project(&root, &query, &replacement, &opts, &skip);
            let _ = tx.send(result);
        });
    }

    /// Check whether the background replace thread has finished.
    ///
    /// Returns `true` when the replace has just completed (UI should redraw).
    pub fn poll_project_replace(&mut self) -> bool {
        let result = match self.project_replace_receiver {
            Some(ref rx) => match rx.try_recv() {
                Ok(r) => r,
                Err(_) => return false,
            },
            None => return false,
        };
        self.project_replace_receiver = None;
        self.project_replace_running = false;
        match result {
            Ok(rr) => self.apply_replace_result(rr),
            Err(e) => {
                self.message = format!("Replace error: {}", e.0);
            }
        }
        true
    }

    /// Apply a completed replace result: reload modified buffers, update status.
    pub(crate) fn apply_replace_result(&mut self, rr: ReplaceResult) {
        // Reload open buffers for modified files.
        for modified_path in &rr.modified_files {
            let canonical = modified_path
                .canonicalize()
                .unwrap_or_else(|_| modified_path.clone());
            // Find the buffer for this file and reload its content from disk.
            let buf_id = self.buffer_manager.list().into_iter().find(|&id| {
                self.buffer_manager.get(id).is_some_and(|s| {
                    s.file_path.as_ref().is_some_and(|p| {
                        p.canonicalize().unwrap_or_else(|_| p.clone()) == canonical
                    })
                })
            });
            if let Some(id) = buf_id {
                if let Ok(content) = std::fs::read_to_string(modified_path) {
                    if let Some(state) = self.buffer_manager.get_mut(id) {
                        state.buffer.content = ropey::Rope::from_str(&content);
                        state.dirty = false;
                        state.undo_stack.clear();
                        state.redo_stack.clear();
                    }
                    self.refresh_git_diff(id);
                }
            }
        }

        // Build status message.
        let mut msg = format!(
            "Replaced {} occurrence{} in {} file{}",
            rr.replacement_count,
            if rr.replacement_count == 1 { "" } else { "s" },
            rr.file_count,
            if rr.file_count == 1 { "" } else { "s" },
        );
        if !rr.skipped_files.is_empty() {
            msg.push_str(&format!(
                " ({} file{} skipped — unsaved changes)",
                rr.skipped_files.len(),
                if rr.skipped_files.len() == 1 { "" } else { "s" },
            ));
        }
        self.message = msg;

        // Clear stale search results since files have changed.
        self.project_search_results.clear();
        self.project_search_selected = 0;
    }
}
