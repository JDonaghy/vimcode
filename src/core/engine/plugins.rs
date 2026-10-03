use super::*;

/// Upper bound on nested plugin events queued during one dispatch. A plugin
/// whose edit fires an event whose handler edits again could otherwise cascade
/// without end; past this point the queue stops growing and the extra events
/// are dropped rather than wedging the editor.
const MAX_DEFERRED_PLUGIN_EVENTS: usize = 64;

/// Upper bound on drain rounds, for the same reason: each round may enqueue
/// more events.
const MAX_DEFERRED_DRAIN_ROUNDS: usize = 8;

/// Upper bound on [`Engine::PluginSpawnEvent`]s drained per
/// `poll_plugin_spawns` tick, across every live `vimcode.loop.spawn` handle
/// combined (#1624 review, non-blocking). Without this, a child that
/// produces output faster than its `on_stdout` Lua callback can be invoked
/// (e.g. a plugin spawning `yes`, or any high-throughput producer) could
/// make a single tick drain an unbounded backlog, starving the main thread —
/// same reasoning as [`MAX_DEFERRED_DRAIN_ROUNDS`] above. Any events left in
/// a handle's channel past the cap are simply picked up on the next tick —
/// nothing is dropped, only deferred.
const MAX_SPAWN_EVENTS_PER_TICK: usize = 256;

/// Upper bound on `Engine::async_shell_last_exit`'s size — see
/// `Engine::record_async_shell_exit`'s doc for why this exists.
const MAX_ASYNC_SHELL_LAST_EXIT_ENTRIES: usize = 256;

/// Which surface currently owns a `vimcode.ui.register_view` view's keyboard
/// focus / selection index — the sidebar body or an editor-area tab
/// (`Engine::open_plugin_view_tab`, #1627).
///
/// The two never hold keyboard focus at once (`Engine::handle_key`'s
/// `ext_panel_has_focus` check always wins over a plugin-view tab's own key
/// routing — see `keys.rs`), but both may be *painted* in the same frame,
/// hosting different views, so each keeps its own selection/scroll index and
/// `FormController` rather than sharing `ext_panel_selected`/
/// `ext_panel_scroll_top`/`plugin_view_form_controller` — see
/// `Engine::plugin_view_tab_selected`'s doc for why sharing would be wrong,
/// not just redundant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PluginViewHost {
    Sidebar,
    Tab,
}

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
    pub fn set_plugin_manager(&mut self, mut mgr: plugin::PluginManager) {
        // #146: seed `plugin_views` so a `vimcode.ui.register_view` panel is
        // recognisable as view-backed before its `render` callback has ever run
        // (the sidebar has to pick a body *shape* on the first frame it paints).
        for name in mgr.view_names() {
            self.plugin_views.entry(name).or_default();
        }
        // #1623: merge load-time `vimcode.keymap.set` registrations into
        // `user_keymaps` so they are consulted through the same
        // before-built-ins path as a config keymap (`Engine::try_user_keymap`).
        // `<leader>` is expanded here — the free-standing harvest inside
        // `PluginManager` has no `Settings` access — mirroring
        // `Engine::rebuild_user_keymaps`.
        let leader = self.settings.leader.to_string();
        for mut km in mgr.take_raw_lua_keymaps() {
            km.keys = expand_leader_tokens(km.keys, &leader);
            self.user_keymaps.push(km);
        }
        // Replace the manager, then drop the outgoing `Rc` before reaping —
        // see `Self::reap_stale_plugin_timers_and_spawns`'s doc for why this
        // ordering (rather than the previous purely-lazy, poll-tick-driven
        // cleanup) is what closes the #1624 review's id-collision finding.
        let old_manager = self.plugin_manager.take();
        self.plugin_manager = Some(std::rc::Rc::new(mgr));
        drop(old_manager);
        self.reap_stale_plugin_timers_and_spawns();
    }

    /// Remove (and, for spawns, kill) every `plugin_timers`/`plugin_spawns`
    /// entry whose owning `PluginManager` is no longer reachable. Called
    /// synchronously from [`Self::set_plugin_manager`], right after
    /// installing the new manager and dropping the old one.
    ///
    /// #1624 review (blocking): `PluginManager::next_handle_id` restarts at
    /// 0 for every fresh manager, while `plugin_timers`/`plugin_spawns` are
    /// single `HashMap`s that persist across manager generations. Cleanup
    /// used to be entirely lazy — done only inside `poll_plugin_timers`/
    /// `poll_plugin_spawns` on the *next* `poll_idle` tick — so a plugin
    /// reload (`plugin_init()`, called mid-session by extension
    /// install/uninstall and `:PluginEnable`/`:PluginDisable`) could install
    /// a manager whose very first `vimcode.loop.timer`/`vimcode.loop.spawn`
    /// call reused id 0 *before* that next tick ran, silently overwriting
    /// (via `HashMap::insert`) the previous generation's still-live id-0
    /// entry — for a spawn, dropping its `PluginSpawnHandle` (and the
    /// `Arc<Mutex<Child>>` inside it) without ever calling `child.kill()`,
    /// leaking an orphaned child process. Reaping here, before any new
    /// registration can happen (nothing in `mgr.load_plugins_dir` above can
    /// call the immediate API — there is no live engine loan yet — so the
    /// earliest a fresh manager can register anything is *after* this
    /// method returns), closes the race instead of merely narrowing it.
    fn reap_stale_plugin_timers_and_spawns(&mut self) {
        self.plugin_timers
            .retain(|_, entry| entry.manager.upgrade().is_some());
        let dead_spawns: Vec<u64> = self
            .plugin_spawns
            .iter()
            .filter(|(_, handle)| handle.manager.upgrade().is_none())
            .map(|(id, _)| *id)
            .collect();
        for id in dead_spawns {
            if let Some(handle) = self.plugin_spawns.remove(&id) {
                if let Ok(mut child) = handle.child.lock() {
                    let _ = child.kill();
                }
            }
        }
        // #1630: same staleness pattern for `vimcode.picker.open` — a picker
        // whose owning manager just went away is closed (if it's the one
        // currently open) without calling into the dead Lua state, the same
        // "clean up, don't call back" rule the spawns loop above follows.
        // The manager itself is already gone (strong count 0, or this
        // `upgrade()` would have succeeded), so there is no `PluginManager`
        // side to clean up — only `Engine`'s own bookkeeping.
        let dead_pickers: Vec<u64> = self
            .plugin_pickers
            .iter()
            .filter(|(_, mgr)| mgr.upgrade().is_none())
            .map(|(id, _)| *id)
            .collect();
        for id in dead_pickers {
            self.plugin_pickers.remove(&id);
            if self.picker_open && self.picker_source == Self::plugin_picker_source(id) {
                self.close_picker();
            }
        }
        // #1632: same staleness pattern for `vimcode.http.request` — an
        // in-flight request whose owning manager just went away is killed
        // (its `curl` child) and dropped without ever calling into the dead
        // Lua state, same reasoning as the spawns loop above.
        let dead_http: Vec<u64> = self
            .plugin_http_requests
            .iter()
            .filter(|(_, handle)| handle.manager.upgrade().is_none())
            .map(|(id, _)| *id)
            .collect();
        for id in dead_http {
            if let Some(handle) = self.plugin_http_requests.remove(&id) {
                if let Ok(mut child) = handle.child.lock() {
                    let _ = child.kill();
                }
            }
        }
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
                    // A shorter field stack must not leave the selection past
                    // its end. `view.body.is_none()` guards this to
                    // field-stack views only (#1631 review) — a body-kind
                    // view's `fields` is *always* empty
                    // (`Some(body)` ⟹ `fields: Vec::new()`, `core::plugin::
                    // parse_view_body`), so `ext_panel_selected >= view.
                    // fields.len()` was `>= 0`, always true: every dispatch
                    // that changed the view's painted output (e.g. a List's
                    // own `ItemSelected`, which this fixture's `on_event`
                    // re-tags on every selection move) reset the selection
                    // straight back to `0` immediately after
                    // `navigate_flat_selection`/`route_plugin_view_list_
                    // click` had just set it — the exact reason repeated
                    // `Down` never accumulated past index 1 before this
                    // fix. Each body-kind handler already clamps its own
                    // selection against the live row/item count at every
                    // read site (`Engine::plugin_view_selected(host).min(len
                    // - 1)`), so it needs no clamp here.
                    if view.body.is_none()
                        && self.ext_panel_active.as_deref() == Some(name)
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
            self.plugin_view_focus_field(name, PluginViewHost::Sidebar, first);
        }
    }

    // ── In-panel text entry / editor-tab hosting (#1627) ────────────────────

    /// Whether `name` names the view currently hosted as an editor-area tab
    /// in the *active* window, if any (`Engine::open_plugin_view_tab`).
    pub fn active_plugin_view_tab(&self) -> Option<String> {
        let buf = self.windows.get(&self.active_window_id())?.buffer_id;
        self.buffer_manager.get(buf)?.plugin_view.clone()
    }

    /// Open `name` (a `vimcode.ui.register_view` view) as a new tab in the
    /// active editor group, the way `vimcode.ui.open_view(name, {location =
    /// "tab"})` is wired (#1627).
    ///
    /// Reuses `Engine::new_tab`'s existing scratch-buffer-in-a-new-tab
    /// machinery verbatim — the resulting tab is a genuine `Tab`/`Window`/
    /// `BufferId` triple, so closing it, splitting its window and moving it
    /// between editor groups all already work with zero new code (#1627's
    /// "closes like any tab, survives split and group moves"). The buffer is
    /// flagged `BufferState::plugin_view` so `render::build_rendered_window`
    /// paints the view's `Form` instead of buffer text (`app.rs`'s
    /// `paint_editor_windows_rung`) — see that field's doc for the rest of
    /// the wiring. `scratch_name` is the pre-existing tab-title mechanism
    /// (`render.rs`'s tab-title fallback already reads it), so no new
    /// tab-title plumbing is needed either.
    pub fn open_plugin_view_tab(&mut self, name: &str) -> bool {
        if !self.is_plugin_view(name) {
            return false;
        }
        let title = self
            .ext_panels
            .get(name)
            .map(|p| p.title.clone())
            .unwrap_or_else(|| name.to_string());
        self.new_tab(None);
        let buf_id = self.active_buffer_id();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.read_only = true;
            state.dirty = false;
            state.scratch_name = Some(title);
            state.plugin_view = Some(name.to_string());
        }
        // `open_plugin_view_tab` runs via the immediate (`live_engine`) API,
        // which is only reachable from within an already-running plugin
        // callback (`vimcode.ui.open_view`'s own doc) — so
        // `plugin_dispatch_depth` is always `> 0` here in practice, and
        // `refresh_plugin_view`'s own `with_plugin_dispatch` call would just
        // no-op (its reentrancy guard, for good reason: Lua cannot be
        // re-entered while the outer call hasn't returned). Deferring to
        // `plugin_view_tab_pending_refresh` — drained by
        // `with_plugin_dispatch` the moment depth returns to `0` — is the
        // same fix `vimcode.ui.refresh` already applies to the identical
        // problem via `PluginCallContext::plugin_view_refresh`.
        if self.plugin_dispatch_depth == 0 {
            self.refresh_plugin_view(name);
        } else {
            self.plugin_view_tab_pending_refresh.push(name.to_string());
        }
        let first = self
            .plugin_views
            .get(name)
            .and_then(|v| v.first_focusable())
            .unwrap_or(0);
        self.plugin_view_focus_field(name, PluginViewHost::Tab, first);
        true
    }

    /// Read the selection index for `host`.
    pub(crate) fn plugin_view_selected(&self, host: PluginViewHost) -> usize {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_selected,
            PluginViewHost::Tab => self.plugin_view_tab_selected,
        }
    }

    /// Write the selection index for `host`.
    pub(crate) fn set_plugin_view_selected(&mut self, host: PluginViewHost, idx: usize) {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_selected = idx,
            PluginViewHost::Tab => self.plugin_view_tab_selected = idx,
        }
    }

    /// Read the scroll offset for `host` (#1631 — `List`/`Table`/`TextView`
    /// body kinds reuse the same flat scroll state a field-stack view uses,
    /// rather than a parallel field per body kind).
    pub(crate) fn plugin_view_scroll_top(&self, host: PluginViewHost) -> usize {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_scroll_top,
            PluginViewHost::Tab => self.plugin_view_tab_scroll_top,
        }
    }

    /// Write the scroll offset for `host`.
    pub(crate) fn set_plugin_view_scroll_top(&mut self, host: PluginViewHost, offset: usize) {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_scroll_top = offset,
            PluginViewHost::Tab => self.plugin_view_tab_scroll_top = offset,
        }
    }

    /// Read the raw selected-column index for `host`'s `Table`-kind view
    /// (#1631). "Raw" — callers that need it clamped to an actually-
    /// editable column (`render::handle_plugin_view_table_key`'s
    /// `current_col`) do that themselves, the same "read-time clamp"
    /// contract `Engine::plugin_view_selected` uses for a stale row index.
    pub(crate) fn plugin_view_table_col(&self, host: PluginViewHost) -> usize {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_table_col,
            PluginViewHost::Tab => self.plugin_view_tab_table_col,
        }
    }

    /// Write the selected-column index for `host`'s `Table`-kind view.
    pub(crate) fn set_plugin_view_table_col(&mut self, host: PluginViewHost, col: usize) {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_table_col = col,
            PluginViewHost::Tab => self.plugin_view_tab_table_col = col,
        }
    }

    /// Nudge `host`'s scroll offset so its selection stays visible — the
    /// tab twin of `Engine::ext_panel_ensure_visible` (fixed 20-row viewport
    /// guess, same as that method's own fallback).
    ///
    /// `pub(crate)` (not just used from this module) since `render::
    /// navigate_flat_selection` — the `List`/`Table` body-kind keyboard
    /// navigation (#1631) — calls this after every selection move, the same
    /// way the field-stack navigation just below does; without it a
    /// `Down`/`End`/`G` press could move `selected` out of the visible
    /// scroll window with nothing to compensate.
    pub(crate) fn plugin_view_ensure_visible(&mut self, host: PluginViewHost) {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_ensure_visible(0),
            PluginViewHost::Tab => {
                const ROWS: usize = 20;
                let sel = self.plugin_view_tab_selected;
                if sel < self.plugin_view_tab_scroll_top {
                    self.plugin_view_tab_scroll_top = sel;
                } else if sel >= self.plugin_view_tab_scroll_top + ROWS {
                    self.plugin_view_tab_scroll_top = sel.saturating_sub(ROWS - 1);
                }
            }
        }
    }

    /// Move `host`'s keyboard focus to field `idx` of view `name`: blurs
    /// whatever text edit was in progress on the previously-focused field
    /// (committing it first if it was a `TextArea`, discarding otherwise —
    /// see [`Self::blur_plugin_view_text_edit`]), moves the selection, and
    /// primes [`Engine::plugin_view_text_edit`] if the newly-focused field is
    /// a `Text`/`Password`/`TextArea` kind.
    ///
    /// The single entry point for changing which field a plugin view's
    /// keyboard focus is on — every navigation path (initial panel/tab focus,
    /// `j`/`k`/`Tab`/`BackTab`, a mouse click) routes through this so blur/
    /// prime can never be skipped on one path and not another.
    pub(crate) fn plugin_view_focus_field(&mut self, name: &str, host: PluginViewHost, idx: usize) {
        self.blur_plugin_view_text_edit(name, host);
        self.set_plugin_view_selected(host, idx);
        self.prime_plugin_view_text_edit(name, idx);
    }

    /// Commit-or-discard whatever [`Engine::plugin_view_text_edit`] holds for
    /// `host`'s currently-focused field in view `name`, then clear it.
    ///
    /// A `TextArea` commits on blur (fires `TextCommitted` with the buffer's
    /// current value); `Text`/`Password` do not — #1627 scopes single-line
    /// blur-commit out (only Enter commits those), so leaving one without
    /// pressing Enter discards the in-progress edit and the next paint shows
    /// whatever the plugin last declared. This mirrors the existing
    /// `Engine::explorer_rename`/`handle_settings_key` precedent of
    /// vimcode-owned, plugin-invisible edit buffers.
    fn blur_plugin_view_text_edit(&mut self, name: &str, host: PluginViewHost) {
        let Some(state) = self.plugin_view_text_edit.take() else {
            return;
        };
        // Only this (view, host)'s own field, identified by the selection
        // index it was primed from — a stale `Some` here (e.g. a full
        // `ui.refresh` mid-edit that dropped fields) is simply discarded.
        if state.view != name {
            return;
        }
        let selected = self.plugin_view_selected(host);
        let Some(field) = self
            .plugin_views
            .get(name)
            .and_then(|v| v.fields.get(selected))
        else {
            return;
        };
        if field.id != state.field_id {
            return;
        }
        if matches!(
            field.kind,
            crate::core::plugin_ui::ViewFieldKind::TextArea { .. }
        ) {
            self.dispatch_plugin_view_event(crate::core::plugin_ui::PluginViewEvent {
                view: name.to_string(),
                widget_id: state.field_id,
                kind: crate::core::plugin_ui::ViewEventKind::TextCommitted { value: state.value },
            });
        }
    }

    /// Populate [`Engine::plugin_view_text_edit`] from field `idx` of view
    /// `name`'s *declared* value (cursor at the end, no selection) — or clear
    /// it when that field isn't a `Text`/`Password`/`TextArea` kind.
    fn prime_plugin_view_text_edit(&mut self, name: &str, idx: usize) {
        use crate::core::plugin_ui::{PluginViewTextEditState, ViewFieldKind};
        let value = match self.plugin_views.get(name).and_then(|v| v.fields.get(idx)) {
            Some(f) => match &f.kind {
                ViewFieldKind::Text { value, .. }
                | ViewFieldKind::Password { value, .. }
                | ViewFieldKind::TextArea { value, .. } => Some((f.id.clone(), value.clone())),
                _ => None,
            },
            None => None,
        };
        self.plugin_view_text_edit = value.map(|(field_id, value)| {
            let cursor = value.len();
            PluginViewTextEditState {
                view: name.to_string(),
                field_id,
                value,
                cursor,
                selection_anchor: None,
            }
        });
    }

    /// Keyboard handling for a view-backed sidebar panel or editor-area tab.
    ///
    /// Returns `true` when the key was consumed. For the sidebar, keys this
    /// returns `false` for fall through to `Engine::handle_ext_panel_key`'s
    /// generic panel bindings (`q`/`Escape` to unfocus, `h`/`Left` back to the
    /// activity bar, `?` help), so a plugin view keeps the same panel chrome
    /// bindings every other sidebar panel has; for a tab there is no such
    /// chrome to fall through to (`keys.rs`'s caller just drops the key).
    ///
    /// Navigation skips rows that cannot emit events (`label`, `read_only`,
    /// `disabled`), so `j`/`k`/`Tab`/`BackTab` never park the selection
    /// somewhere `Enter` does nothing.
    ///
    /// When the focused row is a `Text`/`Password`/`TextArea` field, every key
    /// except `Tab`/`BackTab` (field-to-field navigation always wins, so a
    /// view is never a keyboard trap) is handed to
    /// [`Self::handle_plugin_view_text_key`] instead — typed characters must
    /// insert into the field rather than double as `j`/`k` navigation once a
    /// field can actually take text (#1627).
    pub(crate) fn handle_plugin_view_key(
        &mut self,
        name: &str,
        key: &str,
        ctrl: bool,
        unicode: Option<char>,
        host: PluginViewHost,
    ) -> bool {
        use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind, ViewFieldKind};

        let Some(view) = self.plugin_views.get(name) else {
            return false;
        };

        // #1631: a `ViewBody`-kind view routes through its own kind's
        // keyboard handling instead of the field-stack navigation below —
        // see `render::paint_plugin_view_list`'s sibling doc comment for why
        // this is "one sibling translation per kind" rather than a scattered
        // `Form`-field encoding of list/tree/table semantics.
        if let Some(body_kind) = view.body.as_ref().map(|b| b.kind_name()) {
            return match body_kind {
                "list" => crate::render::handle_plugin_view_list_key(self, name, host, key),
                "tree" => crate::render::handle_plugin_view_tree_key(self, name, host, key),
                "table" => {
                    crate::render::handle_plugin_view_table_key(self, name, host, key, unicode)
                }
                "text_view" => {
                    crate::render::handle_plugin_view_text_key(self, name, host, key, 10)
                }
                _ => false,
            };
        }

        let focusable: Vec<usize> = view
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| f.is_interactive() && !f.disabled)
            .map(|(i, _)| i)
            .collect();
        let selected = self.plugin_view_selected(host);
        let is_text_field = view.fields.get(selected).is_some_and(|f| {
            !f.disabled
                && matches!(
                    f.kind,
                    ViewFieldKind::Text { .. }
                        | ViewFieldKind::Password { .. }
                        | ViewFieldKind::TextArea { .. }
                )
        });

        // Field-to-field navigation always wins, text field or not.
        match key {
            "Tab" => {
                let next = focusable
                    .iter()
                    .copied()
                    .find(|i| *i > selected)
                    .or_else(|| focusable.first().copied());
                if let Some(i) = next {
                    self.plugin_view_focus_field(name, host, i);
                }
                self.plugin_view_ensure_visible(host);
                return true;
            }
            "BackTab" => {
                let prev = focusable
                    .iter()
                    .rev()
                    .copied()
                    .find(|i| *i < selected)
                    .or_else(|| focusable.last().copied());
                if let Some(i) = prev {
                    self.plugin_view_focus_field(name, host, i);
                }
                self.plugin_view_ensure_visible(host);
                return true;
            }
            _ => {}
        }

        if is_text_field {
            return self.handle_plugin_view_text_key(name, host, selected, key, ctrl, unicode);
        }

        match key {
            "j" | "Down" => {
                let next = focusable
                    .iter()
                    .copied()
                    .find(|i| *i > selected)
                    .or_else(|| focusable.first().copied());
                if let Some(i) = next {
                    self.plugin_view_focus_field(name, host, i);
                }
                self.plugin_view_ensure_visible(host);
                true
            }
            "k" | "Up" => {
                let prev = focusable
                    .iter()
                    .rev()
                    .copied()
                    .find(|i| *i < selected)
                    .or_else(|| focusable.last().copied());
                if let Some(i) = prev {
                    self.plugin_view_focus_field(name, host, i);
                }
                self.plugin_view_ensure_visible(host);
                true
            }
            "g" => {
                if let Some(i) = focusable.first().copied() {
                    self.plugin_view_focus_field(name, host, i);
                }
                match host {
                    PluginViewHost::Sidebar => self.ext_panel_scroll_top = 0,
                    PluginViewHost::Tab => self.plugin_view_tab_scroll_top = 0,
                }
                true
            }
            "G" => {
                if let Some(i) = focusable.last().copied() {
                    self.plugin_view_focus_field(name, host, i);
                }
                self.plugin_view_ensure_visible(host);
                true
            }
            "Return" | "Enter" | "Space" | " " => {
                let Some(view) = self.plugin_views.get(name) else {
                    return true;
                };
                let Some(field) = view.fields.get(selected) else {
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
                    ViewFieldKind::Text { .. }
                    | ViewFieldKind::Password { .. }
                    | ViewFieldKind::TextArea { .. } => {
                        // Unreachable: `is_text_field` routes every text-kind
                        // field to `handle_plugin_view_text_key` above, which
                        // owns Enter for these kinds (commit).
                        return true;
                    }
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

    /// Keyboard handling for a focused `Text`/`Password`/`TextArea` row —
    /// insert/delete, cursor movement, selection and clipboard paste, all
    /// vimcode-owned (#1627). Mirrors `Engine::handle_explorer_rename_key`'s
    /// shape (byte-offset cursor/selection, the same key set) since that is
    /// the existing precedent for a vimcode-owned inline text editor; kept as
    /// a separate implementation rather than a shared helper because that
    /// method lives in `buffers.rs`, outside this issue's file list, and its
    /// `ExplorerRenameState` has no plugin/view/commit-event concept to
    /// generalize over.
    ///
    /// `TextArea` additionally supports a literal newline (plain Enter) and
    /// multi-line `Up`/`Down` cursor movement by logical line; `Text`/
    /// `Password` treat `Up`/`Down` as field-to-field navigation, same as a
    /// non-text row, since a single-line value has no "next line". Only
    /// `TextArea` commits on blur/Ctrl-Enter — see
    /// `Self::blur_plugin_view_text_edit`'s doc for why `Text`/`Password`
    /// don't.
    fn handle_plugin_view_text_key(
        &mut self,
        name: &str,
        host: PluginViewHost,
        field_idx: usize,
        key: &str,
        ctrl: bool,
        unicode: Option<char>,
    ) -> bool {
        use crate::core::plugin_ui::{
            PluginViewEvent, PluginViewTextEditState, ViewEventKind, ViewFieldKind,
        };

        let Some(field) = self
            .plugin_views
            .get(name)
            .and_then(|v| v.fields.get(field_idx))
        else {
            return true;
        };
        let field_id = field.id.clone();
        let is_textarea = matches!(field.kind, ViewFieldKind::TextArea { .. });

        // Defensive re-prime: normally already true via
        // `Engine::plugin_view_focus_field`, but a `ui.refresh` mid-edit can
        // in principle replace the field the selection points at.
        if !matches!(&self.plugin_view_text_edit, Some(s) if s.view == name && s.field_id == field_id)
        {
            self.prime_plugin_view_text_edit(name, field_idx);
        }
        let Some(mut state) = self.plugin_view_text_edit.take() else {
            return true;
        };

        // Escape: leave the field entirely (falls through to whatever "leave
        // this view" means for `host` — unfocus the sidebar panel, or a no-op
        // for a tab) rather than being swallowed here. `blur_plugin_view_text_
        // edit` (run when the caller actually changes focus/host state)
        // discards a `Text`/`Password` edit at that point, so this already
        // has "Escape cancels the edit" behaviour, just folded into "leave".
        if key == "Escape" {
            self.plugin_view_text_edit = Some(state);
            return false;
        }

        fn delete_selection(s: &mut PluginViewTextEditState) -> bool {
            if let Some(anchor) = s.selection_anchor.take() {
                let lo = anchor.min(s.cursor);
                let hi = anchor.max(s.cursor);
                if lo != hi {
                    s.value.drain(lo..hi);
                    s.cursor = lo;
                    return true;
                }
            }
            false
        }
        // Byte range `[start, end)` of the logical line containing `cursor`
        // (split on `\n`; a single-line value is its own one line).
        fn line_bounds(value: &str, cursor: usize) -> (usize, usize) {
            let start = value[..cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let end = value[cursor..]
                .find('\n')
                .map(|i| cursor + i)
                .unwrap_or(value.len());
            (start, end)
        }

        let mut changed = false;
        let mut committed = false;
        match key {
            "BackSpace" => {
                if !delete_selection(&mut state) && state.cursor > 0 {
                    let prev = state.value[..state.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    state.value.remove(prev);
                    state.cursor = prev;
                }
                changed = true;
            }
            "Delete" => {
                if !delete_selection(&mut state) && state.cursor < state.value.len() {
                    state.value.remove(state.cursor);
                }
                changed = true;
            }
            "Left" => {
                if state.cursor > 0 {
                    state.cursor = state.value[..state.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
                state.selection_anchor = None;
            }
            "Right" => {
                if state.cursor < state.value.len() {
                    let rest = &state.value[state.cursor..];
                    state.cursor = rest
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| state.cursor + i)
                        .unwrap_or(state.value.len());
                }
                state.selection_anchor = None;
            }
            "Home" => {
                state.cursor = if is_textarea {
                    line_bounds(&state.value, state.cursor).0
                } else {
                    0
                };
                state.selection_anchor = None;
            }
            "End" => {
                state.cursor = if is_textarea {
                    line_bounds(&state.value, state.cursor).1
                } else {
                    state.value.len()
                };
                state.selection_anchor = None;
            }
            "Up" if is_textarea => {
                let (line_start, _) = line_bounds(&state.value, state.cursor);
                if line_start > 0 {
                    let col = state.cursor - line_start;
                    let (prev_start, prev_end) = line_bounds(&state.value, line_start - 1);
                    state.cursor = (prev_start + col).min(prev_end);
                }
                state.selection_anchor = None;
            }
            "Down" if is_textarea => {
                let (_, line_end) = line_bounds(&state.value, state.cursor);
                if line_end < state.value.len() {
                    let (line_start, _) = line_bounds(&state.value, state.cursor);
                    let col = state.cursor - line_start;
                    let next_start = line_end + 1;
                    let (_, next_end) = line_bounds(&state.value, next_start);
                    state.cursor = (next_start + col).min(next_end);
                }
                state.selection_anchor = None;
            }
            "Up" | "Down" => {
                // Text/Password: no vertical concept — behaves like a
                // non-text row's Up/Down, moving focus to the next field.
                self.plugin_view_text_edit = Some(state);
                let focusable: Vec<usize> = self
                    .plugin_views
                    .get(name)
                    .map(|v| {
                        v.fields
                            .iter()
                            .enumerate()
                            .filter(|(_, f)| f.is_interactive() && !f.disabled)
                            .map(|(i, _)| i)
                            .collect()
                    })
                    .unwrap_or_default();
                let target = if key == "Down" {
                    focusable
                        .iter()
                        .copied()
                        .find(|i| *i > field_idx)
                        .or_else(|| focusable.first().copied())
                } else {
                    focusable
                        .iter()
                        .rev()
                        .copied()
                        .find(|i| *i < field_idx)
                        .or_else(|| focusable.last().copied())
                };
                if let Some(i) = target {
                    self.plugin_view_focus_field(name, host, i);
                }
                self.plugin_view_ensure_visible(host);
                return true;
            }
            "Return" | "Enter" if is_textarea && !ctrl => {
                delete_selection(&mut state);
                state.value.insert(state.cursor, '\n');
                state.cursor += 1;
                changed = true;
            }
            "Return" | "Enter" => {
                committed = true;
            }
            _ if ctrl => match key {
                "a" => {
                    state.selection_anchor = Some(0);
                    state.cursor = state.value.len();
                }
                "c" => {
                    if let Some((lo, hi)) = state.selection_range() {
                        if lo != hi {
                            let text = state.value[lo..hi].to_string();
                            if let Some(ref cb) = self.clipboard_write {
                                let _ = cb(&text);
                            }
                        }
                    }
                }
                "x" => {
                    if let Some((lo, hi)) = state.selection_range() {
                        if lo != hi {
                            let text = state.value[lo..hi].to_string();
                            if let Some(ref cb) = self.clipboard_write {
                                let _ = cb(&text);
                            }
                            delete_selection(&mut state);
                            changed = true;
                        }
                    }
                }
                "v" => {
                    delete_selection(&mut state);
                    let paste = if let Some(ref cb) = self.clipboard_read {
                        cb().unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let text = if is_textarea {
                        paste
                    } else {
                        paste.lines().next().unwrap_or("").to_string()
                    };
                    state.value.insert_str(state.cursor, &text);
                    state.cursor += text.len();
                    changed = true;
                }
                _ => {}
            },
            _ => {
                if let Some(ch) = unicode {
                    if !ch.is_control() {
                        delete_selection(&mut state);
                        state.value.insert(state.cursor, ch);
                        state.cursor += ch.len_utf8();
                        changed = true;
                    }
                }
            }
        }

        if committed {
            let value = state.value.clone();
            self.plugin_view_text_edit = None;
            self.dispatch_plugin_view_event(PluginViewEvent {
                view: name.to_string(),
                widget_id: field_id,
                kind: ViewEventKind::TextCommitted { value },
            });
            return true;
        }

        let value_for_event = changed.then(|| state.value.clone());
        self.plugin_view_text_edit = Some(state);
        if let Some(value) = value_for_event {
            self.dispatch_plugin_view_event(PluginViewEvent {
                view: name.to_string(),
                widget_id: field_id,
                kind: ViewEventKind::TextChanged { value },
            });
        }
        true
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
        // #1627: now that depth is genuinely back to `0`, it's safe to run
        // any `Engine::open_plugin_view_tab` refresh that had to defer
        // itself while nested inside this very call — see
        // `plugin_view_tab_pending_refresh`'s doc for why it can't just run
        // eagerly. Drained here (the one place every dispatch, immediate or
        // queued, funnels back through) rather than at `open_plugin_view_
        // tab`'s own call site, since that site has no way to know when the
        // *outermost* call will actually finish.
        if self.plugin_dispatch_depth == 0 && !self.plugin_view_tab_pending_refresh.is_empty() {
            let pending = std::mem::take(&mut self.plugin_view_tab_pending_refresh);
            for name in pending {
                self.refresh_plugin_view(&name);
                // The seeded-empty view had no focusable field yet when
                // `open_plugin_view_tab` first parked the selection; now that
                // real fields exist, re-park it properly.
                if let Some(first) = self
                    .plugin_views
                    .get(&name)
                    .and_then(|v| v.first_focusable())
                {
                    if self.active_plugin_view_tab().as_deref() == Some(name.as_str()) {
                        self.plugin_view_focus_field(&name, PluginViewHost::Tab, first);
                    }
                }
            }
        }
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
        let mut removed_lines = 0usize;
        let mut inserted_lines = 0usize;
        if char_end > char_start {
            let old: String = state.buffer.content.slice(char_start..char_end).to_string();
            removed_lines = old.matches('\n').count();
            state.record_delete(char_start, &old);
            state.buffer.delete_range(char_start, char_end);
        }
        if !lines.is_empty() {
            let mut text = String::new();
            for line in &lines {
                text.push_str(line);
                text.push('\n');
            }
            inserted_lines = text.matches('\n').count();
            let insert_at = char_start.min(state.buffer.len_chars());
            state.record_insert(insert_at, &text);
            state.buffer.insert(insert_at, &text);
        }
        state.dirty = true;
        state.mark_syntax_stale();
        // #1653: `set_lines` is a line-range splice — a delete of `[s, e)`
        // followed by an insert of the new lines at `s` — so decoration
        // marks shift exactly like the `d`/`O` paths in `engine/buffers.rs`,
        // even though this immediate API bypasses `insert_with_undo`/
        // `delete_with_undo` (it records undo directly above instead).
        if removed_lines > 0 {
            self.decor.shift_delete(buf, s, removed_lines);
        }
        if inserted_lines > 0 {
            self.decor.shift_insert(buf, s, inserted_lines, true);
        }
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
        let old_buf_id = self.windows.get(&win_id).map(|w| w.buffer_id);
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
            // #1623: BufLeave for the window's previous buffer, delivering
            // its handle — fired before BufEnter, same ordering vim uses.
            if let Some(old) = old_buf_id {
                self.plugin_event("BufLeave", &old.0.to_string());
            }
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

    // =========================================================================
    // `vimcode.decor.*` (#1653, Native API P5) — namespaces, extmarks,
    // highlights, virtual text and signs. Thin buffer/namespace-handle
    // resolution wrappers around `Engine::decor` (a
    // `crate::core::buffer::DecorState`); the Lua bindings in `plugin.rs`
    // only convert Lua tables to/from the plain types declared there.
    // =========================================================================

    /// `vimcode.decor.namespace(name)` → namespace id.
    pub(crate) fn plugin_api_decor_namespace(&mut self, name: &str) -> i64 {
        self.decor.namespace(name).0 as i64
    }

    pub(crate) fn plugin_api_decor_set_hl(
        &mut self,
        name: &str,
        def: crate::core::buffer::HlGroupDef,
    ) {
        self.decor.set_hl(name, def);
    }

    /// `vimcode.decor.set_mark(buf, ns, opts)` → mark id, or `None` when
    /// `buf` doesn't name a live buffer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn plugin_api_decor_set_mark(
        &mut self,
        buf: i64,
        ns: i64,
        row: usize,
        col: usize,
        end_row: Option<usize>,
        end_col: Option<usize>,
        opts: crate::core::buffer::DecorOpts,
    ) -> Option<i64> {
        let buf_id = self.plugin_api_resolve_buf(buf)?;
        let ns_id = crate::core::buffer::NamespaceId(ns as u32);
        Some(
            self.decor
                .set_mark(buf_id, ns_id, row, col, end_row, end_col, opts)
                .0 as i64,
        )
    }

    /// `vimcode.decor.get_mark(buf, ns, id)` → the mark, or `None` when the
    /// handle is stale, `buf` is dead, or the mark belongs to a different
    /// namespace (namespace isolation — see `DecorState::get_mark`).
    pub(crate) fn plugin_api_decor_get_mark(
        &self,
        buf: i64,
        ns: i64,
        id: i64,
    ) -> Option<crate::core::buffer::DecorMark> {
        let buf_id = self.plugin_api_resolve_buf(buf)?;
        let ns_id = crate::core::buffer::NamespaceId(ns as u32);
        self.decor
            .get_mark(buf_id, ns_id, crate::core::buffer::MarkId(id as u64))
            .cloned()
    }

    /// `vimcode.decor.del_mark(buf, ns, id)` → bool.
    pub(crate) fn plugin_api_decor_del_mark(&mut self, buf: i64, ns: i64, id: i64) -> bool {
        let Some(buf_id) = self.plugin_api_resolve_buf(buf) else {
            return false;
        };
        let ns_id = crate::core::buffer::NamespaceId(ns as u32);
        self.decor
            .del_mark(buf_id, ns_id, crate::core::buffer::MarkId(id as u64))
    }

    /// `vimcode.decor.clear(ns, buf, start?, end?)` — `start`/`end` are
    /// 0-indexed buffer rows with an exclusive `end`, same convention as
    /// `vimcode.buffer.get_lines`/`set_lines`. `None` clears every mark in
    /// `ns` regardless of position.
    pub(crate) fn plugin_api_decor_clear(
        &mut self,
        ns: i64,
        buf: i64,
        range: Option<(usize, usize)>,
    ) -> bool {
        let Some(buf_id) = self.plugin_api_resolve_buf(buf) else {
            return false;
        };
        let ns_id = crate::core::buffer::NamespaceId(ns as u32);
        self.decor.clear(buf_id, ns_id, range);
        true
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
        // Launch async shell requests, built on the same `spawn_piped` core
        // `vimcode.loop.spawn` uses (#1624) — see `execute::AsyncShellTask`
        // and `Self::poll_async_shells` for how the streamed events are
        // accumulated back into `async_shell`'s frozen single-string,
        // deliver-at-exit contract.
        //
        // #948 review (non-blocking): no dedicated regression test for this
        // call site specifically — it goes through the shared `shell_cmd()`
        // construction point (#1492) already covered by `:!`'s tests
        // (`tests/new_vim_features.rs`'s `test_bang_command_honours_shell_
        // env_var` and `src/tui_main/shell_app.rs`'s `bang_command_shell_
        // output_paints_on_command_line_via_shell_app`), so a future
        // divergence here (e.g. someone hand-rolling a shell string again
        // for "just this one" call site) isn't caught by this PR's tests.
        for req in ctx.async_shell_requests {
            let mut cmd = crate::core::terminal::shell_cmd(&req.command);
            if let Some(ref cwd) = req.cwd {
                cmd.current_dir(cwd);
            }
            let task = match execute::spawn_piped(cmd, req.stdin) {
                Ok((_child, _stdin, rx)) => execute::AsyncShellTask {
                    rx,
                    stdout_buf: String::new(),
                },
                Err(_) => {
                    // Match the old behaviour's immediate `(false, "")`
                    // failure result: deliver a synthetic exit on the very
                    // next `poll_async_shells` tick rather than dropping the
                    // request silently.
                    let (tx, rx) = std::sync::mpsc::channel();
                    let _ = tx.send(execute::PluginSpawnEvent::Exit {
                        code: None,
                        signal: None,
                    });
                    execute::AsyncShellTask {
                        rx,
                        stdout_buf: String::new(),
                    }
                }
            };
            // Last-writer-wins: replace any pending task for the same callback event.
            self.async_shell_tasks.insert(req.callback_event, task);
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
    ///
    /// Drains every [`execute::PluginSpawnEvent`] pending for each live task,
    /// accumulating stdout chunks and firing `plugin_event(event, output)`
    /// only once `Exit` arrives — preserving `async_shell`'s frozen "deliver
    /// the whole output at exit" contract even though the underlying process
    /// is now spawned through the same streaming core `vimcode.loop.spawn`
    /// uses (#1624). `Engine::async_shell_last_exit` is updated first, so a
    /// handler reading `vimcode.async_shell_exit_code(event)` from inside the
    /// very callback this fires sees the code that just landed.
    ///
    /// Returns `true` if any results were delivered (caller should redraw).
    pub fn poll_async_shells(&mut self) -> bool {
        let mut completed: Vec<(String, String, Option<i32>)> = Vec::new();
        for (event, task) in self.async_shell_tasks.iter_mut() {
            while let Ok(ev) = task.rx.try_recv() {
                match ev {
                    execute::PluginSpawnEvent::Stdout(chunk) => task.stdout_buf.push_str(&chunk),
                    execute::PluginSpawnEvent::Stderr(_) => {}
                    execute::PluginSpawnEvent::Exit { code, .. } => {
                        completed.push((event.clone(), std::mem::take(&mut task.stdout_buf), code));
                    }
                }
            }
        }
        if completed.is_empty() {
            return false;
        }
        for (event, output, code) in completed {
            self.async_shell_tasks.remove(&event);
            self.record_async_shell_exit(event.clone(), code);
            self.plugin_event(&event, &output);
        }
        true
    }

    /// Record `event`'s exit code for `vimcode.async_shell_exit_code`,
    /// bounding `Engine::async_shell_last_exit`'s size (#1624 review,
    /// non-blocking). Without a cap, a plugin that generates a unique or
    /// otherwise ever-changing `callback_event` name per `async_shell` call
    /// would grow this map unboundedly for the life of the session — every
    /// entry is only ever overwritten by a later run of the *same* event
    /// name, never pruned. `async_shell_last_exit_order` tracks insertion
    /// order (a plain FIFO, not a true LRU: re-recording an existing event
    /// does not move it to the back) purely so *some* bound exists; exactly
    /// which entry gets evicted once at the cap is not a contract plugins
    /// should rely on.
    fn record_async_shell_exit(&mut self, event: String, code: Option<i32>) {
        if !self.async_shell_last_exit.contains_key(&event) {
            self.async_shell_last_exit_order.push_back(event.clone());
            while self.async_shell_last_exit_order.len() > MAX_ASYNC_SHELL_LAST_EXIT_ENTRIES {
                if let Some(oldest) = self.async_shell_last_exit_order.pop_front() {
                    self.async_shell_last_exit.remove(&oldest);
                }
            }
        }
        self.async_shell_last_exit.insert(event, code);
    }

    // ─── `vimcode.loop.timer`/`vimcode.schedule`/`vimcode.defer` (#1624) ────

    /// Register a timer/schedule/defer callback, due `ms` milliseconds from
    /// now (repeating every `ms` if `repeat`). Returns the handle id as an
    /// `i64` for the Lua boundary (`-1` if there is no live plugin manager,
    /// which cannot happen when called through `live_engine` from inside a
    /// dispatch, but keeps the method total).
    pub(crate) fn plugin_api_register_timer(
        &mut self,
        ms: i64,
        repeat: bool,
        callback: mlua::RegistryKey,
    ) -> i64 {
        let Some(pm) = self.plugin_manager.clone() else {
            return -1;
        };
        let id = pm.register_timer_callback(callback);
        let interval = std::time::Duration::from_millis(ms.max(0) as u64);
        let seq = self.plugin_timer_seq;
        self.plugin_timer_seq += 1;
        self.plugin_timers.insert(
            id,
            PluginTimerEntry {
                manager: std::rc::Rc::downgrade(&pm),
                interval,
                repeat,
                next_due: std::time::Instant::now() + interval,
                seq,
            },
        );
        id as i64
    }

    /// `vimcode.loop.timer(...):stop()` — cancel a timer before it (next)
    /// fires. A no-op if `id` already fired (non-repeating) or was already
    /// stopped.
    pub(crate) fn plugin_api_stop_timer(&mut self, id: i64) {
        if id < 0 {
            return;
        }
        if let Some(entry) = self.plugin_timers.remove(&(id as u64)) {
            if let Some(pm) = entry.manager.upgrade() {
                pm.remove_timer_callback(id as u64);
            }
        }
    }

    /// Fire every timer/schedule/defer whose `next_due` has passed. Called
    /// from `poll_idle`. Returns `true` if any callback fired.
    ///
    /// Due entries are processed in `(next_due, seq)` order — a `HashMap`'s
    /// iteration order is unspecified, so without the explicit tie-break
    /// `vimcode.schedule(a); vimcode.schedule(b)` (both due "now") would have
    /// no guaranteed ordering, breaking the "schedule/defer ordering"
    /// acceptance bar.
    pub fn poll_plugin_timers(&mut self) -> bool {
        if self.plugin_timers.is_empty() {
            return false;
        }
        let now = std::time::Instant::now();
        let mut due: Vec<(std::time::Instant, u64, u64)> = self
            .plugin_timers
            .iter()
            .filter(|(_, t)| t.next_due <= now)
            .map(|(id, t)| (t.next_due, t.seq, *id))
            .collect();
        if due.is_empty() {
            return false;
        }
        due.sort();
        let mut fired = false;
        for (_, _, id) in due {
            // A callback earlier in this same batch may have stopped a later
            // one (or itself, for a repeating timer) — re-check liveness.
            let Some(entry) = self.plugin_timers.get(&id) else {
                continue;
            };
            if entry.manager.upgrade().is_none() {
                // The owning plugin was unloaded since this became due
                // (`Engine::set_plugin_manager` replaced it) — drop the
                // stale entry without ever calling into the dead Lua state.
                self.plugin_timers.remove(&id);
                continue;
            }
            let repeat = entry.repeat;
            if repeat {
                let interval = entry.interval;
                if let Some(e) = self.plugin_timers.get_mut(&id) {
                    e.next_due = now + interval;
                }
            } else {
                // Drop the *scheduling* entry now (so a callback that calls
                // `stop()` on its own already-fired, non-repeating handle is
                // a harmless no-op), but keep the Lua callback registered
                // until after it actually runs below — removing it first
                // would make `call_timer_callback` find nothing to call.
                self.plugin_timers.remove(&id);
            }
            let ctx = self.make_plugin_ctx(true);
            let Some(ctx) = self.with_plugin_dispatch(|pm| pm.call_timer_callback(id, ctx)) else {
                continue;
            };
            self.apply_plugin_ctx(ctx);
            fired = true;
            if !repeat {
                if let Some(pm) = self.plugin_manager.clone() {
                    pm.remove_timer_callback(id);
                }
            }
        }
        fired
    }

    // ─── `vimcode.loop.spawn` (#1624) ───────────────────────────────────────

    /// `vimcode.loop.spawn(cmd, args, opts)`: launch a child process and wire
    /// its streamed stdout/stderr and exit status to the given (optional)
    /// callbacks. Returns the handle id, or `None` if the process could not
    /// be started (the Lua binding surfaces that as a runtime error) or
    /// there is no live plugin manager.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn plugin_api_spawn(
        &mut self,
        cmd: String,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        env: Vec<(String, String)>,
        on_stdout: Option<mlua::RegistryKey>,
        on_stderr: Option<mlua::RegistryKey>,
        on_exit: Option<mlua::RegistryKey>,
    ) -> Option<i64> {
        let pm = self.plugin_manager.clone()?;
        let mut command = std::process::Command::new(&cmd);
        command.args(&args);
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        for (k, v) in &env {
            command.env(k, v);
        }
        let (child, stdin, rx) = execute::spawn_piped(command, None).ok()?;
        let id = pm.register_spawn_callbacks(on_stdout, on_stderr, on_exit);
        self.plugin_spawns.insert(
            id,
            execute::PluginSpawnHandle {
                manager: std::rc::Rc::downgrade(&pm),
                child,
                stdin,
                rx,
            },
        );
        Some(id as i64)
    }

    /// `vimcode.loop.spawn(...):write(data)` — write to the child's stdin.
    /// Returns `false` if the handle is unknown, its stdin was already
    /// closed, or the write failed.
    pub(crate) fn plugin_api_spawn_write(&mut self, id: i64, data: &str) -> bool {
        if id < 0 {
            return false;
        }
        let Some(handle) = self.plugin_spawns.get(&(id as u64)) else {
            return false;
        };
        let Ok(mut guard) = handle.stdin.lock() else {
            return false;
        };
        match guard.as_mut() {
            Some(stdin) => std::io::Write::write_all(stdin, data.as_bytes()).is_ok(),
            None => false,
        }
    }

    /// `vimcode.loop.spawn(...):close_stdin()` — close the child's stdin
    /// (EOF), so a filter-style child that reads until EOF can proceed.
    pub(crate) fn plugin_api_spawn_close_stdin(&mut self, id: i64) -> bool {
        if id < 0 {
            return false;
        }
        let Some(handle) = self.plugin_spawns.get(&(id as u64)) else {
            return false;
        };
        let Ok(mut guard) = handle.stdin.lock() else {
            return false;
        };
        *guard = None;
        true
    }

    /// `vimcode.loop.spawn(...):kill()` — send a kill signal to the child.
    /// The resulting exit (signal death, on unix) is delivered to `on_exit`
    /// on a later `poll_plugin_spawns` tick, same as a natural exit.
    pub(crate) fn plugin_api_spawn_kill(&mut self, id: i64) -> bool {
        if id < 0 {
            return false;
        }
        let Some(handle) = self.plugin_spawns.get(&(id as u64)) else {
            return false;
        };
        let Ok(mut child) = handle.child.lock() else {
            return false;
        };
        child.kill().is_ok()
    }

    /// Deliver every pending [`execute::PluginSpawnEvent`] for every live
    /// `vimcode.loop.spawn` handle, in arrival order per handle, through the
    /// plugin dispatch loan so `on_stdout`/`on_stderr`/`on_exit` can use the
    /// immediate API. Called from `poll_idle`.
    ///
    /// A handle whose owning plugin manager was unloaded (`Engine::
    /// set_plugin_manager` replaced it) is killed and dropped here instead
    /// of having its callbacks invoked — #1624's "unloading a plugin cancels
    /// its timers and spawns" with "no calls into an unloaded plugin".
    ///
    /// Returns `true` if any callback fired (caller should redraw).
    pub fn poll_plugin_spawns(&mut self) -> bool {
        if self.plugin_spawns.is_empty() {
            return false;
        }
        let ids: Vec<u64> = self.plugin_spawns.keys().copied().collect();
        let mut redraw = false;
        let mut events_this_tick = 0usize;
        'handles: for id in ids {
            let alive = self
                .plugin_spawns
                .get(&id)
                .map(|h| h.manager.upgrade().is_some())
                .unwrap_or(false);
            if !alive {
                if let Some(handle) = self.plugin_spawns.remove(&id) {
                    if let Ok(mut child) = handle.child.lock() {
                        let _ = child.kill();
                    }
                }
                continue;
            }
            loop {
                // Bounded per-tick drain (#1624 review): check the budget
                // *before* touching the channel. `try_recv()` removes the
                // message the moment it returns `Ok`, so testing the cap
                // after the receive would pop an event and then drop it on
                // the `break` — losing a stdout chunk, or worse an `Exit`,
                // for good. Leaving it queued means the next `poll_idle`
                // tick picks it up: nothing is dropped, only deferred.
                if events_this_tick >= MAX_SPAWN_EVENTS_PER_TICK {
                    break 'handles;
                }
                let Some(event) = self
                    .plugin_spawns
                    .get(&id)
                    .and_then(|h| h.rx.try_recv().ok())
                else {
                    break;
                };
                events_this_tick += 1;
                let mut exited = false;
                let ctx = self.make_plugin_ctx(true);
                let applied =
                    match event {
                        execute::PluginSpawnEvent::Stdout(chunk) => self
                            .with_plugin_dispatch(move |pm| pm.call_spawn_stdout(id, &chunk, ctx)),
                        execute::PluginSpawnEvent::Stderr(chunk) => self
                            .with_plugin_dispatch(move |pm| pm.call_spawn_stderr(id, &chunk, ctx)),
                        execute::PluginSpawnEvent::Exit { code, signal } => {
                            exited = true;
                            self.with_plugin_dispatch(move |pm| {
                                pm.call_spawn_exit(id, code, signal, ctx)
                            })
                        }
                    };
                if let Some(ctx) = applied {
                    self.apply_plugin_ctx(ctx);
                    redraw = true;
                }
                if exited {
                    self.plugin_spawns.remove(&id);
                    if let Some(pm) = self.plugin_manager.clone() {
                        pm.remove_spawn_callbacks(id);
                    }
                    break;
                }
            }
        }
        redraw
    }

    // ─── `vimcode.http` (#1632) ─────────────────────────────────────────────

    /// `vimcode.http.request(opts, cb)`: launch an async HTTP request on a
    /// background thread and wire its eventual response (or error) to `cb`.
    /// Returns the handle id, or `None` if the request could not be started
    /// (failed to spawn `curl` — the Lua binding surfaces that as a runtime
    /// error, matching `plugin_api_spawn`'s "failed to start" convention) or
    /// there is no live plugin manager.
    ///
    /// Registers `on_response` in `pm`'s callback registry *before* spawning
    /// `curl`, deregistering it again if the spawn fails — the reverse order
    /// (spawn first, register only on success) leaked `on_response`'s
    /// `LuaRegistryKey` slot on the "curl failed to start" path, since it was
    /// dropped without ever reaching `remove_http_callback` (#1632 review).
    pub(crate) fn plugin_api_http_request(
        &mut self,
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Option<String>,
        timeout_ms: u64,
        on_response: mlua::RegistryKey,
    ) -> Option<i64> {
        let pm = self.plugin_manager.clone()?;
        let spec = execute::HttpRequestSpec {
            method,
            url,
            headers,
            body,
            timeout_ms,
        };
        let id = pm.register_http_callback(on_response);
        let (child, rx) = match execute::spawn_http_request(spec) {
            Ok(pipes) => pipes,
            Err(_) => {
                pm.remove_http_callback(id);
                return None;
            }
        };
        self.plugin_http_requests.insert(
            id,
            execute::PluginHttpHandle {
                manager: std::rc::Rc::downgrade(&pm),
                child,
                rx,
            },
        );
        Some(id as i64)
    }

    /// `vimcode.http.request(...):cancel()` — kill the underlying `curl`
    /// child and drop the handle immediately. No callback fires for a
    /// cancelled request, whether or not `curl` had already produced a
    /// response by the time this runs — the caller asked to stop caring, not
    /// to be told the answer arrived a moment too late.
    pub(crate) fn plugin_api_http_cancel(&mut self, id: i64) -> bool {
        if id < 0 {
            return false;
        }
        let id = id as u64;
        let Some(handle) = self.plugin_http_requests.remove(&id) else {
            return false;
        };
        if let Ok(mut child) = handle.child.lock() {
            let _ = child.kill();
        }
        if let Some(pm) = self.plugin_manager.clone() {
            pm.remove_http_callback(id);
        }
        true
    }

    /// Deliver every completed `vimcode.http.request` result, through the
    /// plugin dispatch loan so the callback can use the immediate API.
    /// Called from `poll_idle`.
    ///
    /// A handle whose owning plugin manager was unloaded is killed and
    /// dropped here instead of having its callback invoked — same rule as
    /// [`Self::poll_plugin_spawns`].
    ///
    /// Returns `true` if any callback fired (caller should redraw).
    pub fn poll_plugin_http(&mut self) -> bool {
        if self.plugin_http_requests.is_empty() {
            return false;
        }
        let ids: Vec<u64> = self.plugin_http_requests.keys().copied().collect();
        let mut redraw = false;
        for id in ids {
            let alive = self
                .plugin_http_requests
                .get(&id)
                .map(|h| h.manager.upgrade().is_some())
                .unwrap_or(false);
            if !alive {
                if let Some(handle) = self.plugin_http_requests.remove(&id) {
                    if let Ok(mut child) = handle.child.lock() {
                        let _ = child.kill();
                    }
                }
                continue;
            }
            let Some(result) = self
                .plugin_http_requests
                .get(&id)
                .and_then(|h| h.rx.try_recv().ok())
            else {
                continue;
            };
            // Exactly one result per handle — drop the handle the moment it
            // arrives, before dispatching into Lua (mirrors the spawn
            // `Exit` case's "remove, then call" ordering).
            self.plugin_http_requests.remove(&id);
            let ctx = self.make_plugin_ctx(true);
            let applied =
                self.with_plugin_dispatch(move |pm| pm.call_http_response(id, result, ctx));
            if let Some(ctx) = applied {
                self.apply_plugin_ctx(ctx);
                redraw = true;
            }
            if let Some(pm) = self.plugin_manager.clone() {
                pm.remove_http_callback(id);
            }
        }
        redraw
    }

    // ─── `vimcode.picker` (#1630) ───────────────────────────────────────────

    /// `vimcode.picker.open(...)`: open the unified picker fed by plugin
    /// data. Returns the handle id, or `None` if there is no live plugin
    /// manager (the Lua binding surfaces that as a runtime error, matching
    /// `plugin_api_spawn`'s "failed to start" convention).
    pub(crate) fn plugin_api_picker_open(
        &mut self,
        title: String,
        on_select: Option<mlua::RegistryKey>,
        on_cancel: Option<mlua::RegistryKey>,
        on_query: Option<mlua::RegistryKey>,
    ) -> Option<u64> {
        let pm = self.plugin_manager.clone()?;
        // #1630 review: a plugin re-invoking `vimcode.picker.open` while a
        // previous handle is still registered (the natural telescope-style
        // pattern — a command re-run on every keypress) must not leak the
        // old registration. `open_picker` below unconditionally clears
        // `picker_all_items`/`picker_source` with no idea a plugin picker
        // used to own them, so tear the old one down here first — same
        // bookkeeping as an explicit `:close()`, just without firing
        // `on_cancel` (this isn't a user cancel, it's a supersession).
        if let Some(old_id) = self.plugin_picker_id() {
            self.plugin_picker_teardown(old_id);
        }
        let id = pm.register_picker(on_select, on_cancel, on_query);
        self.plugin_pickers.insert(id, std::rc::Rc::downgrade(&pm));
        self.open_picker(PickerSource::Custom(format!("plugin:{id}")));
        // #1630 review: don't let a title-less `open()` call blank out
        // `open_picker`'s own `format!("{:?}", source)` fallback with an
        // empty header.
        self.picker_title = if title.is_empty() {
            "Picker".to_string()
        } else {
            title
        };
        Some(id)
    }

    /// The `PickerSource::Custom("plugin:<id>")` naming scheme both
    /// directions of this feature share: encoding it once here (rather than
    /// inlining `format!("plugin:{id}")` and `strip_prefix("plugin:")` at
    /// every call site) is what makes the two ends impossible to typo out of
    /// sync.
    fn plugin_picker_source(id: u64) -> PickerSource {
        PickerSource::Custom(format!("plugin:{id}"))
    }

    /// The id of the currently-open picker, if (and only if) it is a
    /// `vimcode.picker.open` one — used to gate live-update calls (a
    /// `set_items` from a stale/superseded handle must not touch whatever
    /// picker is open *now*) and to fire `on_cancel`/`on_query` only for a
    /// plugin-owned session.
    pub(crate) fn plugin_picker_is_active(&self, id: u64) -> bool {
        self.picker_open && self.picker_source == Self::plugin_picker_source(id)
    }

    /// The id of the currently-open picker, if it is a plugin one — read by
    /// `handle_picker_key`'s `Escape` arm (fire `on_cancel`) before
    /// `close_picker` clears `picker_open`.
    pub(crate) fn plugin_picker_id(&self) -> Option<u64> {
        match &self.picker_source {
            PickerSource::Custom(key) if self.picker_open => {
                key.strip_prefix("plugin:").and_then(|s| s.parse().ok())
            }
            _ => None,
        }
    }

    /// Parse a `PickerAction::Custom("plugin_item:<picker_id>:<item_id>")`
    /// key, as built by `plugin_api_picker_set_items`.
    pub(crate) fn parse_plugin_item_key(key: &str) -> Option<(u64, u64)> {
        let rest = key.strip_prefix("plugin_item:")?;
        let (a, b) = rest.split_once(':')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    }

    /// Drop picker `id`'s bookkeeping on both sides: `Engine::plugin_
    /// pickers` and `PluginManager::pickers` (which releases every item's
    /// stored `data`/preview with it). Called on select, on cancel
    /// (including the non-Escape ways a picker can be dismissed —
    /// `Engine::close_picker_cancelling_plugin`, used by GTK's
    /// click-outside-to-dismiss), on an explicit `:close()`, when a fresh
    /// `vimcode.picker.open` supersedes a still-registered handle (see
    /// `plugin_api_picker_open` above), and when the owning plugin unloads.
    /// Every path that ends a plugin picker's life routes through here or
    /// through `run_plugin_picker_select`/`run_plugin_picker_cancel` (which
    /// call this after firing their callback) — #1630 review found two
    /// paths that didn't.
    fn plugin_picker_teardown(&mut self, id: u64) {
        self.plugin_pickers.remove(&id);
        if let Some(pm) = self.plugin_manager.clone() {
            pm.remove_picker(id);
        }
    }

    /// `vimcode.picker.open(...):set_items(items)` /
    /// `:append(items)` — `replace = true` for the former, `false` for the
    /// latter. A no-op if `id` doesn't name the *currently open* picker (a
    /// stale handle from a picker the user already closed, or one a later
    /// `vimcode.picker.open` call superseded).
    ///
    /// `append` re-filters against the current query without resetting
    /// `picker_selected`/`picker_scroll_top` — only clamping them if the
    /// filtered list shrank — so a streamed source doesn't yank the cursor
    /// out from under the user on every chunk (#1630's "without resetting
    /// the cursor" acceptance bar).
    pub(crate) fn plugin_api_picker_set_items(
        &mut self,
        id: u64,
        items: Vec<plugin::PluginPickerItemSpec>,
        replace: bool,
    ) {
        if !self.plugin_picker_is_active(id) {
            return;
        }
        let Some(pm) = self.plugin_manager.clone() else {
            return;
        };
        if replace {
            pm.clear_picker_items(id);
        }
        let built: Vec<PickerItem> = items
            .into_iter()
            .map(|spec| {
                let item_id = pm.register_picker_item(id, spec.data, spec.preview);
                let filter_text = spec.filter_text.unwrap_or_else(|| spec.display.clone());
                PickerItem {
                    display: spec.display,
                    filter_text,
                    detail: spec.detail,
                    action: PickerAction::Custom(format!("plugin_item:{id}:{item_id}")),
                    icon: spec.icon,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }
            })
            .collect();
        if replace {
            self.picker_all_items = built;
            self.picker_selected = 0;
            self.picker_scroll_top = 0;
        } else {
            self.picker_all_items.extend(built);
        }
        self.picker_filter();
        let max = self.picker_items.len().saturating_sub(1);
        self.picker_selected = self.picker_selected.min(max);
        self.picker_scroll_top = self.picker_scroll_top.min(self.picker_selected);
        self.picker_load_preview();
    }

    /// `vimcode.picker.open(...):set_loading(bool)`. State only — see
    /// `Engine::picker_loading`'s doc for why this doesn't (yet) paint
    /// anything.
    pub(crate) fn plugin_api_picker_set_loading(&mut self, id: u64, loading: bool) {
        if self.plugin_picker_is_active(id) {
            self.picker_loading = loading;
        }
    }

    /// `vimcode.picker.open(...):close()`.
    pub(crate) fn plugin_api_picker_close(&mut self, id: u64) {
        if self.plugin_picker_is_active(id) {
            self.close_picker();
        }
        self.plugin_picker_teardown(id);
    }

    /// `PickerAction::Custom("plugin_item:<id>:<item_id>")` confirmed —
    /// fire `on_select` with the item's `data`, then tear the picker down.
    /// Called from `Engine::picker_confirm`, *after* it has already called
    /// `close_picker()`.
    pub(crate) fn run_plugin_picker_select(&mut self, id: u64, item_id: u64) {
        let ctx = self.make_plugin_ctx(true);
        if let Some(ctx) = self.with_plugin_dispatch(|pm| pm.call_picker_select(id, item_id, ctx)) {
            self.apply_plugin_ctx(ctx);
        }
        self.plugin_picker_teardown(id);
    }

    /// The user backed out of a plugin-owned picker without confirming an
    /// item — fire `on_cancel`, then tear it down. Called from
    /// `Engine::close_picker_cancelling_plugin` (Escape, and GTK's
    /// click-outside-to-dismiss), after `close_picker()`.
    pub(crate) fn run_plugin_picker_cancel(&mut self, id: u64) {
        let ctx = self.make_plugin_ctx(true);
        if let Some(ctx) = self.with_plugin_dispatch(|pm| pm.call_picker_cancel(id, ctx)) {
            self.apply_plugin_ctx(ctx);
        }
        self.plugin_picker_teardown(id);
    }

    /// The query text changed while a plugin-owned picker is open — fire
    /// `on_query` (a no-op if the picker declared none). Called from
    /// `Engine::picker_filter`. Does *not* tear the picker down; it stays
    /// open regardless of what (if anything) `on_query` does.
    ///
    /// **Reentrancy note (#1630 review):** this goes through
    /// `with_plugin_dispatch`, which is a silent no-op while a dispatch is
    /// already in flight (`plugin_dispatch_depth > 0`). If `:append`/
    /// `:set_items` is called from *inside* an already-running plugin
    /// callback — e.g. a `vimcode.loop.spawn` `on_stdout` handler calling
    /// `handle:append(...)` for a live-grep-style source — the resulting
    /// `picker_filter()` call here is dropped: the local fuzzy re-filter
    /// still runs (so a static list still narrows correctly), but `on_query`
    /// will not fire for that keystroke. A dynamic source that wants
    /// `on_query` to reliably re-run per keystroke while it's also streaming
    /// results from a nested callback should be aware of this gap.
    pub(crate) fn fire_plugin_picker_query(&mut self, id: u64) {
        let query = self.picker_query.clone();
        let ctx = self.make_plugin_ctx(true);
        if let Some(ctx) = self.with_plugin_dispatch(|pm| pm.call_picker_query(id, &query, ctx)) {
            self.apply_plugin_ctx(ctx);
        }
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

    /// Fire `CursorMoved` (or `CursorMovedI` in Insert/Replace mode) for the
    /// active window (#1623). Arg is the active window handle, matching the
    /// immediate API's integer-handle convention (a plugin reads the actual
    /// position back via `vimcode.window.get_cursor`).
    fn fire_cursor_moved_event(&mut self) {
        let event = if matches!(self.mode, Mode::Insert | Mode::Replace) {
            "CursorMovedI"
        } else {
            "CursorMoved"
        };
        let win = self.active_window_id().0.to_string();
        self.plugin_event(event, &win);
    }

    /// Flush the `CursorMoved`/`CursorMovedI` debounce (#1623) — a sibling of
    /// [`Self::flush_cursor_move_hook`], kept on its own timestamp
    /// (`cursor_moved_event_pending`) precisely so it cannot alter that
    /// method's legacy Normal-mode-only firing scope. Called from the same
    /// `poll_idle` tick. Returns `true` when the event fired (needs redraw).
    pub fn flush_cursor_moved_event(&mut self) -> bool {
        let Some(when) = self.cursor_moved_event_pending else {
            return false;
        };
        if when.elapsed() < std::time::Duration::from_millis(150) {
            return false;
        }
        self.cursor_moved_event_pending = None;
        self.fire_cursor_moved_event();
        true
    }

    /// Fire `WinLeave`/`WinEnter` for a window-focus change (#1623). Handles
    /// are the (decimal) window id, matching the immediate API's convention.
    /// No-op — including no event-hook lookup — when the window didn't
    /// actually change, so callers can call this unconditionally after every
    /// "make this window active" operation without worrying about the
    /// common case (already-active window) being expensive.
    pub(crate) fn fire_win_focus_change(&mut self, old: WindowId, new: WindowId) {
        if old == new {
            return;
        }
        self.plugin_event("WinLeave", &old.0.to_string());
        self.plugin_event("WinEnter", &new.0.to_string());
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
