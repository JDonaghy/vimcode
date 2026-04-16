use super::*;

impl Engine {
    // =======================================================================
    // Buffer operations
    // =======================================================================

    pub fn update_syntax(&mut self) {
        self.active_buffer_state_mut().update_syntax();
    }

    /// Mark syntax as stale without doing any parsing work. Call on every
    /// keystroke in insert mode — the idle handler debounces the actual re-parse.
    #[allow(dead_code)]
    pub fn mark_syntax_stale(&mut self) {
        self.active_buffer_state_mut().mark_syntax_stale();
    }

    /// Full re-parse + highlight extraction if syntax is stale.
    /// Called on insert mode exit (Escape) where we need full highlights.
    pub fn refresh_syntax_if_stale(&mut self) {
        self.active_buffer_state_mut().refresh_syntax_if_stale();
    }

    /// Re-parse + extract highlights only for the visible viewport.
    /// Available for future use (e.g. background-thread refresh).
    #[allow(dead_code)]
    pub fn refresh_syntax_visible(&mut self) {
        let scroll_top = self.view().scroll_top;
        let visible = self.view().viewport_lines;
        self.active_buffer_state_mut()
            .refresh_syntax_visible(scroll_top, visible);
    }

    /// Debounced syntax refresh for insert mode. Returns true if a refresh
    /// was performed (caller should redraw). Call from the event loop idle path.
    /// Refreshes highlights after 150ms of no keystrokes, preventing stale
    /// byte offsets from causing wrong colors during typing.
    pub fn tick_syntax_debounce(&mut self) -> bool {
        let bs = self.active_buffer_state();
        if !bs.syntax_stale {
            return false;
        }
        if let Some(since) = bs.syntax_stale_since {
            if since.elapsed() >= std::time::Duration::from_millis(150) {
                self.active_buffer_state_mut().refresh_syntax_if_stale();
                return true;
            }
        }
        false
    }

    // =======================================================================
    // Undo/Redo operations
    // =======================================================================

    /// Start a new undo group for the active buffer.
    pub fn start_undo_group(&mut self) {
        let cursor = *self.cursor();
        // Save line state before modification (for U command)
        self.save_line_for_undo();
        // Record the "before" state in the timeline on first edit
        if self.active_buffer_state().undo_timeline.is_empty() {
            self.active_buffer_state_mut()
                .record_timeline_snapshot(cursor);
        }
        self.active_buffer_state_mut().start_undo_group(cursor);
    }

    /// Finish the current undo group for the active buffer.
    pub fn finish_undo_group(&mut self) {
        self.active_buffer_state_mut().finish_undo_group();
        // Record timeline snapshot for g-/g+ after each completed edit
        let cursor = self.view().cursor;
        self.active_buffer_state_mut()
            .record_timeline_snapshot(cursor);
    }

    /// Return to Normal mode from any mode, performing any necessary cleanup
    /// (finish undo group if in Insert, clear pending state, etc.).
    /// Call this when a UI action (dialog, overlay) takes control outside the
    /// normal keypress flow so the editor doesn't unexpectedly stay in Insert mode.
    pub fn escape_to_normal(&mut self) {
        match self.mode {
            Mode::Insert => {
                self.finish_undo_group();
                self.mode = Mode::Normal;
                self.clamp_cursor_col();
                self.lsp_signature_help = None;
                // Refresh stale syntax highlights deferred from insert mode.
                self.refresh_syntax_if_stale();
            }
            Mode::Visual | Mode::VisualLine | Mode::VisualBlock => {
                self.mode = Mode::Normal;
            }
            _ => {}
        }
        self.pending_key = None;
        self.pending_find_operator = None;
        self.count = None;
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
            let at_saved = self.active_buffer_state().is_at_saved_state();
            self.set_dirty(!at_saved);
            let active_id = self.active_buffer_id();
            self.lsp_dirty_buffers.insert(active_id, true);
            self.swap_mark_dirty();
            // Record state in timeline for g-/g+
            let cur = self.view().cursor;
            self.active_buffer_state_mut().record_timeline_snapshot(cur);
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
            let at_saved = self.active_buffer_state().is_at_saved_state();
            self.set_dirty(!at_saved);
            let active_id = self.active_buffer_id();
            self.lsp_dirty_buffers.insert(active_id, true);
            self.swap_mark_dirty();
            // Record state in timeline for g-/g+
            let cur = self.view().cursor;
            self.active_buffer_state_mut().record_timeline_snapshot(cur);
            true
        } else {
            self.message = "Already at newest change".to_string();
            false
        }
    }

    /// Navigate to an earlier buffer state chronologically (`g-`).
    pub fn g_earlier(&mut self) -> bool {
        let bs = self.active_buffer_state_mut();
        if bs.undo_timeline.is_empty() {
            return false;
        }
        // current_pos points to the timeline entry matching current buffer state.
        // None means "at latest" = last index.
        let current_pos = bs
            .undo_timeline_pos
            .unwrap_or(bs.undo_timeline.len().saturating_sub(1));
        if current_pos == 0 {
            return false; // already at earliest
        }
        let target = current_pos - 1;
        let (ref text, cursor) = bs.undo_timeline[target];
        let text_clone = text.clone();
        let char_len = bs.buffer.len_chars();
        bs.buffer.delete_range(0, char_len);
        if !text_clone.is_empty() {
            bs.buffer.insert(0, &text_clone);
        }
        bs.undo_timeline_pos = Some(target);
        bs.update_syntax();
        self.view_mut().cursor = cursor;
        self.clamp_cursor_col();
        self.set_dirty(true);
        let active_id = self.active_buffer_id();
        self.lsp_dirty_buffers.insert(active_id, true);
        self.swap_mark_dirty();
        let total = self.active_buffer_state().undo_timeline.len();
        self.message = format!("{} change(s); g- #{}/{}", total, target + 1, total);
        true
    }

    /// Navigate to a later buffer state chronologically (`g+`).
    pub fn g_later(&mut self) -> bool {
        let bs = self.active_buffer_state_mut();
        if bs.undo_timeline.is_empty() {
            return false;
        }
        let last = bs.undo_timeline.len() - 1;
        let current_pos = bs.undo_timeline_pos.unwrap_or(last);
        if current_pos >= last {
            return false; // already at latest
        }
        let target = current_pos + 1;
        let (ref text, cursor) = bs.undo_timeline[target];
        let text_clone = text.clone();
        let char_len = bs.buffer.len_chars();
        bs.buffer.delete_range(0, char_len);
        if !text_clone.is_empty() {
            bs.buffer.insert(0, &text_clone);
        }
        if target == last {
            bs.undo_timeline_pos = None; // back at latest
        } else {
            bs.undo_timeline_pos = Some(target);
        }
        bs.update_syntax();
        self.view_mut().cursor = cursor;
        self.clamp_cursor_col();
        self.set_dirty(true);
        let active_id = self.active_buffer_id();
        self.lsp_dirty_buffers.insert(active_id, true);
        self.swap_mark_dirty();
        let total = self.active_buffer_state().undo_timeline.len();
        self.message = format!("{} change(s); g+ #{}/{}", total, target + 1, total);
        true
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
    #[allow(dead_code)]
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
            // Silently do nothing — no "no changes" message, matching the behaviour of u
            // when there's nothing to undo (which shows "Already at oldest change" only once).
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
        // Keymaps scratch buffer: save content back to settings instead of disk
        if self.active_buffer_state().is_keymaps_buf {
            return self.save_keymaps_buffer();
        }
        // Registries scratch buffer: save content back to settings
        if self.active_buffer_state().is_registries_buf {
            return self.save_registries_buffer();
        }

        // Promote preview on save
        let active_id = self.active_buffer_id();
        if self.preview_buffer_id == Some(active_id) {
            self.promote_preview(active_id);
        }
        let state = self.active_buffer_state_mut();
        if let Some(ref path) = state.file_path.clone() {
            match state.save() {
                Ok(line_count) => {
                    let rel = self.copy_relative_path(path);
                    self.message = format!("\"{}\" {}L written", rel, line_count);
                    // Refresh git diff after save
                    let id = self.active_buffer_id();
                    self.refresh_git_diff(id);
                    self.compute_diff();
                    self.lsp_did_save(id);
                    // Delete swap file — content is safely on disk now.
                    self.swap_delete_for_buffer(id);
                    self.swap_write_needed.remove(&id);
                    let path_str = path.to_string_lossy().into_owned();
                    self.plugin_event("save", &path_str);
                    self.plugin_event("BufWrite", &path_str);
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

    /// Check all open buffers for external file modifications.
    ///
    /// For each buffer with a file path, compare the on-disk mtime against the
    /// stored mtime.  If the file has changed:
    /// - Clean buffers with `autoread` enabled: silently reload.
    /// - Dirty buffers (or `autoread` disabled): show a one-time warning.
    ///
    /// Call this from the UI backend on window focus gain or periodic tick.
    /// Returns `true` if any buffer was reloaded or a warning was shown.
    pub fn check_file_changes(&mut self) -> bool {
        if !self.settings.autoread {
            return false;
        }
        let mut any_changed = false;

        // Collect buffer IDs and paths first to avoid borrow conflicts.
        let to_check: Vec<(BufferId, PathBuf, bool, bool, Option<std::time::SystemTime>)> = self
            .buffer_manager
            .iter()
            .filter_map(|(id, state)| {
                let path = state.file_path.as_ref()?.clone();
                Some((
                    *id,
                    path,
                    state.dirty,
                    state.file_change_warned,
                    state.file_mtime,
                ))
            })
            .collect();

        for (buf_id, path, is_dirty, already_warned, stored_mtime) in to_check {
            let disk_mtime = match std::fs::metadata(&path).and_then(|m| m.modified()) {
                Ok(t) => t,
                Err(_) => continue, // file deleted or inaccessible — ignore
            };

            let changed = match stored_mtime {
                Some(prev) => disk_mtime != prev,
                None => false, // no stored mtime (e.g. new buffer) — skip
            };

            if !changed {
                continue;
            }

            if is_dirty {
                // Buffer has unsaved changes — warn (once per external modification).
                if !already_warned {
                    if let Some(state) = self.buffer_manager.get_mut(buf_id) {
                        state.file_change_warned = true;
                    }
                    let name = path.display();
                    self.message = format!(
                        "W12: Warning: File \"{}\" has changed since editing started. Use :e! to reload.",
                        name
                    );
                    any_changed = true;
                }
            } else {
                // Buffer is clean — silently reload.
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if let Some(state) = self.buffer_manager.get_mut(buf_id) {
                    if state.reload_from_disk().is_ok() {
                        self.message = format!("\"{}\" reloaded", name);
                        any_changed = true;
                    }
                }
            }
        }
        any_changed
    }

    /// Save the current buffer, optionally requesting LSP formatting first.
    ///
    /// When `format_on_save` is enabled and an LSP server supports formatting,
    /// this sends a formatting request and defers the actual disk save until the
    /// formatting response arrives (handled in `poll_lsp`).
    pub fn save_with_format(&mut self, quit_after: bool) -> Result<(), String> {
        // Check if we should format first.
        if self.settings.format_on_save && self.settings.lsp_enabled {
            self.ensure_lsp_manager();
            if let Some((path, _, _)) = self.lsp_cursor_position() {
                let tab_size = self.settings.tabstop as u32;
                let insert_spaces = self.settings.expand_tab;
                if let Some(mgr) = &mut self.lsp_manager {
                    if let Some(id) = mgr.request_formatting(&path, tab_size, insert_spaces) {
                        self.lsp_pending_formatting = Some(id);
                        self.format_on_save_pending = Some(self.active_buffer_id());
                        self.quit_after_format_save = quit_after;
                        self.message = "Formatting...".to_string();
                        return Ok(());
                    }
                }
            }
        }
        // No format-on-save — save immediately.
        let result = self.save();
        if quit_after && result.is_ok() {
            // Caller checks EngineAction — handled at call site.
        }
        result
    }

    // =======================================================================
    // Git integration
    // =======================================================================

    /// Refresh git diff markers and structured hunks for the given buffer.
    pub(crate) fn refresh_git_diff(&mut self, buffer_id: BufferId) {
        if let Some(path) = self
            .buffer_manager
            .get(buffer_id)
            .and_then(|s| s.file_path.clone())
        {
            let diff = git::compute_file_diff(&path);
            let hunks = git::compute_file_diff_hunks(&path);
            if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
                state.git_diff = diff;
                state.diff_hunks = hunks;
            }
        }
    }

    // ─── Markdown Preview ─────────────────────────────────────────────────

    /// Open a read-only markdown preview buffer with the given content (vsplit).
    /// Returns the preview buffer ID.
    #[allow(dead_code)]
    pub fn open_markdown_preview(&mut self, content: &str, title: &str) -> BufferId {
        use crate::core::markdown::render_markdown;
        let rendered = render_markdown(content);
        let text = rendered.lines.join("\n");

        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&text);
            state.read_only = true;
            state.md_rendered = Some(rendered);
        }

        // Open in vsplit, then redirect new window to the preview buffer.
        self.split_window(crate::core::window::SplitDirection::Vertical, None);
        self.active_window_mut().buffer_id = buf_id;
        self.message = format!("[Preview] {title}");
        buf_id
    }

    /// Open a live-linked markdown preview of the active buffer (must be .md).
    pub fn open_markdown_preview_linked(&mut self) {
        use crate::core::markdown::render_markdown;
        let source_id = self.active_buffer_id();
        let source_win = self.active_window_id();
        let content = self.buffer().to_string();
        let title = self.active_buffer_state().display_name();

        let rendered = render_markdown(&content);
        let text = rendered.lines.join("\n");

        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&text);
            state.read_only = true;
            state.md_rendered = Some(rendered);
        }

        self.split_window(crate::core::window::SplitDirection::Vertical, None);
        let preview_win = self.active_window_id();
        self.active_window_mut().buffer_id = buf_id;
        self.md_preview_links.insert(buf_id, source_id);
        self.scroll_bind_pairs.push((source_win, preview_win));
        self.message = format!("[Preview] {title}");
    }

    /// Open a markdown preview in a new tab (not a vsplit). Used for extension
    /// READMEs and other standalone rendered markdown content.
    pub fn open_markdown_preview_in_tab(&mut self, content: &str, title: &str) -> BufferId {
        use crate::core::markdown::render_markdown;
        let rendered = render_markdown(content);
        let text = rendered.lines.join("\n");

        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&text);
            state.read_only = true;
            state.md_rendered = Some(rendered);
            state.scratch_name = Some(title.to_string());
        }

        // Open in a new tab (like open_file_in_tab).
        let window_id = self.new_window_id();
        let window = Window::new(window_id, buf_id);
        self.windows.insert(window_id, window);

        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.active_group_mut().tabs.push(tab);
        self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;

        self.message = format!("[README] {title}");
        buf_id
    }

    /// Refresh any live markdown preview linked to the active source buffer.
    pub fn refresh_md_previews(&mut self) {
        use crate::core::markdown::render_markdown;
        let source_id = self.active_buffer_id();
        let content = match self.buffer_manager.get(source_id) {
            Some(s) => s.buffer.to_string(),
            None => return,
        };

        // Collect preview buf IDs linked to this source.
        let preview_ids: Vec<BufferId> = self
            .md_preview_links
            .iter()
            .filter(|(_, &src)| src == source_id)
            .map(|(&prev, _)| prev)
            .collect();

        if preview_ids.is_empty() {
            return;
        }

        let rendered = render_markdown(&content);
        let text = rendered.lines.join("\n");

        for prev_id in preview_ids {
            if let Some(state) = self.buffer_manager.get_mut(prev_id) {
                state.buffer.content = ropey::Rope::from_str(&text);
                state.md_rendered = Some(rendered.clone());
            }
        }
    }

    // =========================================================================
    // Netrw — in-buffer directory browser
    // =========================================================================

    /// Build a directory listing string for netrw.
    pub(crate) fn netrw_build_listing(dir: &Path, show_hidden: bool) -> String {
        let mut lines = Vec::new();
        lines.push(format!("\" {}/", dir.display()));
        lines.push("../".to_string());

        let entries = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                lines.push(format!("Error reading directory: {}", e));
                return lines.join("\n") + "\n";
            }
        };

        let mut dirs = Vec::new();
        let mut files = Vec::new();

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                dirs.push(format!("{}/", name));
            } else {
                files.push(name);
            }
        }

        dirs.sort();
        files.sort();

        for d in dirs {
            lines.push(d);
        }
        for f in files {
            lines.push(f);
        }

        lines.join("\n") + "\n"
    }

    /// Open a netrw directory listing. Optionally split first.
    pub(crate) fn cmd_explore(
        &mut self,
        arg: Option<&str>,
        split: Option<SplitDirection>,
    ) -> EngineAction {
        // Resolve target directory
        let dir = if let Some(a) = arg {
            let p = Path::new(a);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                self.cwd.join(p)
            }
        } else if let Some(fp) = self.file_path().map(|p| p.to_path_buf()) {
            fp.parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| self.cwd.clone())
        } else {
            self.cwd.clone()
        };

        if !dir.is_dir() {
            self.message = format!("Not a directory: {}", dir.display());
            return EngineAction::Error;
        }

        // Split first if requested
        if let Some(direction) = split {
            self.split_window(direction, None);
        }

        // Create netrw buffer
        let listing = Self::netrw_build_listing(&dir, self.settings.show_hidden_files);
        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&listing);
            state.read_only = true;
            state.netrw_dir = Some(dir);
        }

        // Point active window at the netrw buffer
        self.active_window_mut().buffer_id = buf_id;
        self.view_mut().cursor.line = 1; // skip header, land on ../
        self.view_mut().cursor.col = 0;

        EngineAction::None
    }

    /// Get the filesystem path for the entry at the cursor in a netrw buffer.
    pub(crate) fn netrw_entry_at_cursor(&self) -> Option<PathBuf> {
        let netrw_dir = self.active_buffer_state().netrw_dir.as_ref()?;
        let line_idx = self.cursor().line;
        if line_idx == 0 {
            return None; // header line
        }
        let line_text: String = self.buffer().content.line(line_idx).chars().collect();
        let entry = line_text.trim_end_matches('\n').trim();
        if entry.is_empty() {
            return None;
        }
        if entry == "../" {
            netrw_dir.parent().map(|p| p.to_path_buf())
        } else {
            Some(netrw_dir.join(entry))
        }
    }

    /// Activate (open) the netrw entry at cursor.
    pub(crate) fn netrw_activate_entry(&mut self) -> EngineAction {
        let path = match self.netrw_entry_at_cursor() {
            Some(p) => p,
            None => return EngineAction::None, // header line — no-op
        };

        if path.is_dir() {
            // Navigate into directory — reuse current buffer
            let listing = Self::netrw_build_listing(&path, self.settings.show_hidden_files);
            let buf_id = self.active_buffer_id();
            if let Some(state) = self.buffer_manager.get_mut(buf_id) {
                state.read_only = false; // temporarily allow write
                state.buffer.content = ropey::Rope::from_str(&listing);
                state.read_only = true;
                state.netrw_dir = Some(path);
            }
            self.view_mut().cursor.line = 1;
            self.view_mut().cursor.col = 0;
            self.view_mut().scroll_top = 0;
            EngineAction::None
        } else {
            // File — open in current window (replacing netrw buffer)
            let netrw_buf_id = self.active_buffer_id();
            let buf_id = match self.buffer_manager.open_file(&path) {
                Ok(id) => id,
                Err(e) => {
                    self.message = format!("Error: {}", e);
                    return EngineAction::Error;
                }
            };
            self.buffer_manager
                .apply_language_map(buf_id, &self.settings.language_map);
            self.buffer_manager.alternate_buffer = Some(netrw_buf_id);
            self.switch_window_buffer(buf_id);
            // Remove the netrw buffer if it's no longer shown in any window
            let still_used = self.windows.values().any(|w| w.buffer_id == netrw_buf_id);
            if !still_used {
                self.buffer_manager.remove(netrw_buf_id);
            }
            self.message = format!("\"{}\"", path.display());
            self.lsp_did_open(buf_id);
            EngineAction::None
        }
    }

    /// Navigate to parent directory in netrw.
    pub(crate) fn netrw_go_parent(&mut self) -> EngineAction {
        let netrw_dir = match self.active_buffer_state().netrw_dir.clone() {
            Some(d) => d,
            None => return EngineAction::None,
        };
        let parent = match netrw_dir.parent() {
            Some(p) => p.to_path_buf(),
            None => return EngineAction::None, // already at root
        };
        let listing = Self::netrw_build_listing(&parent, self.settings.show_hidden_files);
        let buf_id = self.active_buffer_id();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.read_only = false;
            state.buffer.content = ropey::Rope::from_str(&listing);
            state.read_only = true;
            state.netrw_dir = Some(parent);
        }
        self.view_mut().cursor.line = 1;
        self.view_mut().cursor.col = 0;
        self.view_mut().scroll_top = 0;
        EngineAction::None
    }

    /// Open the git diff for the current file in a vertical split.
    pub(crate) fn cmd_git_diff(&mut self) -> EngineAction {
        // Ensure editor receives keys after opening diff.
        self.sc_has_focus = false;
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
                    state.source_file = Some(path.clone());
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

    /// Open a VSCode-style side-by-side diff: HEAD (read-only) on the left,
    /// working copy (editable) on the right, with LCS diff coloring.
    ///
    /// If a diff split is already active, closes it first to avoid
    /// accumulating splits on repeated invocations.
    /// Clear `diff_label` on both buffers of a diff window pair.
    pub(crate) fn clear_diff_labels(&mut self, win_a: WindowId, win_b: WindowId) {
        for wid in [win_a, win_b] {
            if let Some(buf_id) = self.windows.get(&wid).map(|w| w.buffer_id) {
                if let Some(state) = self.buffer_manager.get_mut(buf_id) {
                    state.diff_label = None;
                }
            }
        }
    }

    pub fn cmd_git_diff_split(&mut self, path: &Path) -> EngineAction {
        // Ensure editor receives keys after opening diff.
        self.sc_has_focus = false;
        // If a diff split is already active, tear it down first.
        if let Some((left_win, right_win)) = self.diff_window_pair.take() {
            self.diff_results.clear();
            self.diff_aligned.clear();
            self.scroll_bind_pairs
                .retain(|&(a, b)| !(a == left_win && b == right_win));
            // Clear the diff label on the working copy buffer.
            if let Some(right_buf) = self.windows.get(&right_win).map(|w| w.buffer_id) {
                if let Some(state) = self.buffer_manager.get_mut(right_buf) {
                    state.diff_label = None;
                }
            }
            // Close the HEAD (left) window + its scratch buffer.
            if self.windows.contains_key(&left_win) {
                let left_buf = self.windows[&left_win].buffer_id;
                self.windows.remove(&left_win);
                // Remove from layout.
                let tab = self.active_tab_mut();
                if let Some(new_layout) = tab.layout.remove(left_win) {
                    tab.layout = new_layout;
                }
                // Delete the scratch buffer if nothing else references it.
                let still_used = self.windows.values().any(|w| w.buffer_id == left_buf);
                if !still_used {
                    let _ = self.buffer_manager.delete(left_buf, true);
                }
            }
            // Make sure we're focused on a valid window.
            if !self
                .active_tab()
                .layout
                .window_ids()
                .contains(&self.active_tab().active_window)
            {
                if let Some(first) = self.active_tab().layout.window_ids().first().copied() {
                    self.active_tab_mut().active_window = first;
                }
            }
        }

        let repo_root = match git::find_repo_root(path) {
            Some(r) => r,
            None => {
                self.message = "Not in a git repository".to_string();
                return EngineAction::Error;
            }
        };
        let rel_path = match path.strip_prefix(&repo_root) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => {
                self.message = "Cannot compute relative path".to_string();
                return EngineAction::Error;
            }
        };
        let head_content = match git::show_file_at_ref(&repo_root, "HEAD", &rel_path) {
            Some(c) => c,
            None => {
                self.message = "File has no HEAD version (untracked?)".to_string();
                return EngineAction::Error;
            }
        };

        // Open working copy in a new tab.
        self.new_tab(Some(path));
        let right_win = self.active_window_id();
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());

        // Label the working copy tab.
        let right_buf_id = self.active_window().buffer_id;
        if let Some(state) = self.buffer_manager.get_mut(right_buf_id) {
            state.diff_label = Some(format!("{file_name} (Working Tree)"));
        }

        // Create scratch buffer with HEAD content.
        let head_buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(head_buf_id) {
            state.buffer.content = ropey::Rope::from_str(&head_content);
            state.read_only = true;
            state.scratch_name = Some(format!("{file_name} (HEAD)"));
            // Set syntax highlighting to match the file type.
            if let Some(syn) = crate::core::syntax::Syntax::new_from_path_with_overrides(
                path.to_str(),
                Some(&self.highlight_overrides),
            ) {
                state.syntax = Some(syn);
            }
            state.update_syntax();
        }

        // Split: new window (left) gets HEAD buffer.
        self.split_window(SplitDirection::Vertical, None);
        let left_win = self.active_window_id();
        self.active_window_mut().buffer_id = head_buf_id;

        // Focus the right (working copy) window.
        let tab = self.active_tab_mut();
        tab.active_window = right_win;

        // Bind scroll and set up diff.
        self.scroll_bind_pairs.push((left_win, right_win));
        self.diff_window_pair = Some((left_win, right_win));
        self.compute_diff();
        self.diff_unchanged_hidden = true;
        self.diff_apply_folds();
        self.diff_jump_to_first_change(left_win, right_win);

        // Refresh git diff on working copy for gutter markers.
        let right_buf_id = self
            .windows
            .get(&right_win)
            .map(|w| w.buffer_id)
            .unwrap_or(head_buf_id);
        self.refresh_git_diff(right_buf_id);

        self.message = format!("Diff split: {}", path.display());
        EngineAction::None
    }

    /// Resolve the currently selected SC item and, for files with a HEAD
    /// version, kick off a background `git show` so the sidebar can repaint
    /// the selection highlight without blocking. For untracked/new files,
    /// worktree switches and log entries the action runs inline.
    /// Returns `true` if the action was fully handled synchronously (the
    /// caller should give focus back to the editor), `false` if a background
    /// diff was requested (the caller should keep SC focus and poll later).
    pub fn sc_open_selected_async(&mut self) -> bool {
        let (section, idx) = self.sc_flat_to_section_idx(self.sc_selected);
        if section == 2 {
            self.sc_switch_worktree(idx);
            return true;
        }
        if section == 3 && idx != usize::MAX {
            if let Some(entry) = self.sc_log.get(idx) {
                self.message = format!("{} {}", entry.hash, entry.message);
            }
            return true;
        }
        if idx == usize::MAX {
            // Header row — no action.
            return true;
        }
        let statuses = self.sc_file_statuses.clone();
        let all_files: Vec<&git::FileStatus> = if section == 0 {
            statuses.iter().filter(|f| f.staged.is_some()).collect()
        } else {
            statuses.iter().filter(|f| f.unstaged.is_some()).collect()
        };
        let f = match all_files.get(idx) {
            Some(f) => *f,
            None => return true,
        };
        let git_root = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        let abs_path = git_root.join(&f.path);
        if !abs_path.exists() {
            self.message = format!("SC: file not found: {}", abs_path.display());
            return true;
        }
        let is_new = matches!(f.unstaged, Some(git::StatusKind::Untracked))
            || matches!(f.staged, Some(git::StatusKind::Added));
        if is_new {
            self.new_tab(Some(&abs_path));
            self.sc_has_focus = false;
            return true;
        }
        // Open the tab immediately so it appears while the background
        // thread fetches the HEAD content for the diff split.
        let file_name = abs_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());
        self.new_tab(Some(&abs_path));
        let right_win = self.active_window_id();
        let right_buf_id = self.active_window().buffer_id;
        if let Some(state) = self.buffer_manager.get_mut(right_buf_id) {
            state.diff_label = Some(format!("{file_name} (Working Tree)"));
        }
        self.sc_diff_pending_win = Some(right_win);

        // Kick off background git show for diff.
        let rel_path = f.path.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.sc_diff_rx = Some(rx);
        let root = git_root.clone();
        std::thread::spawn(move || {
            let content = git::show_file_at_ref(&root, "HEAD", &rel_path).unwrap_or_default();
            let _ = tx.send((abs_path, content));
        });
        false
    }

    /// Poll for a completed background diff request. Returns `true` when
    /// new results arrived and a redraw is needed.
    pub fn poll_sc_diff(&mut self) -> bool {
        let (abs_path, head_content) = match self.sc_diff_rx {
            Some(ref rx) => match rx.try_recv() {
                Ok(r) => r,
                Err(_) => return false,
            },
            None => return false,
        };
        self.sc_diff_rx = None;
        let right_win = match self.sc_diff_pending_win.take() {
            Some(w) => w,
            None => return false,
        };

        // If the pre-opened window was closed before the thread finished, bail.
        if !self.windows.contains_key(&right_win) {
            return false;
        }

        if head_content.is_empty() {
            // No HEAD version — the tab is already open, nothing more to do.
            return true;
        }

        // Add the HEAD split to the already-open tab.
        self.sc_apply_diff_split(right_win, &abs_path, &head_content);
        // Restore SC panel focus so the user can keep navigating.
        self.sc_has_focus = true;
        true
    }

    /// Add the HEAD side of a diff split to a tab that already has the
    /// working copy open in `right_win`.
    pub(crate) fn sc_apply_diff_split(
        &mut self,
        right_win: WindowId,
        path: &Path,
        head_content: &str,
    ) {
        // Tear down any existing diff split.
        if let Some((left_win, old_right)) = self.diff_window_pair.take() {
            self.diff_results.clear();
            self.diff_aligned.clear();
            self.scroll_bind_pairs
                .retain(|&(a, b)| !(a == left_win && b == old_right));
            self.clear_diff_labels(left_win, old_right);
            if self.windows.contains_key(&left_win) {
                let left_buf = self.windows[&left_win].buffer_id;
                self.windows.remove(&left_win);
                let tab = self.active_tab_mut();
                if let Some(new_layout) = tab.layout.remove(left_win) {
                    tab.layout = new_layout;
                }
                let still_used = self.windows.values().any(|w| w.buffer_id == left_buf);
                if !still_used {
                    let _ = self.buffer_manager.delete(left_buf, true);
                }
            }
            if !self
                .active_tab()
                .layout
                .window_ids()
                .contains(&self.active_tab().active_window)
            {
                if let Some(first) = self.active_tab().layout.window_ids().first().copied() {
                    self.active_tab_mut().active_window = first;
                }
            }
        }

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());

        // Ensure the right_win's tab is active so the split lands there.
        // Find the group+tab containing right_win and activate them.
        let target = self.editor_groups.iter().find_map(|(&gid, group)| {
            group
                .tabs
                .iter()
                .enumerate()
                .find(|(_, tab)| tab.layout.window_ids().contains(&right_win))
                .map(|(ti, _)| (gid, ti))
        });
        if let Some((gid, ti)) = target {
            self.active_group = gid;
            self.active_group_mut().active_tab = ti;
            self.active_tab_mut().active_window = right_win;
        }

        // Create scratch buffer with HEAD content.
        let head_buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(head_buf_id) {
            state.buffer.content = ropey::Rope::from_str(head_content);
            state.read_only = true;
            state.scratch_name = Some(format!("{file_name} (HEAD)"));
            if let Some(syn) = crate::core::syntax::Syntax::new_from_path_with_overrides(
                path.to_str(),
                Some(&self.highlight_overrides),
            ) {
                state.syntax = Some(syn);
            }
            state.update_syntax();
        }

        // Split: new window (left) gets HEAD buffer.
        self.split_window(SplitDirection::Vertical, None);
        let left_win = self.active_window_id();
        self.active_window_mut().buffer_id = head_buf_id;

        // Focus the right (working copy) window.
        let tab = self.active_tab_mut();
        tab.active_window = right_win;

        // Bind scroll and set up diff.
        self.scroll_bind_pairs.push((left_win, right_win));
        self.diff_window_pair = Some((left_win, right_win));
        self.compute_diff();
        self.diff_unchanged_hidden = true;
        self.diff_apply_folds();
        self.diff_jump_to_first_change(left_win, right_win);

        let right_buf_id = self
            .windows
            .get(&right_win)
            .map(|w| w.buffer_id)
            .unwrap_or(head_buf_id);
        self.refresh_git_diff(right_buf_id);
        self.message = format!("Diff split: {}", path.display());
    }

    /// Open a side-by-side diff for a file at a specific commit vs its parent.
    /// Both sides are read-only scratch buffers showing `hash~1` (left) and `hash` (right).
    pub fn open_commit_file_diff(&mut self, hash: &str, rel_path: &str) {
        let git_root = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        let file_name = std::path::Path::new(rel_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| rel_path.to_string());
        let short = &hash[..hash.len().min(8)];

        // Fetch "after" (the commit) and "before" (parent commit)
        let after = git::show_file_at_ref(&git_root, hash, rel_path).unwrap_or_default();
        let parent_ref = format!("{hash}~1");
        let before = git::show_file_at_ref(&git_root, &parent_ref, rel_path).unwrap_or_default();

        // If both are empty, nothing to show
        if after.is_empty() && before.is_empty() {
            self.message = format!("No content for {rel_path} at {short}");
            return;
        }

        // Create "after" buffer (right pane)
        let right_buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(right_buf_id) {
            state.buffer.content = ropey::Rope::from_str(&after);
            state.read_only = true;
            state.scratch_name = Some(format!("{file_name} ({short})"));
            state.diff_label = Some(format!("{file_name} ({short})"));
            if let Some(syn) = crate::core::syntax::Syntax::new_from_path_with_overrides(
                Some(rel_path),
                Some(&self.highlight_overrides),
            ) {
                state.syntax = Some(syn);
                state.update_syntax();
            }
        }

        // Open in a new tab
        self.new_tab(None);
        let right_win = self.active_window_id();
        self.active_window_mut().buffer_id = right_buf_id;
        self.active_window_mut().view.cursor = crate::core::cursor::Cursor::default();

        // Create "before" buffer (left pane)
        let left_buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(left_buf_id) {
            state.buffer.content = ropey::Rope::from_str(&before);
            state.read_only = true;
            state.scratch_name = Some(format!("{file_name} ({short}~1)"));
            if let Some(syn) = crate::core::syntax::Syntax::new_from_path_with_overrides(
                Some(rel_path),
                Some(&self.highlight_overrides),
            ) {
                state.syntax = Some(syn);
                state.update_syntax();
            }
        }

        // Split vertically — new window (left) gets the "before" buffer
        self.split_window(SplitDirection::Vertical, None);
        let left_win = self.active_window_id();
        self.active_window_mut().buffer_id = left_buf_id;

        // Focus the right (after) window
        self.active_tab_mut().active_window = right_win;

        // Bind scroll and compute diff
        self.scroll_bind_pairs.push((left_win, right_win));
        self.diff_window_pair = Some((left_win, right_win));
        self.compute_diff();
        self.diff_unchanged_hidden = true;
        self.diff_apply_folds();
        self.diff_jump_to_first_change(left_win, right_win);
        self.message = format!("Diff: {} @ {}", rel_path, short);
    }

    /// Jump to the next changed region below the cursor.
    /// On real files: uses `git_diff` markers. On diff buffers: searches for `@@` headers.
    /// In two-window diff mode: uses `diff_results` for navigation.
    pub fn jump_next_hunk(&mut self) {
        if let Some((a, b)) = self.diff_window_pair {
            let active = self.active_window_id();
            if active == a || active == b {
                self.diff_jump_next();
                return;
            }
        }
        let cur = self.view().cursor.line;
        let bid = self.active_window().buffer_id;
        let git_diff = &self.buffer_manager.get(bid).map(|s| &s.git_diff);
        let has_git = git_diff.is_some_and(|d| !d.is_empty());

        if has_git {
            // Navigate using git_diff markers on real files.
            let gd = self.buffer_manager.get(bid).unwrap();
            let total = gd.git_diff.len();
            // Skip past the current changed region, then find the next one.
            let mut i = cur + 1;
            // Skip lines that are part of the same changed region as the cursor.
            while i < total
                && gd.git_diff.get(cur).copied().flatten().is_some()
                && gd.git_diff.get(i).copied().flatten().is_some()
            {
                i += 1;
            }
            // Find the next changed line.
            while i < total {
                if gd.git_diff[i].is_some() {
                    self.view_mut().cursor.line = i;
                    self.view_mut().cursor.col = 0;
                    self.scroll_cursor_center();
                    return;
                }
                i += 1;
            }
            self.message = "No more hunks".to_string();
        } else {
            // Fallback: search for @@ headers in diff buffers.
            let start = cur + 1;
            let total = self.buffer().len_lines();
            for i in start..total {
                let line: String = self.buffer().content.line(i).chars().collect();
                if line.starts_with("@@") {
                    self.view_mut().cursor.line = i;
                    self.view_mut().cursor.col = 0;
                    return;
                }
            }
            self.message = "No more hunks".to_string();
        }
    }

    /// Jump to the previous changed region above the cursor.
    /// On real files: uses `git_diff` markers. On diff buffers: searches for `@@` headers.
    /// In two-window diff mode: uses `diff_results` for navigation.
    pub fn jump_prev_hunk(&mut self) {
        if let Some((a, b)) = self.diff_window_pair {
            let active = self.active_window_id();
            if active == a || active == b {
                self.diff_jump_prev();
                return;
            }
        }
        let cur = self.view().cursor.line;
        let bid = self.active_window().buffer_id;
        let git_diff = &self.buffer_manager.get(bid).map(|s| &s.git_diff);
        let has_git = git_diff.is_some_and(|d| !d.is_empty());

        if has_git {
            let gd = self.buffer_manager.get(bid).unwrap();
            // Skip backwards past the current changed region.
            let mut i = cur.saturating_sub(1);
            while i > 0
                && gd.git_diff.get(cur).copied().flatten().is_some()
                && gd.git_diff.get(i).copied().flatten().is_some()
            {
                i -= 1;
            }
            // Find the previous changed line.
            loop {
                if gd.git_diff.get(i).copied().flatten().is_some() {
                    // Walk backwards to the start of this changed region.
                    while i > 0 && gd.git_diff.get(i - 1).copied().flatten().is_some() {
                        i -= 1;
                    }
                    self.view_mut().cursor.line = i;
                    self.view_mut().cursor.col = 0;
                    self.scroll_cursor_center();
                    return;
                }
                if i == 0 {
                    break;
                }
                i -= 1;
            }
            self.message = "No more hunks".to_string();
        } else {
            for i in (0..cur).rev() {
                let line: String = self.buffer().content.line(i).chars().collect();
                if line.starts_with("@@") {
                    self.view_mut().cursor.line = i;
                    self.view_mut().cursor.col = 0;
                    return;
                }
            }
            self.message = "No more hunks".to_string();
        }
    }

    /// Toggle inline git blame annotations for the current buffer.
    pub fn toggle_inline_blame(&mut self) {
        if self.blame_annotations_active {
            self.line_annotations.clear();
            self.blame_annotations_active = false;
            self.editor_hover_content.clear();
            self.blame_rx = None;
            self.message = "Inline blame off".to_string();
            return;
        }
        let file = match self.file_path() {
            Some(p) => p.to_path_buf(),
            None => {
                self.message = "No file".to_string();
                return;
            }
        };
        let repo_root = match crate::core::git::find_repo_root(&file) {
            Some(r) => r,
            None => {
                self.message = "Not a git repository".to_string();
                return;
            }
        };
        // Get buffer contents for unsaved changes.
        let bid = self.active_window().buffer_id;
        let buf_content = if self
            .buffer_manager
            .get(bid)
            .map(|s| s.dirty)
            .unwrap_or(false)
        {
            self.buffer_manager.get(bid).map(|s| {
                let rope = &s.buffer.content;
                let mut text = String::new();
                for i in 0..rope.len_lines() {
                    text.push_str(&rope.line(i).to_string());
                }
                text
            })
        } else {
            None
        };
        // Spawn blame on a background thread to avoid blocking the UI.
        let (tx, rx) = std::sync::mpsc::channel();
        let repo = repo_root.clone();
        let f = file.clone();
        std::thread::spawn(move || {
            let entries =
                crate::core::git::blame_file_structured(&repo, &f, buf_content.as_deref());
            let _ = tx.send(entries);
        });
        self.blame_rx = Some(rx);
        self.message = "Loading blame…".to_string();
    }

    /// Poll for async blame results. Call from backend event loops.
    /// Returns true if blame data was applied (triggers redraw).
    pub fn poll_blame(&mut self) -> bool {
        let entries = match self.blame_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            Some(e) => e,
            None => return false,
        };
        self.blame_rx = None;
        if entries.is_empty() {
            self.message = "git blame returned no data".to_string();
            return true;
        }
        // Find repo root for commit URL generation.
        let repo_root = self
            .file_path()
            .and_then(|p| crate::core::git::find_repo_root(p));
        self.line_annotations.clear();
        self.editor_hover_content.clear();
        for (i, info) in entries.iter().enumerate() {
            if info.not_committed {
                self.line_annotations.insert(i, "Not committed".to_string());
            } else {
                self.line_annotations.insert(
                    i,
                    format!("{}, {} — {}", info.author, info.relative_date, info.message),
                );
                // Build GitLens-style rich hover markdown.
                let hash = &info.hash;
                let url = repo_root
                    .as_ref()
                    .map(|r| crate::core::git::commit_url(r, hash))
                    .unwrap_or(None);
                let abs_date = crate::core::git::epoch_to_absolute(info.timestamp, info.tz_offset);
                let hash_link = if let Some(ref u) = url {
                    format!("[`{}`]({})", hash, u)
                } else {
                    format!("`{}`", hash)
                };
                let md = format!(
                    "**{}**, {} ({})\n\n{}\n\n---\n\n{}",
                    info.author, info.relative_date, abs_date, info.message, hash_link
                );
                self.editor_hover_content.insert(i, md);
            }
        }
        self.blame_annotations_active = true;
        self.message = format!("Inline blame on ({} lines)", entries.len());
        true
    }

    /// Open the diff peek popup for the hunk under the cursor on the current buffer.
    pub fn open_diff_peek(&mut self) {
        let bid = self.active_window().buffer_id;
        let cursor_line = self.view().cursor.line;
        let hunks = match self.buffer_manager.get(bid) {
            Some(s) => s.diff_hunks.clone(),
            None => {
                self.message = "No diff data".to_string();
                return;
            }
        };
        if hunks.is_empty() {
            self.message = "No changes in this file".to_string();
            return;
        }
        let idx = match git::hunk_for_line(&hunks, cursor_line) {
            Some(i) => i,
            None => {
                self.message = "No hunk at cursor".to_string();
                return;
            }
        };
        let h = &hunks[idx];
        self.diff_peek = Some(DiffPeekState {
            hunk_index: idx,
            anchor_line: cursor_line,
            hunk_lines: h.hunk.lines.clone(),
            file_header: h.file_header.clone(),
            hunk: h.hunk.clone(),
        });
    }

    /// Close the diff peek popup.
    pub fn close_diff_peek(&mut self) {
        self.diff_peek = None;
    }

    /// Revert the hunk shown in the diff peek popup.
    pub(crate) fn diff_peek_revert(&mut self) {
        let peek = match self.diff_peek.take() {
            Some(p) => p,
            None => return,
        };
        let bid = self.active_window().buffer_id;
        let path = match self
            .buffer_manager
            .get(bid)
            .and_then(|s| s.file_path.clone())
        {
            Some(p) => p,
            None => {
                self.message = "No file path".to_string();
                return;
            }
        };
        let dir = match path.parent() {
            Some(d) => d.to_path_buf(),
            None => return,
        };
        match git::revert_hunk(&dir, &peek.file_header, &peek.hunk) {
            Ok(()) => {
                // Reload buffer contents from disk.
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    if let Some(state) = self.buffer_manager.get_mut(bid) {
                        state.buffer.content = ropey::Rope::from_str(&contents);
                    }
                }
                self.refresh_git_diff(bid);
                self.compute_diff();
                self.message = "Hunk reverted".to_string();
            }
            Err(e) => {
                self.message = format!("Revert failed: {e}");
            }
        }
    }

    /// Stage the hunk shown in the diff peek popup.
    pub(crate) fn diff_peek_stage(&mut self) {
        let peek = match self.diff_peek.take() {
            Some(p) => p,
            None => return,
        };
        let bid = self.active_window().buffer_id;
        let path = match self
            .buffer_manager
            .get(bid)
            .and_then(|s| s.file_path.clone())
        {
            Some(p) => p,
            None => {
                self.message = "No file path".to_string();
                return;
            }
        };
        let dir = match git::find_repo_root(&path) {
            Some(d) => d,
            None => {
                self.message = "Not a git repository".to_string();
                return;
            }
        };
        match git::stage_hunk(&dir, &peek.file_header, &peek.hunk) {
            Ok(()) => {
                self.refresh_git_diff(bid);
                self.compute_diff();
                self.message = format!("Hunk {} staged", peek.hunk_index + 1);
            }
            Err(e) => {
                self.message = format!("Stage failed: {e}");
            }
        }
    }

    /// Handle a keypress while the diff peek popup is open.
    /// Returns true if the key was consumed, false to pass through.
    pub(crate) fn handle_diff_peek_key(&mut self, key_name: &str, unicode: Option<char>) -> bool {
        match key_name {
            "Escape" | "q" => {
                self.close_diff_peek();
                true
            }
            _ => match unicode {
                Some('s') => {
                    self.diff_peek_stage();
                    true
                }
                Some('r') => {
                    self.diff_peek_revert();
                    true
                }
                _ => {
                    // Any other key closes the popup and falls through.
                    self.close_diff_peek();
                    false
                }
            },
        }
    }

    /// Stage the hunk under the cursor using `git apply --cached`.
    pub(crate) fn cmd_git_stage_hunk(&mut self) -> EngineAction {
        let source_file = match self.active_buffer_state().source_file.clone() {
            Some(p) => p,
            None => {
                self.message = "Not a diff buffer (use :Gdiff first)".to_string();
                return EngineAction::None;
            }
        };
        let repo_dir = match git::find_repo_root(&source_file) {
            Some(d) => d,
            None => {
                self.message = "Not a git repository".to_string();
                return EngineAction::Error;
            }
        };
        let diff_text: String = self.buffer().content.chars().collect();
        let cursor_line = self.view().cursor.line;
        let (file_header, hunks) = git::parse_diff_hunks(&diff_text);
        if hunks.is_empty() {
            self.message = "No hunks in buffer".to_string();
            return EngineAction::None;
        }
        // Find which hunk the cursor is in by walking line positions.
        let header_lines = if file_header.is_empty() {
            0
        } else {
            file_header.lines().count()
        };
        let mut pos = header_lines;
        let mut target = hunks.len() - 1; // default: last hunk
        for (i, hunk) in hunks.iter().enumerate() {
            let end = pos + 1 + hunk.lines.len(); // +1 for @@ line
            if cursor_line < end {
                target = i;
                break;
            }
            pos = end;
        }
        let hunk = hunks[target].clone();
        match git::stage_hunk(&repo_dir, &file_header, &hunk) {
            Ok(()) => {
                // Refresh gutter markers on the source buffer if it is open.
                let source_buf_id = self.buffer_manager.list().into_iter().find(|&id| {
                    self.buffer_manager
                        .get(id)
                        .and_then(|s| s.file_path.as_deref())
                        == Some(source_file.as_path())
                });
                if let Some(id) = source_buf_id {
                    self.refresh_git_diff(id);
                }
                self.message = format!("Hunk {} staged", target + 1);
                EngineAction::None
            }
            Err(e) => {
                self.message = format!("Stage hunk failed: {e}");
                EngineAction::Error
            }
        }
    }

    /// Helper: resolve the git repo dir from either the current file's directory or cwd.
    pub(crate) fn git_dir(&self) -> PathBuf {
        self.file_path()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    /// Open `git status` output in a new read-only buffer (vertical split).
    pub(crate) fn cmd_git_status(&mut self) -> EngineAction {
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

    /// Open help text for `topic` in a new read-only vertical split.
    pub(crate) fn cmd_help(&mut self, topic: &str) -> EngineAction {
        let text = match topic {
            "" | "topics" => concat!(
                "VimCode Help\n",
                "=============\n",
                "\n",
                "Available topics:\n",
                "  :help explorer    File explorer sidebar keys\n",
                "  :help keys        Normal mode key reference\n",
                "  :help commands    Command mode reference\n",
                "\n",
                "Type :help <topic> for details.\n",
            )
            .to_string(),
            "explorer" => concat!(
                "Explorer Sidebar\n",
                "================\n",
                "\n",
                "Toggle & Focus:\n",
                "  Ctrl-B            Toggle sidebar visibility\n",
                "  Ctrl-Shift-E      Focus the sidebar (or toggle)\n",
                "\n",
                "Navigation:\n",
                "  j / k             Move selection down / up\n",
                "  Enter             Open file / toggle directory\n",
                "  Esc               Return focus to editor\n",
                "\n",
                "Explorer Mode (press ? to toggle):\n",
                "  a                 New file — type name, Enter to create\n",
                "  A                 New directory — type name, Enter to create\n",
                "  r                 Rename — type new name, Enter to confirm\n",
                "  M                 Move — type destination dir, Enter to confirm\n",
                "  D                 Delete — y to confirm, n to cancel\n",
                "\n",
                "The activity bar (left edge) also provides clickable icons\n",
                "for the explorer, search panel, and settings.\n",
                "\n",
                "Keys are configurable via settings.json under \"explorer_keys\".\n",
                "Example: { \"explorer_keys\": { \"delete\": \"x\", \"rename\": \"R\" } }\n",
            )
            .to_string(),
            "keys" => concat!(
                "Normal Mode Keys\n",
                "================\n",
                "\n",
                "Motion:\n",
                "  h/j/k/l           Left / Down / Up / Right\n",
                "  w/W/b/B/e/E       Word motions\n",
                "  0/^/$              Line start / first non-blank / line end\n",
                "  gg/G              Top / bottom of file\n",
                "  %                 Matching bracket\n",
                "  f/F/t/T + char    Find char on line\n",
                "  {/}               Paragraph up / down\n",
                "  Ctrl-D/Ctrl-U     Half-page down / up\n",
                "  Ctrl-F/Ctrl-B     Full-page down / up\n",
                "\n",
                "Editing:\n",
                "  i/a/o/O           Insert mode (before/after/below/above)\n",
                "  d/c/y + motion    Delete / change / yank with motion\n",
                "  dd/cc/yy          Line-wise delete / change / yank\n",
                "  x/X               Delete char / backspace\n",
                "  p/P               Paste after / before\n",
                "  u / Ctrl-R        Undo / redo\n",
                "  . (dot)           Repeat last change\n",
                "  J                 Join lines\n",
                "  ~ / g~            Toggle case\n",
                "  >> / <<           Indent / dedent\n",
                "\n",
                "Search:\n",
                "  / / ?             Search forward / backward\n",
                "  n/N               Next / previous match\n",
                "  * / #             Search word under cursor fwd / back\n",
                "\n",
                "Other:\n",
                "  :                 Enter command mode\n",
                "  v/V               Visual char / line mode\n",
                "  Ctrl-P / <leader>sf  Fuzzy file finder\n",
                "  Ctrl-Shift-F / <leader>sg  Live grep\n",
                "  Ctrl-Shift-P / <leader>sp  Command palette\n",
                "  gd                Go to definition (LSP)\n",
                "  K                 Hover info (LSP)\n",
                "  ]d / [d           Next / prev diagnostic\n",
                "  ]c / [c           Next / prev hunk\n",
                "  Ctrl-O / Ctrl-I   Jump list back / forward\n",
                "  zz / zt / zb      Scroll cursor center / top / bottom\n",
                "  q<reg> / @<reg>   Record / play macro\n",
            )
            .to_string(),
            "commands" => concat!(
                "Command Mode\n",
                "============\n",
                "\n",
                "File:\n",
                "  :w                Save\n",
                "  :q / :q!          Quit / force quit\n",
                "  :wq / :x          Save and quit\n",
                "  :e <file>         Edit file\n",
                "  :saveas <file>    Save as\n",
                "\n",
                "Buffers & Windows:\n",
                "  :ls / :buffers    List buffers\n",
                "  :bn / :bp / :b#   Next / prev / alternate buffer\n",
                "  :bd               Delete buffer\n",
                "  :split / :vsplit  Horizontal / vertical split\n",
                "  :close / :only    Close window / close others\n",
                "  :tabnew / :tabc   New tab / close tab\n",
                "\n",
                "Search & Replace:\n",
                "  :s/pat/rep/flags  Substitute (current line)\n",
                "  :%s/pat/rep/g     Substitute (all lines)\n",
                "  :grep <pattern>   Grep into quickfix\n",
                "\n",
                "Git:\n",
                "  :Gdiff / :Gd      Git diff in vsplit\n",
                "  :Gstatus / :Gs    Git status\n",
                "  :Gblame / :Gb     Git blame\n",
                "  :Gadd             Stage current file\n",
                "\n",
                "Other:\n",
                "  :set              Show settings\n",
                "  :set <opt>=<val>  Change setting\n",
                "  :norm <keys>      Run normal keys on range\n",
                "  :help <topic>     Show help\n",
                "  :N                Jump to line N\n",
            )
            .to_string(),
            _ => {
                self.message = format!("No help for '{}'. Try :help topics", topic);
                return EngineAction::None;
            }
        };
        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&text);
        }
        self.split_window(SplitDirection::Vertical, None);
        self.active_window_mut().buffer_id = buf_id;
        self.message = if topic.is_empty() {
            "Help".to_string()
        } else {
            format!("Help: {}", topic)
        };
        EngineAction::None
    }

    /// Stage the current file (`:Gadd`) or all changes (`:Gadd!`).
    pub(crate) fn cmd_git_add(&mut self, all: bool) -> EngineAction {
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
    pub(crate) fn cmd_git_commit(&mut self, message: &str) -> EngineAction {
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
    pub(crate) fn cmd_git_push(&mut self) -> EngineAction {
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
    pub(crate) fn cmd_git_blame(&mut self) -> EngineAction {
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
    // File rename / move
    // =======================================================================

    /// Rename or move a single file / directory within the filesystem.
    ///
    /// `new_name` is the bare name (no path separators).  The new location is
    /// built as `old_path.parent() / new_name`.
    ///
    /// Any open buffer whose `file_path` matches `old_path` is updated in
    /// place so the editor does not show a stale path.
    pub fn rename_file(&mut self, old_path: &Path, new_name: &str) -> Result<(), String> {
        if new_name.is_empty() {
            return Err("Name cannot be empty".to_string());
        }
        if new_name.contains('/') || new_name.contains('\\') {
            return Err("Name must not contain path separators".to_string());
        }
        let parent = old_path
            .parent()
            .ok_or_else(|| "Cannot rename root".to_string())?;
        let new_path = parent.join(new_name);
        std::fs::rename(old_path, &new_path).map_err(|e| format!("Rename failed: {}", e))?;

        // Update any open buffer that was showing the old path.
        for id in self.buffer_manager.list() {
            if let Some(state) = self.buffer_manager.get_mut(id) {
                if state.file_path.as_deref() == Some(old_path) {
                    state.canonical_path = new_path.canonicalize().ok();
                    state.file_path = Some(new_path.clone());
                    self.refresh_git_diff(id);
                }
            }
        }
        Ok(())
    }

    /// Start inline rename for the given path in the explorer sidebar.
    ///
    /// Pre-fills the input with the current filename and places the cursor
    /// at the end.  Backends should render an editable text field on the
    /// matching explorer row while this state is active.
    pub fn start_explorer_rename(&mut self, path: PathBuf) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        // Pre-select the stem (filename without extension) so Backspace
        // removes just the name, preserving the extension.
        let stem_end = if path.is_dir() {
            // Directories: select entire name
            name.len()
        } else {
            // Find last '.' that isn't at position 0 (dotfiles like .gitignore)
            name.rfind('.').filter(|&i| i > 0).unwrap_or(name.len())
        };
        self.explorer_rename = Some(ExplorerRenameState {
            path,
            cursor: stem_end,
            selection_anchor: if stem_end > 0 { Some(0) } else { None },
            input: name,
        });
    }

    /// Handle a key press while inline rename is active.
    ///
    /// Returns `true` if the key was consumed.  On Enter the rename is
    /// committed; on Escape it is cancelled.  Sets `explorer_needs_refresh`
    /// on success so backends rebuild the tree.
    pub fn handle_explorer_rename_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
    ) -> bool {
        let state = match self.explorer_rename.as_mut() {
            Some(s) => s,
            None => return false,
        };

        // Helper: get sorted selection range (start, end) or None.
        let sel_range = |s: &ExplorerRenameState| -> Option<(usize, usize)> {
            s.selection_anchor.map(|a| {
                let lo = a.min(s.cursor);
                let hi = a.max(s.cursor);
                (lo, hi)
            })
        };

        // Helper: delete selected text, place cursor at selection start.
        // Returns true if there was a selection to delete.
        fn delete_selection(s: &mut ExplorerRenameState) -> bool {
            if let Some(anchor) = s.selection_anchor.take() {
                let lo = anchor.min(s.cursor);
                let hi = anchor.max(s.cursor);
                if lo != hi {
                    s.input.drain(lo..hi);
                    s.cursor = lo;
                    return true;
                }
            }
            false
        }

        match key_name {
            "Escape" => {
                self.explorer_rename = None;
                return true;
            }
            "Return" => {
                let path = state.path.clone();
                let input = state.input.clone();
                self.explorer_rename = None;
                let new_name = input.trim();
                if new_name.is_empty() {
                    self.message = "Name cannot be empty".to_string();
                } else {
                    match self.rename_file(&path, new_name) {
                        Ok(()) => {
                            self.explorer_needs_refresh = true;
                            self.message = format!("Renamed to '{}'", new_name);
                        }
                        Err(e) => {
                            self.message = e;
                        }
                    }
                }
                return true;
            }
            "BackSpace" => {
                if !delete_selection(state) && state.cursor > 0 {
                    let prev = state.input[..state.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    state.input.remove(prev);
                    state.cursor = prev;
                }
                return true;
            }
            "Delete" => {
                if !delete_selection(state) && state.cursor < state.input.len() {
                    state.input.remove(state.cursor);
                }
                return true;
            }
            "Left" => {
                if state.cursor > 0 {
                    state.cursor = state.input[..state.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
                state.selection_anchor = None;
                return true;
            }
            "Right" => {
                if state.cursor < state.input.len() {
                    let rest = &state.input[state.cursor..];
                    state.cursor = rest
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| state.cursor + i)
                        .unwrap_or(state.input.len());
                }
                state.selection_anchor = None;
                return true;
            }
            "Home" => {
                state.cursor = 0;
                state.selection_anchor = None;
                return true;
            }
            "End" => {
                state.cursor = state.input.len();
                state.selection_anchor = None;
                return true;
            }
            _ => {}
        }

        // Ctrl shortcuts
        if ctrl {
            match key_name {
                "a" => {
                    // Select all
                    state.selection_anchor = Some(0);
                    state.cursor = state.input.len();
                    return true;
                }
                "c" => {
                    // Copy selection to clipboard
                    if let Some((lo, hi)) = sel_range(state) {
                        if lo != hi {
                            let text = state.input[lo..hi].to_string();
                            if let Some(ref cb) = self.clipboard_write {
                                let _ = cb(&text);
                            }
                        }
                    }
                    return true;
                }
                "x" => {
                    // Cut selection to clipboard
                    if let Some((lo, hi)) = sel_range(state) {
                        if lo != hi {
                            let text = state.input[lo..hi].to_string();
                            if let Some(ref cb) = self.clipboard_write {
                                let _ = cb(&text);
                            }
                            delete_selection(state);
                        }
                    }
                    return true;
                }
                "v" => {
                    // Paste from clipboard
                    delete_selection(state);
                    let paste = if let Some(ref cb) = self.clipboard_read {
                        cb().unwrap_or_default()
                    } else {
                        String::new()
                    };
                    // Only use first line
                    let line = paste.lines().next().unwrap_or("");
                    state.input.insert_str(state.cursor, line);
                    state.cursor += line.len();
                    return true;
                }
                _ => {}
            }
            // Consume other ctrl combos
            return true;
        }

        // Printable character insertion (replaces selection if any)
        if let Some(ch) = unicode {
            if !ch.is_control() {
                delete_selection(state);
                state.input.insert(state.cursor, ch);
                state.cursor += ch.len_utf8();
                return true;
            }
        }

        // Consume all other keys while rename is active (don't let them leak)
        true
    }

    // ── Inline new file/folder ──────────────────────────────────────────────

    /// Start inline new-file creation in the explorer sidebar.
    ///
    /// Creates an empty editable entry under `parent_dir`.  Backends should
    /// render a temporary row in the tree for this entry.
    pub fn start_explorer_new_file(&mut self, parent_dir: PathBuf) {
        self.explorer_new_entry = Some(ExplorerNewEntryState {
            parent_dir,
            input: String::new(),
            cursor: 0,
            is_folder: false,
        });
    }

    /// Start inline new-folder creation in the explorer sidebar.
    pub fn start_explorer_new_folder(&mut self, parent_dir: PathBuf) {
        self.explorer_new_entry = Some(ExplorerNewEntryState {
            parent_dir,
            input: String::new(),
            cursor: 0,
            is_folder: true,
        });
    }

    /// Handle a key press while inline new-entry creation is active.
    ///
    /// Returns `true` if the key was consumed.  On Enter the file/folder is
    /// created; on Escape it is cancelled.  Sets `explorer_needs_refresh`
    /// on success so backends rebuild the tree.
    pub fn handle_explorer_new_entry_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
    ) -> bool {
        let state = match self.explorer_new_entry.as_mut() {
            Some(s) => s,
            None => return false,
        };

        match key_name {
            "Escape" => {
                self.explorer_new_entry = None;
                return true;
            }
            "Return" => {
                let parent_dir = state.parent_dir.clone();
                let input = state.input.clone();
                let is_folder = state.is_folder;
                self.explorer_new_entry = None;
                let name = input.trim();
                if name.is_empty() {
                    // Silent cancel on empty name
                    return true;
                }
                let path = parent_dir.join(name);
                if path.exists() {
                    self.message = format!("'{}' already exists", name);
                    return true;
                }
                if is_folder {
                    match std::fs::create_dir_all(&path) {
                        Ok(()) => {
                            self.explorer_needs_refresh = true;
                            self.message = format!("Created folder: {}", name);
                        }
                        Err(e) => {
                            self.message = format!("Error creating folder: {}", e);
                        }
                    }
                } else {
                    match std::fs::write(&path, "") {
                        Ok(()) => {
                            self.explorer_needs_refresh = true;
                            self.message = format!("Created: {}", name);
                            if let Err(e) = self.open_file_with_mode(
                                &path,
                                crate::core::engine::OpenMode::Permanent,
                            ) {
                                self.message = e;
                            }
                        }
                        Err(e) => {
                            self.message = format!("Error creating file: {}", e);
                        }
                    }
                }
                return true;
            }
            "BackSpace" => {
                if state.cursor > 0 {
                    let prev = state.input[..state.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    state.input.remove(prev);
                    state.cursor = prev;
                }
                return true;
            }
            "Delete" => {
                if state.cursor < state.input.len() {
                    state.input.remove(state.cursor);
                }
                return true;
            }
            "Left" => {
                if state.cursor > 0 {
                    state.cursor = state.input[..state.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
                return true;
            }
            "Right" => {
                if state.cursor < state.input.len() {
                    let rest = &state.input[state.cursor..];
                    state.cursor = rest
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| state.cursor + i)
                        .unwrap_or(state.input.len());
                }
                return true;
            }
            "Home" => {
                state.cursor = 0;
                return true;
            }
            "End" => {
                state.cursor = state.input.len();
                return true;
            }
            _ => {}
        }

        // Ctrl shortcuts for new-entry input
        if ctrl {
            match key_name {
                "v" => {
                    // Paste from clipboard
                    let paste = if let Some(ref cb) = self.clipboard_read {
                        cb().unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let line = paste.lines().next().unwrap_or("");
                    state.input.insert_str(state.cursor, line);
                    state.cursor += line.len();
                    return true;
                }
                "a" => {
                    // Select all — no selection support in new-entry, just move to end
                    state.cursor = state.input.len();
                    return true;
                }
                _ => {}
            }
            return true;
        }

        // Printable character insertion
        if let Some(ch) = unicode {
            if !ch.is_control() {
                state.input.insert(state.cursor, ch);
                state.cursor += ch.len_utf8();
                return true;
            }
        }

        // Consume all other keys while new-entry is active
        true
    }

    /// Show a confirmation dialog before moving a file/folder.
    ///
    /// Stores the pending move and displays a Yes/No dialog.  The actual
    /// `move_file()` call happens when the user confirms via the dialog.
    pub fn confirm_move_file(&mut self, src: &Path, dest: &Path) {
        // Suppress dialog when source is already in the destination directory
        // (e.g. accidental micro-drag in the explorer tree).
        if let Some(parent) = src.parent() {
            if parent == dest {
                return;
            }
        }
        let src_name = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| src.to_string_lossy().to_string());
        let dest_display = dest.to_string_lossy().to_string();
        self.pending_move = Some((src.to_path_buf(), dest.to_path_buf()));
        self.show_dialog(
            "confirm_move",
            "Confirm Move",
            vec![format!("Move '{}' to '{}'?", src_name, dest_display)],
            vec![
                DialogButton {
                    label: "Yes".into(),
                    hotkey: 'y',
                    action: "yes".into(),
                },
                DialogButton {
                    label: "No".into(),
                    hotkey: 'n',
                    action: "no".into(),
                },
            ],
        );
    }

    /// Show a confirmation dialog before deleting a file/folder.
    ///
    /// Stores the pending delete path and displays a Yes/No dialog.  The actual
    /// deletion happens when the user confirms via the dialog.
    pub fn confirm_delete_file(&mut self, path: &Path) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        let item_type = if path.is_dir() { "folder" } else { "file" };
        self.pending_delete = Some(path.to_path_buf());
        self.show_dialog(
            "confirm_delete",
            "Confirm Delete",
            vec![format!("Delete {} '{}'?", item_type, name)],
            vec![
                DialogButton {
                    label: "Delete".into(),
                    hotkey: 'd',
                    action: "delete".into(),
                },
                DialogButton {
                    label: "Cancel".into(),
                    hotkey: '\0',
                    action: "cancel".into(),
                },
            ],
        );
    }

    /// Show a dialog with text input for moving a file to a new location.
    ///
    /// The dialog pre-fills the input with the file's current relative path.
    pub fn start_move_file_dialog(&mut self, src: &Path, project_root: &Path) {
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| src.to_string_lossy().to_string());
        let prefill = src
            .strip_prefix(project_root)
            .unwrap_or(src)
            .to_string_lossy()
            .to_string();
        self.pending_move = Some((src.to_path_buf(), PathBuf::new())); // dest filled on confirm
        self.show_dialog(
            "move_file_input",
            &format!("Move '{}'", name),
            vec!["Enter destination path:".into()],
            vec![
                DialogButton {
                    label: "Move".into(),
                    hotkey: '\0',
                    action: "move".into(),
                },
                DialogButton {
                    label: "Cancel".into(),
                    hotkey: '\0',
                    action: "cancel".into(),
                },
            ],
        );
        // Set up the text input field with pre-filled path
        if let Some(ref mut dlg) = self.dialog {
            dlg.input = Some(DialogInput {
                label: "Destination: ".into(),
                value: prefill,
                is_password: false,
            });
        }
    }

    /// Move `src` into `dest_dir` (a directory).
    ///
    /// The filename is preserved.  Any open buffer whose `file_path` matches
    /// `src` is updated to point at the new location.
    pub fn move_file(&mut self, src: &Path, dest: &Path) -> Result<(), String> {
        // If dest is a directory, move file into it keeping the original name.
        // Otherwise treat dest as the full destination path (allows rename+move).
        let final_dest = if dest.is_dir() {
            let file_name = src
                .file_name()
                .ok_or_else(|| "Cannot determine file name".to_string())?;
            dest.join(file_name)
        } else {
            let parent = dest
                .parent()
                .ok_or_else(|| "Invalid destination path".to_string())?;
            if !parent.is_dir() {
                return Err(format!("'{}' is not a directory", parent.display()));
            }
            dest.to_path_buf()
        };
        // Prevent moving a directory into its own subtree.
        if src.is_dir() {
            let canon_src = src.canonicalize().unwrap_or_else(|_| src.to_path_buf());
            let dest_parent = final_dest
                .parent()
                .and_then(|p| p.canonicalize().ok())
                .unwrap_or_else(|| final_dest.clone());
            if dest_parent.starts_with(&canon_src) {
                return Err("Cannot move a folder into its own subtree".to_string());
            }
        }

        // Prevent no-op moves (same location).
        if final_dest == src
            || src
                .canonicalize()
                .ok()
                .zip(final_dest.parent().and_then(|p| p.canonicalize().ok()))
                .is_some_and(|(cs, cd)| {
                    cs.parent() == Some(cd.as_path()) && cs.file_name() == final_dest.file_name()
                })
        {
            return Ok(());
        }

        std::fs::rename(src, &final_dest).map_err(|e| format!("Move failed: {}", e))?;

        // Update any open buffer that was showing the old path.
        for id in self.buffer_manager.list() {
            if let Some(state) = self.buffer_manager.get_mut(id) {
                if state.file_path.as_deref() == Some(src) {
                    state.canonical_path = final_dest.canonicalize().ok();
                    state.file_path = Some(final_dest.clone());
                    self.refresh_git_diff(id);
                }
            }
        }
        Ok(())
    }

    // =======================================================================
    // Two-way diff
    // =======================================================================

    /// Mark the current window as a diff participant (public for UI backends).
    ///
    /// - First call: remembers the window id in `diff_window_pair` (left side).
    /// - Second call: sets both windows and runs `compute_diff()`.
    /// - If diff is already active, resets and re-runs.
    pub fn cmd_diffthis(&mut self) -> EngineAction {
        let win = self.active_window_id();
        match self.diff_window_pair {
            None => {
                // First window: store it as the left side.
                self.diff_window_pair = Some((win, win)); // placeholder; right == left means "waiting"
                self.message = "DiffThis: select second window with :diffthis".to_string();
            }
            Some((a, b)) if a == b && a != win => {
                // Second window chosen: activate diff.
                self.diff_window_pair = Some((a, win));
                self.scroll_bind_pairs.push((a, win));
                self.compute_diff();
                self.diff_unchanged_hidden = true;
                self.diff_apply_folds();
                self.diff_jump_to_first_change(a, win);
                self.message = "Diff active".to_string();
            }
            Some(_) => {
                self.message = "Diff already active. Use :diffoff to reset.".to_string();
            }
        }
        EngineAction::None
    }

    /// Disable diff mode and clear all diff state.
    pub fn cmd_diffoff(&mut self) -> EngineAction {
        // Clear folds and scroll bindings on both diff windows.
        if let Some((a, b)) = self.diff_window_pair {
            if let Some(w) = self.windows.get_mut(&a) {
                w.view.open_all_folds();
            }
            if let Some(w) = self.windows.get_mut(&b) {
                w.view.open_all_folds();
            }
            self.scroll_bind_pairs
                .retain(|&(x, y)| !((x == a && y == b) || (x == b && y == a)));
            self.clear_diff_labels(a, b);
        }
        self.diff_window_pair = None;
        self.diff_results.clear();
        self.diff_aligned.clear();
        self.diff_aligned.clear();
        self.diff_unchanged_hidden = false;
        self.message = "Diff off".to_string();
        EngineAction::None
    }

    /// Open `path` in a new vertical split and immediately diff it against the
    /// current window.  Public for use by UI backends.
    pub fn cmd_diffsplit(&mut self, path: &Path) -> EngineAction {
        let left_win = self.active_window_id();
        self.split_window(SplitDirection::Vertical, Some(path));
        let right_win = self.active_window_id();
        self.scroll_bind_pairs.push((left_win, right_win));
        self.diff_window_pair = Some((left_win, right_win));
        self.compute_diff();
        self.diff_unchanged_hidden = true;
        self.diff_apply_folds();
        self.diff_jump_to_first_change(left_win, right_win);
        self.message = format!("Diff: {}", path.display());
        EngineAction::None
    }

    /// Internal: compute the LCS diff between the two diff windows and store
    /// results in `self.diff_results`.
    pub(crate) fn compute_diff(&mut self) {
        let (a_win, b_win) = match self.diff_window_pair {
            Some(pair) => pair,
            None => return,
        };
        let a_lines: Vec<String> = {
            if let Some(w) = self.windows.get(&a_win) {
                if let Some(s) = self.buffer_manager.get(w.buffer_id) {
                    s.buffer.content.lines().map(|l| l.to_string()).collect()
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        };
        let b_lines: Vec<String> = {
            if let Some(w) = self.windows.get(&b_win) {
                if let Some(s) = self.buffer_manager.get(w.buffer_id) {
                    s.buffer.content.lines().map(|l| l.to_string()).collect()
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        };
        let a_refs: Vec<&str> = a_lines.iter().map(String::as_str).collect();
        let b_refs: Vec<&str> = b_lines.iter().map(String::as_str).collect();
        let (mut da, mut db) = lcs_diff(&a_refs, &b_refs);
        // Post-process: short runs of Same lines sandwiched between changes
        // (blank lines, common braces, shared imports) fragment what the user
        // perceives as a single edit.  Re-classify runs of up to N Same lines
        // so the coloured block stays contiguous.
        merge_short_same_runs(&mut da, DiffLine::Removed);
        merge_short_same_runs(&mut db, DiffLine::Added);

        // Build aligned sequences with padding for visual alignment.
        let (aligned_a, aligned_b) = build_aligned_diff(&da, &db);
        self.diff_aligned.insert(a_win, aligned_a);
        self.diff_aligned.insert(b_win, aligned_b);

        self.diff_results.insert(a_win, da);
        self.diff_results.insert(b_win, db);
        // Re-apply folds if unchanged sections are hidden.
        if self.diff_unchanged_hidden {
            self.diff_apply_folds();
        }
    }

    /// Jump both diff windows to the first changed line, or reset to top if
    /// there are no changes.
    pub(crate) fn diff_jump_to_first_change(&mut self, left_win: WindowId, right_win: WindowId) {
        let first_change = self
            .diff_results
            .get(&right_win)
            .and_then(|d| d.iter().position(|s| *s != DiffLine::Same));
        if let Some(line) = first_change {
            for win_id in [left_win, right_win] {
                if let Some(w) = self.windows.get_mut(&win_id) {
                    w.view.cursor.line = line;
                    w.view.cursor.col = 0;
                    w.view.scroll_top = line.saturating_sub(3);
                }
            }
        } else {
            for win_id in [left_win, right_win] {
                if let Some(w) = self.windows.get_mut(&win_id) {
                    w.view.cursor.line = 0;
                    w.view.cursor.col = 0;
                    w.view.scroll_top = 0;
                }
            }
        }
    }

    /// Returns true if the editor is currently in a two-window diff view and
    /// at least one of the diff windows belongs to the active group.
    pub fn is_in_diff_view(&self) -> bool {
        if let Some((a, b)) = self.diff_window_pair {
            if a == b {
                return false;
            }
            // Check all groups so the toolbar appears on every group
            // that contains a diff window, not just the active one.
            for group in self.editor_groups.values() {
                let win_ids = group.active_tab().layout.window_ids();
                if win_ids.contains(&a) || win_ids.contains(&b) {
                    return true;
                }
            }
            false
        } else {
            false
        }
    }

    /// Collects contiguous runs of non-`DiffLine::Same` lines for a given window
    /// as `(start_line, end_line)` inclusive pairs.
    pub fn diff_change_regions(&self, win_id: WindowId) -> Vec<(usize, usize)> {
        let results = match self.diff_results.get(&win_id) {
            Some(r) => r,
            None => return vec![],
        };
        let mut regions = Vec::new();
        let mut i = 0;
        while i < results.len() {
            if results[i] != DiffLine::Same {
                let start = i;
                while i < results.len() && results[i] != DiffLine::Same {
                    i += 1;
                }
                regions.push((start, i - 1));
            } else {
                i += 1;
            }
        }
        regions
    }

    /// Jump to the next change region in the diff view.
    pub(crate) fn diff_jump_next(&mut self) {
        let win_id = self.active_window_id();
        let regions = self.diff_change_regions(win_id);
        if regions.is_empty() {
            self.message = "No changes in diff".to_string();
            return;
        }
        let cur = self.view().cursor.line;
        // Find first region starting past cursor.
        let (target, region_idx, wrapped) = if let Some((i, &(start, _))) =
            regions.iter().enumerate().find(|&(_, &(s, _))| s > cur)
        {
            (start, i, false)
        } else {
            (regions[0].0, 0, true)
        };
        self.view_mut().cursor.line = target;
        self.view_mut().cursor.col = 0;
        self.scroll_cursor_center();
        if wrapped {
            self.message = "Wrapped to first change".to_string();
        }
        // Move partner window's cursor to the corresponding change region.
        self.diff_sync_partner_cursor(win_id, region_idx);
    }

    /// Jump to the previous change region in the diff view.
    pub(crate) fn diff_jump_prev(&mut self) {
        let win_id = self.active_window_id();
        let regions = self.diff_change_regions(win_id);
        if regions.is_empty() {
            self.message = "No changes in diff".to_string();
            return;
        }
        let cur = self.view().cursor.line;
        // Find last region ending before cursor.
        let (target, region_idx, wrapped) = if let Some((i, &(start, _))) = regions
            .iter()
            .enumerate()
            .rev()
            .find(|&(_, &(_, e))| e < cur)
        {
            (start, i, false)
        } else {
            let last_idx = regions.len() - 1;
            (regions[last_idx].0, last_idx, true)
        };
        self.view_mut().cursor.line = target;
        self.view_mut().cursor.col = 0;
        self.scroll_cursor_center();
        if wrapped {
            self.message = "Wrapped to last change".to_string();
        }
        // Move partner window's cursor to the corresponding change region.
        self.diff_sync_partner_cursor(win_id, region_idx);
    }

    /// Move the partner diff window's cursor to the change region at `region_idx`.
    pub(crate) fn diff_sync_partner_cursor(&mut self, active_win: WindowId, region_idx: usize) {
        let (a, b) = match self.diff_window_pair {
            Some(pair) => pair,
            None => return,
        };
        let partner = if active_win == a {
            b
        } else if active_win == b {
            a
        } else {
            return;
        };
        let partner_regions = self.diff_change_regions(partner);
        if let Some(&(start, _)) = partner_regions.get(region_idx) {
            if let Some(w) = self.windows.get_mut(&partner) {
                w.view.cursor.line = start;
                w.view.cursor.col = 0;
            }
        }
        // Sync scroll so both windows are aligned.
        self.sync_scroll_binds();
    }

    /// Toggle hiding of unchanged sections in the diff view using folds.
    pub fn diff_toggle_hide_unchanged(&mut self) {
        if self.diff_window_pair.is_none() {
            self.message = "Not in diff mode".to_string();
            return;
        }
        self.diff_unchanged_hidden = !self.diff_unchanged_hidden;
        if self.diff_unchanged_hidden {
            self.diff_apply_folds();
            // Check if any folds were actually created.
            let has_folds = if let Some((a, b)) = self.diff_window_pair {
                let af = self
                    .windows
                    .get(&a)
                    .map(|w| !w.view.folds.is_empty())
                    .unwrap_or(false);
                let bf = self
                    .windows
                    .get(&b)
                    .map(|w| !w.view.folds.is_empty())
                    .unwrap_or(false);
                af || bf
            } else {
                false
            };
            if has_folds {
                self.message = "Unchanged sections hidden".to_string();
            } else {
                self.diff_unchanged_hidden = false;
                self.message = "All lines are near changes — nothing to hide".to_string();
            }
        } else {
            // Open all folds on both diff windows.
            if let Some((a, b)) = self.diff_window_pair {
                if let Some(w) = self.windows.get_mut(&a) {
                    w.view.open_all_folds();
                }
                if let Some(w) = self.windows.get_mut(&b) {
                    w.view.open_all_folds();
                }
            }
            self.message = "Unchanged sections visible".to_string();
        }
    }

    /// Apply folds to hide unchanged sections in both diff windows.
    ///
    /// Uses the aligned diff sequences (same length on both sides) so that
    /// fold regions correspond correctly even when the two buffers have
    /// different line counts due to insertions/deletions.
    pub(crate) fn diff_apply_folds(&mut self) {
        let (a_win, b_win) = match self.diff_window_pair {
            Some(pair) => pair,
            None => return,
        };
        let aligned_a = self.diff_aligned.get(&a_win).cloned().unwrap_or_default();
        let aligned_b = self.diff_aligned.get(&b_win).cloned().unwrap_or_default();
        let a_results = self.diff_results.get(&a_win).cloned().unwrap_or_default();
        let b_results = self.diff_results.get(&b_win).cloned().unwrap_or_default();
        let visual_rows = aligned_a.len().max(aligned_b.len());
        if visual_rows == 0 {
            return;
        }

        // Build a per-visual-row "changed" flag: true if either side has a
        // non-Same line or a padding filler at that row.
        let mut changed = vec![false; visual_rows];
        for (row, entry) in aligned_a.iter().enumerate() {
            match entry.source_line {
                Some(line) if line < a_results.len() && a_results[line] != DiffLine::Same => {
                    changed[row] = true;
                }
                None => changed[row] = true, // padding
                _ => {}
            }
        }
        for (row, entry) in aligned_b.iter().enumerate() {
            match entry.source_line {
                Some(line) if line < b_results.len() && b_results[line] != DiffLine::Same => {
                    changed[row] = true;
                }
                None => changed[row] = true,
                _ => {}
            }
        }

        // Mark context lines around changes as visible.
        let mut visible = vec![false; visual_rows];
        for (row, &is_changed) in changed.iter().enumerate() {
            if is_changed {
                let ctx_start = row.saturating_sub(DIFF_CONTEXT_LINES);
                let ctx_end = (row + DIFF_CONTEXT_LINES).min(visual_rows - 1);
                for v in visible.iter_mut().take(ctx_end + 1).skip(ctx_start) {
                    *v = true;
                }
            }
        }

        if visible.iter().all(|&v| v) {
            return; // nothing to fold
        }

        // For each window, translate visible visual rows to buffer lines,
        // then fold the buffer lines that are not visible.
        for (win_id, aligned) in [(a_win, &aligned_a), (b_win, &aligned_b)] {
            let buf_lines = if let Some(w) = self.windows.get(&win_id) {
                self.buffer_manager
                    .get(w.buffer_id)
                    .map(|bs| bs.buffer.len_lines())
                    .unwrap_or(0)
            } else {
                continue;
            };
            if buf_lines == 0 {
                continue;
            }

            // Mark which buffer lines should be visible.
            let mut buf_visible = vec![false; buf_lines];
            for (row, entry) in aligned.iter().enumerate() {
                if row < visible.len() && visible[row] {
                    if let Some(line) = entry.source_line {
                        if line < buf_lines {
                            buf_visible[line] = true;
                        }
                    }
                }
            }

            // Collect contiguous invisible runs as fold regions.
            let mut folds = Vec::new();
            let mut i = 0;
            while i < buf_lines {
                if !buf_visible[i] {
                    let start = i;
                    while i < buf_lines && !buf_visible[i] {
                        i += 1;
                    }
                    folds.push((start, i - 1));
                } else {
                    i += 1;
                }
            }

            if let Some(w) = self.windows.get_mut(&win_id) {
                w.view.open_all_folds();
                for (start, end) in folds {
                    w.view.close_fold(start, end);
                }
            }
        }
    }

    /// Helper: extract change regions from a diff results array (no self needed).
    #[allow(dead_code)]
    pub(crate) fn change_regions_from_results(results: &[DiffLine]) -> Vec<(usize, usize)> {
        let mut regions = Vec::new();
        let mut i = 0;
        while i < results.len() {
            if results[i] != DiffLine::Same {
                let start = i;
                while i < results.len() && results[i] != DiffLine::Same {
                    i += 1;
                }
                regions.push((start, i - 1));
            } else {
                i += 1;
            }
        }
        regions
    }

    /// Returns `(current_1based, total)` for the diff toolbar label, or `None`
    /// if not in diff view or no changes exist.
    /// Count unified diff hunks by walking both windows' diff results in
    /// parallel.  A hunk is any contiguous region where at least one side
    /// has non-Same lines.  Returns the hunk regions for the *active* window
    /// alongside the unified total.
    pub fn diff_unified_regions(&self) -> (Vec<(usize, usize)>, usize) {
        let (a_win, b_win) = match self.diff_window_pair {
            Some(pair) => pair,
            None => return (vec![], 0),
        };
        let active = self.active_window_id();
        let other = if active == a_win { b_win } else { a_win };

        let active_regions = self.diff_change_regions(active);
        let other_regions = self.diff_change_regions(other);

        // Build a unified count by merging both sides' regions based on
        // matching Same-line positions.  The two sides have the same number
        // of Same lines (by construction of the diff), so we use the Same
        // lines as synchronisation points.
        //
        // Simple approach: use the side with more regions as the total.
        // This works because each logical change produces a region on at
        // least one side, and the side with more regions has the correct
        // count (the other side just merges adjacent hunks due to missing
        // Removed/Added lines).
        let total = active_regions.len().max(other_regions.len());
        (active_regions, total)
    }

    pub fn diff_current_change_index(&self) -> Option<(usize, usize)> {
        let (regions, total) = self.diff_unified_regions();
        if total == 0 {
            return None;
        }
        let cur = self.view().cursor.line;
        // Find which region the cursor is in or closest after.
        for (i, &(start, end)) in regions.iter().enumerate() {
            if cur >= start && cur <= end {
                return Some((i + 1, total));
            }
            if start > cur {
                return Some((i + 1, total));
            }
        }
        // Cursor is past all regions on the active side — show total.
        Some((total, total))
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
        self.buffer_manager
            .apply_language_map(buffer_id, &self.settings.language_map);

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
        self.lsp_did_open(buffer_id);
        // Watch the file for external changes
        self.watch_file(path);
        Ok(())
    }
}

// ─── File watching ──────────────────────────────────────────────────────

impl Engine {
    /// Initialize the file watcher. Called once from `Engine::new()`.
    pub(crate) fn init_file_watcher(&mut self) {
        use notify::{RecommendedWatcher, Watcher};

        let (tx, rx) = std::sync::mpsc::channel();
        match RecommendedWatcher::new(tx, notify::Config::default()) {
            Ok(watcher) => {
                self.file_watcher = Some(watcher);
                self.file_watcher_rx = Some(rx);
            }
            Err(_) => {
                // File watching unavailable — not fatal
            }
        }
    }

    /// Add a file to the watch list.
    fn watch_file(&mut self, path: &Path) {
        use notify::{RecursiveMode, Watcher};

        if let Some(ref mut watcher) = self.file_watcher {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            let _ = watcher.watch(&canonical, RecursiveMode::NonRecursive);
        }
    }

    /// Remove a file from the watch list.
    #[allow(dead_code)]
    pub(crate) fn unwatch_file(&mut self, path: &Path) {
        use notify::Watcher;

        if let Some(ref mut watcher) = self.file_watcher {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            let _ = watcher.unwatch(&canonical);
        }
    }

    /// Re-read the current git branch if enough time has passed since the last
    /// check. Catches external branch changes (e.g. `git checkout` in a shell).
    /// Returns `true` if the branch changed (so the caller can redraw).
    /// Called from the backend's tick function.
    pub fn tick_git_branch(&mut self) -> bool {
        let now = std::time::Instant::now();
        let should_check = match self.last_git_branch_check {
            None => true,
            Some(prev) => now.duration_since(prev) >= std::time::Duration::from_secs(2),
        };
        if !should_check {
            return false;
        }
        self.last_git_branch_check = Some(now);
        let dir = self.git_dir();
        let fresh = crate::core::git::current_branch(&dir);
        if fresh != self.git_branch {
            self.git_branch = fresh;
            true
        } else {
            false
        }
    }

    /// Poll the file watcher for external modifications and show reload dialogs.
    /// Called from the backend's tick function.
    pub fn tick_file_watcher(&mut self) {
        use notify::EventKind;

        let Some(ref rx) = self.file_watcher_rx else {
            return;
        };

        let mut modified_paths: Vec<PathBuf> = Vec::new();
        while let Ok(Ok(event)) = rx.try_recv() {
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                for path in event.paths {
                    let canonical = path.canonicalize().unwrap_or(path);
                    if !self.file_watcher_pending.contains(&canonical) {
                        modified_paths.push(canonical);
                    }
                }
            }
        }

        for path in modified_paths {
            // Only prompt for files we have open
            let has_buffer = self.buffer_manager.list().iter().any(|&bid| {
                self.buffer_manager
                    .get(bid)
                    .and_then(|s| s.file_path.as_deref())
                    .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
                    == Some(path.clone())
            });

            if !has_buffer {
                continue;
            }

            // Check if the buffer is dirty — if so, show a dialog; if not, auto-reload
            let is_dirty = self.buffer_manager.list().iter().any(|&bid| {
                self.buffer_manager
                    .get(bid)
                    .filter(|s| {
                        s.file_path
                            .as_deref()
                            .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
                            == Some(path.clone())
                    })
                    .is_some_and(|s| s.dirty)
            });

            if is_dirty {
                // Mark as pending so we don't show multiple dialogs
                self.file_watcher_pending.insert(path.clone());
                let display = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());
                self.show_dialog(
                    "file_changed",
                    "File Changed",
                    vec![format!(
                        "\"{}\" has been changed outside the editor.",
                        display
                    )],
                    vec![
                        DialogButton {
                            label: "Reload".to_string(),
                            action: format!("file_reload:{}", path.display()),
                            hotkey: 'r',
                        },
                        DialogButton {
                            label: "Keep".to_string(),
                            action: format!("file_keep:{}", path.display()),
                            hotkey: 'k',
                        },
                    ],
                );
            } else {
                // Auto-reload non-dirty buffers
                self.reload_file_from_disk(&path);
            }
        }
    }

    /// Reload a buffer's content from disk.
    fn reload_file_from_disk(&mut self, path: &Path) {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let bid = self.buffer_manager.list().iter().find_map(|&bid| {
            let s = self.buffer_manager.get(bid)?;
            let fp = s.file_path.as_deref()?;
            if fp.canonicalize().unwrap_or_else(|_| fp.to_path_buf()) == canonical {
                Some(bid)
            } else {
                None
            }
        });

        if let Some(bid) = bid {
            if let Ok(content) = std::fs::read_to_string(&canonical) {
                if let Some(state) = self.buffer_manager.get_mut(bid) {
                    state.buffer = Buffer::from_text(bid, &content);
                    state.dirty = false;
                    state.update_syntax();
                }
                self.refresh_git_diff(bid);
                let display = canonical
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| canonical.to_string_lossy().to_string());
                self.message = format!("\"{}\" reloaded", display);
            }
        }
        self.file_watcher_pending.remove(&canonical);
    }

    /// Handle file watcher dialog responses.
    pub(crate) fn handle_file_watcher_action(&mut self, action: &str) {
        if let Some(path_str) = action.strip_prefix("file_reload:") {
            let path = PathBuf::from(path_str);
            self.reload_file_from_disk(&path);
        } else if let Some(path_str) = action.strip_prefix("file_keep:") {
            let path = PathBuf::from(path_str);
            let canonical = path.canonicalize().unwrap_or(path);
            self.file_watcher_pending.remove(&canonical);
        }
    }
}

// ─── Additional methods (extracted from mod.rs) ─────────────────────────

impl Engine {
    // ─── Workspace / Open Folder ──────────────────────────────────────────────

    /// Open a folder as the new working directory.  Clears all buffers/tabs,
    /// resets the explorer root, and loads any per-project session state.
    pub fn open_folder(&mut self, path: &Path) {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Save current per-workspace session before switching
        if let Some(ref root) = self.workspace_root.clone() {
            self.save_session_for_workspace(root);
        }

        // Restore user settings baseline before applying any new folder overlay
        if let Some(base) = self.base_settings.take() {
            self.settings = *base;
        }

        // Check if the new folder has a per-folder settings file to apply as overlay
        let folder_settings_path = canonical.join(".vimcode").join("settings.json");
        if folder_settings_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&folder_settings_path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(obj) = json.as_object() {
                        // Save baseline before overlay
                        self.base_settings = Some(Box::new(self.settings.clone()));
                        for (key, value) in obj {
                            let arg = match value {
                                serde_json::Value::Bool(b) => {
                                    if *b {
                                        key.clone()
                                    } else {
                                        format!("no{}", key)
                                    }
                                }
                                serde_json::Value::Number(n) => format!("{}={}", key, n),
                                serde_json::Value::String(s) => format!("{}={}", key, s),
                                _ => continue,
                            };
                            self.settings.parse_set_option(&arg).ok();
                        }
                    }
                }
            }
        }

        // Delete swap files for all current buffers before discarding them.
        self.cleanup_all_swaps();

        // Clear all existing buffers and tabs, reset to single empty window
        self.buffer_manager = crate::core::buffer_manager::BufferManager::new();
        let buffer_id = self.buffer_manager.create();
        let window_id = crate::core::window::WindowId(self.next_window_id);
        self.next_window_id += 1;
        let window = crate::core::window::Window::new(window_id, buffer_id);
        self.windows.clear();
        self.windows.insert(window_id, window);
        let tab = crate::core::tab::Tab::new(crate::core::tab::TabId(self.next_tab_id), window_id);
        self.next_tab_id += 1;
        self.editor_groups.clear();
        let gid = GroupId(0);
        self.editor_groups.insert(gid, EditorGroup::new(tab));
        self.active_group = gid;
        self.group_layout = GroupLayout::leaf(gid);
        self.next_group_id = 1;
        self.mode = Mode::Normal;

        // Update cwd + workspace root + process working directory
        self.cwd = canonical.clone();
        self.workspace_root = Some(canonical.clone());
        let _ = std::env::set_current_dir(&canonical);

        // Update git branch
        self.git_branch = git::current_branch(&canonical);

        // Load per-project session (restores open files + positions)
        let ws_session = SessionState::load_for_workspace(&canonical);
        // Restore open files from session
        let open_files: Vec<PathBuf> = ws_session.open_files.clone();
        let active_file = ws_session.active_file.clone();
        // Merge relevant session fields
        self.session.file_positions = ws_session.file_positions;
        // Add to recent workspaces in global session
        self.session.add_recent_workspace(&canonical);

        // Re-open session files
        for fp in &open_files {
            self.open_file_in_tab(fp);
        }
        // Focus the previously active file
        if let Some(ref af) = active_file {
            self.open_file_in_tab(af);
        }

        self.message = format!("Opened folder: {}", canonical.display());
    }

    /// Parse and load a `.vimcode-workspace` JSON file.
    pub fn open_workspace(&mut self, ws_path: &Path) {
        let content = match std::fs::read_to_string(ws_path) {
            Ok(c) => c,
            Err(e) => {
                self.message = format!("Cannot read workspace: {}", e);
                return;
            }
        };
        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                self.message = format!("Invalid workspace JSON: {}", e);
                return;
            }
        };

        // Resolve folder path relative to workspace file
        let ws_dir = ws_path.parent().unwrap_or(Path::new("."));
        let folder_rel = json
            .get("folders")
            .and_then(|f| f.as_array())
            .and_then(|a| a.first())
            .and_then(|e| e.get("path"))
            .and_then(|p| p.as_str())
            .unwrap_or(".");
        let folder_path = ws_dir.join(folder_rel);
        self.workspace_file = Some(ws_path.to_path_buf());

        // Apply any settings overrides from workspace
        if let Some(settings_obj) = json.get("settings").and_then(|s| s.as_object()) {
            // Save baseline settings before applying workspace overlay (once only)
            if self.base_settings.is_none() {
                self.base_settings = Some(Box::new(self.settings.clone()));
            }
            for (key, value) in settings_obj {
                let arg = match value {
                    serde_json::Value::Bool(b) => {
                        if *b {
                            key.clone()
                        } else {
                            format!("no{}", key)
                        }
                    }
                    serde_json::Value::Number(n) => format!("{}={}", key, n),
                    serde_json::Value::String(s) => format!("{}={}", key, s),
                    _ => continue,
                };
                self.settings.parse_set_option(&arg).ok();
            }
        }

        self.open_folder(&folder_path);
        self.message = format!("Workspace loaded: {}", ws_path.display());
    }

    /// Write a `.vimcode-workspace` file at the given path with the current folder.
    /// Open or create a workspace in the directory of the currently active file.
    /// If a `.vimcode-workspace` file already exists there, open it; otherwise
    /// create one and then open the folder.
    pub fn open_workspace_from_file(&mut self) {
        let buf_id = self.active_window().buffer_id;
        let dir = self
            .buffer_manager
            .get(buf_id)
            .and_then(|bs| bs.file_path.as_ref())
            .and_then(|fp| fp.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.cwd.clone());
        let ws_path = dir.join(".vimcode-workspace");
        if ws_path.exists() {
            self.open_workspace(&ws_path);
        } else {
            self.save_workspace_as(&ws_path);
            self.open_folder(&dir);
        }
    }

    pub fn save_workspace_as(&mut self, ws_path: &Path) {
        let folder_path = if let Some(parent) = ws_path.parent() {
            // Make folder path relative to workspace file location
            let canonical_cwd = self.cwd.canonicalize().unwrap_or_else(|_| self.cwd.clone());
            let canonical_parent = parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf());
            if canonical_cwd == canonical_parent {
                ".".to_string()
            } else {
                canonical_cwd.to_string_lossy().into_owned()
            }
        } else {
            ".".to_string()
        };

        let ws = serde_json::json!({
            "version": 1,
            "folders": [{"path": folder_path}],
            "settings": {}
        });
        match std::fs::write(
            ws_path,
            serde_json::to_string_pretty(&ws).unwrap_or_default(),
        ) {
            Ok(()) => {
                self.workspace_file = Some(ws_path.to_path_buf());
                self.message = format!("Workspace saved: {}", ws_path.display());
            }
            Err(e) => {
                self.message = format!("Cannot save workspace: {}", e);
            }
        }
    }

    /// Save per-workspace session state (open files, cursor positions).
    pub fn save_session_for_workspace(&self, root: &Path) {
        let mut ws_session = SessionState::default();

        // Collect open file paths per group (by iterating each group's tabs).
        let files_for_group = |group: &EditorGroup| -> Vec<PathBuf> {
            let mut files: Vec<PathBuf> = Vec::new();
            for tab in &group.tabs {
                if let Some(window) = self.windows.get(&tab.active_window) {
                    if let Some(bs) = self.buffer_manager.get(window.buffer_id) {
                        if let Some(ref fp) = bs.file_path {
                            if !files.contains(fp) {
                                files.push(fp.clone());
                            }
                        }
                    }
                }
            }
            files
        };

        let group_ids = self.group_layout.group_ids();
        if let Some(gid) = group_ids.first() {
            if let Some(group) = self.editor_groups.get(gid) {
                ws_session.open_files = files_for_group(group);
            }
        }
        if group_ids.len() >= 2 {
            if let Some(group) = self.editor_groups.get(&group_ids[1]) {
                ws_session.open_files_group1 = files_for_group(group);
            }
        }
        ws_session.active_file = self.file_path().cloned();
        ws_session.file_positions = self.session.file_positions.clone();
        // Save active_group as index position in leaf order for backward compat
        ws_session.active_group = group_ids
            .iter()
            .position(|&id| id == self.active_group)
            .unwrap_or(0);
        // For backward-compat, extract direction and ratio from root split
        // (old format only supported a single split).
        if let GroupLayout::Split {
            direction, ratio, ..
        } = &self.group_layout
        {
            ws_session.group_split_direction = match direction {
                SplitDirection::Vertical => 0,
                SplitDirection::Horizontal => 1,
            };
            ws_session.group_split_ratio = *ratio;
        }
        // Save the full recursive tree layout (new format).
        ws_session.group_layout = Some(self.build_session_group_layout(&self.group_layout));
        ws_session.save_for_workspace(root).ok();
    }

    /// Recursively convert the engine's GroupLayout tree into a SessionGroupLayout
    /// for serialization, collecting each leaf group's open file paths.
    pub(crate) fn build_session_group_layout(&self, layout: &GroupLayout) -> SessionGroupLayout {
        match layout {
            GroupLayout::Leaf(gid) => {
                let files = self
                    .editor_groups
                    .get(gid)
                    .map(|group| {
                        let mut files: Vec<PathBuf> = Vec::new();
                        for tab in &group.tabs {
                            if let Some(window) = self.windows.get(&tab.active_window) {
                                if let Some(bs) = self.buffer_manager.get(window.buffer_id) {
                                    if let Some(ref fp) = bs.file_path {
                                        if !files.contains(fp) {
                                            files.push(fp.clone());
                                        }
                                    }
                                }
                            }
                        }
                        files
                    })
                    .unwrap_or_default();
                SessionGroupLayout::Leaf { files }
            }
            GroupLayout::Split {
                direction,
                ratio,
                first,
                second,
            } => SessionGroupLayout::Split {
                direction: match direction {
                    SplitDirection::Vertical => 0,
                    SplitDirection::Horizontal => 1,
                },
                ratio: *ratio,
                first: Box::new(self.build_session_group_layout(first)),
                second: Box::new(self.build_session_group_layout(second)),
            },
        }
    }

    /// Recursively restore groups from a SessionGroupLayout tree.
    /// Returns the reconstructed GroupLayout tree.
    pub(crate) fn restore_session_group_layout(
        &mut self,
        session_layout: &SessionGroupLayout,
    ) -> GroupLayout {
        match session_layout {
            SessionGroupLayout::Leaf { files } => {
                let gid = self.new_group_id();
                // Create the group with the first file (or a scratch tab if no files).
                let valid: Vec<&PathBuf> = files.iter().filter(|p| p.exists()).collect();
                if valid.is_empty() {
                    // Empty group: create a fresh scratch buffer (don't rely on active_group chain).
                    let wid = self.new_window_id();
                    let buf_id = self.buffer_manager.create();
                    let w = Window::new(wid, buf_id);
                    self.windows.insert(wid, w);
                    let tid = self.new_tab_id();
                    let tab = Tab::new(tid, wid);
                    self.editor_groups.insert(gid, EditorGroup::new(tab));
                } else {
                    // Open files in this group's tabs.
                    let mut first = true;
                    for path in &valid {
                        let wid = self.new_window_id();
                        let buf_id = self
                            .buffer_manager
                            .open_file(path)
                            .unwrap_or_else(|_| self.buffer_manager.create());
                        let mut w = Window::new(wid, buf_id);
                        let view = self.restore_file_position(buf_id);
                        w.view = view;
                        self.windows.insert(wid, w);
                        let tid = self.new_tab_id();
                        let tab = Tab::new(tid, wid);
                        if first {
                            self.editor_groups.insert(gid, EditorGroup::new(tab));
                            first = false;
                        } else if let Some(group) = self.editor_groups.get_mut(&gid) {
                            group.tabs.push(tab);
                        }
                    }
                }
                GroupLayout::Leaf(gid)
            }
            SessionGroupLayout::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let dir = if *direction == 1 {
                    SplitDirection::Horizontal
                } else {
                    SplitDirection::Vertical
                };
                let first_layout = self.restore_session_group_layout(first);
                let second_layout = self.restore_session_group_layout(second);
                GroupLayout::Split {
                    direction: dir,
                    ratio: *ratio,
                    first: Box::new(first_layout),
                    second: Box::new(second_layout),
                }
            }
        }
    }
}
