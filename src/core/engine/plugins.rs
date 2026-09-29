use super::*;

/// Upper bound on nested plugin events queued during one dispatch. A plugin
/// whose edit fires an event whose handler edits again could otherwise cascade
/// without end; past this point the queue stops growing and the extra events
/// are dropped rather than wedging the editor.
const MAX_DEFERRED_PLUGIN_EVENTS: usize = 64;

/// Upper bound on drain rounds, for the same reason: each round may enqueue
/// more events.
const MAX_DEFERRED_DRAIN_ROUNDS: usize = 8;

impl Engine {
    // =========================================================================
    // Plugin system
    // =========================================================================

    /// Install a plugin manager. Wraps it in the `Rc` the dispatch path needs:
    /// dispatch clones the `Rc` out rather than `take()`ing the manager, so the
    /// field stays populated for the duration of a Lua call (#1214) — that is
    /// what lets nested code still see "there is a plugin manager" and what
    /// makes loaning `&mut Engine` to Lua sound (no outstanding borrow of
    /// `self.plugin_manager` is held across the call).
    pub fn set_plugin_manager(&mut self, mgr: plugin::PluginManager) {
        // #146: seed `plugin_views` so a `vimcode.ui.register_view` panel is
        // recognisable as view-backed before its `render` callback has ever run
        // (the sidebar has to pick a body *shape* on the first frame it paints).
        for name in mgr.view_names() {
            self.plugin_views.entry(name).or_default();
        }
        self.plugin_manager = Some(std::rc::Rc::new(mgr));
    }

    // ── Plugin-declared UI views (#146) ────────────────────────────────────

    /// Whether `name` names a `vimcode.ui.register_view` view.
    pub fn is_plugin_view(&self, name: &str) -> bool {
        self.plugin_views.contains_key(name)
    }

    /// Re-run `name`'s `render` callback and store the widget tree it returns.
    ///
    /// Returns `true` when the stored tree changed (i.e. a repaint is worth it).
    /// A Lua error is surfaced on the status line and leaves the previous tree in
    /// place, so a broken `render` degrades to a stale panel rather than an empty
    /// one.
    pub fn refresh_plugin_view(&mut self, name: &str) -> bool {
        if !self.plugin_views.contains_key(name) {
            return false;
        }
        // `apply_plugin_ctx` below honours `vimcode.ui.refresh`, so a `render`
        // callback that refreshes a view can re-enter here. Bound it rather than
        // forbid it: one level of "render, then ask for one more pass" is a
        // legitimate pattern (populate-then-redraw), an unbounded chain is a
        // plugin bug that must not become a stack overflow.
        const MAX_RENDER_DEPTH: u32 = 4;
        if self.plugin_view_render_depth >= MAX_RENDER_DEPTH {
            return false;
        }
        self.plugin_view_render_depth += 1;
        let out = self.refresh_plugin_view_inner(name);
        self.plugin_view_render_depth -= 1;
        out
    }

    fn refresh_plugin_view_inner(&mut self, name: &str) -> bool {
        let ctx = self.make_plugin_ctx(true);
        let name_owned = name.to_string();
        let Some((ctx, result)) = self.with_plugin_dispatch(|pm| pm.render_view(&name_owned, ctx))
        else {
            return false;
        };
        self.apply_plugin_ctx(ctx);
        match result {
            Ok(view) => {
                let changed = self.plugin_views.get(name) != Some(&view);
                if changed {
                    // A shorter tree must not leave the selection past its end.
                    if self.ext_panel_active.as_deref() == Some(name)
                        && self.ext_panel_selected >= view.fields.len()
                    {
                        self.ext_panel_selected = view.first_focusable().unwrap_or(0);
                    }
                    self.plugin_views.insert(name.to_string(), view);
                }
                changed
            }
            Err(msg) => {
                self.message = format!("plugin view {name}: {msg}");
                false
            }
        }
    }

    /// Called when an `ext:` sidebar panel becomes the active one.
    ///
    /// Fires the `panel_focus` hook (how a `vimcode.panel.register` panel learns
    /// to populate its sections) and, for a `vimcode.ui.register_view` panel,
    /// re-runs its `render` callback so the form painted this frame is current
    /// (#146). Both activation paths — the activity-bar click router in
    /// `render.rs` and `Engine::activate_activity_bar_item` — go through here so
    /// the two cannot drift.
    pub(crate) fn on_ext_panel_focused(&mut self, name: &str) {
        self.plugin_event("panel_focus", name);
        if self.is_plugin_view(name) {
            self.refresh_plugin_view(name);
            // Park the selection on something `Enter` can act on.
            let first = self
                .plugin_views
                .get(name)
                .and_then(|v| v.first_focusable())
                .unwrap_or(0);
            self.ext_panel_selected = first;
        }
    }

    /// Keyboard handling for a view-backed sidebar panel.
    ///
    /// Returns `true` when the key was consumed. Keys this returns `false` for
    /// fall through to `Engine::handle_ext_panel_key`'s generic panel bindings
    /// (`q`/`Escape` to unfocus, `h`/`Left` back to the activity bar, `?` help),
    /// so a plugin view keeps the same panel chrome bindings every other sidebar
    /// panel has.
    ///
    /// Navigation skips rows that cannot emit events (`label`, `read_only`,
    /// `disabled`), so `j`/`k` never parks the selection somewhere `Enter` does
    /// nothing.
    pub(crate) fn handle_plugin_view_key(&mut self, name: &str, key: &str) -> bool {
        use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind, ViewFieldKind};

        let Some(view) = self.plugin_views.get(name) else {
            return false;
        };
        let focusable: Vec<usize> = view
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| f.is_interactive() && !f.disabled)
            .map(|(i, _)| i)
            .collect();

        match key {
            "j" | "Down" | "Tab" => {
                let next = focusable
                    .iter()
                    .copied()
                    .find(|i| *i > self.ext_panel_selected)
                    .or_else(|| focusable.first().copied());
                if let Some(i) = next {
                    self.ext_panel_selected = i;
                }
                self.ext_panel_ensure_visible(0);
                true
            }
            "k" | "Up" => {
                let prev = focusable
                    .iter()
                    .rev()
                    .copied()
                    .find(|i| *i < self.ext_panel_selected)
                    .or_else(|| focusable.last().copied());
                if let Some(i) = prev {
                    self.ext_panel_selected = i;
                }
                self.ext_panel_ensure_visible(0);
                true
            }
            "g" => {
                if let Some(i) = focusable.first().copied() {
                    self.ext_panel_selected = i;
                }
                self.ext_panel_scroll_top = 0;
                true
            }
            "G" => {
                if let Some(i) = focusable.last().copied() {
                    self.ext_panel_selected = i;
                }
                self.ext_panel_ensure_visible(0);
                true
            }
            "Return" | "Enter" | "Space" | " " => {
                let Some(field) = view.fields.get(self.ext_panel_selected) else {
                    return true;
                };
                if field.disabled {
                    return true;
                }
                // The *plugin* owns the declared value: vimcode reports the value
                // the activation implies and the next `render` reflects whatever
                // the handler decided to store. Nothing is mutated here.
                let (widget_id, kind) = match &field.kind {
                    ViewFieldKind::Button => (field.id.clone(), ViewEventKind::ButtonClicked),
                    ViewFieldKind::Toggle { value } => (
                        field.id.clone(),
                        ViewEventKind::ToggleChanged { value: !*value },
                    ),
                    ViewFieldKind::Toggles { toggles } => match toggles.first() {
                        Some(t) => (
                            t.id.clone(),
                            ViewEventKind::ToggleChanged { value: !t.value },
                        ),
                        None => return true,
                    },
                    ViewFieldKind::Buttons { buttons } => {
                        match buttons.iter().find(|b| !b.disabled) {
                            Some(b) => (b.id.clone(), ViewEventKind::ButtonClicked),
                            None => return true,
                        }
                    }
                    ViewFieldKind::Dropdown { options, selected } => {
                        if options.is_empty() {
                            return true;
                        }
                        let next = (*selected + 1) % options.len();
                        (
                            field.id.clone(),
                            ViewEventKind::DropdownChanged { selected: next },
                        )
                    }
                    ViewFieldKind::Segmented { options, selected } => {
                        if options.is_empty() {
                            return true;
                        }
                        let next = (*selected + 1) % options.len();
                        (
                            field.id.clone(),
                            ViewEventKind::SegmentedChanged { selected: next },
                        )
                    }
                    ViewFieldKind::Text { value, .. }
                    | ViewFieldKind::Password { value, .. }
                    | ViewFieldKind::TextArea { value, .. } => (
                        field.id.clone(),
                        // Text *entry* into a plugin view is #1403 Phase 2; Enter
                        // on a text row commits the value the plugin declared, so
                        // "type in a real buffer, press Enter here" already works
                        // as a submit affordance.
                        ViewEventKind::TextCommitted {
                            value: value.clone(),
                        },
                    ),
                    ViewFieldKind::Label | ViewFieldKind::ReadOnly { .. } => return true,
                };
                self.dispatch_plugin_view_event(PluginViewEvent {
                    view: name.to_string(),
                    widget_id,
                    kind,
                });
                true
            }
            _ => false,
        }
    }

    /// Route one resolved widget event to the owning view's `on_event` callback.
    ///
    /// The view is re-rendered afterwards unconditionally: the whole point of the
    /// callback is that it mutates plugin-side state the next `render` reflects,
    /// and a handler has no way to know whether its own mutation was visible.
    pub fn dispatch_plugin_view_event(&mut self, event: crate::core::plugin_ui::PluginViewEvent) {
        if !self.plugin_views.contains_key(&event.view) {
            return;
        }
        let ctx = self.make_plugin_ctx(true);
        if let Some(ctx) = self.with_plugin_dispatch(|pm| pm.call_view_event(&event, ctx)) {
            self.apply_plugin_ctx(ctx);
        }
        self.refresh_plugin_view(&event.view);
    }

    /// Whether a Lua dispatch could actually run right now — plugins enabled, a
    /// manager installed, and no dispatch already in flight. Callers use this to
    /// skip building a [`plugin::PluginCallContext`] (which snapshots registers,
    /// marks and settings) on hot paths such as the per-keystroke plugin-keymap
    /// fallback.
    pub(crate) fn can_dispatch_to_plugins(&self) -> bool {
        self.settings.plugins_enabled
            && self.plugin_dispatch_depth == 0
            && self.plugin_manager.is_some()
    }

    /// Run one Lua dispatch with a live `&mut Engine` loan installed.
    ///
    /// `f` receives the plugin manager and returns whatever the dispatch
    /// produced (typically the updated [`plugin::PluginCallContext`]).
    ///
    /// Returns `None` — meaning "nothing was dispatched" — when plugins are
    /// disabled, when there is no manager, or when a dispatch is already in
    /// flight. That last case is the **reentrancy guard**: Lua must never be
    /// re-entered while a `&mut Engine` is loaned out, so a plugin-triggered
    /// edit that fires an event gets the event *deferred*
    /// (see [`Engine::plugin_event`]) rather than recursing.
    pub(crate) fn with_plugin_dispatch<R>(
        &mut self,
        f: impl FnOnce(&plugin::PluginManager) -> R,
    ) -> Option<R> {
        if !self.settings.plugins_enabled || self.plugin_dispatch_depth > 0 {
            return None;
        }
        let pm = std::rc::Rc::clone(self.plugin_manager.as_ref()?);
        self.plugin_dispatch_depth += 1;
        let out = {
            // The loan holds a `&mut` borrow of `*self` for this whole block,
            // so nothing here can touch `self` directly while Lua may be
            // mutating it through the loan.
            let _loan = plugin::EngineLoan::new(self);
            f(&pm)
        };
        self.plugin_dispatch_depth -= 1;
        // Close any undo group the immediate API opened during the call, so a
        // plugin's immediate edits collapse into one undo step per buffer —
        // matching the single batch the queued replay path produces.
        self.finish_plugin_undo_groups();
        Some(out)
    }

    // =========================================================================
    // Immediate ("live engine") plugin API — `vimcode.buffer.*` /
    // `vimcode.window.*`. See `core::plugin`'s module docs for the conventions;
    // the Lua wrappers in that file are thin and all logic lives here.
    // =========================================================================

    /// Resolve a Lua buffer handle (`0` = current) to a live [`BufferId`].
    pub(crate) fn plugin_api_resolve_buf(&self, handle: i64) -> Option<BufferId> {
        let id = if handle == 0 {
            self.active_buffer_id()
        } else if handle > 0 {
            BufferId(handle as usize)
        } else {
            return None;
        };
        self.buffer_manager.get(id).map(|_| id)
    }

    /// Resolve a Lua window handle (`0` = current) to a live [`WindowId`].
    pub(crate) fn plugin_api_resolve_win(&self, handle: i64) -> Option<WindowId> {
        let id = if handle == 0 {
            self.active_window_id()
        } else if handle > 0 {
            WindowId(handle as usize)
        } else {
            return None;
        };
        self.windows.get(&id).map(|_| id)
    }

    /// Logical line count: ropey reports a phantom trailing empty line for a
    /// buffer that ends in a newline, which this excludes so that
    /// `get_lines(0, line_count)` / `set_lines(0, line_count, …)` round-trip.
    pub(crate) fn plugin_api_line_count(&self, buf: BufferId) -> usize {
        let Some(state) = self.buffer_manager.get(buf) else {
            return 0;
        };
        Self::plugin_api_logical_lines(&state.buffer.content)
    }

    fn plugin_api_logical_lines(rope: &ropey::Rope) -> usize {
        let n = rope.len_lines();
        if n > 1 && rope.line(n - 1).len_chars() == 0 {
            n - 1
        } else {
            n
        }
    }

    /// Clamp a Lua line index (0-based, negative counts from the end) into
    /// `0..=len`.
    fn plugin_api_clamp_index(idx: i64, len: usize) -> usize {
        if idx < 0 {
            (len as i64 + idx).max(0) as usize
        } else {
            (idx as usize).min(len)
        }
    }

    /// Read lines `[start, end)` of `buf`, **without** trailing newlines.
    pub(crate) fn plugin_api_get_lines(&self, buf: BufferId, start: i64, end: i64) -> Vec<String> {
        let Some(state) = self.buffer_manager.get(buf) else {
            return Vec::new();
        };
        let rope = &state.buffer.content;
        let len = Self::plugin_api_logical_lines(rope);
        let s = Self::plugin_api_clamp_index(start, len);
        let e = Self::plugin_api_clamp_index(end, len);
        if e <= s {
            return Vec::new();
        }
        (s..e)
            .map(|i| {
                let line = rope.line(i).to_string();
                let line = line.strip_suffix('\n').unwrap_or(&line);
                line.strip_suffix('\r').unwrap_or(line).to_string()
            })
            .collect()
    }

    /// Replace lines `[start, end)` of `buf` with `lines`, immediately.
    ///
    /// Every inserted line is newline-terminated, matching the queued replay
    /// path in [`Engine::apply_plugin_ctx`].
    pub(crate) fn plugin_api_set_lines(
        &mut self,
        buf: BufferId,
        start: i64,
        end: i64,
        lines: Vec<String>,
    ) {
        let Some(state) = self.buffer_manager.get(buf) else {
            return;
        };
        let len = Self::plugin_api_logical_lines(&state.buffer.content);
        let s = Self::plugin_api_clamp_index(start, len);
        let e = Self::plugin_api_clamp_index(end, len).max(s);

        // One undo group per buffer per dispatch (see
        // `finish_plugin_undo_groups`) so a plugin that writes ten times
        // leaves one undo step, not ten.
        self.begin_plugin_undo_group(buf);

        let Some(state) = self.buffer_manager.get_mut(buf) else {
            return;
        };
        let raw_lines = state.buffer.len_lines();
        let char_start = if s < raw_lines {
            state.buffer.line_to_char(s)
        } else {
            state.buffer.len_chars()
        };
        let char_end = if e < len && e < raw_lines {
            state.buffer.line_to_char(e)
        } else {
            state.buffer.len_chars()
        };
        if char_end > char_start {
            let old: String = state.buffer.content.slice(char_start..char_end).to_string();
            state.record_delete(char_start, &old);
            state.buffer.delete_range(char_start, char_end);
        }
        if !lines.is_empty() {
            let mut text = String::new();
            for line in &lines {
                text.push_str(line);
                text.push('\n');
            }
            let insert_at = char_start.min(state.buffer.len_chars());
            state.record_insert(insert_at, &text);
            state.buffer.insert(insert_at, &text);
        }
        state.dirty = true;
        state.mark_syntax_stale();
        self.clamp_cursors_to_buffer(buf);
    }

    /// Create a new empty buffer and return its id.
    ///
    /// `scratch` marks it as not file-backed (shown in the tab bar under
    /// `name`, never written to disk by `:w`-less flows); `name` sets that
    /// display name.
    pub(crate) fn plugin_api_create_buffer(
        &mut self,
        scratch: bool,
        name: Option<String>,
    ) -> BufferId {
        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.dirty = false;
            state.file_path = None;
            if scratch || name.is_some() {
                state.scratch_name = Some(name.unwrap_or_else(|| format!("Scratch {}", buf_id.0)));
            }
        }
        buf_id
    }

    /// Buffer currently shown in `win` (`0` = current window).
    pub(crate) fn plugin_api_win_get_buf(&self, win: i64) -> Option<BufferId> {
        let id = self.plugin_api_resolve_win(win)?;
        self.windows.get(&id).map(|w| w.buffer_id)
    }

    /// Show `buf` in `win`. Returns false if either handle is invalid.
    ///
    /// Fires the existing `BufEnter` event for the newly shown buffer — the
    /// same event `lsp_did_open` fires when a file is opened, so a
    /// plugin-driven buffer switch is observable to extensions the same way a
    /// user-driven one is. Because this necessarily runs *while* a plugin
    /// callback is executing, it is the deferred-event path in action: the
    /// event is queued and dispatched as soon as the outer callback returns
    /// (pre-#1214 a nested event silently vanished).
    pub(crate) fn plugin_api_win_set_buf(&mut self, win: i64, buf: i64) -> bool {
        let Some(win_id) = self.plugin_api_resolve_win(win) else {
            return false;
        };
        let Some(buf_id) = self.plugin_api_resolve_buf(buf) else {
            return false;
        };
        let switched = match self.windows.get_mut(&win_id) {
            Some(w) => {
                let changed = w.buffer_id != buf_id;
                w.buffer_id = buf_id;
                w.view.cursor = crate::core::cursor::Cursor::default();
                w.view.scroll_top = 0;
                changed
            }
            None => return false,
        };
        if switched {
            let arg = self
                .buffer_manager
                .get(buf_id)
                .map(|s| match s.file_path.as_ref() {
                    Some(p) => p.to_string_lossy().into_owned(),
                    None => s.display_name(),
                })
                .unwrap_or_default();
            self.plugin_event("BufEnter", &arg);
        }
        true
    }

    /// Cursor of `win` as 1-indexed `(line, col)`.
    pub(crate) fn plugin_api_win_get_cursor(&self, win: i64) -> Option<(usize, usize)> {
        let id = self.plugin_api_resolve_win(win)?;
        let w = self.windows.get(&id)?;
        Some((w.view.cursor.line + 1, w.view.cursor.col + 1))
    }

    /// Move `win`'s cursor to 1-indexed `(line, col)`, clamped to the buffer.
    pub(crate) fn plugin_api_win_set_cursor(&mut self, win: i64, line: usize, col: usize) -> bool {
        let Some(id) = self.plugin_api_resolve_win(win) else {
            return false;
        };
        let Some(buf_id) = self.windows.get(&id).map(|w| w.buffer_id) else {
            return false;
        };
        let max_line = self
            .buffer_manager
            .get(buf_id)
            .map(|s| s.buffer.len_lines().saturating_sub(1))
            .unwrap_or(0);
        let target_line = line.saturating_sub(1).min(max_line);
        let max_col = self.max_cursor_col_in_buffer(buf_id, target_line);
        if let Some(w) = self.windows.get_mut(&id) {
            w.view.cursor.line = target_line;
            w.view.cursor.col = col.saturating_sub(1).min(max_col);
            return true;
        }
        false
    }

    /// Keep every window showing `buf` inside its (possibly shrunken) bounds
    /// after an immediate edit.
    fn clamp_cursors_to_buffer(&mut self, buf: BufferId) {
        let Some(state) = self.buffer_manager.get(buf) else {
            return;
        };
        let max_line = state.buffer.len_lines().saturating_sub(1);
        let win_ids: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, w)| w.buffer_id == buf)
            .map(|(id, _)| *id)
            .collect();
        for id in win_ids {
            let line = self
                .windows
                .get(&id)
                .map(|w| w.view.cursor.line.min(max_line))
                .unwrap_or(0);
            let max_col = self.max_cursor_col_in_buffer(buf, line);
            if let Some(w) = self.windows.get_mut(&id) {
                w.view.cursor.line = line;
                w.view.cursor.col = w.view.cursor.col.min(max_col);
            }
        }
    }

    /// Longest valid cursor column on `line` of `buf` (0-indexed), for
    /// clamping a cursor that belongs to a buffer other than the active one —
    /// the buffer-agnostic twin of [`Engine::get_max_cursor_col`], with the
    /// same trailing-newline handling.
    fn max_cursor_col_in_buffer(&self, buf: BufferId, line: usize) -> usize {
        let Some(state) = self.buffer_manager.get(buf) else {
            return 0;
        };
        let len = state.buffer.line_len_chars(line);
        if len == 0 {
            return 0;
        }
        let ends_with_newline = state.buffer.content.line(line).chars().last() == Some('\n');
        if ends_with_newline {
            len.saturating_sub(2)
        } else {
            len - 1
        }
    }

    /// Open an undo group for `buf` unless the current dispatch already did.
    fn begin_plugin_undo_group(&mut self, buf: BufferId) {
        if self.plugin_undo_groups.contains(&buf) {
            return;
        }
        let cursor = if buf == self.active_buffer_id() {
            *self.cursor()
        } else {
            crate::core::cursor::Cursor::default()
        };
        if let Some(state) = self.buffer_manager.get_mut(buf) {
            state.start_undo_group(cursor);
            self.plugin_undo_groups.push(buf);
        }
    }

    /// Commit every undo group the immediate API opened during this dispatch.
    fn finish_plugin_undo_groups(&mut self) {
        if self.plugin_undo_groups.is_empty() {
            return;
        }
        let active = self.active_buffer_id();
        let active_cursor = *self.cursor();
        for buf in std::mem::take(&mut self.plugin_undo_groups) {
            let cursor = if buf == active {
                active_cursor
            } else {
                crate::core::cursor::Cursor::default()
            };
            if let Some(state) = self.buffer_manager.get_mut(buf) {
                state.finish_undo_group(cursor);
            }
        }
    }

    /// Drain events that were deferred because they fired from inside a plugin
    /// dispatch (see [`Engine::plugin_event`]).
    fn drain_deferred_plugin_events(&mut self) {
        if self.deferred_plugin_events.is_empty() || self.plugin_events_draining {
            return;
        }
        self.plugin_events_draining = true;
        for _ in 0..MAX_DEFERRED_DRAIN_ROUNDS {
            if self.deferred_plugin_events.is_empty() {
                break;
            }
            for (event, arg) in std::mem::take(&mut self.deferred_plugin_events) {
                self.plugin_event(&event, &arg);
            }
        }
        self.deferred_plugin_events.clear();
        self.plugin_events_draining = false;
    }

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
                self.set_plugin_manager(mgr);
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
    ///
    /// `skip_buf_lines` is retained for API back-compat but is now a no-op
    /// in practice: the buffer is exposed to Lua via a cheap `Rope` clone
    /// (O(1) reference-counted) on the context, and `vimcode.buf.lines()`
    /// / `buf.line(n)` / `buf.get_lines(s, e)` allocate `String`s only
    /// for the range the plugin actually reads. Issue #153: the previous
    /// eager `Vec<String>` build was O(N) in buffer line count and, for
    /// plugins that chained `async_shell` callbacks, ran on every tick
    /// — a 190 k-line buffer pinned CPU at 100 %.
    pub(crate) fn make_plugin_ctx(&self, skip_buf_lines: bool) -> plugin::PluginCallContext {
        let _ = skip_buf_lines;
        let cwd = self.cwd.to_string_lossy().into_owned();
        let buf_id = self.active_buffer_id();
        let buf_state = self.buffer_manager.get(buf_id);
        let buf_path_os = buf_state.as_ref().and_then(|s| s.file_path.clone());
        let buf_path = buf_path_os
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let buf_dirty = buf_state.as_ref().map(|s| s.dirty).unwrap_or(false);
        let buf_rope = buf_state.map(|s| s.buffer.content.clone());
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
        // The plugin ABI predates blockwise registers and still exposes a plain
        // `is_linewise` bool (#807); flatten at the boundary.
        let registers_snapshot = self
            .registers
            .iter()
            .map(|(k, (text, ty))| (*k, (text.clone(), ty.is_linewise())))
            .collect();

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
            buf_rope,
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
            "acp_agent_command",
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
            self.registers
                .insert(ch, (content, RegType::from_linewise(linewise)));
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
                use std::process::Stdio;
                // #948 review (non-blocking): no dedicated regression test
                // for this call site specifically — it goes through the
                // shared `shell_cmd()` construction point (#1492) already
                // covered by `:!`'s tests (`tests/new_vim_features.rs`'s
                // `test_bang_command_honours_shell_env_var` and
                // `src/tui_main/shell_app.rs`'s
                // `bang_command_shell_output_paints_on_command_line_via_shell_app`),
                // so a future divergence here (e.g. someone hand-rolling a
                // shell string again for "just this one" call site) isn't
                // caught by this PR's tests.
                let mut cmd = crate::core::terminal::shell_cmd(&req.command);
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
        // #146: honour `vimcode.ui.refresh(name)`. Bounded by
        // `plugin_view_render_depth` inside `refresh_plugin_view`, so a `render`
        // callback that asks to refresh itself settles instead of recursing.
        for name in ctx.plugin_view_refresh {
            self.refresh_plugin_view(&name);
        }
        // A dispatch's queued output is now applied, so this is the end of that
        // dispatch: flush any event that fired from inside it (#1214). Doing it
        // here rather than at each of the four dispatch entry points keeps the
        // "deferred events run once the outer callback's effects have landed"
        // ordering in one place.
        self.drain_deferred_plugin_events();
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
    ///
    /// **Reentrancy (#1214).** An event that fires *while a plugin callback is
    /// running* — e.g. a plugin's own immediate `vimcode.buffer.set_lines`
    /// triggering a buffer-change event — cannot dispatch straight away,
    /// because Lua must not be re-entered while `&mut Engine` is loaned to it.
    /// Such an event is pushed onto `deferred_plugin_events` and dispatched by
    /// [`Engine::drain_deferred_plugin_events`] the moment the outer dispatch
    /// finishes, so it fires for real instead of the pre-#1214 silent no-op
    /// (the old code `take()`-ed the manager for the duration of the call, so a
    /// nested event found `None` and vanished).
    pub fn plugin_event(&mut self, event: &str, arg: &str) {
        if !self.settings.plugins_enabled {
            return;
        }
        // Skip the potentially O(N_lines) context construction if no hooks are
        // registered for this event.  For cursor_move this avoids building
        // Vec<String> of all buffer lines on every keystroke when no extension
        // has registered a cursor_move listener.
        let has_hooks = self
            .plugin_manager
            .as_ref()
            .is_some_and(|pm| pm.has_event_hooks(event));
        if !has_hooks {
            return;
        }
        if self.plugin_dispatch_depth > 0 {
            if self.deferred_plugin_events.len() < MAX_DEFERRED_PLUGIN_EVENTS {
                self.deferred_plugin_events
                    .push((event.to_string(), arg.to_string()));
            }
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
        let Some(ctx) = self.with_plugin_dispatch(|pm| pm.call_event(event, arg, ctx)) else {
            return;
        };
        // `apply_plugin_ctx` drains the deferred-event queue on the way out.
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

#[cfg(test)]
mod settings_snapshot_tests {
    use crate::core::engine::Engine;

    /// Review regression (#952): the VSCode-extension-settings bridge
    /// whitelist listed `ai_provider`/`ai_model`/`ai_base_url`/
    /// `ai_completions` but not the newer `acp_agent_command` setting, so a
    /// plugin/extension reading `vimcode.settings` (backed by this
    /// snapshot) could never see which ACP agent, if any, the panel is
    /// configured to use.
    #[test]
    fn includes_acp_agent_command() {
        let mut engine = Engine::new_for_test();
        engine.settings.acp_agent_command = "claude-code-acp".to_string();
        let snapshot = engine.settings_snapshot();
        assert_eq!(
            snapshot.get("acp_agent_command").map(String::as_str),
            Some("claude-code-acp"),
            "settings_snapshot() must surface acp_agent_command: {snapshot:?}"
        );
    }
}
