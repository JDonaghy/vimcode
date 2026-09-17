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

    /// True if some window other than `except_win` still displays `buf_id`.
    ///
    /// A dirty buffer can be abandoned in one window/tab as long as another
    /// window still shows it — the buffer itself isn't going anywhere, so
    /// there's nothing to lose (`:h E37`). Only when `except_win` is the
    /// *last* window showing `buf_id` is closing it a real "discard my only
    /// copy" decision that deserves a confirmation prompt.
    ///
    /// This predicate used to be duplicated: the `:q` path
    /// (`execute.rs`) got it right, but the tab-bar close path
    /// (`handle_tab_bar_click`'s `CloseTab` arm) never applied it and
    /// prompted on every view of a dirty buffer, not just the last one
    /// (#1038). `:q` only ever closes a single window, so a single
    /// `except_win` is enough for it; see `buffer_has_views_outside` for
    /// the multi-window case (closing a whole tab).
    pub fn buffer_has_other_views(&self, buf_id: BufferId, except_win: WindowId) -> bool {
        self.buffer_has_views_outside(buf_id, std::slice::from_ref(&except_win))
    }

    /// True if some window outside `excluded` still displays `buf_id`.
    ///
    /// Closing an entire tab (`CloseTab`) can destroy more than one window
    /// at once: an ordinary in-tab split (`split_window`/
    /// `split_window_with_new_first`) puts a second `Window` on the *same*
    /// buffer inside the *same* tab, and `close_tab` removes every window
    /// the tab owns, not just the currently-focused one. Excluding only the
    /// active window (as `buffer_has_other_views` does) would find that
    /// sibling split and wrongly conclude another view survives, even
    /// though it's being destroyed in the very same operation — silently
    /// discarding the only copy of the dirty buffer (#1038). Callers that
    /// close more than one window at a time must exclude the *whole* set of
    /// windows about to disappear.
    pub fn buffer_has_views_outside(&self, buf_id: BufferId, excluded: &[WindowId]) -> bool {
        self.windows
            .values()
            .any(|w| w.buffer_id == buf_id && !excluded.contains(&w.id))
    }

    /// Compute explorer tree indicators: git status + deduplicated diagnostic counts.
    /// Returns (git_statuses, diag_counts) where:
    /// - git_statuses: canonical path → git status char (M, A, D, R, U)
    /// - diag_counts: canonical path → (error_lines, warning_lines) deduplicated by line number
    ///
    /// Result is cached in `explorer_indicators_cache`; call
    /// `invalidate_explorer_indicators()` after mutating
    /// `sc_file_statuses` or `lsp_diagnostics` so the next call
    /// recomputes. Caching matters because this function canonicalises
    /// every file-status path (a syscall each) and scans every
    /// diagnostic — on a workspace with thousands of untracked files
    /// the uncached version was costing seconds per explorer draw
    /// (see #153).
    pub fn explorer_indicators(
        &self,
    ) -> (HashMap<PathBuf, char>, HashMap<PathBuf, (usize, usize)>) {
        if let Some(ref cached) = *self.explorer_indicators_cache.borrow() {
            return cached.clone();
        }
        let result = self.compute_explorer_indicators();
        *self.explorer_indicators_cache.borrow_mut() = Some(result.clone());
        result
    }

    /// Invalidate the cached explorer indicators so the next call
    /// recomputes. Call this after any change to `sc_file_statuses` or
    /// `lsp_diagnostics`.
    pub fn invalidate_explorer_indicators(&self) {
        *self.explorer_indicators_cache.borrow_mut() = None;
    }

    fn compute_explorer_indicators(
        &self,
    ) -> (HashMap<PathBuf, char>, HashMap<PathBuf, (usize, usize)>) {
        use crate::core::lsp::DiagnosticSeverity;
        use std::collections::HashSet;

        // Build git status map
        let mut git_statuses: HashMap<PathBuf, char> = HashMap::new();
        let repo_root = git::find_repo_root(&self.cwd);
        if let Some(ref root) = repo_root {
            for fs in &self.sc_file_statuses {
                // #991: a conflicted path carries neither side, so without
                // the `unmerged` arm it got no explorer badge at all.
                let kind = fs
                    .unmerged
                    .map(|_| git::StatusKind::Unmerged)
                    .or(fs.unstaged)
                    .or(fs.staged);
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
        // Priority: M > D > R > A > U
        //
        // #1051: this reads `StatusKind::label()`'s own output (populated
        // into `git_statuses` above), so it must track that mapping — 'U'
        // is the display label for `StatusKind::Untracked`, not git's `?`
        // porcelain notation.
        fn git_priority(c: char) -> u8 {
            match c {
                'M' => 5,
                'D' => 4,
                'R' => 3,
                'A' => 2,
                'U' => 1,
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

    /// Scroll the active window's viewport and adjust the cursor to stay
    /// visible, respecting `scrolloff`. Positive `delta` = down.
    pub fn scroll_viewport_with_cursor(&mut self, delta: isize, count: usize) {
        let lines = self.buffer().len_lines().saturating_sub(1);
        if delta > 0 {
            self.scroll_down_visible(count);
        } else {
            self.scroll_up_visible(count);
        }
        let scrolloff = self.settings.scrolloff;
        let vp = self.effective_viewport_lines().max(1);
        let cur = self.view().cursor.line;
        let new_top = self.view().scroll_top;
        let forced_line = if cur < new_top + scrolloff {
            Some((new_top + scrolloff).min(lines))
        } else if cur >= new_top + vp.saturating_sub(scrolloff) {
            Some((new_top + vp.saturating_sub(scrolloff + 1)).min(lines))
        } else {
            None
        };
        if let Some(line) = forced_line {
            // #805 review: this is a *vertical* cursor move (scrolloff pushed
            // the cursor onto a different line), so it must land on the
            // remembered `curswant` column exactly like `j`/`k`/`<C-d>`/
            // `<C-f>` do — a bare `clamp_cursor_col()` here would drop a
            // `$`-set `CURSWANT_EOL` on the floor, so `$` followed by
            // `<C-e>`/`<C-y>` would stop sticking to end-of-line.
            let want = self.curswant();
            self.view_mut().cursor.line = line;
            self.apply_curswant(want);
        }
    }

    /// Scroll a specific window's viewport and adjust its cursor to stay
    /// visible, respecting `scrolloff`. Positive `delta` = down.
    pub fn scroll_viewport_with_cursor_for_window(
        &mut self,
        window_id: WindowId,
        delta: isize,
        count: usize,
    ) {
        if delta > 0 {
            self.scroll_down_visible_for_window(window_id, count);
        } else {
            self.scroll_up_visible_for_window(window_id, count);
        }
        let scrolloff = self.settings.scrolloff;
        let Some(window) = self.windows.get(&window_id) else {
            return;
        };
        let buf_id = window.buffer_id;
        let Some(bs) = self.buffer_manager.get(buf_id) else {
            return;
        };
        let lines = bs.buffer.len_lines().saturating_sub(1);
        let vp = window.view.viewport_lines.max(1);
        let cur = window.view.cursor.line;
        let top = window.view.scroll_top;
        let new_line = if cur < top + scrolloff {
            Some((top + scrolloff).min(lines))
        } else if cur >= top + vp.saturating_sub(scrolloff) {
            Some((top + vp.saturating_sub(scrolloff + 1)).min(lines))
        } else {
            None
        };
        if let Some(line) = new_line {
            let max_col = bs.buffer.line_len_chars(line).saturating_sub(1);
            let window = self.windows.get_mut(&window_id).unwrap();
            window.view.cursor.line = line;
            if window.view.cursor.col > max_col {
                window.view.cursor.col = max_col;
            }
        }
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
        // Nested closed folds can both contain `line` (#1006) — snap to the
        // outermost (smallest `start`), the one actually visible at the top
        // of the viewport, not just whichever entry comes first.
        folds
            .iter()
            .filter(|f| line > f.start && line <= f.end)
            .map(|f| f.start)
            .min()
            .unwrap_or(line)
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
    pub fn sidebar_has_focus(&self) -> bool {
        self.explorer_has_focus
            || self.search_has_focus
            || self.sc_has_focus
            || self.dap_sidebar_has_focus
            || self.ext_sidebar_has_focus
            || self.ai_has_focus
            || self.settings_has_focus
            || self.ext_panel_has_focus
            || self.activity_bar_focused
    }

    /// Clear all sidebar panel focus flags at once, including the activity bar.
    pub fn clear_sidebar_focus(&mut self) {
        self.explorer_has_focus = false;
        self.search_has_focus = false;
        // #823 item 8: was a direct `self.sc_has_focus = false`, which
        // skipped `sc_set_focus`'s other two effects — clearing
        // `sc_button_focused` and syncing `sc_sidebar_system`'s own
        // focus flag. TUI's mouse.rs open-coded this same 9-flag clear
        // and, unlike this method, already called `sc_set_focus(false)`
        // here — so GTK's two call sites (both "clicking the editor clears
        // every sidebar's focus") were the ones carrying the bug: an SC
        // action button could stay visually focused after focus moved to
        // the editor.
        self.sc_set_focus(false);
        self.dap_sidebar_has_focus = false;
        self.ext_sidebar_has_focus = false;
        self.ai_has_focus = false;
        self.settings_has_focus = false;
        self.ext_panel_has_focus = false;
        self.activity_bar_focused = false;
    }

    /// Collapse (hide) the sidebar: hide it in `app_shell`, clear every
    /// sidebar panel's keyboard focus, mark the explorer no longer visible
    /// in session state, and persist the session.
    ///
    /// Shared by both backends (#823 item 8) — this exact 4-statement
    /// sequence (`app_shell.hide_sidebar()`, `clear_sidebar_focus()`,
    /// `session.explorer_visible = false`, `session.save()`) was pasted at
    /// five call sites in `tui_main/shell_app.rs` and once in `app.rs`.
    /// TUI additionally resets its own `TuiSidebar::has_focus` around each
    /// call — that's TUI-local UI state, not `Engine`'s, so it stays at the
    /// call site rather than becoming a parameter here.
    pub fn collapse_sidebar(&mut self) {
        self.app_shell.hide_sidebar();
        self.clear_sidebar_focus();
        self.session.explorer_visible = false;
        let _ = self.session.save();
    }

    /// Returns true if any user-focused modal popup is currently open
    /// (palette, tab switcher, context menu, dialog, find/replace,
    /// completion). The passive LSP hover popup is NOT counted —
    /// it's an informational overlay, not a modal the user is
    /// interacting with.
    ///
    /// Single source of truth for backends that need to gate
    /// behaviour on modal state — e.g. GTK suppresses the LSP hover
    /// trigger and hides native scrollbar widgets when this returns
    /// true.
    ///
    /// #731 (vimcode): both GTK call sites were inside dead code deleted
    /// by that issue (`App::tick`'s hover-polling block and the
    /// `sync_scrollbar`/`sync_scrollbar_positions` native-widget path) —
    /// both were already gated on Relm4-era widget handles permanently
    /// `None` under the ShellApp runner, so this had no live caller before
    /// the deletion either. Kept `pub` (not deleted) because it is
    /// documented, generically useful core API that the hover-polling
    /// restoration work #731 flags as follow-up will need again.
    #[allow(dead_code)]
    pub fn is_blocking_modal_open(&self) -> bool {
        self.picker_open
            || self.tab_switcher_open
            || self.context_menu.is_some()
            || self.dialog.is_some()
            || self.find_replace_open
            || self.completion_idx.is_some()
    }
}
