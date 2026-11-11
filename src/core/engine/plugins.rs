use super::*;

/// Pack a `vimcode.completion` generation + source id into the opaque
/// `request_id` a source is handed and must echo back to `vimcode.
/// completion.complete`. The generation lives in the high 32 bits so a
/// request from a superseded trigger (the user kept typing) can be told
/// apart from a current one just by comparing against `Engine::
/// completion_generation` — no need to track every outstanding request id
/// explicitly.
fn pack_completion_request_id(generation: u64, source_id: u64) -> u64 {
    debug_assert!(
        source_id <= u32::MAX as u64,
        "completion source id {source_id} doesn't fit in the low 32 bits of the packed request id"
    );
    (generation << 32) | (source_id & 0xFFFF_FFFF)
}

/// Inverse of [`pack_completion_request_id`]: `(generation, source_id)`.
fn unpack_completion_request_id(request_id: u64) -> (u64, u64) {
    (request_id >> 32, request_id & 0xFFFF_FFFF)
}

/// Map an engine key event to the `quadraui::Key` [`quadraui::ComboBox::
/// handle_key`] recognises, for [`Engine::handle_plugin_view_key`]'s
/// `combo_box` branch.
///
/// `None` for a key the primitive has no path for at all (`Home`/`End`/
/// `Delete`/`Left`/`Right`/function keys, or a `ctrl`-chord whose `unicode`
/// is `None`) — unlike the vimcode-owned `Text`/`Password` cursor model,
/// `quadraui::ComboBox` only ever appends to or backspaces from the *end*
/// of `text` (see that primitive's own doc), so there is no richer
/// fallback to hand an unclaimed key off to; it simply falls through to
/// whatever the caller does for an unrecognised key on any other field.
fn combo_box_key_from(key: &str, unicode: Option<char>) -> Option<quadraui::Key> {
    match key {
        "Up" => Some(quadraui::Key::Named(quadraui::NamedKey::Up)),
        "Down" => Some(quadraui::Key::Named(quadraui::NamedKey::Down)),
        "Return" | "Enter" => Some(quadraui::Key::Named(quadraui::NamedKey::Enter)),
        "Escape" => Some(quadraui::Key::Named(quadraui::NamedKey::Escape)),
        "BackSpace" => Some(quadraui::Key::Named(quadraui::NamedKey::Backspace)),
        "Space" | " " => Some(quadraui::Key::Char(' ')),
        _ => unicode.filter(|c| !c.is_control()).map(quadraui::Key::Char),
    }
}

/// Map an engine key event to the `quadraui::Key`
/// [`quadraui::primitives::number_input::NumberInput::handle_key`]
/// recognises, for [`Engine::handle_plugin_view_key`]'s `number_input`
/// branch.
///
/// `None` for a key the primitive has no path for at all (`Left`/`Right`/
/// `Home`/`End`/function keys, letters, or a `ctrl`-chord whose `unicode`
/// is `None`) — unlike [`combo_box_key_from`]'s "any non-control character"
/// fallback, `NumberInput::handle_key` only accepts an ASCII digit, `-` or
/// `.` as typed input (see its own `is_number_char`), so a key outside that
/// set simply falls through to whatever the caller does for an unrecognised
/// key on any other field.
fn number_input_key_from(key: &str, unicode: Option<char>) -> Option<quadraui::Key> {
    match key {
        "Up" => Some(quadraui::Key::Named(quadraui::NamedKey::Up)),
        "Down" => Some(quadraui::Key::Named(quadraui::NamedKey::Down)),
        "Return" | "Enter" => Some(quadraui::Key::Named(quadraui::NamedKey::Enter)),
        "Escape" => Some(quadraui::Key::Named(quadraui::NamedKey::Escape)),
        "BackSpace" => Some(quadraui::Key::Named(quadraui::NamedKey::Backspace)),
        _ => unicode
            .filter(|c| c.is_ascii_digit() || *c == '-' || *c == '.')
            .map(quadraui::Key::Char),
    }
}

/// Indices, within a `Buttons` or `Toggles` row's own item list, that the
/// keyboard can land sub-focus on — every item for `Toggles` (no per-toggle
/// `disabled` key exists), the non-`disabled` ones for `Buttons`. `None` for
/// any other field kind, including a `Buttons`/`Toggles` row with no
/// enabled items at all.
///
/// Shared by [`Engine::plugin_view_focus_field`] (seeding sub-focus on the
/// first enabled item whenever a row gains field-level focus) and
/// [`Engine::handle_plugin_view_key`]'s `Left`/`Right`/`h`/`l` arm (moving
/// within it).
fn plugin_view_row_item_indices(
    kind: &crate::core::plugin_ui::ViewFieldKind,
) -> Option<Vec<usize>> {
    use crate::core::plugin_ui::ViewFieldKind;
    match kind {
        ViewFieldKind::Buttons { buttons } => Some(
            buttons
                .iter()
                .enumerate()
                .filter(|(_, b)| !b.disabled)
                .map(|(i, _)| i)
                .collect(),
        ),
        ViewFieldKind::Toggles { toggles } => Some((0..toggles.len()).collect()),
        _ => None,
    }
}

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
/// combined. Without this, a child that produces output faster than its
/// `on_stdout` Lua callback can be invoked (e.g. a plugin spawning `yes`, or
/// any high-throughput producer) could make a single tick drain an unbounded
/// backlog, starving the main thread — same reasoning as
/// [`MAX_DEFERRED_DRAIN_ROUNDS`] above. Any events left in a handle's channel
/// past the cap are simply picked up on the next tick — nothing is dropped,
/// only deferred.
const MAX_SPAWN_EVENTS_PER_TICK: usize = 256;

/// Upper bound on `Engine::async_shell_last_exit`'s size — see
/// `Engine::record_async_shell_exit`'s doc for why this exists.
const MAX_ASYNC_SHELL_LAST_EXIT_ENTRIES: usize = 256;

/// The form of a workspace root that is safe to hand to
/// [`std::process::Command::current_dir`] for a plugin-spawned shell.
///
/// `Engine::cwd` is frequently a `canonicalize()` result, which on Windows is
/// the verbatim `\\?\C:\...` extended-length form. Every plugin shell command
/// on Windows routes through `cmd /C` (`terminal.rs`'s `shell_cmd`), and
/// `cmd.exe` refuses a non-drive-rooted current directory ("UNC paths are not
/// supported. Defaulting to Windows directory"), so the verbatim prefix has to
/// come off — [`crate::core::paths::strip_unc_prefix`], as `engine/windows.rs`
/// and `render.rs` already do.
///
/// The one shape that must *not* be stripped is a **verbatim UNC** root
/// (`\\?\UNC\server\share\...`, what `canonicalize()` yields for a workspace
/// on a network share): stripping leaves `UNC\server\share\...`, a *relative*
/// path that `current_dir` would resolve against the process cwd — strictly
/// worse than the un-stripped path, since it can silently spawn the child
/// somewhere real but wrong rather than just failing (#1751 review round 2).
/// Keeping the original is the conservative choice: `cmd.exe` can't chdir into
/// a UNC root in either form, so the only difference is "fails loudly" vs
/// "runs in the wrong place".
///
/// A no-op on non-Windows, where `strip_unc_prefix` itself is a no-op.
fn spawn_cwd(cwd: &std::path::Path) -> std::borrow::Cow<'_, std::path::Path> {
    let stripped = crate::core::paths::strip_unc_prefix(cwd);
    if stripped.is_absolute() {
        stripped
    } else {
        std::borrow::Cow::Borrowed(cwd)
    }
}

/// Which surface currently owns a `vimcode.ui.register_view` view's keyboard
/// focus / selection index — the sidebar body, an editor-area tab
/// (`Engine::open_plugin_view_tab`), or the bottom-panel dock
/// (`Engine::open_plugin_view_dock`).
///
/// No two ever hold keyboard focus at once (`Engine::handle_key`'s
/// `ext_panel_has_focus` check always wins over a plugin-view tab's own key
/// routing — see `keys.rs`; the dock does not take keyboard focus at all,
/// mirroring the bottom panel's other non-Terminal content), but any
/// combination may be *painted* in the same frame, hosting different views,
/// so each keeps its own selection/scroll index and `FormController` rather
/// than sharing one — see `Engine::plugin_view_tab_selected`'s doc for why
/// sharing would be wrong, not just redundant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PluginViewHost {
    Sidebar,
    Tab,
    Dock,
    /// A centred modal over the editor (`Engine::open_plugin_view_modal`).
    /// Unlike the dock it *does* take keyboard focus — a modal owns every
    /// key until it is dismissed.
    Modal,
}

/// Who owned the keyboard the moment the plugin-view modal took it
/// (`Engine::open_plugin_view_modal`), so dismissing the modal can hand
/// focus back instead of always dropping the user into the editor.
///
/// Exactly the flags `Engine::clear_sidebar_focus` and the modal's own
/// `terminal_has_focus = false` clear. A partially typed `<leader>ai`
/// gesture (`ai_leader_toggle_pending`) is deliberately *not* captured:
/// `clear_sidebar_focus` discards it on purpose whenever focus moves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PluginViewModalFocus {
    terminal: bool,
    explorer: bool,
    search: bool,
    source_control: bool,
    debug: bool,
    extensions: bool,
    ai: bool,
    settings: bool,
    settings_modal_open: bool,
    ext_panel: bool,
    activity_bar: bool,
}

impl Engine {
    // =========================================================================
    // Plugin system
    // =========================================================================

    /// Install a plugin manager. Wraps it in the `Rc` the dispatch path needs:
    /// dispatch clones the `Rc` out rather than `take()`ing the manager, so the
    /// field stays populated for the duration of a Lua call — that is
    /// what lets nested code still see "there is a plugin manager" and what
    /// makes loaning `&mut Engine` to Lua sound (no outstanding borrow of
    /// `self.plugin_manager` is held across the call).
    pub fn set_plugin_manager(&mut self, mut mgr: plugin::PluginManager) {
        // Seed `plugin_views` so a `vimcode.ui.register_view` panel is
        // recognisable as view-backed before its `render` callback has ever run
        // (the sidebar has to pick a body *shape* on the first frame it paints).
        for name in mgr.view_names() {
            self.plugin_views.entry(name).or_default();
        }
        // Merge load-time `vimcode.keymap.set` registrations into
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
    /// `PluginManager::next_handle_id` restarts at
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
        // Same staleness pattern for `vimcode.picker.open` — a picker
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
        // Same staleness pattern for `vimcode.http.request` — an
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
        // Same staleness pattern for `vimcode.fs.walk`/`vimcode.fs.
        // grep` — a handle whose owning manager just went away is cancelled
        // (stopping the background walk promptly, same as an explicit
        // `:cancel()`) and dropped without calling into the dead Lua state.
        let dead_fs: Vec<u64> = self
            .plugin_fs_ops
            .iter()
            .filter(|(_, handle)| handle.manager.upgrade().is_none())
            .map(|(id, _)| *id)
            .collect();
        for id in dead_fs {
            if let Some(handle) = self.plugin_fs_ops.remove(&id) {
                handle
                    .cancelled
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    // ── Plugin-declared UI views ───────────────────────────────────────────

    /// Whether `name` names a `vimcode.ui.register_view` view.
    pub fn is_plugin_view(&self, name: &str) -> bool {
        self.plugin_views.contains_key(name)
    }

    /// The display title to use for a plugin view's tab/window title.
    ///
    /// Reads the view's own declared `title` via `PluginManager::view_title`,
    /// which every registered view has (tab-only `sidebar = false` views
    /// included, even though they have no `PanelRegistration` in
    /// `ext_panels`). The `ext_panels` lookup and the `name` fallback below
    /// are unreachable for a view that is actually registered — they exist
    /// only so a caller that somehow asks for an unregistered name gets a
    /// sane title instead of a panic.
    pub(crate) fn plugin_view_title(&self, name: &str) -> String {
        self.plugin_manager
            .as_ref()
            .and_then(|pm| pm.view_title(name))
            .map(str::to_string)
            .or_else(|| self.ext_panels.get(name).map(|p| p.title.clone()))
            .unwrap_or_else(|| name.to_string())
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
                    // field-stack views only — a body-kind
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
    /// re-runs its `render` callback so the form painted this frame is current.
    /// Both activation paths — the activity-bar click router in
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

    // ── In-panel text entry / editor-tab hosting ────────────────────────────

    /// Whether `name` names the view currently hosted as an editor-area tab
    /// in the *active* window, if any (`Engine::open_plugin_view_tab`).
    pub fn active_plugin_view_tab(&self) -> Option<String> {
        let buf = self.windows.get(&self.active_window_id())?.buffer_id;
        self.buffer_manager.get(buf)?.plugin_view.clone()
    }

    /// Open `name` (a `vimcode.ui.register_view` view) as a new tab in the
    /// active editor group, the way `vimcode.ui.open_view(name, {location =
    /// "tab"})` is wired.
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
        let title = self.plugin_view_title(name);
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

    /// Open `name` (a `vimcode.ui.register_view` view) as a new window
    /// split alongside the active window, the way `vimcode.ui.open_view(
    /// name, {location = "split"/"vsplit"})` is wired. `direction`/
    /// `new_first` map onto quadraui's split-tree the same way
    /// `Engine::split_window_with_new_first` does for an ordinary file
    /// split — via the same `Engine::split_active_window_for_buffer`
    /// helper, just handed a fresh scratch buffer instead of a file's.
    ///
    /// Shares every buffer-level and focus-level detail with
    /// `Engine::open_plugin_view_tab` (`BufferState::plugin_view`,
    /// `scratch_name`, `read_only`, deferred refresh,
    /// `PluginViewHost::Tab`): rendering and key/event routing both key off
    /// `BufferState::plugin_view` on the *active window's* buffer
    /// (`Engine::active_plugin_view_tab`), which doesn't care whether that
    /// window is a tab's sole window or a split sibling — only how the
    /// window was placed into the layout differs between the two entry
    /// points.
    pub fn open_plugin_view_split(
        &mut self,
        name: &str,
        direction: SplitDirection,
        new_first: bool,
    ) -> bool {
        if !self.is_plugin_view(name) {
            return false;
        }
        let title = self.plugin_view_title(name);
        let buf_id = self.buffer_manager.create();
        self.split_active_window_for_buffer(buf_id, direction, new_first);
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.read_only = true;
            state.dirty = false;
            state.scratch_name = Some(title);
            state.plugin_view = Some(name.to_string());
        }
        // See `open_plugin_view_tab`'s doc for why the refresh must be
        // deferred when called from inside an already-running plugin
        // callback.
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

    /// Open or re-focus `name` (a `vimcode.ui.register_view` view) as the
    /// bottom panel's docked content, the way `vimcode.ui.open_view(name,
    /// {location = "dock"})` is wired.
    ///
    /// Unlike `Engine::open_plugin_view_tab`/`open_plugin_view_split`, this
    /// never creates a new buffer/window/tab — it just re-points
    /// `Engine::bottom_panel_kind` at `BottomPanelKind::Plugin(name)` and
    /// opens the bottom panel, so calling this again for the same name is a
    /// no-op beyond the refresh: "reuse by name" falls out of there being
    /// only one dock slot, rather than needing any dedupe logic of its own.
    ///
    /// Only `Form`- and `TextView`-kind views paint anything useful in the
    /// dock today (`render::paint_bottom_panel_rung`'s `Plugin` arm) —
    /// `List`/`Tree`/`Table` kinds are accepted here (so a plugin can still
    /// dock one) but currently paint as an empty panel, matching
    /// `BottomPanelKind::Problems`/`Output`'s own "accepted, not yet wired"
    /// precedent.
    pub fn open_plugin_view_dock(&mut self, name: &str) -> bool {
        if !self.is_plugin_view(name) {
            return false;
        }
        self.bottom_panel_kind = BottomPanelKind::Plugin(name.to_string());
        self.bottom_panel_open = true;
        self.terminal_has_focus = false;
        // See `open_plugin_view_tab`'s doc for why the refresh must be
        // deferred when called from inside an already-running plugin
        // callback.
        if self.plugin_dispatch_depth == 0 {
            self.refresh_plugin_view(name);
        } else {
            self.plugin_view_tab_pending_refresh.push(name.to_string());
        }
        true
    }

    /// Open or re-focus `name` (a `vimcode.ui.register_view` view) as a
    /// centred modal over the editor, the way `vimcode.ui.open_view(name,
    /// {location = "modal"})` is wired.
    ///
    /// Like the dock there is exactly one modal slot, so "reuse by name"
    /// falls out of re-pointing [`Engine::plugin_view_modal`]: opening the
    /// same view twice never stacks a second dialog, and opening a different
    /// view replaces the current one. Unlike the dock, the modal takes
    /// keyboard focus — every other panel's focus is cleared and the first
    /// focusable field is selected, so a form is typeable the moment it
    /// appears. `Escape` (when not consumed by the focused field), a click
    /// outside the modal, or `vimcode.ui.close_view(name)` dismisses it
    /// ([`Self::dismiss_plugin_view_modal`]).
    ///
    /// Whoever owned the keyboard beforehand (a terminal, a focused sidebar
    /// panel, or the editor) is remembered in
    /// [`Engine::plugin_view_modal_prev_focus`] and handed it back on
    /// dismissal, so a modal opened over a focused panel doesn't silently
    /// move the user to the editor. Re-opening into an already-open modal
    /// keeps the *original* owner rather than recording "the modal".
    ///
    /// The modal paints `Form`- and `TextView`-kind views; other body kinds
    /// are accepted (so a plugin can still open one) but paint an empty
    /// modal, the same "accepted, not yet wired" precedent the dock follows.
    pub fn open_plugin_view_modal(&mut self, name: &str) -> bool {
        if !self.is_plugin_view(name) {
            return false;
        }
        if let Some(prev) = self.plugin_view_modal.clone() {
            self.blur_plugin_view_text_edit(&prev, PluginViewHost::Modal);
        } else {
            self.plugin_view_modal_prev_focus = Some(self.capture_focus_owner());
        }
        self.clear_sidebar_focus();
        self.terminal_has_focus = false;
        self.plugin_view_modal = Some(name.to_string());
        self.plugin_view_modal_scroll_top = 0;
        // See `open_plugin_view_tab`'s doc for why the refresh must be
        // deferred when called from inside an already-running plugin
        // callback.
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
        self.plugin_view_focus_field(name, PluginViewHost::Modal, first);
        true
    }

    /// Dismiss the plugin-view modal the way the *user* dismisses it —
    /// `Escape`, or a click on the scrim outside the dialog. The view's
    /// `on_event` sees a [`crate::core::plugin_ui::ViewEventKind::Dismissed`] event
    /// (`widget_id = ""`, since no field was activated), which is how a
    /// plugin tells "the user abandoned the dialog" from "nothing happened".
    ///
    /// A `TextArea` edit in progress commits on the way out (the same blur
    /// rule every host follows); a `Text`/`Password` edit is discarded.
    /// Focus returns to whoever held it when the modal opened. Returns
    /// whether a modal was open.
    ///
    /// [`Self::close_plugin_view`] uses [`Self::close_plugin_view_modal`]
    /// instead: a plugin that called `vimcode.ui.close_view` itself already
    /// knows the dialog is going away.
    pub fn dismiss_plugin_view_modal(&mut self) -> bool {
        let Some(name) = self.close_plugin_view_modal() else {
            return false;
        };
        self.dispatch_plugin_view_event(crate::core::plugin_ui::PluginViewEvent {
            view: name,
            widget_id: String::new(),
            kind: crate::core::plugin_ui::ViewEventKind::Dismissed,
        });
        true
    }

    /// Close the plugin-view modal without telling the view it was
    /// dismissed, returning the view it was showing (`None` when no modal
    /// was open). The mechanical half of [`Self::dismiss_plugin_view_modal`]
    /// — blur the in-progress edit, drop the slot, restore focus.
    pub(crate) fn close_plugin_view_modal(&mut self) -> Option<String> {
        let name = self.plugin_view_modal.take()?;
        // `blur_plugin_view_text_edit` reads the modal's selection, which
        // survives `take()` above — only the slot's name is cleared.
        self.blur_plugin_view_text_edit(&name, PluginViewHost::Modal);
        self.plugin_view_combo_edit = None;
        self.plugin_view_number_edit = None;
        if let Some(prev) = self.plugin_view_modal_prev_focus.take() {
            self.restore_focus_owner(prev);
        }
        Some(name)
    }

    /// Snapshot the keyboard-focus owners [`Self::open_plugin_view_modal`]
    /// is about to clear.
    fn capture_focus_owner(&self) -> PluginViewModalFocus {
        PluginViewModalFocus {
            terminal: self.terminal_has_focus,
            explorer: self.explorer_has_focus,
            search: self.search_has_focus,
            source_control: self.sc_has_focus,
            debug: self.dap_sidebar_has_focus,
            extensions: self.ext_sidebar_has_focus,
            ai: self.ai_has_focus,
            settings: self.settings_has_focus,
            settings_modal_open: self.settings_modal_open,
            ext_panel: self.ext_panel_has_focus,
            activity_bar: self.activity_bar_focused,
        }
    }

    /// Hand the keyboard back to whoever [`Self::capture_focus_owner`]
    /// recorded. An all-`false` snapshot (the editor had focus) restores
    /// nothing, which is exactly right: the modal already left every flag
    /// cleared.
    fn restore_focus_owner(&mut self, prev: PluginViewModalFocus) {
        self.terminal_has_focus = prev.terminal;
        self.explorer_has_focus = prev.explorer;
        // Assigned rather than routed through `search_set_focus`: that is a
        // focus-*grant* path, which re-seeds the search query from the
        // visual selection. Handing focus back must not rewrite what the
        // user had typed.
        self.search_has_focus = prev.search;
        // `sc_set_focus` keeps the engine flag and `sc_sidebar_system` in
        // sync; setting the flag alone would leave the panel painting
        // unfocused.
        self.sc_set_focus(prev.source_control);
        self.dap_sidebar_has_focus = prev.debug;
        self.ext_sidebar_has_focus = prev.extensions;
        self.ai_has_focus = prev.ai;
        self.settings_has_focus = prev.settings;
        self.settings_modal_open = prev.settings_modal_open;
        self.ext_panel_has_focus = prev.ext_panel;
        self.activity_bar_focused = prev.activity_bar;
        self.app_shell
            .set_activity_keyboard_focused(prev.activity_bar);
    }

    /// Route a key to the open plugin-view modal. Every key is consumed
    /// while a modal is open — it is modal — and an `Escape` the focused
    /// field doesn't claim for itself dismisses the modal. Returns `false`
    /// only when no modal is open.
    pub(crate) fn handle_plugin_view_modal_key(
        &mut self,
        key: &str,
        ctrl: bool,
        unicode: Option<char>,
    ) -> bool {
        let Some(name) = self.plugin_view_modal.clone() else {
            return false;
        };
        let claimed = self.handle_plugin_view_key(&name, key, ctrl, unicode, PluginViewHost::Modal);
        if !claimed && key == "Escape" {
            self.dismiss_plugin_view_modal();
        }
        true
    }

    /// Close every tab, split window, dock slot and modal showing `name`
    /// (a `vimcode.ui.register_view` view), the way
    /// `vimcode.ui.close_view(name)` is wired.
    ///
    /// `None` when `name` isn't a registered view at all — the Lua wrapper
    /// turns that into a loud error, matching `vimcode.ui.open_view`'s "fail
    /// loudly at the Lua/Rust boundary" convention. `Some(false)` when `name`
    /// is registered but not currently open as a tab, split, dock or modal — with
    /// one caveat: if `name`'s only remaining window is the editor's one
    /// remaining window overall, the "refuse to close the last window"
    /// guard below also reports `Some(false)`, even though the view is
    /// still open. A caller cannot tell the two apart from the return value
    /// alone; `vimcode.ui.close_view`'s doc in `EXTENSIONS.md` covers this.
    ///
    /// Each matching editor-area window is closed via `Engine::close_window`
    /// — the same path `<C-w>c`/`:q` use — so its "refuse to close the very
    /// last window in the very last tab of the very last group" guard
    /// applies here too: a plugin cannot use this to quit vimcode. Closing a
    /// window that is the sole window of its tab therefore closes the tab,
    /// matching `Engine::open_plugin_view_tab`'s doc ("closes like any
    /// tab"). That guard's own side effect of setting `self.message` is
    /// swallowed here — it names an engine-internal action the caller never
    /// asked for, and the boolean return already carries "didn't close".
    ///
    /// Picks windows to close in ascending `WindowId` order (rather than
    /// `HashMap` iteration order) so which window is closed — and, when the
    /// guard above stops the loop partway through several matches, which
    /// one survives — is reproducible rather than incidental to hashing.
    /// Restores the caller's own active window afterwards, provided that
    /// window wasn't itself one of the ones just closed — closing a
    /// background view must not leave the user's own active tab/group
    /// switched to wherever the last closed window happened to leave focus.
    pub fn close_plugin_view(&mut self, name: &str) -> Option<bool> {
        if !self.is_plugin_view(name) {
            return None;
        }
        let caller_active = self.active_window_id();
        let message_before_close = self.message.clone();
        let mut closed = false;
        if self.plugin_view_modal.as_deref() == Some(name) {
            // No `Dismissed` event: the plugin asked for this close itself.
            self.close_plugin_view_modal();
            closed = true;
        }
        if let BottomPanelKind::Plugin(dock_name) = &self.bottom_panel_kind {
            if self.bottom_panel_open && dock_name == name {
                self.bottom_panel_open = false;
                closed = true;
            }
        }
        // One window at a time, re-scanning after each close: `close_window`
        // mutates `self.windows` and the active tab's layout (and may remove
        // a whole tab), so holding an iterator across it would be invalid.
        loop {
            let windows = &self.windows;
            let buffer_manager = &self.buffer_manager;
            let mut matches: Vec<WindowId> = windows
                .iter()
                .filter(|(_, w)| {
                    buffer_manager
                        .get(w.buffer_id)
                        .is_some_and(|s| s.plugin_view.as_deref() == Some(name))
                })
                .map(|(&wid, _)| wid)
                .collect();
            matches.sort_by_key(|wid| wid.0);
            let Some(window_id) = matches.into_iter().next() else {
                break;
            };
            self.activate_window(window_id);
            if self.close_window() {
                closed = true;
            } else {
                // Only fails when this is the editor's one remaining window
                // overall — further attempts would just loop forever on the
                // same window. Undo the guard's "Cannot close last window"
                // message; it is an editor-internal detail the caller never
                // asked for, not something `vimcode.ui.close_view` should
                // surface as a side effect of its return value.
                self.message = message_before_close;
                break;
            }
        }
        if self.windows.contains_key(&caller_active) && self.active_window_id() != caller_active {
            self.activate_window(caller_active);
        }
        Some(closed)
    }

    /// Handle a scroll event on the bottom panel's docked plugin view
    /// (`"plugin_dock"`'s scroll surface). No-op when the bottom panel
    /// isn't currently showing a `BottomPanelKind::Plugin`, or that view
    /// isn't `TextView`-kind (`List`/`Tree`/`Table`/`Form` dock bodies don't
    /// scroll via this path yet).
    pub fn handle_plugin_view_dock_scroll(&mut self, delta_y: f32) {
        let BottomPanelKind::Plugin(name) = &self.bottom_panel_kind else {
            return;
        };
        let name = name.clone();
        let step = (delta_y.abs() * 3.0).ceil() as i32 * delta_y.signum() as i32;
        crate::render::scroll_plugin_view_text(self, &name, PluginViewHost::Dock, step);
    }

    /// Read the selection index for `host`.
    pub(crate) fn plugin_view_selected(&self, host: PluginViewHost) -> usize {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_selected,
            PluginViewHost::Tab => self.plugin_view_tab_selected,
            PluginViewHost::Dock => self.plugin_view_dock_selected,
            PluginViewHost::Modal => self.plugin_view_modal_selected,
        }
    }

    /// Write the selection index for `host`.
    pub(crate) fn set_plugin_view_selected(&mut self, host: PluginViewHost, idx: usize) {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_selected = idx,
            PluginViewHost::Tab => self.plugin_view_tab_selected = idx,
            PluginViewHost::Dock => self.plugin_view_dock_selected = idx,
            PluginViewHost::Modal => self.plugin_view_modal_selected = idx,
        }
    }

    /// Read the scroll offset for `host` (#1631 — `List`/`Table`/`TextView`
    /// body kinds reuse the same flat scroll state a field-stack view uses,
    /// rather than a parallel field per body kind).
    pub(crate) fn plugin_view_scroll_top(&self, host: PluginViewHost) -> usize {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_scroll_top,
            PluginViewHost::Tab => self.plugin_view_tab_scroll_top,
            PluginViewHost::Dock => self.plugin_view_dock_scroll_top,
            PluginViewHost::Modal => self.plugin_view_modal_scroll_top,
        }
    }

    /// Write the scroll offset for `host`.
    pub(crate) fn set_plugin_view_scroll_top(&mut self, host: PluginViewHost, offset: usize) {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_scroll_top = offset,
            PluginViewHost::Tab => self.plugin_view_tab_scroll_top = offset,
            PluginViewHost::Dock => self.plugin_view_dock_scroll_top = offset,
            PluginViewHost::Modal => self.plugin_view_modal_scroll_top = offset,
        }
    }

    /// Read the raw selected-column index for `host`'s `Table`-kind view.
    /// "Raw" — callers that need it clamped to an actually-
    /// editable column (`render::handle_plugin_view_table_key`'s
    /// `current_col`) do that themselves, the same "read-time clamp"
    /// contract `Engine::plugin_view_selected` uses for a stale row index.
    pub(crate) fn plugin_view_table_col(&self, host: PluginViewHost) -> usize {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_table_col,
            PluginViewHost::Tab => self.plugin_view_tab_table_col,
            PluginViewHost::Dock => self.plugin_view_dock_table_col,
            PluginViewHost::Modal => self.plugin_view_modal_table_col,
        }
    }

    /// Write the selected-column index for `host`'s `Table`-kind view.
    pub(crate) fn set_plugin_view_table_col(&mut self, host: PluginViewHost, col: usize) {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_table_col = col,
            PluginViewHost::Tab => self.plugin_view_tab_table_col = col,
            PluginViewHost::Dock => self.plugin_view_dock_table_col = col,
            PluginViewHost::Modal => self.plugin_view_modal_table_col = col,
        }
    }

    /// Read the sub-focus index within `host`'s currently-focused `Buttons`/
    /// `Toggles` row — see `Engine::ext_panel_row_focus`'s doc. Meaningless
    /// when the focused field isn't one of those two kinds; callers that
    /// care always check the field kind first.
    pub(crate) fn plugin_view_row_focus(&self, host: PluginViewHost) -> usize {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_row_focus,
            PluginViewHost::Tab => self.plugin_view_tab_row_focus,
            PluginViewHost::Dock => self.plugin_view_dock_row_focus,
            PluginViewHost::Modal => self.plugin_view_modal_row_focus,
        }
    }

    /// Write the sub-focus index within `host`'s currently-focused `Buttons`/
    /// `Toggles` row.
    pub(crate) fn set_plugin_view_row_focus(&mut self, host: PluginViewHost, idx: usize) {
        match host {
            PluginViewHost::Sidebar => self.ext_panel_row_focus = idx,
            PluginViewHost::Tab => self.plugin_view_tab_row_focus = idx,
            PluginViewHost::Dock => self.plugin_view_dock_row_focus = idx,
            PluginViewHost::Modal => self.plugin_view_modal_row_focus = idx,
        }
    }

    /// Nudge `host`'s scroll offset so its selection stays visible — the
    /// tab twin of `Engine::ext_panel_ensure_visible` (fixed 20-row viewport
    /// guess, same as that method's own fallback).
    ///
    /// `pub(crate)` (not just used from this module) since `render::
    /// navigate_flat_selection` — the `List`/`Table` body-kind keyboard
    /// navigation — calls this after every selection move, the same
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
            PluginViewHost::Dock => {
                const ROWS: usize = 20;
                let sel = self.plugin_view_dock_selected;
                if sel < self.plugin_view_dock_scroll_top {
                    self.plugin_view_dock_scroll_top = sel;
                } else if sel >= self.plugin_view_dock_scroll_top + ROWS {
                    self.plugin_view_dock_scroll_top = sel.saturating_sub(ROWS - 1);
                }
            }
            PluginViewHost::Modal => {
                const ROWS: usize = 20;
                let sel = self.plugin_view_modal_selected;
                if sel < self.plugin_view_modal_scroll_top {
                    self.plugin_view_modal_scroll_top = sel;
                } else if sel >= self.plugin_view_modal_scroll_top + ROWS {
                    self.plugin_view_modal_scroll_top = sel.saturating_sub(ROWS - 1);
                }
            }
        }
    }

    /// Move `host`'s keyboard focus to field `idx` of view `name`: blurs
    /// whatever text edit was in progress on the previously-focused field
    /// (committing it first if it was a `TextArea`, discarding otherwise —
    /// see [`Self::blur_plugin_view_text_edit`]), discards whatever
    /// `combo_box` edit was in progress *for a field other than `idx`*
    /// (same "discard, don't commit" contract as `Text`/`Password` — there
    /// is no equivalent of `TextArea`'s blur-commits-the-buffer carve-out
    /// here), moves the selection, and primes [`Engine::plugin_view_text_edit`]
    /// if the newly-focused field is a `Text`/`Password`/`TextArea` kind.
    ///
    /// The single entry point for changing which field a plugin view's
    /// keyboard focus is on — every navigation path (initial panel/tab focus,
    /// `j`/`k`/`Tab`/`BackTab`, a mouse click) routes through this so blur/
    /// prime can never be skipped on one path and not another. That includes
    /// `render::handle_plugin_view_ui_event`'s own re-assertion of the
    /// *already*-focused field after every mouse click or forwarded key (so
    /// selection follows a click) — a `combo_box` key is itself forwarded
    /// through that same function (see
    /// `Engine::handle_plugin_view_key`'s combo-box branch), so this must
    /// not discard the edit state a key's own handling just wrote when
    /// `idx` still names the field that state belongs to.
    pub(crate) fn plugin_view_focus_field(&mut self, name: &str, host: PluginViewHost, idx: usize) {
        self.blur_plugin_view_text_edit(name, host);
        let still_focused = self.plugin_view_combo_edit.as_ref().is_some_and(|state| {
            self.plugin_views
                .get(name)
                .and_then(|v| v.fields.get(idx))
                .is_some_and(|f| state.view == name && state.field_id == f.id)
        });
        if !still_focused {
            self.plugin_view_combo_edit = None;
        }
        let still_focused_number = self.plugin_view_number_edit.as_ref().is_some_and(|state| {
            self.plugin_views
                .get(name)
                .and_then(|v| v.fields.get(idx))
                .is_some_and(|f| state.view == name && state.field_id == f.id)
        });
        if !still_focused_number {
            self.plugin_view_number_edit = None;
        }
        self.set_plugin_view_selected(host, idx);
        self.prime_plugin_view_text_edit(name, idx);
        // Reseed the row sub-focus (see `Engine::ext_panel_row_focus`'s doc)
        // to the newly-focused field's first enabled item every time focus
        // moves — including re-entering the same `Buttons`/`Toggles` row —
        // so a stale position from a different field, or from this same
        // row's last visit, never carries over.
        let first_enabled = self
            .plugin_views
            .get(name)
            .and_then(|v| v.fields.get(idx))
            .and_then(|f| plugin_view_row_item_indices(&f.kind))
            .and_then(|indices| indices.first().copied())
            .unwrap_or(0);
        self.set_plugin_view_row_focus(host, first_enabled);
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
    /// `disabled`), so `j`/`k`/`Tab`/`BackTab`/`ISO_Left_Tab` never park the
    /// selection somewhere `Enter` does nothing.
    ///
    /// When the focused row is a `Text`/`Password`/`TextArea` field, every key
    /// except `Tab`/`BackTab`/`ISO_Left_Tab` (field-to-field navigation always
    /// wins, so a view is never a keyboard trap) is handed to
    /// [`Self::handle_plugin_view_text_key`] instead — typed characters must
    /// insert into the field rather than double as `j`/`k` navigation once a
    /// field can actually take text.
    ///
    /// When the focused row is a `Date` field, `Left`/`Right`/`Up`/`Down`/
    /// `PageUp`/`PageDown`/`Enter` are forwarded to the shared
    /// `quadraui::FormController` as a `KeyPressed` event — the same
    /// controller a mouse click on the field already routes through (see
    /// `render::handle_plugin_view_ui_event`) — because `quadraui::DatePicker`
    /// only exposes keyboard scrub-and-confirm navigation; there is no
    /// click-driven day selection to fall back to. `j`/`k`/`g`/`G` keep their
    /// ordinary field-to-field meaning so vim navigation is never shadowed.
    ///
    /// When the focused row is a `combo_box` field, every key the embedded
    /// `quadraui::ComboBox` recognises — typed characters, `Backspace`,
    /// `Up`/`Down`, `Enter`, `Escape` — is forwarded the same way, through
    /// [`combo_box_key_from`]: `j`/`k`/`g`/`G` are *not* preserved as
    /// navigation here (unlike `Date`'s), since this field is free text and
    /// those letters must be insertable.
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

        // A `ViewBody`-kind view routes through its own kind's
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
                "message_list" => {
                    crate::render::handle_plugin_view_message_list_key(self, name, host, key, 10)
                }
                // `Image` is display-only — no keyboard handling of its own.
                _ => false,
            };
        }

        let focusable: Vec<usize> = view
            .visible_field_indices()
            .into_iter()
            .filter(|&i| view.fields[i].is_interactive() && !view.fields[i].disabled)
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
        let is_date_field = view
            .fields
            .get(selected)
            .is_some_and(|f| !f.disabled && matches!(f.kind, ViewFieldKind::Date { .. }));
        let is_combo_box_field = view
            .fields
            .get(selected)
            .is_some_and(|f| !f.disabled && matches!(f.kind, ViewFieldKind::ComboBox { .. }));
        let is_number_input_field = view
            .fields
            .get(selected)
            .is_some_and(|f| !f.disabled && matches!(f.kind, ViewFieldKind::NumberInput { .. }));
        let is_radio_group_field = view
            .fields
            .get(selected)
            .is_some_and(|f| !f.disabled && matches!(f.kind, ViewFieldKind::RadioGroup { .. }));
        let is_button_row_field = view.fields.get(selected).is_some_and(|f| {
            !f.disabled
                && matches!(
                    f.kind,
                    ViewFieldKind::Buttons { .. } | ViewFieldKind::Toggles { .. }
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
            // `"ISO_Left_Tab"` is what `render::engine_key_from_ui` actually
            // emits for Shift+Tab on both backends (`NamedKey::BackTab` ->
            // `"ISO_Left_Tab"`); `"BackTab"` is kept so a caller that passes
            // the raw named-key spelling still works.
            "BackTab" | "ISO_Left_Tab" => {
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

        if is_date_field {
            let named = match key {
                "Left" => Some(quadraui::NamedKey::Left),
                "Right" => Some(quadraui::NamedKey::Right),
                "Up" => Some(quadraui::NamedKey::Up),
                "Down" => Some(quadraui::NamedKey::Down),
                "PageUp" | "Page_Up" => Some(quadraui::NamedKey::PageUp),
                "PageDown" | "Page_Down" => Some(quadraui::NamedKey::PageDown),
                "Enter" | "Return" => Some(quadraui::NamedKey::Enter),
                _ => None,
            };
            if let Some(named) = named {
                let event = quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(named),
                    modifiers: quadraui::Modifiers {
                        ctrl,
                        ..Default::default()
                    },
                    repeat: false,
                };
                return match host {
                    PluginViewHost::Sidebar => {
                        let rect = self.plugin_view_form_rect.get();
                        crate::render::handle_plugin_view_ui_event(self, &event, rect)
                    }
                    PluginViewHost::Tab => {
                        let rect = self.plugin_view_tab_form_rect.get();
                        crate::render::handle_plugin_view_tab_ui_event(self, name, &event, rect)
                    }
                    PluginViewHost::Modal => {
                        let rect = self.plugin_view_modal_form_rect.get();
                        crate::render::handle_plugin_view_modal_ui_event(self, name, &event, rect)
                    }
                    // The dock never holds keyboard focus, so a
                    // Date-field key never actually reaches this arm in
                    // practice; it exists only to keep the match exhaustive.
                    PluginViewHost::Dock => false,
                };
            }
        }

        // `combo_box`: forward every key the embedded `quadraui::ComboBox`
        // recognises (typed characters, `Backspace`, `Up`/`Down`, `Enter`,
        // `Escape`) to the shared `quadraui::FormController`, which drives
        // its filtering/highlight/commit/cancel logic directly — see
        // `combo_box_key_from`'s doc for why a key it has no path for at
        // all (`Home`/`End`/`Delete`/`Left`/`Right`/function keys) instead
        // falls through to the generic dispatch below, same as an
        // unclaimed key on any other field kind.
        if is_combo_box_field {
            if let Some(quadraui_key) = combo_box_key_from(key, unicode) {
                let event = quadraui::UiEvent::KeyPressed {
                    key: quadraui_key,
                    modifiers: quadraui::Modifiers {
                        ctrl,
                        ..Default::default()
                    },
                    repeat: false,
                };
                return match host {
                    PluginViewHost::Sidebar => {
                        let rect = self.plugin_view_form_rect.get();
                        crate::render::handle_plugin_view_ui_event(self, &event, rect)
                    }
                    PluginViewHost::Tab => {
                        let rect = self.plugin_view_tab_form_rect.get();
                        crate::render::handle_plugin_view_tab_ui_event(self, name, &event, rect)
                    }
                    PluginViewHost::Modal => {
                        let rect = self.plugin_view_modal_form_rect.get();
                        crate::render::handle_plugin_view_modal_ui_event(self, name, &event, rect)
                    }
                    // The dock never holds keyboard focus (see the `Date`
                    // arm's identical comment above), so a `combo_box` key
                    // never actually reaches this arm in practice.
                    PluginViewHost::Dock => false,
                };
            }
        }

        // `number_input`: forward every key the embedded
        // `quadraui::NumberInput` recognises (digits/`-`/`.`, `Backspace`,
        // `Up`/`Down`, `Enter`, `Escape`) to the shared
        // `quadraui::FormController`, which drives its draft/step/commit/
        // cancel logic directly — see `number_input_key_from`'s doc for why
        // a key it has no path for at all (letters, `Home`/`End`/`Left`/
        // `Right`/function keys) instead falls through to the generic
        // dispatch below, same as an unclaimed key on any other field kind.
        if is_number_input_field {
            if let Some(quadraui_key) = number_input_key_from(key, unicode) {
                let event = quadraui::UiEvent::KeyPressed {
                    key: quadraui_key,
                    modifiers: quadraui::Modifiers {
                        ctrl,
                        ..Default::default()
                    },
                    repeat: false,
                };
                return match host {
                    PluginViewHost::Sidebar => {
                        let rect = self.plugin_view_form_rect.get();
                        crate::render::handle_plugin_view_ui_event(self, &event, rect)
                    }
                    PluginViewHost::Tab => {
                        let rect = self.plugin_view_tab_form_rect.get();
                        crate::render::handle_plugin_view_tab_ui_event(self, name, &event, rect)
                    }
                    PluginViewHost::Modal => {
                        let rect = self.plugin_view_modal_form_rect.get();
                        crate::render::handle_plugin_view_modal_ui_event(self, name, &event, rect)
                    }
                    // The dock never holds keyboard focus (see the `Date`
                    // arm's identical comment above), so a `number_input`
                    // key never actually reaches this arm in practice.
                    PluginViewHost::Dock => false,
                };
            }
        }

        // `radio_group`: forward `Up`/`Left`/`Down`/`Right` to the shared
        // `quadraui::FormController`, which drives the embedded
        // `RadioGroup`'s own cursor movement (wrapping, skipping disabled
        // options) and fires `FormEvent::RadioChanged` immediately — no
        // separate confirm step, so there is nothing for `Enter`/`Space` to
        // do on this field kind (unlike `Dropdown`/`Segmented`'s own
        // Enter/Space-cycles-the-value handling below). Every other key
        // falls through to the generic dispatch, same as an unclaimed key
        // on any other field kind.
        if is_radio_group_field {
            let named = match key {
                "Left" => Some(quadraui::NamedKey::Left),
                "Right" => Some(quadraui::NamedKey::Right),
                "Up" => Some(quadraui::NamedKey::Up),
                "Down" => Some(quadraui::NamedKey::Down),
                _ => None,
            };
            if let Some(named) = named {
                let event = quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(named),
                    modifiers: quadraui::Modifiers {
                        ctrl,
                        ..Default::default()
                    },
                    repeat: false,
                };
                return match host {
                    PluginViewHost::Sidebar => {
                        let rect = self.plugin_view_form_rect.get();
                        crate::render::handle_plugin_view_ui_event(self, &event, rect)
                    }
                    PluginViewHost::Tab => {
                        let rect = self.plugin_view_tab_form_rect.get();
                        crate::render::handle_plugin_view_tab_ui_event(self, name, &event, rect)
                    }
                    PluginViewHost::Modal => {
                        let rect = self.plugin_view_modal_form_rect.get();
                        crate::render::handle_plugin_view_modal_ui_event(self, name, &event, rect)
                    }
                    // The dock never holds keyboard focus (see the `Date`
                    // arm's identical comment above), so a `radio_group`
                    // key never actually reaches this arm in practice.
                    PluginViewHost::Dock => false,
                };
            }
        }

        if is_text_field {
            return self.handle_plugin_view_text_key(name, host, selected, key, ctrl, unicode);
        }

        // `buttons`/`toggles`: `Left`/`Right` (and vim's `h`/`l`) move the
        // sub-focus within the row, skipping disabled items, never wrapping
        // past either end — see `Engine::ext_panel_row_focus`'s doc for how
        // it's seeded/reset. `Enter`/`Space`'s own arm below reads this same
        // index instead of always resolving to the row's first item.
        if is_button_row_field {
            let step: i32 = match key {
                "Left" | "h" => -1,
                "Right" | "l" => 1,
                _ => 0,
            };
            if step != 0 {
                let indices = self
                    .plugin_views
                    .get(name)
                    .and_then(|v| v.fields.get(selected))
                    .and_then(|f| plugin_view_row_item_indices(&f.kind))
                    .unwrap_or_default();
                if !indices.is_empty() {
                    let current = self.plugin_view_row_focus(host);
                    let pos = indices.iter().position(|&i| i == current).unwrap_or(0);
                    let next_pos = if step < 0 {
                        pos.saturating_sub(1)
                    } else {
                        (pos + 1).min(indices.len() - 1)
                    };
                    self.set_plugin_view_row_focus(host, indices[next_pos]);
                }
                return true;
            }
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
                    PluginViewHost::Dock => self.plugin_view_dock_scroll_top = 0,
                    PluginViewHost::Modal => self.plugin_view_modal_scroll_top = 0,
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
                    ViewFieldKind::Button { .. } => {
                        (field.id.clone(), ViewEventKind::ButtonClicked)
                    }
                    ViewFieldKind::Toggle { value } => (
                        field.id.clone(),
                        ViewEventKind::ToggleChanged { value: !*value },
                    ),
                    ViewFieldKind::Toggles { toggles } => {
                        // `ext_panel_row_focus`'s sub-focus index, when it
                        // names a toggle that still exists; the row's first
                        // one otherwise (freshly focused, or a stale index
                        // from a `render` that shrank the list without a
                        // refocus).
                        let current = self.plugin_view_row_focus(host);
                        match toggles.get(current).or_else(|| toggles.first()) {
                            Some(t) => (
                                t.id.clone(),
                                ViewEventKind::ToggleChanged { value: !t.value },
                            ),
                            None => return true,
                        }
                    }
                    ViewFieldKind::Buttons { buttons } => {
                        // Same sub-focus contract as `Toggles` above, but
                        // additionally skipping over a disabled button —
                        // `Engine::plugin_view_focus_field` never seeds the
                        // index onto one, but a `render` can disable the
                        // currently sub-focused button without moving
                        // field-level focus off the row.
                        let current = self.plugin_view_row_focus(host);
                        let chosen = buttons
                            .get(current)
                            .filter(|b| !b.disabled)
                            .or_else(|| buttons.iter().find(|b| !b.disabled));
                        match chosen {
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
                    ViewFieldKind::ComboBox { .. } => {
                        // Unreachable: `is_combo_box_field` routes every key
                        // for a focused `combo_box` field to the shared
                        // `quadraui::FormController` above, including
                        // `Enter`/`Space` (`Space` is just `Key::Char(' ')`
                        // to the embedded `ComboBox`, appended to the filter
                        // text like any other character).
                        return true;
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
                    ViewFieldKind::Date { .. } => {
                        // Reached only for `Space`, which is not one of the
                        // `DatePicker`'s own navigation keys: Enter is already
                        // intercepted above (see `is_date_field`) and forwarded to
                        // the `FormController` to confirm the scrubbed day.
                        return true;
                    }
                    ViewFieldKind::Slider {
                        value,
                        min,
                        max,
                        step,
                    } => {
                        // `quadraui::FormController` has no native keyboard path
                        // for a slider (unlike `Date`'s scrub-and-confirm), so
                        // Enter/Space steps and reaches the plugin directly —
                        // there is no separate confirm step.
                        let next = (*value + *step).min(*max).max(*min);
                        (
                            field.id.clone(),
                            ViewEventKind::SliderChanged { value: next },
                        )
                    }
                    ViewFieldKind::Color { .. } => {
                        // No native click or keyboard path exists anywhere
                        // for `ColorPicker` (see `ViewFieldKind::Color`'s
                        // doc) — display-only until a popup/hex editor is
                        // designed.
                        return true;
                    }
                    ViewFieldKind::NumberInput { .. } => {
                        // Reached only for `Space`, which is not one of
                        // `NumberInput::handle_key`'s own claimed keys
                        // (it is not an ASCII digit/`-`/`.`): every other
                        // key is intercepted above (see
                        // `is_number_input_field`) and forwarded to the
                        // `FormController`, including `Enter`/`Up`/`Down`.
                        return true;
                    }
                    ViewFieldKind::RadioGroup { .. } => {
                        // `RadioGroup::handle_key` claims only `Up`/`Down`/
                        // `Left`/`Right` (see `is_radio_group_field` above),
                        // which already select-and-fire with no separate
                        // confirm step, so `Enter`/`Space` have nothing
                        // left to do on this field kind.
                        return true;
                    }
                    ViewFieldKind::Toolbar { buttons } => {
                        match buttons.iter().find(|b| !b.disabled) {
                            Some(b) => (b.id.clone(), ViewEventKind::ToolbarButtonClicked),
                            None => return true,
                        }
                    }
                    ViewFieldKind::Section {
                        collapsible,
                        collapsed,
                    } => {
                        if !*collapsible {
                            return true;
                        }
                        (
                            field.id.clone(),
                            ViewEventKind::SectionToggled {
                                collapsed: !*collapsed,
                            },
                        )
                    }
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
    /// vimcode-owned. Mirrors `Engine::handle_explorer_rename_key`'s
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
                        v.visible_field_indices()
                            .into_iter()
                            .filter(|&i| v.fields[i].is_interactive() && !v.fields[i].disabled)
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
        // Now that depth is genuinely back to `0`, it's safe to run
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
                    if self.plugin_view_modal.as_deref() == Some(name.as_str()) {
                        self.plugin_view_focus_field(&name, PluginViewHost::Modal, first);
                    } else if self.active_plugin_view_tab().as_deref() == Some(name.as_str()) {
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
        // `set_lines` is a line-range splice — a delete of `[s, e)`
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
            // BufLeave for the window's previous buffer, delivering
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

    /// Make `win` the focused window, switching `active_group` and the
    /// owning tab's `active_tab`/`active_window` as needed so the rest of
    /// the engine (status line, LSP-active-buffer tracking, tab-bar scroll)
    /// is consistent afterwards — the cross-group twin of
    /// `activate_group_for_window` plus `goto_tab`'s active-tab bookkeeping,
    /// driven by `find_window_tab` rather than a linear tab-index. Returns
    /// false if `win` is invalid.
    pub(crate) fn plugin_api_win_focus(&mut self, win: i64) -> bool {
        let Some(id) = self.plugin_api_resolve_win(win) else {
            return false;
        };
        let Some((group_id, tab_idx)) = self.find_window_tab(id) else {
            return false;
        };
        self.active_group = group_id;
        if let Some(group) = self.editor_groups.get_mut(&group_id) {
            group.active_tab = tab_idx;
        }
        self.active_tab_mut().focus_window(id);
        self.lsp_ensure_active_buffer();
        self.ensure_active_tab_visible();
        true
    }

    /// Split `win`, giving the new window `direction`'s sibling slot
    /// (`new_first` picks which side) and leaving the new window focused.
    /// Returns the new window's handle, or `None` if `win` is invalid.
    pub(crate) fn plugin_api_win_split(
        &mut self,
        win: i64,
        direction: SplitDirection,
        new_first: bool,
    ) -> Option<i64> {
        if !self.plugin_api_win_focus(win) {
            return None;
        }
        self.split_window_with_new_first(direction, None, new_first);
        Some(self.active_window_id().0 as i64)
    }

    /// Close `win`. Returns false if `win` is invalid or it is the last
    /// window in the editor, matching `Engine::close_window`'s own
    /// "can't close the last window" refusal.
    pub(crate) fn plugin_api_win_close(&mut self, win: i64) -> bool {
        if !self.plugin_api_win_focus(win) {
            return false;
        }
        self.close_window()
    }

    /// Move `win` into a brand-new editor group placed at `direction`'s
    /// edge of the group layout (`forward` = after/below/right of the
    /// existing layout, else before/above/left of it) — the plugin-facing
    /// twin of the `<C-w>H`/`<C-w>J`/`<C-w>K`/`<C-w>L` drag-to-edge
    /// gesture, which drives the same `Engine::move_window_to_edge`.
    /// `move_window_to_edge` reports success only by whether it grew the
    /// group count, since it has no `bool` return of its own. Returns false
    /// if `win` is invalid or it was the last window overall (nothing left
    /// to move).
    pub(crate) fn plugin_api_win_move_to_group(
        &mut self,
        win: i64,
        direction: SplitDirection,
        forward: bool,
    ) -> bool {
        if !self.plugin_api_win_focus(win) {
            return false;
        }
        let groups_before = self.group_layout.group_ids().len();
        self.move_window_to_edge(direction, forward);
        self.group_layout.group_ids().len() > groups_before
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

    /// `vimcode.decor.clear(buf, ns, start?, end?)` — `start`/`end` are
    /// 0-indexed buffer rows with an exclusive `end`, same convention as
    /// `vimcode.buffer.get_lines`/`set_lines`. `None` clears every mark in
    /// `ns` regardless of position. `(buf, ns, ...)` matches the argument
    /// order every other `vimcode.decor.*` mark function uses
    /// (`set_mark`/`get_mark`/`del_mark`) — an earlier `(ns, buf, ...)` here
    /// was the one function in the surface that transposed it (#1653
    /// review).
    pub(crate) fn plugin_api_decor_clear(
        &mut self,
        buf: i64,
        ns: i64,
        range: Option<(usize, usize)>,
    ) -> bool {
        let Some(buf_id) = self.plugin_api_resolve_buf(buf) else {
            return false;
        };
        let ns_id = crate::core::buffer::NamespaceId(ns as u32);
        self.decor.clear(buf_id, ns_id, range);
        true
    }

    // =========================================================================
    // Native API P6 — read-only syntax tree / undo tree / diagnostics.
    // Thin buffer-handle-resolution wrappers; the actual tree-sitter work
    // lives on `crate::core::syntax::Syntax` (`node_at`/`query_captures`),
    // the undo-tree work on `BufferState`/`UndoTree` (`buffer_manager.rs`),
    // and diagnostics read straight off `Engine::lsp_diagnostics`.
    // =========================================================================

    /// `vimcode.syntax.node_at(buf, row, col)`. `Err` surfaces as a Lua
    /// error (invalid buffer, no parser, or out-of-range position) — a
    /// plugin calling this on a buffer with no tree-sitter grammar gets a
    /// clear message rather than a silent `nil`.
    pub(crate) fn plugin_api_syntax_node_at(
        &self,
        buf: i64,
        row: usize,
        col: usize,
    ) -> Result<crate::core::syntax::SyntaxNodeInfo, String> {
        let buf_id = self
            .plugin_api_resolve_buf(buf)
            .ok_or("vimcode.syntax.node_at: invalid buffer")?;
        let state = self
            .buffer_manager
            .get(buf_id)
            .ok_or("vimcode.syntax.node_at: invalid buffer")?;
        let syn = state
            .syntax
            .as_ref()
            .ok_or("vimcode.syntax.node_at: buffer has no parser for its language")?;
        syn.node_at(row, col)
    }

    /// `vimcode.syntax.query(buf, query_string, range?)` → `(language,
    /// captures)`. See [`Self::plugin_api_syntax_node_at`] for the error
    /// cases.
    pub(crate) fn plugin_api_syntax_query(
        &self,
        buf: i64,
        query_source: &str,
        range: Option<(usize, usize)>,
    ) -> Result<(String, Vec<crate::core::syntax::SyntaxCaptureInfo>), String> {
        let buf_id = self
            .plugin_api_resolve_buf(buf)
            .ok_or("vimcode.syntax.query: invalid buffer")?;
        let state = self
            .buffer_manager
            .get(buf_id)
            .ok_or("vimcode.syntax.query: invalid buffer")?;
        let syn = state
            .syntax
            .as_ref()
            .ok_or("vimcode.syntax.query: buffer has no parser for its language")?;
        let text = state.buffer.to_string();
        let captures = syn.query_captures(&text, query_source, range)?;
        Ok((syn.language_id().to_string(), captures))
    }

    /// `vimcode.undo.tree(buf)` — every live undo-tree node for `buf`, in
    /// `seq` order. Empty for an invalid buffer.
    pub(crate) fn plugin_api_undo_tree(
        &self,
        buf: i64,
    ) -> Vec<crate::core::buffer_manager::UndoTreeNode> {
        let Some(buf_id) = self.plugin_api_resolve_buf(buf) else {
            return Vec::new();
        };
        let Some(state) = self.buffer_manager.get(buf_id) else {
            return Vec::new();
        };
        state.undo_tree_for_plugin()
    }

    /// `vimcode.undo.jump(buf, seq)` → bool. The one mutation this read API
    /// ships — see [`crate::core::buffer_manager::UndoTree::jump_to_seq`].
    /// Mirrors the side effects `Engine::undo`/`redo` (`buffers.rs`) apply
    /// for the active buffer, generalised to an arbitrary buffer handle:
    /// every window currently showing `buf` gets the jumped-to cursor,
    /// decoration marks shift across the full-text swap, and the buffer's
    /// dirty/LSP-resync flags are refreshed.
    pub(crate) fn plugin_api_undo_jump(&mut self, buf: i64, seq: usize) -> bool {
        let Some(buf_id) = self.plugin_api_resolve_buf(buf) else {
            return false;
        };
        let Some(state) = self.buffer_manager.get(buf_id) else {
            return false;
        };
        let old_text = state.buffer.to_string();
        let Some(state) = self.buffer_manager.get_mut(buf_id) else {
            return false;
        };
        let Some(cursor) = state.undo_jump(seq) else {
            return false;
        };
        state.dirty = !state.is_at_saved_state();
        let new_text = state.buffer.to_string();
        self.decor
            .shift_for_text_replace(buf_id, &old_text, &new_text);
        self.lsp_dirty_buffers.insert(buf_id, true);
        for win in self.windows.values_mut() {
            if win.buffer_id == buf_id {
                win.view.cursor = cursor;
            }
        }
        self.clamp_cursors_to_buffer(buf_id);
        true
    }

    /// `vimcode.diagnostics.get(buf?)` — diagnostics for `buf`, or the
    /// active buffer when `buf` is omitted (matching this API's other
    /// `handle.unwrap_or(0)` = "current" convention). Empty (not an error)
    /// for an invalid buffer or one with no path to key `lsp_diagnostics`
    /// by — a plugin polling a scratch buffer shouldn't have to special-case
    /// it.
    pub(crate) fn plugin_api_diagnostics_get(&self, buf: Option<i64>) -> Vec<Diagnostic> {
        let Some(buf_id) = self.plugin_api_resolve_buf(buf.unwrap_or(0)) else {
            return Vec::new();
        };
        let Some(path) = self.plugin_api_diagnostics_key(buf_id) else {
            return Vec::new();
        };
        self.lsp_diagnostics.get(&path).cloned().unwrap_or_default()
    }

    /// The key `Engine::lsp_diagnostics` is stored under for `buf` — the
    /// canonical path if one has been resolved, else the raw `file_path`.
    /// Generalises `active_buffer_diagnostics_key` (`ext_panel.rs`) to an
    /// arbitrary buffer handle.
    fn plugin_api_diagnostics_key(&self, buf: BufferId) -> Option<PathBuf> {
        let state = self.buffer_manager.get(buf)?;
        state
            .canonical_path
            .clone()
            .or_else(|| state.file_path.clone())
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
                // An extension can become incompatible with the
                // running vimcode *after* it was installed — most commonly
                // a vimcode downgrade, or `:ExtUpdate` pulling a newer
                // manifest that raises `requires_vimcode`. Resolve
                // manifests once up front so the per-directory loop below
                // can gate on them; LSP/DAP config (wired separately via
                // `ext_installed_manifests`) is unaffected by this skip —
                // only the Lua scripts are gated here.
                let manifests = self.ext_available_manifests();
                let mut skipped_incompatible: Vec<String> = Vec::new();
                // Rebuilt fresh on every `plugin_init()` call (a reload after
                // install/uninstall/enable/disable included) — stale entries
                // for an extension that is no longer loaded must not survive.
                self.ext_plugin_names.clear();
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
                            // Skip this extension's scripts (not its
                            // LSP/DAP config) when its `requires_vimcode`
                            // constraint is no longer met by the running
                            // vimcode, instead of letting a script error out
                            // at load time on a missing `vimcode.*` API.
                            if let Some(manifest) = manifests
                                .iter()
                                .find(|m| m.name.eq_ignore_ascii_case(&ext_name))
                            {
                                if let Some(reason) =
                                    manifest.incompatibility_reason_for_running_vimcode()
                                {
                                    skipped_incompatible
                                        .push(format!("{} ({reason})", manifest.display_or_name()));
                                    continue;
                                }
                            }
                            // Capture exactly the plugin (file-stem) names
                            // this extension's own directory just contributed
                            // to `mgr.plugins` — not `manifest.scripts`,
                            // which only names what the registry *declared*
                            // and can drift from what is actually on disk.
                            let before = mgr.plugins.len();
                            mgr.load_plugins_dir(&ext_dir, &self.settings.disabled_plugins);
                            let loaded: Vec<String> = mgr.plugins[before..]
                                .iter()
                                .map(|p| p.name.clone())
                                .collect();
                            self.ext_plugin_names.insert(ext_name, loaded);
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
                // Surface which installed extensions were skipped for
                // being incompatible with the running vimcode, naming each
                // extension and its required version — after `VimEnter` so a
                // plugin's own startup message doesn't clobber this one.
                if !skipped_incompatible.is_empty() {
                    self.message = format!(
                        "Skipped incompatible extension{}: {}",
                        if skipped_incompatible.len() == 1 {
                            ""
                        } else {
                            "s"
                        },
                        skipped_incompatible.join(", ")
                    );
                }
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
        // `is_linewise` bool; flatten at the boundary.
        let registers_snapshot = self
            .registers
            .iter()
            .map(|(k, (text, ty))| (*k, (text.clone(), ty.is_linewise())))
            .collect();

        // Marks snapshot for the active buffer (1-indexed). `<`/`>` (the
        // visual-selection marks) live outside `self.marks` since they apply
        // across buffers rather than per-buffer letter marks, so they're
        // merged in here from `visual_mark_start`/`visual_mark_end` — falling
        // back to the live selection the same way `ex_mark_line` does, so a
        // callback invoked while Visual mode is still active (no `'<`/`'>`
        // recorded yet) sees the same selection `:'<,'>` would resolve, and
        // clamping the line in case the buffer shrank since the selection
        // was made.
        let last_line_idx = self.buffer().len_lines().saturating_sub(1);
        let mut marks_snapshot: HashMap<char, (usize, usize)> = self
            .marks
            .get(&buf_id)
            .map(|m| {
                m.iter()
                    .map(|(&ch, c)| (ch, (c.line + 1, c.col + 1)))
                    .collect()
            })
            .unwrap_or_default();
        let visual_lt = self.visual_mark_start.or_else(|| {
            self.get_visual_selection_range()
                .map(|(s, _)| (s.line, s.col))
        });
        let visual_gt = self.visual_mark_end.or_else(|| {
            self.get_visual_selection_range()
                .map(|(_, e)| (e.line, e.col))
        });
        if let Some((line, col)) = visual_lt {
            marks_snapshot.insert('<', (line.min(last_line_idx) + 1, col + 1));
        }
        if let Some((line, col)) = visual_gt {
            marks_snapshot.insert('>', (line.min(last_line_idx) + 1, col + 1));
        }

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
        // `vimcode.loop.spawn` uses — see `execute::AsyncShellTask`
        // and `Self::poll_async_shells` for how the streamed events are
        // accumulated back into `async_shell`'s frozen single-string,
        // deliver-at-exit contract.
        //
        // No dedicated regression test for this
        // call site specifically — it goes through the shared `shell_cmd()`
        // construction point already covered by `:!`'s tests
        // (`tests/new_vim_features.rs`'s `test_bang_command_honours_shell_
        // env_var` and `src/tui_main/shell_app.rs`'s `bang_command_shell_
        // output_paints_on_command_line_via_shell_app`), so a future
        // divergence here (e.g. someone hand-rolling a shell string again
        // for "just this one" call site) isn't caught by this PR's tests.
        for req in ctx.async_shell_requests {
            let mut cmd = crate::core::terminal::shell_cmd(&req.command);
            // Default to the engine's own workspace `cwd` when the plugin
            // didn't pass one. Without it, an omitted `cwd` falls
            // through to `std::process::Command`'s default — the *OS
            // process's* current directory, which only matches the open
            // workspace by convention (a terminal that `cd`ed there first).
            // The two genuinely diverge after `adopt_cwd_for_startup_file`
            // (opening a *file* via shortcut / "Open with": the repo root is
            // adopted in memory and the process cwd is deliberately left
            // alone — `mod.rs`, #1797), which is exactly the case this
            // repairs. `open_folder` is *not* an instance: it does call
            // `std::env::set_current_dir` (`buffers.rs`), so a GUI launch
            // followed by "Open Folder" already had the process cwd moved.
            // This is git-insights' blame.lua's failure mode on Win-GUI — it
            // `async_shell`s `git blame -- <relative path>` with no `cwd`, so
            // a GUI launch outside the repo ran `git` in the wrong directory
            // and the blame annotation silently vanished (output discarded,
            // see the `Err(_)` arm below).
            //
            // Seam note: `vimcode.loop.spawn` keeps falling through to the
            // OS process cwd when its own options omit `cwd` (changing a
            // second public API's documented default is out of scope here),
            // and `workspace_storage_key` (`plugin.rs`) keys off
            // `std::env::current_dir()` by design. See `EXTENSIONS.md`'s
            // `async_shell` options table for the documented default.
            match req.cwd.as_deref() {
                // An explicit `cwd` is passed through exactly as before this
                // fix, valid or not — if a plugin hands us a bad path, that
                // is the plugin's bug to see fail, not something to paper
                // over with a silent fallback.
                Some(cwd) => {
                    cmd.current_dir(cwd);
                }
                // No `cwd` given: default to the engine's workspace root,
                // but only if it still resolves to a real directory.
                // `self.cwd` can be stale (folder deleted/renamed, share
                // unmounted); when it is, fall all the way back to "no
                // `current_dir` call at all" rather than guaranteeing every
                // spawn in this loop fails.
                None if self.cwd.is_dir() => {
                    cmd.current_dir(spawn_cwd(&self.cwd).as_ref());
                }
                None => {}
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
        // Stash the last `set_context_menu` call this turn — read (and
        // cleared) by `open_ext_panel_context_menu` right after it fires
        // `panel_context_menu`, so a stale call from an earlier turn never
        // lingers.
        if let Some((panel_name, entries)) = ctx.panel_context_menu_items.into_iter().last() {
            let items = entries
                .into_iter()
                .map(|e| ContextMenuItem {
                    label: e.label,
                    action: e.id,
                    shortcut: String::new(),
                    separator_after: false,
                    enabled: !e.disabled,
                })
                .collect();
            self.ext_panel_context_menu_pending = Some((panel_name, items));
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
        // Honour `vimcode.ui.refresh(name)`. Bounded by
        // `plugin_view_render_depth` inside `refresh_plugin_view`, so a `render`
        // callback that asks to refresh itself settles instead of recursing.
        for name in ctx.plugin_view_refresh {
            self.refresh_plugin_view(&name);
        }
        // A dispatch's queued output is now applied, so this is the end of that
        // dispatch: flush any event that fired from inside it. Doing it
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
    /// uses. `Engine::async_shell_last_exit` is updated first, so a
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

    // ─── `vimcode.loop.timer`/`vimcode.schedule`/`vimcode.defer` ────────────

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

    /// Deliver a `vimcode.platform.show_*_dialog` result to its callback and
    /// drop the now-spent registration. Called from
    /// `run_pending_plugin_dialog` once the native (or TUI-fallback) dialog
    /// call has returned, with `path` already `None` if the user cancelled
    /// or the backend lacks the dialog capability.
    pub(crate) fn deliver_plugin_dialog_result(&mut self, id: u64, path: Option<PathBuf>) {
        let path = path.map(|p| p.to_string_lossy().into_owned());
        let ctx = self.make_plugin_ctx(true);
        let Some(ctx) =
            self.with_plugin_dispatch(|pm| pm.call_platform_dialog_result(id, path, ctx))
        else {
            return;
        };
        self.apply_plugin_ctx(ctx);
        if let Some(pm) = self.plugin_manager.clone() {
            pm.remove_platform_dialog_callback(id);
        }
    }

    /// `vimcode.platform.show_open_dialog`/`show_save_dialog`/
    /// `show_folder_dialog`: register `callback` and queue a
    /// [`PendingPlatformAction::PluginDialog`] for the runner to resolve on
    /// the next tick. A no-op (the callback is dropped unfired) if there is
    /// no live plugin manager — mirrors `plugin_api_spawn`'s `?` early-out.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn plugin_api_show_dialog(
        &mut self,
        kind: PluginDialogKind,
        title: Option<String>,
        initial_dir: Option<PathBuf>,
        initial_filename: Option<String>,
        filters: Vec<(String, Vec<String>)>,
        callback: mlua::RegistryKey,
    ) {
        let Some(pm) = self.plugin_manager.clone() else {
            return;
        };
        let callback_id = pm.register_platform_dialog_callback(callback);
        self.pending_platform_actions
            .push(PendingPlatformAction::PluginDialog(PluginDialogRequest {
                kind,
                title,
                initial_dir,
                initial_filename,
                filters,
                callback_id,
            }));
    }

    // ─── `vimcode.loop.spawn` ───────────────────────────────────────────────

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
                // Bounded per-tick drain: check the budget
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

    // ─── `vimcode.http` ─────────────────────────────────────────────────────

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
    /// dropped without ever reaching `remove_http_callback`.
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

    // ─── `vimcode.completion` ───────────────────────────────────────────────

    /// `vimcode.completion.register(...)`'s runtime path — `id` was
    /// already minted by the Lua binding (`next_completion_source_id`)
    /// before it knew whether this was a load-time or runtime call. Returns
    /// `false` if there is no live plugin manager (the Lua binding
    /// surfaces that as a runtime error, mirroring `plugin_api_picker_
    /// open`'s convention).
    pub(crate) fn plugin_api_completion_register(
        &mut self,
        id: u64,
        filetypes: Option<Vec<String>>,
        trigger_chars: Vec<char>,
        priority: i32,
        callback: mlua::RegistryKey,
    ) -> bool {
        let Some(pm) = self.plugin_manager.clone() else {
            return false;
        };
        pm.register_completion_source(id, filetypes, trigger_chars, priority, callback);
        true
    }

    /// `vimcode.completion.unregister(id)`.
    pub(crate) fn plugin_api_completion_unregister(&mut self, id: u64) {
        if let Some(pm) = self.plugin_manager.clone() {
            pm.remove_completion_source(id);
        }
    }

    /// `vimcode.completion.complete(request_id, items)` — async delivery of
    /// a source's results, from any P3 callback (`vimcode.loop.spawn`'s
    /// `on_exit`/`on_stdout`, `vimcode.http.request`'s callback, a
    /// timer...). Dropped silently if `request_id` names a generation
    /// older than the current one (#1805: "stale async results for an old
    /// prefix must be dropped" — the user kept typing and a fresh
    /// `trigger_completion` already superseded this request), if the popup
    /// is no longer relevant (not in Insert/Vscode mode), or if the owning
    /// source has since been unregistered.
    pub(crate) fn plugin_api_completion_complete(
        &mut self,
        request_id: u64,
        items: Vec<plugin::PluginCompletionItemSpec>,
    ) {
        let (generation, source_id) = unpack_completion_request_id(request_id);
        if generation != self.completion_generation {
            return; // stale — superseded by a later trigger
        }
        if !(self.mode == Mode::Insert || self.is_vscode_mode()) {
            return;
        }
        let Some(pm) = self.plugin_manager.clone() else {
            return;
        };
        let Some(priority) = pm.completion_source_priority(source_id) else {
            return; // source unregistered since the request was issued
        };
        // Same prefix narrowing the LSP merge applies (`cur_prefix.starts_
        // with` in the `CompletionResponse` handler): a matching generation
        // means no trigger has run since this request was issued, so the
        // prefix it was issued against is still `completion_filter_prefix`.
        let items = Self::filter_items_by_prefix(items, &self.completion_filter_prefix);
        self.merge_plugin_completion_items(items, priority);
    }

    /// Drop items whose `label` *and* `insert_text` both fail to start
    /// with `prefix` — same narrowing the built-in LSP merge applies, so a
    /// source returning candidates unrelated to what the user actually
    /// typed can't pollute the popup. Checking `label` too (not just
    /// `insert_text`) matters for the snippet-source use case #1212/#1805
    /// motivate: a snippet labeled `forloop` with `insert_text = "for i =
    /// 1, 10 do\n  \nend"` must survive typing `for`, even though its
    /// insert text doesn't start with the prefix. Skipped when `prefix` is
    /// empty (a trigger-char-driven invocation, e.g. right after `.`, has
    /// no prefix to narrow by).
    fn filter_items_by_prefix(
        items: Vec<plugin::PluginCompletionItemSpec>,
        prefix: &str,
    ) -> Vec<plugin::PluginCompletionItemSpec> {
        if prefix.is_empty() {
            return items;
        }
        items
            .into_iter()
            .filter(|i| i.label.starts_with(prefix) || i.insert_text.starts_with(prefix))
            .collect()
    }

    /// Run every registered `vimcode.completion.register` source that
    /// matches this trigger, merging synchronous results (a source's
    /// direct return value) immediately. Called from `Engine::
    /// trigger_completion` after the buffer-word scan and the
    /// `lsp_request_completion` kick-off — a source that needs to answer
    /// asynchronously instead calls `vimcode.completion.complete` later,
    /// mirroring how the LSP response merges in on its own event.
    pub(crate) fn plugin_request_completions(
        &mut self,
        prefix: &str,
        manual: bool,
        trigger_char: Option<char>,
    ) {
        let Some(pm) = self.plugin_manager.clone() else {
            return;
        };
        self.completion_generation = self.completion_generation.wrapping_add(1);
        let generation = self.completion_generation;
        let filetype = self.active_buffer_state().lsp_language_id.clone();
        let sources = pm.completion_sources_matching(
            filetype.as_deref(),
            prefix.is_empty(),
            manual,
            trigger_char,
        );
        if sources.is_empty() {
            return;
        }
        let buf = self.active_buffer_id().0 as i64;
        let line = self.view().cursor.line + 1;
        let col = self.view().cursor.col + 1;
        // Collect every source's synchronous result before merging any of
        // them, so the final merge sort sees the whole batch at once.
        let mut batches: Vec<(i32, Vec<plugin::PluginCompletionItemSpec>)> = Vec::new();
        for (source_id, priority) in sources {
            let request_id = pack_completion_request_id(generation, source_id);
            let ctx = self.make_plugin_ctx(true);
            let call = plugin::CompletionSourceCall {
                request_id,
                buf,
                line_1indexed: line,
                col_1indexed: col,
                prefix,
            };
            let Some((ctx, result)) =
                self.with_plugin_dispatch(|pm| pm.call_completion_source(source_id, call, ctx))
            else {
                continue;
            };
            self.apply_plugin_ctx(ctx);
            match result {
                Ok(Some(items)) => {
                    let items = Self::filter_items_by_prefix(items, prefix);
                    if !items.is_empty() {
                        batches.push((priority, items));
                    }
                }
                Ok(None) => {} // deferred — will arrive via `complete()` later
                Err(e) => {
                    // Report a broken source once, not once per keystroke
                    // (review finding) — `mark_completion_source_errored`
                    // returns `true` only the first time for this id.
                    if self
                        .plugin_manager
                        .as_ref()
                        .is_some_and(|pm| pm.mark_completion_source_errored(source_id))
                    {
                        self.message = format!("completion source error: {e}");
                    }
                }
            }
        }
        for (priority, items) in batches {
            self.merge_plugin_completion_items(items, priority);
        }
    }

    /// Merge plugin-supplied items into `completion_candidates`, then
    /// re-sort the whole list by priority descending. `sort_by` is stable,
    /// so items at equal priority keep their existing relative order —
    /// buffer/LSP candidates (always priority `0`) stay put unless a
    /// higher-priority plugin item jumps ahead of them, which is exactly
    /// #1805's "priority relative to buffer/LSP candidates" merge-order
    /// requirement. An item whose `insert_text` already matches an
    /// existing candidate replaces it in place rather than duplicating —
    /// a streamed source re-delivering the same item with fresher
    /// detail/documentation doesn't grow the list.
    ///
    /// `completion_idx` is re-found by the *selected candidate's*
    /// `insert_text` after the sort rather than carried over as a bare
    /// index (review finding): an async delivery landing while the user
    /// has cycled the selection to item `n` would otherwise silently
    /// repoint the selection at whatever candidate the sort happens to put
    /// at index `n`, and `<C-y>` would then accept the wrong item.
    ///
    /// `completion_display_only` is only forced to `true` when the popup
    /// wasn't already active (`completion_idx` was `None`) — if it lands
    /// mid `<C-n>`/`<C-p>` cycle (where `display_only` is deliberately
    /// `false` because the text is already in the buffer), merging must
    /// not silently flip the key semantics the user is mid-cycle on
    /// (review finding).
    fn merge_plugin_completion_items(
        &mut self,
        items: Vec<plugin::PluginCompletionItemSpec>,
        priority: i32,
    ) {
        if items.is_empty() {
            return;
        }
        let was_active = self.completion_idx.is_some();
        let selected_insert_text = self
            .completion_idx
            .and_then(|idx| self.completion_candidates.get(idx))
            .map(|c| c.insert_text.clone());
        for spec in items {
            let candidate = CompletionCandidate {
                label: spec.label,
                insert_text: spec.insert_text,
                kind: spec.kind,
                detail: spec.detail,
                documentation: spec.documentation,
                priority,
            };
            if let Some(existing) = self
                .completion_candidates
                .iter_mut()
                .find(|c| c.insert_text == candidate.insert_text)
            {
                *existing = candidate;
            } else {
                self.completion_candidates.push(candidate);
            }
        }
        self.completion_candidates
            .sort_by_key(|c| std::cmp::Reverse(c.priority));
        self.completion_idx = Some(
            selected_insert_text
                .and_then(|text| {
                    self.completion_candidates
                        .iter()
                        .position(|c| c.insert_text == text)
                })
                .unwrap_or(0),
        );
        if !was_active {
            self.completion_display_only = true;
        }
    }

    // ─── `vimcode.picker` ───────────────────────────────────────────────────

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
        // A plugin re-invoking `vimcode.picker.open` while a
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
        // Don't let a title-less `open()` call blank out
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
    /// **Reentrancy note:** this goes through
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
    /// **Reentrancy.** An event that fires *while a plugin callback is
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
    /// active window. Arg is the active window handle, matching the
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

    /// Flush the `CursorMoved`/`CursorMovedI` debounce — a sibling of
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

    /// Fire `WinLeave`/`WinEnter` for a window-focus change. Handles
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
mod spawn_cwd_tests {
    use super::spawn_cwd;
    use std::path::Path;

    /// A plain absolute workspace root is handed through untouched on every
    /// platform — this is the overwhelmingly common case for `async_shell`'s
    /// `cwd` default.
    #[test]
    fn plain_absolute_path_is_unchanged() {
        #[cfg(windows)]
        let cwd = Path::new(r"C:\work\repo");
        #[cfg(not(windows))]
        let cwd = Path::new("/work/repo");
        assert_eq!(spawn_cwd(cwd).as_ref(), cwd);
    }

    /// Windows: `canonicalize()`'s verbatim `\\?\C:\...` form must lose its
    /// prefix, because `cmd /C` (every plugin shell on Windows) refuses a
    /// non-drive-rooted current directory.
    #[test]
    #[cfg(windows)]
    fn verbatim_drive_path_is_stripped() {
        assert_eq!(
            spawn_cwd(Path::new(r"\\?\C:\work\repo")).as_ref(),
            Path::new(r"C:\work\repo")
        );
    }

    /// #1751 review round 2: a verbatim **UNC** root must survive intact.
    /// Naively stripping `\\?\` yields `UNC\server\share\...`, a relative
    /// path `Command::current_dir` would resolve against the process cwd —
    /// which can spawn the child somewhere real but wrong.
    #[test]
    #[cfg(windows)]
    fn verbatim_unc_path_is_left_alone_rather_than_made_relative() {
        let unc = Path::new(r"\\?\UNC\server\share\repo");
        let got = spawn_cwd(unc);
        assert!(
            got.is_absolute(),
            "a verbatim UNC workspace root must never be turned into a \
             relative path: {got:?}"
        );
        assert_eq!(got.as_ref(), unc);
    }
}

#[cfg(test)]
mod requires_vimcode_loader_gate_tests {
    //! `Engine::plugin_init`'s loader-tier gate — the
    //! half of the issue deferred in the original commit over a live
    //! file-overlap concern with #1806's branch. An installed extension
    //! whose `requires_vimcode` constraint is unmet by the running
    //! vimcode (e.g. after a downgrade) must have its Lua scripts
    //! skipped — proven here by a script that would register an
    //! observable panel never actually registering it — while a single
    //! message names the extension and the required version.
    use crate::core::engine::Engine;

    /// Write `extensions/<name>/{manifest.toml, init.lua}` under an
    /// isolated `$HOME` and mark the extension installed, with the given
    /// `requires_vimcode` (or none at all). The script unconditionally
    /// calls `vimcode.panel.register`, so whether or not it ran is
    /// directly observable via `engine.ext_panels` after `plugin_init()`.
    fn install_fixture_extension(
        home: &std::path::Path,
        ext_name: &str,
        requires_vimcode: Option<&str>,
    ) -> Engine {
        let ext_dir = home
            .join(".config")
            .join("vimcode")
            .join("extensions")
            .join(ext_name);
        std::fs::create_dir_all(&ext_dir).unwrap();
        let mut manifest =
            format!("name = \"{ext_name}\"\ndisplay_name = \"1807 Loader Gate Fixture\"\n");
        if let Some(req) = requires_vimcode {
            manifest.push_str(&format!("requires_vimcode = \"{req}\"\n"));
        }
        std::fs::write(ext_dir.join("manifest.toml"), manifest).unwrap();
        std::fs::write(
            ext_dir.join("init.lua"),
            format!(
                "vimcode.panel.register(\"{ext_name}\", {{title = \"Fixture\", \
                 sections = {{\"Default\"}}}})\n"
            ),
        )
        .unwrap();

        let mut engine = Engine::new();
        engine.extension_state.mark_installed(ext_name);
        engine
    }

    /// An installed extension whose `requires_vimcode` is unmet by the
    /// running vimcode must have its script skipped (never registers its
    /// panel) and must produce one message naming the extension and the
    /// required version — LSP/DAP config is untouched by this path (the
    /// fixture declares none), matching the issue's "Its LSP/DAP config
    /// should still load" requirement by not needing any change there.
    ///
    /// RED-verified by hand: with the `incompatibility_reason_for_running_
    /// vimcode` check in `plugin_init`'s extensions loop removed, this
    /// fails both ways — `ext_panels` gains the fixture's panel and
    /// `message` never mentions "Skipped incompatible extension".
    #[test]
    fn incompatible_installed_extension_is_skipped_with_message() {
        let home = std::env::temp_dir().join(format!(
            "vimcode_test_1807_loader_gate_incompatible_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        let _home_guard = crate::core::paths::set_test_home(&home);

        let ext_name = "vc-unit-1807-loader-incompatible";
        let mut engine = install_fixture_extension(&home, ext_name, Some(">=9999.0.0"));
        engine.plugin_init();

        assert!(
            !engine.ext_panels.contains_key(ext_name),
            "an incompatible extension's script must never run — its \
             vimcode.panel.register call must never execute; ext_panels: {:?}",
            engine.ext_panels.keys().collect::<Vec<_>>()
        );
        assert!(
            engine.message.contains("Skipped incompatible extension")
                && engine.message.contains("1807 Loader Gate Fixture")
                && engine.message.contains("9999.0.0"),
            "the skip must post one message naming the extension and the \
             required version; got: {:?}",
            engine.message
        );

        drop(_home_guard);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Compatible installed extension (no `requires_vimcode`, the
    /// pre-#1807 default): script runs normally, no skip message.
    #[test]
    fn compatible_installed_extension_still_loads() {
        let home = std::env::temp_dir().join(format!(
            "vimcode_test_1807_loader_gate_compatible_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        let _home_guard = crate::core::paths::set_test_home(&home);

        let ext_name = "vc-unit-1807-loader-compatible";
        let mut engine = install_fixture_extension(&home, ext_name, None);
        engine.plugin_init();

        assert!(
            engine.ext_panels.contains_key(ext_name),
            "a compatible extension's script must run normally; ext_panels: {:?}",
            engine.ext_panels.keys().collect::<Vec<_>>()
        );
        assert!(
            !engine.message.contains("Skipped incompatible extension"),
            "a compatible extension must never be reported skipped; got: {:?}",
            engine.message
        );

        drop(_home_guard);
        let _ = std::fs::remove_dir_all(&home);
    }
}

/// Extensions panel "Provides" listing: an installed extension's
/// commands, keymaps, panels and views, surfaced by prefixing them onto
/// its README when it's opened (Enter/double-click in the sidebar, or
/// `ext_open_selected_readme`). Verifies the plugin (file-stem) name
/// tracking survives the exact path a real extension takes: a script
/// filename that does *not* match the extension's own name (mirroring
/// `sql-tool`'s `sql_tool.lua`), loaded through `Engine::plugin_init`'s
/// real extensions-directory loop rather than a shortcut.
#[cfg(test)]
mod ext_contributions_tests {
    use crate::core::engine::Engine;

    /// Write `extensions/<ext_name>/{manifest.toml, README.md,
    /// <script_name>.lua}` under an isolated `$HOME` and mark the
    /// extension installed. Returns just the `Engine` — each caller
    /// creates and holds its own `set_test_home` guard, so there is no
    /// separate guard to return here.
    fn install_fixture_extension_with_script(
        home: &std::path::Path,
        ext_name: &str,
        script_name: &str,
        lua_code: &str,
        readme_body: &str,
    ) -> Engine {
        let ext_dir = home
            .join(".config")
            .join("vimcode")
            .join("extensions")
            .join(ext_name);
        std::fs::create_dir_all(&ext_dir).unwrap();
        std::fs::write(
            ext_dir.join("manifest.toml"),
            format!("name = \"{ext_name}\"\ndisplay_name = \"2142 Fixture\"\n"),
        )
        .unwrap();
        std::fs::write(ext_dir.join("README.md"), readme_body).unwrap();
        std::fs::write(ext_dir.join(format!("{script_name}.lua")), lua_code).unwrap();

        let mut engine = Engine::new();
        engine.extension_state.mark_installed(ext_name);
        engine
    }

    const FIXTURE_LUA: &str = r#"
        vimcode.command("SqlNewConnection", function(_) end, {
            title = "New Connection",
            category = "SQL",
        })
        vimcode.keymap.set("n", "<leader>r", function(_) end, {
            desc = "Run query",
        })
        vimcode.panel.register("sql", {
            title = "SQL",
            sections = {"Connections"},
        })
        vimcode.ui.register_view("sql_conn", {
            title = "SQL Connection",
            sidebar = false,
            render = function() return { fields = {} } end,
        })
    "#;

    /// `Engine::ext_plugin_names` must capture the loaded script's own
    /// file-stem (`"sql_tool"`), not the extension's name (`"sql-tool"`) —
    /// they deliberately differ here, the same way a real extension's
    /// script filename need not match its own package name.
    #[test]
    fn plugin_init_records_the_loaded_script_name_under_the_extension_name() {
        let home = std::env::temp_dir().join(format!(
            "vimcode_test_2142_plugin_names_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        let _home_guard = crate::core::paths::set_test_home(&home);

        let mut engine = install_fixture_extension_with_script(
            &home,
            "sql-tool",
            "sql_tool",
            FIXTURE_LUA,
            "A database connection tool.",
        );
        engine.plugin_init();

        assert_eq!(
            engine.ext_plugin_names.get("sql-tool"),
            Some(&vec!["sql_tool".to_string()]),
            "ext_plugin_names: {:?}",
            engine.ext_plugin_names
        );

        drop(_home_guard);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// The combined content `ext_show_readme_or_fetch_async` opens in the
    /// README preview tab must list the extension's command (with its
    /// palette title/category), its leader keymap (lhs expanded, desc
    /// included), its panel and its view — *and* still contain the
    /// original README body beneath that listing.
    #[test]
    fn opening_the_readme_lists_commands_keymaps_panels_and_views() {
        let home = std::env::temp_dir().join(format!(
            "vimcode_test_2142_readme_provides_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        let _home_guard = crate::core::paths::set_test_home(&home);

        let mut engine = install_fixture_extension_with_script(
            &home,
            "sql-tool",
            "sql_tool",
            FIXTURE_LUA,
            "A database connection tool.",
        );
        // A visible, non-whitespace leader so the expanded `<leader>r`
        // lhs is legible in the assertion below (the default leader is a
        // literal space, which `<leader>r` would expand into " r").
        engine.settings.leader = ',';
        engine.plugin_init();

        engine.ext_show_readme_or_fetch_async("sql-tool", "SQL Tool", "", true);
        let content = engine
            .buffer_manager
            .get(engine.active_buffer_id())
            .map(|s| s.buffer.content.to_string())
            .unwrap_or_default();

        // `buffer.content` holds the *rendered* markdown (`open_markdown_
        // preview_in_tab` runs it through `render_markdown` before storing
        // it), so headings lose their `#`/`##` marker and bullets render as
        // `•` rather than the generated `-` — assert against that rendered
        // form, not the raw markdown `ext_contributions_markdown` built.
        for expected in [
            "Provides",
            "SQL: New Connection",
            ",r",
            "Run query",
            "Panels",
            "Views",
            "SQL Connection",
            "database connection tool",
        ] {
            assert!(
                content.contains(expected),
                "expected tab content to contain {expected:?}; got:\n{content}"
            );
        }
        // The Panels heading's own row must be followed by the panel's
        // title as a distinct bullet line, strictly before the Views
        // heading — not merely present anywhere in the content, which
        // `"SQL: New Connection"`'s `"SQL"` substring would otherwise let
        // a missing Panels row slip past.
        let panels_idx = content
            .find("Panels\n")
            .expect("Panels heading must be present");
        let views_idx = content
            .find("Views\n")
            .expect("Views heading must be present");
        assert!(
            panels_idx < views_idx,
            "Panels heading must come before Views heading; got:\n{content}"
        );
        let panels_section = &content[panels_idx..views_idx];
        assert!(
            panels_section.contains("SQL\n") && !panels_section.contains("SQL Connection"),
            "expected a distinct 'SQL' panel row (and not the tab-only \
             view's title) between the Panels and Views headings; \
             got:\n{panels_section}"
        );
        let views_section = &content[views_idx..];
        assert!(
            views_section.contains("SQL Connection"),
            "expected the tab-only view's title under the Views heading; \
             got:\n{views_section}"
        );

        drop(_home_guard);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// An installed extension with no `README.md` on disk (and nowhere to
    /// fetch one from, `base_url == ""`) must still open a tab listing its
    /// contributions — the fallback that keeps the Provides listing from
    /// being reachable only through an extension's README.
    #[test]
    fn opening_the_readme_falls_back_to_contributions_when_no_readme_exists() {
        let home = std::env::temp_dir().join(format!(
            "vimcode_test_2142_no_readme_fallback_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        let _home_guard = crate::core::paths::set_test_home(&home);

        // Write the extension's manifest + script directly (skipping
        // `install_fixture_extension_with_script`'s README.md write) so
        // there is genuinely no README on disk.
        let ext_dir = home
            .join(".config")
            .join("vimcode")
            .join("extensions")
            .join("sql-tool");
        std::fs::create_dir_all(&ext_dir).unwrap();
        std::fs::write(
            ext_dir.join("manifest.toml"),
            "name = \"sql-tool\"\ndisplay_name = \"2142 Fixture\"\n",
        )
        .unwrap();
        std::fs::write(ext_dir.join("sql_tool.lua"), FIXTURE_LUA).unwrap();

        let mut engine = Engine::new();
        engine.extension_state.mark_installed("sql-tool");
        engine.plugin_init();

        engine.ext_show_readme_or_fetch_async("sql-tool", "SQL Tool", "", true);
        let content = engine
            .buffer_manager
            .get(engine.active_buffer_id())
            .map(|s| s.buffer.content.to_string())
            .unwrap_or_default();

        assert!(
            content.contains("SQL: New Connection"),
            "a README-less installed extension must still open a tab \
             listing its contributions; got:\n{content}"
        );

        drop(_home_guard);
        let _ = std::fs::remove_dir_all(&home);
    }
}

#[cfg(test)]
mod settings_snapshot_tests {
    use crate::core::engine::Engine;

    /// Review regression: the VSCode-extension-settings bridge
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
