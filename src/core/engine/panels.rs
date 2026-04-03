use super::*;

impl Engine {
    // ─── Dialog system ─────────────────────────────────────────────────

    /// Show a modal dialog.
    pub fn show_dialog(
        &mut self,
        tag: &str,
        title: &str,
        body: Vec<String>,
        buttons: Vec<DialogButton>,
    ) {
        self.dialog = Some(Dialog {
            title: title.to_string(),
            body,
            buttons,
            selected: 0,
            tag: tag.to_string(),
            input: None,
        });
    }

    /// Convenience: show an error dialog with a single OK button.
    #[allow(dead_code)]
    pub fn show_error_dialog(&mut self, title: &str, message: &str) {
        self.show_dialog(
            "error",
            title,
            vec![message.to_string()],
            vec![DialogButton {
                label: "OK".into(),
                hotkey: 'o',
                action: "ok".into(),
            }],
        );
    }

    /// Click a dialog button by index.  Returns the `EngineAction` from
    /// processing the dialog result, or `None` if the index is out of range.
    pub fn dialog_click_button(&mut self, idx: usize) -> EngineAction {
        let (tag, action, input_value) = {
            let dialog = match self.dialog.as_ref() {
                Some(d) => d,
                None => return EngineAction::None,
            };
            let btn = match dialog.buttons.get(idx) {
                Some(b) => b,
                None => return EngineAction::None,
            };
            let iv = dialog.input.as_ref().map(|i| i.value.clone());
            (dialog.tag.clone(), btn.action.clone(), iv)
        };
        self.dialog = None;
        self.process_dialog_result(&tag, &action, input_value.as_deref())
    }

    /// Handle a key press when a dialog is open.
    /// Returns `Some((tag, action))` when the dialog is dismissed, `None` to keep it open.
    pub(crate) fn handle_dialog_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
    ) -> Option<(String, String)> {
        let dialog = self.dialog.as_mut()?;
        let has_input = dialog.input.is_some();

        // TUI sends key_name="" with the char in unicode; GTK sends key_name="r".
        let effective = if !key_name.is_empty() {
            key_name.to_string()
        } else {
            unicode.map(|c| c.to_string()).unwrap_or_default()
        };

        match effective.as_str() {
            "Escape" => {
                let tag = dialog.tag.clone();
                self.dialog = None;
                Some((tag, "cancel".to_string()))
            }
            "Return" => {
                let tag = dialog.tag.clone();
                let action = dialog
                    .buttons
                    .get(dialog.selected)
                    .map(|b| b.action.clone())
                    .unwrap_or_else(|| "cancel".to_string());
                self.dialog = None;
                Some((tag, action))
            }
            "BackSpace" if has_input => {
                if let Some(ref mut input) = dialog.input {
                    input.value.pop();
                }
                None
            }
            "Tab" | "Shift_Tab" => {
                let len = dialog.buttons.len();
                if len > 0 {
                    if effective == "Shift_Tab" {
                        dialog.selected = if dialog.selected > 0 {
                            dialog.selected - 1
                        } else {
                            len - 1
                        };
                    } else {
                        dialog.selected = (dialog.selected + 1) % len;
                    }
                }
                None
            }
            "Left" | "h" | "Up" | "k" if !has_input => {
                let len = dialog.buttons.len();
                if len > 0 {
                    dialog.selected = if dialog.selected > 0 {
                        dialog.selected - 1
                    } else {
                        len - 1
                    };
                }
                None
            }
            "Right" | "l" | "Down" | "j" if !has_input => {
                let len = dialog.buttons.len();
                if len > 0 {
                    dialog.selected = (dialog.selected + 1) % len;
                }
                None
            }
            _ => {
                // When dialog has a text input, printable chars go there.
                if has_input {
                    if let Some(ch) = unicode {
                        if let Some(ref mut input) = dialog.input {
                            input.value.push(ch);
                        }
                    }
                    return None;
                }
                // Check hotkeys (case-insensitive).
                let ch = effective
                    .chars()
                    .next()
                    .unwrap_or('\0')
                    .to_ascii_lowercase();
                for btn in &dialog.buttons {
                    if btn.hotkey == ch {
                        let tag = dialog.tag.clone();
                        let action = btn.action.clone();
                        self.dialog = None;
                        return Some((tag, action));
                    }
                }
                None
            }
        }
    }

    /// Dispatch a dialog result to the appropriate handler.
    pub(crate) fn process_dialog_result(
        &mut self,
        tag: &str,
        action: &str,
        input_value: Option<&str>,
    ) -> EngineAction {
        match tag {
            "swap_recovery" => self.process_swap_dialog_action(action),
            "confirm_move" => {
                if action == "yes" {
                    if let Some((src, dest)) = self.pending_move.take() {
                        match self.move_file(&src, &dest) {
                            Ok(()) => {
                                let name = src
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                self.message = format!("Moved '{}' to '{}'", name, dest.display());
                                self.explorer_needs_refresh = true;
                            }
                            Err(e) => {
                                self.message = e;
                            }
                        }
                    }
                } else {
                    self.pending_move = None;
                }
                EngineAction::None
            }
            "confirm_delete" => {
                if action == "delete" {
                    if let Some(path) = self.pending_delete.take() {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let is_dir = path.is_dir();
                        let item_type = if is_dir { "folder" } else { "file" };
                        if !path.exists() {
                            self.message = format!("'{}' does not exist", name);
                        } else {
                            let result = if is_dir {
                                std::fs::remove_dir_all(&path)
                            } else {
                                std::fs::remove_file(&path)
                            };
                            match result {
                                Ok(()) => {
                                    self.message = format!("Deleted {}: '{}'", item_type, name);
                                    // If deleted file was open, close its buffer
                                    if !is_dir {
                                        let path_str = path.to_string_lossy();
                                        if let Some(buffer_id) =
                                            self.buffer_manager.find_by_path(&path_str)
                                        {
                                            let _ = self.delete_buffer(buffer_id, true);
                                        }
                                    }
                                    self.explorer_needs_refresh = true;
                                }
                                Err(e) => {
                                    self.message = format!("Error deleting: {}", e);
                                }
                            }
                        }
                    }
                } else {
                    self.pending_delete = None;
                }
                EngineAction::None
            }
            "move_file_input" => {
                if action == "move" {
                    if let Some((src, _)) = self.pending_move.take() {
                        let dest_str = input_value.unwrap_or("").trim();
                        if !dest_str.is_empty() {
                            let dest = if std::path::Path::new(dest_str).is_absolute() {
                                PathBuf::from(dest_str)
                            } else {
                                self.cwd.join(dest_str)
                            };
                            match self.move_file(&src, &dest) {
                                Ok(()) => {
                                    let name = src
                                        .file_name()
                                        .map(|n| n.to_string_lossy().to_string())
                                        .unwrap_or_default();
                                    let final_dest = if dest.is_dir() {
                                        dest.join(src.file_name().unwrap_or_default())
                                    } else {
                                        dest.clone()
                                    };
                                    self.message =
                                        format!("Moved '{}' to '{}'", name, final_dest.display());
                                    self.explorer_needs_refresh = true;
                                }
                                Err(e) => {
                                    self.message = e;
                                }
                            }
                        }
                    }
                } else {
                    self.pending_move = None;
                }
                EngineAction::None
            }
            "ext_remove" => {
                if let Some(name) = self.pending_ext_remove.take() {
                    match action {
                        "remove" | "keep_tools" => self.ext_remove(&name, false),
                        "remove_all" => self.ext_remove(&name, true),
                        _ => {} // cancel — do nothing
                    }
                }
                EngineAction::None
            }
            "ssh_passphrase" => {
                if action == "ok" {
                    let passphrase = input_value.unwrap_or("");
                    if let Some(op) = self.pending_git_remote_op.take() {
                        let dir =
                            git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
                        let result = match op.as_str() {
                            "push" => git::push_with_passphrase(&dir, passphrase),
                            "pull" => git::pull_with_passphrase(&dir, passphrase),
                            "fetch" => git::fetch_with_passphrase(&dir, passphrase),
                            _ => Err(format!("unknown git op: {}", op)),
                        };
                        match result {
                            Ok(msg) => {
                                let default_msg = match op.as_str() {
                                    "push" => "Pushed.",
                                    "pull" => "Already up to date.",
                                    "fetch" => "Fetched.",
                                    _ => "Done.",
                                };
                                self.message = if msg.is_empty() {
                                    default_msg.to_string()
                                } else {
                                    msg
                                };
                            }
                            Err(e) => self.message = format!("{}: {}", op, e),
                        }
                        self.sc_refresh();
                    }
                } else {
                    self.pending_git_remote_op = None;
                }
                EngineAction::None
            }
            tag if tag.starts_with("open_ext_url:") => {
                // Extension-provided link — user confirmed "Open".
                if action == "open" {
                    let url = &tag["open_ext_url:".len()..];
                    if is_safe_url(url) {
                        return EngineAction::OpenUrl(url.to_string());
                    }
                }
                EngineAction::None
            }
            "code_actions" => {
                if let Some(idx_str) = action.strip_prefix("apply_") {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        if let Some(ca) = self.pending_code_action_choices.get(idx).cloned() {
                            self.pending_code_action_choices.clear();
                            if let Some(edit) = ca.edit {
                                self.apply_workspace_edit(edit);
                                self.message = format!("Applied: {}", ca.title);
                            } else {
                                self.message = format!("No edit available for '{}'", ca.title);
                            }
                        }
                    }
                } else {
                    self.pending_code_action_choices.clear();
                }
                EngineAction::None
            }
            _ => EngineAction::None,
        }
    }

    /// Mark the active buffer as needing a swap write.
    pub fn swap_mark_dirty(&mut self) {
        if self.settings.swap_file {
            let id = self.active_buffer_id();
            self.swap_write_needed.insert(id);
        }
    }

    /// Periodically write swap files for dirty buffers.
    /// Called from both GTK and TUI event loops (~20 Hz).  The method only
    /// does real work when `updatetime` milliseconds have elapsed.
    pub fn tick_swap_files(&mut self) {
        if !self.settings.swap_file || self.swap_write_needed.is_empty() {
            return;
        }
        let elapsed = self.swap_last_write.elapsed().as_millis() as u32;
        if elapsed < self.settings.updatetime {
            return;
        }
        let buf_ids: Vec<BufferId> = self.swap_write_needed.drain().collect();
        for buf_id in buf_ids {
            self.swap_create_for_buffer(buf_id);
        }
        self.swap_last_write = std::time::Instant::now();
    }

    /// Delete the swap file for a single buffer.
    pub(crate) fn swap_delete_for_buffer(&self, buf_id: BufferId) {
        let state = match self.buffer_manager.get(buf_id) {
            Some(s) => s,
            None => return,
        };
        let canonical = match &state.canonical_path {
            Some(p) => p,
            None => return,
        };
        let swap_path = crate::core::swap::swap_path_for(canonical);
        crate::core::swap::delete_swap(&swap_path);
    }

    /// Delete swap files for ALL open buffers.  Called on clean shutdown.
    pub fn cleanup_all_swaps(&self) {
        for buf_id in self.buffer_manager.list() {
            self.swap_delete_for_buffer(buf_id);
        }
    }

    /// Emergency flush: write swap files for ALL dirty buffers immediately.
    /// Called from panic handlers to preserve unsaved work before crashing.
    /// Bypasses the `updatetime` debounce and `swap_write_needed` set.
    pub fn emergency_swap_flush(&self) {
        for buf_id in self.buffer_manager.list() {
            let is_dirty = self
                .buffer_manager
                .get(buf_id)
                .map(|s| s.dirty)
                .unwrap_or(false);
            if is_dirty {
                self.swap_create_for_buffer(buf_id);
            }
        }
    }

    /// Check all open buffers for stale swap files.
    /// Called after session restore to catch any crashed sessions.
    /// Check all open buffers for stale swap files.
    /// Called after session restore and after each swap recovery dialog.
    /// Only the first stale swap triggers a recovery dialog — remaining
    /// buffers are left alone so the next dialog dismissal re-scans them.
    pub fn swap_check_all_buffers(&mut self) {
        if !self.settings.swap_file {
            return;
        }
        let buf_ids = self.buffer_manager.list();
        for buf_id in buf_ids {
            if self.pending_swap_recovery.is_some() {
                // Already showing a recovery dialog — don't touch remaining
                // buffers so their stale swaps survive for the next re-scan.
                break;
            }
            self.swap_check_on_open(buf_id);
        }
        // Also scan the swap directory for orphaned swaps (files that
        // aren't in the restored session).
        self.swap_scan_stale();
    }

    /// Re-check open buffers for stale swaps after a recovery dialog is dismissed.
    /// Unlike `swap_check_all_buffers`, this does NOT scan for orphaned swaps.
    pub(crate) fn swap_recheck_open_buffers(&mut self) {
        if !self.settings.swap_file {
            return;
        }
        let buf_ids = self.buffer_manager.list();
        for buf_id in buf_ids {
            if self.pending_swap_recovery.is_some() {
                break;
            }
            self.swap_check_on_open(buf_id);
        }
    }

    /// Scan the swap directory for stale swap files with dead PIDs that
    /// don't correspond to any currently-open buffer.  Opens the first
    /// orphaned file in a new tab and offers recovery.
    pub(crate) fn swap_scan_stale(&mut self) {
        if !self.settings.swap_file || self.pending_swap_recovery.is_some() {
            return;
        }
        let stale = crate::core::swap::find_stale_swaps();
        // Collect canonical paths of all currently-open buffers.
        let open_paths: std::collections::HashSet<PathBuf> = self
            .buffer_manager
            .list()
            .into_iter()
            .filter_map(|id| {
                self.buffer_manager
                    .get(id)
                    .and_then(|s| s.canonical_path.clone())
            })
            .collect();
        for (header, swap_path) in stale {
            if open_paths.contains(&header.file_path) {
                // Already handled by swap_check_all_buffers above.
                continue;
            }
            // The file from this stale swap isn't open — open it and offer recovery.
            if !header.file_path.exists() {
                // Original file was deleted — clean up the orphaned swap.
                crate::core::swap::delete_swap(&swap_path);
                continue;
            }
            // Open the file in a new tab.  `open_file_in_tab` calls
            // `swap_check_on_open` internally, which will detect the
            // stale swap and set `pending_swap_recovery` for us.
            self.open_file_in_tab(&header.file_path);
            if self.pending_swap_recovery.is_some() {
                return;
            }
        }
    }

    /// Accept the current ghost text by inserting it at the cursor.
    pub fn ai_accept_ghost(&mut self) {
        if let Some(ghost) = self.ai_ghost_text.take() {
            if !ghost.is_empty() {
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;
                let char_count = ghost.chars().count();
                self.insert_with_undo(char_idx, &ghost);
                self.view_mut().cursor.col += char_count;
            }
        }
        self.ai_ghost_clear();
    }

    /// Show the next ghost text alternative (Alt+]).
    pub fn ai_ghost_next_alt(&mut self) {
        if self.ai_ghost_alternatives.is_empty() {
            return;
        }
        self.ai_ghost_alt_idx = (self.ai_ghost_alt_idx + 1) % self.ai_ghost_alternatives.len();
        self.ai_ghost_text = Some(self.ai_ghost_alternatives[self.ai_ghost_alt_idx].clone());
    }

    /// Show the previous ghost text alternative (Alt+[).
    pub fn ai_ghost_prev_alt(&mut self) {
        if self.ai_ghost_alternatives.is_empty() {
            return;
        }
        let len = self.ai_ghost_alternatives.len();
        self.ai_ghost_alt_idx = (self.ai_ghost_alt_idx + len - 1) % len;
        self.ai_ghost_text = Some(self.ai_ghost_alternatives[self.ai_ghost_alt_idx].clone());
    }

    /// Re-send didOpen for all open buffers that match a given language ID.
    /// Called after a new server is detected/started mid-session.
    pub(crate) fn lsp_reopen_buffers_for_language(&mut self, lang_id: &str) {
        let buffers: Vec<(PathBuf, String)> = self
            .buffer_manager
            .list()
            .iter()
            .filter_map(|&bid| {
                let s = self.buffer_manager.get(bid)?;
                if s.lsp_language_id.as_deref() == Some(lang_id) {
                    let path = s.file_path.as_ref()?.clone();
                    Some((path, s.buffer.to_string()))
                } else {
                    None
                }
            })
            .collect();
        if let Some(mgr) = &mut self.lsp_manager {
            for (path, text) in buffers {
                let _ = mgr.notify_did_open(&path, &text);
            }
        }
    }

    /// Notify LSP that a file was saved.
    pub(crate) fn lsp_did_save(&mut self, buffer_id: BufferId) {
        if !self.settings.lsp_enabled {
            return;
        }
        let (path, text) = {
            let state = match self.buffer_manager.get(buffer_id) {
                Some(s) => s,
                None => return,
            };
            let path = match &state.file_path {
                Some(p) => p.clone(),
                None => return,
            };
            if state.lsp_language_id.is_none() {
                return;
            }
            (path, state.buffer.to_string())
        };
        if let Some(mgr) = &mut self.lsp_manager {
            mgr.notify_did_save(&path, &text);
        }
        // Also flush any pending didChange
        self.lsp_dirty_buffers.remove(&buffer_id);
    }

    /// Notify LSP that a file was closed.
    pub(crate) fn lsp_did_close(&mut self, buffer_id: BufferId) {
        let path = self
            .buffer_manager
            .get(buffer_id)
            .and_then(|s| s.file_path.clone());
        if let Some(ref path) = path {
            if let Some(mgr) = &mut self.lsp_manager {
                mgr.notify_did_close(path);
            }
            self.lsp_diagnostics.remove(path);
        }
    }

    /// Flush any pending didChange notifications (called from UI poll loop).
    /// Request semantic tokens for a file from the LSP server.
    /// Multiple requests can be in flight simultaneously; responses are matched by request ID.
    pub fn lsp_request_semantic_tokens(&mut self, path: &Path) {
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(req_id) = mgr.request_semantic_tokens(path) {
                self.lsp_pending_semantic_tokens
                    .insert(req_id, path.to_path_buf());
            }
        }
    }

    pub fn lsp_flush_changes(&mut self) {
        if self.lsp_manager.is_none() {
            return;
        }
        let dirty: Vec<BufferId> = self.lsp_dirty_buffers.keys().copied().collect();
        for buffer_id in dirty {
            self.lsp_dirty_buffers.remove(&buffer_id);
            let (path, text) = {
                let state = match self.buffer_manager.get(buffer_id) {
                    Some(s) => s,
                    None => continue,
                };
                let path = match &state.file_path {
                    Some(p) => p.clone(),
                    None => continue,
                };
                if state.lsp_language_id.is_none() {
                    continue;
                }
                (path, state.buffer.to_string())
            };
            // Clear stale position-based data immediately — line numbers from
            // the previous buffer state would highlight/annotate wrong lines.
            if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
                state.semantic_tokens.clear();
            }
            self.lsp_diagnostics.remove(&path);
            self.lsp_code_actions.remove(&path);
            self.lsp_code_action_last_line = None;
            if let Some(mgr) = &mut self.lsp_manager {
                mgr.notify_did_change(&path, &text);
            }
            // Re-request semantic tokens after the server processes the change.
            self.lsp_request_semantic_tokens(&path);
        }
    }

    /// Poll LSP for events. Called every frame from the UI event loop.
    /// Returns true if a redraw is needed.
    pub fn poll_lsp(&mut self) -> bool {
        let events = match &mut self.lsp_manager {
            Some(mgr) => mgr.poll_events(),
            None => return false,
        };
        if events.is_empty() {
            return false;
        }

        // Pre-compute canonical paths for visible buffers (once, not per-event).
        let visible_paths: Vec<PathBuf> = self
            .windows
            .values()
            .filter_map(|w| {
                self.buffer_manager
                    .get(w.buffer_id)?
                    .file_path
                    .as_ref()
                    .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
            })
            .collect();

        let mut redraw = false;
        for event in events {
            match event {
                LspEvent::Initialized(..) => {
                    // Server is ready — re-open any already-open buffers
                    let buffers: Vec<(PathBuf, String)> = self
                        .buffer_manager
                        .list()
                        .iter()
                        .filter_map(|&bid| {
                            let s = self.buffer_manager.get(bid)?;
                            let p = s.file_path.as_ref()?.clone();
                            if s.lsp_language_id.is_some() {
                                Some((p, s.buffer.to_string()))
                            } else {
                                None
                            }
                        })
                        .collect();
                    if let Some(mgr) = &mut self.lsp_manager {
                        for (path, text) in &buffers {
                            let _ = mgr.notify_did_open(path, text);
                        }
                    }
                    // Request semantic tokens for all reopened buffers.
                    for (path, _) in &buffers {
                        self.lsp_request_semantic_tokens(path);
                    }
                }
                LspEvent::Diagnostics {
                    path, diagnostics, ..
                } => {
                    // Only redraw if diagnostics affect a currently visible buffer.
                    if !redraw && visible_paths.contains(&path) {
                        redraw = true;
                    }
                    self.lsp_diagnostics.insert(path, diagnostics);
                }
                LspEvent::CompletionResponse {
                    request_id, items, ..
                } => {
                    if self.lsp_pending_completion == Some(request_id) {
                        // Popup completion response — populate display-only popup
                        self.lsp_pending_completion = None;
                        // Only show completion if still in Insert mode (or VSCode mode).
                        // The user may have pressed Escape between the request and response.
                        let in_insert = self.mode == Mode::Insert || self.is_vscode_mode();
                        if in_insert && !items.is_empty() {
                            let (cur_prefix, _) = self.completion_prefix_at_cursor();
                            let lsp_cands: Vec<String> = items
                                .iter()
                                .filter_map(|item| {
                                    let text = item.insert_text.as_deref().unwrap_or(&item.label);
                                    text.starts_with(&cur_prefix).then(|| text.to_string())
                                })
                                .collect();
                            if !lsp_cands.is_empty() {
                                self.completion_start_col =
                                    self.view().cursor.col - cur_prefix.chars().count();
                                self.completion_candidates = lsp_cands;
                                self.completion_idx = Some(0);
                                self.completion_display_only = true;
                                redraw = true;
                            }
                        }
                    }
                    // else: stale response (request already superseded) — ignore
                }
                LspEvent::DefinitionResponse {
                    server_id,
                    locations,
                    ..
                } => {
                    if !locations.is_empty() {
                        if let Some(mgr) = self.lsp_manager.as_mut() {
                            mgr.mark_server_responded(server_id);
                        }
                    }
                    self.lsp_pending_definition = None;
                    self.message.clear();
                    if let Some(loc) = locations.first() {
                        let path = loc.path.clone();
                        let line = loc.range.start.line as usize;
                        // Open the file and jump
                        if path
                            != self
                                .buffer_manager
                                .get(self.active_buffer_id())
                                .and_then(|s| s.file_path.clone())
                                .unwrap_or_default()
                        {
                            let _ = self.open_file_with_mode(&path, OpenMode::Permanent);
                        }
                        // Jump to line/col
                        self.view_mut().cursor.line = line;
                        let line_text: String = self.buffer().content.line(line).chars().collect();
                        let col = lsp::utf16_offset_to_char(&line_text, loc.range.start.character);
                        self.view_mut().cursor.col = col;
                        self.ensure_cursor_visible();
                        redraw = true;
                    } else {
                        self.message = "No definition found".to_string();
                    }
                }
                LspEvent::HoverResponse {
                    server_id,
                    contents,
                    ..
                } => {
                    if contents.is_some() {
                        if let Some(mgr) = self.lsp_manager.as_mut() {
                            mgr.mark_server_responded(server_id);
                        }
                    }
                    self.lsp_pending_hover = None;
                    // Treat empty/whitespace-only hover as "no hover".
                    let text = contents.filter(|t| !t.trim().is_empty());
                    if let Some(text) = text {
                        self.lsp_hover_null_pos = None;
                        // Cancel "Loading..." auto-dismiss since we got real content.
                        self.editor_hover_dismiss_at = None;
                        if self.editor_hover.is_some() || self.editor_hover_has_focus {
                            // Popup already visible (keyboard hover or has diagnostics) — update it.
                            self.update_editor_hover_with_lsp(&text);
                        } else if let Some((line, col)) = self.lsp_hover_request_pos {
                            // Mouse hover: no popup yet — create one at the request position.
                            self.show_editor_hover(
                                line,
                                col,
                                &text,
                                EditorHoverSource::Lsp,
                                false,
                                true,
                            );
                        } else {
                            self.lsp_hover_text = Some(text);
                        }
                        redraw = true;
                    } else {
                        // LSP returned no hover — remember position to suppress re-requests.
                        if let Some(pos) = self.lsp_hover_request_pos.take() {
                            self.lsp_hover_null_pos = Some(pos);
                        }
                        if self.editor_hover.is_some()
                            && self
                                .editor_hover
                                .as_ref()
                                .is_some_and(|h| matches!(h.source, EditorHoverSource::Lsp))
                        {
                            // Dismiss "Loading..." popup (only if LSP-sourced)
                            self.dismiss_editor_hover();
                            redraw = true;
                        }
                    }
                }
                LspEvent::ServerExited {
                    server_id,
                    stderr,
                    was_initialized,
                } => {
                    let desc = self
                        .lsp_manager
                        .as_mut()
                        .map(|mgr| mgr.handle_server_exited(server_id))
                        .unwrap_or_else(|| format!("server {}", server_id));
                    if was_initialized {
                        self.message = format!("LSP {} exited", desc);
                    } else {
                        let snippet = stderr
                            .lines()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or("no output")
                            .trim();
                        let snippet = if snippet.len() > 100 {
                            &snippet[..100]
                        } else {
                            snippet
                        };
                        self.message = format!("LSP {} failed to start: {}", desc, snippet);
                    }
                    redraw = true;
                }
                LspEvent::RegistryLookup { lang_id, .. } => {
                    // Mason registry lookups are no longer used. Ignore stale events.
                    self.lsp_lookup_in_flight.remove(&lang_id);
                }
                LspEvent::InstallComplete {
                    lang_id,
                    success,
                    output,
                } => {
                    self.lsp_installing.remove(&lang_id);
                    // preLaunchTask completion: resume debug session after build task.
                    if let Some(task_label) = lang_id.strip_prefix("dap_task:") {
                        // Append task output to Debug Output panel.
                        for line in output.lines() {
                            self.dap_output_lines.push(line.to_string());
                        }
                        if success {
                            self.dap_output_lines.push(format!(
                                "[dap] Pre-launch task '{task_label}' completed successfully"
                            ));
                            self.dap_pre_launch_done = true;
                            // Resume the debug session with the stored language.
                            if let Some(lang) = self.dap_deferred_lang.take() {
                                self.dap_start_debug(&lang);
                            }
                        } else {
                            self.dap_output_lines
                                .push(format!("[dap] Pre-launch task '{task_label}' FAILED"));
                            self.message =
                                format!("Pre-launch task '{task_label}' failed — debug aborted");
                            self.dap_session_active = false;
                            self.debug_toolbar_visible = false;
                            self.dap_deferred_lang = None;
                        }
                        redraw = true;
                    } else if let Some(adapter_name) = lang_id.strip_prefix("dap:") {
                        if success {
                            self.message = format!(
                                "DAP adapter '{adapter_name}' installed — press F5 to debug"
                            );
                        } else {
                            let short =
                                output.lines().next().unwrap_or("unknown error").to_string();
                            self.message =
                                format!("DAP install failed for '{adapter_name}': {short}");
                        }
                        redraw = true;
                    } else if success {
                        // LSP install (from :ExtInstall): look up binary from extension manifest
                        // lang_id format is "ext:{ext_name}:lsp"
                        let ext_name = lang_id
                            .strip_prefix("ext:")
                            .and_then(|s| s.strip_suffix(":lsp"))
                            .unwrap_or(&lang_id);
                        let binary = self
                            .ext_available_manifests()
                            .into_iter()
                            .find(|m| m.name == ext_name)
                            .map(|m| m.lsp.binary.clone())
                            .unwrap_or_default();
                        if !binary.is_empty() {
                            // Register the binary so future files auto-start the server.
                            // Use the manifest's args (e.g. ["--stdio"]) so the
                            // server actually communicates correctly.
                            let manifest_args = self
                                .ext_available_manifests()
                                .into_iter()
                                .find(|m| m.name == ext_name)
                                .map(|m| m.lsp.args.clone())
                                .unwrap_or_default();
                            for lsp_lang in self
                                .ext_available_manifests()
                                .into_iter()
                                .find(|m| m.name == ext_name)
                                .map(|m| m.language_ids.clone())
                                .unwrap_or_default()
                            {
                                let config = lsp::LspServerConfig {
                                    command: binary.clone(),
                                    args: manifest_args.clone(),
                                    languages: vec![lsp_lang.clone()],
                                };
                                if let Some(mgr) = &mut self.lsp_manager {
                                    mgr.add_registry_entry(config);
                                    mgr.ensure_server_for_language(&lsp_lang);
                                }
                                self.lsp_reopen_buffers_for_language(&lsp_lang);
                            }
                            self.message = format!(
                                "LSP server for '{ext_name}' installed and started ({binary})"
                            );
                            redraw = true;
                        } else {
                            self.message = format!(
                                "LSP for '{ext_name}' installed — reopen a file to activate"
                            );
                        }
                    } else {
                        let short = output.lines().next().unwrap_or("unknown error").to_string();
                        self.message = format!("LSP install failed: {short}");
                    }
                }
                LspEvent::ReferencesResponse { locations, .. } => {
                    self.lsp_pending_references = None;
                    if locations.is_empty() {
                        self.message = "No references found".to_string();
                    } else if locations.len() == 1 {
                        // Single result — jump directly like gd
                        let loc = &locations[0];
                        let path = loc.path.clone();
                        let line = loc.range.start.line as usize;
                        if path
                            != self
                                .buffer_manager
                                .get(self.active_buffer_id())
                                .and_then(|s| s.file_path.clone())
                                .unwrap_or_default()
                        {
                            let _ = self.open_file_with_mode(&path, OpenMode::Permanent);
                        }
                        self.view_mut().cursor.line = line;
                        let line_text: String = self.buffer().content.line(line).chars().collect();
                        let col = lsp::utf16_offset_to_char(&line_text, loc.range.start.character);
                        self.view_mut().cursor.col = col;
                        self.ensure_cursor_visible();
                    } else {
                        // Multiple results — populate quickfix window
                        self.quickfix_items = locations
                            .into_iter()
                            .map(|l| ProjectMatch {
                                file: l.path,
                                line: l.range.start.line as usize,
                                col: l.range.start.character as usize,
                                line_text: String::new(),
                            })
                            .collect();
                        self.quickfix_selected = 0;
                        self.quickfix_open = true;
                        self.quickfix_has_focus = false;
                        self.message = format!("{} references found", self.quickfix_items.len());
                    }
                    redraw = true;
                }
                LspEvent::ImplementationResponse { locations, .. } => {
                    self.lsp_pending_implementation = None;
                    self.message.clear();
                    if let Some(loc) = locations.first() {
                        let path = loc.path.clone();
                        let line = loc.range.start.line as usize;
                        if path
                            != self
                                .buffer_manager
                                .get(self.active_buffer_id())
                                .and_then(|s| s.file_path.clone())
                                .unwrap_or_default()
                        {
                            let _ = self.open_file_with_mode(&path, OpenMode::Permanent);
                        }
                        self.view_mut().cursor.line = line;
                        let line_text: String = self.buffer().content.line(line).chars().collect();
                        let col = lsp::utf16_offset_to_char(&line_text, loc.range.start.character);
                        self.view_mut().cursor.col = col;
                        self.ensure_cursor_visible();
                        redraw = true;
                    } else {
                        self.message = "No implementation found".to_string();
                    }
                }
                LspEvent::TypeDefinitionResponse { locations, .. } => {
                    self.lsp_pending_type_definition = None;
                    self.message.clear();
                    if let Some(loc) = locations.first() {
                        let path = loc.path.clone();
                        let line = loc.range.start.line as usize;
                        if path
                            != self
                                .buffer_manager
                                .get(self.active_buffer_id())
                                .and_then(|s| s.file_path.clone())
                                .unwrap_or_default()
                        {
                            let _ = self.open_file_with_mode(&path, OpenMode::Permanent);
                        }
                        self.view_mut().cursor.line = line;
                        let line_text: String = self.buffer().content.line(line).chars().collect();
                        let col = lsp::utf16_offset_to_char(&line_text, loc.range.start.character);
                        self.view_mut().cursor.col = col;
                        self.ensure_cursor_visible();
                        redraw = true;
                    } else {
                        self.message = "No type definition found".to_string();
                    }
                }
                LspEvent::SignatureHelpResponse {
                    request_id,
                    label,
                    params,
                    active_param,
                    ..
                } => {
                    if self.lsp_pending_signature == Some(request_id) {
                        self.lsp_pending_signature = None;
                        if !label.is_empty() {
                            self.lsp_signature_help = Some(SignatureHelpData {
                                label,
                                params,
                                active_param,
                            });
                        }
                        redraw = true;
                    }
                }
                LspEvent::FormattingResponse {
                    request_id, edits, ..
                } => {
                    if self.lsp_pending_formatting == Some(request_id) {
                        self.lsp_pending_formatting = None;
                        let buffer_id = self.active_buffer_id();
                        let had_edits = !edits.is_empty();
                        if had_edits {
                            self.apply_lsp_edits(buffer_id, edits);
                            // Mark buffer dirty so lsp_flush_changes sends didChange
                            // and re-requests semantic tokens on the next poll tick.
                            self.lsp_dirty_buffers.insert(buffer_id, true);
                        }
                        // If this was a format-on-save, perform the actual save now.
                        if self.format_on_save_pending.take() == Some(buffer_id) {
                            let _ = self.save();
                            if self.quit_after_format_save {
                                self.quit_after_format_save = false;
                                self.format_save_quit_ready = true;
                            }
                        } else if had_edits {
                            self.message = "Buffer formatted".to_string();
                        } else {
                            self.message = "No formatting changes".to_string();
                        }
                        redraw = true;
                    }
                }
                LspEvent::RenameResponse {
                    request_id,
                    workspace_edit,
                    error_message,
                    ..
                } => {
                    if self.lsp_pending_rename == Some(request_id) {
                        self.lsp_pending_rename = None;
                        let n = workspace_edit.changes.len();
                        if n > 0 {
                            self.apply_workspace_edit(workspace_edit);
                            self.message = format!("Renamed in {n} file(s)");
                        } else if let Some(err) = error_message {
                            self.message = format!("Rename failed: {err}");
                        } else {
                            self.message = "Rename: no changes returned by server".to_string();
                        }
                        redraw = true;
                    }
                }
                LspEvent::SemanticTokensResponse {
                    server_id,
                    request_id,
                    raw_data,
                } => {
                    if let Some(path) = self.lsp_pending_semantic_tokens.remove(&request_id) {
                        // Decode using the cached legend for this server.
                        // If the legend is missing (server restart, cache miss), keep
                        // existing tokens rather than silently replacing with empty.
                        if let Some(decoded) = self
                            .lsp_manager
                            .as_ref()
                            .and_then(|mgr| mgr.semantic_legend_for_server(server_id))
                            .map(|legend| lsp::decode_semantic_tokens(&raw_data, legend))
                        {
                            // Store on the matching buffer.
                            for &bid in self.buffer_manager.list().iter() {
                                if let Some(state) = self.buffer_manager.get_mut(bid) {
                                    if state.file_path.as_deref() == Some(path.as_path()) {
                                        state.semantic_tokens = decoded;
                                        redraw = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                LspEvent::CodeActionResponse {
                    request_id,
                    actions,
                    ..
                } => {
                    if self.lsp_pending_code_action == Some(request_id) {
                        self.lsp_pending_code_action = None;
                        let show_popup = self.lsp_show_code_action_popup_pending;
                        self.lsp_show_code_action_popup_pending = false;
                        if let Some((path, line)) = self.lsp_code_action_request_ctx.take() {
                            self.lsp_code_actions
                                .entry(path)
                                .or_default()
                                .insert(line, actions.clone());
                            if show_popup {
                                if actions.is_empty() {
                                    self.message = "No code actions available".to_string();
                                } else {
                                    self.show_code_actions_hover(line, actions);
                                }
                            }
                            redraw = true;
                        }
                    }
                }
                LspEvent::DocumentSymbolResponse {
                    request_id,
                    symbols,
                    ..
                } => {
                    if self.lsp_pending_document_symbols == Some(request_id) {
                        self.lsp_pending_document_symbols = None;
                        if self.picker_open && self.picker_source == PickerSource::CommandCenter {
                            self.picker_populate_document_symbols(symbols);
                            redraw = true;
                        }
                    }
                }
                LspEvent::WorkspaceSymbolResponse {
                    request_id,
                    symbols,
                    ..
                } => {
                    if self.lsp_pending_workspace_symbols == Some(request_id) {
                        self.lsp_pending_workspace_symbols = None;
                        if self.picker_open && self.picker_source == PickerSource::CommandCenter {
                            self.picker_populate_workspace_symbols(symbols);
                            redraw = true;
                        }
                    }
                }
            }
        }
        redraw
    }

    /// Request LSP completion at cursor position.
    pub(crate) fn lsp_request_completion(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_completion(&path, line, col_utf16) {
                self.lsp_pending_completion = Some(id);
            }
        }
    }

    /// Request LSP go-to-definition at cursor position.
    pub fn lsp_request_definition(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_definition(&path, line, col_utf16) {
                self.lsp_pending_definition = Some(id);
                self.message = "Jumping to definition...".to_string();
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Return which LSP navigation commands are available for the current buffer.
    /// Returns a list of (label, keybind, command_url) triples.
    pub(crate) fn lsp_goto_links(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        let mut result = Vec::new();
        if !self.settings.lsp_enabled {
            return result;
        }
        let Some(path) = self.active_buffer_path() else {
            return result;
        };
        let Some(mgr) = &self.lsp_manager else {
            return result;
        };
        if mgr.server_supports(&path, "definitionProvider") {
            result.push(("Definition", "gd", "command:definition"));
        }
        if mgr.server_supports(&path, "typeDefinitionProvider") {
            result.push(("Type Definition", "gy", "command:type_definition"));
        }
        if mgr.server_supports(&path, "implementationProvider") {
            result.push(("Implementations", "gi", "command:implementation"));
        }
        if mgr.server_supports(&path, "referencesProvider") {
            result.push(("References", "gr", "command:references"));
        }
        result
    }

    /// Extract clickable links from rendered markdown.
    ///
    /// Pairs each `Link` span (the label text) with the following `LinkUrl` span
    /// (the URL) on the same line. The returned click region covers the label,
    /// while the URL is used for dispatch. Command URIs displayed as `:Name?args`
    /// are restored to `command:Name?args`.
    pub(crate) fn extract_hover_links(
        rendered: &crate::core::markdown::MdRendered,
    ) -> Vec<(usize, usize, usize, String)> {
        use crate::core::markdown::MdStyle;
        let mut links = Vec::new();
        for (line_idx, line_spans) in rendered.spans.iter().enumerate() {
            let Some(line) = rendered.lines.get(line_idx) else {
                continue;
            };
            // Find each Link span and pair it with the next LinkUrl on the same line.
            let mut span_iter = line_spans.iter().peekable();
            while let Some(span) = span_iter.next() {
                if span.style == MdStyle::Link {
                    // Look for the following LinkUrl span to get the URL.
                    let url = span_iter
                        .peek()
                        .filter(|next| next.style == MdStyle::LinkUrl)
                        .and_then(|next| {
                            if next.end_byte <= line.len() {
                                Some(&line[next.start_byte..next.end_byte])
                            } else {
                                None
                            }
                        });
                    if let Some(url_text) = url {
                        // Command URIs display as ":Name?args" — restore prefix.
                        let url = if url_text.starts_with(':') {
                            format!("command{}", url_text)
                        } else {
                            url_text.to_string()
                        };
                        if is_safe_url(&url) {
                            // Click region = the Link label span.
                            links.push((line_idx, span.start_byte, span.end_byte, url));
                        }
                    }
                }
            }
        }
        links
    }

    /// Execute an LSP navigation command from a hover popup link.
    /// Moves the cursor to the given position before invoking the LSP request.
    pub fn execute_hover_goto(&mut self, command: &str) {
        // Get the anchor position from the hover popup before dismissing it.
        let (line, col) = if let Some(hover) = &self.editor_hover {
            (hover.anchor_line, hover.anchor_col)
        } else {
            return;
        };
        self.dismiss_editor_hover();
        // Move cursor to the hover anchor position.
        let view = self.view_mut();
        view.cursor.line = line;
        view.cursor.col = col;
        self.push_jump_location();
        match command {
            "command:definition" => self.lsp_request_definition(),
            "command:type_definition" => self.lsp_request_type_definition(),
            "command:implementation" => self.lsp_request_implementation(),
            "command:references" => self.lsp_request_references(),
            _ => {
                // Try dispatching as a command URI to plugin commands.
                self.execute_command_uri(command);
            }
        }
    }

    /// Decode percent-encoded characters in a string (e.g. `%20` → space).
    pub fn percent_decode(input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let bytes = input.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                    result.push((hi << 4 | lo) as char);
                    i += 3;
                    continue;
                }
            }
            result.push(bytes[i] as char);
            i += 1;
        }
        result
    }

    /// Execute a `command:Name` or `command:Name?args` URI.
    /// Returns `true` if a matching command was found and executed.
    pub fn execute_command_uri(&mut self, url: &str) -> bool {
        let rest = match url.strip_prefix("command:") {
            Some(r) => r,
            None => return false,
        };
        if rest.is_empty() {
            return false;
        }
        let (cmd_name, cmd_args) = match rest.split_once('?') {
            Some((name, args)) => (name, Self::percent_decode(args)),
            None => (rest, String::new()),
        };
        if cmd_name.is_empty() {
            return false;
        }
        self.plugin_run_command(cmd_name, &cmd_args)
    }

    /// Request LSP hover at cursor position.
    /// Request LSP hover at a specific buffer position (not necessarily the cursor).
    pub(crate) fn lsp_request_hover_at(&mut self, line: usize, col: usize) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let Some(state) = self.buffer_manager.get(self.active_buffer_id()) else {
            return;
        };
        let Some(path) = state.file_path.as_ref().cloned() else {
            return;
        };
        let line_text: String = state.buffer.content.line(line).chars().collect();
        let col_utf16 = lsp::char_to_utf16_offset(&line_text, col);
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_hover(&path, line as u32, col_utf16) {
                self.lsp_pending_hover = Some(id);
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Request code actions at the exact cursor position.
    /// Called proactively after cursor settles (150ms debounce) and on-demand via `<leader>ca`.
    pub fn lsp_request_code_actions_for_line(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        // Don't send if a request is already in flight.
        if self.lsp_pending_code_action.is_some() {
            return;
        }
        self.ensure_lsp_manager();
        let Some((path, lsp_line, col_utf16)) = self.lsp_cursor_position() else {
            return;
        };
        let line = lsp_line as usize;
        // Clear stale cache for this line — actions depend on exact column.
        if let Some(line_map) = self.lsp_code_actions.get_mut(&path) {
            line_map.remove(&line);
        }
        // Build diagnostics JSON for lines touching the cursor line.
        let diags_json = self.diagnostics_json_for_line(&path, line);
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_code_action(&path, lsp_line, col_utf16, diags_json) {
                self.lsp_pending_code_action = Some(id);
                self.lsp_code_action_last_line = Some((path.clone(), line));
                self.lsp_code_action_request_ctx = Some((path, line));
            }
        }
    }

    /// Build a JSON array of diagnostics touching a specific line (for code action context).
    pub(crate) fn diagnostics_json_for_line(&self, path: &Path, line: usize) -> serde_json::Value {
        let diags = match self.lsp_diagnostics.get(path) {
            Some(d) => d,
            None => return serde_json::json!([]),
        };
        let arr: Vec<serde_json::Value> = diags
            .iter()
            .filter(|d| {
                let start = d.range.start.line as usize;
                let end = d.range.end.line as usize;
                line >= start && line <= end
            })
            .map(|d| {
                serde_json::json!({
                    "range": {
                        "start": { "line": d.range.start.line, "character": d.range.start.character },
                        "end": { "line": d.range.end.line, "character": d.range.end.character }
                    },
                    "severity": d.severity as i32,
                    "message": d.message
                })
            })
            .collect();
        serde_json::Value::Array(arr)
    }

    /// Whether any code actions are available on the given line.
    pub fn has_code_actions_on_line(&self, line: usize) -> bool {
        let Some(path) = self.active_buffer_path() else {
            return false;
        };
        self.lsp_code_actions
            .get(&path)
            .and_then(|m| m.get(&line))
            .is_some_and(|v| !v.is_empty())
    }

    /// Show code actions for the current line in an editor hover popup.
    /// If cached actions exist, shows immediately. Otherwise fires an LSP request
    /// and `lsp_show_code_action_popup_pending` causes the response handler to
    /// display the popup when results arrive.
    pub fn show_code_actions_popup(&mut self) {
        if self.active_buffer_path().is_none() {
            return;
        }
        // Always make a fresh request — code actions depend on exact cursor column.
        self.lsp_show_code_action_popup_pending = true;
        self.lsp_request_code_actions_for_line();
        if self.lsp_pending_code_action.is_none() {
            self.lsp_show_code_action_popup_pending = false;
            self.message = "No code actions available".to_string();
        }
    }

    pub(crate) fn show_code_actions_hover(&mut self, _line: usize, actions: Vec<lsp::CodeAction>) {
        let buttons: Vec<DialogButton> = actions
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let kind_str = a
                    .kind
                    .as_deref()
                    .map(|k| format!(" ({})", k))
                    .unwrap_or_default();
                DialogButton {
                    label: format!("{}{}", a.title, kind_str),
                    hotkey: '\0', // no single-key hotkey
                    action: format!("apply_{}", i),
                }
            })
            .collect();
        self.pending_code_action_choices = actions;
        self.show_dialog("code_actions", "Code Actions", vec![], buttons);
    }

    /// Request LSP find-references at cursor position.
    pub fn lsp_request_references(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_references(&path, line, col_utf16) {
                self.lsp_pending_references = Some(id);
                self.message = "Finding references...".to_string();
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Request LSP go-to-implementation at cursor position.
    pub(crate) fn lsp_request_implementation(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_implementation(&path, line, col_utf16) {
                self.lsp_pending_implementation = Some(id);
                self.message = "Finding implementation...".to_string();
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Request LSP go-to-type-definition at cursor position.
    pub(crate) fn lsp_request_type_definition(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_type_definition(&path, line, col_utf16) {
                self.lsp_pending_type_definition = Some(id);
                self.message = "Finding type definition...".to_string();
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Request LSP signature help at cursor position (triggered in insert mode).
    pub(crate) fn lsp_request_signature_help(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_signature_help(&path, line, col_utf16) {
                self.lsp_pending_signature = Some(id);
            }
        }
    }

    /// Request LSP formatting for the current buffer.
    pub fn lsp_format_current(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, _line, _col) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        let tab_size = self.settings.tabstop as u32;
        let insert_spaces = self.settings.expand_tab;
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_formatting(&path, tab_size, insert_spaces) {
                self.lsp_pending_formatting = Some(id);
                self.message = "Formatting...".to_string();
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Request LSP rename of the symbol at cursor.
    pub(crate) fn lsp_request_rename(&mut self, new_name: &str) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        let new_name = new_name.to_string();
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_rename(&path, line, col_utf16, &new_name) {
                self.lsp_pending_rename = Some(id);
                self.message = format!("Renaming to '{new_name}'...");
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Apply a list of LSP text edits to a buffer as a single undo group.
    /// Edits must be applied in reverse order (last first) to preserve offsets.
    pub(crate) fn apply_lsp_edits(&mut self, buffer_id: BufferId, mut edits: Vec<FormattingEdit>) {
        if edits.is_empty() {
            return;
        }
        // Sort in reverse start order so applying one edit doesn't shift others
        edits.sort_by(|a, b| {
            b.range
                .start
                .line
                .cmp(&a.range.start.line)
                .then(b.range.start.character.cmp(&a.range.start.character))
        });
        // Start undo group on the target buffer (not necessarily the active buffer).
        let cursor = self
            .windows
            .values()
            .find(|w| w.buffer_id == buffer_id)
            .map(|w| w.view.cursor)
            .unwrap_or_default();
        if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
            state.start_undo_group(cursor);
        }
        for edit in &edits {
            let state = match self.buffer_manager.get(buffer_id) {
                Some(s) => s,
                None => break,
            };
            let content = state.buffer.content.clone();
            let total_lines = content.len_lines();
            let start_line = (edit.range.start.line as usize).min(total_lines.saturating_sub(1));
            let end_line = (edit.range.end.line as usize).min(total_lines.saturating_sub(1));

            let start_line_text: String = content.line(start_line).chars().collect();
            let end_line_text: String = content.line(end_line).chars().collect();

            let start_char =
                lsp::utf16_offset_to_char(&start_line_text, edit.range.start.character);
            let end_char = lsp::utf16_offset_to_char(&end_line_text, edit.range.end.character);

            let start_offset = content.line_to_char(start_line) + start_char;
            let end_offset = content.line_to_char(end_line) + end_char;

            if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
                if end_offset > start_offset {
                    let deleted: String = state
                        .buffer
                        .content
                        .slice(start_offset..end_offset)
                        .chars()
                        .collect();
                    state.buffer.content.remove(start_offset..end_offset);
                    state.record_delete(start_offset, &deleted);
                }
                if !edit.new_text.is_empty() {
                    state.buffer.content.insert(start_offset, &edit.new_text);
                    state.record_insert(start_offset, &edit.new_text);
                }
                state.dirty = true;
            }
        }
        if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
            state.finish_undo_group();
            // Clear stale semantic tokens immediately — positions are now wrong.
            state.semantic_tokens.clear();
        }
        // Mark buffer dirty so the next LSP flush sends didChange + re-requests tokens.
        self.lsp_dirty_buffers.insert(buffer_id, true);
    }

    /// Apply a workspace-wide rename edit.
    pub(crate) fn apply_workspace_edit(&mut self, we: WorkspaceEdit) {
        for file_edit in we.changes {
            // Try to find an already-open buffer for this path
            let buffer_id = self.buffer_manager.list().into_iter().find(|&bid| {
                self.buffer_manager
                    .get(bid)
                    .and_then(|s| s.file_path.as_deref())
                    .map(|p| p == file_edit.path)
                    .unwrap_or(false)
            });

            if let Some(bid) = buffer_id {
                self.apply_lsp_edits(bid, file_edit.edits);
            } else {
                // File not open — read, edit, and write back to disk
                if let Ok(text) = std::fs::read_to_string(&file_edit.path) {
                    let mut edits = file_edit.edits;
                    // Sort in reverse order
                    edits.sort_by(|a, b| {
                        b.range
                            .start
                            .line
                            .cmp(&a.range.start.line)
                            .then(b.range.start.character.cmp(&a.range.start.character))
                    });
                    let mut rope = ropey::Rope::from_str(&text);
                    for edit in &edits {
                        let total_lines = rope.len_lines();
                        let start_line =
                            (edit.range.start.line as usize).min(total_lines.saturating_sub(1));
                        let end_line =
                            (edit.range.end.line as usize).min(total_lines.saturating_sub(1));
                        let start_line_text: String = rope.line(start_line).chars().collect();
                        let end_line_text: String = rope.line(end_line).chars().collect();
                        let start_char =
                            lsp::utf16_offset_to_char(&start_line_text, edit.range.start.character);
                        let end_char =
                            lsp::utf16_offset_to_char(&end_line_text, edit.range.end.character);
                        let start_offset = rope.line_to_char(start_line) + start_char;
                        let end_offset = rope.line_to_char(end_line) + end_char;
                        if end_offset > start_offset {
                            rope.remove(start_offset..end_offset);
                        }
                        rope.insert(start_offset, &edit.new_text);
                    }
                    let _ = std::fs::write(&file_edit.path, rope.to_string());
                }
            }
        }
    }

    /// Get the cursor's file path, line, and UTF-16 column for LSP requests.
    pub(crate) fn lsp_cursor_position(&self) -> Option<(PathBuf, u32, u32)> {
        let state = self.buffer_manager.get(self.active_buffer_id())?;
        let path = state.file_path.as_ref()?.clone();
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_text: String = state.buffer.content.line(line).chars().collect();
        let col_utf16 = lsp::char_to_utf16_offset(&line_text, col);
        Some((path, line as u32, col_utf16))
    }

    /// Jump to the next diagnostic in the current buffer.
    pub fn jump_next_diagnostic(&mut self) {
        let path = self
            .buffer_manager
            .get(self.active_buffer_id())
            .and_then(|s| s.file_path.as_ref())
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()));
        let path = match path {
            Some(p) => p,
            None => return,
        };
        let diags = match self.lsp_diagnostics.get(&path) {
            Some(d) if !d.is_empty() => d,
            _ => {
                self.message = "No diagnostics".to_string();
                return;
            }
        };
        let cur_line = self.view().cursor.line as u32;
        let cur_char = self.view().cursor.col as u32;

        // Find the first diagnostic after the current cursor position
        let next = diags.iter().find(|d| {
            d.range.start.line > cur_line
                || (d.range.start.line == cur_line && d.range.start.character > cur_char)
        });
        let diag = next.unwrap_or(&diags[0]).clone();

        let line = diag.range.start.line as usize;
        self.view_mut().cursor.line = line;
        let line_text: String = self.buffer().content.line(line).chars().collect();
        self.view_mut().cursor.col =
            lsp::utf16_offset_to_char(&line_text, diag.range.start.character);
        self.message = format!("{}: {}", diag.severity.symbol(), diag.message);
    }

    /// Jump to the previous diagnostic in the current buffer.
    pub fn jump_prev_diagnostic(&mut self) {
        let path = self
            .buffer_manager
            .get(self.active_buffer_id())
            .and_then(|s| s.file_path.as_ref())
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()));
        let path = match path {
            Some(p) => p,
            None => return,
        };
        let diags = match self.lsp_diagnostics.get(&path) {
            Some(d) if !d.is_empty() => d,
            _ => {
                self.message = "No diagnostics".to_string();
                return;
            }
        };
        let cur_line = self.view().cursor.line as u32;
        let cur_char = self.view().cursor.col as u32;

        // Find the last diagnostic before the current cursor position
        let prev = diags.iter().rev().find(|d| {
            d.range.start.line < cur_line
                || (d.range.start.line == cur_line && d.range.start.character < cur_char)
        });
        let diag = prev.unwrap_or(diags.last().unwrap()).clone();

        let line = diag.range.start.line as usize;
        self.view_mut().cursor.line = line;
        let line_text: String = self.buffer().content.line(line).chars().collect();
        self.view_mut().cursor.col =
            lsp::utf16_offset_to_char(&line_text, diag.range.start.character);
        self.message = format!("{}: {}", diag.severity.symbol(), diag.message);
    }

    /// Shut down all LSP servers (called on quit).
    pub fn lsp_shutdown(&mut self) {
        if let Some(mgr) = &mut self.lsp_manager {
            mgr.shutdown_all();
        }
        self.lsp_manager = None;
    }

    /// Get diagnostic counts for the current buffer (for status bar).
    pub fn diagnostic_counts(&self) -> (usize, usize) {
        let path = self
            .buffer_manager
            .get(self.active_buffer_id())
            .and_then(|s| s.file_path.as_ref())
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()));
        let path = match path {
            Some(p) => p,
            None => return (0, 0),
        };
        let diags = match self.lsp_diagnostics.get(&path) {
            Some(d) => d,
            None => return (0, 0),
        };
        let errors = diags
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .count();
        let warnings = diags
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .count();
        (errors, warnings)
    }
}
