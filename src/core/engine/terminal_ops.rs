use super::*;
use crate::core::terminal::TermSelection;

impl Engine {
    // ── Integrated Terminal ────────────────────────────────────────────────

    /// Get a reference to the active terminal session, if any.
    pub fn active_terminal(&self) -> Option<&TerminalSession> {
        self.terminal_panes
            .get(self.terminal_active)
            .map(|s| &s.session)
    }

    /// Get a mutable reference to the active terminal session, if any.
    pub fn active_terminal_mut(&mut self) -> Option<&mut TerminalSession> {
        self.terminal_panes
            .get_mut(self.terminal_active)
            .map(|s| &mut s.session)
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
        match TerminalSession::spawn(cols, rows, &shell, &cwd, history_cap) {
            Ok(sess) => {
                self.terminal_panes.push(TerminalSlot {
                    session: sess,
                    install_ctx: None,
                });
                self.terminal_active = self.terminal_panes.len() - 1;
                self.terminal_open = true;
                self.terminal_has_focus = true;
            }
            Err(e) => self.message = format!("terminal: failed to open PTY: {e}"),
        }
    }

    /// Run a command in a new terminal pane (visible to the user).
    /// Used for extension installs so the user can see progress, errors, and enter
    /// sudo passwords. The pane waits for Enter after the command finishes, then
    /// the shell **exits** so `poll_terminal` detects `is_exited()` and calls
    /// `finalize_install_from_terminal` to register the LSP/DAP server.
    ///
    /// Spawns an interactive shell via the quadraui `TerminalSession` primitive, then
    /// immediately injects the wrapped command into the PTY so the shell executes it.
    /// The wrapper ends with `exit` / `Exit` so the shell process exits after Enter —
    /// without that suffix the interactive shell would return to its PS1 prompt and
    /// `is_exited()` would never fire, leaving the install context unregistered.
    pub fn terminal_run_command(&mut self, command: &str, cols: u16, rows: u16) {
        let cwd = self.cwd.clone();
        let history_cap = self.settings.terminal_scrollback_lines;
        // Extract install context set by ext_install_from_registry.
        let ctx = self.pending_install_context.take();
        let shell = default_shell();
        let is_powershell =
            shell.to_lowercase().contains("powershell") || shell.to_lowercase().contains("pwsh");
        // Build a wrapper script that runs the command, shows the exit status, waits
        // for Enter, then exits the shell so `TerminalSession::is_exited()` fires and
        // `poll_terminal` can call `finalize_install_from_terminal`.
        let wrapped = build_terminal_install_wrapper(command, is_powershell);
        match TerminalSession::spawn(cols, rows, &shell, &cwd, history_cap) {
            Ok(mut sess) => {
                // Inject the wrapped command immediately.  The PTY master writer is
                // ready as soon as `spawn` returns — the kernel PTY subsystem buffers
                // the bytes and the shell reads them from its stdin when it starts
                // processing input, so there is no race between this write and the
                // shell's readiness.
                sess.write_input(wrapped.as_bytes());
                self.terminal_panes.push(TerminalSlot {
                    session: sess,
                    install_ctx: ctx,
                });
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
                match TerminalSession::spawn(half_cols, rows, &shell, &cwd, history_cap) {
                    Ok(sess) => {
                        self.terminal_panes.push(TerminalSlot {
                            session: sess,
                            install_ctx: None,
                        });
                    }
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
            self.terminal_panes[0].session.resize(half_cols, rows);
            let shell = default_shell();
            let cwd = self.cwd.clone();
            match TerminalSession::spawn(half_cols, rows, &shell, &cwd, history_cap) {
                Ok(sess) => {
                    self.terminal_panes.push(TerminalSlot {
                        session: sess,
                        install_ctx: None,
                    });
                }
                Err(e) => {
                    self.message = format!("terminal: failed to open PTY: {e}");
                    return;
                }
            }
        } else {
            // Two or more panes exist — resize the first two to half-width.
            self.terminal_panes[0].session.resize(half_cols, rows);
            self.terminal_panes[1].session.resize(half_cols, rows);
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
        if let Some(slot) = self.terminal_panes.get_mut(self.terminal_active) {
            slot.session.resize(full_cols, rows);
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
            self.terminal_panes[0].session.resize(left_cols, rows);
            self.terminal_panes[1].session.resize(right_cols, rows);
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
        self.terminal_maximized = false;
        self.terminal_open = false;
        self.terminal_has_focus = false;
    }

    /// Resolve which zone of the bottom panel contains the click y-coordinate
    /// using the geometry cached at paint time. Returns `None` if the panel
    /// isn't currently painted or `y` is above the panel top. `y` is in the
    /// caller's unit (pixels for GTK, character rows for TUI) — must match
    /// what the backend wrote into [`BottomPanelGeometry`] at paint time.
    pub fn resolve_bottom_panel_zone(&self, y: f64) -> Option<BottomPanelZone> {
        let g = (*self.bottom_panel_geometry.borrow())?;
        if y < g.top_y || y >= g.top_y + g.height {
            return None;
        }
        let rel = y - g.top_y;
        let zone = if rel < g.toolbar_y {
            BottomPanelZone::TabBar
        } else if rel < g.content_y {
            BottomPanelZone::Toolbar
        } else if g.content_row_h > 0.0 {
            BottomPanelZone::Content {
                row_offset: ((rel - g.content_y) / g.content_row_h) as u16,
            }
        } else {
            BottomPanelZone::Content { row_offset: 0 }
        };
        Some(zone)
    }

    /// Handle a mouse-button press on a non-split terminal pane.
    ///
    /// **Forwarding first**: when the child process has enabled SGR mouse
    /// reporting (`mouse_reporting_enabled()`) the click is forwarded as an
    /// SGR-1006 `Press` byte sequence and the function returns `true`.  In
    /// that case no local selection is started — the inner app owns the
    /// pointer.
    ///
    /// **Local fallback**: when forwarding returns `false` (ordinary shell on
    /// the primary screen) the function focuses the terminal, resets any
    /// scrollback offset, and starts a zero-length selection at `(col, row)`
    /// (0-based cells within the pane).
    ///
    /// Backends call this for every left- (or right-) click in the content
    /// area, passing the cell coordinates they already had to translate from
    /// their native pixel / cell space.  The forwarding policy lives entirely
    /// here — no backend-specific branching needed.
    ///
    /// Returns `true` when the event was forwarded to the child process.
    ///
    /// # Gold standard
    /// Mirrors `examples/common/terminal_app.rs` `UiEvent::MouseDown` arm
    /// (quadraui#279/#365) — zero backend-specific code.
    pub fn handle_terminal_pane_press(
        &mut self,
        col: u16,
        row: u16,
        button: quadraui::MouseButton,
        mods: quadraui::Modifiers,
    ) -> bool {
        use quadraui::terminal_engine::TerminalMouseKind;
        self.terminal_has_focus = true;
        self.terminal_scroll_reset();
        // Try to forward to the child first.  `forward_mouse` checks
        // `should_forward_mouse(Press)` which is gated on
        // `mouse_reporting_enabled()` only (not alt-screen — clicks are
        // only forwarded when the child explicitly asked for them).
        let forwarded = if let Some(term) = self.active_terminal_mut() {
            term.forward_mouse(TerminalMouseKind::Press, button, col, row, mods)
        } else {
            false
        };
        if !forwarded {
            // Local selection start.
            if let Some(term) = self.active_terminal_mut() {
                term.selection = Some(TermSelection {
                    start_row: row,
                    start_col: col,
                    end_row: row,
                    end_col: col,
                });
            }
        }
        forwarded
    }

    /// Backward-compat wrapper: press with left button and no modifiers.
    ///
    /// Callers that don't have button/modifier info (e.g. split-click
    /// helpers) use this.  New code should prefer
    /// [`handle_terminal_pane_press`](Self::handle_terminal_pane_press).
    pub fn handle_terminal_pane_click(&mut self, col: u16, row: u16) {
        self.handle_terminal_pane_press(
            col,
            row,
            quadraui::MouseButton::Left,
            quadraui::Modifiers::default(),
        );
    }

    /// Update the active pane's selection endpoint during a mouse drag.
    ///
    /// **Forwarding first**: when the child has mouse reporting enabled the
    /// drag is forwarded as a `Move` (button-held) event.  The inner app
    /// sees the live pointer position and can act on it (e.g. select text
    /// inside a nested vim).
    ///
    /// **Local fallback**: when forwarding returns `false` the endpoint of
    /// the in-progress selection is extended to `(col, row)`.
    ///
    /// Both TUI and GTK call this with their pane-relative cell coordinates
    /// — no backend-specific branching.
    ///
    /// # Gold standard
    /// Mirrors the `UiEvent::MouseMoved { buttons: left, .. }` arm of
    /// `examples/common/terminal_app.rs`.
    pub fn handle_terminal_pane_drag(&mut self, col: u16, row: u16) {
        use quadraui::terminal_engine::TerminalMouseKind;
        let forwarded = if let Some(term) = self.active_terminal_mut() {
            term.forward_mouse(
                TerminalMouseKind::Move,
                quadraui::MouseButton::Left,
                col,
                row,
                quadraui::Modifiers::default(),
            )
        } else {
            false
        };
        if !forwarded {
            if let Some(term) = self.active_terminal_mut() {
                if let Some(ref mut sel) = term.selection {
                    sel.end_row = row;
                    sel.end_col = col;
                }
            }
        }
    }

    /// Handle a mouse-button release over the terminal content area.
    ///
    /// Forwards the release to the child when it has mouse reporting enabled
    /// (matches `UiEvent::MouseUp` in `terminal_app.rs`), then auto-copies
    /// any live selection to the clipboard via the engine's `clipboard_write`
    /// callback.
    ///
    /// Returns `true` when text was copied to the clipboard.
    ///
    /// Both TUI and GTK call this on every left-button release over the
    /// terminal panel — no per-backend auto-copy logic needed.
    pub fn handle_terminal_pane_release(
        &mut self,
        col: u16,
        row: u16,
        button: quadraui::MouseButton,
    ) -> bool {
        use quadraui::terminal_engine::TerminalMouseKind;
        // Forward release to child when it owns the pointer.
        if let Some(term) = self.active_terminal_mut() {
            term.forward_mouse(
                TerminalMouseKind::Release,
                button,
                col,
                row,
                quadraui::Modifiers::default(),
            );
        }
        // Auto-copy selection to clipboard.
        self.terminal_autocopy_selection()
    }

    /// Copy the active pane's current text selection to the clipboard via
    /// the engine's `clipboard_write` callback.
    ///
    /// Called from [`handle_terminal_pane_release`] and by each backend on
    /// mouse-up when the terminal has focus.  Returns `true` when text was
    /// copied.
    pub fn terminal_autocopy_selection(&mut self) -> bool {
        if !self.terminal_has_focus {
            return false;
        }
        let text = self
            .active_terminal()
            .and_then(|t| t.selected_text());
        if let Some(ref text) = text {
            if let Some(ref cb) = self.clipboard_write {
                let _ = cb(text);
                return true;
            }
        }
        false
    }

    /// Handle a click on the terminal content area using a
    /// `TerminalSplitHit` from the cached layout. Sets pane focus,
    /// starts selection or forwards press, or signals a divider drag.
    /// Returns `true` if the caller should start a split-divider drag.
    ///
    /// `button` and `mods` are forwarded to
    /// [`handle_terminal_pane_press`](Self::handle_terminal_pane_press)
    /// for the pane-hit branches.
    pub fn handle_terminal_split_click(
        &mut self,
        hit: quadraui::TerminalSplitHit,
        button: quadraui::MouseButton,
        mods: quadraui::Modifiers,
    ) -> bool {
        use quadraui::TerminalSplitHit;
        self.terminal_has_focus = true;
        match hit {
            TerminalSplitHit::Divider => true,
            TerminalSplitHit::LeftPane { col, row } => {
                self.terminal_active = 0;
                self.handle_terminal_pane_press(col, row, button, mods);
                false
            }
            TerminalSplitHit::RightPane { col, row } => {
                self.terminal_active = 1;
                self.handle_terminal_pane_press(col, row, button, mods);
                false
            }
            TerminalSplitHit::Scrollbar | TerminalSplitHit::Outside => false,
        }
    }

    /// Dispatch a click on the bottom panel tab bar using the cached
    /// `TabBarHits` from the last paint. Returns `true` if the click
    /// was consumed (tab switch or panel close).
    pub fn handle_bottom_tab_bar_click(&mut self, click_x: f64) -> bool {
        enum Action {
            Close,
            Switch(BottomPanelKind),
            None,
        }
        let action = {
            let hits = self.bottom_tab_bar_hits.borrow();
            let Some(ref hits) = *hits else {
                return false;
            };
            if hits
                .right_segment_bounds
                .first()
                .is_some_and(|&(sx, ex)| click_x >= sx && click_x < ex)
            {
                Action::Close
            } else {
                let mut kinds = Vec::new();
                if self.terminal_open {
                    kinds.push(BottomPanelKind::Terminal);
                }
                if !self.dap_output_lines.is_empty() {
                    kinds.push(BottomPanelKind::DebugOutput);
                }
                hits.slot_positions
                    .iter()
                    .enumerate()
                    .find(|(_, &(sx, ex))| click_x >= sx && click_x < ex)
                    .and_then(|(idx, _)| kinds.get(idx).cloned())
                    .map_or(Action::None, Action::Switch)
            }
        };
        match action {
            Action::Close => {
                self.bottom_panel_open = false;
                self.close_terminal();
                true
            }
            Action::Switch(kind) => {
                self.bottom_panel_kind = kind;
                true
            }
            Action::None => false,
        }
    }

    /// Resolve a terminal toolbar click to an action using cached hit data.
    /// Both TUI (cell columns) and GTK (pixel positions) pass screen-absolute
    /// coordinates; the method accounts for coordinate-system differences
    /// between `StatusBarLayout` (bar-relative) and `TabBarHits` (absolute).
    pub fn resolve_terminal_toolbar_click(&self, click_x: f64) -> TerminalToolbarAction {
        let hits = self.terminal_toolbar_hits.borrow();
        let Some(ref hits) = *hits else {
            return TerminalToolbarAction::None;
        };
        match hits {
            TerminalToolbarHits::FindBar { layout, origin_x } => {
                let rel_x = click_x - origin_x;
                match layout.hit_test(rel_x as f32, 0.0) {
                    quadraui::StatusBarHit::Segment(id)
                        if id.as_str() == "term_toolbar:find_close" =>
                    {
                        TerminalToolbarAction::CloseFindBar
                    }
                    _ => TerminalToolbarAction::None,
                }
            }
            TerminalToolbarHits::TabStrip(hits) => {
                for (i, &(sx, ex)) in hits.right_segment_bounds.iter().enumerate() {
                    if click_x >= sx && click_x < ex {
                        return match i {
                            0 => TerminalToolbarAction::AddTab,
                            1 => TerminalToolbarAction::ToggleSplit,
                            2 => TerminalToolbarAction::ToggleMaximize,
                            3 => TerminalToolbarAction::CloseTab,
                            _ => TerminalToolbarAction::None,
                        };
                    }
                }
                for (idx, &(sx, ex)) in hits.slot_positions.iter().enumerate() {
                    if click_x >= sx && click_x < ex && sx < ex {
                        return TerminalToolbarAction::SwitchTab(idx);
                    }
                }
                TerminalToolbarAction::StartResize
            }
        }
    }

    /// Execute a terminal toolbar action. Returns `false` for `StartResize`
    /// (backend-local drag state) and `None`; returns `true` for all other
    /// actions handled internally.
    pub fn execute_terminal_toolbar_action(
        &mut self,
        action: TerminalToolbarAction,
        ctx: UiEventContext,
    ) -> bool {
        match action {
            TerminalToolbarAction::SwitchTab(idx) => self.terminal_switch_tab(idx),
            TerminalToolbarAction::CloseTab => self.terminal_close_active_tab(),
            TerminalToolbarAction::ToggleMaximize => {
                self.toggle_terminal_maximize();
                let effective = self.effective_terminal_panel_rows(ctx.terminal_max_rows);
                if self.terminal_panes.is_empty() {
                    self.terminal_new_tab(ctx.terminal_cols, effective);
                } else {
                    self.terminal_resize(ctx.terminal_cols, effective);
                }
            }
            TerminalToolbarAction::ToggleSplit => {
                let rows = self.session.terminal_panel_rows;
                self.terminal_toggle_split(ctx.terminal_cols, rows);
            }
            TerminalToolbarAction::AddTab => {
                let rows = self.session.terminal_panel_rows;
                self.terminal_new_tab(ctx.terminal_cols, rows);
            }
            TerminalToolbarAction::CloseFindBar => {
                self.terminal_find_active = false;
            }
            TerminalToolbarAction::StartResize | TerminalToolbarAction::None => return false,
        }
        true
    }

    /// Toggle "terminal maximized" state.
    ///
    /// This only flips `terminal_maximized`; the stored user-preferred panel
    /// height (`session.terminal_panel_rows`) is left untouched. Each
    /// backend's layout code is responsible for asking
    /// [`Engine::effective_terminal_panel_rows`] on every frame, so window
    /// resizes automatically re-derive the maximized panel size without any
    /// re-trigger from the keybinding / click handlers.
    ///
    /// Opens the terminal panel if it's not already visible, and grabs focus
    /// on maximize.
    pub fn toggle_terminal_maximize(&mut self) {
        if self.terminal_maximized {
            self.terminal_maximized = false;
        } else {
            self.terminal_open = true;
            self.terminal_has_focus = true;
            self.terminal_maximized = true;
        }
    }

    /// Return the effective content-row count for the terminal panel: either
    /// the maximized target (backend-computed `max_target_rows`) when the
    /// maximize flag is set, or the user-preferred `session.terminal_panel_rows`.
    ///
    /// Backends call this **every frame** during layout, after they've
    /// computed how many rows the panel could take given current window
    /// dimensions. That's what makes window-resize handling automatic.
    pub fn effective_terminal_panel_rows(&self, max_target_rows: u16) -> u16 {
        if self.terminal_maximized {
            max_target_rows.max(self.session.terminal_panel_rows).max(5)
        } else {
            self.session.terminal_panel_rows
        }
    }

    /// Toggle the integrated terminal:
    /// - If open and focused → close (hide)
    /// - If open but unfocused → give focus
    /// - If not open → signal UI to open (UI calls terminal_new_tab with correct dimensions)
    ///
    /// Also closes the debug output bottom panel if it is the only thing keeping
    /// the bottom panel visible (no terminal running).
    pub fn toggle_terminal(&mut self) {
        if self.terminal_open && self.terminal_has_focus {
            self.close_terminal();
            // Also close debug output panel if no terminal remains
            if self.bottom_panel_open && !self.terminal_open {
                self.bottom_panel_open = false;
            }
        } else if self.terminal_open {
            self.terminal_has_focus = true;
        } else if self.bottom_panel_open {
            // No terminal but debug output panel is open — close it
            self.bottom_panel_open = false;
        } else {
            // Signal UI to call terminal_new_tab with correct dimensions
            self.terminal_open = true;
            self.terminal_has_focus = true;
        }
    }

    /// Drain PTY output from all sessions and update VT100 screens.
    /// Returns true if a redraw is needed.
    /// Exited sessions are automatically removed; closes the panel when the last one exits.
    pub fn poll_terminal(&mut self) -> bool {
        let mut got_data = false;
        for slot in &mut self.terminal_panes {
            got_data |= slot.session.poll();
        }
        // Remove exited sessions in reverse order (preserves earlier indices during removal).
        // For install panes, finalize the install (check binary, register LSP) before removing.
        let mut i = self.terminal_panes.len();
        while i > 0 {
            i -= 1;
            if self.terminal_panes[i].session.is_exited() {
                let ctx = self.terminal_panes[i].install_ctx.take();
                if let Some(ctx) = ctx {
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
        // Clear the "Installing …" spinner notification.
        self.notify_done_by_kind(&NotificationKind::LspInstall, None);

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
                        ..Default::default()
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
        for slot in &mut self.terminal_panes {
            slot.session.resize(cols, rows);
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

    /// Route a scroll-wheel notch from a raw `UiEvent::Scroll` delta.
    ///
    /// Both the TUI and GTK backends emit `UiEvent::Scroll { delta, .. }`
    /// through `quadraui::dispatch_scroll`, where the canonical sign is:
    ///
    /// - `delta_y < 0` → scroll **up** into history
    /// - `delta_y > 0` → scroll **down** toward the live view
    ///
    /// This matches the convention in `examples/common/terminal_app.rs` and
    /// the GTK `EventControllerScroll` / TUI `crossterm::ScrollUp` (which
    /// vimcode maps to `delta_y = -1.0`).
    ///
    /// A step of `ceil(|delta_y| × 3)` rows mirrors the example app (3 rows
    /// per notch).  The forward-vs-scroll policy is delegated to
    /// [`terminal_wheel`](Self::terminal_wheel) which calls
    /// `TerminalSession::forward_mouse` / `scroll_up` / `scroll_down`.
    ///
    /// Backends call this in **one line** from their
    /// `"terminal_scrollback"` dispatch arm — no per-backend step
    /// computation, sign reversal, or forwarding logic needed.  When
    /// quadraui ships `TerminalSession::handle_wheel` (quadraui#365) this
    /// method will thin further to a single delegation.
    pub fn handle_terminal_scroll(&mut self, delta_y: f32) {
        if delta_y == 0.0 {
            return;
        }
        let step = (delta_y.abs() * 3.0).ceil() as usize;
        let up = delta_y < 0.0;
        self.terminal_wheel(up, step);
    }

    /// Route a mouse-wheel notch for the active terminal pane: forward it to
    /// the child when the child owns the wheel (alt-screen / mouse reporting),
    /// otherwise scroll local scrollback by `step` rows.
    ///
    /// Called by [`handle_terminal_scroll`](Self::handle_terminal_scroll)
    /// which backends should prefer.  Direct callers pass a pre-computed
    /// step count — use this when you already have `up`/`step` (e.g. tests).
    ///
    /// Mirrors quadraui's `examples/common/terminal_app.rs` scroll handler,
    /// which composes `forward_mouse()` + `scroll_up/down` the same way.
    /// The longer-term goal (quadraui#365) is to lift this into
    /// `TerminalSession::handle_wheel`.
    pub fn terminal_wheel(&mut self, up: bool, step: usize) {
        if !self.terminal_forward_wheel(up) {
            if up {
                self.terminal_scroll_up(step);
            } else {
                self.terminal_scroll_down(step);
            }
        }
    }

    /// Forward a mouse-wheel notch to the active pane's child process when it
    /// owns the alternate screen or has enabled mouse reporting (vim, less,
    /// tmux, claude, …). Returns `true` when the wheel was written to the
    /// child — in that case the caller MUST NOT scroll local scrollback.
    /// Returns `false` for an ordinary shell (primary screen, no mouse
    /// reporting), where the caller falls back to
    /// [`terminal_scroll_up`](Self::terminal_scroll_up) /
    /// [`terminal_scroll_down`](Self::terminal_scroll_down).
    ///
    /// quadraui gates this through `TerminalSession::should_forward_wheel()`
    /// (#514 stress-test: a stray wheel must never leak the previous command's
    /// output into the shell's scrollback while an app owns the alt-screen).
    ///
    /// Wheel events report at cell `(0, 0)`.  The pointer position for wheels
    /// is not yet passed through — this is tracked as part of the full
    /// `UiEvent` unification (quadraui#365).
    pub fn terminal_forward_wheel(&mut self, up: bool) -> bool {
        use quadraui::terminal_engine::TerminalMouseKind;
        if let Some(term) = self.active_terminal_mut() {
            let kind = if up {
                TerminalMouseKind::WheelUp
            } else {
                TerminalMouseKind::WheelDown
            };
            term.forward_mouse(
                kind,
                quadraui::MouseButton::Left,
                0,
                0,
                quadraui::Modifiers::default(),
            )
        } else {
            false
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
            if let Some(slot) = self.terminal_panes.get_mut(self.terminal_active) {
                slot.session.set_scroll_offset(req_offset);
            }
        }
    }

    /// Go back to the previous match (wraps around) and scroll to it.
    pub fn terminal_find_prev(&mut self) {
        let n = self.terminal_find_matches.len();
        if n > 0 {
            self.terminal_find_selected = (self.terminal_find_selected + n - 1) % n;
            let (req_offset, _, _) = self.terminal_find_matches[self.terminal_find_selected];
            if let Some(slot) = self.terminal_panes.get_mut(self.terminal_active) {
                slot.session.set_scroll_offset(req_offset);
            }
        }
    }

    /// Scan the entire history buffer and the live screen, rebuilding
    /// `terminal_find_matches`.  Case-insensitive.
    ///
    /// Matches are `(required_scroll_offset, row, col)` where:
    /// - History match at history row H: required_offset = `history_len - H`, row = 0.
    ///   Formula: visible_row = row + current_offset − required_offset.
    /// - Live match at screen row R:     required_offset = 0, row = R.
    ///
    /// Sorted oldest-first (highest required_offset first, then top-to-bottom).
    ///
    /// Uses the quadraui `TerminalSession` public API (`scrollback_text()` /
    /// `screen_text()`) to avoid touching private `history` / `parser` fields.
    fn terminal_find_update_matches(&mut self) {
        self.terminal_find_matches.clear();
        if !self.terminal_find_active || self.terminal_find_query.is_empty() {
            return;
        }
        let q_lower: Vec<char> = self.terminal_find_query.to_lowercase().chars().collect();
        let qlen = q_lower.len();
        let active_idx = self.terminal_active;
        let sess = match self.terminal_panes.get(active_idx) {
            Some(slot) => &slot.session,
            None => return,
        };

        let mut matches: Vec<(usize, u16, u16)> = Vec::new();

        // ── History rows via scrollback_text() ──────────────────────────────
        // hist_len is the total ring-buffer size (including any trailing blank rows
        // that scrollback_text() drops). required_offset uses hist_len so that
        // scroll navigation stays correct for all non-blank rows.
        let hist_len = sess.history_len();
        let scrollback = sess.scrollback_text();
        if !scrollback.is_empty() {
            for (hist_idx, hist_line) in scrollback.split('\n').enumerate() {
                let required_offset = hist_len - hist_idx;
                let row_lower: Vec<char> = hist_line
                    .chars()
                    .map(|ch| ch.to_lowercase().next().unwrap_or(ch))
                    .collect();
                if qlen <= row_lower.len() {
                    for c in 0..=(row_lower.len() - qlen) {
                        if row_lower[c..c + qlen] == q_lower[..] {
                            matches.push((required_offset, 0, c as u16));
                        }
                    }
                }
            }
        }

        // ── Live screen rows via screen_text() ──────────────────────────────
        let screen_str = sess.screen_text();
        if !screen_str.is_empty() {
            for (r, line) in screen_str.split('\n').enumerate() {
                let row_lower: Vec<char> = line
                    .chars()
                    .map(|ch| ch.to_lowercase().next().unwrap_or(ch))
                    .collect();
                if qlen <= row_lower.len() {
                    for c in 0..=(row_lower.len() - qlen) {
                        if row_lower[c..c + qlen] == q_lower[..] {
                            matches.push((0, r as u16, c as u16));
                        }
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

    /// Shared terminal key dispatch (#351). The engine decides what a
    /// keypress means; the backend only needs to execute the returned
    /// action (clipboard I/O, PTY write). `key_name` uses the same
    /// canonical names as `handle_key` (e.g. "Return", "Escape", "Up").
    pub fn handle_terminal_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> TerminalKeyAction {
        // Alt+1–9: switch terminal tab.
        if alt && !ctrl && !shift {
            if let Some(ch) = unicode {
                if ch.is_ascii_digit() && ch != '0' {
                    self.terminal_switch_tab((ch as u8 - b'1') as usize);
                    return TerminalKeyAction::Handled;
                }
            }
        }

        // PageUp / PageDown: scroll scrollback.
        if !ctrl && !alt && !shift {
            if key_name == "Page_Up" || key_name == "Prior" {
                self.terminal_scroll_up(12);
                return TerminalKeyAction::Handled;
            }
            if key_name == "Page_Down" || key_name == "Next" {
                self.terminal_scroll_down(12);
                return TerminalKeyAction::Handled;
            }
        }

        // Ctrl+Y or Ctrl+Shift+C: copy selection.
        if ctrl && !alt {
            if let Some(ch) = unicode {
                if (ch == 'y' || ch == 'Y') && !shift {
                    return TerminalKeyAction::CopySelection;
                }
                if (ch == 'c' || ch == 'C') && shift {
                    return TerminalKeyAction::CopySelection;
                }
            }
        }

        // Ctrl+V / Ctrl+Shift+V: paste clipboard.
        if ctrl && !alt {
            if let Some(ch) = unicode {
                if ch == 'v' || ch == 'V' {
                    return TerminalKeyAction::PasteClipboard;
                }
            }
        }

        // Ctrl+F: toggle terminal find bar.
        if ctrl && !shift && !alt {
            if let Some(ch) = unicode {
                if ch == 'f' || ch == 'F' {
                    if self.terminal_find_active {
                        self.terminal_find_close();
                    } else {
                        self.terminal_find_open();
                    }
                    return TerminalKeyAction::Handled;
                }
            }
        }

        // Find bar active: intercept all keys for search navigation.
        if self.terminal_find_active {
            match key_name {
                "Escape" => self.terminal_find_close(),
                "Return" if shift => self.terminal_find_prev(),
                "Return" => self.terminal_find_next(),
                "BackSpace" => self.terminal_find_backspace(),
                _ => {
                    if !ctrl && !alt {
                        if let Some(ch) = unicode {
                            self.terminal_find_char(ch);
                        }
                    }
                }
            }
            return TerminalKeyAction::Handled;
        }

        // Ctrl+W in split mode: switch focus between panes.
        if ctrl && !shift && !alt && self.terminal_split {
            if let Some(ch) = unicode {
                if ch == 'w' || ch == 'W' {
                    self.terminal_split_switch_focus();
                    return TerminalKeyAction::Handled;
                }
            }
        }

        // Any other key: reset scrollback and forward to PTY.
        self.terminal_scroll_reset();
        let data = key_to_pty_bytes(key_name, unicode, ctrl);
        if data.is_empty() {
            TerminalKeyAction::Ignore
        } else {
            TerminalKeyAction::SendToPty(data)
        }
    }
}

/// Build the PTY-injected wrapper script for `terminal_run_command`.
///
/// Wraps `command` in a shell fragment that:
/// 1. Runs the command.
/// 2. Prints a colour-coded success/failure banner.
/// 3. Prints "Press Enter to close…" and waits for the user.
/// 4. **Exits the shell** — without this final `exit` / `Exit`, the interactive
///    shell returns to its PS1 prompt and `TerminalSession::is_exited()` never
///    fires, so `poll_terminal` would never call `finalize_install_from_terminal`
///    and the LSP/DAP server would never be registered.
///
/// Extracted as a pure function so the exit-suffix invariant can be tested
/// without spawning a real PTY.
pub fn build_terminal_install_wrapper(command: &str, is_powershell: bool) -> String {
    if is_powershell {
        format!(
            concat!(
                "{cmd}; ",
                "$__ec = $LASTEXITCODE; ",
                "Write-Host ''; ",
                "if ($__ec -eq 0 -or $null -eq $__ec) {{ ",
                "Write-Host \"`e[32m✓ Command completed successfully`e[0m\" ",
                "}} else {{ ",
                "Write-Host \"`e[31m✗ Command failed (exit code $__ec)`e[0m\" ",
                "}}; ",
                "Write-Host ''; ",
                "Write-Host 'Press Enter to close…'; ",
                "Read-Host; Exit\n"
            ),
            cmd = command
        )
    } else {
        format!(
            "{cmd}\n__exit_code=$?\necho ''\nif [ $__exit_code -eq 0 ]; then echo '\\033[32m✓ Command completed successfully\\033[0m'; else echo \"\\033[31m✗ Command failed (exit code $__exit_code)\\033[0m\"; fi\necho ''\necho 'Press Enter to close…'\nread __dummy\nexit\n",
            cmd = command
        )
    }
}

/// Translate a key event to PTY input bytes. Shared by both backends (#351).
pub fn key_to_pty_bytes(key_name: &str, unicode: Option<char>, ctrl: bool) -> Vec<u8> {
    if ctrl {
        if let Some(ch) = unicode {
            let b = ch as u8;
            if b.is_ascii() {
                return vec![b & 0x1f];
            }
        }
        if key_name.len() == 1 {
            let b = key_name.as_bytes()[0].to_ascii_lowercase();
            if b.is_ascii_lowercase() {
                return vec![b & 0x1f];
            }
        }
        return match key_name {
            "Return" | "KP_Enter" => b"\r".to_vec(),
            "BackSpace" => b"\x7f".to_vec(),
            "Tab" => b"\t".to_vec(),
            _ => vec![],
        };
    }

    match key_name {
        "Return" | "KP_Enter" => b"\r".to_vec(),
        "BackSpace" => b"\x7f".to_vec(),
        "Tab" | "ISO_Left_Tab" => b"\t".to_vec(),
        "Escape" => b"\x1b".to_vec(),
        "Up" | "KP_Up" => b"\x1b[A".to_vec(),
        "Down" | "KP_Down" => b"\x1b[B".to_vec(),
        "Right" | "KP_Right" => b"\x1b[C".to_vec(),
        "Left" | "KP_Left" => b"\x1b[D".to_vec(),
        "Home" | "KP_Home" => b"\x1b[H".to_vec(),
        "End" | "KP_End" => b"\x1b[F".to_vec(),
        "Delete" | "KP_Delete" => b"\x1b[3~".to_vec(),
        "Insert" | "KP_Insert" => b"\x1b[2~".to_vec(),
        "Page_Up" | "KP_Page_Up" | "Prior" => b"\x1b[5~".to_vec(),
        "Page_Down" | "KP_Page_Down" | "Next" => b"\x1b[6~".to_vec(),
        "F1" => b"\x1bOP".to_vec(),
        "F2" => b"\x1bOQ".to_vec(),
        "F3" => b"\x1bOR".to_vec(),
        "F4" => b"\x1bOS".to_vec(),
        "F5" => b"\x1b[15~".to_vec(),
        "F6" => b"\x1b[17~".to_vec(),
        "F7" => b"\x1b[18~".to_vec(),
        "F8" => b"\x1b[19~".to_vec(),
        "F9" => b"\x1b[20~".to_vec(),
        "F10" => b"\x1b[21~".to_vec(),
        "F11" => b"\x1b[23~".to_vec(),
        "F12" => b"\x1b[24~".to_vec(),
        _ => {
            if let Some(ch) = unicode {
                ch.to_string().into_bytes()
            } else {
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build_terminal_install_wrapper;

    /// Verify that the POSIX wrapper ends with `\nexit\n` so the shell process
    /// exits after the user presses Enter, enabling `poll_terminal` to call
    /// `finalize_install_from_terminal` and register the LSP/DAP server.
    #[test]
    fn posix_wrapper_ends_with_exit() {
        let script = build_terminal_install_wrapper("pip install foo", false);
        assert!(
            script.contains("read __dummy\nexit\n"),
            "POSIX wrapper must end with `read __dummy\\nexit\\n` so the shell exits; got:\n{script}"
        );
    }

    /// Verify that the PowerShell wrapper ends with `Read-Host; Exit\n` for the
    /// same reason.
    #[test]
    fn powershell_wrapper_ends_with_exit() {
        let script = build_terminal_install_wrapper("pip install foo", true);
        assert!(
            script.contains("Read-Host; Exit\n"),
            "PowerShell wrapper must end with `Read-Host; Exit\\n` so the shell exits; got:\n{script}"
        );
    }

    /// The command appears verbatim at the start of both wrapper flavours.
    #[test]
    fn wrapper_contains_command() {
        let cmd = "cargo install my-tool";
        let posix = build_terminal_install_wrapper(cmd, false);
        let ps = build_terminal_install_wrapper(cmd, true);
        assert!(
            posix.starts_with(cmd),
            "POSIX wrapper must start with the command"
        );
        assert!(
            ps.starts_with(cmd),
            "PowerShell wrapper must start with the command"
        );
    }
}
