use super::*;

impl Engine {
    // ── Integrated Terminal ────────────────────────────────────────────────

    /// Get a reference to the active terminal pane, if any.
    pub fn active_terminal(&self) -> Option<&TerminalPane> {
        self.terminal_panes.get(self.terminal_active)
    }

    /// Get a mutable reference to the active terminal pane, if any.
    pub fn active_terminal_mut(&mut self) -> Option<&mut TerminalPane> {
        self.terminal_panes.get_mut(self.terminal_active)
    }

    /// Open the terminal panel. If no panes exist, create the first one.
    /// If panes already exist, just show/focus the panel.
    pub fn open_terminal(&mut self, cols: u16, rows: u16) {
        if self.terminal_panes.is_empty() {
            self.terminal_new_tab(cols, rows);
        } else {
            self.terminal_open = true;
            self.terminal_has_focus = true;
        }
    }

    /// Create a new terminal tab (always spawns a fresh shell in the editor's CWD).
    pub fn terminal_new_tab(&mut self, cols: u16, rows: u16) {
        self.terminal_new_tab_at(cols, rows, None);
    }

    /// Create a new terminal tab, optionally at a specific working directory.
    /// If `dir` is None, uses the editor's CWD.
    pub fn terminal_new_tab_at(&mut self, cols: u16, rows: u16, dir: Option<&Path>) {
        let shell = default_shell();
        let cwd = dir.unwrap_or(&self.cwd).to_path_buf();
        let history_cap = self.settings.terminal_scrollback_lines;
        match TerminalPane::new(cols, rows, &shell, &cwd, history_cap) {
            Ok(pane) => {
                self.terminal_panes.push(pane);
                self.terminal_active = self.terminal_panes.len() - 1;
                self.terminal_open = true;
                self.terminal_has_focus = true;
            }
            Err(e) => self.message = format!("terminal: failed to open PTY: {e}"),
        }
    }

    /// Run a command in a new terminal pane (visible to the user).
    /// Used for extension installs so the user can see progress, errors, and enter
    /// sudo passwords. The pane waits for Enter after the command finishes.
    pub fn terminal_run_command(&mut self, command: &str, cols: u16, rows: u16) {
        let cwd = self.cwd.clone();
        let history_cap = self.settings.terminal_scrollback_lines;
        // Extract install context from pending_install_context (set by ext_install_from_registry).
        let ctx = self.pending_install_context.take();
        match TerminalPane::new_command(cols, rows, command, &cwd, history_cap, ctx) {
            Ok(pane) => {
                self.terminal_panes.push(pane);
                self.terminal_active = self.terminal_panes.len() - 1;
                self.terminal_open = true;
                self.terminal_has_focus = true;
            }
            Err(e) => self.message = format!("terminal: failed to run command: {e}"),
        }
    }

    /// Close the active terminal tab. If it was the last tab, close the panel.
    /// Closing either pane while in split mode also exits split view.
    pub fn terminal_close_active_tab(&mut self) {
        if self.terminal_panes.is_empty() {
            return;
        }
        // Exiting split mode before removing the pane keeps tab indices sane.
        self.terminal_split = false;
        self.terminal_panes.remove(self.terminal_active);
        if self.terminal_panes.is_empty() {
            self.terminal_open = false;
            self.terminal_has_focus = false;
            self.terminal_active = 0;
        } else {
            self.terminal_active = self.terminal_active.min(self.terminal_panes.len() - 1);
        }
    }

    /// Enable horizontal split view.
    /// Ensures at least two panes exist (creates a second if needed), resizes both to
    /// `half_cols`, then sets focus to the right pane (index 1).
    pub fn terminal_open_split(&mut self, half_cols: u16, rows: u16) {
        let history_cap = self.settings.terminal_scrollback_lines;
        if self.terminal_panes.is_empty() {
            // Create two fresh panes.
            let shell = default_shell();
            let cwd = self.cwd.clone();
            for _ in 0..2 {
                match TerminalPane::new(half_cols, rows, &shell, &cwd, history_cap) {
                    Ok(pane) => self.terminal_panes.push(pane),
                    Err(e) => {
                        self.message = format!("terminal: failed to open PTY: {e}");
                        return;
                    }
                }
            }
            self.terminal_open = true;
            self.terminal_has_focus = true;
        } else if self.terminal_panes.len() == 1 {
            // Resize existing pane to half-width, then spawn a second.
            self.terminal_panes[0].resize(half_cols, rows);
            let shell = default_shell();
            let cwd = self.cwd.clone();
            match TerminalPane::new(half_cols, rows, &shell, &cwd, history_cap) {
                Ok(pane) => self.terminal_panes.push(pane),
                Err(e) => {
                    self.message = format!("terminal: failed to open PTY: {e}");
                    return;
                }
            }
        } else {
            // Two or more panes exist — resize the first two to half-width.
            self.terminal_panes[0].resize(half_cols, rows);
            self.terminal_panes[1].resize(half_cols, rows);
        }
        self.terminal_split = true;
        self.terminal_active = 1; // right pane gets focus
    }

    /// Disable horizontal split view and return to single-pane / tab view.
    /// Panes are kept alive as regular tabs; `full_cols` is used to resize the
    /// active pane back to the full panel width.
    pub fn terminal_close_split(&mut self, full_cols: u16, rows: u16) {
        self.terminal_split = false;
        self.terminal_split_left_cols = 0;
        // Resize whatever is now the active pane to full width.
        if let Some(pane) = self.terminal_panes.get_mut(self.terminal_active) {
            pane.resize(full_cols, rows);
        }
    }

    /// Toggle split mode on/off. `full_cols` = total panel width (each pane gets half).
    pub fn terminal_toggle_split(&mut self, full_cols: u16, rows: u16) {
        if self.terminal_split {
            self.terminal_close_split(full_cols, rows);
        } else {
            self.terminal_open_split(full_cols / 2, rows);
        }
    }

    /// Switch keyboard focus between the two split panes (left ↔ right).
    /// No-op when not in split mode.
    pub fn terminal_split_switch_focus(&mut self) {
        if self.terminal_split && self.terminal_panes.len() >= 2 {
            self.terminal_active = 1 - self.terminal_active;
        }
    }

    /// Update the visual divider position during a drag (no PTY resize yet).
    /// Backends call this on every drag event; finalize with `terminal_split_finalize_drag`.
    pub fn terminal_split_set_drag_cols(&mut self, left_cols: u16) {
        self.terminal_split_left_cols = left_cols;
    }

    /// Commit a drag resize: resize both PTY panes to the new sizes.
    /// Clears `terminal_split_left_cols` so PTY cols become authoritative again.
    pub fn terminal_split_finalize_drag(&mut self, left_cols: u16, right_cols: u16, rows: u16) {
        self.terminal_split_left_cols = 0;
        if self.terminal_panes.len() >= 2 {
            self.terminal_panes[0].resize(left_cols, rows);
            self.terminal_panes[1].resize(right_cols, rows);
        }
    }

    /// Switch to the terminal tab at the given index (clamped to valid range).
    pub fn terminal_switch_tab(&mut self, idx: usize) {
        if !self.terminal_panes.is_empty() {
            self.terminal_active = idx.min(self.terminal_panes.len() - 1);
        }
    }

    /// Hide the terminal panel but keep all PTY panes running.
    pub fn close_terminal(&mut self) {
        self.terminal_open = false;
        self.terminal_has_focus = false;
    }

    /// Toggle the integrated terminal:
    /// - If open and focused → close (hide)
    /// - If open but unfocused → give focus
    /// - If not open → signal UI to open (UI calls terminal_new_tab with correct dimensions)
    pub fn toggle_terminal(&mut self) {
        if self.terminal_open && self.terminal_has_focus {
            self.close_terminal();
        } else if self.terminal_open {
            self.terminal_has_focus = true;
        } else {
            // Signal UI to call terminal_new_tab with correct dimensions
            self.terminal_open = true;
            self.terminal_has_focus = true;
        }
    }

    /// Drain PTY output from all panes and update VT100 screens.
    /// Returns true if a redraw is needed.
    /// Exited panes are automatically removed; closes the panel when the last pane exits.
    pub fn poll_terminal(&mut self) -> bool {
        let mut got_data = false;
        for pane in &mut self.terminal_panes {
            got_data |= pane.poll();
        }
        // Remove exited panes in reverse order (preserves earlier indices during removal).
        // For install panes, finalize the install (check binary, register LSP) before removing.
        let mut i = self.terminal_panes.len();
        while i > 0 {
            i -= 1;
            if self.terminal_panes[i].exited {
                if let Some(ctx) = self.terminal_panes[i].install_context.take() {
                    self.finalize_install_from_terminal(&ctx);
                }
                self.terminal_panes.remove(i);
                if self.terminal_active > i {
                    self.terminal_active = self.terminal_active.saturating_sub(1);
                }
            }
        }
        if self.terminal_panes.is_empty() {
            self.terminal_open = false;
            self.terminal_has_focus = false;
            self.terminal_active = 0;
            self.terminal_split = false;
        } else {
            self.terminal_active = self.terminal_active.min(self.terminal_panes.len() - 1);
            // If a pane exited while in split and we're down to one, exit split.
            if self.terminal_split && self.terminal_panes.len() < 2 {
                self.terminal_split = false;
            }
        }
        // Keep find matches fresh if new terminal output arrived while find is active.
        if got_data && self.terminal_find_active {
            self.terminal_find_update_matches();
        }
        got_data
    }

    /// Called when an install terminal pane exits. Checks if the binary is now
    /// available on PATH and registers the LSP/DAP server if so.
    fn finalize_install_from_terminal(&mut self, ctx: &InstallContext) {
        self.lsp_installing.remove(&ctx.install_key);

        let ext_name = &ctx.ext_name;
        let manifest = self
            .ext_available_manifests()
            .into_iter()
            .find(|m| m.name == *ext_name);
        let manifest = match manifest {
            Some(m) => m,
            None => return,
        };

        // Check if LSP binary is now on PATH and register it.
        if !manifest.lsp.binary.is_empty() {
            let all_lsp: Vec<&str> = std::iter::once(manifest.lsp.binary.as_str())
                .chain(manifest.lsp.fallback_binaries.iter().map(|s| s.as_str()))
                .filter(|b| !b.is_empty())
                .collect();
            if let Some(bin) = all_lsp.iter().copied().find(|b| binary_on_path(b)) {
                self.ensure_lsp_manager();
                for lsp_lang in &manifest.language_ids {
                    let config = lsp::LspServerConfig {
                        command: bin.to_string(),
                        args: manifest.lsp.args.clone(),
                        languages: vec![lsp_lang.clone()],
                    };
                    if let Some(mgr) = &mut self.lsp_manager {
                        mgr.add_registry_entry(config);
                        mgr.ensure_server_for_language(lsp_lang);
                    }
                    self.lsp_reopen_buffers_for_language(lsp_lang);
                }
                self.message = format!("LSP server for '{ext_name}' installed and started ({bin})");
            } else {
                self.message = format!(
                    "Install for '{ext_name}' finished — LSP binary '{}' not found on PATH",
                    manifest.lsp.binary
                );
            }
        }

        // Check if DAP binary is now on PATH.
        if !manifest.dap.adapter.is_empty()
            && !manifest.dap.binary.is_empty()
            && binary_on_path(&manifest.dap.binary)
        {
            self.message = format!("DAP adapter for '{ext_name}' installed — press F5 to debug");
        }
    }

    /// Send raw bytes to the active pane's PTY stdin.
    pub fn terminal_write(&mut self, data: &[u8]) {
        if let Some(term) = self.active_terminal_mut() {
            term.write_input(data);
        }
    }

    /// Resize all terminal panes (shared panel height).
    pub fn terminal_resize(&mut self, cols: u16, rows: u16) {
        for pane in &mut self.terminal_panes {
            pane.resize(cols, rows);
        }
    }

    /// Return selected terminal text from the active pane for clipboard copy.
    pub fn terminal_copy_selection(&mut self) -> Option<String> {
        self.active_terminal()?.selected_text()
    }

    /// Scroll the active pane's scrollback view up (away from live output).
    pub fn terminal_scroll_up(&mut self, rows: usize) {
        if let Some(term) = self.active_terminal_mut() {
            term.scroll_up(rows);
        }
    }

    /// Scroll the active pane's scrollback view down (toward live output).
    pub fn terminal_scroll_down(&mut self, rows: usize) {
        if let Some(term) = self.active_terminal_mut() {
            term.scroll_down(rows);
        }
    }

    /// Return the active pane to the live view (cancel any scrollback offset).
    pub fn terminal_scroll_reset(&mut self) {
        if let Some(term) = self.active_terminal_mut() {
            term.scroll_reset();
        }
    }

    // ── Terminal inline find bar ───────────────────────────────────────────

    /// Open the terminal find bar and reset the query.
    pub fn terminal_find_open(&mut self) {
        self.terminal_find_active = true;
        self.terminal_find_query.clear();
        self.terminal_find_selected = 0;
        self.terminal_find_matches.clear();
    }

    /// Close the terminal find bar and clear all match state.
    pub fn terminal_find_close(&mut self) {
        self.terminal_find_active = false;
        self.terminal_find_query.clear();
        self.terminal_find_selected = 0;
        self.terminal_find_matches.clear();
    }

    /// Append a character to the find query and refresh matches.
    pub fn terminal_find_char(&mut self, ch: char) {
        self.terminal_find_query.push(ch);
        self.terminal_find_selected = 0;
        self.terminal_find_update_matches();
    }

    /// Delete the last character from the find query and refresh matches.
    pub fn terminal_find_backspace(&mut self) {
        self.terminal_find_query.pop();
        self.terminal_find_selected = 0;
        self.terminal_find_update_matches();
    }

    /// Advance to the next match (wraps around) and scroll to it.
    pub fn terminal_find_next(&mut self) {
        let n = self.terminal_find_matches.len();
        if n > 0 {
            self.terminal_find_selected = (self.terminal_find_selected + 1) % n;
            let (req_offset, _, _) = self.terminal_find_matches[self.terminal_find_selected];
            if let Some(term) = self.terminal_panes.get_mut(self.terminal_active) {
                term.set_scroll_offset(req_offset);
            }
        }
    }

    /// Go back to the previous match (wraps around) and scroll to it.
    pub fn terminal_find_prev(&mut self) {
        let n = self.terminal_find_matches.len();
        if n > 0 {
            self.terminal_find_selected = (self.terminal_find_selected + n - 1) % n;
            let (req_offset, _, _) = self.terminal_find_matches[self.terminal_find_selected];
            if let Some(term) = self.terminal_panes.get_mut(self.terminal_active) {
                term.set_scroll_offset(req_offset);
            }
        }
    }

    /// Scan the entire history buffer and the live vt100 screen, rebuilding
    /// `terminal_find_matches`.  Case-insensitive.
    ///
    /// Matches are `(required_scroll_offset, row, col)` where:
    /// - History match at `history[H]`: required_offset = `history.len() - H`, row = 0.
    ///   Formula: visible_row = row + current_offset − required_offset.
    /// - Live match at vt100 row R:    required_offset = 0, row = R.
    ///
    /// Sorted oldest-first (highest required_offset first, then top-to-bottom).
    fn terminal_find_update_matches(&mut self) {
        self.terminal_find_matches.clear();
        if !self.terminal_find_active || self.terminal_find_query.is_empty() {
            return;
        }
        let q_lower: Vec<char> = self.terminal_find_query.to_lowercase().chars().collect();
        let qlen = q_lower.len();
        let active_idx = self.terminal_active;
        let term = match self.terminal_panes.get(active_idx) {
            Some(t) => t,
            None => return,
        };

        let mut matches: Vec<(usize, u16, u16)> = Vec::new();
        let hist_len = term.history.len();

        // ── History rows (oldest → newest) ──────────────────────────────────
        for (hist_idx, hist_row) in term.history.iter().enumerate() {
            let required_offset = hist_len - hist_idx;
            let row_lower: Vec<char> = hist_row
                .iter()
                .map(|cell| {
                    let ch = cell.ch;
                    ch.to_lowercase().next().unwrap_or(ch)
                })
                .collect();
            if qlen <= row_lower.len() {
                for c in 0..=(row_lower.len() - qlen) {
                    if row_lower[c..c + qlen] == q_lower[..] {
                        matches.push((required_offset, 0, c as u16));
                    }
                }
            }
        }

        // ── Live vt100 rows (always at scrollback_offset = 0) ───────────────
        let cols = term.cols;
        let rows = term.rows;
        let screen = term.parser.screen();
        for r in 0..rows {
            let row_lower: Vec<char> = (0..cols)
                .map(|c| {
                    let ch = screen
                        .cell(r, c)
                        .map(|cell| {
                            let s = cell.contents();
                            if s.is_empty() {
                                ' '
                            } else {
                                s.chars().next().unwrap_or(' ')
                            }
                        })
                        .unwrap_or(' ');
                    ch.to_lowercase().next().unwrap_or(ch)
                })
                .collect();
            if qlen <= row_lower.len() {
                for c in 0..=(row_lower.len() - qlen) {
                    if row_lower[c..c + qlen] == q_lower[..] {
                        matches.push((0, r, c as u16));
                    }
                }
            }
        }

        // Sort: oldest first (highest required_offset), then top-to-bottom.
        matches.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        self.terminal_find_matches = matches;
        if !self.terminal_find_matches.is_empty() {
            self.terminal_find_selected = self
                .terminal_find_selected
                .min(self.terminal_find_matches.len() - 1);
        } else {
            self.terminal_find_selected = 0;
        }
    }
}
