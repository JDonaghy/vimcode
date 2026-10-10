use super::construction::app_icon_image_for_paint;
use super::*;

impl App {
    pub(crate) fn handle_poll_tick(&mut self, backend: &mut dyn quadraui::Backend) {
        // Schedule a redraw if the colorscheme changed (e.g. via
        // `:colorscheme`). #1498: this used to also reload a GTK-only CSS
        // provider theming the native file dialog here; that provider is
        // gone (JDonaghy/quadraui#1091 moved the equivalent stylesheet into
        // `GtkPlatformServices`, reloaded every frame by
        // `sync_per_frame_backend_state`'s `Backend::set_theme` call), so
        // this block's only remaining job is the redraw schedule.
        {
            let current = self.engine.borrow().settings.colorscheme.clone();
            if current != self.last_colorscheme {
                self.last_colorscheme = current;
                self.draw_needed.set(true);
            }
        }

        // #949: reload settings.json if it changed on disk. This used to be
        // driven by a GTK-only `gio::FileMonitor` that sent a
        // `DeferredAction::SettingsFileChanged` on a native file-change
        // event; that watcher is gone, and `check_settings_reload`'s
        // portable mtime poll (inside `settings_file_changed`) now runs
        // unconditionally every tick instead — the same mechanism TUI's own
        // `tick` has always used. quadraui's GTK/macOS idle-poll fallback
        // ceiling is 250ms (`runner.rs`'s `ShellApp::tick` doc), so the
        // reload lag here matches what TUI already ships, not a regression
        // from the watcher's near-immediate `ChangesDoneHint`.
        self.settings_file_changed();

        // #731: a ~135-line block used to live here polling
        // `self.mouse_pos_cell` at 20Hz for four distinct hover features —
        // h-scrollbar hover, tab-close (×) hover + tab tooltip, debug
        // toolbar button hover, and LSP hover-on-dwell popups
        // (`Engine::editor_hover_mouse_move`). All four were gated on a
        // `da_size` derived from `self.drawing_area`, permanently `None`
        // under the ShellApp runner (nothing assigns it) — so none of the
        // four have worked since the #540 cutover, and nothing else in
        // this file writes `h_sb_hovered`/`tab_close_hover`/
        // `debug_button_hovered`/calls `editor_hover_mouse_move`. This is
        // the single biggest confirmed-dead surface this issue found (see
        // the PR description) — restoring it needs a live, correctly
        // absolute-coordinate DA size (the removed code used a `(0, 0)`
        // origin the neighboring comment already flagged as the #582/#646
        // coordinate-frame bug, so it was not simply "wire the same code
        // back up"), which is follow-up work, not a dead-code deletion.

        // Explorer refresh after confirmed file move — GTK-only, no TUI
        // counterpart (see `Self::explorer_needs_refresh`'s own doc).
        if self.engine.borrow().explorer_needs_refresh {
            self.engine.borrow_mut().explorer_needs_refresh = false;
            self.refresh_file_tree();
        }

        // #1248: the rest of this function's chores — per-window viewport
        // sync, tab-visibility re-check, window title, yank-highlight clear,
        // idle/SC polling, deferred quit, terminal command, ext-panel focus,
        // platform-action drain — are shared with the pre-#1434 TUI shell's
        // `tick`. See `render::run_shared_tick_chores`'s header comment for
        // the full list.
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
    /// area to a character column on `content_line`, using Pango to
    /// measure proportional UI-font widths (#218). The legacy code
    /// did `(rel_x / cached_char_width)` which drifts as the column
    /// index grows because UI_FONT is proportional. Heading rows
    /// (font scale > 1.0) need the scale applied to the layout so
    /// `xy_to_index` returns the right position.
    ///
    /// #731: the Pango-measured path below was gated on
    /// `self.drawing_area`, permanently `None` under the ShellApp runner
    /// (nothing assigns it), so this has always taken the approximate
    /// `rel_x / char-width`-style fallback in practice — see `terminal_cols`
    /// for the same "no live font-metrics source without a widget handle"
    /// root cause.
    /// Run the shared editor-hover-popup rung (#755) against this frame's
    /// painted geometry and apply whatever it decides.
    ///
    /// Returns `true` when the press belonged to the popup and must not fall
    /// through to the editor. Called from `handle_mouse_click_msg` **above**
    /// the scroll-surface dispatch — this backend used to run its bespoke
    /// copy ~90 lines *below* it, which is why a click aimed at the popup's
    /// own scrollbar was swallowed by the surface painted behind it
    /// (#229/#486) — and from `handle_mouse_double_click_msg`, which never
    /// consulted the popup at all, so a double-click on it fell through to
    /// the editor's word-select (#490).
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
                // #1429: `units.hover_popup_pad` — was hardcoded to GTK's
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
            // #1134: `Engine::open_url` validates `is_safe_url` and queues a
            // `PendingPlatformAction::OpenUrl` for `tick_dispatch` to carry
            // out through `PlatformServices` — this method has no `backend`
            // handle of its own (see `Engine::pending_platform_actions`'s doc).
            self.engine.borrow_mut().open_url(&url);
        }
        if let Some(target) = effect.begin_drag {
            let drag_rc = backend.drag_state_handle();
            drag_rc.borrow_mut().begin(target);
            // Seek immediately, with the same thumb-aware math the drag
            // frames will use. This backend used to run a *second*,
            // ratio-based calculation at press time, so the thumb jumped
            // once on press and again on the first drag frame.
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

    /// Run the shared panel-hover-popup click rung (#1067) against this
    /// frame's painted link rects.
    ///
    /// Before #1067 this backend painted and cached `panel_hover_link_rects`
    /// (`render::panel_hover_popup_paint`, called from `render_content`) but
    /// never read them back on click — clicking a link in the source-control
    /// / extension-panel item dwell tooltip was a complete no-op on GTK,
    /// where TUI already copied the URL (or ran the `command:` link) via its
    /// own inline hit test in `mouse::handle_mouse`. `panel_hover_popup_paint`'s
    /// own doc traces this back to the #540 Relm4->ShellApp migration
    /// retiring `Msg::PanelHoverClick` without a replacement.
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
            // #1134: see `route_and_apply_editor_hover_popup`'s identical
            // comment above — `Engine::open_url` validates + queues.
            self.engine.borrow_mut().open_url(&url);
        }
        self.draw_needed.set(true);
        effect.consumed
    }

    /// Popup-content column under `rel_x` (pixels from the content origin).
    ///
    /// Used by the hover-selection *drag* follow-through; the press itself
    /// goes through `render::route_editor_hover_popup_click`, which divides by
    /// the same `col_width`. Before #755 this returned `rel_x as usize` — a
    /// column per *pixel* — so a drag-selection inside the popup ran off the
    /// end of the line on the first few pixels of travel and never agreed
    /// with the column the press had chosen.
    pub(crate) fn pixel_to_editor_hover_col(&self, rel_x: f64, _content_line: usize) -> usize {
        (rel_x.max(0.0) / self.cached_char_width.max(1.0)) as usize
    }

    /// Push or pop the editor hover popup on the modal stack so
    /// click dispatch can decide modal-vs-base for both left- and
    /// right-clicks (#216). The popup is registered whenever it's
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
    /// opened it. Mirrors the modal-stack arbitration
    /// (`quadraui::dispatch_mouse_down` for outside-click dismissal,
    /// `ContextMenuLayout::hit_test` for inner row resolution) that used to
    /// live inline in `handle_mouse_click_msg` (Phase B.5b Stage 4).
    ///
    /// Returns `true` iff a menu was open and this call consumed the click
    /// (dismissed it, fired an item, or kept it open on an inert row) — the
    /// caller should treat that as "handled, stop routing". Returns `false`
    /// when no menu was open, after defensively popping any stale
    /// modal-stack entry left by an Esc/Enter close the click handler never
    /// saw; the caller should then proceed with its own routing.
    ///
    /// Callable from both `handle_mouse_click_msg` (main-content clicks) and
    /// `try_route_sidebar_mouse_event` (#546 FAILED-2: an explorer-sourced
    /// menu typically renders inside the sidebar's own content bounds, so
    /// without giving it priority there too, clicks on it fell straight
    /// through to `TreeController`'s row hit-test underneath instead of
    /// firing the menu action or dismissing it).
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
    /// absolute DA coordinate frame mouse events arrive in (#582).
    ///
    /// Divider hit-testing must run against the geometry the renderer used, not
    /// a parallel re-derivation: `render_content` anchors `editor_bounds` at
    /// `AppShellLayout::main_content_bounds` (offset right by the activity
    /// bar/sidebar, down by the title-bar band), so any handler that rebuilt
    /// bounds at `(0.0, 0.0)` hit-tested a phantom divider displaced by that
    /// offset — the `:vsplit` failure in #582.
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
    /// [`render::BottomPanelMetrics::panel_left`] (#754). Falls back to `0.0`
    /// before the first frame, when there is no panel on screen to click.
    pub(crate) fn painted_bottom_panel_left(&self) -> f64 {
        self.cached_editor_bounds
            .get()
            .map(|(r, _)| r.x)
            .unwrap_or(0.0)
    }

    /// Resolve `(x, y)` against the currently-painted `screen.windows`,
    /// returning the name and rect of the editor-tab-hosted plugin view
    /// under it, if any (#1627, #1631).
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
    /// drawing-area measurement, for the #582 reason recorded there.
    /// #753 named it because the click arm and the drag arm both needed it and
    /// each used to re-derive its own half.
    ///
    /// `on_tab_bar` exists so a click on a group's tab bar reaches the tab
    /// handlers instead of arming a group-divider drag; it is deliberately
    /// GTK-only (see `render::DividerState::on_tab_bar`). It is also skipped
    /// entirely in single-group mode, where `group_dividers` is empty and
    /// nothing could match anyway.
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
    /// the caller's `FrameHitMap` (#449), so the map hit-tests the SAME
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

            // #1627: an editor-tab-hosted `vimcode.ui.register_view` view
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
                // #1631: same body-kind branch as the sidebar arm
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
            // #1690: with exactly one window (no `:split`/`:vsplit`), paint
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
                let backdrop_rect =
                    quadraui::Rect::new(0.0, bar_y as f32, backend.viewport().width, lh as f32);
                render::paint_status_backdrop(
                    backend,
                    &format!("status-backdrop:{}", rw.window_id.0),
                    backdrop_rect,
                    real_bg,
                );
            }
            // #672: recover segment hit zones the same way the dead
            // `draw.rs::draw_window_status_bar` did, so `pixel_to_click_target`'s
            // `WindowZone::StatusBar` arm has a real `status_segment_map` entry
            // to resolve against instead of an always-empty one.
            // `draw_status_bar` lays segments out bar-relative from `(0, 0)`
            // regardless of `sb_rect`'s own origin (see
            // `route_debug_sidebar_event`'s doc comment for the same
            // "`StatusBar::layout` always starts at 0,0" contract), which is
            // exactly the window-relative `local_x` `window_zone_hit_test`
            // hit-tests with — no coordinate translation needed.
            //
            // #764: this is the layout the *paint* resolved, not a second
            // `status_bar_layout` re-measure of the same bar as it used to be —
            // same reasoning as `render::PaintedTabBar::layout`.
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

        // `:split`/`:vsplit` boundaries had no visual of their own in GTK
        // before #582 — nothing told the user where to grab. Painted via
        // quadraui's `Split` primitive rather than hand-rolled Cairo. Both
        // axes: the #582 iteration-2 smoke found `:split` only *seemed*
        // draggable because the per-window status bar happens to sit one line
        // above the boundary and reads as a divider, which is a coincidence of
        // an unrelated feature, not a handle.
        //
        // These are the *within*-group dividers, hence part of this rung
        // rather than `EditorOp::GroupDividers` — the same split TUI makes,
        // where `render_all_windows` paints them via `render_separators`.
        render::draw_dividers_as_splits(backend, &screen.window_dividers, |div| {
            quadraui::WidgetId::new(format!("wdiv:{}:{}", div.group_id.0, div.split_index))
        });
    }

    /// The [`render::EditorOp::TabBars`] rung: paint one tab bar per editor
    /// group and recover the pixel hit geometry the rasteriser resolved.
    ///
    /// Multi-group (post-split) layouts get a bar per group at the top edge of
    /// its own bounds; a single group is a split of one and gets one
    /// full-width bar at the editor top (#515/#551).
    ///
    /// Painting goes through `render::paint_tab_bars` →
    /// `Backend::draw_tab_bar_icons` rather than a `Surface::TabBar` push,
    /// because quadraui's `Surface` enum carries no icon sidecar (adding a
    /// field to it would be the same hard break on downstream consumers that
    /// kept the icons off `TabItem` in the first place). With an empty sidecar
    /// the two are byte-identical — quadraui's `draw_tab_bar` forwards to
    /// `draw_tab_bar_icons` with `&[]` — so this is a pure superset of the old
    /// call (#703). `hit_bars` still collects a `Surface::TabBar` for the
    /// caller's `FrameHitMap`: that map is only ever consumed via `hit_map()`
    /// (never drawn) and its zones are whole-bar rects, which icons do not
    /// move.
    /// Compose the [`render::FrameOp::SidebarPanel`] rung: the *active panel's
    /// body*, into the content rect `AppShell` reserved for it.
    ///
    /// Extracted out of `render_content`'s walk (#766) because it was 210 of
    /// the walk's lines on its own — a `match` over seven panel ids, each with
    /// its own hit-test-cache publication — and #766's whole point is that
    /// `render_content` reads as the frame's *order*, not as the frame's
    /// contents. The surrounding sidebar chrome (activity bar, header,
    /// separator) is quadraui's, painted by the runner before `render_content`
    /// is entered; this fills only `q_sb`.
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
        // Which panel is active? #823 item 7: was its own restatement of
        // `render::sidebar_owner`'s resolution.
        let active_id: String = render::sidebar_owner(engine).panel_id_string();

        match active_id.as_str() {
            PANEL_EXPLORER => {
                // #1389: composed through `SidebarPanelBody::render_with`
                // (quadraui#1059) in a single call — `None`/`None` here
                // reproduce this arm's pre-existing "no chrome" behaviour
                // exactly (`layout.body_rect == q_sb`); TUI's
                // `panels::render_explorer_sidebar_content` uses the same
                // composer with `background: Some(tab_bar_bg)`. `render_with`
                // takes the body as a closure with no `Send + 'static` bound,
                // so the `!Send`, `Rc<RefCell<_>>`-backed `TreeController` on
                // `Engine` can be the body directly instead of the
                // hand-copied `render::paint_sidebar_panel_chrome` split
                // #1242 needed before #1059 existed.
                // #1693: `StatusBars` reserves one row above the tree for
                // the view-actions toolbar (New File / New Folder /
                // Refresh / Collapse All / "..." overflow) — the same
                // `SidebarPanelChrome` variant the Debug sidebar's title/
                // action bars already use (`debug_sidebar_chrome`, just
                // below). `body_rect` (what the closure receives) is
                // already narrowed below the chrome row, so
                // `explorer_tree_rect`/`explorer_viewport_rows` need no
                // change to account for it.
                let panel = render::SidebarPanelBody {
                    background: None,
                    chrome: render::SidebarPanelChrome::StatusBars(vec![
                        render::explorer_toolbar_status_bar(theme),
                    ]),
                    scrollbar_gutter: None,
                };
                render::populate_explorer_tree_controller(engine, theme);
                // Capture the exact metrics the tree is drawn with so the
                // click hit-test (which reads the backend's mutable
                // current_line_height at a later, possibly-different time) can
                // re-apply them and resolve the correct row. (#540)
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
                // #1065: `search_sidebar_system` never had `set_backend_info`
                // called on this backend — the exact #971 gap
                // (`render::UnitProfile::sidebar_system_metrics`'s own doc)
                // that left `SidebarSystem::handle_cached` returning
                // `SidebarEvent::Ignored` unconditionally, fixed for
                // `sc_sidebar_system` (this match's `PANEL_GIT` arm) and
                // `ext_sidebar_system` (`refresh_ext_sidebar_metrics`) but
                // missed here. Every content-row press *and* every wheel
                // notch over the search results list silently no-op'd —
                // `search_panel_click_focuses_the_query_field`'s click landed
                // on the query text box, a separate hit-test that never goes
                // through `handle_cached`, so it never caught this.
                let search_lh = backend.line_height();
                engine
                    .search_sidebar_system
                    .borrow_mut()
                    .set_backend_info(search_lh, (self.units.sidebar_system_metrics)(search_lh));
                render::populate_search_sidebar_system(engine, &engine.cwd, theme);
                engine.search_sidebar_body_rect.set(q_sb);
                engine.search_sidebar_system.borrow().render(backend, q_sb);
            }
            PANEL_DEBUG => {
                // #1392: composed through `SidebarPanelBody::render_with`
                // (quadraui#1059) with `SidebarPanelChrome::StatusBars`
                // (quadraui#1061, `render::debug_sidebar_chrome`) instead of
                // slicing `q_sb` into a title row + action row by hand and
                // calling `Backend::draw_status_bar` on each directly. The
                // returned layout's `status_bar_hit_regions` are already in
                // `q_sb`'s own absolute pixel space, so they're stored
                // straight onto `engine.dap_sidebar_action_hits` for
                // `route_debug_sidebar_event` to read — no more separately
                // cached `action_rect` to translate a press into (that was
                // a second, independently-derived copy of the same
                // geometry the paint used, which is exactly what let paint
                // and click disagree). TUI's `render_debug_sidebar` builds
                // the identical chrome through the same helper.
                let panel = render::SidebarPanelBody {
                    background: None,
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
                // #1390: composed through `SidebarPanelBody::render_with`
                // (quadraui#1059, the composer the `PANEL_EXPLORER` arm
                // uses, #1389) instead of operating on `q_sb` directly with
                // no wrapper. `chrome: SidebarPanelChrome::None` — the
                // shell's own sidebar header already titles this panel
                // "SOURCE CONTROL", so `sc_header_status_bar`'s row below
                // (live branch/ahead-behind) is body content, not a second
                // title (#1256's double-header bug). `None` chrome reserves
                // no rows, so `body_rect` is pixel-identical to `q_sb` —
                // shadow `q_sb` with it below rather than threading a second
                // name through every band/popup rect in this arm.
                let panel = render::SidebarPanelBody {
                    background: None,
                    chrome: render::SidebarPanelChrome::None,
                    scrollbar_gutter: None,
                };
                panel.render_with(backend, q_sb, |backend, body_rect| {
                    let q_sb = body_rect;
                    if let Some(ref sc) = screen.source_control {
                        // Header row + commit-input box (#480). Previously
                        // entirely unpainted under ShellApp — the only place
                        // that ever drew them was the dead
                        // `draw.rs::draw_source_control_panel` Cairo painter,
                        // which has zero live callers (superseded by this
                        // `render_content` path back when the 14 legacy DAs
                        // were collapsed into one, #493). Paint them for
                        // real now that quadraui#222 (TextInput) has landed,
                        // through the same `render::sc_*` adapters TUI uses
                        // so the two renderers can't drift.
                        // Band geometry (header / commit box / slab) comes from
                        // the shared `render::sc_sidebar_bands` so the click
                        // router in `try_route_sidebar_mouse_event` resolves a
                        // press against the *same* derivation that painted it
                        // (#544). `SC_COMMIT_BORDER_PX` is the primitive's 1px
                        // border top+bottom — GTK's native unit is pixels,
                        // unlike TUI's whole-cell border (see
                        // `render::sc_commit_input_box_height` doc).
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
                        // Focused-hint row (#1361): shared with TUI through
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
                        // #971: without this, `sc_sidebar_system.handle_cached`
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

                        // Branch picker / create popup (dual-mode Palette,
                        // quadraui#224) and help dialog (Dialog + DialogTable,
                        // quadraui#225) — both keyboard-reachable via
                        // `dispatch_sc_sidebar_key_unified` even though the
                        // git sidebar has no live mouse-click routing yet
                        // (#449 tracks that separately). Render over the
                        // whole sidebar content area, same popup-over-panel
                        // z-order TUI uses.
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
                // #1343: the shell's own `AppShell` sidebar header already
                // paints " EXTENSIONS " above `q_sb` — this arm paints only
                // the search/filter row (#1256 found GTK painting neither
                // row at all).
                //
                // #1391: composed through `SidebarPanelBody::render_with`
                // (quadraui#1059, the composer the `PANEL_EXPLORER`/
                // `PANEL_GIT` arms use) with `SidebarPanelChrome::Search`
                // (quadraui#1061) instead of the bespoke `render::
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
                // #521: generic Board panel host — the *only* GTK-specific
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
                // #1343: Settings is a shell bottom item that owns the
                // sidebar header (quadraui#1055, #1356's pin bump) — the
                // shell paints " SETTINGS " above `q_sb`, so this arm
                // paints only the filter row, instead of the pre-#1343
                // `SidebarPanelChrome::None` (no chrome at all — #1256's
                // `settings_filter_row_is_painted::gtk` gap).
                //
                // #1391: composed through `SidebarPanelBody::render_with`
                // with `SidebarPanelChrome::Search` (quadraui#1061) instead
                // of the bespoke `render::paint_sidebar_search_row` (deleted
                // by this issue) — see `render::search_only_chrome`'s doc.
                // TUI's `panels::render_settings_panel` builds the identical
                // chrome through the same helper.
                //
                // #1574: `background` is `Some(theme.tab_bar_bg)`, not
                // `None` — the same sidebar background the `PANEL_EXPLORER`
                // arm's doc comment above says TUI passes for this composer.
                // With `None`, nothing fills the strip under the settings
                // scrollbar's gutter, so the backend's own clear colour
                // (dark on macOS regardless of the active colourscheme)
                // showed through there instead of the themed sidebar bg.
                // GTK-only: `src/gtk/testing.rs`'s
                // `settings_panel_scrollbar_gutter_paints_theme_tab_bar_bg`
                // covers it with a pixel probe (that module's own doc
                // records why an equivalent TUI probe can't reproduce this
                // one — `FormController` already paints an opaque per-row
                // background there regardless of this field).
                let panel = render::SidebarPanelBody {
                    background: Some(theme.tab_bar_bg),
                    chrome: render::search_only_chrome(
                        &engine.settings_query,
                        "",
                        engine.settings_input_active,
                        theme,
                    ),
                    scrollbar_gutter: None,
                };
                panel.render_with(backend, q_sb, |backend, body_rect| {
                    // Cache the exact rect this frame painted the form into
                    // (mirrors TUI's `panels::render_settings_panel`, #1238)
                    // — `try_route_sidebar_mouse_event`'s `Settings` arm
                    // reads this back instead of the raw, unshrunk `q_sb`,
                    // which would resolve clicks against `FormController`'s
                    // row geometry one search-row off from what was
                    // actually painted (#1343 review: this is exactly the
                    // `settings_row_click_below_its_glyph_hits_its_own_row_gtk`
                    // regression a stale `sb` reintroduced).
                    engine.settings_form_rect.set(body_rect);
                    render::populate_settings_form_controller(engine);
                    engine
                        .settings_form_controller
                        .borrow_mut()
                        .render_and_cache(backend, body_rect);
                });
            }
            id if id
                .strip_prefix("ext:")
                .is_some_and(|name| engine.is_plugin_view(name)) =>
            {
                // #146: a `vimcode.ui.register_view` panel — the plugin declared
                // a widget tree, so paint it as a `quadraui::Form` through the
                // *same* shared `FormController` the `PANEL_SETTINGS` arm above
                // uses, rather than as `ExtPanelItem` tree rows. This is the
                // whole per-surface cost of plugin UI: one arm in the shared
                // shell, zero lines in `src/gtk/` or `src/tui_main/`.
                //
                // #1631: a view whose `render()` returned a `kind = "..."`
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
                // #1089: a plugin-provided panel — paint its own sections +
                // items via `render::ext_panel_to_tree_view` +
                // `Backend::draw_tree`, the same adapter
                // `tui_main::panels::render_ext_panel` uses, instead of
                // falling through to `ext_sidebar_system` (the extension
                // *marketplace* — INSTALLED/AVAILABLE — which is what this
                // arm painted before this fix, unconditionally, for every
                // `ext:<name>` id).
                //
                // #1242: chrome + body now composed through quadraui#1041's
                // `SidebarPanelBody::render` (`render::ExtPanelTreeBody`,
                // same `&dyn BackendWidget` path `panels::render_ext_panel`
                // uses). `scrollbar_gutter: None` — unlike TUI, this arm has
                // never painted one.
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
                        // #1089: cache the exact `Backend::tree_layout` this
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

                    // #636: the `?`-triggered keybindings help popup, drawn
                    // over the whole panel content area via `Tooltip` +
                    // `Backend::draw_tooltip_with_chrome` (JDonaghy/
                    // quadraui#541, landed at this repo's pinned rev) —
                    // see `render::ext_panel_help_tooltip_layout`'s own doc
                    // for why this is the real fix and not a stand-in.
                    // Nothing painted this popup at all before this change
                    // (the 2026-10-05 triage update on the issue): `?` set
                    // `Engine::ext_panel_help_open` but no backend ever read
                    // it back.
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
                // #819: adopts quadraui's `ChatController` — one shared
                // `render()` for both backends, like `explorer_tree` and
                // `ext_sidebar_system` above, replacing the hand-painted
                // `draw_ai_sidebar_panel`. `ai_chat_rect` is cached on
                // `Engine` (not here) so `route_ai_chat_event` re-derives
                // the identical layout `render()` painted (#544/#582/#646).
                render::populate_ai_chat_controller(engine, theme, backend);
                // #1513: the pinned plan block gets a band carved off the
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
                // #956 (ACP-5): slash-command completions, painted on top —
                // no-op unless the input matches an agent-declared command.
                // `chat_rect` (not `q_sb`), per `paint_ai_command_completions`'s
                // own contract: the same rect `ai_chat.render()` just used,
                // so the popup anchors off the input box's *actual* position
                // — unaffected by the #1513 plan-band split in the common
                // case (the band only eats space off the top), but wrong to
                // silently keep passing the unshrunk sidebar rect here now
                // that the two can differ.
                render::paint_ai_command_completions(backend, engine, chat_rect);
                // `cached_explorer_metrics`'s drift guard, ported: `backend`'s
                // "current" line_height/char_width are mutable and can be
                // overwritten by whatever paints next this frame or the
                // next, so capture what `render()` actually used here for
                // `route_ai_chat_event` to re-apply before `handle()` (#819).
                self.cached_ai_chat_metrics
                    .set((backend.line_height() as f64, backend.char_width() as f64));
            }
            _ => {}
        }

        // The sidebar-item hover popup used to be composed here,
        // nested inside this rung. #765 lifted it out to
        // `BottomOp::PanelHover`: as a *sidebar* rung it could
        // only run while `sidebar_content_bounds` was `Some`,
        // so collapsing the sidebar left
        // `panel_hover_popup_rect` pinned at its last painted
        // value and `handle_mouse_press` went on arbitrating
        // clicks against a popup that was no longer on screen.
    }

    /// Re-derive `ext_sidebar_system`'s backend metrics from what `backend`
    /// is about to paint with (#971).
    ///
    /// Without this, `ext_sidebar_system.handle_cached` returns `Ignored`
    /// unconditionally and every content-row press on the plugin ext panel
    /// (header collapse, row select) is a silent no-op — see
    /// `render::UnitProfile::sidebar_system_metrics`'s own doc for the full
    /// story. Reads `backend.line_height()` directly rather than accepting a
    /// cached `lh` parameter — see the `PANEL_GIT` arm's identical comment
    /// in [`Self::paint_sidebar_panel_rung`] on why: a cached value can lag
    /// behind what `backend` reports by the time `render()` actually reads
    /// it, so the metrics `handle_cached` hit-tests against could disagree
    /// with what this exact `render()` call paints. Shared by the
    /// `PANEL_EXTENSIONS` arm and the `id if id.starts_with("ext:")` arm
    /// above, which were previously two verbatim copies of this same
    /// four-line snippet.
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
    /// Extracted out of `render_content` (#766) for the same reason as
    /// [`Self::paint_sidebar_panel_rung`] — it was ~160 lines of geometry in
    /// the middle of what #766 makes a statement of the frame's *order*.
    /// Deliberately **not** a `FrameOp` rung: these are anchored to the active
    /// window's cursor rather than to a band, and both backends compose them at
    /// exactly this point (TUI through `render_impl.rs`'s `paint_editor_popups`),
    /// between the editor band and the bottom band.
    ///
    /// #1167: the build-adapter → `.layout()` → `backend.draw_*` → cache-
    /// output part (identical to TUI's `render_impl::paint_editor_popups`,
    /// modulo coordinate units) now lives once in `render::paint_editor_popups`.
    /// #1237 folded the anchor-point arithmetic itself into
    /// `render::editor_popup_anchors` too (see its doc — GTK's completion
    /// anchor, and all four non-completion anchors on *both* backends, used
    /// the raw character column as a tab-expanded display column, drifting
    /// left of the cursor on tab-indented lines). This method's own job
    /// shrank to exactly what stays genuinely per-backend: finding the
    /// active window, scaling into GTK's native pixel units (`lh`/`cw`),
    /// and picking each popup's clip viewport.
    ///
    /// `main` is `AppShellLayout::main_content_bounds` — the clip viewport
    /// every popup but the completion menu and editor-hover popup is placed
    /// inside; those two clamp to the active window's own rect instead (see
    /// `render::paint_editor_popups`'s doc for why).
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

        // #1237: the char→display column arithmetic (tab expansion, scroll
        // offset) for every one of the five anchors below now lives once in
        // `render::editor_popup_anchors`, shared with TUI's
        // `render_impl::paint_editor_popups`. GTK uses its raw sub-pixel
        // float window origin as-is (no whole-cell snapping — that's a
        // TUI-only concern).
        //
        // Note the precision drop: `win_origin`/`cw`/`lh` are cast to `f32`
        // *here*, before the anchor arithmetic runs, whereas pre-#1237 each
        // anchor's arithmetic ran entirely in `f64` and only cast to `f32`
        // at the very end for `PopupAnchor`. `editor_popup_anchors` is
        // shared with TUI, whose native units are already `f32` cell
        // columns, so it takes `f32` throughout — GTK pays for that sharing
        // with one extra rounding step. Immaterial at realistic screen-pixel
        // magnitudes (`f32` keeps ~7 significant decimal digits, far more
        // than any on-screen coordinate needs), and reviewed as acceptable
        // in #1237's follow-up round rather than threading `f64` generics
        // through a function TUI also calls.
        let win_origin = active_win.map(|w| (w.rect.x as f32, w.rect.y as f32));
        let anchor_points = render::editor_popup_anchors(screen, win_origin, cw as f32, lh as f32);

        // Completion popup anchor — cache the layout so the click handler
        // (B.5b Stage 5) can hit-test items and register the popup on the
        // modal stack.
        let completion = screen.completion.as_ref().and_then(|menu| {
            let (cursor_x, cursor_y) = anchor_points.completion?;
            // Longest candidate + 4 cells of padding/border, floored at 12
            // cells' worth of raw units (`cw`-scaled, not a bare pixel
            // constant — #1432: this function runs unmodified for both `gtk`
            // and TUI-via-`App`, and a flat `100.0` floor is a GTK pixel
            // width. On TUI `cw` is `1.0` (cell = 1 raw unit), so that same
            // constant used to demand a 100-*cell*-wide popup — wider than
            // most terminals — which made `Completions::layout`'s own
            // right-edge-overflow branch always fire and clamp `x` straight
            // back to `viewport.x`, discarding the correctly-computed cursor
            // anchor outright (observed: popup pinned at the window's left
            // edge regardless of cursor column). Both the `+4`/`.max(12.0)`
            // now match `tui_main::render_impl`'s independent
            // completion-width calc exactly (its own comment there names
            // the same reasoning) — this used to read `+2`, which happened
            // to never matter while `100.0` always dominated it, but once
            // scaled down to a real cell-sized floor a too-narrow candidate
            // term clipped the popup's own last character (observed:
            // `"ZQXWFOOBAR"` painted as `"ZQXWFOOBA"`, missing the border's
            // trailing padding cell). Scaling the shared `12.0` floor by
            // `cw` here keeps GTK's effective minimum close to its old
            // ~100px (12 * a typical ~8px cell) while giving TUI-via-`App`
            // the same sane 12-cell floor `render_impl` already ships.
            //
            // #420: `Completions::layout` clamps the popup's *position*
            // into `win_viewport` (`x.max(viewport.x)`) but never clamps
            // `popup_w` itself, so a long enough candidate label can still
            // render past the window's own right edge even once positioned
            // as far left as it can go. That's a gap in the shared
            // `quadraui::Completions::layout` primitive (it already clips
            // height the same way, via `clipped_h`), not something to patch
            // around per-backend — see the Platform-Neutrality Rule.
            // Tracked as a follow-up pending a quadraui-side fix rather than
            // duplicating a `.min(...)` clamp here and in
            // `tui_main::render_impl`.
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
        // scrollbar geometry are cached for the click + drag handlers
        // (#215), same as `draw.rs::draw_editor_hover_popup` did.
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
    /// `dispatch_scroll` / `pixel_to_click_target` to resolve against
    /// (#592/#672). Extracted out of `render_content` (#766) so that function
    /// reads as the frame's *order* and nothing else.
    pub(crate) fn sync_per_frame_backend_state(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        theme: &Theme,
    ) {
        backend.set_theme(render::to_quadraui_theme(theme));
        // #1580: re-synced every frame, same reasoning as the theme sync
        // right above — a runtime `:set menu_style=...` (or its Settings-
        // sidebar equivalent) reaches quadraui's own `MenuStyle` on the
        // very next paint. `set_menu_style` only persists a value on the
        // backend struct (no popup, no blocking call), so calling it
        // every frame is as cheap as the theme/font syncs it sits next to.
        backend.set_menu_style(render::to_quadraui_menu_style(engine.settings.menu_style));
        // (#547) Re-synced every frame so runtime toggles (`:set
        // nonerdfonts`) take effect immediately, matching TUI.
        render::sync_nerd_fonts(backend, engine);
        // Re-synced every frame so a runtime `:set guifont`/font-size change
        // takes effect immediately (#217/#672). Ported from the dead
        // `draw.rs::draw_editor`'s top-of-frame call, which was the only
        // live caller — `UI_FONT()` (used a few paint calls below for the
        // raw-Pango chrome that doesn't go through `Backend::set_ui_font`)
        // read the process-global atomic this writes, so before this port it
        // silently stayed pinned at the default size forever.
        sync_ui_font_size(&engine.settings, backend);
        // #705 item 3 / quadraui#624: push the same UI_FONT() family+size
        // onto the *paint* backend's `ui_font`, which `draw_status_bar`
        // (breadcrumbs, per-window/global status lines), `draw_tree`
        // (explorer), `draw_tab_bar_icons`, and `draw_menu_bar` now all
        // honour for both paint and their no-paint measurement twins
        // (quadraui#624). Before this call `ui_font` on the paint backend
        // was never touched, so it
        // sat at quadraui's own "Sans 11" default forever: chrome text
        // didn't track `settings.ui_font_size`, and — per #700's item 3 —
        // status-bar-painted breadcrumb text had no font of its own to
        // decouple it from whatever font a prior draw call in the frame
        // left on the shared Pango layout. Re-set every frame (not just
        // once from `setup()`) so a runtime `:set ui_font_size=N` takes
        // effect immediately, matching `sync_ui_font_size`/`sync_nerd_fonts`
        // just above.
        backend.set_ui_font(&UI_FONT());
        // #947 / quadraui#422: push `settings.font_family`/`font_size` onto
        // the *paint* backend's editor font every frame, so a runtime
        // `:set guifont`/`:set font_size=N`/`zoomin`/`zoomout` reaches the
        // painted editor text on the very next frame — mirroring
        // `set_ui_font` immediately above. Before this call nothing in
        // vimcode ever read `settings.font_family`/`font_size` (`grep -rn
        // "set_editor_font" src/` returned zero hits before this issue), so
        // the editor painted at whatever default quadraui's `GtkBackend`/
        // `MacBackend` ship with ("Monospace 11") regardless of the setting.
        //
        // #1542: resolved through `resolve_editor_font` (same function
        // `shell_config`'s pre-seed above uses) instead of the raw fields,
        // so an un-customized setting keeps tracking this backend's
        // platform-native convention every frame, not just at startup.
        let (editor_family, editor_size_pt) = resolve_editor_font(&engine.settings, backend);
        backend.set_editor_font(&editor_family, editor_size_pt);
        // #1864: on the native macOS GUI, override `set_editor_font`'s own
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

        // #672: scroll surfaces are re-registered from scratch every frame
        // (mirrors TUI's `render_impl.rs` `scroll_surfaces.borrow_mut().clear()`)
        // so a panel that closes — or moves — doesn't leave a stale entry
        // behind for `dispatch_scroll`/`dispatch_click` to hit-test against.
        // Ported from the dead `draw.rs::draw_editor`'s equivalent top-of-frame
        // clear, which was this list's only writer under `ShellApp` (#592/#672).
        engine.scroll_surfaces.borrow_mut().clear();
        // Per-window status bar segment hit zones (#672): `click.rs`'s
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

    /// Compose the **editor band** (#764, #735 slice 3; converged into one
    /// shared walk by #1251) and recover this frame's `FrameHitMap` from it.
    ///
    /// Walks `render::paint_editor_band_rungs`, the single loop both this
    /// method and the pre-#1434 TUI shell's `paint_editor_band` now call —
    /// see that function's own doc for exactly which rungs need `self` and
    /// why. Extracted out of `render_content` (#766) so that function reads
    /// as the frame's *order*.
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
        // geometry (#515). Cleared here, before the walk, rather than from an
        // absent-rung branch: `compose_editor_band` returns only the live
        // rungs, so a frame with no tab bars has no arm to clear them from.
        self.cached_group_tab_bar_layouts.borrow_mut().clear();
        self.cached_tab_slots_abs.borrow_mut().clear();
        // #1165: reset alongside the hit caches above — this frame's
        // `TabBars` rung (if any) repopulates it, and `handle_poll_tick`
        // wants this frame's measurements, not an accumulation across
        // frames (mirrors TUI's `tab_visible_counts.borrow_mut().clear()`).
        self.tab_visible_counts.borrow_mut().clear();

        // `window_editors`/`hit_bars` stash what the `Windows`/`TabBars`
        // rungs painted, past the walk (#449), so the `FrameHitMap` built
        // below references the SAME objects just painted rather than a
        // second copy that could drift — see `render::paint_editor_band_rungs`'s
        // doc for why this bookkeeping can't move into the shared walk itself.
        //
        // #1128 (was #731): both scrollbars ARE painted for the editor on
        // GTK today. quadraui#968 taught `gtk::editor::draw_editor` (the
        // rasteriser `Surface::Editor` below calls into) to paint both
        // scrollbars itself, the way `tui::editor::draw_editor` already
        // did — geometry comes from `Editor::layout`, the same call
        // hit-testing uses, so paint and click agree by construction (see
        // that rasteriser's module doc, "Scrollbars" section). This
        // replaced the #731-era claim that GTK "deliberately skips
        // scrollbars and defers to the host" — that sentence described the
        // dead Relm4-era native `gtk4::Scrollbar` overlay path
        // (`sync_scrollbar`/`create_window_scrollbars`) #731 deleted, which
        // never ran under the ShellApp runner in the first place (nothing
        // assigns `self.overlay`/`self.drawing_area`).
        //
        // `app_support::editor_scrollbar_layout` builds the same
        // `Editor`/`EditorLayout` pair (minus the rendered text, which
        // scrollbar geometry never reads) so the axis-parameterised
        // `scrollbar_thumb_geometry`/`scrollbar_hit_test` (#1493, was one
        // hand-rolled pair per axis) below can never resolve a hover/drag
        // rect paint didn't actually draw — #1128 deleted the pre-#968
        // h-scrollbar geometry helper that independently guessed its own
        // track width and could disagree with what was actually painted.
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

        // ── Recover a FrameHitMap for Editor/TabBar zone detection (#449) ──────
        // Pure `.push()` accumulation into a *separate* `ScreenLayout`, built
        // from the same `Editor` objects painted by the `Windows` rung above
        // (`window_editors`, same order as `screen.windows` so
        // `FrameZone::Editor { idx }` maps straight back to
        // `cached_layout.windows[idx]`) plus the `TabBar` surfaces the
        // `TabBars` rung recorded. `ScreenLayout::hit_map()` (quadraui#425)
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
// This impl compiles alongside the Relm4 path but is NOT wired up.
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
    /// sequence; they ride the `MenuDropdown` rung instead (#735).
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
            // leading slot (#720). `MenuSystem::render` positions the open
            // dropdown from this same rect, so passing the narrowed one is
            // what keeps a dropdown under the label that opened it.
            engine.menu_system.borrow().render(backend, menu_items_rect);

            // ── App icon, left of `File` (#720) ──────────────────────────
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
                // [`crate::render::app_icon_image`] builder here (#1102).
                let _ = backend.draw_image(app_icon_rect, &app_icon_image_for_paint());
            }
        }

        // ── Inline window controls (min/max/close) — after the bar (#552) ────
        // `menu_system.render()` above repaints `draw_menu_bar` across the full
        // `menu_row_rect` band, so the controls must be painted *after* it or
        // they get erased (the round-2/3 "buttons render blank" regression).
        // The controls sit in the title-bar band, to the right of the menu
        // labels; the dropdown body drops *below* the band, so painting here
        // never covers an open dropdown.
        //
        // #735 moved this from the very end of `render_content` (below the
        // dialog and context menu) to here. It is title-bar chrome, so the
        // modal rungs of `render::FRAME_Z_ORDER` now paint over it — which is
        // the point: a modal dialog covering the window controls is what
        // "modal" means, and it is what TUI already did with everything it
        // painted into its own title-bar row.
        if let Some(controls_rect) = controls_rect {
            // #1234: read live via `WindowControl::is_maximized()` rather
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
