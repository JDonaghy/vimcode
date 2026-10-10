use super::*;

impl App {
    /// Open the tab context menu for `tab_idx` in `group_id`, anchored at the
    /// click's pixel position.
    ///
    /// #732 tranche 1: was `Msg::TabRightClick`, constructed by
    /// `ShellApp::handle` from a `UiEvent::MouseDown` it already held and
    /// immediately decoded again by `dispatch`.
    pub(crate) fn handle_tab_right_click(
        &mut self,
        group_id: core::window::GroupId,
        tab_idx: usize,
        x: f64,
        y: f64,
    ) {
        let cw = self.cached_char_width.max(1.0);
        let lh = self.cached_line_height.max(1.0);
        let cx = (x / cw) as u16;
        let cy = (y / lh) as u16;
        self.engine
            .borrow_mut()
            .open_tab_context_menu(group_id, tab_idx, cx, cy);
        self.draw_needed.set(true);
    }

    /// Open the editor (buffer text) context menu at the click's pixel
    /// position, unless a focused modal wants to swallow the click.
    pub(crate) fn handle_editor_right_click(
        &mut self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
    ) {
        // Swallow if the click landed on a focused modal that
        // wants to consume it (#216 — editor hover popup).
        self.reconcile_editor_hover_modal(backend);
        let stack_rc = backend.modal_stack_handle();
        let in_modal = stack_rc
            .borrow()
            .hit_test(quadraui::Point {
                x: x as f32,
                y: y as f32,
            })
            .is_some();
        if in_modal {
            return;
        }
        let cw = self.cached_char_width.max(1.0);
        let lh = self.cached_line_height.max(1.0);
        let cx = (x / cw) as u16;
        let cy = (y / lh) as u16;
        self.engine.borrow_mut().open_editor_context_menu(cx, cy);
        self.draw_needed.set(true);
    }

    /// After `handle_dispatch` may have opened a context menu
    /// (`Engine::open_*_context_menu`, from any surface — editor, tab,
    /// Explorer, Board, ...), show it immediately if the backend resolves
    /// `MenuStyle` to `Native` (#1580, and the root-cause fix for the
    /// macOS right-click bug it also closes). `render::show_context_menu_now`
    /// does this through quadraui's own single-call
    /// `quadraui::ContextMenuController::open` (quadraui#1187) rather than
    /// calling `Backend::show_context_menu` directly.
    ///
    /// Called once from `Self::handle`'s single choke point, gated on the
    /// `context_menu` open *transition* (`None` -> `Some`) rather than
    /// from each individual `open_*_context_menu` call site — every
    /// right-click/keyboard path that opens a menu funnels through
    /// `handle_dispatch` before returning to `handle`, so one check there
    /// covers all of them without per-surface backend threading. See
    /// `render::show_context_menu_now`'s doc for why this must run from
    /// event-handling code and never from `render_content`'s paint rung
    /// (`FrameOp::ContextMenu`): `MacBackend::show_context_menu` blocks on
    /// AppKit's modal popup loop, and running that from inside a paint
    /// closure re-enters painting while the closure still holds the
    /// borrows it needs to finish its own frame.
    pub(crate) fn open_context_menu_now_if_native(&mut self, backend: &mut dyn quadraui::Backend) {
        if backend.effective_menu_style() != quadraui::ResolvedMenuStyle::Native {
            return;
        }
        let panel = self
            .engine
            .borrow()
            .context_menu
            .as_ref()
            .map(render::context_menu_state_to_panel);
        let Some(panel) = panel else {
            return;
        };
        if panel.items.is_empty() {
            return;
        }
        let cw = self.cached_char_width.max(1.0);
        let lh = self.cached_line_height.max(1.0);
        render::show_context_menu_now(backend, &panel, cw, lh);
    }

    /// Handle a window/viewport resize.
    pub(crate) fn handle_resize(&mut self) {
        // #731: both branches here were gated on `self.overlay` /
        // `self.drawing_area`, permanently `None` under the
        // ShellApp runner (nothing assigns either field) — so
        // this was already a no-op: the backend viewport is
        // re-derived every frame by the runner itself, and
        // terminal-pane resize-on-window-resize has not fired
        // since the #540 cutover. Re-deriving live terminal
        // pane sizing needs a way to read the live DA's pixel
        // size without a widget handle — see `terminal_cols`.
        self.draw_needed.set(true);
    }

    /// Ctrl+Click — plant a secondary cursor at the clicked buffer position.
    ///
    /// The retired `Msg::CtrlMouseClick` also carried `width`/`height`, but
    /// the arm bound both to `_`, so they are dropped from the signature
    /// rather than threaded through unused.
    pub(crate) fn handle_ctrl_mouse_click(
        &mut self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
    ) {
        let layout_ref = self.cached_screen_layout.borrow();
        if let Some(ref layout) = *layout_ref {
            let mut engine = self.engine.borrow_mut();
            if !engine.picker_open {
                let drag_rc = backend.drag_state_handle();
                let mut drag = drag_rc.borrow_mut();
                if let ClickTarget::BufferPos(_, line, col) = pixel_to_click_target(
                    &mut engine,
                    backend,
                    x,
                    y,
                    self.cached_line_height,
                    self.cached_char_width,
                    layout,
                    &self.cached_group_tab_bar_layouts.borrow(),
                    self.cached_frame_hit_map.borrow().as_ref(),
                    &self.cached_tab_bar_zones.borrow(),
                    true, // real click: focus/tab/gutter side effects are intended
                    &mut drag,
                    false, // Ctrl+click has no Alt-fine-seek concept
                ) {
                    engine.add_cursor_at_pos(line, col);
                }
            }
        }
        self.draw_needed.set(true);
    }

    /// Double-click in the editor drawing area at the given pixel position.
    ///
    /// As with [`App::handle_ctrl_mouse_click`], the `width`/`height` the
    /// retired `Msg::MouseDoubleClick` carried were bound to `_` and are
    /// dropped from the signature.
    pub(crate) fn handle_mouse_double_click_msg(
        &mut self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
    ) {
        // #490: a double-click landing on the editor hover popup used to fall
        // straight through to the editor's word-select underneath, because
        // this handler never consulted the popup at all. It runs the same
        // shared rung the single-click path does, first.
        if self.route_and_apply_editor_hover_popup(backend, x, y) {
            return;
        }
        // ── Editor-tab-hosted plugin view double-click (#1631) ─────────
        //
        // Mirrors `handle_mouse_click_msg`'s tab-hosted press routing: a
        // body-kind view's `List`/`Table` row activates (`ItemActivated`)
        // rather than just selecting, and a field-stack `Form` falls back
        // to `render::handle_plugin_view_tab_ui_event` exactly as the press
        // handler does (that function's own `DoubleClick` arm probes it as
        // a `MouseDown` for `FormController`).
        if let Some((name, rect)) = self.plugin_view_tab_hit(x, y) {
            let mut engine = self.engine.borrow_mut();
            engine.clear_sidebar_focus();
            let event = quadraui::UiEvent::DoubleClick {
                widget: None,
                position: quadraui::Point::new(x as f32, y as f32),
            };
            let backend_rc = self.backend.clone();
            let mut b = backend_rc.borrow_mut();
            let consumed = render::route_plugin_view_body_event(
                &mut engine,
                &name,
                PluginViewHost::Tab,
                &event,
                rect,
                &mut **b,
            );
            drop(b);
            if consumed.is_none() {
                render::handle_plugin_view_tab_ui_event(&mut engine, &name, &event, rect);
            }
            drop(engine);
            self.draw_needed.set(true);
            return;
        }
        let mut engine = self.engine.borrow_mut();
        if engine.picker_open {
            let in_tree_mode = engine.picker_source
                == crate::core::engine::PickerSource::CommandCenter
                && engine.picker_query == "@";
            if in_tree_mode && engine.picker_toggle_expand() {
                engine.picker_load_preview();
            } else {
                let _action = engine.picker_confirm();
            }
            self.draw_needed.set(true);
        } else {
            // Breadcrumb double-click: same shared resolution as the
            // single-click path above (#555). This used to re-derive
            // the bar's geometry by hand — `y >= lh && y < lh * 2.0`
            // plus a per-`char_width` walk over the *active* group's
            // segments — which is pre-#540 Relm4 geometry: under the
            // ShellApp runner the breadcrumb row sits below the title
            // bar, the menu bar and a `1.6 * lh` tab row, so that band
            // never contained it (double-click was dead) while still
            // matching chrome rows that could fire the wrong segment.
            let mut bc_handled = false;
            if engine.settings.breadcrumbs {
                let lh = self.painted_line_height();
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(ref layout) = *layout_ref {
                    match render::resolve_breadcrumb_click(&layout.breadcrumbs, x, y, lh) {
                        render::BreadcrumbClickResult::Hit(group_id, idx) => {
                            drop(layout_ref);
                            engine.handle_breadcrumb_double_click(group_id, idx);
                            bc_handled = true;
                        }
                        render::BreadcrumbClickResult::OnBar => {
                            bc_handled = true;
                        }
                        render::BreadcrumbClickResult::Miss => {}
                    }
                }
            }
            if !bc_handled {
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(ref layout) = *layout_ref {
                    let drag_rc = backend.drag_state_handle();
                    handle_mouse_double_click(
                        &mut engine,
                        backend,
                        x,
                        y,
                        self.cached_line_height,
                        self.cached_char_width,
                        layout,
                        &self.cached_group_tab_bar_layouts.borrow(),
                        self.cached_frame_hit_map.borrow().as_ref(),
                        &self.cached_tab_bar_zones.borrow(),
                        &mut drag_rc.borrow_mut(),
                    );
                }
            }
        }
        self.draw_needed.set(true);
    }

    /// Mouse wheel over the editor drawing area.
    ///
    /// `delta_y` arrives in **GTK's raw polarity** (positive = wheel down) —
    /// see the negation comment at the `UiEvent::Scroll` call site in
    /// `ShellApp::handle`.
    pub(crate) fn handle_mouse_scroll_msg(
        &mut self,
        backend: &dyn quadraui::Backend,
        delta_x: f64,
        delta_y: f64,
    ) {
        let mut engine = self.engine.borrow_mut();
        // Picker open: scroll the picker results.
        //
        // #191: previously used `(delta_y * 3.0).round()`, which
        // rounded small trackpad deltas (dy<0.17) down to 0 and
        // made scrolling feel dead. `.ceil()` on the absolute
        // value guarantees every non-zero event advances at
        // least one row, and the `5.0` amplification is closer
        // to native-app conventions for wheel notches.
        if engine.picker_open && delta_y.abs() > 0.01 {
            let step = (delta_y.abs() * 5.0).ceil() as isize;
            let delta = if delta_y > 0.0 { step } else { -step };
            engine.picker_scroll(delta, 20);
            drop(engine);
            self.draw_needed.set(true);
            return;
        }
        // ── Editor-tab-hosted plugin view scroll (#1631) ───────────────
        //
        // Mirrors the press/double-click tab routing in
        // `handle_mouse_click_msg`/`handle_mouse_double_click_msg`: a wheel
        // notch over a body-kind view's window scrolls its own content
        // instead of falling through to the generic per-window viewport
        // scroll below, and a field-stack `Form` falls back to
        // `render::handle_plugin_view_tab_ui_event` the same way those two
        // handlers do. `delta_y` is negated back to quadraui's polarity —
        // see this function's own doc comment — since `route_plugin_view_
        // body_event`'s `Scroll` arm (shared with the sidebar's already-
        // quadraui-polarity `UiEvent`s) expects it.
        if delta_y.abs() > 0.01 {
            if let Some((px, py)) = self.last_editor_pointer.get() {
                if let Some((name, rect)) = self.plugin_view_tab_hit(px, py) {
                    engine.clear_sidebar_focus();
                    let event = quadraui::UiEvent::Scroll {
                        widget: None,
                        position: quadraui::Point::new(px as f32, py as f32),
                        delta: quadraui::ScrollDelta::new(delta_x as f32, -(delta_y as f32)),
                    };
                    let backend_rc = self.backend.clone();
                    let mut b = backend_rc.borrow_mut();
                    let consumed = render::route_plugin_view_body_event(
                        &mut engine,
                        &name,
                        PluginViewHost::Tab,
                        &event,
                        rect,
                        &mut **b,
                    );
                    drop(b);
                    if consumed.is_none() {
                        render::handle_plugin_view_tab_ui_event(&mut engine, &name, &event, rect);
                    }
                    drop(engine);
                    self.draw_needed.set(true);
                    return;
                }
            }
        }
        // Route scroll through dispatch_scroll using cached scroll surfaces.
        if let Some((px, py)) = self.last_editor_pointer.get() {
            let surfaces = engine.scroll_surfaces.borrow();
            let scroll_events = quadraui::dispatch_scroll(
                &backend.modal_stack_handle().borrow(),
                &surfaces,
                quadraui::Point {
                    x: px as f32,
                    y: py as f32,
                },
                quadraui::ScrollDelta::new(delta_x as f32, delta_y as f32),
            );
            drop(surfaces);
            for sev in &scroll_events {
                if let quadraui::UiEvent::Scroll {
                    widget: Some(id),
                    delta,
                    ..
                } = sev
                {
                    match id.as_str() {
                        "editor_hover" => {
                            let step = (delta.y * 3.0).round() as i32;
                            engine.editor_hover_scroll(step);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                        "debug_output" => {
                            engine.handle_debug_output_scroll(delta.y);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                        "terminal_scrollback" => {
                            // #533: single shared scroll entry point.
                            // delta.y < 0 = up (into history); > 0 =
                            // down (toward live).  Policy + forwarding
                            // live in Engine::handle_terminal_scroll.
                            engine.handle_terminal_scroll(delta.y);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                        _ => {}
                    }
                }
            }
        }
        // #240: route to the window under the pointer, falling back
        // to the active window when the pointer is missing or over
        // a non-window region. Hovering an unfocused group's pane
        // scrolls *that* pane without changing focus or moving its
        // cursor — matches TUI behaviour.
        // #646: resolve the hovered pane against the bounds
        // `render_content` actually painted with (`cached_editor_bounds`,
        // absolute coords including the activity-bar/sidebar x-offset and
        // the title-bar y-offset), not against a re-derived
        // `(0, 0, da.width(), …)` rect. `self.drawing_area` is never
        // assigned under the ShellApp runner — the runner owns the single
        // DrawingArea — so the old `if let Some(da)` arm never ran and this
        // was unconditionally `None`; and even had it run, a `(0, 0)`
        // origin is the exact coordinate-frame mismatch #582 fixed for
        // divider hit-testing.
        //
        // #1433: resolved via `render::find_window_at` against
        // `self.cached_screen_layout` — the same painted-window lookup the
        // double-click branch above already uses — instead of a second,
        // hand-rolled `calculate_group_window_rects` scan. `find_window_at`
        // is the *shared* hit-test both backends' mouse code already routes
        // through everywhere else (`tui_main::mouse`'s `render::
        // find_window_at` call sites), so this was the one remaining
        // GTK-only reimplementation of it — inlined here since #240, before
        // `find_window_at` existed as a shared helper.
        let hovered_window_id = self.last_editor_pointer.get().and_then(|(x, y)| {
            let layout = self.cached_screen_layout.borrow();
            let layout = layout.as_ref()?;
            let idx = render::find_window_at(layout, x, y)?;
            Some(layout.windows[idx].window_id)
        });
        if delta_y.abs() > 0.01 {
            let scroll_count = (delta_y * 3.0).round().abs() as usize;
            let active_id = engine.active_window_id();
            let target = hovered_window_id.unwrap_or(active_id);
            if target == active_id {
                let dir = if delta_y > 0.0 { 1 } else { -1 };
                engine.scroll_viewport_with_cursor(dir, scroll_count);
            } else {
                let dir = if delta_y > 0.0 { 1 } else { -1 };
                engine.scroll_viewport_with_cursor_for_window(target, dir, scroll_count);
            }
            engine.sync_scroll_binds();
        }
        if delta_x.abs() > 0.01 {
            let win_id = engine.active_window_id();
            let current = engine.view().scroll_left;
            let scroll_amount = (delta_x * 3.0).round() as isize;
            let new_left = (current as isize + scroll_amount).max(0) as usize;
            engine.set_scroll_left_for_window(win_id, new_left);
        }
        drop(engine);
        self.draw_needed.set(true);
    }

    /// `settings.json` changed on disk — reload it and, if the reload took,
    /// refresh the file tree (`show_hidden_files` may have flipped).
    pub(crate) fn settings_file_changed(&mut self) {
        if self.engine.borrow_mut().check_settings_reload() {
            self.refresh_file_tree();
            self.draw_needed.set(true);
        }
    }

    /// Reveal `target` in the explorer sidebar: expand all ancestors,
    /// rebuild the row list, select the matching row, scroll into view,
    /// and queue a redraw of the explorer DrawingArea. Phase A.2b-2
    /// replacement for `highlight_file_in_tree` (which operated on the
    /// native `gtk4::TreeView`).
    pub(crate) fn reveal_path_in_explorer(&self, target: &Path) {
        if let Ok(mut engine) = self.engine.try_borrow_mut() {
            engine.explorer_reveal_path(target);
            drop(engine);
            self.queue_explorer_draw();
        }
    }

    pub(crate) fn refresh_explorer(&self) {
        self.engine.borrow_mut().explorer_rebuild_rows();
        self.queue_explorer_draw();
    }

    /// Snapshot the cached window geometry (#1234, extended #1529 for
    /// position/maximized) into a [`core::session::WindowGeometry`] ready to
    /// write into `engine.session.window` before `Engine::save_session_state`
    /// persists it.
    ///
    /// Reads the `cached_window_*` cells rather than a live `backend`
    /// handle: every call site that needs to save on quit
    /// ([`Self::quit_and_save_session`] and, through it,
    /// [`Self::save_session_and_exit`] below) runs through
    /// `apply_engine_action`/`run_shared_tick_chores`/menu/dialog call chains
    /// with no live `backend: &mut dyn quadraui::Backend` in scope — only
    /// `tick`/`setup`/paint entry points have one — the same "no backend in
    /// scope" problem `cached_line_height`/`cached_char_width` solve for text
    /// metrics, solved the same way here. A free function (shared by both
    /// sites) rather than duplicated field-copies keeps the five-field list
    /// in one place.
    pub(crate) fn cached_window_geometry(&self) -> core::session::WindowGeometry {
        core::session::WindowGeometry {
            width: self.cached_window_width.get(),
            height: self.cached_window_height.get(),
            x: self.cached_window_x.get(),
            y: self.cached_window_y.get(),
            maximized: self.cached_window_maximized.get(),
        }
    }

    /// Save session state and request a clean shutdown, given an already
    /// mutably-borrowed `engine`.
    ///
    /// Sets [`App::exit_requested`] rather than calling `process::exit`
    /// itself (#813) — `ShellApp::handle`/`tick` check the flag once they
    /// return and surface [`quadraui::Reaction::Exit`] to the runner, which
    /// tears the window down via `ReactionSink::request_exit`
    /// (`gtk/run.rs`), the same mechanism every other quadraui backend uses.
    ///
    /// Takes `engine` as a parameter rather than borrowing `self.engine`
    /// itself (unlike [`Self::save_session_and_exit`] below) because its
    /// callers — `render::apply_engine_action`'s `Quit`/`SaveQuit`/
    /// `QuitWithUnsaved` arms and `render::run_shared_tick_chores`'s
    /// format-on-save-then-quit chore — already hold `engine: &mut Engine`
    /// borrowed from this same `Rc<RefCell<Engine>>` for the whole call; a
    /// second, independent `self.engine.borrow_mut()` from in here would
    /// double-borrow and panic at runtime (#1063, #1248, folded here by
    /// #1499).
    pub(crate) fn quit_and_save_session(&self, engine: &mut Engine) {
        // Capture the cached window geometry into session state *before*
        // `save_session_state` persists it — `Engine` has no window handle
        // of its own to read this from (#823 item 5). See
        // `cached_window_geometry`'s own doc for why this reads cached
        // cells rather than a live `backend` handle (#1234, #1529).
        engine.session.window = self.cached_window_geometry();
        engine.save_session_state();
        engine.cleanup_all_swaps();
        engine.lsp_shutdown();
        self.exit_requested.set(true);
    }

    /// Save the current session state and request a clean shutdown.
    ///
    /// Thin wrapper around [`Self::quit_and_save_session`] for call sites
    /// that don't already hold `engine` borrowed — see that method's own
    /// doc for why the two can't simply be one.
    pub(crate) fn save_session_and_exit(&self) {
        let mut engine = self.engine.borrow_mut();
        self.quit_and_save_session(&mut engine);
    }

    /// Dispatch an `EngineAction` produced by `handle_key`, macro playback,
    /// or a fired menu item (`handle_menu_action`, below).
    ///
    /// `is_macro`: when true, `OpenTerminal` toggles instead of creating a new
    /// tab, and dialog-open actions are suppressed (macros can't drive
    /// dialogs) — handled here, before ever reaching `apply_engine_action`,
    /// since neither is something a shared applier should know about (a
    /// menu click is never `is_macro`, so this whole branch is dead for that
    /// caller). Every other variant — the exhaustive general-purpose case —
    /// is `render::apply_engine_action` (#1063), the same function
    /// `tui_main::dispatch_post_key_action` now calls too; see that
    /// function's rung header comment in `render.rs`.
    pub(crate) fn dispatch_engine_action(&mut self, action: EngineAction, is_macro: bool) {
        if is_macro {
            match &action {
                EngineAction::OpenTerminal => {
                    self.toggle_terminal();
                    return;
                }
                EngineAction::OpenFolderDialog
                | EngineAction::OpenWorkspaceDialog
                | EngineAction::SaveWorkspaceAsDialog
                | EngineAction::OpenRecentDialog => return,
                _ => {}
            }
        }
        let engine_rc = self.engine.clone();
        render::apply_engine_action(action, &mut engine_rc.borrow_mut(), self);
    }

    /// Return focus to the main editor drawing area when a sidebar loses
    /// focus.
    ///
    /// #731: was `if let Some(ref drawing) = *self.drawing_area.borrow()`
    /// — that field is permanently `None` under the ShellApp runner
    /// (nothing assigns it), so this has been a no-op since #540. Kept as
    /// a named function (rather than deleting every call site) so the
    /// intent stays legible at each of its ~10 callers; a real fix needs a
    /// live way to grab GTK keyboard focus on the editor DA from here,
    /// which nothing in this file currently has under ShellApp.
    pub(crate) fn focus_editor_if_needed(&self, _still_focused: bool) {}

    /// Sync the unnamed `"` register (and explicit `+` register) to the system clipboard
    /// whenever their content changes (clipboard=unnamedplus semantics).
    ///
    /// Thin wrapper — see [`render::sync_register_to_clipboard`] (#1239) for
    /// the shared implementation TUI's `sync_tui_clipboard` also delegated to
    /// before #1434 deleted that wrapper along with the rest of the
    /// pre-#1434 TUI shell.
    pub(crate) fn sync_plus_register_to_clipboard(&mut self) {
        render::sync_register_to_clipboard(
            &mut self.engine.borrow_mut(),
            &mut self.last_clipboard_content,
        );
    }

    #[allow(clippy::too_many_lines)]
    #[allow(clippy::too_many_arguments)]
    /// `ctx` is threaded in solely for the shared Alt rung's
    /// [`render::AltKeyOutcome::ResizeSidebar`] arm: GTK's authoritative
    /// sidebar width *is* the runner's `AppShell` (TUI keeps its own copy and
    /// syncs it out at end of dispatch), and `ShellContext::shell_mut` is the
    /// only handle to it. `ui_event` (#815) is the raw event `key_name`/
    /// `unicode`/... were decoded from, needed by the folder-picker rung —
    /// mirrors TUI's `KeyDispatchState::ui_event`. `backend` (#1428) is
    /// solely for the Ctrl+L rung's `backend.request_full_repaint()` call —
    /// GTK's `DrawingArea` repaints in full every frame regardless (no
    /// incremental diff to desync), so this is a no-op there; see
    /// `render::is_force_redraw_key`'s own doc for why the hook still needs
    /// calling from every backend rather than being TUI-only code.
    pub(crate) fn handle_key_press(
        &mut self,
        key_name: String,
        unicode: Option<char>,
        ctrl: bool,
        shift: bool,
        alt: bool,
        ui_event: &quadraui::UiEvent,
        backend: &mut dyn quadraui::Backend,
        ctx: &quadraui::ShellContext<'_>,
    ) {
        // ── Shared toast-stack keyboard-focus rung (#1577) ──────────────
        // Non-modal, unlike every rung below: `Engine::handle_toast_focus_key`
        // only ever consumes a key once something has explicitly given the
        // stack focus (`:Notifications` / `panel_keys.focus_notifications`)
        // — every other key, and every key while unfocused, comes back
        // `false` untouched, so checking this first (ahead of even the
        // modal-dialog rung) is safe and replaces the old hardcoded `N`
        // hijack (`keys.rs`, removed) with a real keyboard-focus cursor.
        if self.engine.borrow_mut().handle_toast_focus_key(ui_event) {
            self.draw_needed.set(true);
            return;
        }

        // ── Shared modal keyboard rung (#734 slice 1) ──────────────────
        // Bound to a local first: a `RefCell::borrow()` temporary in a `match`
        // scrutinee lives for the whole `match`, and the arms `borrow_mut()`.
        let modal_route = render::route_modal_key(&self.engine.borrow());
        match modal_route {
            render::ModalKeyRoute::Engine => {
                let action = {
                    let mut engine = self.engine.borrow_mut();
                    engine.handle_key(&key_name, unicode, ctrl)
                };
                self.dispatch_engine_action(action, false);
                self.queue_explorer_draw();
                self.draw_needed.set(true);
                return;
            }
            render::ModalKeyRoute::ContextMenu => {
                self.dispatch_context_menu_key(&key_name, unicode);
                return;
            }
            render::ModalKeyRoute::None => {}
        }

        // ── Shared folder-picker rung (#815) ────────────────────────────
        // Above every other tier: once `open_folder_dialog` (below) has
        // populated `folder_picker`, every key belongs to the picker.
        // `FolderPickerController::handle` owns the key→intent mapping
        // itself (Escape/Enter/Up/Down/k/j/-/Backspace/printable,
        // Ctrl-gated) — this rung just feeds it the raw event and applies
        // the outcome. Mirrors TUI's identical rung in `handle_key_pressed`
        // (`shell_app.rs`), same precedence relative to the modal rung above.
        if self.folder_picker.borrow().is_some() {
            self.apply_folder_picker_event(ui_event);
            self.draw_needed.set(true);
            return;
        }

        // Dismiss any panel hover popup on key press.
        self.engine.borrow_mut().dismiss_panel_hover_now();

        // ── Shared Ctrl+L force-redraw rung (#762 / #734 slice 7) ──────
        // New on GTK: there was no Ctrl+L tier here at all, so the chord fell
        // through to whichever tier came next instead of being consumed.
        // `insert_ctrl_x_pending` carves out `<C-x><C-l>` (whole-line
        // completion, #1160) — see `render::is_force_redraw_key`'s doc.
        let insert_ctrl_x_pending = {
            let engine = self.engine.borrow();
            engine.mode == crate::core::Mode::Insert && engine.insert_ctrl_x_pending
        };
        if render::is_force_redraw_key(&key_name, unicode, ctrl, insert_ctrl_x_pending) {
            // #1428: was an ordinary redraw only — no rung called
            // `Backend::request_full_repaint` on GTK at all. Harmless
            // no-op there today (Cairo's `DrawingArea` repaints in full
            // every frame, no incremental diff to desync — see
            // `render::is_force_redraw_key`'s own doc), but load-bearing
            // the moment `App` backs a real terminal (the `tui` harness
            // arm today, a future live TUI-via-`App` entry point
            // eventually): without this, Ctrl+L would only request an
            // ordinary `Reaction::Redraw`, which a diffing backend's next
            // paint could resolve as "nothing changed" for any cell
            // written outside its own diff tracking.
            backend.request_full_repaint();
            self.draw_needed.set(true);
            return;
        }

        // ── Shared clipboard-paste pre-load rung (#760 / #734 slice 5) ─────
        // No Ctrl+Shift+V arm to converge here: quadraui's runner intercepts
        // that chord and redelivers it as `UiEvent::ClipboardPaste`.
        render::preload_paste_clipboard(&mut self.engine.borrow_mut(), &key_name, unicode, ctrl);

        // ── Shared focus-owner keyboard rung (#757 / #734 slice 2) ─────
        // GTK keeps no "the sidebar band holds the keyboard" latch of its own,
        // so it passes `Engine::sidebar_has_focus()` — the disjunction of the
        // very flags the resolver's arms test, making that gate a no-op here.
        let focus_route = {
            let engine = self.engine.borrow();
            let band = engine.sidebar_has_focus();
            render::route_focus_key(&engine, band)
        };

        if focus_route == render::FocusKeyRoute::ActivityBar {
            self.handle_activity_bar_key(&key_name, ctrl);
            self.draw_needed.set(true);
            return;
        }

        // ── Shared terminal (PTY) keyboard rung (#758 / #734 slice 3) ──
        // Above the debug F-keys, the same slot TUI uses: a focused terminal
        // takes F5/F9/F10/F11 to the PTY, as vim/htop expect.
        if render::route_terminal_key(
            &mut self.engine.borrow_mut(),
            &key_name,
            unicode,
            ctrl,
            shift,
            alt,
        ) {
            self.sync_plus_register_to_clipboard();
            self.draw_needed.set(true);
            return;
        }

        // ── Shared debugger F-key rung (#762 / #734 slice 7) ───────────
        // Global, above the sidebar panels. The `shift` half is new here:
        // this block used to test only `!ctrl && !alt`, so Shift+F5 ran
        // *continue* instead of *stop*.
        match render::route_debug_fkey(&key_name, ctrl, shift, alt) {
            Some(render::DebugFKey::Command(cmd)) => {
                let _ = self.engine.borrow_mut().execute_command(cmd);
                self.draw_needed.set(true);
                return;
            }
            Some(render::DebugFKey::EngineKey(name)) => {
                let action = self.engine.borrow_mut().handle_key(name, None, false);
                self.dispatch_engine_action(action, false);
                self.draw_needed.set(true);
                return;
            }
            None => {}
        }

        // ── Shared Ctrl-W sidebar chord rung (#1419, closes #406) ──────
        // GTK kept no per-keypress chord latch of its own, so `Ctrl-W`
        // followed by `h`/`l` in a sidebar panel silently did nothing here
        // while TUI's TUI-only `TuiSidebar::pending_ctrl_w` handled it.
        // `Engine::sidebar_ctrl_w_pending` is the shared latch both backends
        // now read/write through `route_sidebar_chord_key`; GTK has no
        // "sidebar band holds focus" shadow flag to clear on
        // `SidebarChordAction::FocusOut` (unlike TUI's `sidebar.has_focus`),
        // so there is nothing else to do here besides redraw.
        if focus_route != render::FocusKeyRoute::None
            && render::route_sidebar_chord_key(
                &mut self.engine.borrow_mut(),
                &key_name,
                unicode,
                ctrl,
            )
            .is_some()
        {
            self.draw_needed.set(true);
            return;
        }

        // ── Shared focus-owner *dispatch* rung (#762 / #734 slice 7) ───
        // Slice 2 shared only the *routing*; `render::dispatch_sidebar_panel_key`
        // now states the six pure-`Engine` arms too, and TUI's
        // `handle_focus_owner_key` calls the same function (after its own
        // crossterm-spelling translation) — this is no longer GTK-only. It
        // hands back `None` for the two it cannot own — Debug needs a live
        // `Backend`, Explorer is a backend widget — which the fallback match
        // below still spells out.
        //
        // #1422: `key_name` is already the shared `render::engine_key_from_ui`
        // spelling (`"Page_Up"`/`"Page_Down"`, `"ISO_Left_Tab"`, …) — the same
        // one TUI's `engine_name()` produces — so there is no second,
        // GTK-local mapping to apply here. The old `map_gtk_key_name` /
        // `map_gtk_key_with_unicode` pair round-tripped `key_name` through a
        // GDK-spelled table (`"Page_Up"` -> `"PageUp"`, `"ISO_Left_Tab"` ->
        // `"BackTab"`) whose output every downstream consumer already accepts
        // in its *un*-mapped, `key_name` form too (`panels.rs`/`search.rs`/
        // `ext_panel.rs`/`source_control.rs` all dual-accept `"ISO_Left_Tab"`
        // since #1060; nothing reads plain `"PageUp"`/`"PageDown"` at all —
        // see issue #1422). `sc_unicode` collapses into `unicode` the same
        // way: for a `Key::Char` press `unicode` is already `Some(c)`
        // independent of `ctrl` (decoded once, above, when `key_name`/
        // `unicode` were built from the raw `UiEvent`), matching what
        // TUI's Source-Control arm (`shell_app.rs`) re-derives with `ctrl`
        // forced off; for a `Key::Named` press both were always `None`.
        let panel_key = key_name.as_str();
        let shared = render::dispatch_sidebar_panel_key(
            &mut self.engine.borrow_mut(),
            focus_route,
            panel_key,
            unicode,
            unicode,
            ctrl,
            alt,
        );
        if shared.is_some() && focus_route == render::FocusKeyRoute::ExtPanel {
            self.sync_plus_register_to_clipboard();
        }
        let panel_still_focused: Option<bool> = match shared {
            Some(still_focused) => Some(still_focused),
            None => match self.dispatch_focus_owner_residual(
                focus_route,
                &key_name,
                unicode,
                ctrl,
                ui_event,
            ) {
                Some(outcome) => match outcome {
                    Some(still_focused) => Some(still_focused),
                    None => return,
                },
                None => None,
            },
        };
        if let Some(still_focused) = panel_still_focused {
            self.focus_after_sidebar_key(still_focused);
            self.draw_needed.set(true);
            return;
        }

        // ── Shared Alt-modifier / VSCode-mode rung (#759 / #734 slice 4) ──
        // Bound to a local first, for the same reason the modal rung at the
        // top of this method is: a `RefCell::borrow_mut()` temporary in a
        // `match` scrutinee lives for the whole `match`.
        let alt_outcome = render::route_alt_key(
            &mut self.engine.borrow_mut(),
            &key_name,
            unicode,
            shift,
            ctrl,
            alt,
        );
        match alt_outcome {
            render::AltKeyOutcome::ResizeSidebar(delta) => {
                let current = ctx.shell().sidebar_width().round().max(0.0) as u16;
                let next = render::alt_resized_sidebar_width(current, delta);
                ctx.shell_mut().set_sidebar_width(next as f32);
                self.draw_needed.set(true);
                return;
            }
            render::AltKeyOutcome::Handled => {
                self.draw_needed.set(true);
                return;
            }
            render::AltKeyOutcome::Fallthrough => {
                // #1764: an Alt-modified *printable-character* key that no
                // rung above claimed must not reach `Engine::handle_key` as
                // if the modifier never existed — that function takes no
                // `alt` parameter at all, so the fallthrough below would
                // redeliver it as the *bare* key, which in Insert/Replace
                // mode means inserting the character literally. Treating it
                // as an implicit Escape instead (and discarding the letter)
                // is the better of the two unverifiable guesses: Vim mode
                // has no `<M-x>` mappings for a letter to mean anything
                // else, while a real pty collapsing a fast Escape-then-letter
                // into this exact chord is the documented #1763/#1764
                // mechanism — see `render::alt_mnemonic_open_allowed`'s own
                // doc for the matching other half of this fix (keeping the
                // menu bar from stealing the chord first). Escape is a
                // harmless no-op in `Normal` mode (though *not* in the
                // `Visual*` family, which `alt_mnemonic_open_allowed` also
                // admits and where Escape exits Visual mode — still the
                // right substitution there, since the alternative is
                // literal-text corruption, not a no-op either way), so this
                // is safe to apply across every mode this rung can see.
                //
                // Gated on `render::alt_chord_is_printable_char` (review
                // finding, #1764 round 1), not on `alt` alone: a real pty's
                // Escape-then-letter fusion only ever produces a
                // printable-char chord (`alt_chord_is_printable_char`'s own
                // doc), so narrowing to that shape keeps the #1764 fix while
                // restoring every named-key Alt fallthrough that predates it
                // — `Alt+Enter`/`Alt+BackSpace` in Insert mode,
                // `Alt+Up`/`Alt+Down` moving the cursor in Vim mode outside
                // VSCode mode, and `Alt+]`/`Alt+[`/`Home`/`End`/`Delete`/
                // `Page_Up`/`Page_Down` falling through exactly as
                // `route_alt_key`'s own arms document.
                //
                // Not gated on `menu_bar_toggleable` the way
                // `alt_mnemonic_open_allowed` is: on macOS GUI, `Option+
                // <letter>` is the system dead-key/special-character
                // modifier, so a printable-char Alt chord can arrive there
                // too without any pty involved. Pre-#1764 an `Option+e` in
                // Insert mode inserted `e` literally (already the wrong
                // character for that key combo); post-#1764 it exits Insert
                // mode instead, which is a more surprising failure in
                // isolation — but this crate has no `alt`-aware dead-key
                // decoding on any backend today, so "literal wrong
                // character" was never the correct behaviour to preserve
                // either. Left as a known, narrow trade-off (quadraui's
                // macOS backend's dead-key handling, if it grows one, is the
                // real fix) rather than threading a third backend-shaped
                // condition through this already-shared rung.
                if alt && render::alt_chord_is_printable_char(&key_name, unicode) {
                    let action = self.engine.borrow_mut().handle_key("Escape", None, false);
                    self.dispatch_engine_action(action, false);
                    self.draw_needed.set(true);
                    // No `run_post_key_epilogue(ctx)` call, matching every
                    // sibling early-return rung above (`ModalKeyRoute::Engine`
                    // included, which also dispatches an engine action and
                    // returns without it) — review finding confirmed: the
                    // epilogue's only work (sidebar autohide/focus,
                    // explorer-after-move refresh, quickfix scroll clamp) is
                    // driven by state a plain Insert→Normal Escape never
                    // touches, so skipping it here changes nothing it would
                    // have done.
                    return;
                }
            }
        }

        // ── Shared hover-popup copy rung (#762 / #734 slice 7) ─────────
        let hover_copy = render::route_hover_popup_copy(&self.engine.borrow(), &key_name, ctrl);
        if let Some(text) = hover_copy {
            let mut engine = self.engine.borrow_mut();
            if let Some(ref cb) = engine.clipboard_write {
                let _ = cb(text.as_str());
            }
            engine.message = "Hover text copied".to_string();
            drop(engine);
            self.draw_needed.set(true);
            return;
        }

        // ── Shared command-line selection rung (#816) ──────────────────
        // The keyboard side of a command/message-line mouse selection —
        // TUI's `handle_key_pressed` has run this since #762/#734 slice 7;
        // GTK never reached it because `cmd_sel` was TUI-only local state.
        // #816 moved it onto `Engine` and wired GTK's mouse handlers (see
        // `handle_mouse_click_msg` / `handle_mouse_drag_msg`) to populate it
        // via `CommandLineLayout::hit_test`, so the same rung now applies
        // here too. `Clear` deliberately falls through to `handle_key` below.
        {
            let sel = self.engine.borrow().cmd_sel.get();
            let route =
                render::route_cmdline_selection_key(&self.engine.borrow(), unicode, ctrl, sel);
            match route {
                render::CmdSelKeyRoute::Copy(text) => {
                    let engine = self.engine.borrow();
                    if !text.is_empty() {
                        if let Some(ref cb) = engine.clipboard_write {
                            let _ = cb(text.as_str());
                        }
                    }
                    engine.cmd_sel.set(None);
                    drop(engine);
                    self.draw_needed.set(true);
                    return;
                }
                render::CmdSelKeyRoute::Clear => self.engine.borrow().cmd_sel.set(None),
                render::CmdSelKeyRoute::Keep => {}
            }
        }

        let action = {
            let mut engine = self.engine.borrow_mut();
            let a = engine.handle_key(&key_name, unicode, ctrl);
            // After any key press in insert mode, reset the AI completion
            // debounce timer so a new suggestion fires after idle.
            if engine.mode == crate::core::Mode::Insert && engine.settings.ai_completions {
                engine.ai_completion_reset_timer();
            }
            a
        };

        self.dispatch_engine_action(action, false);
        self.draw_needed.set(true);

        // ── Shared post-key epilogue (#762 / #734 slice 7) ─────────────
        self.run_post_key_epilogue(ctx);
        self.draw_needed.set(true);
    }

    /// The Debug, Ai and Explorer halves of the focus-owner dispatch — the
    /// three arms [`render::dispatch_sidebar_panel_key`] hands back as `None`
    /// because they need state this backend owns (a live `Backend` for the
    /// DAP `SidebarSystem` and the AI `ChatController`; the explorer
    /// `DrawingArea`).
    ///
    /// `None` — not one of these three, keep dispatching.
    /// `Some(Some(still_focused))` — handled; run the focus epilogue.
    /// `Some(None)` — handled completely; the caller must return.
    pub(crate) fn dispatch_focus_owner_residual(
        &mut self,
        route: render::FocusKeyRoute,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        ui_event: &quadraui::UiEvent,
    ) -> Option<Option<bool>> {
        match route {
            render::FocusKeyRoute::Debug => {
                // #1422: pass the real `ui_event` straight to `SidebarSystem::
                // handle` instead of reconstructing a `UiEvent` from `key_name`
                // through the old `gtk_key_name_to_quadraui` table (which only
                // covered a dozen named/nav keys and silently skipped `.handle`
                // for everything else). Mirrors TUI's identical, unconditional
                // `.handle(ui_event, backend, rect)` call for this route
                // (`shell_app.rs`'s `handle_focus_owner_key`) — see
                // `dispatch_dap_sidebar_event`'s own doc: it already reports
                // `Ignored` as `false` so an unrecognised key still falls
                // through to `dispatch_dap_sidebar_action_key` below exactly
                // as before.
                let mut engine = self.engine.borrow_mut();
                let rect = engine.dap_sidebar_body_rect.get();
                render::populate_dap_sidebar_system(&engine);
                let backend_rc = self.backend.clone();
                let sidebar_event = engine.dap_sidebar_system.borrow_mut().handle(
                    ui_event,
                    &mut **backend_rc.borrow_mut(),
                    rect,
                );
                let consumed = engine.dispatch_dap_sidebar_event(sidebar_event);
                if !consumed {
                    engine.dispatch_dap_sidebar_action_key(key_name);
                }
                Some(Some(engine.dap_sidebar_has_focus))
            }
            render::FocusKeyRoute::Ai => {
                // Unlike Debug (nav-only), the AI panel's `ChatController`
                // needs the real, un-round-tripped `UiEvent` — `KeyPressed`
                // *or* `CharTyped` — so free-form typed text reaches its
                // input buffer.
                let mut engine = self.engine.borrow_mut();
                let rect = engine.ai_chat_rect.get();
                let theme = render::Theme::from_name(&engine.settings.colorscheme);
                let backend_rc = self.backend.clone();
                // Re-apply the metrics `render()` painted the panel with —
                // see `cached_ai_chat_metrics`'s doc for why this can't be
                // skipped.
                let metrics = self.cached_ai_chat_metrics.get();
                {
                    let mut b = backend_rc.borrow_mut();
                    quadraui::Backend::set_current_line_height(&mut **b, metrics.0 as f32);
                    quadraui::Backend::set_current_char_width(&mut **b, metrics.1 as f32);
                }
                let still_focused = render::route_ai_chat_event(
                    &mut engine,
                    ui_event,
                    rect,
                    &theme,
                    &mut **backend_rc.borrow_mut(),
                );
                Some(Some(still_focused))
            }
            render::FocusKeyRoute::Explorer => {
                // Explorer keys used to be routed through a per-DrawingArea
                // key controller when the DA had focus (#732 retired the
                // `Msg` variant it sent; nothing has produced it since #540).
                self.handle_explorer_da_key(key_name.to_string(), unicode, ctrl);
                self.draw_needed.set(true);
                Some(None)
            }
            _ => None,
        }
    }

    /// Reconcile the runner's own `AppShell` (`ctx.shell()`/`ctx.shell_mut()`)
    /// with `engine.app_shell`'s (the "shadow" copy's) current sidebar
    /// visibility, pushing `show_panel`/`hide_sidebar` through `ctx` if the
    /// two have drifted.
    ///
    /// #1057: extracted out of [`Self::run_post_key_epilogue`] (its
    /// original, and until now only, caller — see that method's own doc for
    /// *why* the two copies need reconciling at all) so
    /// [`Self::on_shell_event_ctx`] can call it too. That second call site
    /// exists because of a gap this issue's fix exposed: `AppShell` never
    /// runs its own toggle for a *bottom* item (`BottomItemClicked`) — it
    /// only ever reports the click, both directions — so
    /// `on_shell_event`'s `BottomItemClicked` arm is 100% responsible for
    /// deciding the new visibility, entirely inside `engine.app_shell`. Left
    /// unsynced, the runner's own `AppShell` (which is what actually
    /// determines whether `render_content`'s sidebar column exists in the
    /// composited frame — `engine.app_shell` only decides *which panel's
    /// content* to paint inside it) never learns the sidebar closed: a
    /// second Settings click flipped `engine.app_shell.sidebar_visible()` to
    /// `false` correctly, but the runner kept laying out and painting the
    /// sidebar as if nothing had changed. Verified directly: before this
    /// method gained its `on_shell_event_ctx` call site, `bottom_item_
    /// second_click_collapses_sidebar`'s `gtk`/`tui` arms in `src/harness.rs`
    /// went red at the second-click assertion — the engine-side state
    /// (`sidebar_visible()`) was already correct, only the paint wasn't.
    pub(crate) fn sync_runner_sidebar_visibility(&self, ctx: &quadraui::ShellContext<'_>) {
        // #1427: moved to `render::sync_runner_sidebar_visibility`, shared
        // with the pre-#1434 TUI shell — see its own doc, including the #1029/#988
        // hamburger guard this method used to lack entirely (harmless on
        // GTK/macOS/Win, which never register the hamburger panel).
        render::sync_runner_sidebar_visibility(&self.engine.borrow(), ctx);
    }

    /// GTK's half of the shared after-every-editor-keypress epilogue.
    /// [`render::post_key_epilogue`] applies everything `Engine` owns; this
    /// applies the residues that need GTK: macro playback (whose
    /// `EngineAction`s only `dispatch_engine_action` can run), the runner ↔
    /// shadow sidebar-visibility sync (below), the deferred clipboard write,
    /// and the GLib one-shot behind the yank highlight.
    ///
    /// `ctx` is threaded in for that visibility sync: `render::post_key_epilogue`'s
    /// autohide arm calls `engine.app_shell.hide_sidebar()`, but
    /// `engine.app_shell` is only the "shadow" copy of shell state — GTK's
    /// actual painted layout (whether the sidebar column exists at all) comes
    /// from the runner's own `AppShell`, reachable solely through
    /// `ShellContext::shell_mut` (see `handle_key_press`'s doc comment on
    /// `ctx`, and TUI's identical shadow/runner split documented on
    /// the pre-#1434 TUI shell's `on_shell_event`). Without pushing the change through,
    /// `should_autohide_sidebar` flips a flag nothing paints from and the
    /// sidebar visually stays open.
    pub(crate) fn run_post_key_epilogue(&mut self, ctx: &quadraui::ShellContext<'_>) {
        loop {
            let (has_more, action) = {
                let mut engine = self.engine.borrow_mut();
                engine.advance_macro_playback()
            };
            self.dispatch_engine_action(action, true);
            if !has_more {
                break;
            }
        }

        // GTK recomputes the quickfix scroll offset statelessly each frame
        // (`draw_bottom_chrome`), so it hands the rung no scroll field.
        let epilogue = render::post_key_epilogue(&mut self.engine.borrow_mut(), None);
        if epilogue.focus_sidebar {
            let current = self.current_active_panel_id();
            let panel_id = if is_ext_panel_id(&current) {
                PANEL_EXPLORER.to_string()
            } else {
                current
            };
            self.engine.borrow_mut().focus_sidebar_panel(&panel_id);
            self.sync_sidebar_from_engine();
        } else if epilogue.focus_activity_bar {
            // New on GTK (#762): the overflow arm used to call
            // `focus_sidebar_panel` unconditionally, so with no sidebar
            // visible the keypress went nowhere. The shared rung has already
            // put the cursor on the activity bar; this just re-syncs.
            self.sync_sidebar_from_engine();
        }

        // Sync the unnamed register to the system clipboard if it changed.
        // The comparison is O(1); actual write is deferred to the background thread.
        self.sync_plus_register_to_clipboard();

        // ── Runner ↔ shadow sidebar-visibility sync (#762) ──────────────
        // The only place above that flips *visibility* (as opposed to which
        // panel/focus owns an already-visible sidebar) is the autohide arm
        // inside `render::post_key_epilogue`, which has no field of its own
        // to report through — so this just reconciles the two copies
        // unconditionally, the same way TUI's `on_shell_event` tail does.
        self.sync_runner_sidebar_visibility(ctx);

        // If a yank just happened, arm a 200 ms deadline; `tick_dispatch`
        // polls it and clears the highlight once it elapses (#813 — ported
        // off a one-shot toolkit timer, mirrors TUI's `yank_hl_deadline` in
        // `tui_main/shell_app.rs`).
        if epilogue.arm_yank_highlight {
            self.yank_hl_deadline.set(Some(
                std::time::Instant::now() + std::time::Duration::from_millis(200),
            ));
        }
    }
}

/// #1745: on the native macOS GUI, a real Cmd keypress reaches this crate as
/// `quadraui::Modifiers::cmd`, a bit `engine_key_from_ui`/`handle_key_press`/
/// every other `modifiers.ctrl` check in this file has never read — see
/// `tests/vscode_keybinding_parity.rs`'s corrected `macos_gui` column. So
/// before this fix Cmd+C/V/X/Z/S/P/… didn't merely *diverge* from VS Code's
/// Mac defaults, they did **nothing at all**: `ctrl` stayed `false` and the
/// plain, unmodified character fell through to whatever Insert-mode typing
/// does with it.
///
/// Folds `cmd` into `ctrl` for a `KeyPressed` event, but only when the live
/// backend's own [`quadraui::PlatformServices::platform_name`] — a runtime
/// capability query, not a `cfg!(target_os = "macos")` guess — reports
/// `"macos"`. That is true only for `quadraui::macos::MacBackend`: GTK's own
/// Cmd-reporting convention (Super/Meta -> `cmd`, quadraui's
/// `gtk/events.rs`) is left completely alone, so a GNOME user's Super key
/// does not suddenly start acting like Ctrl. This is the one shared
/// dispatcher both GTK and the macOS GUI already run through (`App::handle`
/// -> `handle_dispatch`), so the fix lives here once rather than in any
/// backend-specific file — see this file's own module doc and CLAUDE.md's
/// Platform-Neutrality Rule.
///
/// Mouse events are deliberately left untouched: the issue this fixes
/// (#1745) is scoped to VS Code mode's keyboard chords, and folding `cmd`
/// into the `MouseButton::Left if modifiers.ctrl` go-to-definition chord
/// would be a second, separate behaviour change with no test coverage here.
///
/// ## Arrow keys are handled separately, and only in VS Code mode
///
/// VS Code's real Mac defaults do **not** treat Cmd+Arrow as a plain
/// Ctrl-to-Cmd substitution the way every letter/symbol chord above does:
/// Option (`alt`) is the word-wise-navigation modifier on Mac
/// (`cursorWordLeft`/`cursorWordRight`, vimcode's existing Ctrl+Left/Right),
/// while Cmd+Left/Right is line start/end (`cursorHome`/`cursorEnd`, plain
/// `Home`/`End`) and Cmd+Up/Down is document start/end
/// (`cursorTop`/`cursorBottom`, Ctrl+Home/Ctrl+End) — see
/// tests/vscode_keybinding_parity.rs's `cursorWordEndRight / cursorWordLeft` and
/// `cursorTop / cursorBottom` rows. A blanket `cmd -> ctrl` fold would make
/// Cmd+Right *word-move* (vimcode's Ctrl+Right), which is not what either
/// VS Code or this fix wants, so arrows are excluded from the fold above
/// and translated here instead — a key-identity translation (not just a
/// modifier fold), gated on VS Code mode specifically because that's this
/// issue's scope and `route_alt_key`'s own Alt+Left/Right handling (VS
/// Code's Win/Linux `navigateBack`/`navigateForward` default) is itself
/// VS-Code-mode-gated.
///
/// `alt` is cleared on the Option+Left/Right arm so `route_alt_key` (called
/// later in `handle_dispatch`, unconditionally whenever `alt` is set) sees
/// `alt == false` and takes its own `Fallthrough` path instead of also
/// claiming the chord as `navigateBack`/`navigateForward` — Mac's own
/// default for that command is Ctrl+-/Ctrl+Shift+- instead (tracked as a
/// still-open gap; see `tests/vscode_keybinding_parity.rs`'s `KNOWN_GAPS`).
pub(crate) fn normalize_mac_cmd_as_ctrl(
    event: quadraui::UiEvent,
    backend: &dyn quadraui::Backend,
    vscode_mode: bool,
) -> quadraui::UiEvent {
    use quadraui::{Key, Modifiers, NamedKey, UiEvent};
    if backend.services().platform_name() != "macos" {
        return event;
    }
    match event {
        // Option+Left/Right (no Cmd, no physical Ctrl already held): word
        // move, matching vimcode's existing Ctrl+Left/Right.
        UiEvent::KeyPressed {
            key: key @ (Key::Named(NamedKey::Left) | Key::Named(NamedKey::Right)),
            modifiers,
            repeat,
        } if vscode_mode && modifiers.alt && !modifiers.cmd && !modifiers.ctrl => {
            UiEvent::KeyPressed {
                key,
                modifiers: Modifiers {
                    ctrl: true,
                    alt: false,
                    ..modifiers
                },
                repeat,
            }
        }
        // Cmd+Left/Right (no Option): line start/end.
        UiEvent::KeyPressed {
            key: Key::Named(NamedKey::Left),
            modifiers,
            repeat,
        } if vscode_mode && modifiers.cmd && !modifiers.alt => UiEvent::KeyPressed {
            key: Key::Named(NamedKey::Home),
            modifiers,
            repeat,
        },
        UiEvent::KeyPressed {
            key: Key::Named(NamedKey::Right),
            modifiers,
            repeat,
        } if vscode_mode && modifiers.cmd && !modifiers.alt => UiEvent::KeyPressed {
            key: Key::Named(NamedKey::End),
            modifiers,
            repeat,
        },
        // Cmd+Up/Down (no Option): document start/end — `Home`/`End` with
        // an extra `ctrl` bit, the same shape `engine_key_from_ui`'s
        // `NamedKey::Home`/`End` arms already use for Ctrl+Home/Ctrl+End.
        UiEvent::KeyPressed {
            key: Key::Named(NamedKey::Up),
            modifiers,
            repeat,
        } if vscode_mode && modifiers.cmd && !modifiers.alt => UiEvent::KeyPressed {
            key: Key::Named(NamedKey::Home),
            modifiers: Modifiers {
                ctrl: true,
                ..modifiers
            },
            repeat,
        },
        UiEvent::KeyPressed {
            key: Key::Named(NamedKey::Down),
            modifiers,
            repeat,
        } if vscode_mode && modifiers.cmd && !modifiers.alt => UiEvent::KeyPressed {
            key: Key::Named(NamedKey::End),
            modifiers: Modifiers {
                ctrl: true,
                ..modifiers
            },
            repeat,
        },
        // Every other chord: fold `cmd` into `ctrl` unchanged (the plain
        // Ctrl-to-Cmd substitution that covers every letter/symbol VS Code
        // Mac default this file's own doc enumerates). Gated on
        // `vscode_mode`, same as the arrow arms above — this issue is
        // scoped to VS Code mode on the macOS GUI (#1745), and without the
        // gate it would also change default Vim-mode behaviour on Mac:
        // Cmd+W would enter vim's window-command prefix, Cmd+V would enter
        // visual-block, Cmd+D would scroll a half page, etc. Vim mode's own
        // Mac Cmd semantics (if any are ever wanted) are a separate,
        // untested design decision and not part of this fix.
        UiEvent::KeyPressed {
            key,
            mut modifiers,
            repeat,
        } if vscode_mode && modifiers.cmd => {
            modifiers.ctrl = true;
            UiEvent::KeyPressed {
                key,
                modifiers,
                repeat,
            }
        }
        other => other,
    }
}
