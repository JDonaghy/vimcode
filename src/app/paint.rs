use super::construction::app_icon_image_for_paint;
use super::*;

impl App {
    pub(crate) fn handle_poll_tick(&mut self, backend: &mut dyn quadraui::Backend) {
        // Schedule a redraw if the colorscheme changed (e.g. via `:colorscheme`).
        // The only remaining job is the redraw schedule; theming is now handled
        // per-frame by `sync_per_frame_backend_state`'s `Backend::set_theme` call.
        {
            let current = self.engine.borrow().settings.colorscheme.clone();
            if current != self.last_colorscheme {
                self.last_colorscheme = current;
                self.draw_needed.set(true);
            }
        }

        // Reload settings.json if it changed on disk via mtime poll,
        // run unconditionally every tick (same mechanism as TUI's own `tick`).
        self.settings_file_changed();

        // Explorer refresh after confirmed file move — GTK-only, no TUI
        // counterpart (see `Self::explorer_needs_refresh`'s own doc).
        if self.engine.borrow().explorer_needs_refresh {
            self.engine.borrow_mut().explorer_needs_refresh = false;
            self.refresh_file_tree();
        }

        // The rest of this function's chores — per-window viewport sync,
        // tab-visibility re-check, window title, yank-highlight clear,
        // idle/SC polling, deferred quit, terminal command, ext-panel focus,
        // platform-action drain — are shared with TUI's `tick`.
        // See `render::run_shared_tick_chores`'s header comment for the full list.
        let engine_rc = self.engine.clone();
        let needs_redraw = {
            let mut engine = engine_rc.borrow_mut();
            render::run_shared_tick_chores(&mut engine, self, backend)
        };
        if needs_redraw {
            self.draw_needed.set(true);
        }
    }

    /// Map a pixel x-offset within the editor hover popup's content
    /// area to a character column on `content_line`. The fallback uses
    /// approximate `rel_x / char-width`-style calculation because
    /// a live font-metrics source is unavailable.
    ///
    /// Run the shared editor-hover-popup rung against this frame's
    /// painted geometry and apply whatever it decides.
    ///
    /// Returns `true` when the press belonged to the popup and must not fall
    /// through to the editor. Called from `handle_mouse_click_msg` above
    /// the scroll-surface dispatch and from `handle_mouse_double_click_msg`.
    pub(crate) fn route_and_apply_editor_hover_popup(
        &self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
    ) -> bool {
        let (visible, has_focus) = {
            let engine = self.engine.borrow();
            (engine.editor_hover.is_some(), engine.editor_hover_has_focus)
        };
        let links = self.editor_hover_link_rects.borrow();
        let route = render::route_editor_hover_popup_click(
            visible,
            &render::EditorHoverPopupState {
                popup: self.editor_hover_popup_rect.get(),
                links: &links,
                scrollbar: self.editor_hover_scrollbar.get(),
                has_focus,
                // `units.hover_popup_pad` — was hardcoded to GTK's
                // 4px/4px inset even on the `tui` harness arm (`App` on
                // `TuiBackend`), disagreeing with that arm's own painted
                // 2-cell/1-cell popup frame.
                content: render::PopupContentMetrics {
                    pad_x: self.units.hover_popup_pad.0,
                    pad_y: self.units.hover_popup_pad.1,
                    col_width: self.cached_char_width.max(1.0) as f32,
                    line_height: self.cached_line_height.max(1.0) as f32,
                },
            },
            x,
            y,
        );
        let effect = render::apply_editor_hover_popup_route(&mut self.engine.borrow_mut(), route);
        if let Some(url) = effect.open_url {
            // `Engine::open_url` validates `is_safe_url` and queues a
            // `PendingPlatformAction::OpenUrl` for `tick_dispatch` to carry
            // out through `PlatformServices` — this method has no `backend`
            // handle of its own (see `Engine::pending_platform_actions`'s doc).
            self.engine.borrow_mut().open_url(&url);
        }
        if let Some(target) = effect.begin_drag {
            let drag_rc = backend.drag_state_handle();
            drag_rc.borrow_mut().begin(target);
            // Seek immediately, with the same thumb-aware math the drag
            // frames will use.
            let drag = drag_rc.borrow().clone();
            for ev in quadraui::dispatch_mouse_drag(
                &drag,
                quadraui::Point {
                    x: x as f32,
                    y: y as f32,
                },
                Default::default(),
            ) {
                if let quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset } = ev {
                    if widget.as_str() == "editor_hover" {
                        self.engine.borrow_mut().editor_hover_set_scroll(new_offset);
                    }
                }
            }
        }
        self.draw_needed.set(true);
        effect.consumed
    }

    /// Run the shared panel-hover-popup click rung against this
    /// frame's painted link rects.
    ///
    /// Returns `true` when the press landed on a link and must not fall
    /// through to whatever is painted underneath.
    pub(crate) fn route_and_apply_panel_hover_popup(&self, x: f64, y: f64) -> bool {
        let links = self.panel_hover_link_rects.borrow();
        let route = render::route_panel_hover_popup_click(&links, x, y);
        drop(links);
        if route == render::PanelHoverPopupRoute::None {
            return false;
        }
        let effect = render::apply_panel_hover_popup_route(&mut self.engine.borrow_mut(), route);
        if let Some(url) = effect.open_url {
            // See `route_and_apply_editor_hover_popup`'s identical
            // comment above — `Engine::open_url` validates + queues.
            self.engine.borrow_mut().open_url(&url);
        }
        self.draw_needed.set(true);
        effect.consumed
    }

    /// Popup-content column under `rel_x` (pixels from the content origin).
    ///
    /// Used by the hover-selection drag follow-through. Dividing by the same
    /// `col_width` ensures drag-selection inside the popup agrees with the
    /// column the press had chosen.
    pub(crate) fn pixel_to_editor_hover_col(&self, rel_x: f64, _content_line: usize) -> usize {
        (rel_x.max(0.0) / self.cached_char_width.max(1.0)) as usize
    }

    /// Push or pop the editor hover popup on the modal stack so
    /// click dispatch can decide modal-vs-base for both left- and
    /// right-clicks. The popup is registered whenever it's
    /// visible (focused or not) so right-clicks anywhere inside it
    /// stop falling through to the editor's context menu. Picker-
    /// style reconcile: `push` dedupes on id, so calling this every
    /// click is safe.
    pub(crate) fn reconcile_editor_hover_modal(&self, backend: &dyn quadraui::Backend) {
        let editor_hover_id = quadraui::WidgetId::new("editor_hover");
        let engine = self.engine.borrow();
        let visible = engine.editor_hover.is_some();
        let rect = self.editor_hover_popup_rect.get();
        drop(engine);
        let stack_rc = backend.modal_stack_handle();
        let mut stack = stack_rc.borrow_mut();
        match (visible, rect) {
            (true, Some(rect)) => {
                stack.push(editor_hover_id, rect);
            }
            _ => {
                stack.pop(&editor_hover_id);
            }
        }
    }

    /// Route a left-click against the currently open engine-drawn context
    /// menu — `engine.context_menu` is shared by the editor, tab-bar, and
    /// explorer sources, so this applies uniformly regardless of which one
    /// opened it.
    ///
    /// Returns `true` iff a menu was open and this call consumed the click
    /// (dismissed it, fired an item, or kept it open on an inert row) —
    /// the caller should treat that as "handled, stop routing". Returns `false`
    /// when no menu was open; the caller should then proceed with its own routing.
    pub(crate) fn dispatch_context_menu_click(
        &mut self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
    ) -> bool {
        let cm_id = quadraui::WidgetId::new("context_menu");
        if self.engine.borrow().context_menu.is_none() {
            // Defensive cleanup: the menu may have closed via Esc/Enter while
            // no click was seen by us. Pop any stale entry.
            backend.modal_stack_handle().borrow_mut().pop(&cm_id);
            return false;
        }

        // Keep the menu's painted bounds on the modal stack so any other modal
        // that might be open (picker, dialog) is arbitrated against it by the
        // *drag* guard, which still consults the stack.
        if let Some(bounds) = self.context_menu_layout.borrow().as_ref().map(|l| l.bounds) {
            backend
                .modal_stack_handle()
                .borrow_mut()
                .push(cm_id, bounds);
        }

        match self.route_modal_overlay(x, y, render::ModalMouseAction::LeftPress) {
            render::ModalOverlayRoute::ContextMenu(route) => {
                self.apply_context_menu_route(backend, route);
            }
            // A dialog or a toast outranks the menu; the shared router already
            // said so, and re-deciding that here is what let the two backends
            // drift in the first place.
            _ => self.draw_needed.set(true),
        }
        true
    }

    /// Editor content bounds + tab-bar height **as last painted**, in the
    /// absolute DA coordinate frame mouse events arrive in.
    ///
    /// Divider hit-testing must run against the geometry the renderer used, not
    /// a parallel re-derivation: `render_content` anchors `editor_bounds` at
    /// `AppShellLayout::main_content_bounds` (offset right by the activity
    /// bar/sidebar, down by the title-bar band), so any handler that rebuilt
    /// bounds at `(0.0, 0.0)` hit-tested a phantom divider displaced by that offset.
    ///
    /// `None` only before the first frame has been painted, when there is no
    /// divider on screen to hit anyway.
    pub(crate) fn painted_editor_bounds(&self) -> Option<(core::WindowRect, f64)> {
        self.cached_editor_bounds.get()
    }

    /// Left edge of the bottom panel as last painted — the same `x`
    /// `render_content` hands `draw_tab_bar` / the terminal pane, i.e. the
    /// editor's left edge, right of the activity bar and sidebar.
    ///
    /// [`render::BottomPanelMetrics::panel_left`]. Falls back to `0.0`
    /// before the first frame, when there is no panel on screen to click.
    pub(crate) fn painted_bottom_panel_left(&self) -> f64 {
        self.cached_editor_bounds
            .get()
            .map(|(r, _)| r.x)
            .unwrap_or(0.0)
    }

    /// Resolve `(x, y)` against the currently-painted `screen.windows`,
    /// returning the name and rect of the editor-tab-hosted plugin view
    /// under it, if any.
    ///
    /// Shared by every mouse route that needs to know whether a pixel
    /// belongs to a `vimcode.ui.register_view` view opened as an editor-area
    /// tab (`Engine::open_plugin_view_tab`) — press, double-click and wheel
    /// alike — so the three routes can't disagree about which window a
    /// pixel landed in. Walks the same `screen.windows` rects `click.rs`
    /// itself resolves clicks against.
    pub(crate) fn plugin_view_tab_hit(&self, x: f64, y: f64) -> Option<(String, quadraui::Rect)> {
        self.cached_screen_layout.borrow().as_ref().and_then(|l| {
            l.windows.iter().find_map(|w| {
                let name = w.plugin_view.clone()?;
                let rect: quadraui::Rect = w.rect.into();
                rect.contains(quadraui::Point::new(x as f32, y as f32))
                    .then_some((name, rect))
            })
        })
    }

    /// Both divider lists for the frame just painted, plus whether `(x, y)`
    /// lands on a group's tab bar — everything
    /// [`render::route_divider_grab`] needs from this backend.
    ///
    /// Derived from [`Self::painted_editor_bounds`] rather than a fresh
    /// drawing-area measurement, for coordinate-frame accuracy.
    ///
    /// `on_tab_bar` exists so a click on a group's tab bar reaches the tab
    /// handlers instead of arming a group-divider drag; it is deliberately
    /// GTK-only. It is also skipped entirely in single-group mode, where
    /// `group_dividers` is empty and nothing could match anyway.
    pub(crate) fn painted_divider_geometry(
        &self,
        x: f64,
        y: f64,
    ) -> Option<(
        Vec<core::window::GroupDivider>,
        Vec<core::window::WindowDivider>,
        bool,
    )> {
        let (content_bounds, tab_bar_h) = self.painted_editor_bounds()?;
        let engine = self.engine.borrow();
        let single = engine.group_layout.is_single_group();
        let group_dividers = if single {
            Vec::new()
        } else {
            engine.group_layout.dividers(content_bounds, &mut 0)
        };
        let on_tab_bar = !single
            && engine
                .group_layout
                .calculate_group_rects(content_bounds, tab_bar_h)
                .iter()
                .any(|(gid, grect)| {
                    if engine.is_tab_bar_hidden(*gid) {
                        return false;
                    }
                    let ty = grect.y - tab_bar_h;
                    y >= ty && y < ty + tab_bar_h && x >= grect.x && x < grect.x + grect.width
                });
        let (window_rects, _) = engine.calculate_group_window_rects(content_bounds, tab_bar_h);
        let window_dividers = engine.calculate_window_dividers(&window_rects);
        Some((group_dividers, window_dividers, on_tab_bar))
    }

    /// The [`render::EditorOp::Windows`] rung: paint every editor window's
    /// text plus its per-window status line, then the `:split`/`:vsplit`
    /// divider lines *within* each group.
    ///
    /// `window_editors` collects each window's owned `quadraui::Editor` for
    /// the caller's `FrameHitMap`, so the map hit-tests the SAME
    /// objects that were painted rather than a second copy that could drift.
    ///
    /// TUI's twin is `render_impl::render_all_windows`, which also paints its
    /// within-group separators (`render_separators`) from the same rung.
    pub(crate) fn paint_editor_windows_rung(
        &self,
        backend: &mut dyn quadraui::Backend,
        screen: &render::ScreenLayout,
        lh: f64,
        window_editors: &mut Vec<quadraui::Editor>,
    ) {
        use quadraui::{ScreenLayout as QSL, Surface};
        for rw in &screen.windows {
            let editor = render::to_q_editor(rw);
            let rect = editor.rect;

            // An editor-tab-hosted `vimcode.ui.register_view` view
            // paints a `Form`, not buffer text — the same shared
            // `quadraui::Form` primitive + adapter
            // (`render::plugin_view_to_form`) the sidebar body already paints
            // through, just routed through its own `FormController`/rect pair
            // (`Engine::plugin_view_tab_form_controller`/`_tab_form_rect`) so
            // a simultaneously-visible sidebar view can't clobber this one's
            // cached click geometry. `window_editors` still gets `editor`
            // (an empty placeholder — `rw.lines` is `vec![]` for a
            // plugin-view window, see `render::build_rendered_window`'s
            // short-circuit) so its indices stay 1:1 with `screen.windows`
            // for `compose_editor_band_rungs`' later `FrameHitMap` build.
            if let Some(view_name) = &rw.plugin_view {
                let engine = self.engine.borrow();
                engine.plugin_view_tab_form_rect.set(rect);
                // Same body-kind branch as the sidebar arm
                // (`paint_sidebar_panel_rung`'s `ext:` case) — see that
                // arm's comment for why `plugin_view_tab_form_rect` is
                // reused as "the rect the active body last painted into"
                // for every body kind, not just the field-stack `Form`.
                let kind = engine
                    .plugin_views
                    .get(view_name)
                    .and_then(|v| v.body.as_ref())
                    .map(|b| b.kind_name());
                match kind {
                    Some("list") => {
                        render::paint_plugin_view_list(
                            &engine,
                            view_name,
                            PluginViewHost::Tab,
                            rw.is_active,
                            rect,
                            backend,
                        );
                    }
                    Some("tree") => {
                        if render::populate_plugin_view_tree_controller(
                            &engine,
                            view_name,
                            PluginViewHost::Tab,
                            rw.is_active,
                        ) {
                            engine
                                .plugin_view_tab_tree_controller
                                .borrow()
                                .render(backend, rect);
                        }
                    }
                    Some("table") => {
                        render::paint_plugin_view_table(
                            &engine,
                            view_name,
                            PluginViewHost::Tab,
                            rw.is_active,
                            rect,
                            backend,
                        );
                    }
                    Some("text_view") => {
                        render::paint_plugin_view_text(
                            &engine,
                            view_name,
                            PluginViewHost::Tab,
                            rw.is_active,
                            rect,
                            backend,
                        );
                    }
                    _ => {
                        if render::populate_plugin_view_tab_form_controller(
                            &engine,
                            view_name,
                            rw.is_active,
                        ) {
                            engine
                                .plugin_view_tab_form_controller
                                .borrow_mut()
                                .render_and_cache(backend, rect);
                        }
                    }
                }
                drop(engine);
                window_editors.push(editor);
                continue;
            }

            let mut frame = QSL::new();
            frame.push(Surface::Editor {
                rect,
                editor: &editor,
            });
            frame.draw(backend);
            window_editors.push(editor);

            // Per-window status bar (when `window_status_line` is true, which
            // is the default; `global_status_bar` is None in that mode).
            let Some(ref status) = rw.status_line else {
                continue;
            };
            let bar_y = rw.rect.y + rw.rect.height - lh;
            let sb_rect = quadraui::Rect::new(
                rw.rect.x as f32,
                bar_y as f32,
                rw.rect.width as f32,
                lh as f32,
            );
            let win_bar = render::window_status_line_to_status_bar(
                status,
                quadraui::WidgetId::new(format!("status:{}", rw.window_id.0)),
            );
            // With exactly one window (no `:split`/`:vsplit`), paint
            // a plain backdrop fill edge-to-edge across the real window/
            // terminal width *before* the real (window-bounded) bar below —
            // VS Code's status bar runs under the activity bar and sidebar
            // too, not just the editor pane (see the issue's side-by-side
            // pixel sampling). `AppShell::render` paints the sidebar/
            // activity bar *before* `render_content` ever runs (see
            // `shell_adapter::render`), so this backdrop, painted here,
            // after, simply draws over their bottom edge, capping them —
            // with no `AppShellLayout`/sidebar-height change needed. See
            // `render::paint_status_backdrop`'s doc for why this is a
            // separate, content-free paint rather than widening `sb_rect`
            // itself. With two or more windows there is no single
            // VS-Code-shaped bar to backdrop this way — each split keeps
            // its own, Vim-style, window-bounded status line, unchanged.
            if screen.windows.len() == 1 {
                let real_bg = win_bar
                    .left_segments
                    .first()
                    .or(win_bar.right_segments.first())
                    .map(|s| s.bg);
                // In `status_bar_under_main_column` mode, the sidebar
                // runs full height — the backdrop must stay within
                // the window bounds, not extend under the sidebar.
                let backdrop_rect = sb_rect;
                render::paint_status_backdrop(
                    backend,
                    &format!("status-backdrop:{}", rw.window_id.0),
                    backdrop_rect,
                    real_bg,
                );
            }
            // Recover segment hit zones the same way `draw_status_bar` does, so
            // `pixel_to_click_target`'s `WindowZone::StatusBar` arm has a real
            // `status_segment_map` entry to resolve against.
            // `draw_status_bar` lays segments out bar-relative from `(0, 0)`
            // regardless of `sb_rect`'s own origin, which is exactly the
            // window-relative `local_x` `window_zone_hit_test` hit-tests with.
            // This is the layout the paint resolved, not a second re-measure.
            let sb_layout = backend.draw_status_bar_interactive(
                sb_rect,
                &win_bar,
                &quadraui::InteractionState::new(),
            );
            self.status_segment_map.borrow_mut().insert(
                rw.window_id.0,
                render::status_bar_zones_from_layout(&sb_layout),
            );
        }

        // Divider boundaries painted via quadraui's `Split` primitive.
        // These are the within-group dividers, part of this rung rather than
        // `EditorOp::GroupDividers` — the same split TUI makes.
        render::draw_dividers_as_splits(backend, &screen.window_dividers, |div| {
            quadraui::WidgetId::new(format!("wdiv:{}:{}", div.group_id.0, div.split_index))
        });
    }

    /// The [`render::EditorOp::TabBars`] rung: paint one tab bar per editor
    /// group and recover the pixel hit geometry the rasteriser resolved.
    ///
    /// Multi-group (post-split) layouts get a bar per group at the top edge of
    /// its own bounds; a single group gets one full-width bar at the editor top.
    ///
    /// Painting goes through `render::paint_tab_bars` →
    /// `Backend::draw_tab_bar_icons` rather than a `Surface::TabBar` push,
    /// because quadraui's `Surface` enum carries no icon sidecar. With an
    /// empty sidecar the two are byte-identical — quadraui's `draw_tab_bar`
    /// forwards to `draw_tab_bar_icons` with `&[]` — so this is a pure
    /// superset of the old call.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_sidebar_panel_rung(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        screen: &render::ScreenLayout,
        theme: &Theme,
        q_sb: quadraui::Rect,
        lh: f64,
        cw: f64,
    ) {
        // Which panel is active?
        let active_id: String = render::sidebar_owner(engine).panel_id_string();

        match active_id.as_str() {
            PANEL_EXPLORER => {
                // Composed through `SidebarPanelBody::render_with`
                // in a single call — `None`/`None` here
                // reproduce this arm's pre-existing "no chrome" behaviour
                // exactly (`layout.body_rect == q_sb`); TUI's
                // `panels::render_explorer_sidebar_content` uses the same
                // composer with `background: Some(tab_bar_bg)`. `render_with`
                // takes the body as a closure with no `Send + 'static` bound,
                // so the `!Send`, `Rc<RefCell<_>>`-backed `TreeController` on
                // `Engine` can be the body directly instead of the
                // hand-copied `render::paint_sidebar_panel_chrome` split
                // #1242 needed before #1059 existed.
                // `StatusBars` reserves one row above the tree for
                // the view-actions toolbar (New File / New Folder /
                // Refresh / Collapse All / "..." overflow) — the same
                // `SidebarPanelChrome` variant the Debug sidebar's title/
                // action bars already use (`debug_sidebar_chrome`, just
                // below). `body_rect` (what the closure receives) is
                // already narrowed below the chrome row, so
                // `explorer_tree_rect`/`explorer_viewport_rows` need no
                // change to account for it.
                let panel = render::SidebarPanelBody {
                    background: Some(theme.tab_bar_bg),
                    chrome: render::SidebarPanelChrome::StatusBars(vec![
                        render::explorer_toolbar_status_bar(theme),
                    ]),
                    scrollbar_gutter: None,
                };
                render::populate_explorer_tree_controller(engine, theme);
                // Capture the exact metrics the tree is drawn with so the
                // click hit-test (which reads the backend's mutable
                // current_line_height at a later, possibly-different time) can
                // re-apply them and resolve the correct row.
                self.cached_explorer_metrics
                    .set((backend.line_height() as f64, backend.char_width() as f64));
                let layout = panel.render_with(backend, q_sb, |backend, body_rect| {
                    engine.explorer_tree_rect.set(body_rect);
                    engine.explorer_viewport_rows.set(body_rect.height as usize);
                    engine.explorer_tree.borrow().render(backend, body_rect);
                });
                engine
                    .explorer_toolbar_hits
                    .replace(layout.status_bar_hit_regions);
            }
            PANEL_SEARCH => {
                // `search_sidebar_system` needs backend info set so `handle_cached`
                // returns real events instead of `Ignored` unconditionally.
                // Every content-row press and every wheel notch over the search results
                // list was silently no-op before this call was added.
                let search_lh = backend.line_height();
                engine
                    .search_sidebar_system
                    .borrow_mut()
                    .set_backend_info(search_lh, (self.units.sidebar_system_metrics)(search_lh));
                render::populate_search_sidebar_system(engine, &engine.cwd, theme);
                // Wrap in `SidebarPanelBody` so the sidebar background is
                // filled with `tab_bar_bg` before the search system renders.
                // Without this, unfilled cells default to the terminal
                // background on TUI (black), unlike every other panel.
                let panel = render::SidebarPanelBody {
                    background: Some(theme.tab_bar_bg),
                    chrome: render::SidebarPanelChrome::None,
                    scrollbar_gutter: None,
                };
                panel.render_with(backend, q_sb, |backend, body_rect| {
                    engine.search_sidebar_body_rect.set(body_rect);
                    engine.search_sidebar_system.borrow().render(backend, body_rect);
                });
            }
            PANEL_DEBUG => {
                // Composed through `SidebarPanelBody::render_with`
                // with `SidebarPanelChrome::StatusBars` instead of slicing `q_sb` into
                // rows by hand. The returned layout's `status_bar_hit_regions` are
                // already in `q_sb`'s own absolute pixel space and stored onto
                // `engine.dap_sidebar_action_hits`, eliminating a second independently-derived
                // copy of geometry that could disagree with what was painted.
                let panel = render::SidebarPanelBody {
                    background: Some(theme.tab_bar_bg),
                    chrome: render::debug_sidebar_chrome(&screen.debug_sidebar, theme),
                    scrollbar_gutter: None,
                };
                let layout = panel.render_with(backend, q_sb, |backend, body_rect| {
                    engine.dap_sidebar_body_rect.set(body_rect);
                    render::populate_dap_sidebar_system(engine);
                    engine
                        .dap_sidebar_system
                        .borrow()
                        .render(backend, body_rect);
                });
                engine
                    .dap_sidebar_action_hits
                    .replace(layout.status_bar_hit_regions);
            }
            PANEL_GIT => {
                // Composed through `SidebarPanelBody::render_with` with
                // `chrome: SidebarPanelChrome::None` — the shell's sidebar header
                // already titles this panel "SOURCE CONTROL", and `sc_header_status_bar`'s
                // row below is body content, not a second title. `None` chrome reserves
                // no rows, so `body_rect` is pixel-identical to `q_sb`.
                let panel = render::SidebarPanelBody {
                    background: None,
                    chrome: render::SidebarPanelChrome::None,
                    scrollbar_gutter: None,
                };
                panel.render_with(backend, q_sb, |backend, body_rect| {
                    let q_sb = body_rect;
                    if let Some(ref sc) = screen.source_control {
                        // Header row + commit-input box. Paint them via `render::sc_*` adapters
                        // the same way TUI uses so the two renderers cannot drift.
                        // Band geometry comes from the shared `render::sc_sidebar_bands` so the
                        // click router in `try_route_sidebar_mouse_event` resolves a press
                        // against the same derivation that painted it. `SC_COMMIT_BORDER_PX` is
                        // the primitive's 1px border top+bottom.
                        let bands = render::sc_sidebar_bands(
                            &sc.commit_message,
                            q_sb,
                            lh as f32,
                            SC_COMMIT_BORDER_PX,
                            sc.has_focus,
                        );
                        self.cached_sc_bands.set(Some(bands));
                        let header_bar = render::sc_header_status_bar(sc, theme);
                        let _ = backend.draw_status_bar_interactive(
                            bands.header,
                            &header_bar,
                            &quadraui::InteractionState::new(),
                        );

                        let ti = render::sc_commit_message_to_text_input(sc);
                        backend.draw_text_input(bands.commit_input, &ti);

                        // Render the toolbar-slab + section list below the
                        // header + commit input.
                        let slab_rect = bands.slab;
                        render::draw_sc_sidebar_panel(backend, engine, sc, slab_rect);
                        // Focused-hint row: shared with TUI through
                        // `render::sc_hint_status_bar`, painted only when
                        // `ScSidebarBands::hint` was reserved (i.e. the panel
                        // has focus) so it can't show unreserved space.
                        if let Some(hint_rect) = bands.hint {
                            let hint_bar = render::sc_hint_status_bar(theme);
                            let _ = backend.draw_status_bar_interactive(
                                hint_rect,
                                &hint_bar,
                                &quadraui::InteractionState::new(),
                            );
                        }
                        let body_rect = engine
                            .sc_panel_layout
                            .borrow()
                            .as_ref()
                            .map(|l| l.content_bounds)
                            .unwrap_or(slab_rect);
                        engine.sc_sidebar_body_rect.set(body_rect);
                        // Without this, `sc_sidebar_system.handle_cached`
                        // returns `Ignored` unconditionally and every
                        // content-row press (header collapse, row select) is a
                        // silent no-op — see `render::UnitProfile::
                        // sidebar_system_metrics`'s own doc for the full
                        // story. Reads `backend.line_height()`
                        // directly — not the `lh` parameter above, whose
                        // `self.cached_line_height.max(backend.line_height())`
                        // derivation (`render_content`'s own top) can lag behind
                        // what `backend` reports by the time `render()` a few
                        // lines down actually reads it — so the metrics
                        // `handle_cached` hit-tests against can never disagree
                        // with what this exact `render()` call paints.
                        let sc_lh = backend.line_height();
                        engine
                            .sc_sidebar_system
                            .borrow_mut()
                            .set_backend_info(sc_lh, (self.units.sidebar_system_metrics)(sc_lh));
                        render::populate_sc_sidebar_system(engine, theme);
                        engine.sc_sidebar_system.borrow().render(backend, body_rect);

                        // Branch picker / create popup and help dialog — both keyboard-reachable
                        // even though the git sidebar has no live mouse-click routing yet.
                        // Render over the whole sidebar content area.
                        if let Some(ref bp) = sc.branch_picker {
                            let palette = render::sc_branch_picker_to_palette(bp);
                            let popup_w = q_sb.width.min(40.0 * cw as f32);
                            let popup_h = if bp.create_mode {
                                4.0 * lh as f32
                            } else {
                                (q_sb.height * 0.6).min(15.0 * lh as f32)
                            };
                            let popup_x = q_sb.x + (q_sb.width - popup_w) / 2.0;
                            let popup_y = q_sb.y + 2.0 * lh as f32;
                            backend.draw_palette(
                                quadraui::Rect::new(popup_x, popup_y, popup_w, popup_h),
                                &palette,
                            );
                        }

                        if sc.help_open {
                            let viewport = q_sb;
                            let (dialog, dlayout) =
                                render::sc_help_dialog_layout(viewport, cw as f32, lh as f32);
                            backend.draw_dialog(&dialog, &dlayout);
                        }
                    } else {
                        // Git panel is the active tab but there's no repo open
                        // (e.g. the user closed it, or switched to a non-git
                        // folder, without also switching sidebar tabs) — nothing
                        // paints this frame. Clear the cached band geometry so a
                        // stray click doesn't get resolved against stale
                        // coordinates from the last time a repo *was* open
                        // (`route_sc_sidebar_event` reads this cache directly).
                        self.cached_sc_bands.set(None);
                    }
                });
            }
            PANEL_EXTENSIONS => {
                // The shell's own `AppShell` sidebar header already
                // paints " EXTENSIONS " above `q_sb` — this arm paints only
                // the search/filter row (#1256 found GTK painting neither
                // row at all).
                //
                // Composed through `SidebarPanelBody::render_with`
                // (quadraui#1059, the composer the `PANEL_EXPLORER`/
                // `PANEL_GIT` arms use) with `SidebarPanelChrome::Search`
                // instead of the bespoke `render::
                // paint_sidebar_search_row` (deleted by this issue) — see
                // `render::search_only_chrome`'s doc. TUI's `panels::
                // render_ext_sidebar` builds the identical chrome through
                // the same helper.
                let panel = render::SidebarPanelBody {
                    background: None,
                    chrome: render::search_only_chrome(
                        &engine.ext_sidebar_query,
                        "Search extensions (press /)",
                        engine.ext_sidebar_input_active,
                        theme,
                    ),
                    scrollbar_gutter: None,
                };
                panel.render_with(backend, q_sb, |backend, body_rect| {
                    Self::refresh_ext_sidebar_metrics(backend, engine, self.units);
                    render::populate_ext_sidebar_system(engine);
                    engine.ext_sidebar_body_rect.set(body_rect);
                    engine
                        .ext_sidebar_system
                        .borrow()
                        .render(backend, body_rect);
                });
            }
            PANEL_BOARD => {
                // Generic Board panel host — the *only* GTK-specific
                // code here is picking which `screen.board` field to read
                // and where to put the rect; the actual rasterisation is
                // `Backend::draw_board`, the same call TUI's
                // `panels::render_board_panel` makes (Platform-Neutrality
                // Rule: no bespoke board drawing per backend).
                if let Some(ref board) = screen.board {
                    if let Some(ref model) = board.model {
                        let layout = backend.draw_board(q_sb, model);
                        engine.board_layout.replace(Some(layout));
                    } else {
                        engine.board_layout.replace(None);
                        if let Some(ref status) = board.status {
                            let bar = render::board_status_bar(status, theme);
                            let rect = quadraui::Rect::new(q_sb.x, q_sb.y, q_sb.width, lh as f32);
                            let _ = backend.draw_status_bar_interactive(
                                rect,
                                &bar,
                                &quadraui::InteractionState::new(),
                            );
                        }
                    }
                }
            }
            PANEL_SETTINGS => {
                // Settings only ever shows as a centred modal overlay
                // (the SettingsModal rung). The sidebar slot is always
                // blank — just background fill.
                backend.draw_solid_fill(q_sb, theme.tab_bar_bg);
            }
            id if id
                .strip_prefix("ext:")
                .is_some_and(|name| engine.is_plugin_view(name)) =>
            {
                // A `vimcode.ui.register_view` panel — the plugin declared
                // a widget tree, so paint it as a `quadraui::Form` through the
                // *same* shared `FormController` the `PANEL_SETTINGS` arm above
                // uses, rather than as `ExtPanelItem` tree rows. This is the
                // whole per-surface cost of plugin UI: one arm in the shared
                // shell, zero lines in `src/gtk/` or `src/tui_main/`.
                //
                // A view whose `render()` returned a `kind = "..."`
                // table instead of `fields` paints through the matching
                // `ListView`/`TreeView`/`DataTable`/`TextDisplay` primitive
                // instead — still one arm here, still zero lines in
                // `src/gtk/`/`src/tui_main/`. `plugin_view_form_rect` is
                // reused as "the rect the active body last painted into"
                // regardless of which body kind is active (mutually
                // exclusive per view, so there is no collision).
                let name = id.strip_prefix("ext:").unwrap_or(id).to_string();
                let panel = render::SidebarPanelBody {
                    background: Some(theme.tab_bar_bg),
                    chrome: render::SidebarPanelChrome::Header(format!(
                        " {}",
                        screen
                            .ext_panel
                            .as_ref()
                            .map(|p| p.title.clone())
                            .unwrap_or_default()
                    )),
                    scrollbar_gutter: None,
                };
                panel.render_with(backend, q_sb, |backend, body_rect| {
                    // Same contract as the Settings arm: cache the exact rect
                    // painted, because the click routers re-derive row
                    // geometry from it.
                    engine.plugin_view_form_rect.set(body_rect);
                    engine.ext_panel_content_rect.set(q_sb);
                    let has_focus = engine.ext_panel_has_focus;
                    let kind = engine
                        .plugin_views
                        .get(&name)
                        .and_then(|v| v.body.as_ref())
                        .map(|b| b.kind_name());
                    match kind {
                        Some("list") => {
                            render::paint_plugin_view_list(
                                engine,
                                &name,
                                PluginViewHost::Sidebar,
                                has_focus,
                                body_rect,
                                backend,
                            );
                        }
                        Some("tree") => {
                            if render::populate_plugin_view_tree_controller(
                                engine,
                                &name,
                                PluginViewHost::Sidebar,
                                has_focus,
                            ) {
                                engine
                                    .plugin_view_tree_controller
                                    .borrow()
                                    .render(backend, body_rect);
                            }
                        }
                        Some("table") => {
                            render::paint_plugin_view_table(
                                engine,
                                &name,
                                PluginViewHost::Sidebar,
                                has_focus,
                                body_rect,
                                backend,
                            );
                        }
                        Some("text_view") => {
                            render::paint_plugin_view_text(
                                engine,
                                &name,
                                PluginViewHost::Sidebar,
                                has_focus,
                                body_rect,
                                backend,
                            );
                        }
                        _ => {
                            if render::populate_plugin_view_form_controller(engine) {
                                engine
                                    .plugin_view_form_controller
                                    .borrow_mut()
                                    .render_and_cache(backend, body_rect);
                            }
                        }
                    }
                });
            }
            id if id.starts_with("ext:") => {
                // A plugin-provided panel — paint its own sections + items via
                // `render::ext_panel_to_tree_view` + `Backend::draw_tree`, the same
                // adapter `tui_main::panels::render_ext_panel` uses.
                // Composed through `SidebarPanelBody::render` with chrome, same as TUI.
                if let Some(ref panel) = screen.ext_panel {
                    // Cache the whole content rect (chrome included),
                    // verbatim — mirrors `render_ext_panel`'s own
                    // `ext_panel_content_rect` write so a future hover/geometry
                    // consumer can't tell which backend painted this frame.
                    engine.ext_panel_content_rect.set(q_sb);

                    let input_visible = panel.input_active || !panel.input_text.is_empty();
                    let header_title = format!(" {}", panel.title);
                    let sidebar_panel = render::SidebarPanelBody {
                        background: None,
                        chrome: if input_visible {
                            render::SidebarPanelChrome::HeaderAndSearch {
                                header: header_title,
                                query: panel.input_text.clone(),
                                placeholder: String::new(),
                                active: panel.input_active,
                            }
                        } else {
                            render::SidebarPanelChrome::Header(header_title)
                        },
                        scrollbar_gutter: None,
                    };
                    let body =
                        render::ExtPanelTreeBody(render::ext_panel_to_tree_view(panel, theme));
                    let layout = sidebar_panel.render(backend, q_sb, &body);

                    if layout.body_rect.height > 0.0 {
                        // Cache the exact `Backend::tree_layout` this
                        // frame painted with — the click router
                        // (`render::route_ext_panel_click`, shared with TUI's
                        // `mouse::handle_mouse`) reads this instead of
                        // re-deriving row geometry from a uniform row height.
                        // See `Engine::ext_panel_tree_layout`'s own doc for why
                        // that matters here: this backend pitches a tree's
                        // header rows shorter than its item rows.
                        let tree_layout = backend.tree_layout(layout.body_rect, &body.0);
                        engine
                            .ext_panel_tree_layout
                            .replace(Some((layout.body_rect, tree_layout)));
                    } else {
                        engine.ext_panel_tree_layout.replace(None);
                    }

                    // The `?`-triggered keybindings help popup, drawn over the whole panel
                    // content area via `Tooltip` + `Backend::draw_tooltip_with_chrome` — see
                    // `render::ext_panel_help_tooltip_layout`'s own doc for why this is the
                    // real implementation and not a stand-in.
                    if panel.help_open {
                        let (tooltip, tlayout, chrome) = render::ext_panel_help_tooltip_layout(
                            &panel.name,
                            &panel.help_bindings,
                            q_sb,
                            cw as f32,
                            lh as f32,
                        );
                        backend.draw_tooltip_with_chrome(&tooltip, &tlayout, &chrome);
                    }
                } else {
                    engine.ext_panel_tree_layout.replace(None);
                }
            }
            PANEL_AI => {
                // Adopts quadraui's `ChatController` — one shared
                // `render()` for both backends, like `explorer_tree` and
                // `ext_sidebar_system` above, replacing the hand-painted
                // `draw_ai_sidebar_panel`. `ai_chat_rect` is cached on
                // `Engine` (not here) so `route_ai_chat_event` re-derives
                // the identical layout `render()` painted.
                render::populate_ai_chat_controller(engine, theme, backend);
                // The pinned plan block gets a band carved off the
                // top of the panel rect *before* `ChatController` ever sees
                // the remainder — `ai_chat_rect` (and therefore every
                // `route_ai_chat_event` hit-test) is the shrunk rect, so a
                // click inside the band never reaches the transcript below
                // it. Zero-height (no-op split) while there's no plan.
                let plan_h = render::ai_plan_band_height(engine, backend.line_height());
                let (plan_rect, chat_rect) = if plan_h > 0.0 {
                    (
                        quadraui::Rect::new(q_sb.x, q_sb.y, q_sb.width, plan_h),
                        quadraui::Rect::new(
                            q_sb.x,
                            q_sb.y + plan_h,
                            q_sb.width,
                            (q_sb.height - plan_h).max(0.0),
                        ),
                    )
                } else {
                    (quadraui::Rect::new(q_sb.x, q_sb.y, 0.0, 0.0), q_sb)
                };
                render::paint_ai_plan_band(backend, engine, theme, plan_rect);
                engine.ai_chat_rect.set(chat_rect);
                engine.ai_chat.borrow().render(backend, chat_rect);
                // Slash-command completions, painted on top — no-op unless the input
                // matches an agent-declared command. Pass `chat_rect` (not `q_sb`) so
                // the popup anchors off the input box's actual position, unaffected by
                // layout changes.
                // `cached_ai_chat_metrics`'s drift guard: capture what `render()` used
                // so `route_ai_chat_event` can re-apply before `handle()`.
                render::paint_ai_command_completions(backend, engine, chat_rect);
                // `cached_explorer_metrics`'s drift guard, ported: `backend`'s
                // "current" line_height/char_width are mutable and can be
                // overwritten by whatever paints next this frame or the
                // next, so capture what `render()` actually used here for
                // `route_ai_chat_event` to re-apply before `handle()`.
                self.cached_ai_chat_metrics
                    .set((backend.line_height() as f64, backend.char_width() as f64));
            }
            _ => {}
        }

        // The sidebar-item hover popup is composed in a later rung (not here)
        // to ensure it stays visible when the sidebar is collapsed.
    }

    /// Re-derive `ext_sidebar_system`'s backend metrics from what `backend`
    /// is about to paint with.
    ///
    /// Without this, `ext_sidebar_system.handle_cached` returns `Ignored`
    /// unconditionally and every content-row press on the plugin ext panel
    /// is a silent no-op. Reads `backend.line_height()` directly rather than
    /// accepting a cached parameter — a cached value can lag behind what
    /// `backend` reports by the time `render()` actually reads it. Shared by
    /// multiple sidebar panel arms that were previously verbatim copies of
    /// the same code.
    pub(crate) fn refresh_ext_sidebar_metrics(
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        units: render::UnitProfile,
    ) {
        let ext_lh = backend.line_height();
        engine
            .ext_sidebar_system
            .borrow_mut()
            .set_backend_info(ext_lh, (units.sidebar_system_metrics)(ext_lh));
    }

    /// Compose the editor-anchored popups: completion menu, LSP hover, editor
    /// hover (rich markdown), diff peek and signature help.
    ///
    /// Extracted out of `render_content` to make that function read as the
    /// frame's order. Deliberately **not** a `FrameOp` rung: these are anchored
    /// to the active window's cursor rather than to a band.
    ///
    /// The build-adapter → `.layout()` → `backend.draw_*` → cache-output part
    /// now lives once in `render::paint_editor_popups`. #1237 folded the
    /// anchor-point arithmetic into `render::editor_popup_anchors` too (see its
    /// doc for why). This method's own job is: finding the active window,
    /// scaling into GTK's native pixel units (`lh`/`cw`), and picking each
    /// popup's clip viewport.
    ///
    /// `main` is `AppShellLayout::main_content_bounds` — the clip viewport
    /// every popup but the completion menu and editor-hover popup is placed
    /// inside; those two clamp to the active window's own rect instead.
    pub(crate) fn paint_editor_popups_rung(
        &self,
        backend: &mut dyn quadraui::Backend,
        screen: &render::ScreenLayout,
        theme: &Theme,
        main: quadraui::Rect,
        lh: f64,
        cw: f64,
    ) {
        let active_win = screen
            .windows
            .iter()
            .find(|w| w.window_id == screen.active_window_id);

        let win_viewport = active_win.map(|active_win| {
            quadraui::Rect::new(
                active_win.rect.x as f32,
                active_win.rect.y as f32,
                active_win.rect.width as f32,
                active_win.rect.height as f32,
            )
        });

        // The char→display column arithmetic (tab expansion, scroll offset)
        // for every anchor now lives once in `render::editor_popup_anchors`,
        // shared with TUI. GTK uses its raw sub-pixel float window origin as-is
        // (no whole-cell snapping).
        // Note the precision drop: `win_origin`/`cw`/`lh` are cast to `f32`
        // *here*, before anchor arithmetic, whereas pre-#1237 each anchor ran
        // entirely in `f64` and only cast to `f32` at the end. GTK pays for
        // sharing with TUI an extra rounding step here, but this is immaterial
        // at realistic screen-pixel magnitudes.
        let win_origin = active_win.map(|w| (w.rect.x as f32, w.rect.y as f32));
        let anchor_points = render::editor_popup_anchors(screen, win_origin, cw as f32, lh as f32);

        // Completion popup anchor — cache the layout so the click handler
        // (B.5b Stage 5) can hit-test items and register the popup on the
        // modal stack.
        let completion = screen.completion.as_ref().and_then(|menu| {
            let (cursor_x, cursor_y) = anchor_points.completion?;
            // Longest candidate + 4 cells of padding/border, floored at 12 cells.
            // Both the `+4`/`.max(12.0)` match `tui_main::render_impl`'s completion
            // width calculation. Scaling the `12.0` floor by `cw` keeps GTK's effective
            // minimum close to its old ~100px while giving TUI-via-`App` the same
            // sane 12-cell floor `render_impl` already ships.
            // `Completions::layout` clamps position but never clamps `popup_w` itself,
            // so a long candidate can still render past the window's right edge — a gap
            // in the shared quadraui primitive, not something to patch per-backend.
            let popup_w = ((menu.max_width + 4) as f64 * cw).max(12.0 * cw);
            let max_popup_h = 10.0 * lh;
            Some((
                render::PopupAnchor {
                    x: cursor_x,
                    y: cursor_y,
                    viewport: win_viewport?,
                },
                popup_w as f32,
                max_popup_h as f32,
            ))
        });

        // Simple LSP hover popup anchor (plain text, non-interactive).
        let hover = anchor_points.hover.map(|(x, y)| render::PopupAnchor {
            x,
            y,
            viewport: main,
        });

        // Signature-help popup anchor (insert mode, cursor inside a call).
        let signature_help = anchor_points
            .signature_help
            .map(|(x, y)| render::PopupAnchor {
                x,
                y,
                viewport: main,
            });

        // Diff-peek popup anchor (inline git hunk preview).
        let diff_peek = anchor_points.diff_peek.map(|(x, y)| render::PopupAnchor {
            x,
            y,
            viewport: main,
        });

        // Editor hover popup anchor (rich markdown; `gh` key, diagnostic/
        // annotation/plugin hovers, or mouse dwell). Bounds/link rects/
        // scrollbar geometry are cached for the click + drag handlers,
        // same as `draw.rs::draw_editor_hover_popup` did.
        let editor_hover = anchor_points.editor_hover.and_then(|(x, y)| {
            Some(render::PopupAnchor {
                x,
                y,
                viewport: win_viewport?,
            })
        });

        let mut completion_layout = self.completion_layout.borrow_mut();
        let mut editor_hover_link_rects = self.editor_hover_link_rects.borrow_mut();
        let mut editor_hover_popup_rect = self.editor_hover_popup_rect.get();
        let mut editor_hover_scrollbar = self.editor_hover_scrollbar.get();
        render::paint_editor_popups(
            backend,
            screen,
            theme,
            cw as f32,
            lh as f32,
            completion,
            hover,
            editor_hover,
            diff_peek,
            signature_help,
            &mut completion_layout,
            &mut editor_hover_link_rects,
            &mut editor_hover_popup_rect,
            &mut editor_hover_scrollbar,
        );
        self.editor_hover_popup_rect.set(editor_hover_popup_rect);
        self.editor_hover_scrollbar.set(editor_hover_scrollbar);
    }

    /// Per-frame state pushes that must happen before anything is composed.
    ///
    /// Theme, nerd-font flag and UI font are re-synced *every* frame so a
    /// runtime `:set colorscheme` / `:set nonerdfonts` / `:set guifont` /
    /// `:set ui_font_size=N` reaches the rasterisers on the very next paint
    /// instead of never; the two hit-test registries are cleared so a panel or
    /// window that closed this frame leaves no stale entry for
    /// `dispatch_scroll` / `pixel_to_click_target` to resolve against.
    /// Extracted out of `render_content` so that function
    /// reads as the frame's *order* and nothing else.
    pub(crate) fn sync_per_frame_backend_state(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        theme: &Theme,
    ) {
        backend.set_theme(render::to_quadraui_theme(theme));
        // Re-synced every frame, same reasoning as the theme sync
        // right above — a runtime `:set menu_style=...` (or its Settings-
        // sidebar equivalent) reaches quadraui's own `MenuStyle` on the
        // very next paint. `set_menu_style` only persists a value on the
        // backend struct (no popup, no blocking call), so calling it
        // every frame is as cheap as the theme/font syncs it sits next to.
        backend.set_menu_style(render::to_quadraui_menu_style(engine.settings.menu_style));
        // Re-synced every frame so runtime toggles (`:set
        // nonerdfonts`) take effect immediately, matching TUI.
        render::sync_nerd_fonts(backend, engine);
        // Re-synced every frame so a runtime `:set guifont`/font-size change
        // takes effect immediately. Ported from the dead
        // `draw.rs::draw_editor`'s top-of-frame call, which was the only
        // live caller — `UI_FONT()` (used a few paint calls below for the
        // raw-Pango chrome that doesn't go through `Backend::set_ui_font`)
        // read the process-global atomic this writes, so before this port it
        // silently stayed pinned at the default size forever.
        sync_ui_font_size(&engine.settings, backend);
        // Push the UI_FONT() family+size onto the paint backend's `ui_font`,
        // which `draw_status_bar`, `draw_tree`, `draw_tab_bar_icons`, and
        // `draw_menu_bar` now all honour for both paint and measurement. Re-set
        // every frame (not just once) so a runtime `:set ui_font_size=N` takes
        // effect immediately.
        // Push `settings.font_family`/`font_size` onto the paint backend's editor
        // font every frame, so a runtime change reaches the painted editor text
        // on the very next frame.
        backend.set_ui_font(&UI_FONT());
        // Push `settings.font_family`/`font_size` onto
        // the *paint* backend's editor font every frame, so a runtime
        // `:set guifont`/`:set font_size=N`/`zoomin`/`zoomout` reaches the
        // painted editor text on the very next frame — mirroring
        // `set_ui_font` immediately above. Before this call nothing in
        // vimcode ever read `settings.font_family`/`font_size` (`grep -rn
        // "set_editor_font" src/` returned zero hits before this issue), so
        // the editor painted at whatever default quadraui's `GtkBackend`/
        // `MacBackend` ship with ("Monospace 11") regardless of the setting.
        //
        // Resolved through `resolve_editor_font` (same function
        // `shell_config`'s pre-seed above uses) instead of the raw fields,
        // so an un-customized setting keeps tracking this backend's
        // platform-native convention every frame, not just at startup.
        let (editor_family, editor_size_pt) = resolve_editor_font(&engine.settings, backend);
        backend.set_editor_font(&editor_family, editor_size_pt);
        // On the native macOS GUI, override `set_editor_font`'s own
        // natural (Core Text ascent+descent+leading, ~1.17x) line height
        // with VS Code's row pitch (`round(font_px * 1.5)` by default, or
        // `settings.line_height`'s explicit multiplier) — see
        // `resolve_editor_line_height_px`'s doc for why this is scoped to
        // macOS only and re-applied every frame, same reasoning as
        // `set_editor_font` immediately above (`zoomin`/`zoomout`/`:set
        // line_height=N` must reach the painted row pitch next frame, not
        // just at startup).
        //
        // Review round 1: this override is deliberately global for the
        // whole frame, not scoped to only the `Surface::Editor` paint call
        // — everything downstream of it (tab bars, scrollbars, completion/
        // hover/dialog/picker popups) is anchored and hit-tested in the
        // *same* `lh` units (`App::painted_line_height`), so suspending the
        // override partway through the frame would desync paint from
        // click/hover for all of those, not just the editor. The one place
        // that must NOT see this value — sidebar panel content
        // (`tree_layout`/`list_layout`/`form_layout`/`msv_layout` all read
        // `current_line_height` directly) — gets it suspended narrowly,
        // around that one call; see `render_content`'s `FrameOp::
        // SidebarPanel` arm.
        if let Some(lh) = resolve_editor_line_height_px(&engine.settings, backend, editor_size_pt) {
            quadraui::Backend::set_current_line_height(backend, lh);
        }

        // Scroll surfaces are re-registered from scratch every frame
        // (mirrors TUI's `render_impl.rs` `scroll_surfaces.borrow_mut().clear()`)
        // so a panel that closes — or moves — doesn't leave a stale entry
        // behind for `dispatch_scroll`/`dispatch_click` to hit-test against.
        // Ported from the dead `draw.rs::draw_editor`'s equivalent top-of-frame
        // clear, which was this list's only writer under `ShellApp`.
        engine.scroll_surfaces.borrow_mut().clear();
        // Per-window status bar segment hit zones: `click.rs`'s
        // `pixel_to_click_target` reads this to resolve `WindowZone::StatusBar`
        // clicks (goto-line, change-language, switch-branch, ...) to a
        // `StatusAction`, but under `ShellApp` nothing ever populated it — the
        // dead `draw.rs::draw_window_status_bar` was the only writer, so every
        // per-window status bar segment click silently resolved to
        // `ClickTarget::None`. Cleared here and re-inserted per window below
        // (and for the separated status line further down) so a window that
        // closes doesn't leave a stale, now-wrong entry keyed by its id.
        self.status_segment_map.borrow_mut().clear();
    }

    /// Compose the **editor band** and recover this frame's `FrameHitMap` from it.
    ///
    /// Walks `render::paint_editor_band_rungs`, the single loop both this
    /// method and TUI now call — see that function's own doc for which rungs
    /// need `self` and why. Extracted out of `render_content` so that function
    /// reads as the frame's order.
    ///
    /// `band` carries the editor column's origin and width (its height is not
    /// used); `metrics` is `(line_height, char_width)` and `tab_metrics` is
    /// `(tab_row_h, tab_bar_h)`, both in pixels.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compose_editor_band_rungs(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        screen: &render::ScreenLayout,
        theme: &Theme,
        band: quadraui::Rect,
        metrics: (f64, f64),
        tab_metrics: (f64, f64),
    ) {
        use quadraui::{ScreenLayout as QSL, Surface};
        let (lh, cw) = metrics;
        let (tab_row_h, tab_bar_h) = tab_metrics;

        // Reset the pixel-accurate hit caches; repopulated by the `TabBars`
        // rung below so the click / hover hit-tests use the exact drawn
        // geometry. Cleared here, before the walk, rather than from an
        // absent-rung branch: `compose_editor_band` returns only the live
        // rungs, so a frame with no tab bars has no arm to clear them from.
        self.cached_group_tab_bar_layouts.borrow_mut().clear();
        self.cached_tab_slots_abs.borrow_mut().clear();
        // Reset alongside the hit caches above — this frame's
        // `TabBars` rung (if any) repopulates it, and `handle_poll_tick`
        // wants this frame's measurements, not an accumulation across
        // frames (mirrors TUI's `tab_visible_counts.borrow_mut().clear()`).
        self.tab_visible_counts.borrow_mut().clear();

        // `window_editors`/`hit_bars` stash what the `Windows`/`TabBars` rungs
        // painted, past the walk, so the `FrameHitMap` references the SAME objects
        // just painted rather than a second copy that could drift.
        // Both scrollbars ARE painted for the editor on GTK. quadraui#968 taught
        // `gtk::editor::draw_editor` to paint both scrollbars itself. Geometry comes
        // from `Editor::layout`, the same call hit-testing uses, so paint and click
        // agree by construction.
        // `app_support::editor_scrollbar_layout` builds the same `Editor`/`EditorLayout`
        // pair (minus the rendered text) so the axis-parameterised geometry helpers
        // can never resolve a hover/drag rect paint didn't actually draw.
        // That agreement also depends on both sides reading the same `v_scrollbar_w`.
        // Paint reads it from `backend.editor_v_scrollbar_width()`, so
        // `editor_scrollbar_layout`'s callers must pass that same value through.
        let mut window_editors = Vec::with_capacity(screen.windows.len());
        let mut hit_bars = Vec::new();
        let units = render::EditorBandUnits::px(lh, cw, tab_row_h);
        let composed_editor = render::paint_editor_band_rungs(
            backend,
            engine,
            screen,
            theme,
            band,
            units,
            tab_bar_h,
            self.tab_drag.is_dragging(),
            self,
            &mut window_editors,
            &mut hit_bars,
        );
        *self.composed_editor_band.borrow_mut() = composed_editor;
        // Same contract as the chrome/overlay bands: read back through the
        // field rather than the local, so the *stored* observable is what gets
        // validated — a frame that recorded one thing and composed another
        // would be a lie the tests then trusted.
        if let Err(why) = render::check_editor_band_order(&self.composed_editor_band.borrow()) {
            debug_assert!(false, "GTK {why}");
        }

        // ── Recover a FrameHitMap for Editor/TabBar zone detection ─────────────
        // Pure `.push()` accumulation into a *separate* `ScreenLayout`, built
        // from the same `Editor` objects painted by the `Windows` rung above
        // (`window_editors`, same order as `screen.windows` so
        // `FrameZone::Editor { idx }` maps straight back to
        // `cached_layout.windows[idx]`) plus the `TabBar` surfaces the
        // `TabBars` rung recorded. `ScreenLayout::hit_map()`
        // makes no `backend.draw_*()` calls, so accumulating into it can never
        // reorder or repeat the real painting done above — see
        // `click::pixel_to_click_target` for the consumer side.
        //
        // Editors are pushed first and the tab bars after, so the first tab
        // bar's `FrameZone::TabBar { idx }` is `window_editors.len()`, not `0`
        // — the *global* surface index `cached_tab_bar_zones` is keyed by.
        let mut hit_frame = QSL::new();
        for editor in &window_editors {
            hit_frame.push(Surface::Editor {
                rect: editor.rect,
                editor,
            });
        }
        let mut tab_bar_zones: HashMap<usize, (core::window::GroupId, quadraui::Rect)> =
            HashMap::new();
        for (surface_idx, (group_id, rect, bar)) in (window_editors.len()..).zip(hit_bars) {
            hit_frame.push(Surface::TabBar {
                rect,
                bar,
                hovered_close: None,
            });
            tab_bar_zones.insert(surface_idx, (group_id, rect));
        }
        *self.cached_frame_hit_map.borrow_mut() = Some(hit_frame.hit_map());
        *self.cached_tab_bar_zones.borrow_mut() = tab_bar_zones;

        // Refresh the drop geometry the *drag hit-test* reads
        // (`handle_mouse_drag_msg` → `compute_tab_drop_zone`), unconditionally
        // and for every frame — a drag has to be able to *start*, which means
        // the cache must be current on frames where no drag is live and the
        // `TabDragOverlay` rung therefore never ran. The rung calls this same
        // function rather than carrying a second copy of the computation, so
        // the overlay and the hit-test can only ever agree.
        self.cache_tab_drop_geometry(screen, engine, tab_bar_h);
    }
}

// ── Dormant ShellApp impl (#448-B) ──────────────────────────────────────────
// This impl compiles but is NOT wired up.
impl App {
    /// Paint the title-bar band: the menu bar + any open dropdown, the app-icon
    /// slot, and the inline window controls.
    ///
    /// One function because all three draw into the *same* strip and their
    /// order within it is fixed by a rasteriser detail rather than by taste:
    /// `MenuSystem::render` calls `draw_menu_bar` across the whole band, so the
    /// icon slot and the controls must follow it or get erased (the #552
    /// round-2/3 "buttons render blank" regression). Kept off
    /// [`render::FRAME_Z_ORDER`]'s overlay tail because TUI has neither an app icon nor
    /// in-canvas window controls, so they cannot be part of a *shared*
    /// sequence; they ride the `MenuDropdown` rung instead.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_title_bar_band(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        theme: &Theme,
        menu_row_rect: quadraui::Rect,
        menu_items_rect: quadraui::Rect,
        app_icon_rect: quadraui::Rect,
        controls_rect: Option<quadraui::Rect>,
    ) {
        {
            // `menu_items_rect`, not `menu_row_rect` — the app icon owns the
            // leading slot. `MenuSystem::render` positions the open
            // dropdown from this same rect, so passing the narrowed one is
            // what keeps a dropdown under the label that opened it.
            engine.menu_system.borrow().render(backend, menu_items_rect);

            // ── App icon, left of `File` ─────────────────────────────────
            // `draw_menu_bar` above only filled `menu_items_rect`, so the
            // reserved slot still shows the frame-clear colour
            // (`theme.background`) rather than the bar's own `tab_bar_bg`.
            // Painting an *item-less* `MenuBar` across the slot fills it
            // through the very same rasteriser as the strip beside it, so
            // the two backgrounds cannot drift apart the way a hand-picked
            // theme colour would.
            if app_icon_rect.width > 0.0 && app_icon_rect.height > 0.0 {
                let filler = quadraui::MenuBar {
                    id: quadraui::WidgetId::new("app_icon_slot"),
                    items: Vec::new(),
                    open_item: None,
                    focused_item: None,
                };
                let _ = backend.draw_menu_bar(
                    quadraui::Rect::new(
                        menu_row_rect.x,
                        menu_row_rect.y,
                        (menu_items_rect.x - menu_row_rect.x).max(0.0),
                        menu_row_rect.height,
                    ),
                    &filler,
                );
                // `app_icon_image_for_paint` (this file) — see its doc
                // comment for why every backend can now share the same
                // [`crate::render::app_icon_image`] builder here.
                let _ = backend.draw_image(app_icon_rect, &app_icon_image_for_paint());
            }
        }

        // ── Inline window controls (min/max/close) — after the bar ───────────
        // `menu_system.render()` above repaints `draw_menu_bar` across the full
        // `menu_row_rect` band, so the controls must be painted *after* it or
        // they get erased. The controls sit in the title-bar band, to the right
        // of the menu labels; the dropdown body drops *below* the band, so painting
        // here never covers an open dropdown. It is title-bar chrome, so the
        // modal rungs paint over it — which is the point: a modal dialog should
        // cover the window controls.
        if let Some(controls_rect) = controls_rect {
            // Read live via `WindowControl::is_maximized()` rather
            // than the deleted `self.window`/`PlatformWindowHandle` seam —
            // `backend` is already in hand here, so there is no "no backend
            // in scope" problem to cache around (contrast
            // `App::cached_window_width`'s doc).
            let maximized = backend
                .window()
                .and_then(|w| w.is_maximized().ok())
                .unwrap_or(false);
            let controls_bar = render::window_controls_status_bar(theme, maximized);
            let interaction = self.title_bar_interaction.borrow();
            let hits = backend.draw_status_bar_interactive(
                controls_rect,
                &controls_bar,
                &quadraui::InteractionState::from_parts(
                    interaction.hovered_id().cloned(),
                    interaction.pressed_id().cloned(),
                ),
            );
            interaction.set_layout(hits);
        }
    }
}
