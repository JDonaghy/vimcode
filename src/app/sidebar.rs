use super::*;

impl App {
    /// Toggle the integrated terminal panel open/closed.
    pub(crate) fn toggle_terminal(&mut self) {
        let needs_new_tab = {
            let engine = self.engine.borrow();
            (!engine.terminal_open || !engine.terminal_has_focus)
                && engine.terminal_panes.is_empty()
        };
        if needs_new_tab {
            // Use the actual editor content width so the PTY matches the visible panel.
            let cols = self.terminal_panel_cols(self.painted_editor_content_width());
            let rows = self.engine.borrow().session.terminal_panel_rows;
            self.engine.borrow_mut().terminal_new_tab(cols, rows);
        } else {
            self.engine.borrow_mut().toggle_terminal();
        }
        self.draw_needed.set(true);
    }

    /// Toggle the "terminal maximized" state (panel fills editor area).
    pub(crate) fn toggle_terminal_maximize(&mut self) {
        // Phase B.2: route through engine's UiEvent dispatch — same
        // path as the keybinding above + the EngineAction handler
        // + the toolbar click handler.
        let ctx = crate::core::engine::UiEventContext {
            terminal_cols: self.terminal_panel_cols(self.painted_editor_content_width()),
            terminal_max_rows: self.terminal_maximize_target_rows(&self.engine.borrow()),
        };
        self.engine.borrow_mut().handle_ui_event(
            crate::core::engine::UiEvent::Accelerator(
                crate::core::engine::AcceleratorId::new("terminal.toggle_maximize"),
                quadraui::Modifiers::default(),
            ),
            ctx,
        );
        self.draw_needed.set(true);
    }

    /// This method is a named no-op: the menu bar is repainted every frame
    /// by `render_content` from engine state instead.
    pub(crate) fn sync_menu_overlay(&self) {}

    /// Dispatch a menu action by command string, as produced by
    /// `quadraui::MenuEvent::Activated`.
    pub(crate) fn handle_menu_action(&mut self, action: String) {
        match action.as_str() {
            "open_file_dialog" => {
                self.open_file_dialog();
            }
            "open_folder_dialog" => {
                self.open_folder_dialog();
            }
            "open_workspace_dialog" => {
                self.engine.borrow_mut().open_workspace_from_file();
                self.refresh_file_tree();
            }
            "save_workspace_as_dialog" => {
                self.save_workspace_as_dialog();
            }
            "openrecent" => {
                self.open_recent_dialog();
            }
            "find" => {
                self.engine.borrow_mut().open_find_replace();
                self.draw_needed.set(true);
            }
            "quit_menu" => {
                if self.engine.borrow().has_any_unsaved() {
                    self.show_quit_confirm();
                } else {
                    self.save_session_and_exit();
                }
            }
            // `dispatch_engine_action(_, false)` is exhaustive and handles every
            // variant this catch-all would name, ensuring real handling for all.
            _ => {
                let engine_action = self.engine.borrow_mut().dispatch_menu_action(&action);
                self.dispatch_engine_action(engine_action, false);
            }
        }
        self.sync_menu_overlay();
        self.draw_needed.set(true);
    }

    /// Effective sidebar visibility — reads directly from `engine.app_shell`.
    /// Replaces the former local cache so GTK and engine state can never drift.
    pub(crate) fn current_sidebar_visible(&self) -> bool {
        self.engine.borrow().app_shell.sidebar_visible()
    }

    /// Effective active panel id, accounting for ext-panel synthetic IDs.
    pub(crate) fn current_active_panel_id(&self) -> String {
        render::sidebar_owner(&self.engine.borrow()).panel_id_string()
    }

    /// Re-sync GTK widget tree from engine sidebar state.
    /// The cache is gone (engine.app_shell is the single source of truth)
    /// so this is now just a redraw trigger.
    pub(crate) fn sync_sidebar_from_engine(&mut self) {
        self.sync_sidebar_widgets();
    }

    /// Queue a redraw after sidebar visibility/focus state changes.
    ///
    /// Under the ShellApp runner there is no widget tree to sync —
    /// `render_content` repaints the whole sidebar from `engine.app_shell`
    /// every frame — so this is now just the redraw trigger.
    pub(crate) fn sync_sidebar_widgets(&mut self) {
        self.draw_needed.set(true);
    }

    /// Toggle sidebar visibility.
    pub(crate) fn toggle_sidebar_panel(&mut self) {
        self.engine.borrow_mut().toggle_sidebar();
        self.sync_sidebar_from_engine();
    }

    /// Switch the sidebar to a different panel.
    ///
    /// Ext-panel vs built-in bookkeeping is handled by
    /// `render::apply_activity_panel_switch`. The only thing left here is
    /// this backend's own widget re-sync, which differs by branch (a plugin
    /// panel does not move `app_shell.active_panel_id()`).
    pub(crate) fn switch_panel(&mut self, panel_id: String) {
        let is_ext = panel_id.starts_with("ext:");
        render::apply_activity_panel_switch(&mut self.engine.borrow_mut(), &panel_id);
        if is_ext {
            self.sync_sidebar_widgets();
        } else {
            self.sync_sidebar_from_engine();
        }
    }

    /// Refresh the file tree from the current working directory.
    pub(crate) fn refresh_file_tree(&mut self) {
        self.refresh_explorer();
        if let Some(path) = self.engine.borrow().file_path().cloned() {
            self.reveal_path_in_explorer(&path);
        }
        self.draw_needed.set(true);
    }

    /// Toggle focus between the explorer and the editor.
    pub(crate) fn toggle_focus_explorer(&mut self) {
        if self.engine.borrow().explorer_has_focus {
            self.engine.borrow_mut().explorer_has_focus = false;
        } else {
            let mut engine = self.engine.borrow_mut();
            engine.ext_panel_active = None;
            engine.focus_sidebar_panel(PANEL_EXPLORER);
            drop(engine);
            self.sync_sidebar_widgets();
        }
        self.draw_needed.set(true);
    }

    /// Toggle focus between the search panel and the editor.
    pub(crate) fn toggle_focus_search(&mut self) {
        if self.current_active_panel_id() == PANEL_SEARCH && self.current_sidebar_visible() {
            // Just give the editor DA back keyboard focus.
        } else {
            let mut engine = self.engine.borrow_mut();
            engine.ext_panel_active = None;
            engine.focus_sidebar_panel(PANEL_SEARCH);
            drop(engine);
            self.sync_sidebar_widgets();
        }
        self.draw_needed.set(true);
    }

    /// Resolve a press against the Explorer header's view-actions toolbar row.
    ///
    /// `render::explorer_toolbar_hit_at` does the engine-only hit-test (button
    /// index); this wrapper converts `pos` to character-cell coordinates that
    /// `Engine::open_explorer_overflow_menu` needs to anchor the popup,
    /// using `self.cached_explorer_metrics`.
    ///
    /// Returns `false` (and does nothing) when `pos` doesn't land on a toolbar
    /// segment, so the caller falls through to the tree's own click routing.
    pub(crate) fn route_explorer_toolbar_click(&mut self, pos: quadraui::Point) -> bool {
        let idx = {
            let engine = self.engine.borrow();
            render::explorer_toolbar_hit_at(&engine, pos)
        };
        let Some(idx) = idx else {
            return false;
        };
        let mut engine = self.engine.borrow_mut();
        if idx == 4 {
            let (line_height, char_width) = self.cached_explorer_metrics.get();
            let col = (pos.x as f64 / char_width.max(1.0)) as u16;
            let row = (pos.y as f64 / line_height.max(1.0)) as u16;
            engine.open_explorer_overflow_menu(col, row, 1.0);
        } else {
            engine.explorer_activate_toolbar_action(idx);
        }
        // "Refresh" (idx 2) sets this immediately rather than waiting for
        // the next idle poll tick — same immediacy
        // `apply_context_menu_route`/`dispatch_context_menu_key` already
        // give a context-menu-driven explorer action.
        let needs_refresh = engine.explorer_needs_refresh;
        if needs_refresh {
            engine.explorer_needs_refresh = false;
        }
        drop(engine);
        if needs_refresh {
            self.refresh_file_tree();
        }
        self.queue_explorer_draw();
        self.draw_needed.set(true);
        true
    }

    /// `UiEvent` (scroll, mouse) over the explorer panel — routed through
    /// `TreeController::handle` for scrollbar interaction.
    ///
    /// The `TreeController` widget dispatch itself is
    /// [`render::route_explorer_tree_event`], shared with TUI. What stays
    /// here is GTK-only plumbing: which events this panel claims at all
    /// (`dominated`), pulling the metrics/backend/theme it needs, and
    /// draw-invalidation bookkeeping.
    pub(crate) fn explorer_ui_event(&mut self, ev: quadraui::UiEvent) {
        let dominated = matches!(
            ev,
            quadraui::UiEvent::MouseDown { .. }
                | quadraui::UiEvent::DoubleClick { .. }
                | quadraui::UiEvent::MouseUp { .. }
                | quadraui::UiEvent::Scroll { .. }
        ) || matches!(
            ev,
            quadraui::UiEvent::MouseMoved {
                buttons: quadraui::ButtonMask { left: true, .. },
                ..
            }
        );
        if !dominated {
            return;
        }
        let rect = self.engine.borrow().explorer_tree_rect.get();
        if rect.width <= 0.0 {
            return;
        }
        let theme = {
            let eng = self.engine.borrow();
            render::Theme::from_name(&eng.settings.colorscheme)
        };
        // Re-apply the metrics the tree was drawn with so the hit-test row
        // math matches the rendered rows. `set_current_line_height`/
        // `set_current_char_width` are inherent on `GtkBackend`, not trait
        // methods on `dyn Backend`, so they must be set from here rather
        // than inside the shared function.
        let metrics = self.cached_explorer_metrics.get();
        let backend_rc = self.backend.clone();
        let mut b = backend_rc.borrow_mut();
        quadraui::Backend::set_current_line_height(&mut **b, metrics.0 as f32);
        quadraui::Backend::set_current_char_width(&mut **b, metrics.1 as f32);
        let tree_event = {
            let mut engine = self.engine.borrow_mut();
            render::route_explorer_tree_event(&mut engine, &ev, rect, metrics, &theme, &mut **b)
        };
        drop(b);

        // `None` means either the event was fully resolved inside the tree routing
        // or the rect wasn't paintable; either way this panel already did everything it needs to.
        let Some(tree_event) = tree_event else {
            self.queue_explorer_draw();
            self.draw_needed.set(true);
            return;
        };
        // Shared empty-space right-click fallback, mirroring `tui_main::mouse`'s
        // right-click arm — see `route_tree_empty_space_context_menu`'s doc.
        if let quadraui::UiEvent::MouseDown {
            button: quadraui::MouseButton::Right,
            position,
            ..
        } = ev
        {
            render::route_tree_empty_space_context_menu(
                &mut self.engine.borrow_mut(),
                rect,
                metrics,
                position,
            );
        }
        if matches!(ev, quadraui::UiEvent::DoubleClick { .. }) {
            self.engine
                .borrow_mut()
                .dispatch_explorer_tree_event(tree_event);
        } else if matches!(ev, quadraui::UiEvent::MouseDown { .. }) {
            // Record a potential drag-and-drop source — mirrors
            // TUI's identical arm in `mouse::handle_mouse` — only a genuine
            // row selection (not a chevron toggle or a scrollbar drag) arms
            // one.
            if let quadraui::TreeControllerEvent::RowSelected { ref path } = tree_event {
                if let Some(&row_idx) = path.first() {
                    self.explorer_drag_src = Some(row_idx as usize);
                }
            }
            self.engine
                .borrow_mut()
                .handle_explorer_mouse_event(tree_event);
        }
        self.queue_explorer_draw();
        self.draw_needed.set(true);
    }

    /// Drop the server-side WM titlebar in favour of the drawn CSD row from
    /// `render_content`. Called from both `setup()` (fast path, usually too
    /// early) and `tick()` (reliable path — retried every frame until it succeeds).
    ///
    /// Gated on `!backend.backend_caps().native_menu`: a backend with a real
    /// OS menu bar (macOS's `MacBackend`) keeps its native titlebar too.
    /// The drawn CSD row only exists on backends that also draw their own menu bar.
    /// This is the same portable capability check `setup()` uses to decide whether
    /// to install the drawn menu bar at all.
    pub(crate) fn capture_window_and_apply_csd(&mut self, backend: &mut dyn quadraui::Backend) {
        if self.csd_applied.get() || backend.backend_caps().native_menu {
            return;
        }
        if let Some(w) = backend.window() {
            if w.set_decorated(false).is_ok() {
                self.csd_applied.set(true);
            }
        }
    }

    /// Restore the window's saved size, position and maximized state
    ///, the first time each run `Backend::window()` returns `Some`.
    /// Called from both `setup()` (fast path, usually too early — the
    /// runner hasn't called `window.present()` yet, so `backend.window()`
    /// is still `None`) and `tick()` (reliable path — retried every frame
    /// via `window_geometry_restored` until it succeeds), mirroring
    /// `capture_window_and_apply_csd`'s own identical two-call-site shape
    /// and doc.
    ///
    /// Order matters: size and (clamped) position are applied first, then
    /// maximized state last — `Backend::toggle_window_maximize` is a
    /// *toggle*, not a setter (`WindowControl` has no `maximize()`; see
    /// that trait's own doc), so it only flips from "restored" to
    /// "maximized" correctly if the restored geometry is already in place
    /// underneath it, the same way GTK's own `restore()` falls back to the
    /// pre-maximize size/position rather than a hardcoded one.
    ///
    /// `saved.x`/`y` are clamped against the live display list
    /// (`WindowGeometry::clamp_to_displays`) before use, so a monitor that
    /// was unplugged since the position was saved can never strand the
    /// restored window off-screen; an unclamped position (or no displays
    /// at all) falls back to `None`, leaving placement to the OS/window
    /// manager default. `set_size`/`set_bounds` failing (e.g. GTK's
    /// structural inability to reposition at all — see
    /// `WindowControl::set_bounds`'s own doc) is not an error here — every
    /// call is best-effort, exactly like `capture_window_and_apply_csd`'s
    /// own `set_decorated` call.
    pub(crate) fn restore_window_geometry(&mut self, backend: &mut dyn quadraui::Backend) {
        if self.window_geometry_restored.get() {
            return;
        }
        let saved = self.engine.borrow().session.window.clone();
        let displays = backend.services().displays().unwrap_or_default();
        let clamped = saved.clamp_to_displays(&displays);
        let already_maximized = {
            let Some(w) = backend.window() else {
                return;
            };
            let _ = w.set_size(clamped.width as f32, clamped.height as f32);
            if let Some((x, y)) = clamped.x.zip(clamped.y) {
                let _ = w.set_bounds(quadraui::Rect::new(
                    x as f32,
                    y as f32,
                    clamped.width as f32,
                    clamped.height as f32,
                ));
            }
            matches!(w.is_maximized(), Ok(true))
        };
        self.window_geometry_restored.set(true);
        if clamped.maximized && !already_maximized {
            backend.toggle_window_maximize();
        }
    }

    /// Forward a pointer event over the sidebar content area to the active panel's
    /// controller. In ShellApp mode the sidebar has no dedicated per-panel
    /// `DrawingArea`, so events must be routed here instead. Returns `true` when
    /// the event was consumed.
    ///
    /// The panel arms mirror `render_content`'s own match, each feeding the
    /// controller that painted the panel at the rect it painted into.
    /// A few panels (settings, extensions, debug/git) need extra bookkeeping or
    /// coordinate translation, but none re-derive hit geometry the painter doesn't
    /// already own.
    ///
    /// # Drag / release follow-through
    ///
    /// A press claimed here sets `sidebar_pointer_captured`, and while that is set
    /// subsequent `MouseMoved` (left held) / `MouseUp` are routed to the same panel
    /// so scrollbar thumbs and tree drags track the pointer. An unclaimed
    /// move/release is deliberately left alone, so an editor text-drag that crosses
    /// into the sidebar still finalizes through the editor's own mouse-up path.
    pub(crate) fn try_route_sidebar_mouse_event(
        &mut self,
        backend: &dyn quadraui::Backend,
        event: &quadraui::UiEvent,
        ctx: &quadraui::ShellContext<'_>,
    ) -> bool {
        use quadraui::UiEvent;

        let Some(sb) = ctx.layout.sidebar_content_bounds else {
            self.sidebar_pointer_captured.set(false);
            return false;
        };
        let dragging = self.sidebar_pointer_captured.get();
        // Once an explorer row has been picked up (`explorer_drag_src`)
        // or the drag is already active (`explorer_drag_active`), the *move*
        // and *release* that follow the initial press must reach
        // `handle_mouse_drag_msg`/`handle_mouse_up_msg` — the
        // `MouseDragRoute::ExplorerDnd` rung and `render::apply_explorer_drop`
        // — not loop back through here into `explorer_ui_event`, which would
        // hand a plain `MouseMoved` to `TreeController::handle` and get its
        // *scrollbar*-drag `drag_to`, never the row-under-pointer tracking a
        // DnD gesture needs. The press itself still claims capture as usual
        // (`explorer_ui_event` arms `explorer_drag_src` from that same press).
        let explorer_dnd_active =
            self.explorer_drag_src.is_some() || self.explorer_drag_active.is_some();
        let pos = match event {
            UiEvent::MouseDown { position, .. }
            | UiEvent::DoubleClick { position, .. }
            | UiEvent::Scroll { position, .. } => *position,
            // Follow-through only: never *start* an interaction from a move or
            // a release (see the doc comment above).
            UiEvent::MouseUp { position, .. } if dragging && !explorer_dnd_active => {
                self.sidebar_pointer_captured.set(false);
                *position
            }
            UiEvent::MouseMoved {
                position,
                buttons: quadraui::ButtonMask { left: true, .. },
            } if dragging && !explorer_dnd_active => *position,
            _ => return false,
        };
        // A captured drag keeps its grab even when the pointer leaves the
        // sidebar — otherwise dragging a scrollbar thumb sideways would silently
        // hand the rest of the gesture to the editor.
        let starts_interaction =
            !matches!(event, UiEvent::MouseUp { .. } | UiEvent::MouseMoved { .. });
        if starts_interaction
            && (pos.x < sb.x
                || pos.x >= sb.x + sb.width
                || pos.y < sb.y
                || pos.y >= sb.y + sb.height)
        {
            return false;
        }
        // Only a *press* moves keyboard focus into the panel. A wheel notch is
        // deliberately excluded: hovering-and-scrolling must not steal focus,
        // the same rule the editor's own wheel path follows.
        let is_press = matches!(
            event,
            UiEvent::MouseDown { .. } | UiEvent::DoubleClick { .. }
        );

        // An open picker / command palette is painted *over* the sidebar and
        // owns every press while it is up. `render_content` centres the
        // popup on the whole window, so with the sidebar open its left half
        // sits on top of the explorer tree — and without this the tree's row
        // hit-test underneath ate those presses before they could reach
        // `handle_mouse_click_msg`'s picker block. The dropdown a breadcrumb
        // click opens therefore looked completely inert on its left half:
        // rows highlighted nothing, selection never moved.
        //
        // Falling through is also what makes *dismissal* correct: a press on
        // the sidebar while the picker is up reaches the picker's own
        // modal-stack dispatch, which resolves it as an outside-click and
        // closes the popup (rather than silently driving the tree beneath it).
        if self.engine.borrow().picker_open {
            return false;
        }

        // Same reasoning as the picker above, for
        // the change-review surface — full-viewport, so its diff pane sits
        // squarely on top of the sidebar body underneath. Without this, a
        // click on the diff's left pane (which happens to fall inside
        // `sidebar_content_bounds`, since the surface deliberately doesn't
        // resize/hide the sidebar it's painted over) drove the sidebar's own
        // `TreeController`/panel row hit-test instead of ever reaching
        // `handle_mouse_click_msg`'s change-review block further down —
        // `route_and_apply_change_review_click` was correct but unreachable
        // for any click whose *coordinates* happened to land in that band.
        if self.engine.borrow().change_review.is_some() {
            return false;
        }

        // An engine-drawn context menu takes priority over the sidebar's own click
        // routing. Without this check a click on the menu fell through to the tree
        // underneath. Only a left press drives the menu's own hit-test/dismissal;
        // any other press/double-click/scroll while a menu is open is swallowed here.
        if self.engine.borrow().context_menu.is_some() {
            if matches!(
                event,
                UiEvent::MouseDown {
                    button: quadraui::MouseButton::Left,
                    ..
                }
            ) {
                self.dispatch_context_menu_click(backend, pos.x as f64, pos.y as f64);
            }
            self.draw_needed.set(true);
            return true;
        }

        // Which panel owns the sidebar body? `render::sidebar_owner` states that
        // precedence once so the click router, hover router and painter never disagree.
        let owner = render::sidebar_owner(&self.engine.borrow());

        let consumed = match &owner {
            render::SidebarOwner::Explorer => {
                // The view-actions toolbar row sits above the tree
                // (`paint_sidebar_panel_rung`'s `PANEL_EXPLORER` arm), so a
                // press is checked against it first — mirrors
                // `Self::route_debug_sidebar_event`'s chrome-band check.
                // Only a genuine press/release (`starts_interaction`) can
                // open the overflow menu or run an action; a drag
                // follow-through always falls to the tree.
                if !starts_interaction || !self.route_explorer_toolbar_click(pos) {
                    self.explorer_ui_event(event.clone());
                }
                true
            }
            render::SidebarOwner::Search => {
                let mut engine = self.engine.borrow_mut();
                if is_press {
                    engine.search_set_focus(true);
                }
                engine.handle_search_sidebar_ui_event(event.clone());
                true
            }
            render::SidebarOwner::Debug => {
                self.route_debug_sidebar_event(event, pos, starts_interaction)
            }
            render::SidebarOwner::Git => {
                self.route_sc_sidebar_event(event, pos, starts_interaction)
            }
            render::SidebarOwner::Extensions => {
                let mut engine = self.engine.borrow_mut();
                if is_press {
                    engine.ext_sidebar_has_focus = true;
                }
                engine.handle_ext_sidebar_ui_event(event.clone());
                if matches!(event, UiEvent::DoubleClick { .. }) {
                    engine.ext_open_selected_readme();
                }
                true
            }
            render::SidebarOwner::Settings => {
                let mut engine = self.engine.borrow_mut();
                if is_press {
                    engine.settings_has_focus = true;
                }
                // The position is inside the sidebar's content bounds, so even a click on
                // empty panel padding belongs to this panel, not the editor underneath.
                // The form rect omits the search row above it (added per quadraui#1061),
                // which `paint_sidebar_panel_rung` reserves one row above the form.
                let settings_rect = engine.settings_form_rect.get();
                render::handle_settings_form_ui_event(&mut engine, event, settings_rect);
                true
            }
            render::SidebarOwner::ExtPanel(_) => {
                // `render_content` now paints this panel through
                // `render::ext_panel_to_tree_view` — the same adapter TUI
                // uses — not `ext_sidebar_system` (the extension
                // marketplace), so routing goes through the shared
                // `render::route_ext_panel_click` router instead, built on
                // the `Backend::tree_layout` cached at paint time
                // (`Engine::ext_panel_tree_layout`) rather than a
                // hand-rolled row formula (this backend pitches a tree's
                // header rows shorter than its item rows — see that cache
                // field's own doc).
                let mut engine = self.engine.borrow_mut();
                if is_press {
                    engine.ext_panel_has_focus = true;
                }
                // A view-backed panel painted a `quadraui::Form`, so its
                // clicks resolve through `FormController` (and dispatch to the
                // plugin's `on_event`), not through the tree-row router.
                let is_view = engine
                    .ext_panel_active
                    .as_deref()
                    .is_some_and(|n| engine.is_plugin_view(n));
                // `handle_plugin_view_ui_event`'s own `bool` return is ignored
                // for the same reason the Settings arm ignores it: the position
                // is already known to be inside the sidebar, so even a click on
                // empty panel padding belongs here rather than leaking to the
                // editor underneath.
                match event {
                    // A `ViewBody`-kind view routes through its own
                    // matching primitive's click/scroll resolution instead
                    // of `FormController` — `render::route_plugin_view_body_
                    // event`, shared with the tab-hosted arm below. See the
                    // paint arm (`paint_sidebar_panel_rung`'s `ext:` case)
                    // for the matching kind branch.
                    _ if is_view => {
                        let name = engine.ext_panel_active.clone().unwrap_or_default();
                        let rect = engine.plugin_view_form_rect.get();
                        let backend_rc = self.backend.clone();
                        let mut b = backend_rc.borrow_mut();
                        let consumed = render::route_plugin_view_body_event(
                            &mut engine,
                            &name,
                            PluginViewHost::Sidebar,
                            event,
                            rect,
                            &mut **b,
                        );
                        drop(b);
                        if consumed.is_none() {
                            render::handle_plugin_view_ui_event(&mut engine, event, rect);
                        }
                    }
                    UiEvent::Scroll { delta, .. } => {
                        let flat_len = engine.ext_panel_flat_len();
                        let step = (delta.y.abs() * 3.0).round().max(1.0) as usize;
                        if delta.y > 0.0 {
                            // Positive y = up toward the top of the content
                            // (quadraui's convention for an already-resolved
                            // `UiEvent::Scroll`; see `handle_mouse_scroll_msg`'s
                            // #554 comment on the raw-vs-quadraui polarity
                            // split this event predates).
                            engine.ext_panel_scroll_top =
                                engine.ext_panel_scroll_top.saturating_sub(step);
                        } else {
                            engine.ext_panel_scroll_top = (engine.ext_panel_scroll_top + step)
                                .min(flat_len.saturating_sub(1));
                        }
                    }
                    UiEvent::DoubleClick { .. } => {
                        render::route_ext_panel_click(&mut engine, pos, true);
                    }
                    UiEvent::MouseDown {
                        button: quadraui::MouseButton::Left,
                        ..
                    } => {
                        render::route_ext_panel_click(&mut engine, pos, false);
                    }
                    // Right-click context menu and mid-drag follow-through aren't wired yet;
                    // still consumed below so neither leaks through to the editor.
                    _ => {}
                }
                true
            }
            render::SidebarOwner::Ai => self.route_ai_sidebar_event(event, pos, starts_interaction),
            render::SidebarOwner::Board => {
                self.route_board_sidebar_event(event, pos, starts_interaction)
            }
            // Unknown panel id: nothing was painted, so there is nothing
            // for a click to hit — let it fall through rather than
            // swallow it.
            render::SidebarOwner::Unknown => false,
        };

        if consumed {
            if starts_interaction {
                self.sidebar_pointer_captured
                    .set(matches!(event, UiEvent::MouseDown { .. }));
            }
            self.draw_needed.set(true);
        }
        consumed
    }

    /// Sidebar routing for the Debug panel.
    ///
    /// `render_content` paints the title + action-button chrome and stashes its
    /// hit regions in `engine.dap_sidebar_action_hits` in absolute pixel space.
    /// Everything below the chrome goes to the shared `SidebarSystem` at the body
    /// rect it painted into ([`render::dispatch_dap_sidebar_body_event`]).
    pub(crate) fn route_debug_sidebar_event(
        &mut self,
        event: &quadraui::UiEvent,
        pos: quadraui::Point,
        starts_interaction: bool,
    ) -> bool {
        let body_rect = self.engine.borrow().dap_sidebar_body_rect.get();
        if body_rect.width <= 0.0 {
            return false;
        }
        let mut engine = self.engine.borrow_mut();
        if starts_interaction {
            engine.dap_sidebar_has_focus = true;
        }
        // Chrome band (title + action row) — above the body rect.
        if starts_interaction && pos.y < body_rect.y {
            render::dap_sidebar_action_click_at(&mut engine, pos);
            // Claimed either way: the press landed on this panel's own chrome,
            // so it must not leak through to the editor beneath.
            return true;
        }
        let backend_rc = self.backend.clone();
        render::dispatch_dap_sidebar_body_event(
            &mut engine,
            event,
            body_rect,
            &mut **backend_rc.borrow_mut(),
        );
        true
    }

    /// Sidebar routing for the git ("source control") panel.
    ///
    /// The panel is three stacked bands — header, commit-message input, and
    /// toolbar + change sections. `render_content` derives them and caches the
    /// result here. The dispatch itself is [`render::route_sc_sidebar_click`],
    /// shared with TUI.
    pub(crate) fn route_sc_sidebar_event(
        &mut self,
        event: &quadraui::UiEvent,
        pos: quadraui::Point,
        starts_interaction: bool,
    ) -> bool {
        let Some(bands) = self.cached_sc_bands.get() else {
            return false;
        };
        let mut engine = self.engine.borrow_mut();
        render::route_sc_sidebar_click(&mut engine, event, pos, &bands, starts_interaction);
        true
    }

    /// Sidebar routing for the Board panel.
    ///
    /// `render::route_board_click` resolves the press against the
    /// `quadraui::BoardLayout` cached at paint time — the same "paint caches,
    /// click reads" contract as `Engine::ext_panel_tree_layout`. Consumed
    /// unconditionally like every other panel arm, so clicks don't leak to the
    /// editor. Right-click pixel→cell conversion uses `self.metrics` since
    /// `Engine::open_board_context_menu` expects cell coordinates.
    pub(crate) fn route_board_sidebar_event(
        &mut self,
        event: &quadraui::UiEvent,
        pos: quadraui::Point,
        starts_interaction: bool,
    ) -> bool {
        let mut engine = self.engine.borrow_mut();
        if starts_interaction {
            engine.board_has_focus = true;
        }
        match event {
            quadraui::UiEvent::DoubleClick { .. } => {
                render::route_board_click(&mut engine, pos, true);
            }
            quadraui::UiEvent::MouseDown {
                button: quadraui::MouseButton::Left,
                ..
            } => {
                render::route_board_click(&mut engine, pos, false);
            }
            quadraui::UiEvent::MouseDown {
                button: quadraui::MouseButton::Right,
                ..
            } => {
                if let Some(card_id) = render::board_right_click_card(&engine, pos) {
                    let cw = self.cached_char_width.max(1.0);
                    let lh = self.cached_line_height.max(1.0);
                    let cx = (pos.x as f64 / cw) as u16;
                    let cy = (pos.y as f64 / lh) as u16;
                    engine.open_board_context_menu(card_id, cx, cy);
                }
            }
            _ => {}
        }
        true
    }

    /// Sidebar routing for the AI panel.
    ///
    /// `render_content` caches the panel rect in `Engine::ai_chat_rect` at
    /// paint time — resolving a press against that means the click router
    /// can never derive a different layout than the one actually on screen
    ///. The dispatch itself is [`render::route_ai_chat_event`],
    /// which needs a live `Backend` (like [`Self::route_debug_sidebar_event`])
    /// for `ChatController::handle`'s own layout/hit-test math. Consumes the
    /// press unconditionally like every other panel arm in
    /// `try_route_sidebar_mouse_event` — a click on empty panel padding still
    /// belongs to this panel, not the editor underneath it.
    pub(crate) fn route_ai_sidebar_event(
        &mut self,
        event: &quadraui::UiEvent,
        pos: quadraui::Point,
        starts_interaction: bool,
    ) -> bool {
        let mut engine = self.engine.borrow_mut();
        let rect = engine.ai_chat_rect.get();
        let plan_rect = engine.ai_plan_rect.get();
        if rect.width <= 0.0 && plan_rect.width <= 0.0 {
            return false;
        }
        if starts_interaction {
            engine.ai_has_focus = true;
            // A mouse click into the panel isn't part of any
            // keyboard `<leader>ai` gesture, so it breaks one exactly like a
            // Named key would — discard rather than replay, since a click
            // (unlike a keystroke) has no natural place in the input to
            // insert buffered text.
            engine.ai_leader_toggle_pending.clear();
            // A press landing on the pinned plan block's band
            // toggles its collapse state instead of reaching `ChatController`
            // — checked first (before the metrics re-apply/`route_ai_chat_
            // event` call below) exactly like `route_board_sidebar_event`'s
            // right-click resolves against its own cached layout ahead of
            // falling through to generic nav.
            if plan_rect.width > 0.0
                && plan_rect.height > 0.0
                && pos.y >= plan_rect.y
                && pos.y < plan_rect.y + plan_rect.height
            {
                render::route_ai_plan_band_click(&mut engine, pos);
                return true;
            }
        }
        if rect.width <= 0.0 {
            return true;
        }
        let theme = render::Theme::from_name(&engine.settings.colorscheme);
        let backend_rc = self.backend.clone();
        // Re-apply the metrics `render()` painted the panel with — see
        // `cached_ai_chat_metrics`'s doc for why this can't be skipped.
        let metrics = self.cached_ai_chat_metrics.get();
        {
            let mut b = backend_rc.borrow_mut();
            quadraui::Backend::set_current_line_height(&mut **b, metrics.0 as f32);
            quadraui::Backend::set_current_char_width(&mut **b, metrics.1 as f32);
        }
        render::route_ai_chat_event(
            &mut engine,
            event,
            rect,
            &theme,
            &mut **backend_rc.borrow_mut(),
        );
        true
    }

    pub(crate) fn handle_explorer_da_key(
        &mut self,
        key_name: String,
        unicode: Option<char>,
        ctrl: bool,
    ) {
        // The explorer-ctx-menu intercept and dialog patch-up that used to open
        // this function are gone — both were local re-statements that
        // `render::route_modal_key` now resolves at the top of `handle_key_press`.

        // Panel-nav shortcuts before engine dispatch.
        let (pk_toggle, pk_explorer, pk_search) = {
            let eng = self.engine.borrow();
            (
                eng.settings.panel_keys.toggle_sidebar.clone(),
                eng.settings.panel_keys.focus_explorer.clone(),
                eng.settings.panel_keys.focus_search.clone(),
            )
        };
        let printable = match (ctrl, unicode) {
            (true, Some(c)) => format!("Ctrl-{}", c.to_ascii_uppercase()),
            (false, Some(c)) => c.to_string(),
            _ => key_name.clone(),
        };
        if printable == pk_toggle {
            self.toggle_sidebar_panel();
            return;
        }
        if printable == pk_explorer {
            self.toggle_focus_explorer();
            return;
        }
        if printable == pk_search {
            self.toggle_focus_search();
            return;
        }

        use crate::core::engine::ExplorerKeyResult;
        let result = self
            .engine
            .borrow_mut()
            .dispatch_explorer_key(&key_name, unicode, ctrl);

        match result {
            ExplorerKeyResult::Unfocused => {
                self.engine.borrow_mut().explorer_has_focus = false;
            }
            ExplorerKeyResult::FocusToolbar => {
                // engine.activity_bar_focus_in_at(1) was already called inside
                // dispatch_explorer_key. Redraw the activity bar for the
                // selection highlight; key events route through the editor DA
                // whose handle_key_press checks activity_bar_focused and
                // dispatches to handle_activity_bar_key. The activity bar DA
                // has no EventControllerKey, so grab_focus on it drops keys.
                self.engine.borrow_mut().explorer_has_focus = false;
            }
            _ => {}
        }
        self.queue_explorer_draw();
        self.draw_needed.set(true);
    }

    /// Was a redraw hint on `self.explorer_sidebar_da_ref`, permanently
    /// `None` under the ShellApp runner. `render_content` repaints the whole
    /// sidebar from engine state every frame, so callers only need
    /// `self.draw_needed.set(true)` — kept as a named no-op (rather than
    /// touching every call site) so the intent at each call site stays
    /// legible.
    pub(crate) fn queue_explorer_draw(&self) {}

    /// After a sidebar panel processes a key, queue a redraw of the activity
    /// bar if the engine just set `activity_bar_focused`, and in all cases
    /// give GTK widget focus to the editor DA so its `handle_key_press` can
    /// route the next key via engine flags (`activity_bar_focused`,
    /// `ext_panel_has_focus`, …).
    ///
    /// Why the editor DA, not the activity bar DA?  The activity bar DA has
    /// no `EventControllerKey`; routing GTK focus there drops subsequent key
    /// events.  The editor DA's capture-phase controller checks engine focus
    /// flags and dispatches to `handle_activity_bar_key` when needed — the
    /// same engine-flag routing that the TUI backend uses.
    ///
    /// `fallback_focused` is the "panel still has focus" flag passed through
    /// to `focus_editor_if_needed` when neither activity-bar nor editor focus
    /// applies (i.e. the sidebar panel kept focus → don't steal it).
    pub(crate) fn focus_after_sidebar_key(&self, fallback_focused: bool) {
        if self.engine.borrow().activity_bar_focused {
            // Activity bar has logical focus — `render_content` repaints it
            // every frame from engine state, so there's no separate redraw
            // hint to give here. Key routing flows through the editor DA.
            self.focus_editor_if_needed(false);
        } else {
            self.focus_editor_if_needed(fallback_focused);
        }
    }

    /// Handle a key press while the activity bar has keyboard focus. The key
    /// table itself is shared (`render::activity_bar_key_action`); this is the
    /// GTK sink for the actions it names.
    pub(crate) fn handle_activity_bar_key(&mut self, key_name: &str, ctrl: bool) {
        use render::ActivityBarKeyAction;
        match render::activity_bar_key_action(key_name, ctrl) {
            ActivityBarKeyAction::MoveDown => self.engine.borrow_mut().activity_bar_move_down(),
            ActivityBarKeyAction::MoveUp => self.engine.borrow_mut().activity_bar_move_up(),
            ActivityBarKeyAction::Activate => {
                use crate::core::engine::sidebar::ActivityBarActivation;
                let activation = self.engine.borrow_mut().activity_bar_activate();
                match activation {
                    // The menu bar is repainted every frame by `render_content` from engine state.
                    ActivityBarActivation::MenuToggled => self.draw_needed.set(true),
                    ActivityBarActivation::PanelFocused
                    | ActivityBarActivation::ExtPanelFocused(_) => {
                        self.sync_sidebar_from_engine();
                    }
                    ActivityBarActivation::NoOp => {}
                }
            }
            ActivityBarKeyAction::FocusOut => self.engine.borrow_mut().activity_bar_focus_out(),
            ActivityBarKeyAction::Collapse => {
                let mut engine = self.engine.borrow_mut();
                engine.activity_bar_focus_out();
                engine.collapse_sidebar();
            }
            ActivityBarKeyAction::Ignore => {}
        }
        // Suppress the default engine key handler — key is consumed.
    }

    /// The single sink for the shared context-menu key rung
    /// (`render::ModalKeyRoute::ContextMenu`). Dispatches the confirmed action,
    /// since `new_file` / `open_terminal` / `find_in_folder` need backend plumbing.
    /// The engine owns selection movement and menu navigation; only the final
    /// action dispatch is per-backend.
    pub(crate) fn dispatch_context_menu_key(&mut self, key_name: &str, unicode: Option<char>) {
        let effective_key = if key_name.is_empty() {
            unicode.map(|c| c.to_string()).unwrap_or_default()
        } else {
            key_name.to_string()
        };
        let target = self.engine.borrow().context_menu_target_path();
        let action = {
            let mut engine = self.engine.borrow_mut();
            let (_consumed, action) = engine.handle_context_menu_key(&effective_key);
            action
        };
        if let (Some(ref act), Some((ref path, is_dir))) = (action, target) {
            let engine_rc = self.engine.clone();
            // Was the `"open_terminal"` arm of the deleted
            // `App::dispatch_explorer_ctx_action`, which called the deleted
            // `App::open_terminal_at` — inlined here (rather than calling a
            // method) because that method reached for its own
            // `self.engine.borrow_mut()`, and this closure runs while
            // `engine_rc` is already borrowed mutably below; a second borrow
            // would panic (`RefCell` already mutably borrowed).
            render::apply_explorer_context_action(
                &mut engine_rc.borrow_mut(),
                act,
                path,
                is_dir,
                &mut |engine: &mut Engine, dir: std::path::PathBuf| {
                    let cols = self.terminal_panel_cols(self.painted_editor_content_width());
                    let rows = engine.session.terminal_panel_rows;
                    engine.terminal_new_tab_at(cols, rows, Some(&dir));
                    self.draw_needed.set(true);
                },
            );
        }
        let needs_refresh = {
            let mut engine = self.engine.borrow_mut();
            let r = engine.explorer_needs_refresh;
            engine.explorer_needs_refresh = false;
            r
        };
        if needs_refresh {
            self.refresh_file_tree();
        }
        self.queue_explorer_draw();
        self.draw_needed.set(true);
    }
}
