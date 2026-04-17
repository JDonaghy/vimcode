use super::*;

impl Engine {
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
        // Spell suggestion selection intercepts all keys.
        if self.spell_suggestions.is_some() {
            self.handle_spell_suggestion_key(key_name, unicode);
            return EngineAction::None;
        }

        // Clear message on any keypress (unless we're in command/search mode
        // or a dialog is open)
        if self.mode != Mode::Command && self.mode != Mode::Search && self.dialog.is_none() {
            self.message.clear();
        }
        // Dismiss LSP hover popup on any keypress
        self.lsp_hover_text = None;
        // Dismiss editor hover popup and dwell on any keypress (unless it has focus)
        if !self.editor_hover_has_focus {
            self.editor_hover = None;
            self.editor_hover_dwell = None;
        }
        // Dismiss panel hover popup on any keypress (immediate, no delay)
        self.dismiss_panel_hover_now();

        // Record keystroke if macro recording is active
        // Skip recording the 'q' that stops recording
        if self.macro_recording.is_some() {
            let is_stop_q =
                self.mode == Mode::Normal && unicode == Some('q') && self.pending_key.is_none();

            if !is_stop_q {
                // Encode the keystroke for recording
                let encoded = self.encode_key_for_macro(key_name, unicode, ctrl);
                for ch in encoded.chars() {
                    self.recording_buffer.push(ch);
                }
            }
        }

        // Modal dialog intercepts all keys.
        if self.dialog.is_some() {
            // Capture the input value before the dialog key handler clears it.
            let input_value = self
                .dialog
                .as_ref()
                .and_then(|d| d.input.as_ref())
                .map(|i| i.value.clone());
            if let Some((tag, action)) = self.handle_dialog_key(key_name, unicode) {
                return self.process_dialog_result(&tag, &action, input_value.as_deref());
            }
            return EngineAction::None;
        }

        // Context menu intercepts all keys when open.
        if self.context_menu.is_some() {
            let (consumed, _action) = self.handle_context_menu_key(key_name);
            if consumed {
                return EngineAction::None;
            }
        }

        // Ctrl+Tab opens the tab switcher (or cycles forward if already open).
        if ctrl && key_name == "Tab" {
            if self.tab_switcher_open {
                let len = self.tab_mru.len();
                if len > 0 {
                    self.tab_switcher_selected = (self.tab_switcher_selected + 1) % len;
                }
            } else {
                self.open_tab_switcher();
            }
            return EngineAction::None;
        }
        // Ctrl+Shift+Tab cycles backward.
        if ctrl && key_name == "ISO_Left_Tab" {
            if self.tab_switcher_open {
                let len = self.tab_mru.len();
                if len > 0 {
                    self.tab_switcher_selected = if self.tab_switcher_selected == 0 {
                        len - 1
                    } else {
                        self.tab_switcher_selected - 1
                    };
                }
            } else {
                self.open_tab_switcher();
                let len = self.tab_mru.len();
                if len > 0 {
                    self.tab_switcher_selected = len - 1;
                }
            }
            return EngineAction::None;
        }

        // Tab switcher intercepts all keys when open.
        if self.tab_switcher_open {
            match key_name {
                "Tab" => {
                    // Tab (no ctrl): cycle forward
                    let len = self.tab_mru.len();
                    if len > 0 {
                        self.tab_switcher_selected = (self.tab_switcher_selected + 1) % len;
                    }
                    return EngineAction::None;
                }
                "ISO_Left_Tab" => {
                    // Shift-Tab: cycle backward
                    let len = self.tab_mru.len();
                    if len > 0 {
                        self.tab_switcher_selected = if self.tab_switcher_selected == 0 {
                            len - 1
                        } else {
                            self.tab_switcher_selected - 1
                        };
                    }
                    return EngineAction::None;
                }
                "Escape" => {
                    self.tab_switcher_open = false;
                    return EngineAction::None;
                }
                "Return" => {
                    self.tab_switcher_confirm();
                    return EngineAction::None;
                }
                _ => {
                    // Any other key confirms and is consumed
                    self.tab_switcher_confirm();
                    return EngineAction::None;
                }
            }
        }

        // Find/replace overlay intercepts all keys when open.
        if self.find_replace_open {
            self.handle_find_replace_key(key_name, unicode, ctrl, false);
            return EngineAction::None;
        }

        // Unified picker intercepts all keys when open.
        if self.picker_open {
            return self.handle_picker_key(key_name, unicode, ctrl);
        }

        // Breadcrumb focus mode intercepts keys when active.
        if self.breadcrumb_focus {
            return self.handle_breadcrumb_key(key_name, unicode, ctrl);
        }

        // Diff peek popup intercepts keys when open.
        if self.diff_peek.is_some() && self.handle_diff_peek_key(key_name, unicode) {
            return EngineAction::None;
        }

        // Editor hover popup intercepts keys when it has focus.
        if self.editor_hover_has_focus {
            // Use unicode char for printable keys (TUI sends key_name="" for them)
            let hover_key = if key_name.is_empty() {
                unicode.map(|c| c.to_string()).unwrap_or_default()
            } else {
                key_name.to_string()
            };
            self.handle_editor_hover_key(&hover_key, ctrl);
            return EngineAction::None;
        }

        // Quickfix panel intercepts all keys when it has focus.
        if self.quickfix_has_focus {
            return self.handle_quickfix_key(key_name, ctrl);
        }

        // Debug sidebar intercepts all keys when it has focus.
        if self.dap_sidebar_has_focus {
            return self.handle_debug_sidebar_key(key_name, ctrl);
        }

        // Extension panel input field intercepts keys when active.
        if self.ext_panel_has_focus && self.ext_panel_input_active {
            self.handle_ext_panel_input_key(key_name, ctrl, unicode);
            return EngineAction::None;
        }

        // Extension panel intercepts all keys when it has focus.
        if self.ext_panel_has_focus {
            self.handle_ext_panel_key(key_name, ctrl, unicode);
            return EngineAction::None;
        }

        // Source Control panel intercepts all keys when it has focus.
        if self.sc_has_focus {
            self.handle_sc_key(key_name, ctrl, unicode);
            return EngineAction::None;
        }

        // Settings panel intercepts all keys when it has focus.
        if self.settings_has_focus {
            self.handle_settings_key(key_name, ctrl, unicode);
            return EngineAction::None;
        }

        // Explorer and Search panels intercept all keys when focused.
        // Key handling is done by the UI backend; engine just blocks normal processing.
        if self.explorer_has_focus || self.search_has_focus {
            return EngineAction::None;
        }

        // Ctrl-S: save in any mode (does not change mode).
        if ctrl && key_name == "s" {
            if let Err(e) = self.save_with_format(false) {
                self.message = format!("Save failed: {}", e);
            }
            return EngineAction::None;
        }

        // VSCode mode: all keys go through the vscode handler, which internally
        // delegates to handle_command_key() / handle_search_key() when those overlays
        // are open (e.g. after F1 opens the command bar).
        if self.is_vscode_mode() {
            return self.handle_vscode_key(key_name, unicode, ctrl);
        }

        let mut changed = false;
        let mut action = EngineAction::None;
        let was_normal = matches!(self.mode, Mode::Normal);

        // User-defined keymaps from settings (checked before built-in handlers).
        // Skip in Command/Search modes where input goes to the command line,
        // and skip when a panel has focus (those already intercepted above).
        if !matches!(self.mode, Mode::Command | Mode::Search) && !self.user_keymaps.is_empty() {
            if let Some(km_action) = self.try_user_keymap(key_name, unicode, ctrl, &mut changed) {
                if changed {
                    self.set_dirty(true);
                }
                return km_action;
            }
        }

        // N-to-dismiss extension hint: intercept 'N' in Normal mode when a hint is visible.
        // Only active while the hint is still the current status message (cleared on any edit).
        if let Some(ref name) = self.ext_hint_pending_name.clone() {
            if !self.message.contains(name.as_str()) {
                // Message was overwritten — forget the pending name silently.
                self.ext_hint_pending_name = None;
            } else if key_name == "N"
                && !ctrl
                && matches!(self.mode, Mode::Normal)
                && self.pending_key.is_none()
                && self.pending_operator.is_none()
            {
                let name = self.ext_hint_pending_name.take().unwrap();
                self.extension_state.mark_dismissed(&name);
                let _ = self.extension_state.save();
                self.message =
                    format!("Extension '{name}' dismissed — :ExtEnable {name} to re-enable");
                return EngineAction::None;
            }
        }

        // Safety: dismiss completion popup if it's visible outside Insert mode.
        // This can happen if a late-arriving LSP response set it after mode change.
        if self.completion_idx.is_some() && self.mode != Mode::Insert && !self.is_vscode_mode() {
            self.dismiss_completion();
        }

        // Capture cursor position before dispatching (used by cursor_move hook below).
        let pre_cursor_line = self.cursor().line;
        let pre_cursor_col = self.cursor().col;
        let pre_mode = self.mode;

        // Ctrl+F: open find/replace from any mode (Visual captures the selection)
        if ctrl
            && key_name == "f"
            && self.settings.ctrl_f_action == "find"
            && matches!(
                self.mode,
                Mode::Visual | Mode::VisualLine | Mode::VisualBlock | Mode::Insert
            )
        {
            self.open_find_replace();
            return EngineAction::None;
        }

        match self.mode {
            Mode::Normal => {
                action = self.handle_normal_key(key_name, unicode, ctrl, &mut changed);
                // Ctrl-O auto-return: after one Normal command, return to Insert.
                // Only if we're still in Normal mode (the command didn't change mode itself)
                // and no pending key/operator is waiting for more input.
                if self.insert_ctrl_o_active {
                    if self.mode == Mode::Normal
                        && self.pending_key.is_none()
                        && self.pending_operator.is_none()
                    {
                        self.mode = Mode::Insert;
                        self.start_undo_group();
                        self.insert_ctrl_o_active = false;
                    } else if self.mode != Mode::Normal {
                        // Command changed mode (e.g. entered Insert via i/a/o) — clear flag
                        self.insert_ctrl_o_active = false;
                    }
                    // If pending_key/operator is set, keep flag active for next iteration
                }
            }
            Mode::Insert => {
                self.handle_insert_key(key_name, unicode, ctrl, &mut changed);
            }
            Mode::Replace => {
                self.handle_replace_key(key_name, unicode, ctrl, &mut changed);
            }
            Mode::Command => {
                action = self.handle_command_key(key_name, unicode, ctrl);
            }
            Mode::Search => {
                self.handle_search_key(key_name, unicode, ctrl);
            }
            Mode::Visual | Mode::VisualLine | Mode::VisualBlock => {
                // Save pre-call visual state for 'gv' support
                let pre_mode = self.mode;
                let pre_anchor = self.visual_anchor;
                let pre_cursor = self.view().cursor;
                action = self.handle_visual_key(key_name, unicode, ctrl, &mut changed);
                // If we just left visual mode, record the selection for gv and '< / '>
                if !matches!(
                    self.mode,
                    Mode::Visual | Mode::VisualLine | Mode::VisualBlock
                ) {
                    self.last_visual_mode = pre_mode;
                    self.last_visual_anchor = pre_anchor;
                    self.last_visual_cursor = Some(pre_cursor);
                    // Save '< and '> marks (sorted)
                    if let Some(anchor) = pre_anchor {
                        let (start, end) =
                            if (anchor.line, anchor.col) <= (pre_cursor.line, pre_cursor.col) {
                                (anchor, pre_cursor)
                            } else {
                                (pre_cursor, anchor)
                            };
                        self.visual_mark_start = Some((start.line, start.col));
                        self.visual_mark_end = Some((end.line, end.col));
                    }
                }
            }
        }

        // Track where insert mode was entered (for Ctrl-U boundary)
        if !matches!(pre_mode, Mode::Insert) && self.mode == Mode::Insert {
            self.insert_enter_col = self.view().cursor.col;
        }

        if changed {
            let _t0 = std::time::Instant::now();

            // Track last edit position for '. mark and change list
            let cur = self.view().cursor;
            self.last_edit_pos = Some((cur.line, cur.col));
            self.push_change_location(cur.line, cur.col);

            // A normal-mode buffer modification (paste, delete, replace…) invalidates
            // any extra-cursor positions — clear them so stale cursors don't appear.
            if was_normal && !self.view().extra_cursors.is_empty() {
                self.view_mut().extra_cursors.clear();
            }
            self.set_dirty(true);

            let t1 = std::time::Instant::now();
            // Always do a full re-parse + highlight extraction so byte
            // offsets stay correct.  Tree-sitter incremental parsing is fast
            // enough for interactive use; deferring caused garbled colors
            // because stale byte offsets produced partial-word highlighting.
            self.update_syntax();
            let t2 = std::time::Instant::now();

            // Auto-promote preview buffer on text modification
            let active_id = self.active_buffer_id();
            if self.preview_buffer_id == Some(active_id) {
                self.promote_preview(active_id);
            }
            // Mark buffer as needing an LSP didChange (debounced)
            self.lsp_dirty_buffers.insert(active_id, true);

            let t3 = std::time::Instant::now();
            // Live-refresh any linked markdown preview.
            self.refresh_md_previews();
            let t4 = std::time::Instant::now();

            // Mark swap file as needing a write.
            self.swap_mark_dirty();

            let t5 = std::time::Instant::now();
            // Refresh search highlights so they track the new buffer content.
            if !self.search_matches.is_empty() {
                self.run_search();
            }
            let t6 = std::time::Instant::now();

            // Log timing for performance profiling (only when total > 10ms)
            let total = t6.duration_since(_t0);
            if total.as_millis() > 10 {
                self.perf_log = Some(format!(
                    "PERF handle_key changed: syntax={:.1}ms md_preview={:.1}ms search={:.1}ms total={:.1}ms",
                    t2.duration_since(t1).as_secs_f64() * 1000.0,
                    t4.duration_since(t3).as_secs_f64() * 1000.0,
                    t6.duration_since(t5).as_secs_f64() * 1000.0,
                    total.as_secs_f64() * 1000.0,
                ));
            }
        }

        self.ensure_cursor_visible();
        self.sync_scroll_binds();
        self.update_bracket_match();

        // Mark cursor_move as pending when the cursor position changed.
        // The actual plugin hook + code action request are fired by the backend
        // after a debounce delay (idle poll), avoiding expensive work on every
        // keystroke during rapid navigation (e.g. holding j/k).
        if !matches!(self.mode, Mode::Command | Mode::Search | Mode::Insert) {
            let (cur_line, cur_col) = {
                let cur = self.cursor();
                (cur.line, cur.col)
            };
            if cur_line != pre_cursor_line || cur_col != pre_cursor_col {
                self.cursor_move_pending = Some(std::time::Instant::now());
            }
        }

        action
    }

    /// Decode a sequence from the macro playback queue.
    /// Returns (key_name, unicode, ctrl) tuple and the number of characters consumed.
    pub(crate) fn decode_macro_sequence(&mut self) -> Option<(String, Option<char>, bool, usize)> {
        if self.macro_playback_queue.is_empty() {
            return None;
        }

        let first_char = *self.macro_playback_queue.front().unwrap();

        // Check for angle-bracket notation (e.g., <Left>, <C-D>)
        if first_char == '<' {
            // Collect characters until we find '>'
            let mut sequence = String::new();
            let temp_queue: Vec<char> = self.macro_playback_queue.iter().copied().collect();

            for (i, &ch) in temp_queue.iter().enumerate() {
                sequence.push(ch);
                if ch == '>' {
                    // Found complete sequence
                    let len = i + 1;

                    // Parse the sequence
                    if let Some((key_name, unicode, ctrl)) = self.parse_key_sequence(&sequence) {
                        return Some((key_name, unicode, ctrl, len));
                    } else {
                        // Invalid sequence, treat '<' as literal
                        return Some(("".to_string(), Some('<'), false, 1));
                    }
                }
            }

            // No closing '>', treat '<' as literal
            return Some(("".to_string(), Some('<'), false, 1));
        }

        // Handle ESC
        if first_char == '\x1b' {
            return Some(("Escape".to_string(), None, false, 1));
        }

        // Regular character
        Some(("".to_string(), Some(first_char), false, 1))
    }

    /// Parse a key sequence like "<Left>", "<C-D>", "<CR>", etc.
    pub(crate) fn parse_key_sequence(&self, seq: &str) -> Option<(String, Option<char>, bool)> {
        if !seq.starts_with('<') || !seq.ends_with('>') {
            return None;
        }

        let inner = &seq[1..seq.len() - 1];

        // Check for Ctrl combinations: <C-X>
        if inner.starts_with("C-") && inner.len() == 3 {
            let ch = inner.chars().nth(2).unwrap().to_lowercase().next().unwrap();
            return Some((ch.to_string(), Some(ch), true));
        }

        // Special keys
        match inner {
            "CR" => Some(("Return".to_string(), None, false)),
            "BS" => Some(("BackSpace".to_string(), None, false)),
            "Del" => Some(("Delete".to_string(), None, false)),
            "Left" => Some(("Left".to_string(), None, false)),
            "Right" => Some(("Right".to_string(), None, false)),
            "Up" => Some(("Up".to_string(), None, false)),
            "Down" => Some(("Down".to_string(), None, false)),
            "Home" => Some(("Home".to_string(), None, false)),
            "End" => Some(("End".to_string(), None, false)),
            "PageUp" => Some(("Page_Up".to_string(), None, false)),
            "PageDown" => Some(("Page_Down".to_string(), None, false)),
            _ => None,
        }
    }

    /// Advance macro playback by processing the next keystroke in the queue.
    /// Returns true if there are more keys to process.
    pub fn advance_macro_playback(&mut self) -> (bool, EngineAction) {
        // Decode the next key sequence
        if let Some((key_name, unicode, ctrl, consume_count)) = self.decode_macro_sequence() {
            // Remove consumed characters from queue
            for _ in 0..consume_count {
                self.macro_playback_queue.pop_front();
            }

            self.macro_recursion_depth += 1;
            let action = self.handle_key(&key_name, unicode, ctrl);
            self.macro_recursion_depth -= 1;

            // Check if we hit recursion limit
            if self.macro_recursion_depth >= MAX_MACRO_RECURSION {
                self.macro_playback_queue.clear();
                self.message = "Macro recursion limit reached".to_string();
                return (false, EngineAction::Error);
            }

            (!self.macro_playback_queue.is_empty(), action)
        } else {
            (false, EngineAction::None)
        }
    }

    pub(crate) fn handle_normal_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        changed: &mut bool,
    ) -> EngineAction {
        // Read-only guard: block keys that would enter Insert/Replace mode.
        if self.active_buffer_state().read_only {
            let blocked = matches!(
                unicode,
                Some('i' | 'a' | 'o' | 'O' | 'I' | 'A' | 's' | 'S' | 'c' | 'C' | 'R')
            );
            if blocked && self.pending_key.is_none() && self.pending_operator.is_none() {
                self.message = "Buffer is read-only".to_string();
                return EngineAction::None;
            }
        }

        // Netrw: Enter opens entry, - goes to parent directory
        if self.active_buffer_state().netrw_dir.is_some() {
            if key_name == "Return" || key_name == "KP_Enter" {
                return self.netrw_activate_entry();
            }
            if unicode == Some('-') && self.pending_key.is_none() && self.pending_operator.is_none()
            {
                return self.netrw_go_parent();
            }
        }

        // Command-line window: Enter executes, q closes
        if self.active_buffer_state().is_cmdline_buf {
            if key_name == "Return" || key_name == "KP_Enter" {
                return self.cmdline_window_execute();
            }
            if unicode == Some('q') && self.pending_key.is_none() {
                self.close_tab();
                return EngineAction::None;
            }
        }

        // If leader mode is active, route all keypresses there first.
        if self.leader_partial.is_some() {
            return self.handle_leader_key(unicode);
        }

        // Detect leader keypress (non-ctrl, no pending key/operator/find, matches configured leader char).
        if !ctrl
            && self.pending_key.is_none()
            && self.pending_operator.is_none()
            && self.pending_find_operator.is_none()
            && self.pending_text_object.is_none()
            && unicode == Some(self.settings.leader)
        {
            self.leader_partial = Some(String::new());
            return EngineAction::None;
        }

        // Handle Ctrl combinations first
        if ctrl {
            match key_name {
                "d" => {
                    // Half-page down (fold-aware)
                    let count = self.take_count();
                    let half = self.viewport_lines() / 2;
                    let scroll_amount = half * count;
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    let cur = self.view().cursor.line;
                    let new_line = self.view().next_visible_line(cur, scroll_amount, max_line);
                    self.view_mut().cursor.line = new_line;
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "u" => {
                    // Ctrl-U: Half-page up (fold-aware)
                    let count = self.take_count();
                    let half = self.viewport_lines() / 2;
                    let scroll_amount = half * count;
                    let cur = self.view().cursor.line;
                    let new_line = self.view().prev_visible_line(cur, scroll_amount);
                    self.view_mut().cursor.line = new_line;
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "r" => {
                    // Ctrl-R: Redo
                    self.redo();
                    self.refresh_md_previews();
                    return EngineAction::None;
                }
                "f" => {
                    if self.settings.ctrl_f_action == "find" {
                        // Open find/replace overlay
                        self.open_find_replace();
                        return EngineAction::None;
                    }
                    // Full page down (fold-aware)
                    let count = self.take_count();
                    let viewport = self.viewport_lines();
                    let scroll_amount = viewport * count;
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    let cur = self.view().cursor.line;
                    let new_line = self.view().next_visible_line(cur, scroll_amount, max_line);
                    self.view_mut().cursor.line = new_line;
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "b" => {
                    // Full page up (fold-aware)
                    let count = self.take_count();
                    let viewport = self.viewport_lines();
                    let scroll_amount = viewport * count;
                    let cur = self.view().cursor.line;
                    let new_line = self.view().prev_visible_line(cur, scroll_amount);
                    self.view_mut().cursor.line = new_line;
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "w" => {
                    // Ctrl-W prefix for window commands
                    self.pending_key = Some('\x17'); // Ctrl-W marker
                    return EngineAction::None;
                }
                "v" => {
                    if self.pending_operator.is_some() {
                        // Ctrl-V with pending operator: force blockwise motion
                        self.force_motion_mode = Some('\x16');
                        return EngineAction::None;
                    }
                    // Ctrl-V: Enter visual block mode
                    self.mode = Mode::VisualBlock;
                    self.visual_anchor = Some(self.view().cursor);
                    return EngineAction::None;
                }
                "o" => {
                    // Ctrl-O: Jump list back
                    self.jump_list_back();
                    return EngineAction::None;
                }
                "i" => {
                    // Ctrl-I: Jump list forward (same as Tab in many terminals)
                    self.jump_list_forward();
                    return EngineAction::None;
                }
                "p" => {
                    // Ctrl-P: Open unified file picker
                    self.open_picker(PickerSource::Files);
                    return EngineAction::None;
                }
                "P" => {
                    // Ctrl-Shift-P: Open command palette
                    self.open_picker(PickerSource::Commands);
                    return EngineAction::None;
                }
                "g" => {
                    // Ctrl-G: show file info (Vim compat)
                    let name = self
                        .file_path()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "[No Name]".to_string());
                    let modified = if self.dirty() { " [Modified]" } else { "" };
                    let total = self.buffer().len_lines();
                    let cur_line = self.view().cursor.line + 1;
                    let col = self.view().cursor.col + 1;
                    let pct = (cur_line * 100).checked_div(total).unwrap_or(0);
                    self.message = format!(
                        "\"{name}\"{modified} line {cur_line} of {total} --{pct}%-- col {col}"
                    );
                    return EngineAction::None;
                }
                "e" => {
                    // Ctrl-E: scroll down one line (fold-aware, cursor stays)
                    let count = self.take_count();
                    self.scroll_down_visible(count);
                    // Keep cursor visible
                    let viewport = self.viewport_lines();
                    if viewport > 0 && self.view().cursor.line < self.view().scroll_top {
                        self.view_mut().cursor.line = self.view().scroll_top;
                        self.clamp_cursor_col();
                    }
                    return EngineAction::None;
                }
                "y" => {
                    // Ctrl-Y: scroll up one line (fold-aware, cursor stays)
                    let count = self.take_count();
                    self.scroll_up_visible(count);
                    // Keep cursor visible
                    let viewport = self.viewport_lines();
                    if viewport > 0 && self.view().cursor.line >= self.view().scroll_top + viewport
                    {
                        self.view_mut().cursor.line = self.view().scroll_top + viewport - 1;
                        self.clamp_cursor_col();
                    }
                    return EngineAction::None;
                }
                "a" => {
                    // Ctrl-A: increment number under cursor
                    let count = self.take_count();
                    self.increment_number_at_cursor(count as i64, &mut false);
                    return EngineAction::None;
                }
                "x" => {
                    // Ctrl-X: decrement number under cursor
                    let count = self.take_count();
                    self.increment_number_at_cursor(-(count as i64), &mut false);
                    return EngineAction::None;
                }
                "6" => {
                    // Ctrl-^ (Ctrl-6): edit alternate file
                    // Takes priority over Ctrl+6 focus group
                    self.alternate_buffer();
                    return EngineAction::None;
                }
                "l" => {
                    // Ctrl-L: redraw screen (no-op in VimCode, just clear message)
                    self.message.clear();
                    return EngineAction::None;
                }
                "backslash" => {
                    // Ctrl+\: Split editor group to the right (VSCode style)
                    self.open_editor_group(SplitDirection::Vertical);
                    return EngineAction::None;
                }
                "1" | "2" | "3" | "4" | "5" | "7" | "8" | "9" => {
                    // Ctrl+N: Focus group N (1-indexed)
                    let n: usize = key_name.parse().unwrap_or(1);
                    if let Some(gid) = self.group_layout.nth_leaf(n - 1) {
                        self.active_group = gid;
                    }
                    return EngineAction::None;
                }
                _ => {}
            }
        }

        // Handle pending multi-key sequences (gg, dd, Ctrl-W x, gt, r, f, t, m, etc.)
        // MUST come before count accumulation: pending keys like 'r' expect the next
        // character verbatim, including digits (e.g. r1 replaces with '1', not a count).
        if let Some(pending) = self.pending_key.take() {
            return self.handle_pending_key(pending, key_name, unicode, changed);
        }

        // Handle count accumulation (digits 1-9, and 0 when count already exists)
        if let Some(ch) = unicode {
            match ch {
                '1'..='9' => {
                    let digit = ch.to_digit(10).unwrap() as usize;
                    let new_count = self.count.unwrap_or(0) * 10 + digit;
                    if new_count > 10_000 {
                        self.count = Some(10_000);
                        self.message = "Count limited to 10,000".to_string();
                    } else {
                        self.count = Some(new_count);
                    }
                    return EngineAction::None;
                }
                '0' => {
                    if self.count.is_some() {
                        // Accumulate: 10, 20, 100, etc.
                        let new_count = self.count.unwrap() * 10;
                        if new_count > 10_000 {
                            self.count = Some(10_000);
                            self.message = "Count limited to 10,000".to_string();
                        } else {
                            self.count = Some(new_count);
                        }
                        return EngineAction::None;
                    }
                    // Fall through to handle '0' as "go to column 0" below
                }
                _ => {}
            }
        }

        // Handle pending find operator (dfx, dtx, dFx, dTx — waiting for target char)
        if let Some((op, find_type)) = self.pending_find_operator.take() {
            if let Some(target) = unicode {
                self.apply_operator_find_char(op, find_type, target, changed);
            } else {
                self.count = None;
            }
            return EngineAction::None;
        }

        // Handle pending operator + motion (dw, cw, etc.)
        if let Some(op) = self.pending_operator.take() {
            return self.handle_operator_motion(op, key_name, unicode, changed);
        }

        // In normal mode, check the unicode char for vim keys
        match unicode {
            Some('h') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_left();
                }
            }
            Some('j') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_down();
                }
            }
            Some('k') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_up();
                }
            }
            Some('l') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_right();
                }
            }
            Some('i') => {
                self.start_undo_group();
                self.insert_text_buffer.clear();
                self.set_mode(Mode::Insert);
                self.count = None; // Clear count when entering insert mode
            }
            Some('a') => {
                self.start_undo_group();
                self.insert_text_buffer.clear();
                let max_col = self.get_max_cursor_col(self.view().cursor.line);
                if self.view().cursor.col < max_col {
                    self.view_mut().cursor.col += 1;
                } else {
                    let line = self.view().cursor.line;
                    let insert_max = self.get_line_len_for_insert(line);
                    self.view_mut().cursor.col = insert_max;
                }
                self.set_mode(Mode::Insert);
                self.count = None; // Clear count when entering insert mode
            }
            Some('A') => {
                self.start_undo_group();
                self.insert_text_buffer.clear();
                let line = self.view().cursor.line;
                self.view_mut().cursor.col = self.get_line_len_for_insert(line);
                self.set_mode(Mode::Insert);
                self.count = None; // Clear count when entering insert mode
            }
            Some('I') => {
                self.start_undo_group();
                self.insert_text_buffer.clear();
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
                self.count = None; // Clear count when entering insert mode
            }
            Some('o') => {
                let count = self.take_count();
                self.insert_open_count = count;
                self.start_undo_group();
                let line = self.view().cursor.line;
                let indent = self.smart_indent_for_newline(line);
                let indent_len = indent.len();
                let line_end =
                    self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
                let line_content = self.buffer().content.line(line);
                let insert_pos = if self.buffer().line_len_chars(line) > 0 {
                    let len = line_content.len_chars();
                    let last = line_content.char(len - 1);
                    if last == '\n' {
                        // Check for \r\n (CRLF) — skip both chars
                        if len >= 2 && line_content.char(len - 2) == '\r' {
                            line_end - 2
                        } else {
                            line_end - 1
                        }
                    } else if last == '\r' {
                        // Lone \r line ending — insert before it
                        line_end - 1
                    } else {
                        line_end
                    }
                } else {
                    line_end
                };
                // Open one new line (count is handled on Escape via insert_open_count)
                let text = format!("\n{}", indent);
                self.insert_with_undo(insert_pos, &text);
                self.insert_text_buffer.clear();
                self.view_mut().cursor.line += 1;
                self.view_mut().cursor.col = indent_len;
                self.mode = Mode::Insert;
                self.count = None; // Clear count when entering insert mode
                *changed = true;
            }
            Some('O') => {
                let count = self.take_count();
                self.insert_open_count = count;
                self.start_undo_group();
                let line = self.view().cursor.line;
                let indent = if self.settings.auto_indent {
                    self.get_line_indent_str(line)
                } else {
                    String::new()
                };
                let indent_len = indent.len();
                let line_start = self.buffer().line_to_char(line);
                // Open one new line above (count is handled on Escape via insert_open_count)
                let text = format!("{}\n", indent);
                self.insert_with_undo(line_start, &text);
                self.insert_text_buffer.clear();
                self.view_mut().cursor.col = indent_len;
                self.mode = Mode::Insert;
                self.count = None; // Clear count when entering insert mode
                *changed = true;
            }
            Some('0') => self.view_mut().cursor.col = 0,
            Some('^') => {
                // ^ : first non-blank character of line
                let line = self.view().cursor.line;
                self.view_mut().cursor.col = self.first_non_blank_col(line);
            }
            Some('+') => {
                // + : first non-blank of next line (count supported)
                let count = self.take_count();
                let max_line = self.buffer().len_lines().saturating_sub(1);
                let target = (self.view().cursor.line + count).min(max_line);
                self.view_mut().cursor.line = target;
                self.view_mut().cursor.col = self.first_non_blank_col(target);
            }
            Some('-') => {
                // - : first non-blank of previous line (count supported)
                let count = self.take_count();
                let target = self.view().cursor.line.saturating_sub(count);
                self.view_mut().cursor.line = target;
                self.view_mut().cursor.col = self.first_non_blank_col(target);
            }
            Some('_') => {
                // _ : first non-blank of N-1 lines down (1_ = current line)
                let count = self.take_count();
                let max_line = self.buffer().len_lines().saturating_sub(1);
                let target = (self.view().cursor.line + count - 1).min(max_line);
                self.view_mut().cursor.line = target;
                self.view_mut().cursor.col = self.first_non_blank_col(target);
            }
            Some('|') => {
                // | : go to column N (1-indexed, default 1)
                let count = self.take_count();
                let target_col = count.saturating_sub(1);
                let line = self.view().cursor.line;
                let max_col = self.get_max_cursor_col(line);
                self.view_mut().cursor.col = target_col.min(max_col);
            }
            Some('&') => {
                // & : repeat last :s on current line
                if let Some((pattern, replacement, flags)) = self.last_substitute.clone() {
                    let line = self.view().cursor.line;
                    match self.replace_in_range(Some((line, line)), &pattern, &replacement, &flags)
                    {
                        Ok(count) => {
                            self.message = format!(
                                "{} substitution{}",
                                count,
                                if count == 1 { "" } else { "s" }
                            );
                        }
                        Err(e) => {
                            self.message = e;
                        }
                    }
                    *changed = true;
                } else {
                    self.message = "No previous substitute command".to_string();
                }
            }
            Some('$') => {
                let line = self.view().cursor.line;
                self.view_mut().cursor.col = self.get_max_cursor_col(line);
            }
            Some('x') => {
                let count = self.take_count();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let max_col = self.get_max_cursor_col(line);
                if max_col > 0 || self.buffer().line_len_chars(line) > 0 {
                    let char_idx = self.buffer().line_to_char(line) + col;
                    // Calculate how many chars we can actually delete
                    let line_end =
                        self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
                    let available = line_end - char_idx;
                    let to_delete = count.min(available);

                    if to_delete > 0 && char_idx < self.buffer().len_chars() {
                        // Save deleted chars to register (characterwise)
                        let deleted_chars: String = self
                            .buffer()
                            .content
                            .slice(char_idx..char_idx + to_delete)
                            .chars()
                            .collect();
                        let reg = self.active_register();
                        self.set_delete_register(reg, deleted_chars, false);
                        self.clear_selected_register();

                        self.start_undo_group();
                        self.delete_with_undo(char_idx, char_idx + to_delete);
                        self.finish_undo_group();
                        self.clamp_cursor_col();
                        *changed = true;

                        // Record for repeat
                        self.last_change = Some(Change {
                            op: ChangeOp::Delete,
                            text: String::new(),
                            count,
                            motion: Some(Motion::Right),
                        });
                    }
                }
            }
            Some('X') => {
                // X: delete character(s) before cursor (like Backspace in normal mode)
                let count = self.take_count();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                if col > 0 {
                    let to_delete = count.min(col);
                    let line_start = self.buffer().line_to_char(line);
                    let char_idx = line_start + col - to_delete;
                    let deleted_chars: String = self
                        .buffer()
                        .content
                        .slice(char_idx..char_idx + to_delete)
                        .chars()
                        .collect();
                    let reg = self.active_register();
                    self.set_delete_register(reg, deleted_chars, false);
                    self.clear_selected_register();

                    self.start_undo_group();
                    self.delete_with_undo(char_idx, char_idx + to_delete);
                    self.finish_undo_group();
                    self.view_mut().cursor.col = col - to_delete;
                    *changed = true;
                }
            }
            Some('w') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_word_forward();
                }
            }
            Some('W') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_bigword_forward();
                }
            }
            Some('b') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_word_backward();
                }
            }
            Some('B') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_bigword_backward();
                }
            }
            Some('e') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_word_end();
                }
            }
            Some('E') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_bigword_end();
                }
            }
            Some('H') => {
                // H: jump to top of visible screen
                let count = self.take_count().max(1);
                let scroll_top = self.view().scroll_top;
                let viewport = self.viewport_lines();
                let max_line = self.buffer().len_lines().saturating_sub(1);
                let target = (scroll_top + count - 1).min(max_line);
                let target = target.min(scroll_top + viewport.saturating_sub(1));
                self.push_jump_location();
                self.view_mut().cursor.line = target;
                self.clamp_cursor_col();
            }
            Some('M') => {
                // M: jump to middle of visible screen
                let scroll_top = self.view().scroll_top;
                let viewport = self.viewport_lines();
                let max_line = self.buffer().len_lines().saturating_sub(1);
                let mid = scroll_top + viewport / 2;
                self.push_jump_location();
                self.view_mut().cursor.line = mid.min(max_line);
                self.clamp_cursor_col();
            }
            Some('L') => {
                // L: jump to bottom of visible screen
                let count = self.take_count().max(1);
                let scroll_top = self.view().scroll_top;
                let viewport = self.viewport_lines();
                let max_line = self.buffer().len_lines().saturating_sub(1);
                let target_from_bottom = scroll_top + viewport.saturating_sub(count);
                self.push_jump_location();
                self.view_mut().cursor.line = target_from_bottom.min(max_line);
                self.clamp_cursor_col();
            }
            Some('R') => {
                // R: enter Replace mode
                self.start_undo_group();
                self.insert_text_buffer.clear();
                self.mode = Mode::Replace;
                self.count = None;
            }
            Some('(') => {
                // (: backward sentence
                let count = self.take_count();
                self.push_jump_location();
                for _ in 0..count {
                    self.move_sentence_backward();
                }
            }
            Some(')') => {
                // ): forward sentence
                let count = self.take_count();
                self.push_jump_location();
                for _ in 0..count {
                    self.move_sentence_forward();
                }
            }
            Some('f') => {
                self.pending_key = Some('f');
            }
            Some('F') => {
                self.pending_key = Some('F');
            }
            Some('t') => {
                self.pending_key = Some('t');
            }
            Some('T') => {
                self.pending_key = Some('T');
            }
            Some('r') => {
                self.pending_key = Some('r');
            }
            Some(';') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.repeat_find(false);
                }
            }
            Some(',') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.repeat_find(true);
                }
            }
            Some('{') => {
                let count = self.take_count();
                self.push_jump_location();
                for _ in 0..count {
                    self.move_paragraph_backward();
                }
            }
            Some('}') => {
                let count = self.take_count();
                self.push_jump_location();
                for _ in 0..count {
                    self.move_paragraph_forward();
                }
            }
            Some('d') => {
                // 'd' can be both operator (dw) and motion (dd)
                // Save count as operator_count so 2d3w = 6w (count multiplication)
                self.operator_count = self.count.take();
                self.pending_operator = Some('d');
            }
            Some('D') => {
                let count = self.take_count();
                self.start_undo_group();
                // D with count deletes from cursor to end of line, then (count-1) full lines below
                self.delete_to_end_of_line_with_count(count, changed);
                self.finish_undo_group();
            }
            Some('c') => {
                // 'c' operator (change) - delete then enter insert mode
                self.operator_count = self.count.take();
                self.pending_operator = Some('c');
            }
            Some('C') => {
                // C: delete from cursor to end of line, enter insert mode.
                // Save cursor col before delete — clamp_cursor_col will pull
                // it left, but for C we need to insert at the original position.
                let count = self.take_count();
                let saved_col = self.view().cursor.col;
                self.start_undo_group();
                self.delete_to_end_of_line_with_count(count, changed);
                self.view_mut().cursor.col = saved_col;
                self.insert_text_buffer.clear();
                self.mode = Mode::Insert;
                self.count = None;
                // Don't finish_undo_group here - let insert mode do it
            }
            Some('s') => {
                // s: substitute char (delete char under cursor, enter insert mode)
                let count = self.take_count();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let max_col = self.get_max_cursor_col(line);
                if max_col > 0 || self.buffer().line_len_chars(line) > 0 {
                    let char_idx = self.buffer().line_to_char(line) + col;
                    let line_end =
                        self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
                    let available = line_end - char_idx;
                    let to_delete = count.min(available);

                    if to_delete > 0 && char_idx < self.buffer().len_chars() {
                        let deleted_chars: String = self
                            .buffer()
                            .content
                            .slice(char_idx..char_idx + to_delete)
                            .chars()
                            .collect();
                        let reg = self.active_register();
                        self.set_register(reg, deleted_chars, false);
                        self.clear_selected_register();

                        self.start_undo_group();
                        self.delete_with_undo(char_idx, char_idx + to_delete);
                        *changed = true;
                    } else {
                        self.start_undo_group();
                    }
                } else {
                    self.start_undo_group();
                }
                self.insert_text_buffer.clear();
                self.mode = Mode::Insert;
                self.count = None;
            }
            Some('S') => {
                // S: substitute line (delete entire line content, enter insert mode)
                // With auto_indent, preserve the leading whitespace (Vim behavior).
                let count = self.take_count();
                let start_line = self.view().cursor.line;
                let _end_line = (start_line + count).min(self.buffer().len_lines());

                // Capture indent before deletion
                let indent = if self.settings.auto_indent {
                    self.get_line_indent_str(start_line)
                } else {
                    String::new()
                };

                self.start_undo_group();

                // Delete content of lines but keep one line structure
                for i in 0..count {
                    let line_idx = start_line + i;
                    if line_idx >= self.buffer().len_lines() {
                        break;
                    }

                    let line_start = self.buffer().line_to_char(line_idx);
                    let line_len = self.buffer().line_len_chars(line_idx);
                    let line_content = self.buffer().content.line(line_idx);

                    // Calculate what to delete (exclude trailing newline)
                    let delete_end = if line_content.chars().last() == Some('\n') && line_len > 0 {
                        line_start + line_len - 1
                    } else {
                        line_start + line_len
                    };

                    if line_start < delete_end {
                        let deleted: String = self
                            .buffer()
                            .content
                            .slice(line_start..delete_end)
                            .chars()
                            .collect();
                        let reg = self.active_register();
                        self.set_register(reg, deleted, false);
                        self.clear_selected_register();

                        self.delete_with_undo(line_start, delete_end);
                        *changed = true;
                        break; // After first deletion, line indices change
                    }
                }

                // Re-insert preserved indent
                if !indent.is_empty() {
                    let line_start = self.buffer().line_to_char(start_line);
                    self.insert_with_undo(line_start, &indent);
                }

                self.view_mut().cursor.col = indent.len();
                self.insert_text_buffer.clear();
                self.mode = Mode::Insert;
                self.count = None;
            }
            Some('K') => {
                // K: Show editor hover popup at cursor (diagnostics + LSP hover)
                self.trigger_editor_hover_at_cursor();
            }
            Some('g') => {
                self.pending_key = Some('g');
            }
            Some(']') => {
                self.pending_key = Some(']');
            }
            Some('[') => {
                self.pending_key = Some('[');
            }
            Some('z') => {
                // Fold + scroll commands
                self.pending_key = Some('z');
                self.message =
                    "z: a/o/c=fold  M=closeAll  R=openAll  d/D=del  f=create  j/k=nav  z/t/b=scroll  h/l=hscroll"
                        .to_string();
            }
            Some('m') => {
                // Set mark: m{a-z}
                self.pending_key = Some('m');
            }
            Some('\'') => {
                // Jump to mark line: '{a-z}
                self.pending_key = Some('\'');
            }
            Some('`') => {
                // Jump to exact mark position: `{a-z}
                self.pending_key = Some('`');
            }
            Some('G') => {
                self.push_jump_location();
                if self.peek_count().is_some() {
                    // Count provided: go to line N (1-indexed)
                    let count = self.take_count();
                    let target_line = (count - 1).min(self.buffer().len_lines().saturating_sub(1));
                    self.view_mut().cursor.line = target_line;
                } else {
                    // No count: go to last line
                    let last = self.buffer().len_lines().saturating_sub(1);
                    self.view_mut().cursor.line = last;
                }
                self.clamp_cursor_col();
            }
            Some('~') => {
                // Toggle case of char(s) under cursor
                let count = self.take_count();
                self.toggle_case_at_cursor(count, changed);
                self.last_change = Some(Change {
                    op: ChangeOp::ToggleCase,
                    text: String::new(),
                    count,
                    motion: None,
                });
            }
            Some('J') => {
                // Join lines
                let count = self.take_count().max(1);
                self.push_jump_location();
                self.join_lines(count, changed);
                self.last_change = Some(Change {
                    op: ChangeOp::Join,
                    text: String::new(),
                    count,
                    motion: None,
                });
            }
            Some('*') => {
                // Search forward for word under cursor
                let count = self.take_count();
                self.push_jump_location();
                self.search_word_under_cursor(true);
                for _ in 1..count {
                    self.search_next();
                }
            }
            Some('#') => {
                // Search backward for word under cursor
                let count = self.take_count();
                self.push_jump_location();
                self.search_word_under_cursor(false);
                for _ in 1..count {
                    self.search_prev();
                }
            }
            Some('>') => {
                // > operator: set pending for >>
                self.operator_count = self.count.take();
                self.pending_operator = Some('>');
            }
            Some('<') => {
                // < operator: set pending for <<
                self.operator_count = self.count.take();
                self.pending_operator = Some('<');
            }
            Some('=') => {
                // = operator: auto-indent (== indents current line)
                self.operator_count = self.count.take();
                self.pending_operator = Some('=');
            }
            Some('!') => {
                // ! operator: filter through external command
                self.operator_count = self.count.take();
                self.pending_operator = Some('!');
            }
            Some('u') => {
                self.undo();
                self.refresh_md_previews();
            }
            Some('U') => {
                *changed = self.undo_line();
            }
            Some('.') => {
                // Repeat last change
                let count = self.take_count();
                self.repeat_last_change(count, changed);
            }
            Some('y') => {
                self.operator_count = self.count.take();
                self.pending_operator = Some('y');
            }
            Some('Y') => {
                let count = self.take_count();
                self.yank_lines(count);
            }
            Some('p') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.paste_after(changed);
                }
            }
            Some('P') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.paste_before(changed);
                }
            }
            Some('q') => {
                // If already recording, stop recording
                if self.macro_recording.is_some() {
                    self.stop_macro_recording();
                    return EngineAction::None;
                }

                // Otherwise, start pending key for register selection
                self.pending_key = Some('q');
            }
            Some('@') => {
                // Start pending key for register selection (@ + register)
                // @@ is handled in handle_pending_key
                self.pending_key = Some('@');
            }
            Some('"') => {
                self.pending_key = Some('"');
            }
            Some('n') => {
                let count = self.take_count();
                self.push_jump_location();
                for _ in 0..count {
                    match self.search_direction {
                        SearchDirection::Forward => self.search_next(),
                        SearchDirection::Backward => self.search_prev(),
                    }
                }
            }
            Some('N') => {
                let count = self.take_count();
                self.push_jump_location();
                for _ in 0..count {
                    match self.search_direction {
                        SearchDirection::Forward => self.search_prev(),
                        SearchDirection::Backward => self.search_next(),
                    }
                }
            }
            Some('v') => {
                self.set_mode(Mode::Visual);
                self.visual_anchor = Some(self.view().cursor);
            }
            Some('V') => {
                let count = self.take_count();
                self.set_mode(Mode::VisualLine);
                self.visual_anchor = Some(self.view().cursor);
                // Count extends selection: 2V selects 2 lines (current + 1 below)
                if count > 1 {
                    let target = (self.view().cursor.line + count - 1)
                        .min(self.buffer().len_lines().saturating_sub(1));
                    self.view_mut().cursor.line = target;
                }
            }
            Some('%') => {
                let pre_line = self.view().cursor.line;
                if self.peek_count().is_some() {
                    // N% — go to N% of file
                    let pct = self.take_count().min(100);
                    let total = self.buffer().len_lines();
                    let target = if total == 0 {
                        0
                    } else {
                        ((total.saturating_sub(1)) * pct) / 100
                    };
                    self.push_jump_location();
                    self.view_mut().cursor.line = target;
                    let fnb = self.first_non_blank_col(target);
                    self.view_mut().cursor.col = fnb;
                } else {
                    self.push_jump_location();
                    self.move_to_matching_bracket();
                }
                // Center viewport when the match is far from the current view,
                // so the matched brace is clearly visible (like search `n`).
                let post_line = self.view().cursor.line;
                let vp = self.view().viewport_lines;
                if vp > 0 && pre_line.abs_diff(post_line) > vp / 2 {
                    self.scroll_cursor_center();
                }
            }
            Some(':') => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
                self.command_cursor = 0;
                self.count = None; // Clear count when entering command mode
            }
            Some('/') => {
                self.mode = Mode::Search;
                self.command_buffer.clear();
                self.command_cursor = 0;
                self.search_direction = SearchDirection::Forward;
                self.search_start_cursor = Some(self.view().cursor);
                self.search_word_bounded = false; // Clear word-boundary mode
                self.count = None; // Clear count when entering search mode
            }
            Some('?') => {
                self.mode = Mode::Search;
                self.command_buffer.clear();
                self.command_cursor = 0;
                self.search_direction = SearchDirection::Backward;
                self.search_start_cursor = Some(self.view().cursor);
                self.search_word_bounded = false; // Clear word-boundary mode
                self.count = None; // Clear count when entering search mode
            }
            _ => match key_name {
                "Escape" => {
                    // Clear count, pending key, and any extra cursors in normal mode
                    self.count = None;
                    self.operator_count = None;
                    self.pending_key = None;
                    self.view_mut().extra_cursors.clear();
                    // Clear search highlights (like :noh)
                    if !self.search_matches.is_empty() {
                        self.search_matches.clear();
                        self.search_index = None;
                    }
                }
                "Left" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_left();
                    }
                }
                "Down" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_down();
                    }
                }
                "Up" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_up();
                    }
                }
                "Right" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_right();
                    }
                }
                "Home" => self.view_mut().cursor.col = 0,
                "End" => {
                    let line = self.view().cursor.line;
                    self.view_mut().cursor.col = self.get_max_cursor_col(line);
                }
                // Tab = Ctrl-I in terminals (same byte 0x09); both advance the jump list
                "Tab" => self.jump_list_forward(),
                // F1: open command palette
                "F1" => {
                    self.open_picker(PickerSource::Commands);
                    return EngineAction::None;
                }
                // Debug / DAP function keys
                "F5" => {
                    let cmd = if self.dap_session_active {
                        "continue"
                    } else {
                        "debug"
                    };
                    let _ = self.execute_command(cmd);
                }
                "F6" => {
                    let _ = self.execute_command("pause");
                }
                "F9" => {
                    let _ = self.execute_command("brkpt");
                }
                "F10" => {
                    let _ = self.execute_command("stepover");
                }
                "F11" => {
                    let _ = self.execute_command("stepin");
                }
                _ => {}
            },
        }
        EngineAction::None
    }

    pub(crate) fn handle_pending_key(
        &mut self,
        pending: char,
        key_name: &str,
        unicode: Option<char>,
        changed: &mut bool,
    ) -> EngineAction {
        match pending {
            'g' => match unicode {
                Some('g') => {
                    if let Some(op) = self.pending_operator.take() {
                        // dgg/ygg/cgg/zfgg etc: linewise operator to target line
                        self.operator_count = None;
                        let count = self.count.take();
                        let target_line = count
                            .map(|n| {
                                n.saturating_sub(1)
                                    .min(self.buffer().len_lines().saturating_sub(1))
                            })
                            .unwrap_or(0);
                        let current_line = self.view().cursor.line;
                        let (start, end) = if target_line <= current_line {
                            (target_line, current_line)
                        } else {
                            (current_line, target_line)
                        };
                        if op == 'Z' {
                            // zfgg: fold from cursor to target
                            if end > start {
                                self.cmd_fold_create(start, end);
                            }
                        } else {
                            self.apply_linewise_operator(op, start, end, changed);
                        }
                    } else {
                        self.push_jump_location();
                        if self.peek_count().is_some() {
                            let count = self.take_count();
                            let target_line =
                                (count - 1).min(self.buffer().len_lines().saturating_sub(1));
                            self.view_mut().cursor.line = target_line;
                        } else {
                            self.view_mut().cursor.line = 0;
                        }
                        self.view_mut().cursor.col = 0;
                    }
                }
                Some('e') => {
                    if let Some(op) = self.pending_operator.take() {
                        // dge: delete backward to end of previous word (charwise)
                        let count = self.take_count();
                        let start_pos = self.buffer().line_to_char(self.view().cursor.line)
                            + self.view().cursor.col;
                        for _ in 0..count {
                            self.move_word_end_backward();
                        }
                        let end_pos = self.buffer().line_to_char(self.view().cursor.line)
                            + self.view().cursor.col;
                        if end_pos < start_pos {
                            // Include the character at end_pos
                            self.apply_charwise_operator(op, end_pos, start_pos + 1, changed);
                        }
                    } else {
                        let count = self.take_count();
                        for _ in 0..count {
                            self.move_word_end_backward();
                        }
                    }
                }
                Some('E') => {
                    if let Some(op) = self.pending_operator.take() {
                        // dgE: delete backward to end of previous WORD (charwise)
                        let count = self.take_count();
                        let start_pos = self.buffer().line_to_char(self.view().cursor.line)
                            + self.view().cursor.col;
                        for _ in 0..count {
                            self.move_bigword_end_backward();
                        }
                        let end_pos = self.buffer().line_to_char(self.view().cursor.line)
                            + self.view().cursor.col;
                        if end_pos < start_pos {
                            self.apply_charwise_operator(op, end_pos, start_pos + 1, changed);
                        }
                    } else {
                        let count = self.take_count();
                        for _ in 0..count {
                            self.move_bigword_end_backward();
                        }
                    }
                }
                Some('_') => {
                    // g_: last non-blank character of line (count: Nth line below)
                    let count = self.take_count();
                    let line = (self.view().cursor.line + count - 1)
                        .min(self.buffer().len_lines().saturating_sub(1));
                    self.view_mut().cursor.line = line;
                    self.view_mut().cursor.col = self.last_non_blank_col(line);
                }
                Some('*') => {
                    // g*: forward search for word under cursor (no word boundaries)
                    let count = self.take_count();
                    self.push_jump_location();
                    self.search_word_under_cursor_partial(true);
                    for _ in 1..count {
                        self.search_next();
                    }
                }
                Some('#') => {
                    // g#: backward search for word under cursor (no word boundaries)
                    let count = self.take_count();
                    self.push_jump_location();
                    self.search_word_under_cursor_partial(false);
                    for _ in 1..count {
                        self.search_prev();
                    }
                }
                Some('J') => {
                    // gJ: join lines without inserting space
                    let count = self.take_count().max(1);
                    self.push_jump_location();
                    self.join_lines_no_space(count, changed);
                }
                Some('f') => {
                    // gf: open file path under cursor
                    if let Some(path) = self.file_path_under_cursor() {
                        let abs_path = if path.is_absolute() {
                            path
                        } else {
                            self.cwd.join(&path)
                        };
                        return EngineAction::OpenFile(abs_path);
                    } else {
                        self.message = "No file path under cursor".to_string();
                    }
                }
                Some('F') => {
                    // gF: open file under cursor + jump to line number
                    if let Some((path, line_num)) = self.file_path_and_line_under_cursor() {
                        let abs_path = if path.is_absolute() {
                            path
                        } else {
                            self.cwd.join(&path)
                        };
                        match self.open_file_with_mode(
                            &abs_path,
                            crate::core::engine::OpenMode::Permanent,
                        ) {
                            Ok(()) => {
                                if let Some(n) = line_num {
                                    let target = n.saturating_sub(1); // 1-based → 0-based
                                    let max = self.buffer().len_lines().saturating_sub(1);
                                    self.view_mut().cursor.line = target.min(max);
                                    let fnb = self.first_non_blank_col(target.min(max));
                                    self.view_mut().cursor.col = fnb;
                                    self.scroll_cursor_center();
                                }
                            }
                            Err(e) => self.message = e,
                        }
                    } else {
                        self.message = "No file path under cursor".to_string();
                    }
                }
                Some('t') => {
                    if let Some(n) = self.count {
                        // Ngt → go to tab N (1-based, like Vim)
                        // Clamp to last tab if N exceeds tab count.
                        let total = self.active_group().tabs.len();
                        let idx = if n >= total {
                            total.saturating_sub(1)
                        } else {
                            n.saturating_sub(1)
                        };
                        self.goto_tab(idx);
                        self.count = None;
                    } else {
                        self.next_tab();
                    }
                }
                Some('T') => {
                    self.prev_tab();
                }
                Some('\t') => {
                    // g<Tab>: jump to last-accessed tab
                    self.goto_last_accessed_tab();
                }
                None if key_name == "Tab" => {
                    // g<Tab> via feed_keys (unicode is None for special keys)
                    self.goto_last_accessed_tab();
                }
                Some('s') => {
                    return self.cmd_git_stage_hunk();
                }
                Some('d') => {
                    self.push_jump_location();
                    self.lsp_request_definition();
                }
                Some('D') => {
                    self.open_diff_peek();
                }
                Some('h') => {
                    // gh: show editor hover popup at cursor position
                    self.trigger_editor_hover_at_cursor();
                }
                Some('r') => {
                    self.push_jump_location();
                    self.lsp_request_references();
                }
                Some('i') => {
                    // gi: go to last insert position and enter Insert mode
                    if let Some((line, col)) = self.last_insert_pos {
                        let max_line = self.buffer().len_lines().saturating_sub(1);
                        let target_line = line.min(max_line);
                        self.view_mut().cursor.line = target_line;
                        let line_len = self.buffer().line_len_chars(target_line);
                        let max_col = if line_len > 0 { line_len - 1 } else { 0 };
                        self.view_mut().cursor.col = col.min(max_col);
                        self.mode = Mode::Insert;
                        self.insert_text_buffer.clear();
                    } else {
                        // No previous insert position: just enter Insert mode
                        self.mode = Mode::Insert;
                        self.insert_text_buffer.clear();
                    }
                }
                Some('y') => {
                    self.push_jump_location();
                    self.lsp_request_type_definition();
                }
                Some('j') => {
                    // gj: move down one visual row (stays in wrapped line when possible)
                    let count = self.take_count().max(1);
                    for _ in 0..count {
                        self.move_visual_down();
                    }
                }
                Some('k') => {
                    // gk: move up one visual row (stays in wrapped line when possible)
                    let count = self.take_count().max(1);
                    for _ in 0..count {
                        self.move_visual_up();
                    }
                }
                Some('0') => {
                    // g0: start of screen line
                    self.move_screen_line_start();
                }
                Some('^') => {
                    // g^: first non-blank on screen line
                    self.move_screen_line_first_non_blank();
                }
                Some('$') => {
                    // g$: end of screen line
                    self.move_screen_line_end();
                }
                Some('~') => {
                    if self.pending_operator == Some('~') {
                        // g~g~: doubled operator — apply linewise (same as g~~)
                        let count = self.take_count();
                        let mut changed = false;
                        self.apply_linewise_operator(
                            '~',
                            self.view().cursor.line,
                            self.view().cursor.line + count.max(1) - 1,
                            &mut changed,
                        );
                        self.pending_operator = None;
                    } else {
                        // g~{motion}: toggle case operator
                        self.operator_count = self.count.take();
                        self.pending_operator = Some('~');
                    }
                }
                Some('u') => {
                    if self.pending_operator == Some('u') {
                        // gugu: doubled operator — apply linewise (same as guu)
                        let count = self.take_count();
                        let mut changed = false;
                        self.apply_linewise_operator(
                            'u',
                            self.view().cursor.line,
                            self.view().cursor.line + count.max(1) - 1,
                            &mut changed,
                        );
                        self.pending_operator = None;
                    } else {
                        // gu{motion}: lowercase operator
                        self.operator_count = self.count.take();
                        self.pending_operator = Some('u');
                    }
                }
                Some('U') => {
                    if self.pending_operator == Some('U') {
                        // gUgU: doubled operator — apply linewise (same as gUU)
                        let count = self.take_count();
                        let mut changed = false;
                        self.apply_linewise_operator(
                            'U',
                            self.view().cursor.line,
                            self.view().cursor.line + count.max(1) - 1,
                            &mut changed,
                        );
                        self.pending_operator = None;
                    } else {
                        // gU{motion}: uppercase operator
                        self.operator_count = self.count.take();
                        self.pending_operator = Some('U');
                    }
                }
                Some('n') => {
                    // gn: enter visual mode selecting next search match
                    // If an operator was saved (cgn), apply it; otherwise just select
                    let op = self.pending_operator.take();
                    self.cmd_gn(op, false, changed);
                }
                Some('N') => {
                    // gN: enter visual mode selecting previous search match
                    let op = self.pending_operator.take();
                    self.cmd_gn(op, true, changed);
                }
                Some('p') => {
                    // gp: paste after, leave cursor after pasted text
                    let count = self.take_count();
                    for _ in 0..count {
                        self.paste_after_cursor_after(changed);
                    }
                }
                Some('P') => {
                    // gP: paste before, leave cursor after pasted text
                    let count = self.take_count();
                    for _ in 0..count {
                        self.paste_before_cursor_after(changed);
                    }
                }
                Some('v') => {
                    // gv: reselect last visual selection
                    if let (Some(anchor), Some(cursor)) =
                        (self.last_visual_anchor, self.last_visual_cursor)
                    {
                        let mode = self.last_visual_mode;
                        self.mode = mode;
                        self.visual_anchor = Some(anchor);
                        self.view_mut().cursor = cursor;
                    }
                }
                Some(';') => {
                    // g;: jump to previous change position
                    if self.change_list.is_empty() {
                        self.message = "Change list is empty".to_string();
                    } else if self.change_list_pos == 0 {
                        self.message = "Already at oldest change".to_string();
                    } else {
                        self.change_list_pos -= 1;
                        let (line, col) = self.change_list[self.change_list_pos];
                        let max_line = self.buffer().len_lines().saturating_sub(1);
                        self.view_mut().cursor.line = line.min(max_line);
                        self.view_mut().cursor.col = col;
                        self.clamp_cursor_col();
                    }
                }
                Some(',') => {
                    // g,: jump to next change position
                    if self.change_list.is_empty() {
                        self.message = "Change list is empty".to_string();
                    } else if self.change_list_pos >= self.change_list.len() {
                        self.message = "Already at newest change".to_string();
                    } else {
                        let (line, col) = self.change_list[self.change_list_pos];
                        let max_line = self.buffer().len_lines().saturating_sub(1);
                        self.view_mut().cursor.line = line.min(max_line);
                        self.view_mut().cursor.col = col;
                        self.clamp_cursor_col();
                        self.change_list_pos =
                            (self.change_list_pos + 1).min(self.change_list.len());
                    }
                }
                Some('m') => {
                    // gm: go to middle of screen line
                    let vp_cols = self.view().viewport_cols;
                    let mid = vp_cols / 2;
                    self.view_mut().cursor.col = mid;
                    self.clamp_cursor_col();
                }
                Some('M') => {
                    // gM: go to middle of text line
                    let line = self.view().cursor.line;
                    let line_len = self.buffer().line_len_chars(line).saturating_sub(1); // exclude newline
                    self.view_mut().cursor.col = line_len / 2;
                    self.clamp_cursor_col();
                }
                Some('a') => {
                    // ga: print ASCII value of character under cursor
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let char_idx = self.buffer().line_to_char(line) + col;
                    if char_idx < self.buffer().content.len_chars() {
                        let ch = self.buffer().content.char(char_idx);
                        let code = ch as u32;
                        self.message =
                            format!("<{}>  {},  Hex {:02x},  Oct {:03o}", ch, code, code, code);
                    }
                }
                Some('8') => {
                    // g8: print UTF-8 byte sequence of character under cursor
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let char_idx = self.buffer().line_to_char(line) + col;
                    if char_idx < self.buffer().content.len_chars() {
                        let ch = self.buffer().content.char(char_idx);
                        let mut buf = [0u8; 4];
                        let encoded = ch.encode_utf8(&mut buf);
                        let hex: Vec<String> =
                            encoded.bytes().map(|b| format!("{:02x}", b)).collect();
                        self.message = hex.join(" ");
                    }
                }
                Some('I') => {
                    // gI: insert at column 0 (unlike I which goes to first non-blank)
                    self.view_mut().cursor.col = 0;
                    self.mode = Mode::Insert;
                    self.insert_text_buffer.clear();
                    self.start_undo_group();
                }
                Some('&') => {
                    // g&: repeat last substitution on all lines
                    if let Some((pat, rep, flags)) = self.last_substitute.clone() {
                        let cmd = format!("%s/{}/{}/{}", pat, rep, flags);
                        self.execute_substitute_command(&cmd);
                        *changed = true;
                    } else {
                        self.message = "No previous substitute command".to_string();
                    }
                }
                Some('+') => {
                    // g+: go to newer text state (chronological)
                    let count = self.take_count();
                    for _ in 0..count {
                        if !self.g_later() {
                            break;
                        }
                    }
                    *changed = true;
                }
                Some('-') => {
                    // g-: go to older text state (chronological)
                    let count = self.take_count();
                    for _ in 0..count {
                        if !self.g_earlier() {
                            break;
                        }
                    }
                    *changed = true;
                }
                Some('o') => {
                    // go: go to byte N in the buffer (1-indexed, like Vim)
                    let byte_n = self.take_count().saturating_sub(1);
                    let char_offset = self.buffer().content.try_byte_to_char(byte_n).unwrap_or(0);
                    let line = self.buffer().content.char_to_line(char_offset);
                    let line_start = self.buffer().line_to_char(line);
                    self.view_mut().cursor.line = line;
                    self.view_mut().cursor.col = char_offset - line_start;
                    self.clamp_cursor_col();
                }
                Some('x') => {
                    // gx: open URL or file path under cursor externally
                    if let Some(url) = self.word_under_cursor() {
                        #[cfg(not(test))]
                        {
                            let _ = std::process::Command::new("xdg-open")
                                .arg(&url)
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .spawn();
                        }
                        self.message = format!("Opening: {}", url);
                    }
                }
                Some('\'') => {
                    // g': jump to mark line WITHOUT adding to jump list
                    self.pending_key = Some('\x07'); // sentinel for g' handler
                }
                Some('`') => {
                    // g`: jump to mark position WITHOUT adding to jump list
                    self.pending_key = Some('\x08'); // sentinel for g` handler
                }
                Some('q') => {
                    // gq{motion}: format text operator
                    self.operator_count = self.count.take();
                    self.pending_operator = Some('q');
                }
                Some('w') => {
                    // gw{motion}: format text, keep cursor
                    self.operator_count = self.count.take();
                    self.pending_operator = Some('Q');
                }
                Some('R') => {
                    // gR: enter Virtual Replace mode (tab-aware overwrite)
                    self.start_undo_group();
                    self.insert_text_buffer.clear();
                    self.virtual_replace = true;
                    self.mode = Mode::Replace;
                    self.count = None;
                }
                Some('?') => {
                    // g?{motion}: ROT13 encode operator
                    self.operator_count = self.count.take();
                    self.pending_operator = Some('R');
                }
                Some('@') => {
                    // g@{motion}: call user-defined operatorfunc
                    self.operator_count = self.count.take();
                    self.pending_operator = Some('@');
                }
                Some('c') => {
                    // gc: commentary — wait for next key (gcc = current line)
                    self.pending_key = Some('\x03'); // sentinel for gc handler
                }
                None if key_name == "Home" => {
                    // g<Home>: same as g0
                    self.move_screen_line_start();
                }
                None if key_name == "End" => {
                    // g<End>: same as g$
                    self.move_screen_line_end();
                }
                _ => {}
            },
            // gc pending: gcc toggles comment on current line (count-aware)
            '\x03' => {
                if let Some('c') = unicode {
                    let count = self.take_count().max(1);
                    let line = self.view().cursor.line + 1; // 1-indexed
                    self.toggle_comment(line, line + count - 1);
                    *changed = true;
                }
            }
            ']' => match unicode {
                Some('c') => self.jump_next_hunk(),
                Some('d') => self.jump_next_diagnostic(),
                Some('s') => self.jump_next_spell_error(),
                Some('p') => {
                    // ]p: paste after with indent matching current line
                    let count = self.take_count();
                    for _ in 0..count {
                        self.paste_after_adjusted_indent(changed);
                    }
                }
                Some(']') => {
                    // ]]: jump to next section ('{' in column 0)
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_section_forward(false);
                    }
                }
                Some('[') => {
                    // ][: jump to next section end ('}' in column 0)
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_section_forward(true);
                    }
                }
                Some('m') => {
                    // ]m: jump to next method start
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_method_start_forward();
                    }
                }
                Some('M') => {
                    // ]M: jump to next method end
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_method_end_forward();
                    }
                }
                Some('}') => {
                    // ]}: jump to next unmatched '}'
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_unmatched_forward('{', '}');
                    }
                }
                Some(')') => {
                    // ]): jump to next unmatched ')'
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_unmatched_forward('(', ')');
                    }
                }
                Some('z') => {
                    // ]z: move to end of current open fold
                    let line = self.view().cursor.line;
                    let folds = &self.view().folds;
                    let mut best = None;
                    for fold in folds {
                        if fold.start <= line && fold.end >= line {
                            match best {
                                None => best = Some(fold.end),
                                Some(prev) => {
                                    // pick the innermost (smallest end)
                                    if fold.end < prev {
                                        best = Some(fold.end);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(end) = best {
                        self.view_mut().cursor.line = end;
                        self.view_mut().cursor.col = 0;
                        self.clamp_cursor_col();
                    }
                }
                Some('*') | Some('/') => {
                    // ]* / ]/: jump to end of comment block (/* ... */)
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_comment_end();
                    }
                }
                Some('#') => {
                    // ]#: jump to next unmatched #else or #endif
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_preproc_forward();
                    }
                }
                _ => {}
            },
            '[' => match unicode {
                Some('c') => self.jump_prev_hunk(),
                Some('d') => self.jump_prev_diagnostic(),
                Some('s') => self.jump_prev_spell_error(),
                Some('p') => {
                    // [p: paste before with indent matching current line
                    let count = self.take_count();
                    for _ in 0..count {
                        self.paste_before_adjusted_indent(changed);
                    }
                }
                Some('[') => {
                    // [[: jump to previous section ('{' in column 0)
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_section_backward(false);
                    }
                }
                Some(']') => {
                    // []: jump to previous section end ('}' in column 0)
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_section_backward(true);
                    }
                }
                Some('m') => {
                    // [m: jump to previous method start
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_method_start_backward();
                    }
                }
                Some('M') => {
                    // [M: jump to previous method end
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_method_end_backward();
                    }
                }
                Some('{') => {
                    // [{: jump to previous unmatched '{'
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_unmatched_backward('{', '}');
                    }
                }
                Some('(') => {
                    // [(: jump to previous unmatched '('
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_unmatched_backward('(', ')');
                    }
                }
                Some('z') => {
                    // [z: move to start of current open fold
                    let line = self.view().cursor.line;
                    let folds = &self.view().folds;
                    let mut best = None;
                    for fold in folds {
                        if fold.start <= line && fold.end >= line {
                            match best {
                                None => best = Some(fold.start),
                                Some(prev) => {
                                    if fold.start > prev {
                                        best = Some(fold.start);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(start) = best {
                        self.view_mut().cursor.line = start;
                        self.view_mut().cursor.col = 0;
                        self.clamp_cursor_col();
                    }
                }
                Some('*') | Some('/') => {
                    // [* / [/: jump to start of comment block (/* ... */)
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_comment_start();
                    }
                }
                Some('#') => {
                    // [#: jump to previous unmatched #if or #else
                    let count = self.take_count();
                    for _ in 0..count {
                        self.jump_preproc_backward();
                    }
                }
                _ => {}
            },
            'd' => {
                // This should not be reached - 'd' is now handled as pending_operator
                // But keep for backward compatibility during transition
                if unicode == Some('d') {
                    let count = self.take_count();
                    self.start_undo_group();
                    self.delete_lines(count, changed);
                    self.finish_undo_group();
                }
            }
            '"' => {
                // Register selection: "x sets selected_register for next operation
                // Uppercase A-Z appends to lowercase register
                if let Some(ch) = unicode {
                    if ch.is_ascii_lowercase()
                        || ch.is_ascii_uppercase()
                        || ch == '"'
                        || ch == '+'
                        || ch == '*'
                        || ch.is_ascii_digit()
                    {
                        self.selected_register = Some(ch);
                    }
                }
            }
            'q' => {
                // Macro recording: q<register>, or command-line window: q: / q/ / q?
                if let Some(ch) = unicode {
                    if ch == ':' {
                        self.open_cmdline_window(false);
                    } else if ch == '/' || ch == '?' {
                        self.open_cmdline_window(true);
                    } else if ch.is_ascii_lowercase() {
                        self.start_macro_recording(ch);
                    } else {
                        self.message = "Invalid register for macro".to_string();
                    }
                }
            }
            '@' => {
                // Macro playback: @<register> or @@ or @:
                if let Some(ch) = unicode {
                    if ch == '@' {
                        // @@ - repeat last macro
                        if let Some(last_reg) = self.last_macro_register {
                            let count = self.take_count();
                            let _ = self.play_macro_with_count(last_reg, count);
                        } else {
                            self.message = "No previous macro".to_string();
                        }
                    } else if ch == ':' {
                        // @: - repeat last ex command
                        if let Some(last_cmd) = self.last_ex_command.clone() {
                            let count = self.take_count();
                            for _ in 0..count {
                                let _ = self.execute_command(&last_cmd);
                            }
                        } else {
                            self.message = "No previous command".to_string();
                        }
                    } else if ch.is_ascii_lowercase() {
                        let count = self.take_count();
                        let _ = self.play_macro_with_count(ch, count);
                    } else {
                        self.message = "Invalid register for macro playback".to_string();
                    }
                }
            }
            'f' | 'F' | 't' | 'T' => {
                // Character find motions
                if let Some(target) = unicode {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.find_char(pending, target);
                    }
                    // Remember this find for ; and , repeat
                    self.last_find = Some((pending, target));
                }
            }
            'r' => {
                // Replace character: r followed by a character replaces char under cursor.
                // Special case: Return/Enter replaces with newline (splits line).
                let replacement = unicode.or_else(|| {
                    if key_name == "Return" {
                        Some('\n')
                    } else {
                        None
                    }
                });
                if let Some(replacement) = replacement {
                    let count = self.take_count();
                    self.start_undo_group();
                    self.replace_chars(replacement, count, changed);
                    self.finish_undo_group();

                    // Record for repeat (.)
                    self.last_change = Some(Change {
                        op: ChangeOp::Replace,
                        text: replacement.to_string(),
                        count,
                        motion: None,
                    });
                }
            }
            '\x17' => {
                // Ctrl-W prefix — delegate to execute_wincmd
                if let Some(ch) = unicode {
                    let count = self.take_count().max(1);
                    return self.execute_wincmd(ch, count);
                } else {
                    // Arrow key fallback for special keys
                    match key_name {
                        "Left" => self.focus_window_direction(SplitDirection::Vertical, false),
                        "Down" => self.focus_window_direction(SplitDirection::Horizontal, true),
                        "Up" => self.focus_window_direction(SplitDirection::Horizontal, false),
                        "Right" => self.focus_window_direction(SplitDirection::Vertical, true),
                        _ => {}
                    }
                }
            }
            'm' => {
                // Set mark: m{a-z} (local) or m{A-Z} (global)
                if let Some(ch) = unicode {
                    if ch.is_ascii_lowercase() {
                        let buffer_id = self.active_window().buffer_id;
                        let cursor = self.view().cursor;
                        self.marks.entry(buffer_id).or_default().insert(ch, cursor);
                        self.message = format!("Mark '{}' set", ch);
                    } else if ch.is_ascii_uppercase() {
                        let file = self.active_buffer_state().file_path.clone();
                        let line = self.view().cursor.line;
                        let col = self.view().cursor.col;
                        self.global_marks.insert(ch, (file, line, col));
                        self.message = format!("Mark '{}' set", ch);
                    } else {
                        self.message = "Marks must be a letter (a-z or A-Z)".to_string();
                    }
                }
            }
            '\'' => {
                // d'{mark} / y'{mark} / c'{mark}: linewise operator to mark
                if let Some(op) = self.pending_operator.take() {
                    if let Some(ch) = unicode {
                        let target_line = if ch.is_ascii_lowercase() {
                            let buffer_id = self.active_window().buffer_id;
                            self.marks
                                .get(&buffer_id)
                                .and_then(|m| m.get(&ch))
                                .map(|c| c.line)
                        } else if ch.is_ascii_uppercase() {
                            self.global_marks.get(&ch).map(|&(_, line, _)| line)
                        } else {
                            None
                        };
                        if let Some(target) = target_line {
                            let current = self.view().cursor.line;
                            let (start, end) = if current <= target {
                                (current, target)
                            } else {
                                (target, current)
                            };
                            self.apply_linewise_operator(op, start, end, changed);
                        } else {
                            self.message = format!("Mark '{}' not set", ch);
                        }
                    }
                    return EngineAction::None;
                }
                // Jump to mark line: '{a-z|A-Z|'|.|<|>}
                if let Some(ch) = unicode {
                    match ch {
                        '\'' => {
                            // '' jump to position before last jump
                            if let Some((line, _)) = self.last_jump_pos {
                                let max_line = self.buffer().len_lines().saturating_sub(1);
                                let target = line.min(max_line);
                                self.view_mut().cursor.line = target;
                                self.view_mut().cursor.col = self.first_non_blank_col(target);
                                self.clamp_cursor_col();
                            } else {
                                self.message = "No previous jump position".to_string();
                            }
                        }
                        '.' => {
                            // '. jump to last edit position
                            if let Some((line, _)) = self.last_edit_pos {
                                let max_line = self.buffer().len_lines().saturating_sub(1);
                                let target = line.min(max_line);
                                self.view_mut().cursor.line = target;
                                self.view_mut().cursor.col = self.first_non_blank_col(target);
                                self.clamp_cursor_col();
                            } else {
                                self.message = "No previous edit position".to_string();
                            }
                        }
                        '<' => {
                            // '< jump to start of last visual selection
                            if let Some((line, _)) = self.visual_mark_start {
                                let max_line = self.buffer().len_lines().saturating_sub(1);
                                let target = line.min(max_line);
                                self.view_mut().cursor.line = target;
                                self.view_mut().cursor.col = self.first_non_blank_col(target);
                                self.clamp_cursor_col();
                            } else {
                                self.message = "No previous visual selection".to_string();
                            }
                        }
                        '>' => {
                            // '> jump to end of last visual selection
                            if let Some((line, _)) = self.visual_mark_end {
                                let max_line = self.buffer().len_lines().saturating_sub(1);
                                let target = line.min(max_line);
                                self.view_mut().cursor.line = target;
                                self.view_mut().cursor.col = self.first_non_blank_col(target);
                                self.clamp_cursor_col();
                            } else {
                                self.message = "No previous visual selection".to_string();
                            }
                        }
                        _ if ch.is_ascii_lowercase() => {
                            let buffer_id = self.active_window().buffer_id;
                            if let Some(buffer_marks) = self.marks.get(&buffer_id) {
                                if let Some(mark_cursor) = buffer_marks.get(&ch) {
                                    let target = mark_cursor.line;
                                    self.view_mut().cursor.line = target;
                                    self.view_mut().cursor.col = self.first_non_blank_col(target);
                                    self.clamp_cursor_col();
                                } else {
                                    self.message = format!("Mark '{}' not set", ch);
                                }
                            } else {
                                self.message = format!("Mark '{}' not set", ch);
                            }
                        }
                        _ if ch.is_ascii_uppercase() => {
                            if let Some(&(_, line, _)) = self.global_marks.get(&ch) {
                                let max_line = self.buffer().len_lines().saturating_sub(1);
                                let target = line.min(max_line);
                                self.view_mut().cursor.line = target;
                                self.view_mut().cursor.col = self.first_non_blank_col(target);
                                self.clamp_cursor_col();
                            } else {
                                self.message = format!("Mark '{}' not set", ch);
                            }
                        }
                        _ => {
                            self.message = "Marks must be a letter or special char".to_string();
                        }
                    }
                }
            }
            '`' => {
                // Jump to exact mark position: `{a-z|A-Z|`|.|<|>}
                if let Some(ch) = unicode {
                    match ch {
                        '`' => {
                            // `` jump to exact position before last jump
                            if let Some((line, col)) = self.last_jump_pos {
                                let max_line = self.buffer().len_lines().saturating_sub(1);
                                self.view_mut().cursor.line = line.min(max_line);
                                self.view_mut().cursor.col = col;
                                self.clamp_cursor_col();
                            } else {
                                self.message = "No previous jump position".to_string();
                            }
                        }
                        '.' => {
                            if let Some((line, col)) = self.last_edit_pos {
                                let max_line = self.buffer().len_lines().saturating_sub(1);
                                self.view_mut().cursor.line = line.min(max_line);
                                self.view_mut().cursor.col = col;
                                self.clamp_cursor_col();
                            } else {
                                self.message = "No previous edit position".to_string();
                            }
                        }
                        '<' => {
                            if let Some((line, col)) = self.visual_mark_start {
                                let max_line = self.buffer().len_lines().saturating_sub(1);
                                self.view_mut().cursor.line = line.min(max_line);
                                self.view_mut().cursor.col = col;
                                self.clamp_cursor_col();
                            } else {
                                self.message = "No previous visual selection".to_string();
                            }
                        }
                        '>' => {
                            if let Some((line, col)) = self.visual_mark_end {
                                let max_line = self.buffer().len_lines().saturating_sub(1);
                                self.view_mut().cursor.line = line.min(max_line);
                                self.view_mut().cursor.col = col;
                                self.clamp_cursor_col();
                            } else {
                                self.message = "No previous visual selection".to_string();
                            }
                        }
                        _ if ch.is_ascii_lowercase() => {
                            let buffer_id = self.active_window().buffer_id;
                            if let Some(buffer_marks) = self.marks.get(&buffer_id) {
                                if let Some(mark_cursor) = buffer_marks.get(&ch) {
                                    self.view_mut().cursor = *mark_cursor;
                                    self.clamp_cursor_col();
                                } else {
                                    self.message = format!("Mark `{}` not set", ch);
                                }
                            } else {
                                self.message = format!("Mark `{}` not set", ch);
                            }
                        }
                        _ if ch.is_ascii_uppercase() => {
                            if let Some(&(_, line, col)) = self.global_marks.get(&ch) {
                                let max_line = self.buffer().len_lines().saturating_sub(1);
                                self.view_mut().cursor.line = line.min(max_line);
                                self.view_mut().cursor.col = col;
                                self.clamp_cursor_col();
                            } else {
                                self.message = format!("Mark `{}` not set", ch);
                            }
                        }
                        _ => {
                            self.message = "Marks must be a letter or special char".to_string();
                        }
                    }
                }
            }
            '\x07' => {
                // g': jump to mark line WITHOUT adding to jump list
                if let Some(ch) = unicode {
                    if ch.is_ascii_lowercase() {
                        let buffer_id = self.active_window().buffer_id;
                        if let Some(buffer_marks) = self.marks.get(&buffer_id) {
                            if let Some(mark_cursor) = buffer_marks.get(&ch) {
                                self.view_mut().cursor.line = mark_cursor.line;
                                self.view_mut().cursor.col = 0;
                                self.clamp_cursor_col();
                            } else {
                                self.message = format!("Mark '{}' not set", ch);
                            }
                        } else {
                            self.message = format!("Mark '{}' not set", ch);
                        }
                    } else if ch.is_ascii_uppercase() {
                        if let Some(&(_, line, _)) = self.global_marks.get(&ch) {
                            let max_line = self.buffer().len_lines().saturating_sub(1);
                            self.view_mut().cursor.line = line.min(max_line);
                            self.view_mut().cursor.col = 0;
                            self.clamp_cursor_col();
                        } else {
                            self.message = format!("Mark '{}' not set", ch);
                        }
                    }
                }
            }
            '\x08' => {
                // g`: jump to mark position WITHOUT adding to jump list
                if let Some(ch) = unicode {
                    if ch.is_ascii_lowercase() {
                        let buffer_id = self.active_window().buffer_id;
                        if let Some(buffer_marks) = self.marks.get(&buffer_id) {
                            if let Some(mark_cursor) = buffer_marks.get(&ch) {
                                self.view_mut().cursor = *mark_cursor;
                                self.clamp_cursor_col();
                            } else {
                                self.message = format!("Mark `{}` not set", ch);
                            }
                        } else {
                            self.message = format!("Mark `{}` not set", ch);
                        }
                    } else if ch.is_ascii_uppercase() {
                        if let Some(&(_, line, col)) = self.global_marks.get(&ch) {
                            let max_line = self.buffer().len_lines().saturating_sub(1);
                            self.view_mut().cursor.line = line.min(max_line);
                            self.view_mut().cursor.col = col;
                            self.clamp_cursor_col();
                        } else {
                            self.message = format!("Mark `{}` not set", ch);
                        }
                    }
                }
            }
            'z' => {
                // Scroll position + fold commands
                match unicode {
                    // Scroll: cursor position
                    Some('z') => self.scroll_cursor_center(),
                    Some('t') => self.scroll_cursor_top(),
                    Some('b') => self.scroll_cursor_bottom(),
                    // Scroll + first non-blank
                    Some('.') => self.scroll_cursor_center_first_nonblank(),
                    Some('-') => self.scroll_cursor_bottom_first_nonblank(),
                    // Horizontal scroll
                    Some('h') => {
                        let count = self.take_count();
                        self.scroll_left_by(count);
                    }
                    Some('l') => {
                        let count = self.take_count();
                        self.scroll_right_by(count);
                    }
                    Some('H') => self.scroll_left_half_screen(),
                    Some('L') => self.scroll_right_half_screen(),
                    Some('e') => {
                        // ze: scroll so cursor is at right edge of screen
                        let col = self.view().cursor.col;
                        let vp_cols = self.view().viewport_cols;
                        self.view_mut().scroll_left = col.saturating_sub(vp_cols.saturating_sub(1));
                    }
                    Some('s') => {
                        // zs: scroll so cursor is at left edge of screen
                        let col = self.view().cursor.col;
                        self.view_mut().scroll_left = col;
                    }
                    // Fold: basic
                    Some('a') => self.cmd_fold_toggle(),
                    Some('o') => self.cmd_fold_open(),
                    Some('c') => self.cmd_fold_close(),
                    Some('R') => {
                        self.view_mut().open_all_folds();
                        if self.diff_unchanged_hidden {
                            self.diff_unchanged_hidden = false;
                        }
                    }
                    Some('M') => self.cmd_fold_close_all(),
                    // Fold: recursive
                    Some('A') => self.cmd_fold_toggle_recursive(),
                    Some('O') => self.cmd_fold_open_recursive(),
                    Some('C') => self.cmd_fold_close_recursive(),
                    // Fold: delete
                    Some('d') => self.cmd_fold_delete(),
                    Some('D') => self.cmd_fold_delete_recursive(),
                    // Fold: create (zf{motion})
                    Some('f') => {
                        self.operator_count = self.count.take();
                        self.pending_operator = Some('Z');
                    }
                    Some('F') => {
                        // zF: create fold for N lines from cursor
                        let count = self.take_count();
                        let line = self.view().cursor.line;
                        let total = self.buffer().len_lines();
                        let end = (line + count).min(total.saturating_sub(1));
                        if end > line {
                            self.cmd_fold_create(line, end);
                        }
                    }
                    // Fold: utilities
                    Some('v') => self.cmd_fold_open_cursor_visible(),
                    Some('x') => self.cmd_fold_recompute(),
                    // Fold: navigation
                    Some('j') => self.cmd_fold_move_next(),
                    Some('k') => self.cmd_fold_move_prev(),
                    // Spell checking
                    Some('=') => self.spell_show_suggestions(),
                    Some('g') => self.spell_add_good_word(),
                    Some('w') => self.spell_mark_wrong(),
                    _ => {
                        // z<CR> — scroll cursor to top + first non-blank
                        if key_name == "Return" {
                            self.scroll_cursor_top_first_nonblank();
                        }
                    }
                }
            }
            _ => {}
        }
        // Try plugin normal-mode keymaps as a fallback
        self.plugin_run_keymap("n", key_name);
        EngineAction::None
    }

    pub(crate) fn handle_operator_motion(
        &mut self,
        operator: char,
        _key_name: &str,
        unicode: Option<char>,
        changed: &mut bool,
    ) -> EngineAction {
        // Handle 'Z' sentinel for zf{motion} — fold creation operator.
        if operator == 'Z' {
            let cursor_line = self.view().cursor.line;
            let total = self.buffer().len_lines();
            let target = match unicode {
                Some('j') => {
                    let count = self.take_count();
                    Some((cursor_line + count).min(total.saturating_sub(1)))
                }
                Some('k') => {
                    let count = self.take_count();
                    Some(cursor_line.saturating_sub(count))
                }
                Some('G') => {
                    self.operator_count = None;
                    let count = self.count.take();
                    Some(
                        count
                            .map(|n| n.saturating_sub(1).min(total.saturating_sub(1)))
                            .unwrap_or_else(|| total.saturating_sub(1)),
                    )
                }
                Some('}') => {
                    let count = self.take_count();
                    let saved = self.view().cursor;
                    for _ in 0..count {
                        self.move_paragraph_forward();
                    }
                    let target = self.view().cursor.line;
                    self.view_mut().cursor = saved;
                    Some(target)
                }
                Some('{') => {
                    let count = self.take_count();
                    let saved = self.view().cursor;
                    for _ in 0..count {
                        self.move_paragraph_backward();
                    }
                    let target = self.view().cursor.line;
                    self.view_mut().cursor = saved;
                    Some(target)
                }
                Some('g') => {
                    // zfg -> wait for 'g' to complete (zfgg)
                    self.pending_key = Some('g');
                    self.pending_operator = Some('Z');
                    return EngineAction::None;
                }
                _ => None,
            };
            if let Some(target_line) = target {
                let start = cursor_line.min(target_line);
                let end = cursor_line.max(target_line);
                if end > start {
                    self.cmd_fold_create(start, end);
                }
            }
            return EngineAction::None;
        }

        // Handle 'g' motion for operator + g{x} (cgn/dgn, dgg, dge, etc.)
        if unicode == Some('g') {
            // Re-set pending_key = 'g' and pending_operator = operator so next key
            // is handled by handle_pending_key('g') which checks pending_operator.
            self.pending_key = Some('g');
            self.pending_operator = Some(operator);
            return EngineAction::None;
        }

        // Handle force motion mode: v/V/CTRL-V while operator is pending.
        // Sets force_motion_mode and re-queues the operator for the next keystroke.
        if unicode == Some('v') || unicode == Some('V') {
            self.force_motion_mode = Some(unicode.unwrap());
            self.pending_operator = Some(operator);
            return EngineAction::None;
        }

        // Handle case-transform operators (~=toggle, u=lower, U=upper)
        if operator == '~' || operator == 'u' || operator == 'U' {
            // Doubled operator (g~~, guu, gUU) = apply to current line
            let is_doubled = match operator {
                '~' => unicode == Some('~'),
                'u' => unicode == Some('u'),
                'U' => unicode == Some('U'),
                _ => false,
            };
            if is_doubled {
                let count = self.take_count();
                let line = self.view().cursor.line;
                let num_lines = self.buffer().len_lines();
                for i in 0..count {
                    let ln = line + i;
                    if ln >= num_lines {
                        break;
                    }
                    let line_start = self.buffer().line_to_char(ln);
                    let line_len = self.buffer().line_len_chars(ln);
                    // Exclude trailing newline
                    let line_end = line_start
                        + if self.buffer().content.line(ln).chars().last() == Some('\n') {
                            line_len.saturating_sub(1)
                        } else {
                            line_len
                        };
                    if line_start < line_end {
                        self.apply_case_range(line_start, line_end, operator, changed);
                    }
                }
            } else if unicode == Some('i') || unicode == Some('a') {
                // Text object: g~iw, guaw, etc.
                self.pending_text_object = unicode;
                self.pending_operator = Some(operator);
            } else if unicode == Some('f')
                || unicode == Some('t')
                || unicode == Some('F')
                || unicode == Some('T')
            {
                self.pending_find_operator = Some((operator, unicode.unwrap()));
            } else if unicode.is_some() {
                // Fall through to common motion dispatch below
            } else {
                self.count = None; // Cancel
                return EngineAction::None;
            }
            if is_doubled
                || unicode == Some('i')
                || unicode == Some('a')
                || unicode == Some('f')
                || unicode == Some('t')
                || unicode == Some('F')
                || unicode == Some('T')
            {
                return EngineAction::None;
            }
            // Fall through to the common motion match block below
        }

        // Handle indent/dedent operators (>> and <<)
        if operator == '>' || operator == '<' {
            let is_doubled = (operator == '>' && unicode == Some('>'))
                || (operator == '<' && unicode == Some('<'));
            if is_doubled {
                let count = self.take_count();
                let line = self.view().cursor.line;
                if operator == '>' {
                    self.indent_lines(line, count, changed);
                    self.last_change = Some(Change {
                        op: ChangeOp::Indent,
                        text: String::new(),
                        count,
                        motion: None,
                    });
                } else {
                    self.dedent_lines(line, count, changed);
                    self.last_change = Some(Change {
                        op: ChangeOp::Dedent,
                        text: String::new(),
                        count,
                        motion: None,
                    });
                }
                return EngineAction::None;
            }
            if unicode == Some('i') || unicode == Some('a') {
                self.pending_text_object = unicode;
                self.pending_operator = Some(operator);
                return EngineAction::None;
            }
            if unicode == Some('f')
                || unicode == Some('t')
                || unicode == Some('F')
                || unicode == Some('T')
            {
                self.pending_find_operator = Some((operator, unicode.unwrap()));
                return EngineAction::None;
            }
            // Fall through to common motion match block below
        }

        // Handle filter operator (!)
        // Vim drops into command mode with `:.,.+N!` pre-filled after resolving the motion.
        if operator == '!' {
            // !! = filter current line(s)
            if unicode == Some('!') {
                let count = self.take_count();
                let line = self.view().cursor.line;
                let end_line = (line + count - 1).min(self.buffer().len_lines().saturating_sub(1));
                // Switch to command mode with range pre-filled
                self.mode = Mode::Command;
                if line == end_line {
                    self.command_buffer = ".!".to_string();
                } else {
                    self.command_buffer = format!(".,{}!", end_line + 1);
                }
                self.command_cursor = self.command_buffer.chars().count();
                return EngineAction::None;
            }
            if unicode == Some('i') || unicode == Some('a') {
                self.pending_text_object = unicode;
                self.pending_operator = Some(operator);
                return EngineAction::None;
            }
            if unicode == Some('f')
                || unicode == Some('t')
                || unicode == Some('F')
                || unicode == Some('T')
            {
                self.pending_find_operator = Some((operator, unicode.unwrap()));
                return EngineAction::None;
            }
            // Fall through to common motion match block below
        }

        // Handle auto-indent operator (=)
        if operator == '=' {
            if unicode == Some('=') {
                let count = self.take_count();
                let line = self.view().cursor.line;
                self.auto_indent_lines(line, count, changed);
                return EngineAction::None;
            }
            if unicode == Some('i') || unicode == Some('a') {
                self.pending_text_object = unicode;
                self.pending_operator = Some(operator);
                return EngineAction::None;
            }
            if unicode == Some('f')
                || unicode == Some('t')
                || unicode == Some('F')
                || unicode == Some('T')
            {
                self.pending_find_operator = Some((operator, unicode.unwrap()));
                return EngineAction::None;
            }
            // Fall through to common motion match block below
        }

        // Handle gq (format text) and gw (format text, keep cursor) operators
        // 'q' = gq, 'Q' = gw
        if operator == 'q' || operator == 'Q' {
            let keep_cursor = operator == 'Q';
            // gqq / gww = format current line(s)
            if (operator == 'q' && unicode == Some('q'))
                || (operator == 'Q' && unicode == Some('w'))
            {
                let count = self.take_count();
                let line = self.view().cursor.line;
                let saved_cursor = self.view().cursor;
                self.format_lines(line, line + count.saturating_sub(1), changed);
                if keep_cursor {
                    self.view_mut().cursor = saved_cursor;
                }
                return EngineAction::None;
            }
            if unicode == Some('i') || unicode == Some('a') {
                self.pending_text_object = unicode;
                self.pending_operator = Some(operator);
                return EngineAction::None;
            }
            if unicode == Some('f')
                || unicode == Some('t')
                || unicode == Some('F')
                || unicode == Some('T')
            {
                self.pending_find_operator = Some((operator, unicode.unwrap()));
                return EngineAction::None;
            }
            // Fall through to common motion match block below
        }

        // Handle g? (ROT13 encode) operator — sentinel 'R'
        if operator == 'R' {
            // g?? = ROT13 current line(s)
            if unicode == Some('?') {
                let count = self.take_count();
                let line = self.view().cursor.line;
                let num_lines = self.buffer().len_lines();
                for i in 0..count {
                    let ln = line + i;
                    if ln >= num_lines {
                        break;
                    }
                    let line_start = self.buffer().line_to_char(ln);
                    let line_len = self.buffer().line_len_chars(ln);
                    let line_end = line_start
                        + if self.buffer().content.line(ln).chars().last() == Some('\n') {
                            line_len.saturating_sub(1)
                        } else {
                            line_len
                        };
                    if line_start < line_end {
                        self.apply_rot13_range(line_start, line_end, changed);
                    }
                }
                return EngineAction::None;
            }
            if unicode == Some('i') || unicode == Some('a') {
                self.pending_text_object = unicode;
                self.pending_operator = Some(operator);
                return EngineAction::None;
            }
            if unicode == Some('f')
                || unicode == Some('t')
                || unicode == Some('F')
                || unicode == Some('T')
            {
                self.pending_find_operator = Some((operator, unicode.unwrap()));
                return EngineAction::None;
            }
            // Fall through to common motion match block below
        }

        // Check if we're waiting for a text object type (after 'i' or 'a')
        if let Some(modifier) = self.pending_text_object.take() {
            if let Some(obj_type) = unicode {
                self.apply_operator_text_object(operator, modifier, obj_type, changed);
            }
            return EngineAction::None;
        }

        // Check if the next character is a text object modifier ('i' or 'a')
        if unicode == Some('i') || unicode == Some('a') {
            self.pending_text_object = unicode;
            self.pending_operator = Some(operator); // Put the operator back!
            return EngineAction::None;
        }

        // Handle operator + motion combinations (dw, cw, db, cb, de, ce, etc.)
        match unicode {
            Some('y') if operator == 'y' => {
                // yy: yank current line(s)
                let count = self.take_count();
                self.yank_lines(count);
            }
            Some('o') if operator == 'd' => {
                // do: diff obtain — pull line from other diff window
                self.diff_obtain(changed);
            }
            Some('p') if operator == 'd' => {
                // dp: diff put — push line to other diff window
                self.diff_put(changed);
            }
            Some('d') if operator == 'd' => {
                // dd: delete line
                let count = self.take_count();
                self.start_undo_group();
                self.delete_lines(count, changed);
                self.finish_undo_group();

                // Record for repeat
                self.last_change = Some(Change {
                    op: ChangeOp::Delete,
                    text: String::new(),
                    count,
                    motion: Some(Motion::DeleteLine),
                });
            }
            Some('c') if operator == 'c' => {
                // cc: change line (like S)
                let count = self.take_count();
                let start_line = self.view().cursor.line;

                self.start_undo_group();

                // Delete content of lines
                for i in 0..count {
                    let line_idx = start_line + i;
                    if line_idx >= self.buffer().len_lines() {
                        break;
                    }

                    let line_start = self.buffer().line_to_char(line_idx);
                    let line_len = self.buffer().line_len_chars(line_idx);
                    let line_content = self.buffer().content.line(line_idx);

                    let delete_end = if line_content.chars().last() == Some('\n') && line_len > 0 {
                        line_start + line_len - 1
                    } else {
                        line_start + line_len
                    };

                    if line_start < delete_end {
                        let deleted: String = self
                            .buffer()
                            .content
                            .slice(line_start..delete_end)
                            .chars()
                            .collect();
                        let reg = self.active_register();
                        self.set_register(reg, deleted, false);
                        self.clear_selected_register();

                        self.delete_with_undo(line_start, delete_end);
                        *changed = true;
                        break;
                    }
                }

                self.view_mut().cursor.col = 0;
                self.insert_text_buffer.clear();
                self.mode = Mode::Insert;
                self.count = None;
            }
            Some('w') => {
                // dw: delete to start of next word
                // cw: special case — does NOT include trailing whitespace when
                //     cursor is in a word. Vim's `:help cw` says cw/cW only
                //     change up to end of the word, not across whitespace.
                let count = self.take_count();
                if operator == 'c' {
                    self.apply_cw_special(count, false, changed);
                } else {
                    self.apply_operator_with_motion(operator, 'w', count, changed);
                }
            }
            Some('W') => {
                // dW/cW: delete/change WORD forward
                let count = self.take_count();
                if operator == 'c' {
                    // cW: special case — stop at end of WORD, no trailing whitespace
                    self.apply_cw_special(count, true, changed);
                } else {
                    let start_cursor = self.view().cursor;
                    let start_pos =
                        self.buffer().line_to_char(start_cursor.line) + start_cursor.col;
                    for _ in 0..count {
                        self.move_bigword_forward();
                    }
                    let end_cursor = self.view().cursor;
                    let mut end_pos = self.buffer().line_to_char(end_cursor.line) + end_cursor.col;
                    self.view_mut().cursor = start_cursor;
                    // Clamp at line boundary for forward W (like w)
                    if end_cursor.line > start_cursor.line {
                        let line_char_start = self.buffer().line_to_char(start_cursor.line);
                        let line_len = self.buffer().line_len_chars(start_cursor.line);
                        let has_nl = self.buffer().content.line(start_cursor.line).chars().last()
                            == Some('\n');
                        end_pos = if has_nl {
                            (line_char_start + line_len - 1).min(end_pos)
                        } else {
                            (line_char_start + line_len).min(end_pos)
                        };
                    }
                    if start_pos < end_pos {
                        self.apply_charwise_operator(operator, start_pos, end_pos, changed);
                    }
                }
            }
            Some('B') => {
                // dB: delete WORD backward
                let count = self.take_count();
                let start_cursor = self.view().cursor;
                let start_pos = self.buffer().line_to_char(start_cursor.line) + start_cursor.col;
                for _ in 0..count {
                    self.move_bigword_backward();
                }
                let end_pos =
                    self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
                if end_pos < start_pos {
                    self.apply_charwise_operator(operator, end_pos, start_pos, changed);
                }
            }
            Some('E') => {
                // dE: delete to end of WORD
                let count = self.take_count();
                let start_cursor = self.view().cursor;
                let start_pos = self.buffer().line_to_char(start_cursor.line) + start_cursor.col;
                for _ in 0..count {
                    self.move_bigword_end();
                }
                let end_pos =
                    self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
                self.view_mut().cursor = start_cursor;
                if end_pos >= start_pos {
                    let end = (end_pos + 1).min(self.buffer().len_chars());
                    self.apply_charwise_operator(operator, start_pos, end, changed);
                }
            }
            Some('b') => {
                // db/cb: delete/change back to start of word
                let count = self.take_count();
                self.apply_operator_with_motion(operator, 'b', count, changed);
            }
            Some('e') => {
                // de/ce: delete/change to end of word
                let count = self.take_count();
                self.apply_operator_with_motion(operator, 'e', count, changed);
            }
            Some('%') => {
                // d%/c%: delete/change to matching bracket
                self.apply_operator_bracket_motion(operator, changed);
            }
            Some('$') => {
                // y$/d$/c$: to end of current line (does not include the newline)
                let _ = self.take_count(); // count ignored for simplicity (same as D/C)
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let line_char_start = self.buffer().line_to_char(line);
                let line_len = self.buffer().line_len_chars(line);
                let has_newline = self.buffer().content.line(line).chars().last() == Some('\n');
                let line_end = if has_newline {
                    line_char_start + line_len - 1
                } else {
                    line_char_start + line_len
                };
                let start_pos = line_char_start + col;
                if start_pos >= line_end {
                    return EngineAction::None;
                }
                self.apply_charwise_operator(operator, start_pos, line_end, changed);
            }
            Some('0') => {
                // y0/d0: from start of line to (not including) cursor
                let _ = self.take_count();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                if col == 0 {
                    return EngineAction::None;
                }
                let line_char_start = self.buffer().line_to_char(line);
                self.view_mut().cursor.col = 0;
                self.apply_charwise_operator(
                    operator,
                    line_char_start,
                    line_char_start + col,
                    changed,
                );
            }
            Some('^') => {
                // d^: delete to first non-blank char
                let _ = self.take_count();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let target = self.first_non_blank_col(line);
                if col == target {
                    return EngineAction::None;
                }
                let line_char_start = self.buffer().line_to_char(line);
                let (start, end) = if col > target {
                    (line_char_start + target, line_char_start + col)
                } else {
                    (line_char_start + col, line_char_start + target)
                };
                self.view_mut().cursor.col = target.min(col);
                self.apply_charwise_operator(operator, start, end, changed);
            }
            Some('h') => {
                // dh: delete count chars to the left
                let count = self.take_count();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let chars_to_delete = count.min(col);
                if chars_to_delete == 0 {
                    return EngineAction::None;
                }
                let line_char_start = self.buffer().line_to_char(line);
                let start = line_char_start + col - chars_to_delete;
                let end = line_char_start + col;
                self.view_mut().cursor.col = col - chars_to_delete;
                self.apply_charwise_operator(operator, start, end, changed);
            }
            Some('l') => {
                // dl: delete count chars to the right
                let count = self.take_count();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let line_len = self.buffer().line_len_chars(line);
                let has_nl = self.buffer().content.line(line).chars().last() == Some('\n');
                let max_col = if has_nl {
                    line_len.saturating_sub(1)
                } else {
                    line_len
                };
                let chars_to_delete = count.min(max_col.saturating_sub(col));
                if chars_to_delete == 0 {
                    return EngineAction::None;
                }
                let line_char_start = self.buffer().line_to_char(line);
                let start = line_char_start + col;
                let end = line_char_start + col + chars_to_delete;
                self.apply_charwise_operator(operator, start, end, changed);
            }
            Some('j') => {
                // dj: delete current line + count lines below (linewise)
                let count = self.take_count();
                let current_line = self.view().cursor.line;
                let last_line = self.buffer().len_lines().saturating_sub(1);
                let end_line = (current_line + count).min(last_line);
                self.apply_linewise_operator(operator, current_line, end_line, changed);
            }
            Some('k') => {
                // dk: delete current line + count lines above (linewise)
                let count = self.take_count();
                let current_line = self.view().cursor.line;
                let start_line = current_line.saturating_sub(count);
                self.apply_linewise_operator(operator, start_line, current_line, changed);
            }
            Some('G') => {
                // dG: delete from current line to end (or to line N if count given)
                // G uses motion count as line number, not multiplied with operator count
                self.operator_count = None;
                let count = self.count.take();
                let current_line = self.view().cursor.line;
                let target_line = count
                    .map(|n| {
                        n.saturating_sub(1)
                            .min(self.buffer().len_lines().saturating_sub(1))
                    })
                    .unwrap_or_else(|| self.buffer().len_lines().saturating_sub(1));
                let (start, end) = if target_line >= current_line {
                    (current_line, target_line)
                } else {
                    (target_line, current_line)
                };
                self.apply_linewise_operator(operator, start, end, changed);
            }
            Some('{') => {
                // d{: delete backward to previous paragraph boundary (linewise).
                // { is exclusive: the destination (backward) is included but cursor
                // line is excluded.  Range: [target_line, cursor_line - 1].
                // Neovim example: "aaa\n\nbbb\nccc\nddd" cursor on ddd (line 4):
                //   d{ → "aaa\nddd" (deletes blank + bbb + ccc, keeps cursor line)
                let count = self.take_count();
                let cursor_line = self.view().cursor.line;
                for _ in 0..count {
                    self.move_paragraph_backward();
                }
                let target_line = self.view().cursor.line;
                if target_line < cursor_line {
                    self.apply_linewise_operator(
                        operator,
                        target_line,
                        cursor_line.saturating_sub(1),
                        changed,
                    );
                } else if target_line == cursor_line && target_line == 0 {
                    self.apply_linewise_operator(operator, cursor_line, cursor_line, changed);
                }
            }
            Some('}') => {
                // d}: delete forward to next paragraph boundary (linewise).
                // } is exclusive: cursor line is included but the destination
                // (the blank line } lands on) is excluded.
                // Range: [cursor_line, target_line - 1] when target is blank.
                // Neovim example: "line one\n\nline three" cursor on line one:
                //   d} → "\nline three" (deletes line one, keeps blank line)
                let count = self.take_count();
                let cursor_line = self.view().cursor.line;
                for _ in 0..count {
                    self.move_paragraph_forward();
                }
                let target_line = self.view().cursor.line;
                let last_line = self.buffer().len_lines().saturating_sub(1);
                // Exclude target blank line, unless } hit EOF (last line)
                let range_end = if target_line > cursor_line
                    && self.is_line_empty(target_line)
                    && target_line <= last_line
                {
                    target_line.saturating_sub(1)
                } else {
                    target_line
                };
                if cursor_line <= range_end {
                    self.apply_linewise_operator(operator, cursor_line, range_end, changed);
                }
            }
            Some('(') => {
                // d(: delete to previous sentence start (charwise)
                let count = self.take_count();
                let start_pos =
                    self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
                for _ in 0..count {
                    self.move_sentence_backward();
                }
                let end_pos =
                    self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
                if end_pos < start_pos {
                    self.apply_charwise_operator(operator, end_pos, start_pos, changed);
                }
            }
            Some(')') => {
                // d): delete to next sentence start (charwise)
                let count = self.take_count();
                let start_pos =
                    self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
                for _ in 0..count {
                    self.move_sentence_forward();
                }
                let end_pos =
                    self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
                if end_pos > start_pos {
                    self.view_mut().cursor.col = start_pos
                        - self
                            .buffer()
                            .line_to_char(self.buffer().content.char_to_line(start_pos));
                    self.view_mut().cursor.line = self.buffer().content.char_to_line(start_pos);
                    self.apply_charwise_operator(operator, start_pos, end_pos, changed);
                }
            }
            Some('H') => {
                // dH: delete from current line to top of screen (linewise)
                let _ = self.take_count();
                let current_line = self.view().cursor.line;
                let top = self.view().scroll_top;
                let (s, e) = if top <= current_line {
                    (top, current_line)
                } else {
                    (current_line, top)
                };
                self.apply_linewise_operator(operator, s, e, changed);
            }
            Some('M') => {
                // dM: delete from current line to middle of screen (linewise)
                let _ = self.take_count();
                let current_line = self.view().cursor.line;
                let viewport_lines = self.view().viewport_lines.max(1);
                let mid = self.view().scroll_top + viewport_lines / 2;
                let mid = mid.min(self.buffer().len_lines().saturating_sub(1));
                let (s, e) = if mid <= current_line {
                    (mid, current_line)
                } else {
                    (current_line, mid)
                };
                self.apply_linewise_operator(operator, s, e, changed);
            }
            Some('L') => {
                // dL: delete from current line to bottom of screen (linewise)
                let _ = self.take_count();
                let current_line = self.view().cursor.line;
                let viewport_lines = self.view().viewport_lines.max(1);
                let bot = (self.view().scroll_top + viewport_lines).saturating_sub(1);
                let bot = bot.min(self.buffer().len_lines().saturating_sub(1));
                let (s, e) = if bot >= current_line {
                    (current_line, bot)
                } else {
                    (bot, current_line)
                };
                self.apply_linewise_operator(operator, s, e, changed);
            }
            Some('+') => {
                // d+: delete from current line through N lines down (linewise)
                let count = self.take_count();
                let current_line = self.view().cursor.line;
                let last_line = self.buffer().len_lines().saturating_sub(1);
                let end_line = (current_line + count).min(last_line);
                self.apply_linewise_operator(operator, current_line, end_line, changed);
            }
            Some('-') => {
                // d-: delete from current line through N lines up (linewise)
                let count = self.take_count();
                let current_line = self.view().cursor.line;
                let start_line = current_line.saturating_sub(count);
                self.apply_linewise_operator(operator, start_line, current_line, changed);
            }
            Some('_') => {
                // d_: delete from current line through N-1 lines down (linewise)
                let count = self.take_count();
                let current_line = self.view().cursor.line;
                let last_line = self.buffer().len_lines().saturating_sub(1);
                let end_line = (current_line + count - 1).min(last_line);
                self.apply_linewise_operator(operator, current_line, end_line, changed);
            }
            Some('|') => {
                // d|: delete from cursor to column N (charwise)
                let count = self.take_count();
                let target_col = count.saturating_sub(1);
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let max_col = self.get_max_cursor_col(line);
                let target = target_col.min(max_col);
                if target != col {
                    let line_start = self.buffer().line_to_char(line);
                    let (start, end) = if col > target {
                        (line_start + target, line_start + col)
                    } else {
                        (line_start + col, line_start + target)
                    };
                    self.view_mut().cursor.col = target.min(col);
                    self.apply_charwise_operator(operator, start, end, changed);
                }
            }
            Some(';') => {
                // d;: delete to next find repeat
                if let Some((find_type, target)) = self.last_find {
                    self.apply_operator_find_char(operator, find_type, target, changed);
                }
            }
            Some(',') => {
                // d,: delete to previous find repeat (reverse direction)
                if let Some((find_type, target)) = self.last_find {
                    let reversed = match find_type {
                        'f' => 'F',
                        'F' => 'f',
                        't' => 'T',
                        'T' => 't',
                        _ => find_type,
                    };
                    self.apply_operator_find_char(operator, reversed, target, changed);
                }
            }
            Some('f') | Some('t') | Some('F') | Some('T') => {
                // Deferred: need one more keystroke for the target char
                self.pending_find_operator = Some((operator, unicode.unwrap()));
            }
            Some('\'') => {
                // d'{mark}: linewise delete to mark — wait for mark char
                self.pending_key = Some('\'');
                self.pending_operator = Some(operator);
            }
            _ => {
                // Invalid motion - cancel operator
                self.count = None;
            }
        }
        EngineAction::None
    }

    /// Apply a case transform (toggle/lower/upper) over a char range in the buffer.
    pub(crate) fn apply_case_range(
        &mut self,
        start: usize,
        end: usize,
        op: char,
        changed: &mut bool,
    ) {
        if start >= end {
            return;
        }
        let original: String = self.buffer().content.slice(start..end).chars().collect();
        let transformed: String = original
            .chars()
            .map(|c| match op {
                '~' => {
                    if c.is_uppercase() {
                        c.to_lowercase().next().unwrap_or(c)
                    } else if c.is_lowercase() {
                        c.to_uppercase().next().unwrap_or(c)
                    } else {
                        c
                    }
                }
                'u' => c.to_lowercase().next().unwrap_or(c),
                'U' => c.to_uppercase().next().unwrap_or(c),
                _ => c,
            })
            .collect();
        if transformed != original {
            self.start_undo_group();
            self.delete_with_undo(start, end);
            self.insert_with_undo(start, &transformed);
            self.finish_undo_group();
            *changed = true;
        }
    }

    /// Apply ROT13 encoding to a char range.
    pub(crate) fn apply_rot13_range(&mut self, start: usize, end: usize, changed: &mut bool) {
        if start >= end {
            return;
        }
        let original: String = self.buffer().content.slice(start..end).chars().collect();
        let transformed: String = original
            .chars()
            .map(|c| match c {
                'A'..='M' | 'a'..='m' => (c as u8 + 13) as char,
                'N'..='Z' | 'n'..='z' => (c as u8 - 13) as char,
                _ => c,
            })
            .collect();
        if transformed != original {
            self.start_undo_group();
            self.delete_with_undo(start, end);
            self.insert_with_undo(start, &transformed);
            self.finish_undo_group();
            *changed = true;
        }
    }

    /// Apply a charwise operator on a byte/char range [start..end).
    pub(crate) fn apply_charwise_operator(
        &mut self,
        operator: char,
        start: usize,
        end: usize,
        changed: &mut bool,
    ) {
        if start >= end {
            return;
        }
        // Force linewise mode: convert char range to line range and redirect
        if self.force_motion_mode == Some('V') {
            self.force_motion_mode = None;
            let start_line = self.buffer().content.char_to_line(start);
            let end_line = self
                .buffer()
                .content
                .char_to_line(end.saturating_sub(1).max(start));
            self.apply_linewise_operator(operator, start_line, end_line, changed);
            return;
        }
        // Force blockwise mode: apply operator as a block (rectangle)
        if self.force_motion_mode == Some('\x16') {
            self.force_motion_mode = None;
            let start_line = self.buffer().content.char_to_line(start);
            let start_line_char = self.buffer().line_to_char(start_line);
            let start_col = start - start_line_char;
            let end_char = end.saturating_sub(1).max(start);
            let end_line = self.buffer().content.char_to_line(end_char);
            let end_line_char = self.buffer().line_to_char(end_line);
            let end_col = end_char - end_line_char;
            let left_col = start_col.min(end_col);
            let right_col = start_col.max(end_col);
            self.apply_blockwise_operator(
                operator, start_line, end_line, left_col, right_col, changed,
            );
            return;
        }
        self.force_motion_mode = None;
        match operator {
            'y' => {
                let text: String = self.buffer().content.slice(start..end).chars().collect();
                let reg = self.active_register();
                self.set_yank_register(reg, text.clone(), false);
                self.clear_selected_register();
                // Record yank highlight
                let start_line = self.buffer().content.char_to_line(start);
                let start_line_char = self.buffer().line_to_char(start_line);
                let start_col = start - start_line_char;
                let end_char = end.saturating_sub(1).max(start);
                let end_line = self.buffer().content.char_to_line(end_char);
                let end_line_char = self.buffer().line_to_char(end_line);
                let end_col = end_char - end_line_char;
                self.record_yank_highlight(
                    Cursor {
                        line: start_line,
                        col: start_col,
                    },
                    Cursor {
                        line: end_line,
                        col: end_col,
                    },
                    false,
                );
            }
            'd' => {
                let text: String = self.buffer().content.slice(start..end).chars().collect();
                let reg = self.active_register();
                self.set_delete_register(reg, text, false);
                self.clear_selected_register();
                self.start_undo_group();
                self.delete_with_undo(start, end);
                self.clamp_cursor_col();
                self.finish_undo_group();
                *changed = true;
            }
            'c' => {
                let text: String = self.buffer().content.slice(start..end).chars().collect();
                let reg = self.active_register();
                self.set_delete_register(reg, text, false);
                self.clear_selected_register();
                self.start_undo_group();
                self.delete_with_undo(start, end);
                self.clamp_cursor_col_insert();
                self.mode = Mode::Insert;
                self.insert_text_buffer.clear();
                self.count = None;
                *changed = true;
            }
            '~' | 'u' | 'U' => {
                self.apply_case_range(start, end, operator, changed);
            }
            'R' => {
                // g?: ROT13 encode
                self.apply_rot13_range(start, end, changed);
            }
            '>' | '<' | '=' => {
                // Indent/dedent/auto-indent: operate on full lines containing the range
                let start_line = self.buffer().content.char_to_line(start);
                let end_line = self
                    .buffer()
                    .content
                    .char_to_line(end.saturating_sub(1).max(start));
                let count = end_line - start_line + 1;
                if operator == '>' {
                    self.indent_lines(start_line, count, changed);
                } else if operator == '<' {
                    self.dedent_lines(start_line, count, changed);
                } else {
                    self.auto_indent_lines(start_line, count, changed);
                }
            }
            'q' | 'Q' => {
                // gq/gw: format lines containing the range
                let start_line = self.buffer().content.char_to_line(start);
                let end_line = self
                    .buffer()
                    .content
                    .char_to_line(end.saturating_sub(1).max(start));
                let saved = self.view().cursor;
                self.format_lines(start_line, end_line, changed);
                if operator == 'Q' {
                    self.view_mut().cursor = saved;
                    self.clamp_cursor_col();
                }
            }
            '!' => {
                // Filter: switch to command mode with range + !
                let start_line = self.buffer().content.char_to_line(start);
                let end_line = self
                    .buffer()
                    .content
                    .char_to_line(end.saturating_sub(1).max(start));
                self.mode = Mode::Command;
                self.command_buffer = format!("{},{}!", start_line + 1, end_line + 1);
                self.command_cursor = self.command_buffer.chars().count();
            }
            '@' => {
                // g@: call user-defined operatorfunc (charwise)
                let start_line = self.buffer().content.char_to_line(start);
                self.view_mut().cursor.line = start_line;
                self.view_mut().cursor.col = start - self.buffer().line_to_char(start_line);
                self.plugin_run_operatorfunc("char");
            }
            _ => {}
        }
    }

    /// Apply a linewise operator on a range of lines [start_line..=end_line].
    pub(crate) fn apply_linewise_operator(
        &mut self,
        operator: char,
        start_line: usize,
        end_line: usize,
        changed: &mut bool,
    ) {
        if start_line > end_line {
            return;
        }
        // Force charwise mode: convert line range to char range and redirect
        if self.force_motion_mode == Some('v') {
            self.force_motion_mode = None;
            let start = self.buffer().line_to_char(start_line);
            let end = self
                .buffer()
                .line_to_char((end_line + 1).min(self.buffer().len_lines()));
            self.apply_charwise_operator(operator, start, end, changed);
            return;
        }
        // Force blockwise mode: convert to block using cursor column
        if self.force_motion_mode == Some('\x16') {
            self.force_motion_mode = None;
            let col = self.view().cursor.col;
            self.apply_blockwise_operator(operator, start_line, end_line, col, col, changed);
            return;
        }
        self.force_motion_mode = None;
        let count = end_line - start_line + 1;
        self.view_mut().cursor.line = start_line;
        self.view_mut().cursor.col = 0;
        match operator {
            'y' => {
                self.yank_lines(count);
            }
            'd' => {
                self.start_undo_group();
                self.delete_lines(count, changed);
                self.finish_undo_group();
            }
            'c' => {
                // Delete lines content, enter insert mode (like cc on range)
                self.start_undo_group();
                // Delete the remaining lines below (count - 1)
                if count > 1 {
                    let next_line = start_line + 1;
                    let next_start = self.buffer().line_to_char(next_line);
                    let last_line = (start_line + count).min(self.buffer().len_lines());
                    let last_end = self.buffer().line_to_char(last_line);
                    if next_start < last_end {
                        let deleted: String = self
                            .buffer()
                            .content
                            .slice(next_start..last_end)
                            .chars()
                            .collect();
                        let reg = self.active_register();
                        self.set_delete_register(reg, deleted, true);
                        self.clear_selected_register();
                        self.delete_with_undo(next_start, last_end);
                    }
                }
                // Clear first line content
                let first_line_start = self.buffer().line_to_char(start_line);
                let first_line_len = self.buffer().line_len_chars(start_line);
                let first_has_nl =
                    self.buffer().content.line(start_line).chars().last() == Some('\n');
                let first_end = if first_has_nl && first_line_len > 0 {
                    first_line_start + first_line_len - 1
                } else {
                    first_line_start + first_line_len
                };
                if first_line_start < first_end {
                    self.delete_with_undo(first_line_start, first_end);
                }
                self.view_mut().cursor.col = 0;
                self.mode = Mode::Insert;
                self.insert_text_buffer.clear();
                self.count = None;
                *changed = true;
            }
            '>' => {
                self.indent_lines(start_line, count, changed);
            }
            '<' => {
                self.dedent_lines(start_line, count, changed);
            }
            '=' => {
                self.auto_indent_lines(start_line, count, changed);
            }
            '~' | 'u' | 'U' => {
                let start = self.buffer().line_to_char(start_line);
                let end = self
                    .buffer()
                    .line_to_char((end_line + 1).min(self.buffer().len_lines()));
                self.apply_case_range(start, end, operator, changed);
            }
            'R' => {
                // g?: ROT13 encode lines
                let start = self.buffer().line_to_char(start_line);
                let end = self
                    .buffer()
                    .line_to_char((end_line + 1).min(self.buffer().len_lines()));
                self.apply_rot13_range(start, end, changed);
            }
            'q' => {
                // gq: format lines
                self.format_lines(start_line, end_line, changed);
            }
            'Q' => {
                // gw: format lines, keep cursor
                let saved = self.view().cursor;
                self.format_lines(start_line, end_line, changed);
                self.view_mut().cursor = saved;
                self.clamp_cursor_col();
            }
            '!' => {
                // Filter: switch to command mode with range + !
                self.mode = Mode::Command;
                self.command_buffer = format!("{},{}!", start_line + 1, end_line + 1);
                self.command_cursor = self.command_buffer.chars().count();
            }
            '@' => {
                // g@: call user-defined operatorfunc (linewise)
                // Set '[ and '] marks for the range, then call the plugin
                self.view_mut().cursor.line = start_line;
                self.view_mut().cursor.col = 0;
                self.plugin_run_operatorfunc("line");
            }
            _ => {}
        }
    }

    /// Apply an operator with a find-char motion (dfx, dtx, dFx, dTx).
    pub(crate) fn apply_operator_find_char(
        &mut self,
        operator: char,
        find_type: char,
        target: char,
        changed: &mut bool,
    ) {
        let count = self.take_count();
        let start_col = self.view().cursor.col;
        let line = self.view().cursor.line;
        let line_start = self.buffer().line_to_char(line);

        // Execute the find motion (repeat for count)
        let mut found = false;
        for _ in 0..count {
            if self.find_char(find_type, target) {
                found = true;
            } else {
                break;
            }
        }
        self.last_find = Some((find_type, target));

        if !found {
            return; // find_char found no match
        }
        let end_col = self.view().cursor.col;

        // Calculate range
        let (range_start, range_end) = if find_type == 'f' || find_type == 't' {
            // Forward: start_col..=end_col (inclusive of end for operators)
            (line_start + start_col, line_start + end_col + 1)
        } else {
            // Backward (F/T): end_col..start_col (exclusive of start — don't delete char under cursor)
            (line_start + end_col, line_start + start_col)
        };

        // Restore cursor to start of range for charwise operations
        self.view_mut().cursor.col = if find_type == 'F' || find_type == 'T' {
            end_col
        } else {
            start_col
        };

        self.apply_charwise_operator(operator, range_start, range_end, changed);
    }

    /// gn: find next (or prev if `backward`) search match, enter Visual mode selecting it.
    /// If `op` is Some('c'), delete the match and enter Insert (cgn).
    pub(crate) fn cmd_gn(&mut self, op: Option<char>, backward: bool, changed: &mut bool) {
        if self.search_matches.is_empty() {
            self.message = format!("Pattern not found: {}", self.search_query);
            return;
        }
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let cursor_char = self.buffer().line_to_char(line) + col;

        let idx = if backward {
            self.search_matches
                .iter()
                .rposition(|(start, _)| *start < cursor_char)
                .unwrap_or(self.search_matches.len() - 1)
        } else {
            self.search_matches
                .iter()
                .position(|(start, _)| *start >= cursor_char)
                .unwrap_or(0)
        };

        let (match_start, match_end) = self.search_matches[idx];

        // Convert char positions to line/col
        let start_line = self.buffer().content.char_to_line(match_start);
        let start_line_char = self.buffer().line_to_char(start_line);
        let start_col = match_start - start_line_char;

        let end_char = match_end.saturating_sub(1).max(match_start);
        let end_line = self.buffer().content.char_to_line(end_char);
        let end_line_char = self.buffer().line_to_char(end_line);
        let end_col = end_char - end_line_char;

        if let Some(op_char) = op {
            // Operator mode: delete the match, optionally enter Insert
            self.view_mut().cursor.line = start_line;
            self.view_mut().cursor.col = start_col;
            let delete_start = match_start;
            let delete_end = match_end;
            let deleted: String = self
                .buffer()
                .content
                .slice(delete_start..delete_end)
                .chars()
                .collect();
            let reg = self.active_register();
            self.set_delete_register(reg, deleted, false);
            self.start_undo_group();
            self.delete_with_undo(delete_start, delete_end);
            *changed = true;
            if op_char == 'c' {
                self.mode = Mode::Insert;
                self.insert_text_buffer.clear();
                // Don't finish_undo_group — let insert mode do it
            } else {
                self.finish_undo_group();
                self.clamp_cursor_col();
            }
        } else {
            // Select the match in Visual mode
            self.mode = Mode::Visual;
            self.visual_anchor = Some(Cursor {
                line: start_line,
                col: start_col,
            });
            self.view_mut().cursor.line = end_line;
            self.view_mut().cursor.col = end_col;
            self.search_index = Some(idx);
        }
    }

    /// Handle a key press while leader mode is active (after pressing the leader key).
    /// Handle keys while breadcrumb focus mode is active.
    /// h/l navigate segments, Enter opens scoped picker, Escape exits.
    fn handle_breadcrumb_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        _ctrl: bool,
    ) -> EngineAction {
        match (key_name, unicode) {
            (_, Some('h')) | ("Left", _) => {
                if self.breadcrumb_selected > 0 {
                    self.breadcrumb_selected -= 1;
                }
            }
            (_, Some('l')) | ("Right", _) => {
                if self.breadcrumb_selected + 1 < self.breadcrumb_segments.len() {
                    self.breadcrumb_selected += 1;
                }
            }
            ("Return", _) => {
                self.breadcrumb_focus = false;
                self.breadcrumb_open_scoped();
            }
            _ => {
                // Escape or any other key exits breadcrumb focus
                self.breadcrumb_focus = false;
            }
        }
        EngineAction::None
    }

    pub(crate) fn handle_leader_key(&mut self, unicode: Option<char>) -> EngineAction {
        let ch = match unicode {
            Some(c) => c,
            None => {
                // Non-unicode key (e.g. arrow) — cancel leader
                self.leader_partial = None;
                return EngineAction::None;
            }
        };
        let mut partial = self.leader_partial.take().unwrap_or_default();
        partial.push(ch);

        // All known built-in leader sequences
        const SEQUENCES: &[&str] = &[
            "b", "rn", "gf", "gF", "gi", "gb", "ca", "sb", "sf", "sg", "sk", "so", "sp", "sw",
        ];

        match partial.as_str() {
            "b" => {
                // Enter breadcrumb focus mode
                self.rebuild_breadcrumb_segments();
                if !self.breadcrumb_segments.is_empty() {
                    self.breadcrumb_focus = true;
                    self.breadcrumb_selected = self.breadcrumb_segments.len() - 1;
                } else {
                    self.message = "No breadcrumb segments".to_string();
                }
            }
            "rn" => {
                // LSP rename — enter command mode pre-filled with :Rename <word>
                let word = self.word_under_cursor().unwrap_or_default();
                self.mode = crate::core::Mode::Command;
                self.command_buffer = format!("Rename {word}");
                self.command_cursor = self.command_buffer.chars().count();
            }
            "gf" | "gF" => {
                // LSP format whole file
                self.lsp_format_current();
            }
            "gi" => {
                // LSP go to implementation
                self.push_jump_location();
                self.lsp_request_implementation();
            }
            "ca" => {
                // Show LSP code actions for the current line
                self.show_code_actions_popup();
            }
            "gb" => {
                // Toggle inline git blame
                self.toggle_inline_blame();
            }
            "sb" => {
                self.open_picker(PickerSource::Buffers);
            }
            "sf" => {
                self.open_picker(PickerSource::Files);
            }
            "sg" => {
                self.open_picker(PickerSource::Grep);
            }
            "sk" => {
                self.open_picker(PickerSource::Keybindings);
            }
            "so" => {
                // Document outline / symbol navigation
                self.open_picker(PickerSource::CommandCenter);
                self.picker_query = "@".to_string();
                self.picker_filter();
                self.picker_load_preview();
            }
            "sp" => {
                self.open_picker(PickerSource::Commands);
            }
            "sw" => {
                // Grep word under cursor
                if let Some(word) = self.word_under_cursor() {
                    self.open_picker(PickerSource::Grep);
                    self.picker_query = word;
                    self.picker_filter();
                    self.picker_load_preview();
                } else {
                    self.open_picker(PickerSource::Grep);
                }
            }
            s => {
                // Check if this is a complete plugin keymap match
                let leader_key = format!("<leader>{s}");
                if self.plugin_run_keymap("n", &leader_key) {
                    return EngineAction::None;
                }

                // Check if partial is a prefix of a built-in sequence
                if SEQUENCES.iter().any(|seq| seq.starts_with(s)) {
                    self.leader_partial = Some(partial);
                    return EngineAction::None;
                }

                // Check if partial is a prefix of any plugin keymap
                let prefix = format!("<leader>{s}");
                let has_plugin_prefix = self
                    .plugin_manager
                    .as_ref()
                    .is_some_and(|pm| pm.has_keymap_prefix("n", &prefix));
                if has_plugin_prefix {
                    self.leader_partial = Some(partial);
                } else {
                    self.message = format!("Unknown leader sequence: <leader>{partial}");
                }
            }
        }
        EngineAction::None
    }

    /// Special `cw`/`cW` handler: change to end of word without including
    /// trailing whitespace.  Vim's `:help cw`: "When the cursor is in a word,
    /// `cw` and `cW` do not include the white space after a word."
    fn apply_cw_special(&mut self, count: usize, bigword: bool, changed: &mut bool) {
        let start_cursor = self.view().cursor;
        let start_pos = self.buffer().line_to_char(start_cursor.line) + start_cursor.col;
        let total = self.buffer().len_chars();

        // Check if cursor is on a word char — if not, fall back to ce.
        if start_pos < total {
            let ch = self.buffer().content.char(start_pos);
            let on_word = if bigword {
                !ch.is_whitespace()
            } else {
                is_word_char(ch)
            };
            if !on_word {
                // Not on a word char: cw behaves like ce
                self.apply_operator_with_motion(
                    'c',
                    if bigword { 'E' } else { 'e' },
                    count,
                    changed,
                );
                return;
            }
        }

        // Find end of Nth word. For count=1, stop at end of current word.
        // For count>1, skip whitespace between words but NOT after the last.
        let mut end = start_pos;
        for i in 0..count {
            // Skip to end of current word
            if bigword {
                while end < total && !self.buffer().content.char(end).is_whitespace() {
                    end += 1;
                }
            } else {
                while end < total && is_word_char(self.buffer().content.char(end)) {
                    end += 1;
                }
            }
            // Between words (not after last): skip whitespace to reach next word
            if i + 1 < count {
                while end < total && self.buffer().content.char(end).is_whitespace() {
                    end += 1;
                }
            }
        }

        if start_pos >= end {
            return;
        }

        // Record motion for dot repeat
        self.pending_change_motion = Some((if bigword { 'W' } else { 'w' }, count));

        self.apply_charwise_operator('c', start_pos, end, changed);
    }

    pub(crate) fn apply_operator_with_motion(
        &mut self,
        operator: char,
        motion: char,
        count: usize,
        changed: &mut bool,
    ) {
        // Save cursor position
        let start_cursor = self.view().cursor;
        let start_pos = self.buffer().line_to_char(start_cursor.line) + start_cursor.col;

        // Execute motion to find end position
        for _ in 0..count {
            match motion {
                'w' => self.move_word_forward(),
                'b' => self.move_word_backward(),
                'e' => self.move_word_end(),
                _ => return,
            }
        }

        let end_cursor = self.view().cursor;
        let end_pos = self.buffer().line_to_char(end_cursor.line) + end_cursor.col;

        // Restore cursor to start position
        self.view_mut().cursor = start_cursor;

        // Vim rule: 'w' operator motion does not cross line boundaries.
        // When the motion lands on the next line (col 0), clamp the end to
        // just before the newline on the start line, so dw/yw on the last word
        // of a line does not delete/yank the newline character.
        // Exception: on an empty line (only '\n'), dw should delete the newline
        // and join with the next line (Neovim behavior).
        let end_pos = if motion == 'w' && end_cursor.line > start_cursor.line {
            let line = start_cursor.line;
            let line_char_start = self.buffer().line_to_char(line);
            let line_len = self.buffer().line_len_chars(line);
            let is_empty_line =
                line_len == 1 && self.buffer().content.line(line).chars().next() == Some('\n');
            if is_empty_line {
                // Empty line: dw deletes the newline (joins with next line)
                end_pos
            } else {
                let has_newline = self.buffer().content.line(line).chars().last() == Some('\n');
                if has_newline {
                    (line_char_start + line_len - 1).min(end_pos) // before the \n
                } else {
                    (line_char_start + line_len).min(end_pos)
                }
            }
        } else {
            end_pos
        };

        // Determine range to delete
        let (delete_start, delete_end) = match start_pos.cmp(&end_pos) {
            std::cmp::Ordering::Less => {
                // Forward motion: delete from start to end (inclusive for 'e', exclusive for 'w')
                if motion == 'e' {
                    // 'e' moves to end of word, so include that character
                    (start_pos, (end_pos + 1).min(self.buffer().len_chars()))
                } else {
                    // 'w' moves to start of next word (exclusive end).
                    // Exception: when the file has no trailing newline and the word
                    // is the last one, move_word_forward clamps to total_chars-1
                    // (the last char) instead of going past it.  The normal range
                    // [start, end_pos) would then miss that final character.
                    // Detect this by checking that end_pos is the last char and it
                    // is not a newline (if it were '\n', the existing line-boundary
                    // clamping has already handled things correctly).
                    let total = self.buffer().len_chars();
                    let end = if end_pos + 1 == total
                        && total > 0
                        && self.buffer().content.char(end_pos) != '\n'
                    {
                        total
                    } else {
                        end_pos
                    };
                    (start_pos, end)
                }
            }
            std::cmp::Ordering::Greater => {
                // Backward motion (db): delete from end to start
                (end_pos, start_pos)
            }
            std::cmp::Ordering::Equal => {
                // No movement (e.g. single word on line, w clamps to end of word)
                return;
            }
        };

        if delete_start >= delete_end {
            return;
        }

        // For backward motion, move cursor to start of range
        if start_pos > end_pos {
            self.view_mut().cursor = end_cursor;
        }

        // Record the motion so `.` can replay c{motion} properly.
        if operator == 'c' {
            self.pending_change_motion = Some((motion, count));
        }

        self.apply_charwise_operator(operator, delete_start, delete_end, changed);

        // Record for dot repeat (d/y/>/</gu/gU/g~ with motion).
        // 'c' is recorded after Esc from insert mode, not here.
        if operator != 'c' && operator != 'y' {
            let motion_enum = match motion {
                'w' => Some(Motion::WordForward),
                'b' => Some(Motion::WordBackward),
                'e' => Some(Motion::WordEnd),
                _ => None,
            };
            if let Some(m) = motion_enum {
                self.last_change = Some(Change {
                    op: match operator {
                        'd' => ChangeOp::Delete,
                        '>' => ChangeOp::Indent,
                        '<' => ChangeOp::Dedent,
                        '~' => ChangeOp::ToggleCase,
                        _ => ChangeOp::Delete,
                    },
                    text: String::new(),
                    count,
                    motion: Some(m),
                });
            }
        }
    }

    pub(crate) fn apply_operator_bracket_motion(&mut self, operator: char, changed: &mut bool) {
        let start_line = self.view().cursor.line;
        let start_col = self.view().cursor.col;
        let start_pos = self.buffer().line_to_char(start_line) + start_col;

        if start_pos >= self.buffer().len_chars() {
            return;
        }

        let current_char = self.buffer().content.char(start_pos);

        // Find matching bracket and determine search parameters
        let (is_opening, open_char, close_char) = match current_char {
            '(' => (true, '(', ')'),
            ')' => (false, '(', ')'),
            '{' => (true, '{', '}'),
            '}' => (false, '{', '}'),
            '[' => (true, '[', ']'),
            ']' => (false, '[', ']'),
            _ => {
                // Not on a bracket - cancel operation
                return;
            }
        };

        // Find the matching bracket position
        if let Some(match_pos) =
            self.find_matching_bracket(start_pos, open_char, close_char, is_opening)
        {
            // Determine range to delete (inclusive of both brackets)
            let (delete_start, delete_end) = if is_opening {
                (start_pos, match_pos + 1)
            } else {
                (match_pos, start_pos + 1)
            };

            // Save text to register
            let text: String = self
                .buffer()
                .content
                .slice(delete_start..delete_end)
                .chars()
                .collect();
            let reg = self.active_register();
            self.set_register(reg, text, false);
            self.clear_selected_register();

            if operator == 'y' {
                // Yank only — move cursor to start of range, no deletion
                let start_line = self.buffer().content.char_to_line(delete_start);
                let start_line_char = self.buffer().line_to_char(start_line);
                let end_line = self
                    .buffer()
                    .content
                    .char_to_line((delete_end).saturating_sub(1));
                let end_line_char = self.buffer().line_to_char(end_line);
                self.view_mut().cursor.line = start_line;
                self.view_mut().cursor.col = delete_start - start_line_char;
                self.record_yank_highlight(
                    Cursor {
                        line: start_line,
                        col: delete_start - start_line_char,
                    },
                    Cursor {
                        line: end_line,
                        col: (delete_end - 1) - end_line_char,
                    },
                    false,
                );
            } else {
                // Delete or change
                self.start_undo_group();
                self.delete_with_undo(delete_start, delete_end);

                // Move cursor to start of deletion
                let new_line = self.buffer().content.char_to_line(delete_start);
                let line_start = self.buffer().line_to_char(new_line);
                self.view_mut().cursor.line = new_line;
                self.view_mut().cursor.col = delete_start - line_start;

                self.clamp_cursor_col();
                *changed = true;

                if operator == 'c' {
                    self.mode = Mode::Insert;
                    self.count = None;
                    // Don't finish_undo_group - let insert mode do it
                } else {
                    self.finish_undo_group();
                }
            }
        }
    }

    pub(crate) fn handle_insert_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        changed: &mut bool,
    ) {
        // ── Configured completion trigger (e.g. Ctrl-Space) ──────────────────
        {
            let trigger = self.settings.completion_keys.trigger.clone();
            if let Some((t_ctrl, _t_shift, _t_alt, t_ch)) =
                crate::core::settings::parse_key_binding(&trigger)
            {
                let key_char = key_name.chars().next().unwrap_or('\0');
                if ctrl == t_ctrl && (key_char == t_ch || (key_name == "space" && t_ch == ' ')) {
                    self.trigger_auto_completion();
                    return;
                }
            } else if ctrl && key_name == "space" {
                // Fallback for default <C-Space> trigger
                self.trigger_auto_completion();
                return;
            }
        }

        // ── Ctrl-N / Ctrl-P: word completion ─────────────────────────────────
        if ctrl && (key_name == "n" || key_name == "p") {
            let next = key_name == "n";
            if self.completion_display_only && self.completion_idx.is_some() {
                // Auto-popup is active: just cycle the index (don't insert)
                let len = self.completion_candidates.len();
                let cur = self.completion_idx.unwrap();
                let new_idx = if next {
                    (cur + 1) % len
                } else {
                    (cur + len - 1) % len
                };
                self.completion_idx = Some(new_idx);
                return;
            }
            if self.completion_idx.is_none() {
                let (prefix, start_col) = self.completion_prefix_at_cursor();
                let candidates = self.word_completions_for_prefix(&prefix);
                if candidates.is_empty() {
                    self.message = "No completions".to_string();
                    return;
                }
                self.completion_start_col = start_col;
                self.completion_candidates = candidates;
                let idx = if next {
                    0
                } else {
                    self.completion_candidates.len() - 1
                };
                self.completion_idx = Some(idx);
                self.apply_completion_candidate(idx);
            } else {
                let len = self.completion_candidates.len();
                let cur = self.completion_idx.unwrap();
                let new_idx = if next {
                    (cur + 1) % len
                } else {
                    (cur + len - 1) % len
                };
                self.completion_idx = Some(new_idx);
                self.apply_completion_candidate(new_idx);
            }
            *changed = true;
            return;
        }

        // ── AI ghost text: accept with Tab; clear on any other key ───────────
        if self.ai_ghost_text.is_some() {
            if !ctrl && key_name == "Tab" {
                self.ai_accept_ghost();
                *changed = true;
                return;
            }
            // If the typed character matches the start of the ghost text,
            // consume it rather than clearing — avoids doubled characters
            // when the AI includes a character the user just typed (e.g.
            // typing `"` when ghost starts with `"PlayerObject":`).
            if let Some(ch) = unicode {
                if !ctrl {
                    let ghost_starts_with_ch =
                        self.ai_ghost_text.as_deref().and_then(|g| g.chars().next()) == Some(ch);
                    if ghost_starts_with_ch {
                        // Advance all alternatives past this character.
                        for alt in &mut self.ai_ghost_alternatives {
                            if alt.starts_with(ch) {
                                *alt = alt[ch.len_utf8()..].to_string();
                            } else {
                                alt.clear();
                            }
                        }
                        self.ai_ghost_text = self
                            .ai_ghost_alternatives
                            .get(self.ai_ghost_alt_idx)
                            .cloned();
                        if self.ai_ghost_text.as_deref() == Some("") {
                            self.ai_ghost_clear();
                        }
                        // Fall through — let the character be inserted normally.
                    } else {
                        self.ai_ghost_clear();
                    }
                } else {
                    self.ai_ghost_clear();
                }
            } else {
                // Non-printable key (arrow, backspace, etc.) — clear ghost.
                self.ai_ghost_clear();
            }
        }

        // ── Tab: accept display-only popup OR fall through ────────────────────
        if !ctrl && key_name == "Tab" && self.completion_display_only {
            if let Some(idx) = self.completion_idx {
                self.apply_completion_candidate(idx);
                self.dismiss_completion();
                *changed = true;
                return;
            }
        }
        // No display-only popup — fall through to regular Tab handling in match below.

        // ── Down/Up: navigate completion popup if active ──────────────────────
        if !ctrl
            && (key_name == "Down" || key_name == "Up")
            && self.completion_display_only
            && self.completion_idx.is_some()
        {
            let next = key_name == "Down";
            let len = self.completion_candidates.len();
            let cur = self.completion_idx.unwrap();
            let new_idx = if next {
                (cur + 1) % len
            } else {
                (cur + len - 1) % len
            };
            self.completion_idx = Some(new_idx);
            return;
        }

        // Clear completion state on any non-completion key.
        if self.completion_idx.is_some() {
            self.dismiss_completion();
        }

        // Ctrl+R: insert register content at cursor
        if ctrl && key_name == "r" {
            self.insert_ctrl_r_pending = true;
            return;
        }
        // When Ctrl+R pending, next char selects the register to insert
        if self.insert_ctrl_r_pending {
            self.insert_ctrl_r_pending = false;
            if let Some(reg_char) = unicode {
                if let Some((content, _)) = self.get_register_content(reg_char) {
                    let content_clone = content.clone();
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let char_idx = self.buffer().line_to_char(line) + col;
                    let char_count = content_clone.chars().count();
                    self.insert_with_undo(char_idx, &content_clone);
                    self.view_mut().cursor.col += char_count;
                    *changed = true;
                }
            }
            return;
        }

        // Ctrl+U: delete from cursor back to insert-start column (Vim behavior)
        if ctrl && key_name == "u" {
            let line = self.view().cursor.line;
            let col = self.view().cursor.col;
            let del_to = self.insert_enter_col;
            if col > del_to {
                let line_start = self.buffer().line_to_char(line);
                let from = line_start + del_to;
                let to = line_start + col;
                self.delete_with_undo(from, to);
                self.view_mut().cursor.col = del_to;
                *changed = true;
            }
            return;
        }

        // Ctrl+O: execute one normal-mode command then return to insert
        if ctrl && key_name == "o" {
            self.finish_undo_group();
            self.mode = Mode::Normal;
            self.insert_ctrl_o_active = true;
            return;
        }

        // Ctrl+V: insert next character literally (two-key sequence)
        if ctrl && key_name == "v" {
            self.insert_ctrl_v_pending = true;
            return;
        }
        if self.insert_ctrl_v_pending {
            self.insert_ctrl_v_pending = false;
            // Insert the raw character regardless of what it is
            let literal = if let Some(ch) = unicode {
                Some(ch.to_string())
            } else if key_name == "Tab" {
                Some("\t".to_string())
            } else if key_name == "Return" {
                Some("\n".to_string())
            } else {
                None
            };
            if let Some(s) = literal {
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;
                self.insert_with_undo(char_idx, &s);
                self.insert_text_buffer.push_str(&s);
                if s == "\n" {
                    self.view_mut().cursor.line += 1;
                    self.view_mut().cursor.col = 0;
                } else {
                    self.view_mut().cursor.col += 1;
                }
                *changed = true;
            }
            return;
        }

        // Ctrl+W: delete word backward from cursor
        if ctrl && key_name == "w" {
            let line = self.view().cursor.line;
            let col = self.view().cursor.col;
            if col > 0 {
                // Find start of word backward
                let line_start = self.buffer().line_to_char(line);
                let char_idx = line_start + col;
                let line_text: String = self
                    .buffer()
                    .content
                    .slice(line_start..char_idx)
                    .chars()
                    .collect();
                // Skip trailing spaces, then skip non-spaces
                let chars: Vec<char> = line_text.chars().collect();
                let mut i = chars.len();
                // Skip trailing whitespace
                while i > 0 && chars[i - 1] == ' ' {
                    i -= 1;
                }
                // Skip word chars
                while i > 0 && chars[i - 1] != ' ' {
                    i -= 1;
                }
                let delete_start = line_start + i;
                let delete_end = char_idx;
                if delete_start < delete_end {
                    self.delete_with_undo(delete_start, delete_end);
                    self.view_mut().cursor.col = i;
                    *changed = true;
                }
            }
            return;
        }

        // Ctrl+T: indent current line by shiftwidth
        if ctrl && key_name == "t" {
            let line = self.view().cursor.line;
            let line_start = self.buffer().line_to_char(line);
            let sw = self.effective_shift_width();
            let indent = if self.settings.expand_tab {
                " ".repeat(sw)
            } else {
                "\t".to_string()
            };
            self.insert_with_undo(line_start, &indent);
            self.view_mut().cursor.col += indent.len();
            *changed = true;
            return;
        }

        // Ctrl+@ (Ctrl+2 / Ctrl+Space): insert prev text and stop insert
        if ctrl && (key_name == "2" || key_name == "space" || key_name == "at") {
            if !self.last_inserted_text.is_empty() {
                let text = self.last_inserted_text.clone();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;
                self.insert_with_undo(char_idx, &text);
                self.insert_text_buffer.push_str(&text);
                let lines: Vec<&str> = text.split('\n').collect();
                if lines.len() > 1 {
                    self.view_mut().cursor.line = line + lines.len() - 1;
                    self.view_mut().cursor.col = lines.last().map_or(0, |l| l.len());
                } else {
                    self.view_mut().cursor.col = col + text.chars().count();
                }
                *changed = true;
            }
            // Exit insert mode (stop insert)
            self.finish_undo_group();
            if !self.insert_text_buffer.is_empty() {
                self.last_inserted_text = self.insert_text_buffer.clone();
                self.last_change = Some(Change {
                    op: ChangeOp::Insert,
                    text: self.insert_text_buffer.clone(),
                    count: 1,
                    motion: None,
                });
            }
            self.pending_change_motion = None;
            self.mode = Mode::Normal;
            self.clamp_cursor_col();
            self.view_mut().extra_cursors.clear();
            return;
        }

        // Ctrl+A: re-insert last inserted text
        if ctrl && key_name == "a" {
            if !self.last_inserted_text.is_empty() {
                let text = self.last_inserted_text.clone();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;
                let char_count = text.chars().count();
                self.insert_with_undo(char_idx, &text);
                self.insert_text_buffer.push_str(&text);
                // Position cursor after the inserted text
                // Handle multi-line: find the last line of the insertion
                let lines: Vec<&str> = text.split('\n').collect();
                if lines.len() > 1 {
                    self.view_mut().cursor.line = line + lines.len() - 1;
                    self.view_mut().cursor.col = lines.last().map_or(0, |l| l.len());
                } else {
                    self.view_mut().cursor.col = col + char_count;
                }
                *changed = true;
            }
            return;
        }

        // Ctrl+G then subcommand: u = break undo, j/k = move line in insert mode
        if ctrl && key_name == "g" {
            self.insert_ctrl_g_pending = true;
            return;
        }
        if self.insert_ctrl_g_pending {
            self.insert_ctrl_g_pending = false;
            if let Some(ch) = unicode {
                match ch {
                    'u' => {
                        // CTRL-G u: break undo sequence (start new undo group)
                        self.finish_undo_group();
                        self.start_undo_group();
                    }
                    'j' | 'J' => {
                        // CTRL-G j: move cursor down one line in insert mode
                        let line = self.view().cursor.line;
                        if line + 1 < self.buffer().len_lines() {
                            self.view_mut().cursor.line = line + 1;
                            self.clamp_cursor_col();
                        }
                    }
                    'k' | 'K' => {
                        // CTRL-G k: move cursor up one line in insert mode
                        let line = self.view().cursor.line;
                        if line > 0 {
                            self.view_mut().cursor.line = line - 1;
                            self.clamp_cursor_col();
                        }
                    }
                    _ => {}
                }
            }
            return;
        }

        // Ctrl+E: insert character from line below cursor
        if ctrl && key_name == "e" {
            let line = self.view().cursor.line;
            let col = self.view().cursor.col;
            if line + 1 < self.buffer().len_lines() {
                let below_line = line + 1;
                let below_len = self.buffer().line_len_chars(below_line);
                let below_start = self.buffer().line_to_char(below_line);
                if col < below_len {
                    let ch = self.buffer().content.char(below_start + col);
                    if ch != '\n' {
                        let char_idx = self.buffer().line_to_char(line) + col;
                        let s = ch.to_string();
                        self.insert_with_undo(char_idx, &s);
                        self.insert_text_buffer.push_str(&s);
                        self.view_mut().cursor.col += 1;
                        *changed = true;
                    }
                }
            }
            return;
        }

        // Ctrl+Y: insert character from line above cursor
        if ctrl && key_name == "y" {
            let line = self.view().cursor.line;
            let col = self.view().cursor.col;
            if line > 0 {
                let above_line = line - 1;
                let above_len = self.buffer().line_len_chars(above_line);
                let above_start = self.buffer().line_to_char(above_line);
                if col < above_len {
                    let ch = self.buffer().content.char(above_start + col);
                    if ch != '\n' {
                        let char_idx = self.buffer().line_to_char(line) + col;
                        let s = ch.to_string();
                        self.insert_with_undo(char_idx, &s);
                        self.insert_text_buffer.push_str(&s);
                        self.view_mut().cursor.col += 1;
                        *changed = true;
                    }
                }
            }
            return;
        }

        // Ctrl+D: dedent current line by shiftwidth
        if ctrl && key_name == "d" {
            let line = self.view().cursor.line;
            let line_start = self.buffer().line_to_char(line);
            let sw = self.effective_shift_width();
            // Count leading spaces
            let line_text: String = self.buffer().content.line(line).chars().take(sw).collect();
            let spaces = line_text.chars().take_while(|c| *c == ' ').count();
            let to_remove = spaces.min(sw);
            if to_remove > 0 {
                self.delete_with_undo(line_start, line_start + to_remove);
                self.view_mut().cursor.col = self.view().cursor.col.saturating_sub(to_remove);
                *changed = true;
            }
            return;
        }

        match key_name {
            "Escape" => {
                self.finish_undo_group();
                // Record the insert operation for repeat and ". register
                if !self.insert_text_buffer.is_empty() {
                    self.last_inserted_text = self.insert_text_buffer.clone();
                    if let Some((motion_ch, count)) = self.pending_change_motion.take() {
                        // Insert was entered via c{motion} — record as Change so `.`
                        // replays the delete-motion + insert, not just the insert.
                        let motion = match motion_ch {
                            'w' => Some(Motion::WordForward),
                            'e' => Some(Motion::WordEnd),
                            'b' => Some(Motion::WordBackward),
                            _ => None,
                        };
                        self.last_change = Some(Change {
                            op: ChangeOp::Change,
                            text: self.insert_text_buffer.clone(),
                            count,
                            motion,
                        });
                    } else {
                        self.last_change = Some(Change {
                            op: ChangeOp::Insert,
                            text: self.insert_text_buffer.clone(),
                            count: 1,
                            motion: None,
                        });
                    }
                }
                self.pending_change_motion = None;
                // Apply visual block insert/append to remaining lines
                if let Some((start_line, end_line, col, _is_append, virtual_end)) =
                    self.visual_block_insert_info.take()
                {
                    let text = self.insert_text_buffer.clone();
                    if !text.is_empty() {
                        // The first line was already typed into; apply to remaining lines
                        let first_typed_line = start_line;
                        self.start_undo_group();
                        for line in start_line..=end_line {
                            if line == first_typed_line {
                                continue;
                            }
                            if line >= self.buffer().len_lines() {
                                break;
                            }
                            let line_len = self.buffer().line_len_chars(line);
                            let line_len_no_nl = if line_len > 0
                                && self
                                    .buffer()
                                    .content
                                    .char(self.buffer().line_to_char(line) + line_len - 1)
                                    == '\n'
                            {
                                line_len - 1
                            } else {
                                line_len
                            };
                            // In virtual-end mode (`$<C-v>...A`), the insert column is this
                            // specific line's own end — no padding needed, just append.
                            // Otherwise use the captured column and pad if the line is shorter.
                            let target_col = if virtual_end { line_len_no_nl } else { col };
                            let insert_col = target_col.min(line_len_no_nl);
                            let pad = target_col.saturating_sub(line_len_no_nl);
                            let char_idx = self.buffer().line_to_char(line) + insert_col;
                            if pad > 0 {
                                let spaces = " ".repeat(pad);
                                self.insert_with_undo(char_idx, &spaces);
                            }
                            self.insert_with_undo(
                                self.buffer().line_to_char(line) + target_col,
                                &text,
                            );
                        }
                        self.finish_undo_group();
                    }
                }
                // Repeat o/O insert for count > 1: duplicate typed text on new lines
                if self.insert_open_count > 1 && !self.insert_text_buffer.is_empty() {
                    let text = self.insert_text_buffer.clone();
                    let repeat = self.insert_open_count - 1;
                    self.start_undo_group();
                    for _ in 0..repeat {
                        let line = self.view().cursor.line;
                        let line_start = self.buffer().line_to_char(line);
                        let line_len = self.buffer().line_len_chars(line);
                        // Insert before the trailing newline
                        let insert_pos = line_start + line_len;
                        let has_nl =
                            line_len > 0 && self.buffer().content.char(insert_pos - 1) == '\n';
                        let insert_pos = if has_nl { insert_pos - 1 } else { insert_pos };
                        let new_line = format!("\n{}", text);
                        self.insert_with_undo(insert_pos, &new_line);
                        self.view_mut().cursor.line += 1;
                        self.view_mut().cursor.col = text.chars().count();
                    }
                    self.finish_undo_group();
                }
                self.insert_open_count = 0;
                // Track cursor pos for gi (insert at last insert position)
                let cur = self.view().cursor;
                self.last_insert_pos = Some((cur.line, cur.col));
                self.set_mode(Mode::Normal);
                // Vim moves cursor one left when leaving insert mode (unless at col 0)
                if self.view().cursor.col > 0 {
                    self.view_mut().cursor.col -= 1;
                }
                self.clamp_cursor_col();
                // Dismiss signature help when leaving insert mode
                self.lsp_signature_help = None;
                // Collapse all extra cursors.
                self.view_mut().extra_cursors.clear();
                // Refresh stale syntax highlights deferred from insert mode.
                self.refresh_syntax_if_stale();
                // Refresh position-aware annotations (e.g. git blame) now that
                // we're back in Normal mode. cursor_move is suppressed during
                // Insert mode to avoid stale blame on uncommitted lines.
                self.fire_cursor_move_hook();
            }
            "BackSpace" => {
                if !self.view().extra_cursors.is_empty() {
                    // Multi-cursor BackSpace: delete before every eligible cursor.
                    if self.mc_backspace() {
                        *changed = true;
                    } else {
                        // All cursors at col==0 — do nothing in multi-cursor mode.
                    }
                } else {
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let char_idx = self.buffer().line_to_char(line) + col;
                    if col > 0 {
                        // Auto-pair backspace: delete both opener and closer
                        let prev_char = self.buffer().content.char(char_idx - 1);
                        let next_char_matches =
                            if self.settings.auto_pairs && char_idx < self.buffer().len_chars() {
                                let next = self.buffer().content.char(char_idx);
                                auto_pair_closer(prev_char) == Some(next)
                            } else {
                                false
                            };
                        if next_char_matches {
                            // Delete both the opener (before cursor) and closer (after cursor)
                            self.delete_with_undo(char_idx - 1, char_idx + 1);
                        } else {
                            self.delete_with_undo(char_idx - 1, char_idx);
                        }
                        self.view_mut().cursor.col -= 1;
                        *changed = true;
                    } else if line > 0 {
                        let prev_line_len = self.buffer().line_len_chars(line - 1);
                        let new_col = if prev_line_len > 0 {
                            prev_line_len - 1
                        } else {
                            0
                        };
                        self.delete_with_undo(char_idx - 1, char_idx);
                        self.view_mut().cursor.line -= 1;
                        self.view_mut().cursor.col = new_col;
                        *changed = true;
                    }
                }
                if *changed {
                    self.trigger_auto_completion();
                }
            }
            "Delete" => {
                if !self.view().extra_cursors.is_empty() {
                    if self.mc_delete_forward() {
                        *changed = true;
                    }
                } else {
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let char_idx = self.buffer().line_to_char(line) + col;
                    if char_idx < self.buffer().len_chars() {
                        self.delete_with_undo(char_idx, char_idx + 1);
                        *changed = true;
                    }
                }
            }
            "Return" => {
                if !self.view().extra_cursors.is_empty() {
                    self.mc_return();
                    self.insert_text_buffer.push('\n');
                    *changed = true;
                } else {
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let char_idx = self.buffer().line_to_char(line) + col;
                    let indent = self.smart_indent_for_newline(line);
                    let indent_len = indent.len();
                    let text = format!("\n{}", indent);
                    self.insert_with_undo(char_idx, &text);
                    self.insert_text_buffer.push('\n');
                    self.view_mut().cursor.line += 1;
                    self.view_mut().cursor.col = indent_len;
                    *changed = true;
                }
            }
            "Tab" => {
                if !self.view().extra_cursors.is_empty() {
                    let tab_text = if self.settings.expand_tab {
                        " ".repeat(self.settings.tabstop as usize)
                    } else {
                        "\t".to_string()
                    };
                    self.insert_text_buffer.push_str(&tab_text);
                    self.mc_insert(&tab_text);
                    *changed = true;
                } else {
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let char_idx = self.buffer().line_to_char(line) + col;
                    if self.settings.expand_tab {
                        let n = self.settings.tabstop as usize;
                        let spaces = " ".repeat(n);
                        self.insert_with_undo(char_idx, &spaces);
                        self.insert_text_buffer.push_str(&spaces);
                        self.view_mut().cursor.col += n;
                    } else {
                        self.insert_with_undo(char_idx, "\t");
                        self.insert_text_buffer.push('\t');
                        self.view_mut().cursor.col += 1;
                    }
                    *changed = true;
                }
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
                // Try plugin insert-mode keymaps first (for non-printable or special keys)
                if unicode.is_none() && self.plugin_run_keymap("i", key_name) {
                    // keymap handled it — skip default character insertion
                } else if let Some(ch) = unicode {
                    if !self.view().extra_cursors.is_empty() {
                        // Multi-cursor character insert.
                        let mut buf = [0u8; 4];
                        let s = ch.encode_utf8(&mut buf).to_string();
                        self.mc_insert(&s);
                        self.insert_text_buffer.push(ch);
                        *changed = true;
                    } else {
                        let line = self.view().cursor.line;
                        let col = self.view().cursor.col;
                        let char_idx = self.buffer().line_to_char(line) + col;

                        // Auto-pairs: skip-over closing bracket/quote
                        let closing_pair = auto_pair_closer(ch);
                        if self.settings.auto_pairs
                            && is_closing_pair(ch)
                            && char_idx < self.buffer().len_chars()
                            && self.buffer().content.char(char_idx) == ch
                        {
                            // Skip over the existing closing char
                            self.view_mut().cursor.col += 1;
                            self.insert_text_buffer.push(ch);
                            *changed = true;
                        } else if self.settings.auto_pairs && closing_pair.is_some() {
                            let closer = closing_pair.unwrap();
                            // Smart context for quotes: only auto-pair if preceded by
                            // whitespace, bracket, or BOL
                            let should_pair = if is_quote_char(ch) {
                                if char_idx == 0 {
                                    true
                                } else {
                                    let prev = self.buffer().content.char(char_idx - 1);
                                    prev.is_whitespace()
                                        || matches!(prev, '(' | '[' | '{' | ',' | ';' | ':')
                                }
                            } else {
                                true
                            };
                            if should_pair {
                                let mut buf = [0u8; 4];
                                let open_s = ch.encode_utf8(&mut buf).to_string();
                                let mut buf2 = [0u8; 4];
                                let close_s = closer.encode_utf8(&mut buf2).to_string();
                                let pair = format!("{}{}", open_s, close_s);
                                self.insert_with_undo(char_idx, &pair);
                                self.insert_text_buffer.push(ch);
                                self.view_mut().cursor.col += 1;
                                *changed = true;
                            } else {
                                let mut buf = [0u8; 4];
                                let s = ch.encode_utf8(&mut buf);
                                self.insert_with_undo(char_idx, s);
                                self.insert_text_buffer.push(ch);
                                self.view_mut().cursor.col += 1;
                                *changed = true;
                            }
                        } else {
                            let mut buf = [0u8; 4];
                            let s = ch.encode_utf8(&mut buf);
                            self.insert_with_undo(char_idx, s);
                            self.insert_text_buffer.push(ch);
                            self.view_mut().cursor.col += 1;
                            *changed = true;
                        }
                    }
                    // Auto-outdent when typing a closing bracket as the
                    // first non-blank character on a line.
                    if matches!(ch, '}' | ')' | ']') {
                        let line = self.view().cursor.line;
                        if let Some(new_indent) = self.auto_outdent_for_closing(line) {
                            let old_indent = self.get_line_indent_str(line);
                            if new_indent != old_indent {
                                let line_start = self.buffer().line_to_char(line);
                                let old_len = old_indent.chars().count();
                                self.delete_with_undo(line_start, line_start + old_len);
                                if !new_indent.is_empty() {
                                    self.insert_with_undo(line_start, &new_indent);
                                }
                                let diff = old_len - new_indent.chars().count();
                                self.view_mut().cursor.col =
                                    self.view().cursor.col.saturating_sub(diff);
                            }
                        }
                    }
                    // Trigger signature help after '(' or ','
                    if ch == '(' || ch == ',' {
                        self.ensure_lsp_manager();
                        self.lsp_request_signature_help();
                    }
                }
                if *changed {
                    self.trigger_auto_completion();
                }
            }
        }
    }

    /// Bulk-insert `text` at the cursor in Insert mode.
    /// Unlike feeding each character through `handle_key()`, this inserts the
    /// entire string in one `insert_with_undo()` call and runs expensive
    /// post-processing (syntax reparse, bracket match, auto-completion, etc.)
    /// only once at the end.  This makes pasting large text instant instead of
    /// O(n) tree-sitter reparses.
    pub fn paste_in_insert_mode(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        // Dismiss AI ghost text and completion popup (same as regular insert keys).
        if self.ai_ghost_text.is_some() {
            self.ai_ghost_clear();
        }
        if self.completion_idx.is_some() {
            self.dismiss_completion();
        }

        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let char_idx = self.buffer().line_to_char(line) + col;

        // Build the final string to insert.
        // Replace \r\n and \r with \n. Do NOT auto-indent — pasted text
        // already carries its own whitespace; adding indent causes a
        // cumulative staircase effect (see #65).
        let to_insert = text.replace("\r\n", "\n").replace('\r', "\n");

        self.insert_with_undo(char_idx, &to_insert);
        self.insert_text_buffer.push_str(&to_insert);

        // Update cursor position: count newlines and find final line/col.
        let newlines = to_insert.chars().filter(|&c| c == '\n').count();
        if newlines > 0 {
            self.view_mut().cursor.line = line + newlines;
            let last_nl = to_insert.rfind('\n').unwrap();
            self.view_mut().cursor.col = to_insert[last_nl + 1..].chars().count();
        } else {
            self.view_mut().cursor.col = col + to_insert.chars().count();
        }

        // Run post-change bookkeeping once (normally done per-char in handle_key).
        let cur = self.view().cursor;
        self.last_edit_pos = Some((cur.line, cur.col));
        self.push_change_location(cur.line, cur.col);
        self.set_dirty(true);
        self.update_syntax();
        let active_id = self.active_buffer_id();
        if self.preview_buffer_id == Some(active_id) {
            self.promote_preview(active_id);
        }
        self.lsp_dirty_buffers.insert(active_id, true);
        self.refresh_md_previews();
        self.swap_mark_dirty();
        if !self.search_matches.is_empty() {
            self.run_search();
        }
        self.ensure_cursor_visible();
        self.sync_scroll_binds();
        self.update_bracket_match();
        self.trigger_auto_completion();
    }

    /// Insert `text` at the current command-line cursor position and advance the cursor.
    /// Used by backends to paste clipboard content into the command line.
    pub fn command_insert_str(&mut self, text: &str) {
        let byte_off = cmd_char_to_byte(&self.command_buffer, self.command_cursor);
        self.command_buffer.insert_str(byte_off, text);
        self.command_cursor += text.chars().count();
    }

    /// Clear the wildmenu completion state.
    pub(crate) fn wildmenu_clear(&mut self) {
        self.wildmenu_items.clear();
        self.wildmenu_selected = None;
        self.wildmenu_original.clear();
    }

    pub(crate) fn handle_command_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
    ) -> EngineAction {
        // --- Ctrl-R: activate / cycle reverse history search ---
        if ctrl && key_name == "r" {
            if self.history.command_history.is_empty() {
                return EngineAction::None;
            }
            if !self.history_search_active {
                // Enter history search: save current command buffer
                self.history_search_active = true;
                self.history_search_query = String::new();
                self.history_search_index = None;
                self.command_typing_buffer = self.command_buffer.clone();
            }
            // Find next (older) match from current index
            self.history_search_step(true);
            return EngineAction::None;
        }

        // --- Ctrl-G: cancel history search ---
        if ctrl && key_name == "g" && self.history_search_active {
            self.history_search_active = false;
            self.history_search_query.clear();
            self.history_search_index = None;
            self.command_buffer = self.command_typing_buffer.clone();
            self.command_cursor = self.command_buffer.chars().count();
            self.command_typing_buffer.clear();
            return EngineAction::None;
        }

        // --- Ctrl-A / Ctrl-E: move cursor to start/end of command line ---
        if ctrl && key_name == "a" && !self.history_search_active {
            self.command_cursor = 0;
            return EngineAction::None;
        }
        if ctrl && key_name == "e" && !self.history_search_active {
            self.command_cursor = self.command_buffer.chars().count();
            return EngineAction::None;
        }
        // --- Ctrl-K: kill to end of line ---
        if ctrl && key_name == "k" && !self.history_search_active {
            let byte_off = cmd_char_to_byte(&self.command_buffer, self.command_cursor);
            self.command_buffer.truncate(byte_off);
            return EngineAction::None;
        }
        // --- Ctrl-V: paste from clipboard ---
        if ctrl && key_name == "v" && !self.history_search_active {
            if let Some(text) = Self::clipboard_paste() {
                let line = text.lines().next().unwrap_or("");
                for ch in line.chars() {
                    if !ch.is_control() {
                        let byte_off = cmd_char_to_byte(&self.command_buffer, self.command_cursor);
                        self.command_buffer.insert(byte_off, ch);
                        self.command_cursor += 1;
                    }
                }
            }
            return EngineAction::None;
        }

        match key_name {
            "Escape" => {
                self.wildmenu_clear();
                if self.history_search_active {
                    // Cancel history search, restore original buffer
                    self.history_search_active = false;
                    self.history_search_query.clear();
                    self.history_search_index = None;
                    self.command_buffer = self.command_typing_buffer.clone();
                    self.command_cursor = self.command_buffer.chars().count();
                    self.command_typing_buffer.clear();
                } else {
                    self.mode = if self.is_vscode_mode() {
                        Mode::Insert
                    } else {
                        Mode::Normal
                    };
                    // Clear visual state if we came from visual mode
                    if self.command_from_visual.is_some() {
                        self.visual_anchor = None;
                        self.command_from_visual = None;
                    }
                    self.command_buffer.clear();
                    self.command_cursor = 0;
                    self.command_history_index = None;
                    self.command_typing_buffer.clear();
                }
                EngineAction::None
            }
            "Return" => {
                self.wildmenu_clear();
                self.mode = Mode::Normal;
                // If in history search, the matched command is already in command_buffer
                self.history_search_active = false;
                self.history_search_query.clear();
                self.history_search_index = None;

                let cmd = self.command_buffer.clone();
                self.command_buffer.clear();
                self.command_cursor = 0;
                self.history.add_command(&cmd);
                self.command_history_index = None;
                self.command_typing_buffer.clear();
                // Clear visual state before executing (anchor is still available
                // for `'<,'>` range resolution via get_visual_selection_range)
                let was_from_visual = self.command_from_visual.take();
                let _ = self.session.save();
                let _ = self.history.save();
                let result = self.execute_command(&cmd);
                // Clear visual anchor after execution
                if was_from_visual.is_some() {
                    self.visual_anchor = None;
                }
                // If still in VSCode mode after the command, return to Insert (EDIT) mode.
                // (If the command switched to Vim mode, is_vscode_mode() will be false,
                //  so mode stays Normal — which is correct.)
                if self.is_vscode_mode() {
                    self.mode = Mode::Insert;
                }
                result
            }
            "Left" => {
                if !self.history_search_active && self.command_cursor > 0 {
                    self.command_cursor -= 1;
                }
                EngineAction::None
            }
            "Right" => {
                if !self.history_search_active {
                    let len = self.command_buffer.chars().count();
                    if self.command_cursor < len {
                        self.command_cursor += 1;
                    }
                }
                EngineAction::None
            }
            "Home" => {
                if !self.history_search_active {
                    self.command_cursor = 0;
                }
                EngineAction::None
            }
            "End" => {
                if !self.history_search_active {
                    self.command_cursor = self.command_buffer.chars().count();
                }
                EngineAction::None
            }
            "Delete" => {
                if !self.history_search_active {
                    let len = self.command_buffer.chars().count();
                    if self.command_cursor < len {
                        let byte_off = cmd_char_to_byte(&self.command_buffer, self.command_cursor);
                        let next_off =
                            cmd_char_to_byte(&self.command_buffer, self.command_cursor + 1);
                        self.command_buffer.drain(byte_off..next_off);
                    }
                }
                EngineAction::None
            }
            "Up" => {
                // Exit history search first
                self.history_search_active = false;
                self.history_search_query.clear();
                self.history_search_index = None;

                if self.history.command_history.is_empty() {
                    return EngineAction::None;
                }
                if self.command_history_index.is_none() {
                    self.command_typing_buffer = self.command_buffer.clone();
                    self.command_history_index = Some(self.history.command_history.len() - 1);
                } else if let Some(idx) = self.command_history_index {
                    if idx > 0 {
                        self.command_history_index = Some(idx - 1);
                    }
                }
                if let Some(idx) = self.command_history_index {
                    if let Some(cmd) = self.history.command_history.get(idx) {
                        self.command_buffer = cmd.clone();
                        self.command_cursor = self.command_buffer.chars().count();
                    }
                }
                EngineAction::None
            }
            "Down" => {
                // Exit history search first
                self.history_search_active = false;
                self.history_search_query.clear();
                self.history_search_index = None;

                if self.command_history_index.is_none() {
                    return EngineAction::None;
                }
                let idx = self.command_history_index.unwrap();
                if idx + 1 >= self.history.command_history.len() {
                    self.command_buffer = self.command_typing_buffer.clone();
                    self.command_cursor = self.command_buffer.chars().count();
                    self.command_history_index = None;
                } else {
                    self.command_history_index = Some(idx + 1);
                    if let Some(cmd) = self.history.command_history.get(idx + 1) {
                        self.command_buffer = cmd.clone();
                        self.command_cursor = self.command_buffer.chars().count();
                    }
                }
                EngineAction::None
            }
            "Tab" | "ISO_Left_Tab" => {
                // Exit history search, then complete
                self.history_search_active = false;
                self.history_search_query.clear();
                self.history_search_index = None;

                let is_backtab = key_name == "ISO_Left_Tab";

                if !self.wildmenu_items.is_empty() {
                    // Wildmenu already open — cycle through items
                    if is_backtab {
                        // Shift-Tab: cycle backwards
                        match self.wildmenu_selected {
                            None | Some(0) => {
                                self.wildmenu_selected = Some(self.wildmenu_items.len() - 1);
                            }
                            Some(i) => {
                                self.wildmenu_selected = Some(i - 1);
                            }
                        }
                    } else {
                        // Tab: cycle forwards
                        match self.wildmenu_selected {
                            None => {
                                self.wildmenu_selected = Some(0);
                            }
                            Some(i) if i + 1 >= self.wildmenu_items.len() => {
                                self.wildmenu_selected = Some(0);
                            }
                            Some(i) => {
                                self.wildmenu_selected = Some(i + 1);
                            }
                        }
                    }
                    // Update command buffer to selected item
                    if let Some(idx) = self.wildmenu_selected {
                        self.command_buffer = self.wildmenu_items[idx].clone();
                        self.command_cursor = self.command_buffer.chars().count();
                        // If selected item ends with space, it takes an argument —
                        // clear wildmenu so next Tab triggers argument completion.
                        if self.command_buffer.ends_with(' ') {
                            self.wildmenu_clear();
                        }
                    }
                } else {
                    // First Tab press — compute completions
                    let partial = self.command_buffer.clone();
                    let completions = self.complete_command(&partial);
                    if completions.is_empty() {
                        return EngineAction::None;
                    } else if completions.len() == 1 {
                        // Single match: auto-complete, no wildmenu
                        self.command_buffer = completions[0].clone();
                        self.command_cursor = self.command_buffer.chars().count();
                    } else {
                        // Multiple matches: expand common prefix & show wildmenu
                        let common = Self::find_common_prefix(&completions);
                        self.wildmenu_original = partial;
                        self.wildmenu_items = completions;
                        self.wildmenu_selected = None;
                        if common.len() > self.command_buffer.len() {
                            self.command_buffer = common;
                            self.command_cursor = self.command_buffer.chars().count();
                        }
                    }
                }
                EngineAction::None
            }
            "BackSpace" => {
                self.wildmenu_clear();
                if self.history_search_active {
                    // Remove last char from search query and re-search
                    self.history_search_query.pop();
                    self.history_search_index = None; // restart from most recent
                    self.history_search_step(false);
                } else {
                    self.command_history_index = None;
                    self.command_typing_buffer.clear();
                    if self.command_cursor > 0 {
                        self.command_cursor -= 1;
                        let byte_off = cmd_char_to_byte(&self.command_buffer, self.command_cursor);
                        let next_off =
                            cmd_char_to_byte(&self.command_buffer, self.command_cursor + 1);
                        self.command_buffer.drain(byte_off..next_off);
                    } else {
                        // Backspace at position 0 with empty buffer exits command mode.
                        // This matches Vim: you must press Backspace once on the empty
                        // line (not while there are still characters) to exit.
                        if self.command_buffer.is_empty() {
                            self.mode = Mode::Normal;
                        }
                    }
                }
                EngineAction::None
            }
            _ => {
                self.wildmenu_clear();
                if self.history_search_active {
                    // Append char to search query and find match
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            self.history_search_query.push(ch);
                            self.history_search_index = None; // restart from most recent
                            self.history_search_step(false);
                        }
                    }
                } else {
                    self.command_history_index = None;
                    self.command_typing_buffer.clear();
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            let byte_off =
                                cmd_char_to_byte(&self.command_buffer, self.command_cursor);
                            self.command_buffer.insert(byte_off, ch);
                            self.command_cursor += 1;
                        }
                    } else {
                        // Try plugin command-mode keymaps for unhandled special keys
                        self.plugin_run_keymap("c", key_name);
                    }
                }
                EngineAction::None
            }
        }
    }

    /// Find a history match for the current `history_search_query`.
    /// If `next` is true, start searching one step older than `history_search_index`.
    /// Updates `command_buffer` with the match, or shows "no match" message.
    pub(crate) fn history_search_step(&mut self, next: bool) {
        let query = self.history_search_query.clone();
        let history = &self.history.command_history;
        if history.is_empty() {
            return;
        }

        // Determine start index: search from end (most recent) backwards
        let start = if next {
            // Step one older than current match
            match self.history_search_index {
                Some(0) => {
                    self.message = "(reverse-i-search): no more matches".to_string();
                    return;
                }
                Some(idx) => idx - 1,
                None => history.len() - 1,
            }
        } else {
            history.len() - 1
        };

        // Search backwards from start
        let found = (0..=start)
            .rev()
            .find(|&i| history[i].contains(query.as_str()));

        match found {
            Some(idx) => {
                self.history_search_index = Some(idx);
                self.command_buffer = history[idx].clone();
                self.command_cursor = self.command_buffer.chars().count();
                self.message.clear();
            }
            None => {
                self.message = format!("(reverse-i-search): no match for '{}'", query);
            }
        }
    }

    pub(crate) fn handle_search_key(&mut self, key_name: &str, unicode: Option<char>, ctrl: bool) {
        // Ctrl-A / Ctrl-E: move cursor to start/end
        if ctrl && key_name == "a" {
            self.command_cursor = 0;
            return;
        }
        if ctrl && key_name == "e" {
            self.command_cursor = self.command_buffer.chars().count();
            return;
        }
        // Ctrl-K: kill to end of search query
        if ctrl && key_name == "k" {
            let byte_off = cmd_char_to_byte(&self.command_buffer, self.command_cursor);
            self.command_buffer.truncate(byte_off);
            if self.settings.incremental_search {
                self.perform_incremental_search();
            }
            return;
        }
        // Ctrl-V: paste from clipboard
        if ctrl && key_name == "v" {
            if let Some(text) = Self::clipboard_paste() {
                let line = text.lines().next().unwrap_or("");
                for ch in line.chars() {
                    if !ch.is_control() {
                        let byte_off = cmd_char_to_byte(&self.command_buffer, self.command_cursor);
                        self.command_buffer.insert(byte_off, ch);
                        self.command_cursor += 1;
                    }
                }
                if self.settings.incremental_search {
                    self.perform_incremental_search();
                }
            }
            return;
        }
        match key_name {
            "Escape" => {
                self.mode = Mode::Normal;
                self.command_buffer.clear();
                self.search_history_index = None;
                self.search_typing_buffer.clear();

                // Restore cursor to original position (incremental search)
                if let Some(start_cursor) = self.search_start_cursor.take() {
                    self.view_mut().cursor = start_cursor;
                    // Clear search matches and query
                    self.search_matches.clear();
                    self.search_index = None;
                    self.search_query.clear();
                }
            }
            "Return" => {
                self.mode = Mode::Normal;
                let query = self.command_buffer.clone();
                self.command_buffer.clear();

                // Add to search history
                if !query.is_empty() {
                    // Push the pre-search position to the jump list so Ctrl-O returns here.
                    // With incremental search the cursor has already moved; push the saved
                    // start position (where the cursor was before the user typed `/`).
                    if let Some(start) = self.search_start_cursor {
                        let live = self.view().cursor;
                        self.view_mut().cursor = start;
                        self.push_jump_location();
                        self.view_mut().cursor = live;
                    }

                    self.search_start_cursor = None; // Clear saved cursor position

                    self.history.add_search(&query);
                    self.search_history_index = None;
                    self.search_typing_buffer.clear();

                    // Save session state
                    let _ = self.session.save();
                    let _ = self.history.save();

                    self.search_query = query;
                    self.run_search();
                    // If incremental search is enabled, cursor is already at the correct match
                    // Otherwise, jump to first match in the appropriate direction
                    if !self.settings.incremental_search {
                        match self.search_direction {
                            SearchDirection::Forward => self.search_next(),
                            SearchDirection::Backward => self.search_prev(),
                        }
                    }
                } else {
                    self.search_start_cursor = None;
                    // Empty query with existing search — repeat in current direction
                    if !self.search_query.is_empty() {
                        self.run_search();
                        match self.search_direction {
                            SearchDirection::Forward => self.search_next(),
                            SearchDirection::Backward => self.search_prev(),
                        }
                    }
                }
            }
            "Up" => {
                // Cycle to previous search
                if self.history.search_history.is_empty() {
                    return;
                }

                // First Up press: save current typing
                if self.search_history_index.is_none() {
                    self.search_typing_buffer = self.command_buffer.clone();
                    self.search_history_index = Some(self.history.search_history.len() - 1);
                } else if let Some(idx) = self.search_history_index {
                    if idx > 0 {
                        self.search_history_index = Some(idx - 1);
                    }
                }

                // Load history entry
                if let Some(idx) = self.search_history_index {
                    if let Some(query) = self.history.search_history.get(idx) {
                        self.command_buffer = query.clone();
                        self.command_cursor = self.command_buffer.chars().count();
                    }
                }
            }
            "Down" => {
                // Cycle to next search (or back to typing buffer)
                if self.search_history_index.is_none() {
                    return;
                }

                let idx = self.search_history_index.unwrap();
                if idx + 1 >= self.history.search_history.len() {
                    // Reached end, restore typing buffer
                    self.command_buffer = self.search_typing_buffer.clone();
                    self.command_cursor = self.command_buffer.chars().count();
                    self.search_history_index = None;
                } else {
                    self.search_history_index = Some(idx + 1);
                    if let Some(query) = self.history.search_history.get(idx + 1) {
                        self.command_buffer = query.clone();
                        self.command_cursor = self.command_buffer.chars().count();
                    }
                }
            }
            "Left" => {
                if self.command_cursor > 0 {
                    self.command_cursor -= 1;
                }
            }
            "Right" => {
                let len = self.command_buffer.chars().count();
                if self.command_cursor < len {
                    self.command_cursor += 1;
                }
            }
            "Home" => {
                self.command_cursor = 0;
            }
            "End" => {
                self.command_cursor = self.command_buffer.chars().count();
            }
            "Delete" => {
                self.search_history_index = None;
                self.search_typing_buffer.clear();
                let len = self.command_buffer.chars().count();
                if self.command_cursor < len {
                    let byte_off = cmd_char_to_byte(&self.command_buffer, self.command_cursor);
                    let next_off = cmd_char_to_byte(&self.command_buffer, self.command_cursor + 1);
                    self.command_buffer.drain(byte_off..next_off);
                    if self.settings.incremental_search {
                        self.perform_incremental_search();
                    }
                }
            }
            "BackSpace" => {
                // Reset history navigation when editing
                self.search_history_index = None;
                self.search_typing_buffer.clear();

                if self.command_cursor > 0 {
                    self.command_cursor -= 1;
                    let byte_off = cmd_char_to_byte(&self.command_buffer, self.command_cursor);
                    let next_off = cmd_char_to_byte(&self.command_buffer, self.command_cursor + 1);
                    self.command_buffer.drain(byte_off..next_off);
                }
                if self.command_buffer.is_empty() {
                    self.mode = Mode::Normal;
                    // Restore cursor to original position
                    if let Some(start_cursor) = self.search_start_cursor.take() {
                        self.view_mut().cursor = start_cursor;
                        self.search_matches.clear();
                        self.search_index = None;
                        self.search_query.clear();
                    }
                } else if self.settings.incremental_search {
                    // Incremental search: update search as user types
                    self.perform_incremental_search();
                }
            }
            _ => {
                // Reset history navigation when typing
                self.search_history_index = None;
                self.search_typing_buffer.clear();

                if let Some(ch) = unicode {
                    if !ch.is_control() {
                        let byte_off = cmd_char_to_byte(&self.command_buffer, self.command_cursor);
                        self.command_buffer.insert(byte_off, ch);
                        self.command_cursor += 1;
                        // Incremental search: update search as user types
                        if self.settings.incremental_search {
                            self.perform_incremental_search();
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn handle_visual_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        changed: &mut bool,
    ) -> EngineAction {
        // Handle Escape to exit visual mode
        if key_name == "Escape" {
            self.mode = Mode::Normal;
            self.visual_anchor = None;
            self.visual_dollar = false;
            self.count = None; // Clear count on mode exit
            return EngineAction::None;
        }

        // Leader key in visual mode
        if self.leader_partial.is_some() {
            return self.handle_leader_key(unicode);
        }
        if !ctrl && self.pending_key.is_none() && unicode == Some(self.settings.leader) {
            self.leader_partial = Some(String::new());
            return EngineAction::None;
        }

        // Handle digit accumulation for count (same logic as normal mode)
        if let Some(ch) = unicode {
            if ch.is_ascii_digit() {
                let digit = ch.to_digit(10).unwrap() as usize;
                // Special case: '0' alone should NOT start count accumulation (reserved for column 0)
                // But '0' after a digit (like "10") should accumulate
                if digit == 0 && self.count.is_none() {
                    // Let '0' be handled as a motion command (go to column 0)
                } else {
                    // Accumulate digit into count
                    let current = self.count.unwrap_or(0);
                    let new_count = current * 10 + digit;
                    if new_count > 10000 {
                        self.message = "Count limited to 10,000".to_string();
                        self.count = Some(10000);
                    } else {
                        self.count = Some(new_count);
                    }
                    return EngineAction::None;
                }
            }
        }

        // Handle Ctrl-V for visual block mode switching
        if ctrl && key_name == "v" {
            if self.mode == Mode::VisualBlock {
                // Exit to normal mode
                self.mode = Mode::Normal;
                self.visual_anchor = None;
                self.count = None;
            } else {
                // Switch to VisualBlock mode, preserve anchor
                self.mode = Mode::VisualBlock;
            }
            return EngineAction::None;
        }

        // Handle mode switching: v toggles to Visual, V toggles to VisualLine
        if let Some(ch) = unicode {
            match ch {
                'v' => {
                    if self.mode == Mode::Visual {
                        // Exit to normal mode
                        self.mode = Mode::Normal;
                        self.visual_anchor = None;
                        self.count = None; // Clear count on mode exit
                    } else {
                        // Switch to Visual mode, preserve anchor
                        self.mode = Mode::Visual;
                    }
                    return EngineAction::None;
                }
                'V' => {
                    if self.mode == Mode::VisualLine {
                        // Exit to normal mode
                        self.mode = Mode::Normal;
                        self.visual_anchor = None;
                        self.count = None; // Clear count on mode exit
                    } else {
                        // Switch to VisualLine mode, preserve anchor
                        self.mode = Mode::VisualLine;
                    }
                    return EngineAction::None;
                }
                _ => {}
            }
        }

        // Register selection: "x sets selected_register for next operation
        // Only trigger when no pending_key is active (i" / a" are text objects, not register)
        if !ctrl && unicode == Some('"') && self.pending_key.is_none() {
            self.pending_key = Some('"');
            return EngineAction::None;
        }

        // Handle text objects (iw, aw, i", a(, etc.) - set pending key
        // Skip when ctrl is held (Ctrl-A/Ctrl-I are not text objects)
        // Skip when pending_key is already set (e.g. "x register selection in progress)
        if !ctrl && self.pending_key.is_none() {
            if let Some(ch) = unicode {
                if ch == 'i' || ch == 'a' {
                    self.pending_key = Some(ch);
                    return EngineAction::None;
                }
            }
        }

        // Handle operators: d (delete), y (yank), c (change), u (lowercase), U (uppercase)
        // Note: count is NOT applied to visual operators - they operate on the selection
        if let Some(ch) = unicode {
            match ch {
                'p' | 'P' if self.pending_key.is_none() => {
                    self.count = None;
                    self.paste_visual_selection(changed);
                    return EngineAction::None;
                }
                'd' | 'x' if !ctrl && self.pending_key.is_none() => {
                    self.count = None; // Clear count (not used for visual operators)
                    self.delete_visual_selection(changed);
                    return EngineAction::None;
                }
                'y' => {
                    self.count = None; // Clear count (not used for visual operators)
                    self.yank_visual_selection();
                    return EngineAction::None;
                }
                'c' if self.pending_key.is_none() => {
                    self.count = None; // Clear count (not used for visual operators)
                    self.change_visual_selection(changed);
                    return EngineAction::None;
                }
                'u' if !ctrl && self.pending_key.is_none() => {
                    self.count = None; // Clear count (not used for visual operators)
                    self.lowercase_visual_selection(changed);
                    return EngineAction::None;
                }
                'U' if self.pending_key.is_none() => {
                    self.count = None; // Clear count (not used for visual operators)
                    self.uppercase_visual_selection(changed);
                    return EngineAction::None;
                }
                'J' if self.pending_key.is_none() => {
                    // Visual J: join all selected lines
                    self.count = None;
                    if let Some((start, end)) = self.get_visual_selection_range() {
                        let start_line = start.line;
                        let end_line = end.line;
                        let line_count = end_line - start_line + 1;
                        // Exit visual mode first
                        self.mode = Mode::Normal;
                        self.visual_anchor = None;
                        // Move cursor to start of selection, then join
                        self.view_mut().cursor.line = start_line;
                        self.view_mut().cursor.col = start.col;
                        if line_count > 1 {
                            self.join_lines(line_count, changed);
                        }
                    }
                    return EngineAction::None;
                }
                '>' => {
                    // Visual indent: indent all selected lines
                    self.count = None;
                    if let Some((start, end)) = self.get_visual_selection_range() {
                        let start_line = start.line;
                        let end_line = end.line;
                        let line_count = end_line - start_line + 1;
                        // Exit visual mode first
                        self.mode = Mode::Normal;
                        self.visual_anchor = None;
                        self.indent_lines(start_line, line_count, changed);
                        self.view_mut().cursor.line = start_line;
                        self.last_change = Some(Change {
                            op: ChangeOp::Indent,
                            text: String::new(),
                            count: line_count,
                            motion: None,
                        });
                    }
                    return EngineAction::None;
                }
                '<' => {
                    // Visual dedent: dedent all selected lines
                    self.count = None;
                    if let Some((start, end)) = self.get_visual_selection_range() {
                        let start_line = start.line;
                        let end_line = end.line;
                        let line_count = end_line - start_line + 1;
                        // Exit visual mode first
                        self.mode = Mode::Normal;
                        self.visual_anchor = None;
                        self.dedent_lines(start_line, line_count, changed);
                        self.view_mut().cursor.line = start_line;
                        self.last_change = Some(Change {
                            op: ChangeOp::Dedent,
                            text: String::new(),
                            count: line_count,
                            motion: None,
                        });
                    }
                    return EngineAction::None;
                }
                '~' => {
                    // Visual toggle case
                    self.count = None;
                    self.transform_visual_selection(
                        |s| {
                            s.chars()
                                .map(|c| {
                                    if c.is_uppercase() {
                                        c.to_lowercase().next().unwrap_or(c)
                                    } else if c.is_lowercase() {
                                        c.to_uppercase().next().unwrap_or(c)
                                    } else {
                                        c
                                    }
                                })
                                .collect()
                        },
                        changed,
                    );
                    return EngineAction::None;
                }
                ':' => {
                    self.command_from_visual = Some(self.mode);
                    self.mode = Mode::Command;
                    self.command_buffer = "'<,'>".to_string();
                    self.command_cursor = self.command_buffer.chars().count();
                    self.count = None;
                    return EngineAction::None;
                }
                'o' => {
                    // o: swap cursor to other end of selection
                    self.count = None;
                    if let Some(anchor) = self.visual_anchor {
                        let cursor = self.view().cursor;
                        self.visual_anchor = Some(cursor);
                        self.view_mut().cursor = anchor;
                    }
                    return EngineAction::None;
                }
                'r' => {
                    // r{char}: replace all selected characters
                    self.pending_key = Some('r');
                    return EngineAction::None;
                }
                'O' => {
                    // O in VisualBlock: swap to opposite column corner
                    self.count = None;
                    if self.mode == Mode::VisualBlock {
                        if let Some(anchor) = self.visual_anchor {
                            let cursor_col = self.view().cursor.col;
                            let anchor_col = anchor.col;
                            self.view_mut().cursor.col = anchor_col;
                            self.visual_anchor = Some(Cursor {
                                line: anchor.line,
                                col: cursor_col,
                            });
                        }
                    } else {
                        // In Visual/VisualLine O is same as o
                        if let Some(anchor) = self.visual_anchor {
                            let cursor = self.view().cursor;
                            self.visual_anchor = Some(cursor);
                            self.view_mut().cursor = anchor;
                        }
                    }
                    return EngineAction::None;
                }
                'I' => {
                    // Visual block I: insert at left column of block on all lines
                    if self.mode == Mode::VisualBlock {
                        if let Some(anchor) = self.visual_anchor {
                            let cursor = self.view().cursor;
                            let start_line = anchor.line.min(cursor.line);
                            let end_line = anchor.line.max(cursor.line);
                            let left_col = anchor.col.min(cursor.col);
                            // Store block info for applying on Escape
                            self.visual_block_insert_info =
                                Some((start_line, end_line, left_col, false, false));
                            // Exit visual mode and enter insert at left col of first line
                            self.mode = Mode::Insert;
                            self.visual_anchor = None;
                            self.view_mut().cursor.line = start_line;
                            self.view_mut().cursor.col = left_col;
                            self.start_undo_group();
                            self.insert_text_buffer.clear();
                        }
                    } else {
                        // In Visual/VisualLine, I acts like normal I (go to first non-blank + insert)
                        self.count = None;
                        self.change_visual_selection(changed);
                    }
                    return EngineAction::None;
                }
                'A' => {
                    // Visual block A: append at right column of block on all lines.
                    // When the block was started with $, use each line's actual end
                    // instead of the captured column (Vim "virtual end" behaviour).
                    if self.mode == Mode::VisualBlock {
                        if let Some(anchor) = self.visual_anchor {
                            let cursor = self.view().cursor;
                            let start_line = anchor.line.min(cursor.line);
                            let end_line = anchor.line.max(cursor.line);
                            let virtual_end = self.visual_dollar;
                            let first_line_col = if virtual_end {
                                // Actual end of content (excluding trailing newline) of first line.
                                let line_len = self.buffer().line_len_chars(start_line);
                                let line_start = self.buffer().line_to_char(start_line);
                                if line_len > 0
                                    && self.buffer().content.char(line_start + line_len - 1) == '\n'
                                {
                                    line_len - 1
                                } else {
                                    line_len
                                }
                            } else {
                                anchor.col.max(cursor.col) + 1
                            };
                            self.visual_block_insert_info =
                                Some((start_line, end_line, first_line_col, true, virtual_end));
                            // Exit visual mode and enter insert at the chosen col of first line
                            self.mode = Mode::Insert;
                            self.visual_anchor = None;
                            self.visual_dollar = false;
                            self.view_mut().cursor.line = start_line;
                            self.view_mut().cursor.col = first_line_col;
                            self.start_undo_group();
                            self.insert_text_buffer.clear();
                        }
                    } else {
                        // In Visual/VisualLine, A acts like normal A (go to end of line + insert)
                        self.count = None;
                        self.change_visual_selection(changed);
                    }
                    return EngineAction::None;
                }
                _ => {}
            }
        }

        // Handle navigation keys (extend selection)
        // These use the same movement logic as normal mode (fold-aware)
        if ctrl {
            match key_name {
                "d" => {
                    let count = self.take_count();
                    let half = self.viewport_lines() / 2;
                    let scroll_amount = half * count;
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    let cur = self.view().cursor.line;
                    let new_line = self.view().next_visible_line(cur, scroll_amount, max_line);
                    self.view_mut().cursor.line = new_line;
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "u" => {
                    let count = self.take_count();
                    let half = self.viewport_lines() / 2;
                    let scroll_amount = half * count;
                    let cur = self.view().cursor.line;
                    let new_line = self.view().prev_visible_line(cur, scroll_amount);
                    self.view_mut().cursor.line = new_line;
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "f" => {
                    let count = self.take_count();
                    let viewport = self.viewport_lines();
                    let scroll_amount = viewport * count;
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    let cur = self.view().cursor.line;
                    let new_line = self.view().next_visible_line(cur, scroll_amount, max_line);
                    self.view_mut().cursor.line = new_line;
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                "b" => {
                    let count = self.take_count();
                    let viewport = self.viewport_lines();
                    let scroll_amount = viewport * count;
                    let cur = self.view().cursor.line;
                    let new_line = self.view().prev_visible_line(cur, scroll_amount);
                    self.view_mut().cursor.line = new_line;
                    self.clamp_cursor_col();
                    return EngineAction::None;
                }
                _ => {}
            }
        }

        // Handle multi-key sequences (gg, {, }, text objects, register selection)
        if let Some(pending) = self.pending_key.take() {
            if pending == '"' {
                // Register selection: "x (uppercase A-Z appends to lowercase)
                if let Some(ch) = unicode {
                    if ch.is_ascii_lowercase()
                        || ch.is_ascii_uppercase()
                        || ch.is_ascii_digit()
                        || ch == '"'
                        || ch == '+'
                        || ch == '*'
                    {
                        self.selected_register = Some(ch);
                    }
                }
                return EngineAction::None;
            }
            if pending == 'i' || pending == 'a' {
                // Text object selection
                if let Some(obj_type) = unicode {
                    let cursor = self.view().cursor;
                    let cursor_pos = self.buffer().line_to_char(cursor.line) + cursor.col;

                    if let Some((start_pos, end_pos)) =
                        self.find_text_object_range(pending, obj_type, cursor_pos)
                    {
                        // Set visual selection to the text object range
                        let start_line = self.buffer().content.char_to_line(start_pos);
                        let start_line_char = self.buffer().line_to_char(start_line);
                        let start_col = start_pos - start_line_char;

                        let end_line = self
                            .buffer()
                            .content
                            .char_to_line(end_pos.saturating_sub(1).max(start_pos));
                        let end_line_char = self.buffer().line_to_char(end_line);
                        let end_col = (end_pos - 1).saturating_sub(end_line_char);

                        self.visual_anchor = Some(Cursor {
                            line: start_line,
                            col: start_col,
                        });
                        self.view_mut().cursor.line = end_line;
                        self.view_mut().cursor.col = end_col;

                        // Switch to character visual mode for text objects
                        self.mode = Mode::Visual;
                    }
                }
                return EngineAction::None;
            } else if pending == 'r' {
                // r{char}: replace all selected characters with the given character
                if let Some(replacement) = unicode {
                    self.replace_visual_selection(replacement, changed);
                }
                return EngineAction::None;
            } else if pending == 'g' && unicode == Some('g') {
                // gg in visual mode: with count, go to line N; without count, go to first line
                if let Some(count) = self.peek_count() {
                    self.count = None; // Consume count
                    let target_line = count.saturating_sub(1); // 1-indexed to 0-indexed
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    self.view_mut().cursor.line = target_line.min(max_line);
                } else {
                    self.view_mut().cursor.line = 0;
                }
                self.view_mut().cursor.col = 0;
                return EngineAction::None;
            } else if pending == 'g' && ctrl && (key_name == "a" || key_name == "x") {
                // g Ctrl-A / g Ctrl-X in visual mode: sequential increment/decrement
                let base_count = self.take_count().max(1) as i64;
                let delta_sign: i64 = if key_name == "a" { 1 } else { -1 };
                if let Some((start, end)) = self.get_visual_selection_range() {
                    let saved_line = self.view().cursor.line;
                    let saved_col = self.view().cursor.col;
                    self.start_undo_group();
                    for (i, line) in (start.line..=end.line).enumerate() {
                        let delta = delta_sign * base_count * (i as i64 + 1);
                        self.view_mut().cursor.line = line;
                        self.view_mut().cursor.col = 0;
                        self.increment_number_at_cursor(delta, changed);
                    }
                    self.finish_undo_group();
                    self.view_mut().cursor.line = saved_line;
                    self.view_mut().cursor.col = saved_col;
                    self.mode = Mode::Normal;
                    self.visual_anchor = None;
                }
                return EngineAction::None;
            } else if pending == 'g' && unicode == Some('c') {
                // gc in visual mode: toggle comment on selected lines
                self.count = None;
                if let Some((start, end)) = self.get_visual_selection_range() {
                    let start_line = start.line + 1; // 1-indexed
                    let end_line = end.line + 1;
                    self.mode = Mode::Normal;
                    self.visual_anchor = None;
                    self.toggle_comment(start_line, end_line);
                    *changed = true;
                }
                return EngineAction::None;
            } else if pending == 'g' && (unicode == Some('q') || unicode == Some('w')) {
                // gq / gw in visual mode: format selected lines
                self.count = None;
                if let Some((start, end)) = self.get_visual_selection_range() {
                    let saved_cursor = self.view().cursor;
                    self.mode = Mode::Normal;
                    self.visual_anchor = None;
                    self.format_lines(start.line, end.line, changed);
                    if unicode == Some('w') {
                        self.view_mut().cursor = saved_cursor;
                        self.clamp_cursor_col();
                    }
                }
                return EngineAction::None;
            } else if pending == 'g' && unicode == Some('u') {
                // gu in visual mode: lowercase selection
                self.count = None;
                self.lowercase_visual_selection(changed);
                return EngineAction::None;
            } else if pending == 'g' && unicode == Some('U') {
                // gU in visual mode: uppercase selection
                self.count = None;
                self.uppercase_visual_selection(changed);
                return EngineAction::None;
            } else if pending == 'g' && unicode == Some('~') {
                // g~ in visual mode: toggle case of selection
                self.count = None;
                self.transform_visual_selection(
                    |s| {
                        s.chars()
                            .map(|c| {
                                if c.is_uppercase() {
                                    c.to_lowercase().next().unwrap_or(c)
                                } else if c.is_lowercase() {
                                    c.to_uppercase().next().unwrap_or(c)
                                } else {
                                    c
                                }
                            })
                            .collect()
                    },
                    changed,
                );
                return EngineAction::None;
            }
        }

        // Single-key navigation
        match unicode {
            Some('h') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_left();
                }
            }
            Some('j') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_down();
                }
            }
            Some('k') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_up();
                }
            }
            Some('l') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_right();
                }
            }
            Some('w') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_word_forward();
                }
            }
            Some('b') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_word_backward();
                }
            }
            Some('e') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_word_end();
                }
            }
            Some('0') => {
                self.view_mut().cursor.col = 0;
                self.visual_dollar = false;
            }
            Some('$') => {
                let line = self.view().cursor.line;
                self.view_mut().cursor.col = self.get_max_cursor_col(line);
                self.visual_dollar = true;
            }
            Some('g') => {
                self.pending_key = Some('g');
                self.visual_dollar = false;
            }
            Some('G') => {
                let last_line = self.buffer().len_lines().saturating_sub(1);
                self.view_mut().cursor.line = last_line;
                self.clamp_cursor_col();
            }
            Some('{') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_paragraph_backward();
                }
            }
            Some('}') => {
                let count = self.take_count();
                for _ in 0..count {
                    self.move_paragraph_forward();
                }
            }
            Some('%') => {
                self.move_to_matching_bracket();
            }
            _ => match key_name {
                "Left" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_left();
                    }
                }
                "Down" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_down();
                    }
                }
                "Up" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_up();
                    }
                }
                "Right" => {
                    let count = self.take_count();
                    for _ in 0..count {
                        self.move_right();
                    }
                }
                "Home" => self.view_mut().cursor.col = 0,
                "End" => {
                    let line = self.view().cursor.line;
                    self.view_mut().cursor.col = self.get_max_cursor_col(line);
                }
                _ => {}
            },
        }

        // Try plugin visual-mode keymaps as a fallback
        self.plugin_run_keymap("v", key_name);
        EngineAction::None
    }
}

// ─── Additional methods (extracted from mod.rs) ─────────────────────────

impl Engine {
    // =======================================================================
    // Repeat command (.)
    // =======================================================================

    pub(crate) fn repeat_last_change(&mut self, repeat_count: usize, changed: &mut bool) {
        let change = match &self.last_change {
            Some(c) => c.clone(),
            None => return, // No change to repeat
        };

        let final_count = if repeat_count > 1 {
            repeat_count
        } else {
            change.count
        };

        match change.op {
            ChangeOp::Insert => {
                // Repeat insert: insert the same text at current position
                self.start_undo_group();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;

                // Insert the text final_count times
                let repeated_text = change.text.repeat(final_count);
                self.insert_with_undo(char_idx, &repeated_text);

                // Update cursor position based on inserted text
                let newlines = repeated_text.matches('\n').count();
                if newlines > 0 {
                    self.view_mut().cursor.line += newlines;
                    // Find column after last newline
                    if let Some(last_nl) = repeated_text.rfind('\n') {
                        self.view_mut().cursor.col = repeated_text[last_nl + 1..].chars().count();
                    }
                } else {
                    self.view_mut().cursor.col += repeated_text.chars().count();
                }
                self.finish_undo_group();
                *changed = true;
            }
            ChangeOp::Delete => {
                // Repeat delete with motion — use final_count (not change.count)
                // so that `4x` then `.` deletes 4 chars, and `2.` deletes 2 chars.
                if let Some(motion) = &change.motion {
                    self.start_undo_group();
                    match motion {
                        Motion::Right => {
                            // Delete character(s) at cursor (like x)
                            let line = self.view().cursor.line;
                            let col = self.view().cursor.col;
                            let char_idx = self.buffer().line_to_char(line) + col;
                            let line_end = self.buffer().line_to_char(line)
                                + self.buffer().line_len_chars(line);
                            let available = line_end - char_idx;
                            let to_delete = final_count.min(available);

                            if to_delete > 0 && char_idx < self.buffer().len_chars() {
                                let deleted_chars: String = self
                                    .buffer()
                                    .content
                                    .slice(char_idx..char_idx + to_delete)
                                    .chars()
                                    .collect();
                                let reg = self.active_register();
                                self.set_register(reg, deleted_chars, false);
                                self.clear_selected_register();
                                self.delete_with_undo(char_idx, char_idx + to_delete);
                                self.clamp_cursor_col();
                                *changed = true;
                            }
                        }
                        Motion::DeleteLine => {
                            // Repeat dd
                            self.delete_lines(final_count, changed);
                        }
                        Motion::WordForward
                        | Motion::WordBackward
                        | Motion::WordEnd
                        | Motion::WordBackwardEnd => {
                            // Repeat dw/db/de/dge
                            let m = match motion {
                                Motion::WordForward => 'w',
                                Motion::WordBackward => 'b',
                                Motion::WordEnd => 'e',
                                Motion::WordBackwardEnd => 'e', // ge uses 'e' in backward direction
                                _ => unreachable!(),
                            };
                            self.apply_operator_with_motion('d', m, final_count, changed);
                        }
                        _ => {}
                    }
                    self.finish_undo_group();
                }
            }
            ChangeOp::Change => {
                // Repeat c{motion}: delete the motion range, then insert the text.
                if let Some(motion) = &change.motion {
                    for _ in 0..final_count {
                        let motion_char = match motion {
                            Motion::WordForward => 'w',
                            Motion::WordEnd => 'e',
                            Motion::WordBackward => 'b',
                            _ => continue,
                        };
                        // Reuse the same code path as the original cw/ce/cb:
                        // apply_operator_with_motion deletes the range and enters
                        // insert mode.  We then immediately insert the recorded
                        // text and return to normal mode instead.
                        let start_cursor = self.view().cursor;
                        let start_pos =
                            self.buffer().line_to_char(start_cursor.line) + start_cursor.col;
                        let total = self.buffer().len_chars();

                        // For cw dot repeat, use cw-special logic: find end of
                        // word without eating trailing whitespace.
                        let delete_end = if motion_char == 'w' || motion_char == 'W' {
                            let bigword = motion_char == 'W';
                            let mut end = start_pos;
                            for i in 0..change.count {
                                if bigword {
                                    while end < total
                                        && !self.buffer().content.char(end).is_whitespace()
                                    {
                                        end += 1;
                                    }
                                } else {
                                    while end < total
                                        && is_word_char(self.buffer().content.char(end))
                                    {
                                        end += 1;
                                    }
                                }
                                if i + 1 < change.count {
                                    while end < total
                                        && self.buffer().content.char(end).is_whitespace()
                                    {
                                        end += 1;
                                    }
                                }
                            }
                            end
                        } else {
                            for _ in 0..change.count {
                                match motion_char {
                                    'b' => self.move_word_backward(),
                                    'e' => self.move_word_end(),
                                    _ => {}
                                }
                            }
                            let end_cursor = self.view().cursor;
                            let end_pos =
                                self.buffer().line_to_char(end_cursor.line) + end_cursor.col;
                            self.view_mut().cursor = start_cursor;
                            if motion_char == 'e' {
                                (end_pos + 1).min(total)
                            } else {
                                end_pos
                            }
                        };
                        if start_pos < delete_end {
                            self.start_undo_group();
                            self.delete_with_undo(start_pos, delete_end);
                            if !change.text.is_empty() {
                                self.insert_with_undo(start_pos, &change.text);
                                let inserted_chars = change.text.chars().count();
                                let newlines = change.text.matches('\n').count();
                                if newlines > 0 {
                                    self.view_mut().cursor.line += newlines;
                                    if let Some(last_nl) = change.text.rfind('\n') {
                                        self.view_mut().cursor.col =
                                            change.text[last_nl + 1..].chars().count();
                                    }
                                } else {
                                    self.view_mut().cursor.col += inserted_chars;
                                }
                            }
                            self.clamp_cursor_col();
                            self.finish_undo_group();
                            *changed = true;
                        }
                    }
                }
            }
            ChangeOp::Substitute => {
                // Repeat s command
                for _ in 0..final_count {
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let max_col = self.get_max_cursor_col(line);
                    if max_col > 0 || self.buffer().line_len_chars(line) > 0 {
                        let char_idx = self.buffer().line_to_char(line) + col;
                        let line_end =
                            self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
                        let available = line_end - char_idx;
                        let to_delete = change.count.min(available);

                        self.start_undo_group();
                        if to_delete > 0 && char_idx < self.buffer().len_chars() {
                            self.delete_with_undo(char_idx, char_idx + to_delete);
                            *changed = true;
                        }

                        // Insert the recorded text
                        if !change.text.is_empty() {
                            self.insert_with_undo(char_idx, &change.text);
                            *changed = true;
                        }
                        self.finish_undo_group();
                    }
                }
            }
            ChangeOp::SubstituteLine | ChangeOp::DeleteToEnd | ChangeOp::ChangeToEnd => {
                // Handle other operations
            }
            ChangeOp::Replace => {
                // Repeat r command — final_count is the number of chars to replace
                if let Some(replacement_char) = change.text.chars().next() {
                    self.start_undo_group();
                    self.replace_chars(replacement_char, final_count, changed);
                    self.finish_undo_group();
                }
            }
            ChangeOp::ToggleCase => {
                // Repeat ~ command
                for _ in 0..final_count {
                    self.toggle_case_at_cursor(change.count, changed);
                }
            }
            ChangeOp::Join => {
                // Repeat J command
                for _ in 0..final_count {
                    self.join_lines(change.count, changed);
                }
            }
            ChangeOp::Indent => {
                // Repeat >> command
                let line = self.view().cursor.line;
                for _ in 0..final_count {
                    self.indent_lines(line, change.count, changed);
                }
            }
            ChangeOp::Dedent => {
                // Repeat << command
                let line = self.view().cursor.line;
                for _ in 0..final_count {
                    self.dedent_lines(line, change.count, changed);
                }
            }
        }
    }

    /// Available commands for auto-completion
    pub(crate) fn available_commands() -> &'static [&'static str] {
        &[
            // File operations
            "w",
            "q",
            "q!",
            "wq",
            "wq!",
            "wa",
            "wqa",
            "qa",
            "qa!",
            "e ",
            "e!",
            "enew",
            // Buffers
            "bn",
            "bp",
            "bd",
            "b#",
            "ls",
            "buffers",
            "files",
            // Splits & tabs
            "split",
            "vsplit",
            "close",
            "only",
            "new",
            "wincmd ",
            "tabnew",
            "tabnext",
            "tabprev",
            "tabclose",
            "tabmove",
            // Search & replace
            "s/",
            "%s/",
            "noh",
            "nohlsearch",
            // Settings & config
            "set ",
            "config reload",
            "Settings",
            "Keymaps",
            "Keybindings",
            "Keybindings ",
            "colorscheme ",
            // Editor groups
            "EditorGroupSplit",
            "EditorGroupSplitDown",
            "EditorGroupClose",
            "EditorGroupFocus",
            "EditorGroupMoveTab",
            "egsp",
            "egspd",
            "egc",
            "egf",
            "egmt",
            // Netrw / file browser
            "Explore",
            "Ex",
            "Sexplore",
            "Sex",
            "Vexplore",
            "Vex",
            // Git
            "Gdiff",
            "Gd",
            "Gdiffsplit",
            "Gds",
            "Gstatus",
            "Gs",
            "Gadd",
            "Ga",
            "Gcommit",
            "Gc",
            "Gpush",
            "Gp",
            "Gblame",
            "Gb",
            "Ghs",
            "Ghunk",
            "Gpull",
            "Gfetch",
            "Gswitch",
            "GSwitch",
            "Gsw",
            "Gbranch",
            "GBranch",
            "GWorktreeAdd",
            "GWorktreeRemove",
            "DiffPeek",
            "DiffNext",
            "DiffPrev",
            "DiffToggleContext",
            // LSP
            "LspInfo",
            "LspRestart",
            "LspStop",
            "LspInstall",
            "Lformat",
            "Rename",
            "def",
            "refs",
            "hover",
            "LspImpl",
            "LspTypedef",
            "CodeAction",
            // Navigation
            "nextdiag",
            "prevdiag",
            "nexthunk",
            "prevhunk",
            "fuzzy",
            "sidebar",
            "palette",
            // DAP / Debug
            "DapInfo",
            "DapInstall",
            "DapCondition",
            "DapHitCondition",
            "DapLogMessage",
            "DapWatch",
            "DapBottomPanel",
            "DapEval",
            "DapExpand",
            "debug",
            "continue",
            "pause",
            "stop",
            "restart",
            "stepover",
            "stepin",
            "stepout",
            "brkpt",
            // Extensions
            "ExtInstall",
            "ExtList",
            "ExtEnable",
            "ExtDisable",
            "ExtRemove",
            "ExtRefresh",
            // AI
            "AI ",
            "AiClear",
            // Markdown
            "MarkdownPreview",
            "MdPreview",
            // Display / info
            "registers",
            "display",
            "marks",
            "jumps",
            "changes",
            "history",
            "echo ",
            // Diff
            "diffthis",
            "diffoff",
            "diffsplit",
            // Misc ex commands
            "sort",
            "terminal",
            "cd ",
            "make",
            "copen",
            "cn",
            "cp",
            "cc",
            "r ",
            "norm ",
            "Plugin",
            "map",
            "unmap",
        ]
    }

    /// All setting names recognized by `:set`.
    pub(crate) fn setting_names() -> &'static [&'static str] {
        &[
            // Boolean options (full names + aliases)
            "number",
            "nu",
            "relativenumber",
            "rnu",
            "expandtab",
            "et",
            "autoindent",
            "ai",
            "incsearch",
            "is",
            "lsp",
            "wrap",
            "hlsearch",
            "hls",
            "ignorecase",
            "ic",
            "smartcase",
            "scs",
            "cursorline",
            "cul",
            "autoread",
            "ar",
            "splitbelow",
            "sb",
            "splitright",
            "spr",
            "ai_completions",
            "formatonsave",
            "fos",
            "showhiddenfiles",
            "shf",
            "swapfile",
            "breadcrumbs",
            "autohidepanels",
            // Value options
            "tabstop",
            "ts",
            "shiftwidth",
            "sw",
            "scrolloff",
            "so",
            "colorcolumn",
            "cc",
            "textwidth",
            "tw",
            "updatetime",
            "ut",
            "mode",
            "filetype",
            "ft",
        ]
    }

    /// Find completions for partial command, including argument completion.
    pub(crate) fn complete_command(&self, partial: &str) -> Vec<String> {
        if partial.is_empty() {
            return Vec::new();
        }

        // Check if we're completing an argument (text after a space)
        if let Some(space_pos) = partial.find(' ') {
            let cmd_prefix = &partial[..space_pos];
            let arg_partial = partial[space_pos + 1..].trim_start();

            return match cmd_prefix {
                "set" => {
                    // Complete setting names, including "no" prefixed variants
                    let mut results: Vec<String> = Self::setting_names()
                        .iter()
                        .filter(|name| name.starts_with(arg_partial))
                        .map(|name| format!("set {name}"))
                        .collect();
                    // Also offer "no" prefixed boolean disable variants
                    for name in Self::setting_names() {
                        let no_name = format!("no{name}");
                        if no_name.starts_with(arg_partial) && !arg_partial.is_empty() {
                            results.push(format!("set {no_name}"));
                        }
                    }
                    results.sort();
                    results.dedup();
                    results
                }
                "Keybindings" | "keybindings" => ["vim", "vscode"]
                    .iter()
                    .filter(|m| m.starts_with(arg_partial))
                    .map(|m| format!("Keybindings {m}"))
                    .collect(),
                "colorscheme" => {
                    // Complete theme names
                    let mut names = vec![
                        "onedark".to_string(),
                        "gruvbox-dark".to_string(),
                        "tokyo-night".to_string(),
                        "solarized-dark".to_string(),
                        "vscode-dark".to_string(),
                        "vscode-light".to_string(),
                        "gruvbox".to_string(),
                        "tokyonight".to_string(),
                        "solarized".to_string(),
                    ];
                    names.extend(list_custom_theme_names());
                    names.sort();
                    names.dedup();
                    names
                        .into_iter()
                        .filter(|name| name.starts_with(arg_partial))
                        .map(|name| format!("colorscheme {name}"))
                        .collect()
                }
                _ => {
                    // For other commands with trailing space in available_commands,
                    // fall through to prefix matching
                    Self::available_commands()
                        .iter()
                        .filter(|cmd| cmd.starts_with(partial))
                        .map(|s| s.to_string())
                        .collect()
                }
            };
        }

        // First-word completion
        Self::available_commands()
            .iter()
            .filter(|cmd| cmd.starts_with(partial))
            .map(|s| s.to_string())
            .collect()
    }

    /// Find common prefix of strings
    pub(crate) fn find_common_prefix(strings: &[String]) -> String {
        if strings.is_empty() {
            return String::new();
        }

        let first = &strings[0];
        let mut common = String::new();

        for (i, ch) in first.chars().enumerate() {
            if strings.iter().all(|s| s.chars().nth(i) == Some(ch)) {
                common.push(ch);
            } else {
                break;
            }
        }

        common
    }

    // ─── User keymaps ────────────────────────────────────────────────────────

    /// Rebuild the parsed user_keymaps cache from settings.keymaps.
    /// Call after loading or changing settings.
    pub fn rebuild_user_keymaps(&mut self) {
        self.user_keymaps = self
            .settings
            .keymaps
            .iter()
            .filter_map(|s| parse_keymap_def(s))
            .collect();
    }

    /// Check user keymaps for the current keypress. Returns `Some(action)` if
    /// an exact match was found, `None` to fall through to built-in handling.
    /// Handles multi-key sequences by buffering keypresses.
    pub(crate) fn try_user_keymap(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        changed: &mut bool,
    ) -> Option<EngineAction> {
        if self.keymap_replaying || self.user_keymaps.is_empty() {
            return None;
        }

        let mode_str = if self.is_vscode_mode() {
            // VSCode mode has no modal distinction; "n" keymaps apply.
            "n"
        } else {
            match self.mode {
                Mode::Normal => "n",
                Mode::Visual | Mode::VisualLine | Mode::VisualBlock => "v",
                Mode::Insert => "i",
                Mode::Command => "c",
                _ => return None,
            }
        };

        let encoded = encode_keypress(key_name, unicode, ctrl);
        self.keymap_buf.push(encoded);

        let mut exact_match_action = None;
        let mut has_prefix = false;

        for km in &self.user_keymaps {
            if km.mode != mode_str {
                continue;
            }
            if km.keys == self.keymap_buf {
                exact_match_action = Some(km.action.clone());
            } else if km.keys.len() > self.keymap_buf.len()
                && km.keys[..self.keymap_buf.len()] == self.keymap_buf[..]
            {
                has_prefix = true;
            }
        }

        if let Some(action) = exact_match_action {
            self.keymap_buf.clear();
            let count = self.take_count();
            // Substitute {count} in the action, or append count as argument
            let cmd = if action.contains("{count}") {
                action.replace("{count}", &count.to_string())
            } else if count > 1 {
                format!("{action} {count}")
            } else {
                action
            };
            *changed = true;
            return Some(self.execute_command(&cmd));
        }

        if has_prefix {
            // More keys needed — consume this keypress
            return Some(EngineAction::None);
        }

        // No match and no prefix. Replay buffered keys.
        let buf: Vec<String> = self.keymap_buf.drain(..).collect();
        if buf.len() <= 1 {
            // Single key, no match — fall through to built-in handling
            return None;
        }

        // Multi-key sequence that didn't match any keymap: replay all keys
        self.keymap_replaying = true;
        let mut last_action = EngineAction::None;
        for encoded_key in buf {
            let (rk_name, rk_unicode, rk_ctrl) = decode_keypress(&encoded_key);
            last_action = self.handle_key(&rk_name, rk_unicode, rk_ctrl);
        }
        self.keymap_replaying = false;
        Some(last_action)
    }

    /// Try to run a named plugin command. Returns `true` if the command was found.
    pub fn plugin_run_command(&mut self, name: &str, args: &str) -> bool {
        if !self.settings.plugins_enabled {
            return false;
        }
        let pm = match self.plugin_manager.take() {
            Some(p) => p,
            None => return false,
        };
        let ctx = self.make_plugin_ctx(false);
        let (found, ctx) = pm.call_command(name, args, ctx);
        self.plugin_manager = Some(pm);
        self.apply_plugin_ctx(ctx);
        found
    }

    /// Try to run a plugin keymap. Returns `true` if a mapping was found and executed.
    pub fn plugin_run_keymap(&mut self, mode: &str, key: &str) -> bool {
        if !self.settings.plugins_enabled {
            return false;
        }
        let pm = match self.plugin_manager.take() {
            Some(p) => p,
            None => return false,
        };
        let ctx = self.make_plugin_ctx(false);
        let (found, ctx) = pm.call_keymap(mode, key, ctx);
        self.plugin_manager = Some(pm);
        self.apply_plugin_ctx(ctx);
        found
    }

    /// Run the user-defined operatorfunc (g@) with the given motion type.
    /// Returns `true` if an operatorfunc was registered and executed.
    pub(crate) fn plugin_run_operatorfunc(&mut self, motion_type: &str) -> bool {
        let pm = match self.plugin_manager.take() {
            Some(p) => p,
            None => return false,
        };
        let ctx = self.make_plugin_ctx(false);
        let (found, ctx) = pm.call_operatorfunc(motion_type, ctx);
        self.plugin_manager = Some(pm);
        self.apply_plugin_ctx(ctx);
        found
    }

    // =======================================================================
    // Mouse selection (called by UI backends after coordinate conversion)
    // =======================================================================

    /// Handle a single mouse click at the given buffer position.
    /// Exits visual mode if active, positions cursor, clears drag state.
    pub fn mouse_click(&mut self, window_id: WindowId, line: usize, col: usize) {
        // Exit visual mode if active
        if matches!(
            self.mode,
            Mode::Visual | Mode::VisualLine | Mode::VisualBlock
        ) {
            self.mode = Mode::Normal;
            self.visual_anchor = None;
        }
        self.mouse_drag_word_mode = false;
        self.mouse_drag_word_origin = None;
        self.mouse_drag_active = false;
        self.mouse_drag_origin_window = None;
        // Switch to the group that owns this window.
        self.focus_group_for_window(window_id);
        self.set_cursor_for_window(window_id, line, col);
    }

    /// Handle mouse drag to the given buffer position.
    /// On first drag: enters Visual mode with anchor at current cursor.
    /// On subsequent drags: extends selection by moving cursor.
    /// If already in Visual mode (e.g. from double-click word select),
    /// preserves the existing anchor and just extends.
    pub fn mouse_drag(&mut self, window_id: WindowId, line: usize, col: usize) {
        // Lock drag to the originating window so selections don't leak
        // across editor groups.
        if let Some(origin) = self.mouse_drag_origin_window {
            if window_id != origin {
                return; // Drag crossed into another window — ignore.
            }
        }

        // Ensure this window's group and tab are active.
        self.focus_group_for_window(window_id);
        if self.windows.contains_key(&window_id) {
            self.active_tab_mut().active_window = window_id;
        }

        if !self.mouse_drag_active {
            // First drag event — only set anchor if not already in visual mode
            // (double-click word select already set the anchor at word start).
            // Also require the drag to actually reach a *different* buffer position
            // than the click origin; sub-character mouse jitter on mousedown otherwise
            // silently enters visual mode before `:` can be pressed.
            let cursor = self.view().cursor;
            let moved = line != cursor.line || col != cursor.col;
            if !moved {
                return; // Sub-pixel jitter at same cell — ignore.
            }
            if !matches!(
                self.mode,
                Mode::Visual | Mode::VisualLine | Mode::VisualBlock
            ) {
                self.visual_anchor = Some(cursor);
                self.mode = Mode::Visual;
            }
            self.mouse_drag_active = true;
            self.mouse_drag_origin_window = Some(window_id);
        }

        // Move cursor to drag position (extends visual selection)
        let buffer = self.buffer();
        let max_line = buffer.content.len_lines().saturating_sub(1);
        let clamped_line = line.min(max_line);
        let max_col = self.get_max_cursor_col(clamped_line);
        let clamped_col = col.min(max_col);

        if self.mouse_drag_word_mode {
            // Word-wise drag: snap to word boundaries
            if let Some((orig_start, orig_end, orig_line)) = self.mouse_drag_word_origin {
                let line_text: Vec<char> =
                    self.buffer().content.line(clamped_line).chars().collect();
                let drag_before_origin = clamped_line < orig_line
                    || (clamped_line == orig_line && clamped_col < orig_start);

                if drag_before_origin {
                    // Dragging before the original word — anchor at word end, cursor at word start
                    let mut word_start = clamped_col.min(line_text.len().saturating_sub(1));
                    if word_start < line_text.len() && Self::is_word_char(line_text[word_start]) {
                        while word_start > 0 && Self::is_word_char(line_text[word_start - 1]) {
                            word_start -= 1;
                        }
                    }
                    self.visual_anchor = Some(Cursor {
                        line: orig_line,
                        col: orig_end,
                    });
                    let view = self.view_mut();
                    view.cursor.line = clamped_line;
                    view.cursor.col = word_start;
                } else {
                    // Dragging after the original word — anchor at word start, cursor at word end
                    let mut word_end = clamped_col.min(line_text.len().saturating_sub(1));
                    if word_end < line_text.len() && Self::is_word_char(line_text[word_end]) {
                        while word_end + 1 < line_text.len()
                            && Self::is_word_char(line_text[word_end + 1])
                        {
                            word_end += 1;
                        }
                    }
                    // Exclude trailing newline
                    if word_end < line_text.len() && line_text[word_end] == '\n' && word_end > 0 {
                        word_end -= 1;
                    }
                    self.visual_anchor = Some(Cursor {
                        line: orig_line,
                        col: orig_start,
                    });
                    let view = self.view_mut();
                    view.cursor.line = clamped_line;
                    view.cursor.col = word_end;
                }
            }
        } else {
            let view = self.view_mut();
            view.cursor.line = clamped_line;
            view.cursor.col = clamped_col;
        }
    }

    /// Handle mouse double-click: select the word under the cursor.
    /// Positions cursor, finds word boundaries, enters Visual mode.
    pub fn mouse_double_click(&mut self, window_id: WindowId, line: usize, col: usize) {
        self.mouse_drag_active = false;
        self.mouse_drag_origin_window = None;
        self.mouse_drag_word_mode = false;
        self.mouse_drag_word_origin = None;
        self.focus_group_for_window(window_id);
        self.set_cursor_for_window(window_id, line, col);

        // Find word boundaries at cursor
        let cursor_line = self.view().cursor.line;
        let cursor_col = self.view().cursor.col;
        let line_text: Vec<char> = self.buffer().content.line(cursor_line).chars().collect();

        if cursor_col >= line_text.len() || !Self::is_word_char(line_text[cursor_col]) {
            // Clicked on non-word character — don't select
            return;
        }

        // Find word start
        let mut word_start = cursor_col;
        while word_start > 0 && Self::is_word_char(line_text[word_start - 1]) {
            word_start -= 1;
        }

        // Find word end (inclusive)
        let mut word_end = cursor_col;
        while word_end + 1 < line_text.len() && Self::is_word_char(line_text[word_end + 1]) {
            word_end += 1;
        }
        // Exclude trailing newline from word end
        if word_end < line_text.len() && line_text[word_end] == '\n' && word_end > word_start {
            word_end -= 1;
        }

        // Enter visual mode with anchor at word start, cursor at word end
        self.visual_anchor = Some(Cursor {
            line: cursor_line,
            col: word_start,
        });
        let view = self.view_mut();
        view.cursor.col = word_end;
        self.mode = Mode::Visual;
        self.mouse_drag_word_mode = true;
        self.mouse_drag_word_origin = Some((word_start, word_end, cursor_line));
    }

    // =======================================================================
    // Clipboard paste into command/search mode
    // =======================================================================

    /// Paste the first line from the system clipboard into the command buffer.
    /// Works in Command and Search modes. For Search mode with incremental search,
    /// also triggers a search update.
    #[allow(dead_code)]
    pub fn paste_clipboard_to_input(&mut self) {
        let text = match self.clipboard_read {
            Some(ref cb_read) => match cb_read() {
                Ok(t) => t,
                Err(e) => {
                    self.message = format!("Clipboard read failed: {}", e);
                    return;
                }
            },
            None => return,
        };
        self.paste_text_to_input(&text);
    }

    /// Paste the given text into the command/search buffer (first line only).
    /// Called by backends that have already fetched the clipboard text themselves.
    pub fn paste_text_to_input(&mut self, text: &str) {
        let first_line = text.lines().next().unwrap_or("");
        if first_line.is_empty() {
            return;
        }
        match self.mode {
            Mode::Command | Mode::Search => {
                self.command_insert_str(first_line);
                if self.mode == Mode::Search && self.settings.incremental_search {
                    self.perform_incremental_search();
                }
            }
            _ => {}
        }
    }

    /// Pre-load clipboard text into the `+` and `*` registers.
    /// Called by GTK backend after an async GDK clipboard read, before paste.
    pub fn load_clipboard_register(&mut self, text: String) {
        self.registers.insert('+', (text.clone(), false));
        self.registers.insert('*', (text, false));
    }

    /// Pre-load clipboard text into `"`, `+`, and `*` before a p/P keypress.
    ///
    /// If the clipboard content matches what is already in `"`, the existing
    /// `is_linewise` flag is **preserved** — this covers the common `yy` → `p`
    /// flow where the yank wrote linewise text to both the register and the
    /// system clipboard.  When the clipboard holds different text (from another
    /// application) `is_linewise` is set to `false` as usual.
    ///
    /// The text is normalized (`\r\n` → `\n`) before storing, and the comparison
    /// ignores a trailing newline difference, because the OS clipboard round-trip
    /// can alter line endings (e.g. Windows `Get-Clipboard` returns `\r\n` and
    /// may strip trailing newlines).
    pub fn load_clipboard_for_paste(&mut self, text: String) {
        // Normalize CRLF → LF so pasted text doesn't introduce \r into the buffer.
        let text = text.replace("\r\n", "\n");

        let existing_lw = self
            .registers
            .get(&'"')
            .map(|(reg_content, lw)| {
                if !*lw {
                    return false;
                }
                // Compare without trailing \n — clipboard may strip it
                let clip_trimmed = text.trim_end_matches('\n');
                let reg_trimmed = reg_content.trim_end_matches('\n');
                clip_trimmed == reg_trimmed
            })
            .unwrap_or(false);

        // For linewise content, ensure it ends with \n so paste inserts complete lines.
        let text = if existing_lw && !text.ends_with('\n') {
            format!("{text}\n")
        } else {
            text
        };

        self.registers.insert('"', (text.clone(), existing_lw));
        self.registers.insert('+', (text.clone(), false));
        self.registers.insert('*', (text, false));
    }

    /// Feed a key sequence string into the engine, parsing special key notation.
    ///
    /// Supports: plain characters (`dw`), special keys (`<Esc>`, `<CR>`, `<BS>`,
    /// `<Tab>`, `<Del>`, `<Up>`, `<Down>`, `<Left>`, `<Right>`), and Ctrl
    /// combinations (`<C-a>`).  This is the same notation used in Neovim's
    /// `nvim_feedkeys` / `nvim_input`.
    pub fn feed_keys(&mut self, keys: &str) {
        let mut chars = keys.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '<' {
                // Only parse as <SpecialKey> if the remaining string contains '>'.
                let rest: String = chars.clone().collect();
                let has_closing = rest.contains('>');
                let starts_special = chars
                    .peek()
                    .map(|&c| c.is_ascii_uppercase() || c == 'C')
                    .unwrap_or(false);
                if has_closing && starts_special {
                    let name: String = chars.by_ref().take_while(|&c| c != '>').collect();
                    match name.as_str() {
                        "Esc" => {
                            self.handle_key("Escape", None, false);
                        }
                        "CR" | "Enter" => {
                            self.handle_key("Return", None, false);
                        }
                        "BS" => {
                            self.handle_key("BackSpace", None, false);
                        }
                        "Tab" => {
                            self.handle_key("Tab", None, false);
                        }
                        "Del" | "Delete" => {
                            self.handle_key("Delete", None, false);
                        }
                        "Up" => {
                            self.handle_key("Up", None, false);
                        }
                        "Down" => {
                            self.handle_key("Down", None, false);
                        }
                        "Left" => {
                            self.handle_key("Left", None, false);
                        }
                        "Right" => {
                            self.handle_key("Right", None, false);
                        }
                        n if n.starts_with("C-") => {
                            let ctrl_char = n.chars().nth(2).unwrap_or(' ');
                            self.handle_key(&ctrl_char.to_string(), Some(ctrl_char), true);
                        }
                        other => {
                            self.handle_key(other, None, false);
                        }
                    }
                } else {
                    self.handle_key(&ch.to_string(), Some(ch), false);
                }
            } else {
                self.handle_key(&ch.to_string(), Some(ch), false);
            }
            // Drain macro playback queue (populated by @q etc.)
            self.drain_macro_queue();
        }
    }

    /// Drain the macro playback queue, executing each queued keystroke.
    fn drain_macro_queue(&mut self) {
        // Guard against infinite recursion from self-referencing macros
        let max_iterations = 100_000;
        let mut iterations = 0;
        while !self.macro_playback_queue.is_empty() && iterations < max_iterations {
            if let Some((key_name, unicode, ctrl, consumed)) = self.decode_macro_sequence() {
                for _ in 0..consumed {
                    self.macro_playback_queue.pop_front();
                }
                self.handle_key(&key_name, unicode, ctrl);
            } else {
                self.macro_playback_queue.pop_front();
            }
            iterations += 1;
        }
    }
}
