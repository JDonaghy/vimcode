use super::*;

impl Engine {
    // =========================================================================
    // Plugin system
    // =========================================================================

    /// Initialize the plugin manager: load all `.lua` files / `init.lua` dirs
    /// from `~/.config/vimcode/plugins/`.
    pub fn plugin_init(&mut self) {
        let config_base = paths::vimcode_config_dir();
        let plugins_dir = config_base.join("plugins");
        let extensions_dir = config_base.join("extensions");

        // Create a plugin manager even if neither directory exists, so that
        // extensions installed during this session can register commands.
        let has_plugins = plugins_dir.exists();
        let has_extensions = extensions_dir.exists();
        if !has_plugins && !has_extensions {
            return;
        }

        match plugin::PluginManager::new() {
            Ok(mut mgr) => {
                if has_plugins {
                    mgr.load_plugins_dir(&plugins_dir, &self.settings.disabled_plugins);
                }
                // Load Lua scripts from each installed extension sub-directory.
                // Only load extensions that are in extension_state.installed
                // (or disabled_plugins for the skip check).  Extensions whose
                // scripts exist on disk but are not marked installed are ignored.
                if has_extensions {
                    if let Ok(entries) = std::fs::read_dir(&extensions_dir) {
                        let mut dirs: Vec<_> = entries
                            .filter_map(|e| e.ok().map(|e| e.path()))
                            .filter(|p| p.is_dir())
                            .collect();
                        dirs.sort();
                        for ext_dir in dirs {
                            let ext_name = ext_dir
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            if self.settings.disabled_plugins.contains(&ext_name) {
                                continue;
                            }
                            // Only load scripts for extensions the user has
                            // explicitly installed.  Scripts on disk from a
                            // previous install (or leftover extraction) should
                            // not run unless the extension is installed.
                            if !self.extension_state.is_installed(&ext_name) {
                                continue;
                            }
                            mgr.load_plugins_dir(&ext_dir, &self.settings.disabled_plugins);
                        }
                    }
                }
                // Harvest panel registrations from plugin scripts
                for (name, panel) in &mgr.panels {
                    if !self.ext_panel_sections_expanded.contains_key(name) {
                        self.ext_panel_sections_expanded
                            .insert(name.clone(), vec![true; panel.sections.len()]);
                    }
                    self.ext_panels.insert(name.clone(), panel.clone());
                }
                for (panel_name, bindings) in &mgr.help_bindings {
                    self.ext_panel_help_bindings
                        .insert(panel_name.clone(), bindings.clone());
                }
                self.plugin_manager = Some(mgr);
                // Load per-extension settings for installed extensions
                let installed_names: Vec<String> = self
                    .extension_state
                    .installed
                    .iter()
                    .map(|e| e.name.clone())
                    .collect();
                for name in &installed_names {
                    self.load_ext_settings(name);
                }
                // Populate comment style and highlight query overrides from installed extensions
                self.populate_comment_overrides();
                self.populate_highlight_overrides();
                // Fire VimEnter event after plugin initialization is complete
                self.plugin_event("VimEnter", "");
            }
            Err(e) => {
                self.message = format!("Plugin init error: {e}");
            }
        }
    }

    /// Build a `PluginCallContext` from the current active buffer state.
    /// When `skip_buf_lines` is true the expensive O(N) line-by-line
    /// collection is omitted (used for cursor_move on clean buffers where
    /// no plugin actually needs the line contents).
    pub(crate) fn make_plugin_ctx(&self, skip_buf_lines: bool) -> plugin::PluginCallContext {
        let cwd = self.cwd.to_string_lossy().into_owned();
        let buf_id = self.active_buffer_id();
        let buf_state = self.buffer_manager.get(buf_id);
        let buf_path_os = buf_state.as_ref().and_then(|s| s.file_path.clone());
        let buf_path = buf_path_os
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let buf_dirty = buf_state.as_ref().map(|s| s.dirty).unwrap_or(false);
        let buf_lines = if skip_buf_lines {
            Vec::new()
        } else {
            buf_state
                .map(|s| {
                    (0..s.buffer.len_lines())
                        .map(|i| s.buffer.content.line(i).to_string())
                        .collect()
                })
                .unwrap_or_default()
        };
        let cursor = self.cursor();

        // Mode name
        let mode_name = match self.mode {
            Mode::Normal => "Normal",
            Mode::Insert => "Insert",
            Mode::Replace => {
                if self.virtual_replace {
                    "VReplace"
                } else {
                    "Replace"
                }
            }
            Mode::Command => "Command",
            Mode::Search => "Search",
            Mode::Visual => "Visual",
            Mode::VisualLine => "VisualLine",
            Mode::VisualBlock => "VisualBlock",
        }
        .to_string();

        // Registers snapshot
        let registers_snapshot = self.registers.clone();

        // Marks snapshot for the active buffer (1-indexed)
        let marks_snapshot = self
            .marks
            .get(&buf_id)
            .map(|m| {
                m.iter()
                    .map(|(&ch, c)| (ch, (c.line + 1, c.col + 1)))
                    .collect()
            })
            .unwrap_or_default();

        // Filetype from active buffer's language ID
        let filetype = self
            .buffer_manager
            .get(buf_id)
            .and_then(|s| s.lsp_language_id.clone())
            .unwrap_or_default();

        // Settings snapshot
        let settings_snapshot = self.settings_snapshot();

        plugin::PluginCallContext {
            cwd,
            buf_path,
            buf_lines,
            buf_dirty,
            cursor_line: cursor.line + 1,
            cursor_col: cursor.col + 1,
            cwd_path: Some(self.cwd.clone()),
            buf_path_os,
            mode_name,
            registers_snapshot,
            marks_snapshot,
            filetype,
            settings_snapshot,
            panel_input_snapshot: self.ext_panel_input_text.clone(),
            ..Default::default()
        }
    }

    /// Build a snapshot of all settings as string key-value pairs.
    pub(crate) fn settings_snapshot(&self) -> HashMap<String, String> {
        let keys = [
            "colorscheme",
            "font_family",
            "font_size",
            "line_numbers",
            "cursorline",
            "tabstop",
            "shift_width",
            "expand_tab",
            "auto_indent",
            "wrap",
            "scrolloff",
            "colorcolumn",
            "textwidth",
            "hlsearch",
            "ignorecase",
            "smartcase",
            "incremental_search",
            "editor_mode",
            "explorer_visible_on_startup",
            "autoread",
            "splitbelow",
            "splitright",
            "lsp_enabled",
            "format_on_save",
            "terminal_scrollback_lines",
            "plugins_enabled",
            "ai_provider",
            "ai_model",
            "ai_base_url",
            "ai_completions",
            "swapfile",
            "updatetime",
            "breadcrumbs",
        ];
        let mut map = HashMap::new();
        for key in &keys {
            let val = self.settings.get_value_str(key);
            if !val.is_empty() {
                map.insert(key.to_string(), val);
            }
        }
        // Include extension settings with "extname.key" namespace
        for (ext_name, values) in &self.ext_settings {
            for (key, val) in values {
                map.insert(format!("{ext_name}.{key}"), val.clone());
            }
        }
        map
    }

    /// Apply the output side of a `PluginCallContext` back to the engine.
    pub(crate) fn apply_plugin_ctx(&mut self, ctx: plugin::PluginCallContext) {
        if let Some(msg) = ctx.message {
            self.message = msg;
        }
        if !ctx.set_lines.is_empty() {
            self.start_undo_group();
            let buf_id = self.active_buffer_id();
            for (line_idx, text) in ctx.set_lines {
                if let Some(state) = self.buffer_manager.get_mut(buf_id) {
                    let line_count = state.buffer.len_lines();
                    if line_idx < line_count {
                        let start = state.buffer.line_to_char(line_idx);
                        let end = if line_idx + 1 < line_count {
                            state.buffer.line_to_char(line_idx + 1)
                        } else {
                            state.buffer.len_chars()
                        };
                        let new_text = if text.ends_with('\n') {
                            text
                        } else {
                            format!("{text}\n")
                        };
                        // Record for undo before mutating
                        let old_text: String = state.buffer.content.slice(start..end).to_string();
                        state.record_delete(start, &old_text);
                        state.buffer.delete_range(start, end);
                        state.record_insert(start, &new_text);
                        state.buffer.insert(start, &new_text);
                        state.dirty = true;
                    }
                }
            }
            self.finish_undo_group();
        }
        // Apply virtual-text line annotations
        if ctx.clear_annotations {
            self.line_annotations.clear();
            self.editor_hover_content.clear();
            self.blame_annotations_active = false;
        }
        for (line_1indexed, text) in ctx.annotate_lines {
            if line_1indexed > 0 {
                self.line_annotations.insert(line_1indexed - 1, text);
            }
        }
        // Apply cursor position
        if let Some((line_1, col_1)) = ctx.set_cursor {
            let line = line_1.saturating_sub(1);
            let col = col_1.saturating_sub(1);
            let max_line = self.buffer().len_lines().saturating_sub(1);
            let clamped_line = line.min(max_line);
            self.view_mut().cursor.line = clamped_line;
            let max_col = self.get_max_cursor_col(clamped_line);
            self.view_mut().cursor.col = col.min(max_col);
        }
        // Apply settings changes — "extname.key" routes to extension settings
        for (key, value) in ctx.set_settings {
            if let Some((ext_name, ext_key)) = key.split_once('.') {
                if self.ext_settings.contains_key(ext_name) {
                    self.set_ext_setting(ext_name, ext_key, &value);
                    continue;
                }
            }
            let _ = self.settings.set_value_str(&key, &value);
        }
        // Apply register writes
        for (ch, content, linewise) in ctx.set_registers {
            if ch == '+' || ch == '*' {
                if let Some(ref cb) = self.clipboard_write {
                    let _ = cb(&content);
                }
            }
            self.registers.insert(ch, (content, linewise));
        }
        // Apply line insertions (process in reverse to keep indices stable)
        if !ctx.insert_lines.is_empty() {
            let buf_id = self.active_buffer_id();
            let mut insertions = ctx.insert_lines;
            insertions.sort_by_key(|b| std::cmp::Reverse(b.0));
            for (line_1, text) in insertions {
                let line_idx = line_1.saturating_sub(1);
                if let Some(state) = self.buffer_manager.get_mut(buf_id) {
                    let line_count = state.buffer.len_lines();
                    let insert_at = if line_idx >= line_count {
                        state.buffer.len_chars()
                    } else {
                        state.buffer.line_to_char(line_idx)
                    };
                    let new_text = if text.ends_with('\n') {
                        text
                    } else {
                        format!("{text}\n")
                    };
                    state.buffer.insert(insert_at, &new_text);
                    state.dirty = true;
                }
            }
        }
        // Apply line deletions (process in reverse to keep indices stable)
        if !ctx.delete_lines.is_empty() {
            let buf_id = self.active_buffer_id();
            let mut deletions = ctx.delete_lines;
            deletions.sort_unstable();
            deletions.dedup();
            deletions.reverse();
            for line_1 in deletions {
                let line_idx = line_1.saturating_sub(1);
                if let Some(state) = self.buffer_manager.get_mut(buf_id) {
                    let line_count = state.buffer.len_lines();
                    if line_idx < line_count {
                        let start = state.buffer.line_to_char(line_idx);
                        let end = if line_idx + 1 < line_count {
                            state.buffer.line_to_char(line_idx + 1)
                        } else {
                            state.buffer.len_chars()
                        };
                        if start < end {
                            state.buffer.delete_range(start, end);
                            state.dirty = true;
                        }
                    }
                }
            }
        }
        for cmd in ctx.run_commands {
            let _ = self.execute_command(&cmd);
        }
        // Apply range-based line replacements (Neovim-compatible set_lines)
        if !ctx.set_lines_range.is_empty() {
            self.start_undo_group();
            let buf_id = self.active_buffer_id();
            // Process in reverse order so earlier indices stay valid
            let mut ranges = ctx.set_lines_range;
            ranges.reverse();
            for (start, end, new_lines) in ranges {
                if let Some(state) = self.buffer_manager.get_mut(buf_id) {
                    let line_count = state.buffer.len_lines();
                    let s = start.min(line_count);
                    let e = end.min(line_count);
                    if s <= e {
                        // Delete old lines [s, e)
                        if s < e && s < line_count {
                            let char_start = state.buffer.line_to_char(s);
                            let char_end = if e < line_count {
                                state.buffer.line_to_char(e)
                            } else {
                                state.buffer.len_chars()
                            };
                            if char_start < char_end {
                                let old: String =
                                    state.buffer.content.slice(char_start..char_end).to_string();
                                state.record_delete(char_start, &old);
                                state.buffer.delete_range(char_start, char_end);
                            }
                        }
                        // Insert new lines at position s
                        if !new_lines.is_empty() {
                            let insert_at = if s < state.buffer.len_lines() {
                                state.buffer.line_to_char(s)
                            } else {
                                state.buffer.len_chars()
                            };
                            let mut text = String::new();
                            for line in &new_lines {
                                text.push_str(line);
                                text.push('\n');
                            }
                            state.record_insert(insert_at, &text);
                            state.buffer.insert(insert_at, &text);
                        }
                        state.dirty = true;
                    }
                }
            }
            self.finish_undo_group();
        }
        // Apply feedkeys sequences
        for keys in ctx.feedkeys_sequences {
            self.feed_keys(&keys);
        }
        // Spawn background threads for async shell requests.
        for req in ctx.async_shell_requests {
            let (tx, rx) = std::sync::mpsc::channel();
            // Last-writer-wins: replace any pending task for the same callback event.
            self.async_shell_tasks.insert(req.callback_event, rx);
            std::thread::spawn(move || {
                use std::process::{Command, Stdio};
                let mut cmd = Command::new("sh");
                cmd.arg("-c").arg(&req.command);
                if let Some(ref cwd) = req.cwd {
                    cmd.current_dir(cwd);
                }
                if req.stdin.is_some() {
                    cmd.stdin(Stdio::piped());
                }
                cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
                let result = if let Some(ref input) = req.stdin {
                    match cmd.spawn() {
                        Ok(mut child) => {
                            if let Some(ref mut stdin_pipe) = child.stdin.take() {
                                use std::io::Write;
                                let _ = stdin_pipe.write_all(input.as_bytes());
                            }
                            child.wait_with_output()
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    cmd.output()
                };
                match result {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                        let _ = tx.send((out.status.success(), stdout));
                    }
                    Err(_) => {
                        let _ = tx.send((false, String::new()));
                    }
                }
            });
        }
        // Open scratch buffers requested by plugins
        for req in ctx.scratch_buffers {
            let buf_id = self.buffer_manager.create();
            if let Some(state) = self.buffer_manager.get_mut(buf_id) {
                state.buffer.content = ropey::Rope::from_str(&req.content);
                state.dirty = false;
                // Set a display name for the tab (e.g. "[GitFileHistory]")
                state.file_path = None;
                // Store the name in a way the tab bar can use it:
                // we set the scratch_name field if available, otherwise use file_path
                state.scratch_name = Some(req.name.clone());
                if req.read_only {
                    state.read_only = true;
                }
                if let Some(ref ft) = req.filetype {
                    // Map filetype to a fake path extension for syntax detection
                    let ext = match ft.as_str() {
                        "rust" => "rs",
                        "python" => "py",
                        "javascript" => "js",
                        "typescript" => "ts",
                        "diff" => "diff",
                        other => other,
                    };
                    let fake_path = format!("scratch.{ext}");
                    if let Some(syn) = Syntax::new_from_path_with_overrides(
                        Some(&fake_path),
                        Some(&self.highlight_overrides),
                    ) {
                        state.syntax = Some(syn);
                        state.update_syntax();
                    }
                    state.lsp_language_id = Some(ft.clone());
                }
            }
            match req.split.as_deref() {
                Some("vertical") => {
                    self.split_window(SplitDirection::Vertical, None);
                    let win = self.active_window_mut();
                    win.buffer_id = buf_id;
                    win.view.cursor = crate::core::cursor::Cursor::default();
                    win.view.scroll_top = 0;
                }
                Some("horizontal") => {
                    self.split_window(SplitDirection::Horizontal, None);
                    let win = self.active_window_mut();
                    win.buffer_id = buf_id;
                    win.view.cursor = crate::core::cursor::Cursor::default();
                    win.view.scroll_top = 0;
                }
                _ => {
                    // Replace current window's buffer
                    let win = self.active_window_mut();
                    win.buffer_id = buf_id;
                    win.view.cursor = crate::core::cursor::Cursor::default();
                    win.view.scroll_top = 0;
                }
            }
        }
        // Apply comment style overrides (highest priority — from plugin runtime)
        for (lang_id, line, block_open, block_close) in ctx.comment_style_overrides {
            self.comment_overrides.insert(
                lang_id,
                comment::CommentStyleOwned {
                    line,
                    block_open,
                    block_close,
                },
            );
        }
        // Apply extension panel registrations
        for reg in ctx.panel_registrations {
            let name = reg.name.clone();
            // Initialize expanded state for new panels
            if !self.ext_panel_sections_expanded.contains_key(&name) {
                self.ext_panel_sections_expanded
                    .insert(name.clone(), vec![true; reg.sections.len()]);
            }
            self.ext_panels.insert(name, reg);
        }
        // Apply extension panel item updates
        for (panel, section, items) in ctx.panel_set_items {
            self.ext_panel_items.insert((panel, section), items);
        }
        // Register panel hover content from plugin callbacks
        for (panel_name, item_id, markdown) in ctx.panel_hover_entries {
            self.panel_hover_registry
                .insert((panel_name, item_id), markdown);
        }
        // Register panel help bindings from plugin callbacks
        for (panel, bindings) in ctx.panel_help_entries {
            self.ext_panel_help_bindings.insert(panel, bindings);
        }
        // Apply panel input field text values from plugin callbacks
        for (panel_name, text) in ctx.panel_input_values {
            self.ext_panel_input_text.insert(panel_name, text);
        }
        // Register editor hover content from plugin callbacks
        for (line, markdown) in ctx.editor_hover_entries {
            self.editor_hover_content.insert(line, markdown);
        }
        // Handle panel reveal request: switch to panel, fire panel_focus, highlight item
        if let Some((panel_name, section_name, item_id)) = ctx.panel_reveal_request {
            self.ext_panel_active = Some(panel_name.clone());
            self.ext_panel_has_focus = true;
            // Clear tree expanded state so all nodes start collapsed — this ensures
            // the flat index calculation matches the freshly populated items.
            self.ext_panel_tree_expanded
                .retain(|(p, _), _| p != &panel_name);
            // Fire panel_focus event so the plugin populates items
            self.plugin_event("panel_focus", &panel_name);
            // Now find and reveal the item
            self.ext_panel_reveal_item(&panel_name, &section_name, &item_id);
            // Signal backends to switch the sidebar to this panel
            self.ext_panel_focus_pending = Some(panel_name);
        }
        // Handle commit file diff request: open side-by-side diff
        if let Some((hash, path)) = ctx.commit_file_diff {
            self.open_commit_file_diff(&hash, &path);
        }
        for url in ctx.open_urls {
            self.open_url(&url);
        }
    }

    /// Poll for completed async shell tasks spawned by plugins.
    /// Returns `true` if any results were delivered (caller should redraw).
    pub fn poll_async_shells(&mut self) -> bool {
        let mut completed = Vec::new();
        for (event, rx) in &self.async_shell_tasks {
            if let Ok(result) = rx.try_recv() {
                completed.push((event.clone(), result));
            }
        }
        if completed.is_empty() {
            return false;
        }
        for (event, (_success, output)) in &completed {
            self.async_shell_tasks.remove(event.as_str());
            self.plugin_event(event, output);
        }
        true
    }

    /// Set the editor mode with autocmd event firing.
    /// Fires `ModeChanged` (arg: "OldMode:NewMode"), plus `InsertEnter`/`InsertLeave`
    /// when transitioning to/from Insert mode.
    pub fn set_mode(&mut self, new_mode: Mode) {
        let old_mode = self.mode;
        if old_mode == new_mode {
            return;
        }
        self.mode = new_mode;

        // Build mode name strings for events
        let old_name = Self::mode_event_name(old_mode);
        let new_name = Self::mode_event_name(new_mode);

        // Fire InsertLeave when leaving Insert or Replace mode
        if old_mode == Mode::Insert || old_mode == Mode::Replace {
            self.plugin_event("InsertLeave", old_name);
        }

        // Fire InsertEnter when entering Insert or Replace mode
        if new_mode == Mode::Insert || new_mode == Mode::Replace {
            self.plugin_event("InsertEnter", new_name);
        }

        // Fire ModeChanged with "OldMode:NewMode" argument
        let arg = format!("{old_name}:{new_name}");
        self.plugin_event("ModeChanged", &arg);
    }

    /// Get a short event name for a mode (used in ModeChanged events).
    pub(crate) fn mode_event_name(mode: Mode) -> &'static str {
        match mode {
            Mode::Normal => "Normal",
            Mode::Insert => "Insert",
            Mode::Replace => "Replace",
            Mode::Command => "Command",
            Mode::Search => "Search",
            Mode::Visual => "Visual",
            Mode::VisualLine => "VisualLine",
            Mode::VisualBlock => "VisualBlock",
        }
    }

    /// Fire an event hook (e.g. "save", "open") for all registered listeners.
    pub fn plugin_event(&mut self, event: &str, arg: &str) {
        if !self.settings.plugins_enabled {
            return;
        }
        let pm = match self.plugin_manager.take() {
            Some(p) => p,
            None => return,
        };
        // Skip the potentially O(N_lines) context construction if no hooks are
        // registered for this event.  For cursor_move this avoids building
        // Vec<String> of all buffer lines on every keystroke when no extension
        // has registered a cursor_move listener.
        if !pm.has_event_hooks(event) {
            self.plugin_manager = Some(pm);
            return;
        }
        // For cursor_move on clean buffers, skip the O(N) buf_lines build.
        // blame_line() already skips --contents stdin when buf_dirty is false,
        // so the lines would never be read.
        let skip = event == "cursor_move"
            && !self
                .buffer_manager
                .get(self.active_buffer_id())
                .map(|s| s.dirty)
                .unwrap_or(false);
        let ctx = self.make_plugin_ctx(skip);
        let ctx = pm.call_event(event, arg, ctx);
        self.plugin_manager = Some(pm);
        self.apply_plugin_ctx(ctx);
    }

    /// Fire the `cursor_move` plugin hook for the current cursor position.
    /// Mark cursor_move as pending (deferred to backend idle loop for debouncing).
    /// Call this after any cursor movement that doesn't go through `handle_key()`
    /// (e.g. mouse click, session restore).
    pub fn fire_cursor_move_hook(&mut self) {
        self.cursor_move_pending = Some(std::time::Instant::now());
    }

    /// Immediately fire the cursor_move plugin hook.
    /// Use for one-shot events (file open) where the debounce delay is unwanted.
    pub fn fire_cursor_move_hook_now(&mut self) {
        self.cursor_move_pending = None;
        let cursor = self.cursor();
        let arg = format!("{},{}", cursor.line + 1, cursor.col + 1);
        self.plugin_event("cursor_move", &arg);
    }

    /// Flush pending cursor_move hook if the debounce delay (150ms) has elapsed.
    /// Called by backends from their idle/poll loop.
    /// Returns true if the hook was fired (needs redraw).
    pub fn flush_cursor_move_hook(&mut self) -> bool {
        let Some(when) = self.cursor_move_pending else {
            return false;
        };
        if when.elapsed() < std::time::Duration::from_millis(150) {
            return false;
        }
        self.cursor_move_pending = None;
        let cursor = self.cursor();
        let arg = format!("{},{}", cursor.line + 1, cursor.col + 1);
        self.plugin_event("cursor_move", &arg);
        // Proactively request code actions for the new cursor position (lightbulb).
        self.lsp_request_code_actions_for_line();
        true
    }
}
