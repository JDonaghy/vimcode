use super::*;

pub enum SearchInputAction {
    Consumed,
    StartSearch,
    StartReplace,
    Unfocused,
    Ignored,
}

pub enum SearchKeyResult {
    Consumed,
    Unfocused,
}

/// Find word boundaries around char position `pos` in `text`.
/// Returns `(start, end)` where `start..end` is the word range.
/// A "word" character is alphanumeric or underscore.
/// If `pos` is not on a word character, returns `(pos, pos)`.
/// Used by TUI/GTK/Win-GUI backends for double-click word select in find/replace.
#[allow(dead_code)]
pub fn find_word_boundaries(text: &str, pos: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    if pos >= chars.len() {
        return (chars.len(), chars.len());
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    if !is_word(chars[pos]) {
        return (pos, pos);
    }
    let start = (0..=pos)
        .rev()
        .take_while(|&i| is_word(chars[i]))
        .last()
        .unwrap_or(pos);
    let end = (pos..chars.len())
        .take_while(|&i| is_word(chars[i]))
        .last()
        .map(|i| i + 1)
        .unwrap_or(pos);
    (start, end)
}

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
    ///
    /// #185: `view.viewport_lines` is updated by the backends on the next
    /// draw pass, but engine-side operations (e.g. `quickfix_jump`)
    /// may call this synchronously BEFORE the next draw — at which
    /// point the cached `viewport_lines` reflects the previous chrome
    /// configuration. If a panel (quickfix, terminal, debug output,
    /// wildmenu, debug toolbar) just opened, the cursor can be
    /// positioned into what used to be the visible area but is now
    /// hidden behind the new panel. Compute an effective viewport
    /// that subtracts the currently-visible bottom chrome here.
    fn effective_viewport_lines(&self) -> usize {
        let cached = self.view().viewport_lines;
        let bottom_chrome = self.bottom_chrome_rows_since_last_draw();
        cached.saturating_sub(bottom_chrome)
    }

    /// Rows of bottom-of-editor chrome that may have been added since
    /// the last draw refreshed `view.viewport_lines`. Used to keep
    /// `ensure_cursor_visible` from scrolling the cursor into a newly
    /// opened quickfix / terminal / wildmenu panel's area.
    ///
    /// Backends are the source of truth for chrome heights and
    /// normally keep `viewport_lines` fresh — this helper is the
    /// safety-net for the "state changed in the current tick, draw
    /// hasn't happened yet" window.
    fn bottom_chrome_rows_since_last_draw(&self) -> usize {
        // Quickfix panel. Fixed 6-row height in both backends when open.
        let qf = if self.quickfix_open && !self.quickfix_items.is_empty() {
            6
        } else {
            0
        };
        // Terminal / bottom panel. `effective_terminal_panel_rows` +
        // 2 (1 for the tab bar, 1 for the panel header) matches what
        // both backends reserve.
        let term = if self.terminal_open || self.bottom_panel_open {
            // `0` as the `target` argument gives the current effective
            // size — we're not in a maximize transition here, just
            // reading the steady-state row count.
            self.effective_terminal_panel_rows(0).saturating_add(2) as usize
        } else {
            0
        };
        // Wildmenu row (command-line Tab-complete).
        let wildmenu = if !self.wildmenu_items.is_empty() {
            1
        } else {
            0
        };
        // Debug toolbar row (F5 / F6 / F9 / F10 / F11 buttons).
        let dbg = if self.debug_toolbar_visible { 1 } else { 0 };
        qf + term + wildmenu + dbg
    }

    /// Ensure the cursor is visible within the viewport, adjusting scroll_top.
    pub fn ensure_cursor_visible(&mut self) {
        if self.settings.wrap && self.view().viewport_cols > 0 {
            self.ensure_cursor_visible_wrap();
        } else {
            let scrolloff = self.settings.scrolloff;
            let viewport_lines = self.effective_viewport_lines();
            let cursor_line = self.view().cursor.line;
            let scroll_top = self.view().scroll_top;
            if cursor_line < scroll_top + scrolloff {
                self.view_mut().scroll_top = cursor_line.saturating_sub(scrolloff);
            } else if viewport_lines > 0
                && cursor_line + scrolloff + 1 > scroll_top + viewport_lines
            {
                self.view_mut().scroll_top = cursor_line + scrolloff + 1 - viewport_lines;
            }

            // Horizontal: keep cursor within the visible column range.
            // Prefer paint-time viewport_cols (exact) over the resize
            // handler's approximate value.
            let wid = self.active_window_id();
            let viewport_cols = self
                .paint_viewport_cols
                .borrow()
                .get(&wid)
                .copied()
                .unwrap_or(self.view().viewport_cols);
            let cursor_col = self.view().cursor.col;
            let scroll_left = self.view().scroll_left;
            if viewport_cols > 0 {
                if cursor_col < scroll_left {
                    self.view_mut().scroll_left = cursor_col;
                } else if cursor_col >= scroll_left + viewport_cols {
                    self.view_mut().scroll_left = cursor_col + 1 - viewport_cols;
                }
            }
        }
    }

    /// Wrap-aware scroll-to-cursor. Counts visual rows (accounting for
    /// soft-wrapped buffer lines) to determine when to adjust `scroll_top`.
    pub(crate) fn ensure_cursor_visible_wrap(&mut self) {
        let viewport_cols = self.view().viewport_cols;
        // #185: use effective viewport (accounts for bottom chrome like
        // the quickfix panel that may have opened in the current tick).
        let viewport_lines = self.effective_viewport_lines();
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

    /// Move cursor to the start of the current screen line (`g0` / `g<Home>`).
    /// When wrap is off, equivalent to `0`.
    pub(crate) fn move_screen_line_start(&mut self) {
        if !self.settings.wrap {
            self.view_mut().cursor.col = 0;
        } else {
            let vp = self.view().viewport_cols.max(1);
            let col = self.view().cursor.col;
            self.view_mut().cursor.col = (col / vp) * vp;
        }
        self.ensure_cursor_visible();
    }

    /// Move cursor to the first non-blank character on the current screen line (`g^`).
    /// When wrap is off, equivalent to `^`.
    pub(crate) fn move_screen_line_first_non_blank(&mut self) {
        if !self.settings.wrap {
            let line = self.view().cursor.line;
            let fnb = self.first_non_blank_col(line);
            self.view_mut().cursor.col = fnb;
        } else {
            let vp = self.view().viewport_cols.max(1);
            let col = self.view().cursor.col;
            let seg_start = (col / vp) * vp;
            let line = self.view().cursor.line;
            let line_start = self.buffer().line_to_char(line);
            let line_len = self.buffer().line_len_chars(line);
            let seg_end = (seg_start + vp).min(line_len);
            let mut target = seg_start;
            for i in seg_start..seg_end {
                let ch = self.buffer().content.char(line_start + i);
                if ch != ' ' && ch != '\t' && ch != '\n' && ch != '\r' {
                    target = i;
                    break;
                }
            }
            self.view_mut().cursor.col = target;
        }
        self.ensure_cursor_visible();
    }

    /// Move cursor to the end of the current screen line (`g$` / `g<End>`).
    /// When wrap is off, equivalent to `$`.
    pub(crate) fn move_screen_line_end(&mut self) {
        let line = self.view().cursor.line;
        let line_len = self
            .buffer()
            .content
            .line(line)
            .len_chars()
            .saturating_sub(1); // exclude newline
        if !self.settings.wrap {
            self.view_mut().cursor.col = line_len.saturating_sub(1);
        } else {
            let vp = self.view().viewport_cols.max(1);
            let col = self.view().cursor.col;
            let seg_start = (col / vp) * vp;
            let seg_end = (seg_start + vp).min(line_len);
            self.view_mut().cursor.col = seg_end.saturating_sub(1);
        }
        self.clamp_cursor_col();
        self.ensure_cursor_visible();
    }

    /// Synchronise the scroll_top of scroll-bound window pairs.
    /// Called after every key that may move the cursor or scroll, and also
    /// after direct scroll_top mutations (e.g. scrollbar drag).
    ///
    /// In aligned-diff mode, also stamps `view.aligned_top` on both panes
    /// to a single shared aligned-row index so the render starts both
    /// panes at exactly the same row. Without this, scrolling past a
    /// hunk drifts the panes by `(adds − removes)` rows of every prior
    /// hunk because `scroll_top` (a buffer line) cannot identify a
    /// padding row uniquely (#166).
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

        // Pre-compute the active pane's effective aligned-row index from
        // its scroll_top using the same seek + backup-over-padding logic
        // the render uses, so both sides see one consistent index.
        let active_aligned_top: Option<usize> = self.diff_aligned.get(&active_id).map(|aligned| {
            let seek_idx = aligned
                .iter()
                .position(|e| e.source_line.is_some_and(|l| l >= active_scroll))
                .unwrap_or(aligned.len().saturating_sub(1));
            let mut k = seek_idx;
            while k > 0 && aligned[k - 1].source_line.is_none() {
                k -= 1;
            }
            k
        });

        // Stamp active's aligned_top up front; mirror onto partners below.
        if let Some(w) = self.windows.get_mut(&active_id) {
            w.view.aligned_top = active_aligned_top;
        }

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
                // Compute partner's target (line + aligned_top) ahead of
                // taking a mutable borrow on the partner window.
                let partner_aligned_target: Option<(usize, usize)> =
                    match (active_aligned_top, self.diff_aligned.get(&pid)) {
                        (Some(k), Some(partner_aligned)) if !partner_aligned.is_empty() => {
                            let line = if k < partner_aligned.len() {
                                partner_aligned[k..]
                                    .iter()
                                    .find_map(|e| e.source_line)
                                    .or_else(|| {
                                        partner_aligned[..k]
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
                            Some((k, line))
                        }
                        _ => None,
                    };
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
                        w.view.aligned_top = None;
                    } else if let Some((aligned_top, partner_line)) = partner_aligned_target {
                        w.view.scroll_top = partner_line;
                        w.view.aligned_top = Some(aligned_top);
                    } else {
                        w.view.scroll_top = active_scroll;
                        w.view.aligned_top = None;
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
            self.message = "Search query is empty".to_string();
            return;
        }
        let opts = self.project_search_options.clone();
        match project_search::search_in_project(root, &query, &opts) {
            Ok(results) => self.apply_search_results(results, &query),
            Err(e) => {
                self.project_search_results.clear();
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
            self.message = "Search query is empty".to_string();
            return;
        }
        self.project_search_running = true;
        self.notify(
            NotificationKind::ProjectSearch,
            &format!("Searching for \"{query}\"…"),
        );
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
        self.notify_done_by_kind(&NotificationKind::ProjectSearch, Some("Search complete"));
        match result {
            Ok(results) => self.apply_search_results(results, &query),
            Err(e) => {
                self.project_search_results.clear();
                self.message = format!("Invalid regex: {}", e.0);
            }
        }
        true
    }

    /// Store search results and update the status message. Called by both sync and async paths.
    pub(crate) fn apply_search_results(&mut self, results: Vec<ProjectMatch>, query: &str) {
        let capped = results.len() >= 10_000;
        let file_count = {
            let mut files: Vec<&std::path::Path> =
                results.iter().map(|m| m.file.as_path()).collect();
            files.sort();
            files.dedup();
            files.len()
        };
        let status = if results.is_empty() {
            format!("No results for \"{}\"", query)
        } else {
            format!(
                "{} match{} in {} file{}{}",
                results.len(),
                if results.len() == 1 { "" } else { "es" },
                file_count,
                if file_count == 1 { "" } else { "s" },
                if capped { " (capped at 10000)" } else { "" }
            )
        };
        self.message = status.clone();
        self.project_search_status = status;
        self.project_search_results = results;
        self.search_collapsed_files.borrow_mut().clear();
        self.search_sidebar_system
            .borrow_mut()
            .set_selected_path(1, None);
    }

    /// Toggle case-sensitive project search and re-run if a query exists.
    pub fn toggle_project_search_case(&mut self) {
        self.project_search_options.case_sensitive = !self.project_search_options.case_sensitive;
        self.rerun_project_search_if_active();
    }

    /// Toggle whole-word project search and re-run if a query exists.
    pub fn toggle_project_search_whole_word(&mut self) {
        self.project_search_options.whole_word = !self.project_search_options.whole_word;
        self.rerun_project_search_if_active();
    }

    /// Toggle regex project search and re-run if a query exists.
    pub fn toggle_project_search_regex(&mut self) {
        self.project_search_options.use_regex = !self.project_search_options.use_regex;
        self.rerun_project_search_if_active();
    }

    fn rerun_project_search_if_active(&mut self) {
        if !self.project_search_query.is_empty() && !self.project_search_results.is_empty() {
            let root = self.cwd.clone();
            self.start_project_search(root);
        }
    }

    pub fn handle_search_input_key(
        &mut self,
        key: &str,
        unicode: Option<char>,
    ) -> SearchInputAction {
        let focus = self.search_panel_form_focus.borrow().clone();
        let is_replace = focus.as_deref() == Some("search:replace");
        let is_query = focus.as_deref() == Some("search:query");

        match key {
            "Return" => {
                if is_replace {
                    let cwd = self.cwd.clone();
                    self.start_project_replace(cwd);
                    return SearchInputAction::StartReplace;
                }
                let cwd = self.cwd.clone();
                self.start_project_search(cwd);
                SearchInputAction::StartSearch
            }
            "BackSpace" => {
                self.search_input_backspace(is_replace);
                SearchInputAction::Consumed
            }
            "Delete" => {
                self.search_input_delete(is_replace);
                SearchInputAction::Consumed
            }
            "Left" | "Right" | "Home" | "End" => {
                self.search_input_move_caret(is_replace, key);
                SearchInputAction::Consumed
            }
            "Tab" | "BackTab" => {
                if is_query {
                    self.search_panel_form_focus
                        .replace(Some("search:replace".to_string()));
                } else {
                    self.search_panel_form_focus
                        .replace(Some("search:query".to_string()));
                }
                SearchInputAction::Consumed
            }
            "Escape" => {
                self.search_panel_form_focus.replace(None);
                SearchInputAction::Unfocused
            }
            _ => {
                if let Some(ch) = unicode {
                    let target_replace = if !is_query && !is_replace {
                        self.search_panel_form_focus
                            .replace(Some("search:query".to_string()));
                        false
                    } else {
                        is_replace
                    };
                    self.search_input_insert_char(target_replace, ch);
                    SearchInputAction::Consumed
                } else {
                    SearchInputAction::Ignored
                }
            }
        }
    }

    pub fn search_input_insert_char(&mut self, is_replace: bool, ch: char) {
        let (text, caret) = if is_replace {
            (&mut self.project_replace_text, &self.replace_text_caret)
        } else {
            (&mut self.project_search_query, &self.search_query_caret)
        };
        let pos = caret.get().min(text.len());
        text.insert(pos, ch);
        caret.set(pos + ch.len_utf8());
    }

    pub fn search_input_backspace(&mut self, is_replace: bool) {
        let (text, caret) = if is_replace {
            (&mut self.project_replace_text, &self.replace_text_caret)
        } else {
            (&mut self.project_search_query, &self.search_query_caret)
        };
        let pos = caret.get().min(text.len());
        if pos == 0 {
            return;
        }
        let prev = text[..pos]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        text.remove(prev);
        caret.set(prev);
    }

    pub fn search_input_delete(&mut self, is_replace: bool) {
        let (text, caret) = if is_replace {
            (&mut self.project_replace_text, &self.replace_text_caret)
        } else {
            (&mut self.project_search_query, &self.search_query_caret)
        };
        let pos = caret.get().min(text.len());
        if pos < text.len() {
            text.remove(pos);
        }
    }

    pub fn search_input_move_caret(&mut self, is_replace: bool, key: &str) {
        let (text, caret) = if is_replace {
            (&mut self.project_replace_text, &self.replace_text_caret)
        } else {
            (&mut self.project_search_query, &self.search_query_caret)
        };
        let pos = caret.get().min(text.len());
        match key {
            "Left" => {
                let prev = text[..pos]
                    .char_indices()
                    .next_back()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                caret.set(prev);
            }
            "Right" => {
                let next = text[pos..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| pos + i)
                    .unwrap_or(text.len());
                caret.set(next);
            }
            "Home" => caret.set(0),
            "End" => caret.set(text.len()),
            _ => {}
        }
    }

    pub fn search_input_paste(&mut self, is_replace: bool, clip: &str) {
        let line = clip.lines().next().unwrap_or("");
        for ch in line.chars() {
            if !ch.is_control() {
                self.search_input_insert_char(is_replace, ch);
            }
        }
    }

    /// Handle a click on a search-panel form element (toggle, button) by widget ID.
    pub fn handle_search_form_hit(&mut self, id: &str) {
        match id {
            "search:case" => self.toggle_project_search_case(),
            "search:word" => self.toggle_project_search_whole_word(),
            "search:regex" => self.toggle_project_search_regex(),
            "search:find_next" => {
                let root = self.cwd.clone();
                self.start_project_search(root);
            }
            "search:replace_all" => {
                if self.project_search_results.is_empty() {
                    self.message = "No search results to replace".to_string();
                    return;
                }
                let query = self.project_search_query.clone();
                let replace = self.project_replace_text.clone();
                let file_count = {
                    let mut files: Vec<&std::path::Path> = self
                        .project_search_results
                        .iter()
                        .map(|m| m.file.as_path())
                        .collect();
                    files.sort();
                    files.dedup();
                    files.len()
                };
                let match_count = self.project_search_results.len();
                let replace_display = if replace.is_empty() {
                    "(empty string)".to_string()
                } else {
                    format!("\"{}\"", replace)
                };
                self.show_dialog(
                    "search_replace_all",
                    "Replace All",
                    vec![format!(
                        "Replace {} occurrence{} of \"{}\" with {} across {} file{}?",
                        match_count,
                        if match_count == 1 { "" } else { "s" },
                        query,
                        replace_display,
                        file_count,
                        if file_count == 1 { "" } else { "s" },
                    )],
                    vec![
                        DialogButton {
                            label: "Cancel".to_string(),
                            hotkey: 'c',
                            action: "cancel".to_string(),
                        },
                        DialogButton {
                            label: "Replace All".to_string(),
                            hotkey: 'r',
                            action: "replace".to_string(),
                        },
                    ],
                );
            }
            "search:replace_next" => {
                if let Some(idx) = self.search_selected_result_idx() {
                    self.open_search_result(idx);
                }
            }
            "search:buttons" => {
                let root = self.cwd.clone();
                self.start_project_search(root);
            }
            "search:toggles" => {}
            "search:query" => {
                self.search_panel_form_focus
                    .replace(Some("search:query".to_string()));
            }
            "search:replace" => {
                self.search_panel_form_focus
                    .replace(Some("search:replace".to_string()));
            }
            _ => {}
        }
    }

    /// Map a tree path (file_idx, match_within_file) back to a flat result index.
    fn tree_path_to_result_idx(&self, file_idx: usize, match_idx: usize) -> Option<usize> {
        let mut current_file: usize = 0;
        let mut last_file: Option<&std::path::Path> = None;
        let mut match_within: usize = 0;

        for (i, m) in self.project_search_results.iter().enumerate() {
            if last_file != Some(m.file.as_path()) {
                if last_file.is_some() {
                    current_file += 1;
                }
                last_file = Some(m.file.as_path());
                match_within = 0;
            }
            if current_file == file_idx && match_within == match_idx {
                return Some(i);
            }
            match_within += 1;
        }
        None
    }

    /// Set search panel focus state. Both backends call this when
    /// activating/deactivating the search panel.
    pub fn search_set_focus(&mut self, focused: bool) {
        self.search_has_focus = focused;
        if focused {
            if matches!(
                self.mode,
                Mode::Visual | Mode::VisualLine | Mode::VisualBlock
            ) {
                if let Some((text, _)) = self.get_visual_selection_text() {
                    let first_line = text.lines().next().unwrap_or("").to_string();
                    if !first_line.is_empty() {
                        self.project_search_query = first_line;
                        self.search_query_caret.set(self.project_search_query.len());
                    }
                }
                self.mode = Mode::Normal;
                self.visual_anchor = None;
            }
            self.search_panel_form_focus
                .replace(Some("search:query".to_string()));
            self.search_sidebar_system
                .borrow_mut()
                .set_active_section(Some(0));
        }
    }

    /// Switch the search panel from form input to results navigation.
    /// Called when search results arrive.
    pub fn search_switch_to_results(&mut self) {
        if self.search_has_focus && !self.project_search_results.is_empty() {
            self.search_panel_form_focus.replace(None);
            self.search_sidebar_system
                .borrow_mut()
                .set_active_section(Some(1));
        }
    }

    /// Open the file at the given result index and jump to the match line.
    pub(crate) fn open_search_result(&mut self, result_idx: usize) {
        if let Some(m) = self.project_search_results.get(result_idx) {
            let path = m.file.clone();
            let line = m.line;
            self.open_file_in_tab(&path);
            let wid = self.active_window_id();
            self.set_cursor_for_window(wid, line, 0);
            self.ensure_cursor_visible();
        }
    }

    // =======================================================================
    // SidebarSystem unified dispatch
    // =======================================================================

    pub fn dispatch_search_sidebar_key_unified(
        &mut self,
        key: &str,
        ctrl: bool,
        alt: bool,
        unicode: Option<char>,
    ) -> SearchKeyResult {
        // Alt shortcuts work in both form and results mode.
        if alt {
            match key {
                "c" => {
                    self.toggle_project_search_case();
                    return SearchKeyResult::Consumed;
                }
                "w" => {
                    self.toggle_project_search_whole_word();
                    return SearchKeyResult::Consumed;
                }
                "r" => {
                    self.toggle_project_search_regex();
                    return SearchKeyResult::Consumed;
                }
                "h" => {
                    let root = self.cwd.clone();
                    self.start_project_replace(root);
                    return SearchKeyResult::Consumed;
                }
                _ => {}
            }
        }

        if ctrl && key == "b" {
            self.search_has_focus = false;
            return SearchKeyResult::Unfocused;
        }

        let active = self.search_sidebar_system.borrow().active_section();
        let form_active = active == Some(0);

        if form_active {
            let action = self.handle_search_input_key(key, unicode);
            if let SearchInputAction::Unfocused = action {
                self.search_panel_form_focus.replace(None);
                self.search_sidebar_system
                    .borrow_mut()
                    .set_active_section(Some(1));
            }
            return SearchKeyResult::Consumed;
        }

        // Results section active — navigation + domain keys.
        use quadraui::{Key, NamedKey};
        let nav_key = match key {
            "j" | "Down" => Some(Key::Named(NamedKey::Down)),
            "k" | "Up" => Some(Key::Named(NamedKey::Up)),
            "Home" => Some(Key::Named(NamedKey::Home)),
            "End" => Some(Key::Named(NamedKey::End)),
            "Page_Up" => Some(Key::Named(NamedKey::PageUp)),
            "Page_Down" => Some(Key::Named(NamedKey::PageDown)),
            _ => None,
        };
        if let Some(k) = nav_key {
            self.search_sidebar_navigate(k);
            return SearchKeyResult::Consumed;
        }

        match key {
            "Return" | "Enter" => {
                if let Some(idx) = self.search_selected_result_idx() {
                    self.open_search_result(idx);
                    self.search_has_focus = false;
                    return SearchKeyResult::Unfocused;
                }
                SearchKeyResult::Consumed
            }
            "Tab" => {
                self.search_panel_form_focus
                    .replace(Some("search:query".to_string()));
                self.search_sidebar_system
                    .borrow_mut()
                    .set_active_section(Some(0));
                SearchKeyResult::Consumed
            }
            "Escape" | "q" => {
                self.search_has_focus = false;
                SearchKeyResult::Unfocused
            }
            "h" | "Left" => {
                self.search_has_focus = false;
                SearchKeyResult::Unfocused
            }
            _ => {
                if let Some(ch) = unicode {
                    if !ctrl {
                        self.search_panel_form_focus
                            .replace(Some("search:query".to_string()));
                        self.search_input_insert_char(false, ch);
                        self.search_sidebar_system
                            .borrow_mut()
                            .set_active_section(Some(0));
                        return SearchKeyResult::Consumed;
                    }
                }
                SearchKeyResult::Consumed
            }
        }
    }

    fn search_sidebar_navigate(&mut self, key: quadraui::Key) {
        let rect = self.search_sidebar_body_rect.get();
        let ev = quadraui::UiEvent::KeyPressed {
            key,
            modifiers: quadraui::Modifiers::default(),
            repeat: false,
        };
        let sidebar_event = self
            .search_sidebar_system
            .borrow_mut()
            .handle_cached(&ev, rect);
        self.dispatch_search_sidebar_event(sidebar_event);
    }

    pub fn dispatch_search_sidebar_event(&mut self, event: quadraui::SidebarEvent) -> bool {
        match event {
            quadraui::SidebarEvent::RowActivated { .. } => {
                if let Some(idx) = self.search_selected_result_idx() {
                    self.open_search_result(idx);
                    self.search_has_focus = false;
                }
                true
            }
            quadraui::SidebarEvent::FormEvent { event, .. } => {
                use quadraui::FormEvent;
                match event {
                    FormEvent::ButtonClicked { id } => {
                        self.handle_search_form_hit(id.as_str());
                    }
                    FormEvent::FocusChanged { id } => {
                        let id_str = id.as_str();
                        if id_str == "search:query" || id_str == "search:replace" {
                            self.search_panel_form_focus
                                .replace(Some(id_str.to_string()));
                            self.search_sidebar_system
                                .borrow_mut()
                                .set_active_section(Some(0));
                        }
                    }
                    FormEvent::ToggleChanged { id, .. } => {
                        self.handle_search_form_hit(id.as_str());
                    }
                    _ => {}
                }
                true
            }
            quadraui::SidebarEvent::RowSelected { section, path } => {
                self.search_panel_form_focus.replace(None);
                if section == 1 && path.len() == 1 {
                    let fi = path[0] as usize;
                    let mut collapsed = self.search_collapsed_files.borrow_mut();
                    if collapsed.contains(&fi) {
                        collapsed.remove(&fi);
                    } else {
                        collapsed.insert(fi);
                    }
                }
                true
            }
            quadraui::SidebarEvent::Ignored => false,
            _ => true,
        }
    }

    pub fn handle_search_sidebar_ui_event(&mut self, event: quadraui::UiEvent) -> bool {
        let rect = self.search_sidebar_body_rect.get();
        let sidebar_event = self
            .search_sidebar_system
            .borrow_mut()
            .handle_cached(&event, rect);
        self.dispatch_search_sidebar_event(sidebar_event)
    }

    pub fn search_selected_result_idx(&self) -> Option<usize> {
        let sidebar = self.search_sidebar_system.borrow();
        let path = sidebar.selected_path(1)?;
        if path.len() >= 2 {
            self.tree_path_to_result_idx(path[0] as usize, path[1] as usize)
        } else {
            None
        }
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
        self.notify(
            NotificationKind::ProjectReplace,
            &format!("Replacing \"{query}\" → \"{replacement}\"…"),
        );
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
        self.notify_done_by_kind(&NotificationKind::ProjectReplace, Some("Replace complete"));
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
        self.search_sidebar_system
            .borrow_mut()
            .set_selected_path(1, None);
    }
}

// ─── Additional methods (extracted from mod.rs) ─────────────────────────

impl Engine {
    // =======================================================================
    // Search word under cursor (* / #)
    // =======================================================================

    /// Extract the word under the cursor. Returns None if cursor is not on a word char.
    pub(crate) fn word_under_cursor(&self) -> Option<String> {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_content: String = self.buffer().content.line(line).chars().collect();
        let chars: Vec<char> = line_content.chars().collect();

        if col >= chars.len() {
            return None;
        }
        if !Self::is_word_char(chars[col]) {
            return None;
        }

        // Find start of word
        let start = (0..=col)
            .rev()
            .take_while(|&i| Self::is_word_char(chars[i]))
            .last()
            .unwrap_or(col);
        // Find end of word (exclusive)
        let end = (col..chars.len())
            .take_while(|&i| Self::is_word_char(chars[i]))
            .last()
            .map(|i| i + 1)
            .unwrap_or(col + 1);

        Some(chars[start..end].iter().collect())
    }

    /// Search forward (*) or backward (#) for the word under cursor with word boundaries.
    pub(crate) fn search_word_under_cursor(&mut self, forward: bool) {
        let word = match self.word_under_cursor() {
            Some(w) => w,
            None => {
                self.message = "No word under cursor".to_string();
                return;
            }
        };

        self.search_query = word.clone();
        self.search_direction = if forward {
            SearchDirection::Forward
        } else {
            SearchDirection::Backward
        };
        self.search_word_bounded = true;

        // Build word-boundary matches manually
        self.build_word_bounded_matches();

        if self.search_matches.is_empty() {
            self.message = format!("Pattern not found: {}", word);
            return;
        }

        // Jump to first match in the appropriate direction
        if forward {
            self.search_next();
        } else {
            self.search_prev();
        }
    }

    /// Like run_search but only keeps matches that are whole words.
    pub(crate) fn build_word_bounded_matches(&mut self) {
        self.search_matches.clear();
        self.search_index = None;

        if self.search_query.is_empty() {
            return;
        }

        let text = self.buffer().to_string();
        let query = self.search_query.clone();
        let mut byte_pos = 0;

        while let Some(found) = text[byte_pos..].find(&query) {
            let start_byte = byte_pos + found;
            let end_byte = start_byte + query.len();

            // Check word boundaries
            let before_ok = start_byte == 0 || {
                let c = text[..start_byte].chars().last().unwrap_or(' ');
                !Self::is_word_char(c)
            };
            let after_ok = end_byte >= text.len() || {
                let c = text[end_byte..].chars().next().unwrap_or(' ');
                !Self::is_word_char(c)
            };

            if before_ok && after_ok {
                let start_char = self.buffer().content.byte_to_char(start_byte);
                let end_char = self.buffer().content.byte_to_char(end_byte);
                self.search_matches.push((start_char, end_char));
            }

            byte_pos = start_byte + 1;
        }
    }

    // ===================================================================
    // Find/Replace overlay (Ctrl+F)
    // ===================================================================

    /// Open the find/replace overlay.
    pub fn open_find_replace(&mut self) {
        let had_selection = self.visual_anchor.is_some()
            && matches!(
                self.mode,
                Mode::Visual | Mode::VisualLine | Mode::VisualBlock
            );

        // Capture selection BEFORE clearing visual mode
        let sel_text = if had_selection {
            self.get_visual_selection_text().map(|(t, _)| t)
        } else {
            None
        };
        let sel_range = if had_selection {
            self.get_visual_selection_range().map(|(start, end)| {
                let start_char = self.buffer().line_to_char(start.line) + start.col;
                let end_line_start = self.buffer().line_to_char(end.line);
                let end_char = end_line_start + end.col + 1;
                (start.line, end.line, start_char, end_char)
            })
        } else {
            None
        };

        // Keep visual selection visible while find/replace is open (like `:` in visual mode).
        // Don't clear visual_anchor — build_selection() will render the highlight.
        // Freeze the cursor position so the selection doesn't change as search moves the cursor.
        if had_selection {
            self.find_replace_visual_end = Some(*self.cursor());
            self.command_from_visual = Some(self.mode);
            self.mode = Mode::Normal;
        }

        // If already open and no new selection, just refocus
        if self.find_replace_open && !had_selection {
            self.find_replace_focus = 0;
            self.find_replace_cursor = self.find_replace_query.chars().count();
            return;
        }

        self.find_replace_open = true;
        self.find_replace_focus = 0;

        if let Some((start_line, end_line, start_char, end_char)) = sel_range {
            // Save selection range for "find in selection"
            self.find_replace_selection_range = Some((start_char, end_char));

            if start_line == end_line {
                // Single-line: populate the find box with the selected text
                if let Some(text) = &sel_text {
                    let trimmed = text.trim_end_matches('\n').to_string();
                    if !trimmed.is_empty() {
                        self.find_replace_query = trimmed;
                    }
                }
            } else {
                // Multi-line: auto-enable "find in selection", don't populate query
                self.find_replace_options.in_selection = true;
            }
        } else if !self.find_replace_open || self.find_replace_query.is_empty() {
            // No selection — pre-fill from last search query (only on first open)
            if !self.search_query.is_empty() {
                self.find_replace_query = self.search_query.clone();
            }
            self.find_replace_selection_range = None;
            self.find_replace_options.in_selection = false;
        }

        self.find_replace_cursor = self.find_replace_query.chars().count();
        self.find_replace_sel_anchor = None;
        self.run_find_replace_search();
    }

    /// Close the find/replace overlay, preserving search state for n/N.
    pub fn close_find_replace(&mut self) {
        self.find_replace_open = false;
        self.find_replace_options.in_selection = false;
        self.find_replace_selection_range = None;
        // Clear visual selection that was preserved while overlay was open
        if self.command_from_visual.is_some() {
            self.visual_anchor = None;
            self.command_from_visual = None;
            self.find_replace_visual_end = None;
        }
    }

    /// Run a search using the find/replace overlay's query and options.
    /// This populates `search_query`, `search_matches`, and `search_index`
    /// so that existing highlighting and n/N navigation work.
    pub fn run_find_replace_search(&mut self) {
        self.search_query = self.find_replace_query.clone();
        self.search_direction = SearchDirection::Forward;
        self.search_word_bounded = false;
        self.search_matches.clear();
        self.search_index = None;

        if self.search_query.is_empty() {
            return;
        }

        let text = self.buffer().to_string();
        let query = self.search_query.clone();
        let opts = &self.find_replace_options;

        if opts.use_regex {
            // Use regex crate for regex search (multiline so ^ and $ match line boundaries)
            let mut flags = String::from("(?m");
            if !opts.case_sensitive {
                flags.push('i');
            }
            flags.push(')');
            let pattern = format!("{}{}", flags, query);
            if let Ok(re) = regex::Regex::new(&pattern) {
                for m in re.find_iter(&text) {
                    let start_char = self.buffer().content.byte_to_char(m.start());
                    let end_char = self.buffer().content.byte_to_char(m.end());
                    self.search_matches.push((start_char, end_char));
                }
            }
        } else {
            // Plain text search
            let case_insensitive = !opts.case_sensitive;
            let (search_text, search_query) = if case_insensitive {
                (text.to_lowercase(), query.to_lowercase())
            } else {
                (text.clone(), query.clone())
            };

            let mut byte_pos = 0;
            while let Some(found) = search_text[byte_pos..].find(&search_query) {
                let start_byte = byte_pos + found;
                let end_byte = start_byte + search_query.len();

                // Whole word check
                if opts.whole_word {
                    let before_ok = start_byte == 0 || {
                        let c = text[..start_byte].chars().last().unwrap_or(' ');
                        !Self::is_word_char(c)
                    };
                    let after_ok = end_byte >= text.len() || {
                        let c = text[end_byte..].chars().next().unwrap_or(' ');
                        !Self::is_word_char(c)
                    };
                    if !before_ok || !after_ok {
                        byte_pos = start_byte + 1;
                        continue;
                    }
                }

                let start_char = self.buffer().content.byte_to_char(start_byte);
                let end_char = self.buffer().content.byte_to_char(end_byte);
                self.search_matches.push((start_char, end_char));
                byte_pos = start_byte + 1;
            }
        }

        // Filter to selection range if "find in selection" is active
        if self.find_replace_options.in_selection {
            if let Some((sel_start, sel_end)) = self.find_replace_selection_range {
                self.search_matches
                    .retain(|(s, e)| *s >= sel_start && *e <= sel_end);
            }
        }

        // Set search_index to nearest match from cursor
        if !self.search_matches.is_empty() {
            let cursor_char = {
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                self.buffer().line_to_char(line) + col
            };
            let idx = self
                .search_matches
                .iter()
                .position(|(start, _)| *start >= cursor_char)
                .unwrap_or(0);
            self.search_index = Some(idx);
        }
    }

    /// Navigate to the next match in the find/replace overlay.
    pub fn find_replace_next(&mut self) {
        self.search_next();
    }

    /// Navigate to the previous match in the find/replace overlay.
    pub fn find_replace_prev(&mut self) {
        self.search_prev();
    }

    /// Replace the current match and advance to the next.
    pub fn find_replace_replace_current(&mut self) {
        let idx = match self.search_index {
            Some(i) => i,
            None => return,
        };
        let (start_char, end_char) = match self.search_matches.get(idx) {
            Some(&pair) => pair,
            None => return,
        };

        let replacement = self.find_replace_replacement.clone();
        let repl_len = replacement.chars().count();
        self.start_undo_group();
        self.delete_with_undo(start_char, end_char);
        self.insert_with_undo(start_char, &replacement);
        self.finish_undo_group();

        // Move cursor past the replacement so next search finds the next match
        let new_pos = start_char + repl_len;
        let total_chars = self.buffer().len_chars();
        let clamped = new_pos.min(total_chars.saturating_sub(1));
        let line = self.buffer().content.char_to_line(clamped);
        let line_start = self.buffer().line_to_char(line);
        self.view_mut().cursor.line = line;
        self.view_mut().cursor.col = clamped - line_start;

        // Re-run search and advance to next match
        self.run_find_replace_search();
    }

    /// Replace all matches in the buffer (respects "find in selection" range).
    pub fn find_replace_replace_all(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }

        let replacement = self.find_replace_replacement.clone();
        let query = self.find_replace_query.clone();

        let flags = if self.find_replace_options.case_sensitive {
            "g"
        } else {
            "gi"
        };

        // Determine line range: selection range or entire buffer
        let (start_line, end_line) = if self.find_replace_options.in_selection {
            if let Some((sel_start, sel_end)) = self.find_replace_selection_range {
                let sl = self.buffer().content.char_to_line(sel_start);
                let el = self
                    .buffer()
                    .content
                    .char_to_line(sel_end.min(self.buffer().len_chars().saturating_sub(1)));
                (sl, el)
            } else {
                (0, self.buffer().len_lines().saturating_sub(1))
            }
        } else {
            (0, self.buffer().len_lines().saturating_sub(1))
        };

        match self.replace_in_range(Some((start_line, end_line)), &query, &replacement, flags) {
            Ok(count) => {
                self.message = format!("{} replacement(s) made", count);
            }
            Err(e) => {
                self.message = format!("Replace error: {}", e);
            }
        }

        // Re-run search to update highlights
        self.run_find_replace_search();
    }

    /// Toggle case-sensitive search and re-run.
    pub fn toggle_find_replace_case(&mut self) {
        self.find_replace_options.case_sensitive = !self.find_replace_options.case_sensitive;
        self.run_find_replace_search();
    }

    /// Toggle whole-word search and re-run.
    pub fn toggle_find_replace_whole_word(&mut self) {
        self.find_replace_options.whole_word = !self.find_replace_options.whole_word;
        self.run_find_replace_search();
    }

    /// Toggle regex search and re-run.
    pub fn toggle_find_replace_regex(&mut self) {
        self.find_replace_options.use_regex = !self.find_replace_options.use_regex;
        self.run_find_replace_search();
    }

    /// Toggle preserve-case replacement.
    pub fn toggle_find_replace_preserve_case(&mut self) {
        self.find_replace_options.preserve_case = !self.find_replace_options.preserve_case;
    }

    /// Toggle find-in-selection mode and re-run.
    pub fn toggle_find_replace_in_selection(&mut self) {
        self.find_replace_options.in_selection = !self.find_replace_options.in_selection;
        self.run_find_replace_search();
    }

    /// Dispatch a find/replace click target to the appropriate engine method.
    /// Backends resolve native coordinates → `FindReplaceClickTarget`, then call this.
    pub fn handle_find_replace_click(&mut self, target: FindReplaceClickTarget) {
        use FindReplaceClickTarget::*;
        match target {
            Chevron => {
                self.find_replace_show_replace = !self.find_replace_show_replace;
            }
            FindInput(pos) => {
                self.find_replace_focus = 0;
                self.find_replace_cursor = pos.min(self.find_replace_query.chars().count());
                self.find_replace_sel_anchor = Some(self.find_replace_cursor);
            }
            ReplaceInput(pos) => {
                self.find_replace_focus = 1;
                self.find_replace_cursor = pos.min(self.find_replace_replacement.chars().count());
                self.find_replace_sel_anchor = Some(self.find_replace_cursor);
            }
            ToggleCase => self.toggle_find_replace_case(),
            ToggleWholeWord => self.toggle_find_replace_whole_word(),
            ToggleRegex => self.toggle_find_replace_regex(),
            PrevMatch => self.find_replace_prev(),
            NextMatch => self.find_replace_next(),
            ToggleInSelection => self.toggle_find_replace_in_selection(),
            Close => self.close_find_replace(),
            TogglePreserveCase => self.toggle_find_replace_preserve_case(),
            ReplaceCurrent => self.find_replace_replace_current(),
            ReplaceAll => self.find_replace_replace_all(),
        }
    }

    /// Get the selected range in the focused input field, if any.
    /// Returns (start, end) as char offsets where start < end.
    fn fr_input_selection(&self) -> Option<(usize, usize)> {
        let anchor = self.find_replace_sel_anchor?;
        let cursor = self.find_replace_cursor;
        if anchor == cursor {
            return None;
        }
        Some((anchor.min(cursor), anchor.max(cursor)))
    }

    /// Delete the selected text in the focused input field.
    /// Returns true if something was deleted.
    fn fr_delete_selection(&mut self) -> bool {
        let (start, end) = match self.fr_input_selection() {
            Some(r) => r,
            None => return false,
        };
        let field = if self.find_replace_focus == 0 {
            &mut self.find_replace_query
        } else {
            &mut self.find_replace_replacement
        };
        let start_byte = field
            .char_indices()
            .nth(start)
            .map(|(i, _)| i)
            .unwrap_or(field.len());
        let end_byte = field
            .char_indices()
            .nth(end)
            .map(|(i, _)| i)
            .unwrap_or(field.len());
        field.replace_range(start_byte..end_byte, "");
        self.find_replace_cursor = start;
        self.find_replace_sel_anchor = None;
        true
    }

    /// Handle a key press in the find/replace overlay.
    pub(crate) fn handle_find_replace_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        shift: bool,
    ) {
        match key_name {
            "Escape" => {
                self.close_find_replace();
            }
            "Return" => {
                if ctrl && self.find_replace_focus == 1 {
                    // Ctrl+Enter in replace field → replace all
                    self.find_replace_replace_all();
                } else {
                    self.find_replace_next();
                }
            }
            "Up" => {
                self.find_replace_prev();
            }
            "Down" => {
                self.find_replace_next();
            }
            "Tab" | "ISO_Left_Tab" => {
                if !self.find_replace_show_replace {
                    self.find_replace_show_replace = true;
                }
                self.find_replace_focus = if self.find_replace_focus == 0 { 1 } else { 0 };
                // Update cursor to the focused field's length
                let len = if self.find_replace_focus == 0 {
                    self.find_replace_query.len()
                } else {
                    self.find_replace_replacement.len()
                };
                self.find_replace_cursor = len;
            }
            "BackSpace" => {
                let is_find = self.find_replace_focus == 0;
                if self.fr_delete_selection() {
                    // Deleted selected text
                } else if self.find_replace_cursor > 0 {
                    let field = if is_find {
                        &mut self.find_replace_query
                    } else {
                        &mut self.find_replace_replacement
                    };
                    let byte_idx = field
                        .char_indices()
                        .nth(self.find_replace_cursor - 1)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    let next_byte = field
                        .char_indices()
                        .nth(self.find_replace_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(field.len());
                    field.replace_range(byte_idx..next_byte, "");
                    self.find_replace_cursor -= 1;
                }
                if is_find {
                    self.run_find_replace_search();
                }
            }
            "Delete" => {
                let (field, is_find) = if self.find_replace_focus == 0 {
                    (&mut self.find_replace_query, true)
                } else {
                    (&mut self.find_replace_replacement, false)
                };
                let char_len = field.chars().count();
                if self.find_replace_cursor < char_len {
                    let byte_idx = field
                        .char_indices()
                        .nth(self.find_replace_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(field.len());
                    let next_byte = field
                        .char_indices()
                        .nth(self.find_replace_cursor + 1)
                        .map(|(i, _)| i)
                        .unwrap_or(field.len());
                    field.replace_range(byte_idx..next_byte, "");
                    if is_find {
                        self.run_find_replace_search();
                    }
                }
            }
            "Left" => {
                self.find_replace_sel_anchor = None;
                if self.find_replace_cursor > 0 {
                    self.find_replace_cursor -= 1;
                }
            }
            "Right" => {
                self.find_replace_sel_anchor = None;
                let len = if self.find_replace_focus == 0 {
                    self.find_replace_query.chars().count()
                } else {
                    self.find_replace_replacement.chars().count()
                };
                if self.find_replace_cursor < len {
                    self.find_replace_cursor += 1;
                }
            }
            "Home" => {
                self.find_replace_sel_anchor = None;
                self.find_replace_cursor = 0;
            }
            "End" => {
                self.find_replace_sel_anchor = None;
                self.find_replace_cursor = if self.find_replace_focus == 0 {
                    self.find_replace_query.chars().count()
                } else {
                    self.find_replace_replacement.chars().count()
                };
            }
            _ => {
                // Alt+key toggles
                if key_name == "c" && !ctrl && unicode == Some('c') {
                    // check for alt below
                }
                // Handle Alt+C/W/R toggles
                if !ctrl {
                    match key_name {
                        "c" if unicode.is_none() || shift => {
                            // Alt+C — toggle case
                            self.toggle_find_replace_case();
                            return;
                        }
                        "w" if unicode.is_none() || shift => {
                            self.toggle_find_replace_whole_word();
                            return;
                        }
                        "r" if unicode.is_none() || shift => {
                            self.toggle_find_replace_regex();
                            return;
                        }
                        _ => {}
                    }
                }

                // Ctrl+Z: pass through to engine undo
                if ctrl && key_name == "z" {
                    self.undo();
                    self.run_find_replace_search();
                    return;
                }

                // Ctrl+A: select all text in the focused field
                if ctrl && key_name == "a" {
                    let len = if self.find_replace_focus == 0 {
                        self.find_replace_query.chars().count()
                    } else {
                        self.find_replace_replacement.chars().count()
                    };
                    self.find_replace_sel_anchor = Some(0);
                    self.find_replace_cursor = len;
                    return;
                }

                // Ctrl+V paste (replaces selection if any)
                if ctrl && key_name == "v" {
                    if let Some(clip) = Self::clipboard_paste() {
                        let paste = clip.lines().next().unwrap_or("").to_string();
                        self.fr_delete_selection(); // remove selected text first
                        let (field, is_find) = if self.find_replace_focus == 0 {
                            (&mut self.find_replace_query, true)
                        } else {
                            (&mut self.find_replace_replacement, false)
                        };
                        let byte_idx = field
                            .char_indices()
                            .nth(self.find_replace_cursor)
                            .map(|(i, _)| i)
                            .unwrap_or(field.len());
                        field.insert_str(byte_idx, &paste);
                        self.find_replace_cursor += paste.chars().count();
                        if is_find {
                            self.run_find_replace_search();
                        }
                    }
                    return;
                }

                // Ctrl+Shift+H — replace current match
                if ctrl && shift && key_name == "h" {
                    self.find_replace_replace_current();
                    return;
                }

                // Ctrl+H — toggle replace row visibility
                if ctrl && !shift && key_name == "h" {
                    self.find_replace_show_replace = !self.find_replace_show_replace;
                    if self.find_replace_show_replace && self.find_replace_focus == 0 {
                        // Optionally switch focus to replace
                    }
                    return;
                }

                // Printable character insertion (replaces selection if any)
                if let Some(ch) = unicode {
                    if !ctrl && !ch.is_control() {
                        self.fr_delete_selection(); // remove selected text first
                        let (field, is_find) = if self.find_replace_focus == 0 {
                            (&mut self.find_replace_query, true)
                        } else {
                            (&mut self.find_replace_replacement, false)
                        };
                        let byte_idx = field
                            .char_indices()
                            .nth(self.find_replace_cursor)
                            .map(|(i, _)| i)
                            .unwrap_or(field.len());
                        field.insert(byte_idx, ch);
                        self.find_replace_cursor += 1;
                        self.find_replace_sel_anchor = None;
                        if is_find {
                            self.run_find_replace_search();
                        }
                    }
                }
            }
        }
    }
}
