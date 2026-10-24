use super::construction::dialog_btn_index;
use super::*;

impl App {
    /// Compose the **bottom band**: the chrome vimcode stacks below the
    /// editor column — quickfix, the terminal/debug bottom panel, the debug
    /// toolbar, the separated status line and the sidebar hover popup.
    ///
    /// Walks `render::compose_bottom_band`; geometry stays here, in pixels,
    /// mirroring `compute_editor_layout`'s `unit_h = line_height` convention.
    /// Extracted out of `render_content` so that function reads as the
    /// frame's *order* and nothing else. `main` is
    /// `AppShellLayout::main_content_bounds`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compose_bottom_band_rungs(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        screen: &render::ScreenLayout,
        layout: &quadraui::AppShellLayout,
        theme: &Theme,
        main: quadraui::Rect,
        el: &render::EditorLayout,
        editor_area_h: f64,
        metrics: (f64, f64),
    ) {
        let (lh, cw) = metrics;
        let _ = cw;

        // ══ Bottom band ════════════════════════════════════════════════════════
        //
        // Composed from `render::compose_bottom_band` — the single ordered
        // artefact both backends walk for the chrome stacked below the editor
        // column, exactly as `EDITOR_Z_ORDER` (above) is for the column itself
        // and `CHROME_Z_ORDER` (below) for the surrounding chrome. Geometry
        // stays here, in pixels, mirroring `compute_editor_layout`'s
        // `unit_h = line_height` convention; only the *order* and the *gates*
        // moved. `BOTTOM_Z_ORDER`'s doc comment records the convergences that
        // unified the backends' bottom-band logic.
        //
        // `editor_area_h` above (`el.editor_bottom`) already reserves the whole
        // stack, so these bands sit directly below it with no gap and no
        // overlap with `status_y`.
        //
        // Caches whose owning rung may be gated off are cleared *here*, before
        // the walk, never from an `else` arm inside it: `compose_bottom_band`
        // returns only the live rungs, so an absent rung has no arm to run.
        engine.bottom_panel_geometry.replace(None);
        self.debug_toolbar_y_offset.set(0.0);
        self.debug_toolbar_height.set(0.0);
        self.separated_status_bar_rect.set(None);
        self.panel_hover_popup_rect.set(None);
        self.panel_hover_link_rects.borrow_mut().clear();

        let quickfix_y = main.y as f64 + editor_area_h;
        let terminal_y = quickfix_y + el.quickfix_h;
        let debug_toolbar_y = terminal_y + el.terminal_h;
        let separated_status_y = debug_toolbar_y + el.debug_toolbar_h;
        let mut composed_bottom: Vec<render::BottomOp> = Vec::new();
        for op in
            render::compose_bottom_band(engine, screen, layout.sidebar_content_bounds.is_some())
        {
            match op {
                render::BottomOp::Quickfix => {
                    let Some(ref qf) = screen.quickfix else {
                        continue;
                    };
                    // The "keep the selection visible" offset is recomputed statelessly
                    // each frame through the shared `quickfix_scroll_top`, so the two
                    // backends cannot disagree about what that means.
                    let visible_rows = ((el.quickfix_h / lh) as usize).saturating_sub(1);
                    render::paint_quickfix_rung(
                        backend,
                        qf,
                        quadraui::Rect::new(
                            main.x,
                            quickfix_y as f32,
                            main.width,
                            el.quickfix_h as f32,
                        ),
                        render::quickfix_scroll_top(qf, visible_rows),
                    );
                    composed_bottom.push(render::BottomOp::Quickfix);
                }
                render::BottomOp::BottomPanel => {
                    // An explicit width in `self.units`'s own units on every
                    // backend — real pixels on GTK/macOS/Win, one cell on
                    // TUI, matching quadraui's own
                    // `Backend::terminal_scrollbar_default_width` so the
                    // painted gutter is unchanged. Passing `None` here would
                    // leave vimcode's own `ScrollSurface` with no `scrollbar`
                    // to hit-test thumb-drag/track-click against (mouse-wheel
                    // scroll needs only `surface.bounds`, so it would keep
                    // working while clicking/dragging silently did nothing).
                    let terminal_scrollbar = Some(self.units.terminal_scrollbar_gutter as u16);
                    render::paint_bottom_panel_rung(
                        backend,
                        engine,
                        screen,
                        theme,
                        quadraui::Rect::new(
                            main.x,
                            terminal_y as f32,
                            main.width,
                            el.terminal_h as f32,
                        ),
                        render::BottomPanelUnits::new(lh, cw, terminal_scrollbar),
                    );
                    composed_bottom.push(render::BottomOp::BottomPanel);
                }
                render::BottomOp::DebugToolbar => {
                    let rect =
                        quadraui::Rect::new(main.x, debug_toolbar_y as f32, main.width, lh as f32);
                    render::draw_debug_toolbar(backend, engine, rect);
                    self.debug_toolbar_y_offset.set(debug_toolbar_y);
                    self.debug_toolbar_height.set(lh);
                    composed_bottom.push(render::BottomOp::DebugToolbar);
                }
                // Shown below the terminal band when `window_status_line` is on
                // but `status_line_above_terminal` is off (see
                // `compute_editor_layout`'s `has_separated`).
                // `el.editor_bottom` already reserved `el.separated_status_h`
                // of vertical space right here — between the debug toolbar and
                // `status_y` below.
                render::BottomOp::SeparatedStatus => {
                    let Some(ref status) = screen.separated_status_line else {
                        continue;
                    };
                    let sb_rect = quadraui::Rect::new(
                        main.x,
                        separated_status_y as f32,
                        main.width,
                        el.separated_status_h as f32,
                    );
                    // Backdrop fill, full window/terminal width (not
                    // `main.width` — main-content-column-only), painted
                    // *before* the real, window-bounded bar below — see
                    // `render::paint_status_backdrop`'s doc (the same
                    // helper `paint_editor_windows_rung`'s single-window
                    // case uses) for why this is a separate, content-free
                    // paint rather than widening `sb_rect` itself.
                    let real_bg = status
                        .left_segments
                        .first()
                        .or(status.right_segments.first())
                        .map(|s| quadraui::Color::rgb(s.bg.r, s.bg.g, s.bg.b));
                    let backdrop_rect = quadraui::Rect::new(
                        main.x,
                        separated_status_y as f32,
                        main.width,
                        el.separated_status_h as f32,
                    );
                    render::paint_status_backdrop(
                        backend,
                        "status-backdrop:separated",
                        backdrop_rect,
                        real_bg,
                    );
                    // Segment hit-zone recovery keyed by
                    // `active_window_id` — the separated line shows the active
                    // window's status, so that's the id `pixel_to_click_target`
                    // looks its zones up under. The layout comes back from the
                    // paint itself now rather than from a second
                    // `status_bar_layout` call on the same rect.
                    let sb_layout = render::paint_separated_status_rung(backend, status, sb_rect);
                    self.status_segment_map.borrow_mut().insert(
                        screen.active_window_id.0,
                        render::status_bar_zones_from_layout(&sb_layout),
                    );
                    self.separated_status_bar_rect.set(Some(sb_rect));
                    composed_bottom.push(render::BottomOp::SeparatedStatus);
                }
                // Source-control / extension-panel item dwell tooltip, rendered
                // markdown, via the shared `quadraui::RichTextPopup` path.
                // Clamped against the full content viewport so it can extend
                // rightward into the editor area past the sidebar's own bounds
                // — which is why it is composed here, after everything it can
                // overhang, rather than inside the sidebar rung where it used
                // to live.
                render::BottomOp::PanelHover => {
                    // `sidebar_open` — the gate this rung was composed behind
                    // — *is* `sidebar_content_bounds.is_some()`, so this
                    // `else` is unreachable; kept as the same
                    // `let … else { continue }` shape the sibling arms use
                    // rather than an `unwrap` that would panic if the gate and
                    // the anchor ever stopped agreeing.
                    let Some(q_sb) = layout.sidebar_content_bounds else {
                        continue;
                    };
                    let hover_viewport = main;
                    let (links, rect) = render::panel_hover_popup_paint(
                        backend,
                        screen,
                        theme,
                        q_sb.x + q_sb.width,
                        q_sb.y,
                        hover_viewport,
                        main.width,
                        lh as f32,
                    );
                    self.panel_hover_popup_rect.set(rect);
                    *self.panel_hover_link_rects.borrow_mut() = links;
                    composed_bottom.push(render::BottomOp::PanelHover);
                }
            }
        }
        *self.composed_bottom_band.borrow_mut() = composed_bottom;
        // Debug-only: a rung hoisted back out of the walk, or composed early,
        // shows up here as a diagnosable string rather than a visual mystery.
        if let Err(why) = render::check_bottom_band_order(&self.composed_bottom_band.borrow()) {
            debug_assert!(false, "{why}");
        }
    }

    pub(crate) fn paint_tab_bars_rung<'a>(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        screen: &'a render::ScreenLayout,
        tab_row_h: f64,
        tab_bar_h: f64,
        hit_bars: &mut Vec<(core::window::GroupId, quadraui::Rect, &'a quadraui::TabBar)>,
    ) {
        let painted = render::paint_tab_bars(
            backend,
            engine,
            screen,
            tab_row_h,
            tab_bar_h,
            self.tab_close_hover
                .map(|(gid, i)| (core::window::GroupId(gid), i)),
        );
        let char_width = backend.char_width();
        let mut group_layouts = self.cached_group_tab_bar_layouts.borrow_mut();
        let mut slots_abs = self.cached_tab_slots_abs.borrow_mut();
        let mut visible_counts = self.tab_visible_counts.borrow_mut();
        for bar in painted {
            // The engine-feedback "how many columns of tab
            // strip fit" number `Engine::set_tab_visible_count` budgets
            // against — see `click::tab_bar_available_cols`'s doc for why
            // this reads the paint's own rect/layout instead of the
            // deprecated `TabBarHits::available_cols`.
            visible_counts.push((
                bar.group_id,
                tab_bar_available_cols(bar.rect, &bar.layout, char_width),
            ));
            // Cache the exact pixel geometry the rasteriser just drew for
            // hit-testing and tab-drop geometry. The painted layout already has
            // the correct icon-adjusted widths, so reading the paint's own
            // answer avoids measure/paint desync.
            slots_abs.insert(
                bar.group_id.0,
                abs_slot_positions_from_layout(bar.rect, bar.bar, &bar.layout),
            );
            hit_bars.push((bar.group_id, bar.rect, bar.bar));
            group_layouts.insert(bar.group_id.0, (bar.rect, bar.layout));
        }
    }

    /// Recompute the per-group tab-drop geometry from this frame's screen
    /// layout and stash it in `cached_drop_ctx`.
    ///
    /// One source for two consumers that must never disagree: the drag
    /// hit-test (`handle_mouse_drag_msg` → `render::resolve_tab_drop_zone`)
    /// and the `EditorOp::TabDragOverlay` rung's own highlight. `render_content`
    /// calls this unconditionally once per frame — a drag has to be able to
    /// *start*, so the cache must be current on frames where no drag is live —
    /// and the rung calls it again rather than carrying a second copy of the
    /// computation.
    ///
    /// Origin convention: `gtb.bounds` are always absolute (built from absolute
    /// window rects), so there is no origin offset to apply. `group_tab_bars`
    /// covers tab-bar geometry directly, so both branches and the parameters
    /// they fed are unified.
    pub(crate) fn cache_tab_drop_geometry(
        &self,
        screen: &render::ScreenLayout,
        engine: &Engine,
        tab_bar_h: f64,
    ) {
        let bounds = render::screen_to_drop_group_bounds(screen);
        // Per-tab slot x-positions (absolute) are captured by the `TabBars`
        // rung while drawing. Feeding them here makes a drag inside a group's
        // own tab bar resolve to a `TabReorder` (insertion bar) instead of
        // falling through to a new-split/center overlay.
        let slots_abs = self.cached_tab_slots_abs.borrow();
        let ctx = render::build_tab_drop_ctx(&bounds, engine, tab_bar_h as f32, &slots_abs);
        drop(slots_abs);
        *self.cached_drop_ctx.borrow_mut() = ctx;
    }

    /// Re-resolve a tab-drag press point to `(group, tab index)`, or `None`
    /// when the press was not on a tab after all.
    ///
    /// GTK arms the drag for the whole tab-bar band (its tab geometry is
    /// proportional-font pixel bounds, resolved by `pixel_to_click_target`, not
    /// the exact cell hit TUI gets for free), so the confirmation that
    /// `render::TabDragMove::Crossed` asks for is a real second hit-test here.
    pub(crate) fn tab_drag_source_at(
        &self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
    ) -> Option<(core::window::GroupId, usize)> {
        let layout_ref = self.cached_screen_layout.borrow();
        let layout = layout_ref.as_ref()?;
        let mut engine = self.engine.borrow_mut();
        let drag_rc = backend.drag_state_handle();
        let target = pixel_to_click_target(
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
            true, // resolving the original tab-bar mouse-down; switching tabs is intended
            &mut drag_rc.borrow_mut(),
            false, // re-resolving a tab-bar press; the minimap rung cannot match here
        );
        if !matches!(target, ClickTarget::TabBar) {
            return None;
        }
        // The tab was already switched by `pixel_to_click_target`, so the
        // active group + active tab *is* the drag source.
        let gid = engine.active_group;
        let tidx = engine
            .editor_groups
            .get(&gid)
            .map(|g| g.active_tab)
            .unwrap_or(0);
        Some((gid, tidx))
    }

    /// Resolve the modal-overlay rung for one point/action against
    /// the layouts the last frame actually painted
    /// (`dialog_layout`, `tab_switcher_popup_rect`, `completion_layout`,
    /// `Engine::toast_layout`), never freshly recomputed ones.
    ///
    /// Shared by every mouse-button path that needs to know whether a
    /// modal overlay owns the event — left-click dispatch
    /// (`handle_mouse_click_msg`, `ModalMouseAction::LeftPress`) and
    /// right-click dispatch (the `MouseButton::Right` arm of `handle`,
    /// `ModalMouseAction::Other`) both call this rather than re-deriving
    /// the state. TUI's `handle_mouse` already funnels every mouse event
    /// through one call to `render::route_modal_overlay_click`; this is
    /// GTK's equivalent single call site.
    pub(crate) fn route_modal_overlay(
        &self,
        x: f64,
        y: f64,
        action: render::ModalMouseAction,
    ) -> render::ModalOverlayRoute {
        let engine_ref = self.engine.borrow();
        let toast = engine_ref.toast_layout.borrow().clone();
        let dialog = self.dialog_layout.borrow().clone();
        let completion = self.completion_layout.borrow().clone();
        let context_menu = self.context_menu_layout.borrow().clone();
        let tab_switcher_bounds = self.tab_switcher_popup_rect.get();
        let lh = self.painted_line_height() as f32;
        // Both geometries come from what the last frame PAINTED — the picker's
        // own published rect, and the `FindReplacePanel` the frame was built
        // from — never a re-derivation off the drawing-area size.
        let picker = self.picker_popup_rect.get().map(|rect| {
            render::PickerHitGeometry::new(
                rect,
                lh,
                engine_ref.picker_preview.is_some(),
                &(self.units.picker_rows)(lh),
                &engine_ref,
            )
        });
        let screen_ref = self.cached_screen_layout.borrow();
        let find_replace = screen_ref
            .as_ref()
            .and_then(|s| s.find_replace.as_ref())
            .map(|panel| {
                render::FindReplaceHitGeometry::from_panel(
                    panel,
                    (self.painted_char_width() as f32, lh),
                    &(self.units.find_replace_anchor),
                )
            });

        render::route_modal_overlay_click(
            &render::ModalOverlayState {
                toast: toast.as_ref(),
                dialog_open: engine_ref.dialog.is_some(),
                dialog: dialog.as_ref(),
                context_menu_open: engine_ref.context_menu.is_some(),
                context_menu: context_menu.as_ref(),
                // The GTK rasteriser strokes the menu's border *inside*
                // `ContextMenuLayout::bounds`, so there is no frame outside it.
                context_menu_border: 0.0,
                tab_switcher_open: engine_ref.tab_switcher_open,
                tab_switcher_bounds,
                completion_open: engine_ref.completion_idx.is_some(),
                completion: completion.as_ref(),
                picker_open: engine_ref.picker_open,
                picker,
                find_replace_open: engine_ref.find_replace_open,
                find_replace,
            },
            x as f32,
            y as f32,
            action,
        )
    }

    /// Apply a unified-picker verdict from
    /// [`render::route_modal_overlay_click`].
    ///
    /// The verdict itself — which result row, thumb vs. track, inside vs.
    /// outside — is `render::PickerHitGeometry`'s, shared with TUI's
    /// `handle_mouse`. Both backends now resolve it from the same router,
    /// so their hit-test logic no longer drifts.
    pub(crate) fn apply_picker_route(
        &mut self,
        backend: &dyn quadraui::Backend,
        route: render::PickerRoute,
    ) {
        let picker_id = quadraui::WidgetId::new("picker");
        let Some(rect) = self.picker_popup_rect.get() else {
            return;
        };
        let lh = self.painted_line_height() as f32;
        let geo = {
            let engine = self.engine.borrow();
            render::PickerHitGeometry::new(
                rect,
                lh,
                engine.picker_preview.is_some(),
                &(self.units.picker_rows)(lh),
                &engine,
            )
        };
        // Keep the stack in step: the drag guard in `handle_mouse_drag_msg`
        // consults it to stop a gesture leaking to the editor behind the modal
        backend
            .modal_stack_handle()
            .borrow_mut()
            .push(picker_id, geo.bounds);

        match route {
            render::PickerRoute::Row(idx) => {
                render::apply_picker_row_click(&mut self.engine.borrow_mut(), idx);
            }
            render::PickerRoute::ScrollbarThumb { grab_offset } => {
                backend
                    .drag_state_handle()
                    .borrow_mut()
                    .begin(geo.drag_target(picker_id, grab_offset));
            }
            render::PickerRoute::ScrollbarTrack { toward_end } => {
                render::apply_picker_scroll_offset(
                    &mut self.engine.borrow_mut(),
                    geo.paged_offset(toward_end),
                    geo.visible_rows,
                );
            }
            render::PickerRoute::Consume => {}
            render::PickerRoute::Dismiss => {
                // Clicking away from a `vimcode.picker.open`
                // picker is a user-initiated cancel exactly like Escape —
                // use the same helper so `on_cancel` fires and the plugin's
                // registration is released instead of leaking forever.
                self.engine.borrow_mut().close_picker_cancelling_plugin();
                backend.modal_stack_handle().borrow_mut().pop(&picker_id);
            }
        }
    }

    /// Apply a context-menu verdict from [`render::route_modal_overlay_click`].
    ///
    /// Returns `true` when the event was consumed. The route itself — which
    /// item, hover vs. click, dismiss vs. keep-open — is decided once in
    /// `render.rs` and shared with TUI's `handle_mouse`; what stays here is
    /// GTK's own plumbing (modal-stack bookkeeping, file-tree refresh).
    pub(crate) fn apply_context_menu_route(
        &mut self,
        backend: &dyn quadraui::Backend,
        route: render::ContextMenuRoute,
    ) -> bool {
        let cm_id = quadraui::WidgetId::new("context_menu");
        let pop_stack = || {
            backend.modal_stack_handle().borrow_mut().pop(&cm_id);
        };
        match route {
            render::ContextMenuRoute::Item(idx) => {
                let mut engine = self.engine.borrow_mut();
                if let Some(ref mut cm) = engine.context_menu {
                    cm.selected = idx;
                }
                let _act = engine.context_menu_confirm();
                let needs_tree_refresh = engine.explorer_needs_refresh;
                if needs_tree_refresh {
                    engine.explorer_needs_refresh = false;
                }
                drop(engine);
                pop_stack();
                if needs_tree_refresh {
                    self.refresh_file_tree();
                }
            }
            render::ContextMenuRoute::Hover(idx) => {
                let mut engine = self.engine.borrow_mut();
                if let Some(ref mut cm) = engine.context_menu {
                    if cm.selected == idx {
                        return true;
                    }
                    cm.selected = idx;
                }
            }
            render::ContextMenuRoute::Consume => {}
            render::ContextMenuRoute::Dismiss => {
                self.engine.borrow_mut().close_context_menu();
                pop_stack();
            }
            render::ContextMenuRoute::Fallthrough => return false,
        }
        self.draw_needed.set(true);
        true
    }

    /// Resolve a press against one editor scrollbar axis — thumb-drag vs.
    /// track page-jump — shared between the horizontal and vertical rungs
    /// of `handle_mouse_click_msg`. Returns `true` when the point
    /// landed on a scrollbar and the click was consumed (either scrolled
    /// immediately or armed a drag); the caller should `return` without
    /// falling through to the next rung.
    ///
    /// Window rects always come from [`Self::painted_editor_bounds`] — the
    /// same cached, already-painted geometry the divider rung below reads
    /// via `painted_divider_geometry` — never a `compute_editor_window_rects`
    /// recompute from the drawing area's raw `width`/`height`, which always
    /// assumes the editor area starts at `x = 0`. That only holds with the
    /// activity bar/sidebar at zero width; with either painted, the real
    /// left edge is `AppShellLayout::main_content_bounds.x`.
    pub(crate) fn editor_scrollbar_press(
        &mut self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
        axis: ScrollbarAxis,
    ) -> bool {
        let Some((content_bounds, tab_bar_h)) = self.painted_editor_bounds() else {
            return false;
        };
        let lh = self.cached_line_height;
        let cw = self.cached_char_width;
        // Hit-testing must use the backend's own scrollbar-width override
        // (GTK/macOS/Win's 14px, `None` on TUI) so it agrees with whatever
        // width paint actually drew.
        let v_scrollbar_w = backend.editor_v_scrollbar_width();
        let engine = self.engine.borrow();
        let (rects, _dividers) = engine.calculate_group_window_rects(content_bounds, tab_bar_h);
        let Some((win_id, scroll_at_click)) =
            scrollbar_hit_test(&engine, x, y, &rects, cw, lh, v_scrollbar_w, axis)
        else {
            return false;
        };
        let win_rect = rects.iter().find(|(id, _)| *id == win_id).map(|(_, r)| *r);
        let geom = win_rect.and_then(|rect| {
            scrollbar_thumb_geometry(&engine, win_id, &rect, cw, lh, v_scrollbar_w, axis)
        });
        drop(engine);
        let Some((track_x, track_y, track_w, track_h, thumb_pos, thumb_len, scroll_range, _)) =
            geom
        else {
            return false;
        };
        let max_scroll = scroll_range.round() as usize;
        // `resolve_editor_scrollbar_click` below is the shared click-vs-drag
        // resolver. See that function's doc for the full click/drag rationale.
        let (click_pos, track_visible, track_start, track_length) = match axis {
            ScrollbarAxis::Horizontal => (
                x as f32,
                (track_w / cw).floor() as usize,
                track_x as f32,
                track_w as f32,
            ),
            ScrollbarAxis::Vertical => (
                y as f32,
                (track_h / lh.max(1.0)).floor() as usize,
                track_y as f32,
                track_h as f32,
            ),
        };
        match render::resolve_editor_scrollbar_click(
            click_pos,
            thumb_pos as f32,
            (thumb_pos + thumb_len) as f32,
            track_visible,
            max_scroll,
            scroll_at_click,
        ) {
            render::EditorScrollbarClick::PageTo(new_scroll) => {
                let mut engine = self.engine.borrow_mut();
                match axis {
                    ScrollbarAxis::Horizontal => {
                        engine.set_scroll_left_for_window(win_id, new_scroll);
                    }
                    ScrollbarAxis::Vertical => {
                        engine.set_scroll_top_for_window(win_id, new_scroll);
                        engine.sync_scroll_binds();
                    }
                }
            }
            render::EditorScrollbarClick::BeginDrag { grab_offset } => {
                let drag_rc = backend.drag_state_handle();
                let widget_prefix = match axis {
                    ScrollbarAxis::Horizontal => "editor:h_sb",
                    ScrollbarAxis::Vertical => "editor:v_sb",
                };
                let widget = quadraui::WidgetId::new(format!("{widget_prefix}:{}", win_id.0));
                let target = match axis {
                    ScrollbarAxis::Horizontal => quadraui::DragTarget::ScrollbarX {
                        widget,
                        track_start,
                        track_length,
                        thumb_length: thumb_len as f32,
                        max_scroll,
                        grab_offset,
                        inverted: false,
                    },
                    ScrollbarAxis::Vertical => quadraui::DragTarget::ScrollbarY {
                        widget,
                        track_start,
                        track_length,
                        thumb_length: thumb_len as f32,
                        max_scroll,
                        grab_offset,
                        inverted: false,
                    },
                };
                drag_rc.borrow_mut().begin(target);
            }
        }
        self.draw_needed.set(true);
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn handle_mouse_click_msg(
        &mut self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
        width: f64,
        alt: bool,
    ) {
        if self.engine.borrow().activity_bar_focused {
            self.engine.borrow_mut().activity_bar_focus_out();
        }

        // ── Folder picker mouse handling ────────────────────────────────
        // Checked before every other rung: like a modal dialog, the picker
        // swallows every click while open rather than competing for z-order
        // through `route_modal_overlay_click` / `MOUSE_ARBITRATION_ORDER` —
        // see `render::route_folder_picker_click`'s doc comment. TUI's
        // `mouse::handle_mouse` checks the identical shared helper.
        if self.folder_picker.borrow().is_some() {
            self.route_and_apply_folder_picker_click(x, y);
            return;
        }

        // ── Change-review surface mouse handling ──────────────────────────
        // Same "checked before every other rung, swallows every click"
        // policy as the folder picker above — see
        // `render::route_change_review_click`'s doc comment. TUI's
        // `mouse::handle_mouse` checks the identical shared helper.
        if self.engine.borrow().change_review.is_some() {
            self.route_and_apply_change_review_click(x, y);
            return;
        }

        // ── Settings modal overlay ───────────────────────────────────────
        // When the settings panel is shown as a centred modal, clicks
        // inside the painted form rect route to the FormController; clicks
        // outside dismiss the modal.  Checked before the general modal
        // ladder because the scrim covers the editor band.
        if self.engine.borrow().settings_modal_open {
            let form_rect = self.engine.borrow().settings_form_rect.get();
            let fx = x as f32;
            let fy = y as f32;
            let inside = fx >= form_rect.x
                && fx < form_rect.x + form_rect.width
                && fy >= form_rect.y
                && fy < form_rect.y + form_rect.height;
            if inside {
                let event = quadraui::UiEvent::DoubleClick {
                    widget: None,
                    position: quadraui::Point::new(fx, fy),
                };
                render::handle_settings_form_ui_event(
                    &mut self.engine.borrow_mut(),
                    &event,
                    form_rect,
                );
            } else {
                self.engine.borrow_mut().collapse_sidebar();
            }
            self.draw_needed.set(true);
            return;
        }

        self.reconcile_editor_hover_modal(backend);

        // ── Modal-overlay rung ────────────────────────────────────────────
        //
        // Toast → dialog → tab switcher → completion, sequenced ONCE in
        // `render::route_modal_overlay_click` and shared verbatim with
        // TUI's `handle_mouse`.
        let modal_route = self.route_modal_overlay(x, y, render::ModalMouseAction::LeftPress);
        match modal_route {
            render::ModalOverlayRoute::Toast(hit) => {
                if self.engine.borrow_mut().handle_toast_hit(hit) {
                    self.draw_needed.set(true);
                    return;
                }
            }
            render::ModalOverlayRoute::Dialog(hit) => {
                match hit {
                    quadraui::DialogHit::Button(id) => {
                        if let Some(idx) = dialog_btn_index(&id) {
                            let action = self.engine.borrow_mut().dialog_click_button(idx);
                            self.apply_dialog_action(action);
                        }
                    }
                    quadraui::DialogHit::Outside => {
                        let mut engine = self.engine.borrow_mut();
                        engine.dialog = None;
                        engine.pending_move = None;
                    }
                    quadraui::DialogHit::Body | quadraui::DialogHit::BodyToolbarButton(_) => {}
                }
                self.draw_needed.set(true);
                return;
            }
            render::ModalOverlayRoute::ContextMenu(route) => {
                if self.apply_context_menu_route(backend, route) {
                    return;
                }
            }
            render::ModalOverlayRoute::TabSwitcher { inside } => {
                // Click anywhere dismisses; inside also consumes so the
                // editor underneath doesn't take a cursor move through it.
                self.engine.borrow_mut().tab_switcher_open = false;
                self.draw_needed.set(true);
                if inside {
                    return;
                }
            }
            render::ModalOverlayRoute::Completion(hit) => {
                let consumed = self.engine.borrow_mut().handle_completion_click(hit);
                self.draw_needed.set(true);
                if consumed {
                    return;
                }
            }
            render::ModalOverlayRoute::UnifiedPicker(hit) => {
                self.apply_picker_route(backend, hit);
                self.draw_needed.set(true);
                return;
            }
            render::ModalOverlayRoute::FindReplace(hit) => {
                if let render::FindReplaceRoute::Target { target, is_input } = hit {
                    if is_input {
                        self.fr_input_dragging = true;
                    }
                    self.engine.borrow_mut().handle_find_replace_click(target);
                }
                self.draw_needed.set(true);
                return;
            }
            render::ModalOverlayRoute::Swallow => return,
            render::ModalOverlayRoute::None => {}
        }

        // ── Editor hover popup rung ───────────────────────────────────────
        //
        // Link click, scrollbar grab, focus-or-select and dismiss-on-outside,
        // sequenced ONCE in `render::route_editor_hover_popup_click` and
        // shared verbatim with TUI's `handle_mouse`. The ~100 lines this
        // replaced sat *below* the scroll-surface dispatch, so a press aimed
        // at the popup's own scrollbar was consumed by the surface painted
        // behind it. It runs above that dispatch now — where TUI
        // always had it — because the popup paints on top of the editor.
        if self.route_and_apply_editor_hover_popup(backend, x, y) {
            return;
        }

        // ── Panel-hover popup link click ──────────────────────────────────
        //
        // Shared with TUI's `mouse::handle_mouse` via `render::
        // route_panel_hover_popup_click` + `render::
        // apply_panel_hover_popup_route` — see `route_and_apply_panel_hover_
        // popup`'s doc for why this backend never had it before. Checked
        // above the scroll-surface dispatch for the same reason the editor
        // hover popup is: the popup paints on top of whatever is under it.
        if self.route_and_apply_panel_hover_popup(x, y) {
            return;
        }

        // ── Scroll-surface click dispatch (scrollbar thumb-drag + track-page). ──
        {
            let surfaces = self.engine.borrow().scroll_surfaces.borrow().clone();
            let modal = backend.modal_stack_handle().borrow().clone();
            let mut drag = backend.drag_state_handle().borrow().clone();
            let click_events = quadraui::dispatch_click(
                &modal,
                &surfaces,
                &[],
                &mut drag,
                quadraui::Point {
                    x: x as f32,
                    y: y as f32,
                },
                quadraui::MouseButton::Left,
                Default::default(),
            );
            *backend.drag_state_handle().borrow_mut() = drag;
            for cev in &click_events {
                match cev {
                    quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset } => {
                        // Shared with TUI's click table and the drag
                        // path — `render::apply_scroll_offset` is the
                        // union of every id either backend emits here. GTK
                        // only ever registers `debug_output`/
                        // `terminal_scrollback` into `scroll_surfaces`
                        // (`explorer:sb`/`ext_panel:sb` are TUI-only), so
                        // this is a like-for-like replacement of the two
                        // hand-rolled arms above, not a behavior change.
                        if render::apply_scroll_offset(
                            &mut self.engine.borrow_mut(),
                            widget.as_str(),
                            *new_offset,
                            render::ScrollApplyContext {
                                picker_visible_rows: 0,
                            },
                        ) {
                            self.draw_needed.set(true);
                            return;
                        }
                    }
                    quadraui::UiEvent::MouseDown {
                        widget: Some(id), ..
                    } if id.as_str() == "debug_output" => {
                        return;
                    }
                    _ => {}
                }
            }
        }

        // The context-menu, find/replace and unified-picker rungs are now
        // decided by `render::route_modal_overlay_click` at the top of this
        // handler and applied by `apply_context_menu_route` / `apply_picker_route`.
        // The shared router sequences them consistently across backends.

        // ── Chrome rung ───────────────────────────────────────────────────
        //
        // Breadcrumbs → status bands → global status bar, sequenced ONCE in
        // `render::route_chrome_click` and shared verbatim with TUI's
        // `handle_mouse`.
        if self.route_and_apply_chrome_click(x, y, render::ChromeMouseAction::LeftPress) {
            return;
        }

        // ── Command line click — start text selection ────────────────────
        // `render::command_line_click_char_idx` hit-tests
        // `engine.command_line_rect` (cached at paint time, just above) the
        // same way TUI's press rung does, so this is thin wiring, not a new
        // GTK-specific selection implementation.
        if render::command_line_selection_allowed(&self.engine.borrow()) {
            let data = render::build_command_line(&self.engine.borrow());
            // Use the real painted char width, not a stale default: this frame's
            // `painted_char_width()` matches what the command line was actually
            // drawn with.
            let char_width = self.painted_char_width() as f32;
            let rect = self.engine.borrow().command_line_rect.get();
            let point = quadraui::Point::new(x as f32, y as f32);
            if let Some(char_idx) =
                render::command_line_click_char_idx(rect, &data.text, char_width, point)
            {
                let mut engine = self.engine.borrow_mut();
                if matches!(
                    engine.mode,
                    crate::core::Mode::Command | crate::core::Mode::Search
                ) {
                    let buf_len = engine.command_buffer.chars().count();
                    engine.command_cursor = char_idx.saturating_sub(1).min(buf_len);
                }
                engine.cmd_sel.set(Some((char_idx, char_idx)));
                engine.cmd_dragging.set(true);
                drop(engine);
                self.draw_needed.set(true);
                return;
            }
        }

        // Debug toolbar click: resolve via cached ToolbarLayout on engine.
        {
            let dbg_y = self.debug_toolbar_y_offset.get();
            let dbg_h = self.debug_toolbar_height.get();
            if dbg_h > 0.0 && y >= dbg_y && y < dbg_y + dbg_h {
                let idx = self.engine.borrow().debug_button_hit(x as f32, y as f32);
                self.engine.borrow_mut().debug_button_pressed = idx;
                self.draw_needed.set(true);
                if let Some(i) = idx {
                    if let Some(btn) = render::DEBUG_BUTTONS.get(i) {
                        let _ = self.engine.borrow_mut().execute_command(btn.action);
                        return;
                    }
                }
                return;
            }
        }

        // The dialog rung moved to the shared modal-overlay router
        // at the top of this handler (`render::route_modal_overlay_click`),
        // which TUI's `handle_mouse` calls too. Control only reaches here
        // when no dialog is open.
        {
            // The git-branch hit test is now the global-status-bar rung of
            // `render::route_chrome_click`, called at the top of this handler.
            //
            // Clicking in the editor clears every sidebar's keyboard focus.
            // Without this, focus stays on whichever sidebar grabbed it last
            // and the editor key handler keeps routing keys to that sidebar.
            self.engine.borrow_mut().clear_sidebar_focus();
            // ── Bottom panel (tab strip / toolbar / terminal content) ──────────
            // Zone, split hit-test and pane-cell translation are all
            // `render::route_bottom_panel_click`, shared verbatim with TUI's
            // `handle_mouse`. `panel_left` is now a required input to ensure the
            // hit-test uses the same offset the paint used.
            let route = render::route_bottom_panel_click(
                &self.engine.borrow(),
                x,
                y,
                render::BottomPanelMetrics {
                    panel_left: self.painted_bottom_panel_left(),
                    col_width: self.cached_char_width.max(1.0),
                },
            );
            if let Some(route) = route {
                if !matches!(route, render::BottomPanelRoute::TabBar) {
                    self.terminal_resize_dragging = false;
                }
                let ctx = crate::core::engine::UiEventContext {
                    // Was `self.terminal_cols()` (pinned at 80) — this
                    // handler already has the real live panel width, so use
                    // it. This is what feeds `ToggleSplit`'s initial
                    // full_cols when opening a split.
                    terminal_cols: self.terminal_panel_cols(width),
                    terminal_max_rows: self.terminal_maximize_target_rows(&self.engine.borrow()),
                };
                let effect =
                    render::apply_bottom_panel_route(&mut self.engine.borrow_mut(), route, x, ctx);
                self.terminal_split_dragging |= effect.split_drag;
                self.terminal_resize_dragging |= effect.resize_drag;
                if effect.relayout {
                    self.handle_resize();
                    return;
                }
                self.draw_needed.set(true);
            } else {
                {
                    let mut engine = self.engine.borrow_mut();
                    // Clicking outside the terminal panel returns focus to the editor.
                    engine.terminal_has_focus = false;
                }

                // Dropdown clicks are fully handled by the menu_dropdown_da overlay
                // widget (which has can_target=true while a menu is open).
                // If we reach here, no menu is open and we proceed with normal handling.

                // ── H/V scrollbar hit-test (before divider) ──────────────────────
                // If the click lands on either editor scrollbar:
                //   - on the thumb → start a DragTarget::ScrollbarX/Y drag.
                //   - on the empty track → page-jump toward the click.
                // Either way, consume the click before the divider hit-test below
                // gets a look. Shared axis-parameterised `App::editor_scrollbar_press`
                // prevents per-axis divergence.
                if self.editor_scrollbar_press(backend, x, y, ScrollbarAxis::Horizontal) {
                    return;
                }
                if self.editor_scrollbar_press(backend, x, y, ScrollbarAxis::Vertical) {
                    return;
                }

                // ── Divider hit-test ──────────────────────────────────────────────
                // Editor-group boundaries then `:split`/`:vsplit` boundaries
                // sequenced by `render::route_divider_grab`. This backend's only
                // contribution is its own painted geometry and grab margin —
                // `self.units.divider_metrics`, a symmetric 6px around the thin
                // drawn line on GTK (`quantize: false`) vs TUI's cell metrics.
                if let Some((group_dividers, window_dividers, on_tab_bar)) =
                    self.painted_divider_geometry(x, y)
                {
                    let breadcrumbs = self.engine.borrow().settings.breadcrumbs;
                    if let Some(grab) = render::route_divider_grab(
                        &render::DividerState {
                            group_dividers: &group_dividers,
                            window_dividers: &window_dividers,
                            metrics: (self.units.divider_metrics)(breadcrumbs),
                            on_tab_bar,
                        },
                        x,
                        y,
                    ) {
                        self.divider_grab = Some(grab);
                        return;
                    }
                }

                // ── Editor-tab-hosted plugin view click ────────────────────
                //
                // A `vimcode.ui.register_view` view opened as an editor-area
                // tab (`Engine::open_plugin_view_tab`) paints either a
                // `Form` or a body-kind primitive (`List`/`Tree`/`Table`/
                // `TextView`), not buffer text (`App::paint_editor_windows_
                // rung`) — a click inside its window rect has no
                // buffer-click meaning for `click::handle_mouse_click` to
                // resolve (no cursor to place, no file to reveal), so it's
                // routed through `render::route_plugin_view_body_event`
                // first (the same body-kind router the sidebar's `ExtPanel`
                // arm uses) and, when that reports the view is a
                // field-stack `Form` instead (`None`), through
                // `render::handle_plugin_view_tab_ui_event` — before falling
                // into the generic buffer-click block below.
                // `Self::plugin_view_tab_hit` walks the same `screen.windows`
                // rects `click.rs` itself resolves clicks against, so the
                // two can't disagree about which window a click landed in.
                if let Some((name, rect)) = self.plugin_view_tab_hit(x, y) {
                    let mut engine = self.engine.borrow_mut();
                    engine.clear_sidebar_focus();
                    let event = quadraui::UiEvent::MouseDown {
                        widget: None,
                        button: quadraui::MouseButton::Left,
                        position: quadraui::Point::new(x as f32, y as f32),
                        modifiers: quadraui::Modifiers::default(),
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

                {
                    let mut engine = self.engine.borrow_mut();

                    if engine.is_vscode_mode() {
                        engine.vscode_clear_selection();
                    }
                    let (click_result, engine_action) = {
                        let layout_ref = self.cached_screen_layout.borrow();
                        if let Some(ref layout) = *layout_ref {
                            let drag_rc = backend.drag_state_handle();
                            let mut drag = drag_rc.borrow_mut();
                            handle_mouse_click(
                                &mut engine,
                                backend,
                                x,
                                y,
                                alt,
                                self.cached_line_height,
                                self.cached_char_width,
                                layout,
                                &self.cached_group_tab_bar_layouts.borrow(),
                                self.cached_frame_hit_map.borrow().as_ref(),
                                &self.cached_tab_bar_zones.borrow(),
                                &mut drag,
                            )
                        } else {
                            (None, None)
                        }
                    };
                    match engine_action {
                        Some(core::engine::EngineAction::ToggleSidebar) => {
                            drop(engine);
                            self.sync_sidebar_from_engine();
                            return;
                        }
                        Some(core::engine::EngineAction::OpenTerminal) => {
                            // Create the terminal tab immediately (not via
                            // the deferred `DeferredAction::ToggleTerminal`)
                            // so the panel appears on this same draw cycle.
                            // `width` is this handler's own live panel
                            // width — prefer it over the cached one.
                            let cols = self.terminal_panel_cols(width);
                            let rows = engine.session.terminal_panel_rows;
                            engine.terminal_new_tab(cols, rows);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                        _ => {}
                    }
                    match click_result {
                        Some(true) => {
                            drop(engine);
                            self.show_close_tab_confirm();
                            self.draw_needed.set(true);
                            return;
                        }
                        Some(false) => {
                            // Buffer click — fire hooks and reveal file
                        }
                        None => {
                            // Engine-drawn action menu is already opened with the
                            // correct anchor by click.rs::handle_mouse_click. The
                            // engine-drawn renderer at draw.rs:906 + click dispatch
                            // at line ~6022 take over from here.
                            if engine.context_menu.as_ref().is_some_and(|cm| {
                                matches!(
                                    cm.target,
                                    core::engine::ContextMenuTarget::EditorActionMenu { .. }
                                )
                            }) {
                                drop(engine);
                                self.draw_needed.set(true);
                                return;
                            }
                            // Tab bar / split button click — skip hooks.
                            // Record drag start position for tab drag-and-drop.
                            self.tab_drag.arm(x, y);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                    }

                    // Fire cursor_move hook so plugins (e.g. git-insights blame)
                    // see the new cursor position after a mouse click.
                    engine.fire_cursor_move_hook();
                    drop(engine);
                    self.draw_needed.set(true);
                }
            }
        } // close else (dialog not open)
    }

    /// Line height the last frame actually painted with, falling back to the
    /// `setup()`-seeded `cached_line_height` before the first frame.
    ///
    /// Every click hit-test that measures *painted* geometry must use this
    /// rather than `cached_line_height` — see the note where
    /// `render_content` publishes it.
    pub(crate) fn painted_line_height(&self) -> f64 {
        self.painted_line_height
            .get()
            .unwrap_or(self.cached_line_height)
            .max(1.0)
    }

    /// Assemble this backend's [`render::ChromeState`] from the geometry the
    /// last frame actually painted, run the shared chrome rung over it, and
    /// apply whatever it decides. Returns `true` when the event was consumed.
    ///
    /// Every rect fed in here is a *painted* one — `status_segment_map` and
    /// `separated_status_bar_rect` are filled by `render_content` from the
    /// same `Surface::StatusBar` rects it draws, `global_status_rect` likewise,
    /// and the breadcrumb bars carry their own draw-time layout. Never hit-test
    /// against freshly recomputed geometry.
    ///
    /// The `StatusBand` assembly — separated line, then each window's own
    /// line, then the global bar — is [`render::status_bands`], shared with
    /// TUI's `mouse::route_and_apply_chrome_click`.
    pub(crate) fn route_and_apply_chrome_click(
        &mut self,
        x: f64,
        y: f64,
        action: render::ChromeMouseAction,
    ) -> bool {
        let lh = self.painted_line_height();

        let layout_ref = self.cached_screen_layout.borrow();
        let Some(ref screen) = *layout_ref else {
            return false;
        };
        let engine = self.engine.borrow();
        let segment_map = self.status_segment_map.borrow();
        let global_status_zones = self.global_status_zones.borrow();

        // The separated/per-window/global assembly (in that
        // arbitration order — see `render::status_bands`'s own doc comment
        // for why) is now the one shared builder both backends call, rather
        // than this loop and TUI's near-identical twin in
        // `mouse::route_and_apply_chrome_click`.
        let global_rect = engine.global_status_rect.get();
        let bands = render::status_bands(
            &screen.windows,
            lh,
            &segment_map,
            self.separated_status_bar_rect
                .get()
                .map(|rect| (rect, screen.active_window_id)),
            (global_rect.width > 0.0 && global_rect.height > 0.0)
                .then(|| (global_rect, &*global_status_zones)),
        );

        // The same shared hit test, with the same tolerances, the window-split
        // divider rung in `handle_mouse_click_msg` runs — see
        // `render::ChromeState::on_window_divider`.
        let on_window_divider =
            self.painted_editor_bounds()
                .is_some_and(|(content_bounds, tab_bar_h)| {
                    let (window_rects, _) =
                        engine.calculate_group_window_rects(content_bounds, tab_bar_h);
                    render::divider_hit_test(
                        &engine.calculate_window_dividers(&window_rects),
                        x,
                        y,
                        self.units.hit_tolerance,
                        self.units.hit_tolerance,
                        (self.units.divider_metrics)(engine.settings.breadcrumbs).quantize,
                    )
                    .is_some()
                });

        let route = render::route_chrome_click(
            &render::ChromeState {
                breadcrumbs_enabled: engine.settings.breadcrumbs,
                breadcrumbs: &screen.breadcrumbs,
                line_height: lh,
                status_bands: &bands,
                on_window_divider,
            },
            action,
            x,
            y,
        );

        drop(segment_map);
        drop(global_status_zones);
        drop(engine);
        drop(layout_ref);

        match route {
            render::ChromeRoute::None => return false,
            render::ChromeRoute::Breadcrumb { group_id, idx } => {
                self.engine
                    .borrow_mut()
                    .handle_breadcrumb_click(group_id, idx);
            }
            render::ChromeRoute::StatusAction(action) => {
                let cols = self.terminal_panel_cols(self.painted_editor_content_width());
                let follow_up =
                    render::apply_status_action(&mut self.engine.borrow_mut(), &action, cols);
                if matches!(
                    follow_up,
                    Some(crate::core::engine::EngineAction::ToggleSidebar)
                ) {
                    self.sync_sidebar_from_engine();
                }
            }
            render::ChromeRoute::BreadcrumbBar | render::ChromeRoute::StatusBar => {}
        }
        self.draw_needed.set(true);
        true
    }

    /// Character-cell advance the last frame actually painted with — the
    /// horizontal twin of [`Self::painted_line_height`]. See the field's doc
    /// for why `cached_char_width` is the wrong number at click time.
    pub(crate) fn painted_char_width(&self) -> f64 {
        self.painted_char_width
            .get()
            .unwrap_or(self.cached_char_width)
            .max(1.0)
    }

    /// Compute the picker popup's bounds in DA-local pixels. Shared by
    /// the click handler (to push into the modal stack) and the drag
    /// guard (to decide if a drag started inside the popup).
    ///
    /// Prefers the rect the last frame **actually painted**
    /// (`picker_popup_rect`), and only re-derives from `viewport` when no
    /// frame has painted the picker yet. `viewport` must be the same
    /// editor-pane rect `render_content` paints the popup into, so the hit
    /// rect pushed onto the `ModalStack` lands exactly where the pixels are.
    pub(crate) fn compute_picker_popup_bounds(&self, viewport: quadraui::Rect) -> quadraui::Rect {
        if let Some(rect) = self.picker_popup_rect.get() {
            return rect;
        }
        let engine = self.engine.borrow();
        let has_preview = engine.picker_preview.is_some();
        let item_count = engine.picker_items.len();
        drop(engine);
        let sizing = render::PickerSizing {
            header_h: 0.0,
            line_h: 1.0,
            ..(self.units.picker)(1.0)
        };
        let geo = render::PickerGeometry::compute(viewport, item_count, has_preview, &sizing);
        quadraui::Rect::new(geo.popup_x, geo.popup_y, geo.popup_w, geo.popup_h)
    }

    /// Continue or end an editor-tab-hosted plugin table's header-divider
    /// drag, called before the generic dispatch match (mirrors
    /// `try_route_sidebar_mouse_event`'s early-intercept position in
    /// `dispatch.rs`) so `MouseMoved`/`MouseUp` reach the drag even once the
    /// pointer has strayed outside the view's window rect.
    ///
    /// Unlike the sidebar's `sidebar_pointer_captured` flag (which gates many
    /// different interactions, not just table resize), a Tab-hosted drag
    /// needs no separate capture flag: `Engine::plugin_view_table_resize_drag`
    /// being armed for `PluginViewHost::Tab` *is* the capture state, and
    /// `continue_plugin_view_table_resize_drag`/`end_plugin_view_table_resize_drag`
    /// already check the drag's own `host`/`view` before acting on it.
    ///
    /// Returns `false` (so the caller falls through to its usual handling)
    /// when there is no active Tab-hosted drag, or the event isn't a
    /// `MouseMoved`/`MouseUp`.
    pub(crate) fn try_route_tab_plugin_table_drag(
        &mut self,
        backend: &mut dyn quadraui::Backend,
        event: &quadraui::UiEvent,
    ) -> bool {
        use quadraui::UiEvent;
        if !matches!(event, UiEvent::MouseMoved { .. } | UiEvent::MouseUp { .. }) {
            return false;
        }
        let mut engine = self.engine.borrow_mut();
        let Some(drag) = engine.plugin_view_table_resize_drag.as_ref() else {
            return false;
        };
        if drag.host != PluginViewHost::Tab {
            return false;
        }
        let name = drag.view.clone();
        let Some(rect) = self.plugin_view_tab_window_rect(&name) else {
            return false;
        };
        let consumed = render::route_plugin_view_body_event(
            &mut engine,
            &name,
            PluginViewHost::Tab,
            event,
            rect,
            backend,
        )
        .unwrap_or(false);
        drop(engine);
        if consumed {
            self.draw_needed.set(true);
        }
        consumed
    }

    // ── Drag-follow-through rung ──────────────────────────────────────
    //
    // Which gesture owns a move-with-the-button-held is
    // `render::route_mouse_drag`, sequenced ONCE and shared verbatim with TUI's
    // `handle_mouse`. See the rung's banner in `render.rs`.
    pub(crate) fn handle_mouse_drag_msg(
        &mut self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
        viewport: quadraui::Rect,
    ) {
        // Unrelated drag arms below (terminal split/panel resize) clamp
        // against the pane's plain width/height, not its origin.
        let width = viewport.width as f64;
        let height = viewport.height as f64;
        // Keep the picker's modal-stack entry fresh before anything hit-tests
        // the stack: the popup's size depends on `has_preview`, which can change
        // mid-picker.
        let picker_open = self.engine.borrow().picker_open;
        {
            let picker_id = quadraui::WidgetId::new("picker");
            let stack_rc = backend.modal_stack_handle();
            let mut stack = stack_rc.borrow_mut();
            if picker_open {
                let rect = self.compute_picker_popup_bounds(viewport);
                stack.push(picker_id, rect);
            } else {
                stack.pop(&picker_id);
            }
        }

        let bottom_metrics = render::BottomPanelMetrics {
            panel_left: self.painted_bottom_panel_left(),
            col_width: self.cached_char_width.max(1.0),
        };
        let drag_rc = backend.drag_state_handle();
        let stack_rc = backend.modal_stack_handle();
        let route = {
            let engine = self.engine.borrow();
            let layout_ref = self.cached_screen_layout.borrow();
            let state = render::MouseDragState {
                layout: layout_ref.as_ref(),
                armed_target: render::drag_state_arms_scrollbar(&drag_rc.borrow()),
                hover_popup_selecting: engine.editor_hover_has_focus
                    && engine
                        .editor_hover
                        .as_ref()
                        .is_some_and(|h| h.selection.is_some())
                    && self.editor_hover_popup_rect.get().is_some(),
                modal_hit: stack_rc
                    .borrow()
                    .hit_test(quadraui::Point {
                        x: x as f32,
                        y: y as f32,
                    })
                    .is_some(),
                // Explorer drag-and-drop and command-line selection are shared now.
                // `engine.cmd_dragging` is armed by `handle_mouse_click_msg`'s press
                // rung the same way TUI's `mouse::handle_mouse` does, and
                // `explorer_drag_src` tracks an active explorer DnD gesture.
                sidebar_resizing: false,
                explorer_dnd_active: self.explorer_drag_src.is_some()
                    || self.explorer_drag_active.is_some(),
                sidebar_body: None,
                command_line_selecting: engine.cmd_dragging.get(),
                tab_dragging: self.tab_drag.is_armed_or_dragging(),
                divider_grabbed: self.divider_grab.is_some(),
                terminal_split_dragging: self.terminal_split_dragging,
                terminal_panel_resizing: self.terminal_resize_dragging,
                // Mirrors TUI's guard — see the field's doc
                // comment in `render.rs`. GTK's `EditorText` arm doesn't run
                // through the shared `DragState`, but it drives the same
                // `Engine::mouse_drag`, so `mouse_drag_active` is just as
                // valid a "already extending" signal here.
                text_selection_active: engine.mouse_drag_active,
                in_terminal_content: render::in_terminal_pane_content(
                    &engine,
                    x,
                    y,
                    bottom_metrics,
                ),
                cell: (
                    self.cached_char_width.max(1.0),
                    self.cached_line_height.max(1.0),
                ),
            };
            render::route_mouse_drag(&state, x, y)
        };

        match route {
            render::MouseDragRoute::ArmedTarget => {
                let events = quadraui::dispatch_mouse_drag(
                    &drag_rc.borrow(),
                    quadraui::Point {
                        x: x as f32,
                        y: y as f32,
                    },
                    Default::default(),
                );
                let picker_visible_rows = if picker_open {
                    let lh = self.cached_line_height.max(1.0);
                    let engine = self.engine.borrow();
                    let has_preview = engine.picker_preview.is_some();
                    let item_count = engine.picker_items.len();
                    drop(engine);
                    render::PickerGeometry::compute(
                        viewport,
                        item_count,
                        has_preview,
                        &(self.units.picker)(lh as f32),
                    )
                    .visible_rows
                } else {
                    0
                };
                for ev in &events {
                    if let quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset } = ev {
                        render::apply_scroll_offset(
                            &mut self.engine.borrow_mut(),
                            widget.as_str(),
                            *new_offset,
                            render::ScrollApplyContext {
                                picker_visible_rows,
                            },
                        );
                    }
                }
            }
            render::MouseDragRoute::HoverPopupSelection => {
                if let Some(quadraui::Rect { x: px, y: py, .. }) =
                    self.editor_hover_popup_rect.get()
                {
                    let px = px as f64;
                    let py = py as f64;
                    // Same `units.hover_popup_pad` the press rung
                    // (`route_and_apply_editor_hover_popup`) now reads —
                    // was hardcoded `4.0`/`4.0` here too, which is why a
                    // TUI-native `App` (`tui` harness arm) picked a
                    // different content cell mid-drag than the press had
                    // already landed on.
                    let (pad_x, pad_y) = self.units.hover_popup_pad;
                    let lh = self.cached_line_height.max(1.0);
                    let scroll = self
                        .engine
                        .borrow()
                        .editor_hover
                        .as_ref()
                        .map(|h| h.scroll_top)
                        .unwrap_or(0);
                    let rel_x = x - px - pad_x as f64;
                    let rel_y = y - py - pad_y as f64;
                    let content_line = (rel_y / lh).max(0.0) as usize + scroll;
                    let content_col = self.pixel_to_editor_hover_col(rel_x, content_line);
                    self.engine
                        .borrow_mut()
                        .editor_hover_extend_selection(content_line, content_col);
                }
            }
            render::MouseDragRoute::TabDrag => {
                // `64.0` is the squared 8-device-pixel threshold.
                match self.tab_drag.handle_move(x, y, 64.0) {
                    render::TabDragMove::Tracking => {
                        // Cursor and the cached per-group bounds are both in
                        // absolute surface coordinates, so the hit-test matches
                        // what the overlay draws.
                        if let Some(source) = self.tab_drag.source() {
                            let ctx = self.cached_drop_ctx.borrow();
                            let zone =
                                render::resolve_tab_drop_zone(&ctx, source, x as f32, y as f32);
                            drop(ctx);
                            self.tab_drag.track(zone);
                        }
                    }
                    render::TabDragMove::Crossed { press_x, press_y } => {
                        // Unlike TUI, this backend's arm fires for the whole
                        // tab-bar band, so the press has to be re-resolved to
                        // confirm it was on a tab. If it was not, disarm and
                        // re-route the same event with the machine idle — the
                        // one rung that can decline after being asked.
                        if let Some(source) = self.tab_drag_source_at(backend, press_x, press_y) {
                            self.tab_drag.begin(source, x, y);
                        } else {
                            self.tab_drag.disarm();
                            self.draw_needed.set(true);
                            self.handle_mouse_drag_msg(backend, x, y, viewport);
                            return;
                        }
                    }
                    render::TabDragMove::Pending | render::TabDragMove::Idle => {}
                }
            }
            render::MouseDragRoute::Divider => {
                if let (Some(grab), Some((group_dividers, window_dividers, _))) =
                    (self.divider_grab, self.painted_divider_geometry(x, y))
                {
                    render::apply_divider_drag(
                        &mut self.engine.borrow_mut(),
                        grab,
                        &group_dividers,
                        &window_dividers,
                        x,
                        y,
                    );
                }
            }
            render::MouseDragRoute::TerminalSplitDivider => {
                if self.cached_char_width > 0.0 {
                    let min_x = self.cached_char_width * 5.0;
                    let gutter = self.units.terminal_scrollbar_gutter;
                    let max_x = (width - gutter - self.cached_char_width * 5.0).max(min_x);
                    let clamped_x = x.clamp(min_x, max_x);
                    let left_cols = (clamped_x / self.cached_char_width) as u16;
                    self.engine
                        .borrow_mut()
                        .terminal_split_set_drag_cols(left_cols);
                }
            }
            render::MouseDragRoute::TerminalPanelResize => {
                if self.cached_line_height > 0.0 {
                    let global_status_rows =
                        if render::global_status_bar_visible(&self.engine.borrow()) {
                            1.0
                        } else {
                            0.0
                        };
                    let status_h = (1.0 + global_status_rows) * self.cached_line_height;
                    let available = (height - y - status_h).max(0.0);
                    // Leave at least 4 editor lines visible (+ tab bar chrome)
                    let min_editor_lines = 4.0 + 1.0;
                    let max_rows =
                        ((height - status_h - min_editor_lines * self.cached_line_height)
                            / self.cached_line_height) as u16;
                    let max_rows = max_rows.saturating_sub(2).max(5);
                    let new_rows = ((available / self.cached_line_height) as u16)
                        .saturating_sub(2)
                        .clamp(5, max_rows);
                    self.engine.borrow_mut().session.terminal_panel_rows = new_rows;
                }
            }
            render::MouseDragRoute::Minimap => {
                // A real minimap drag now arms a `DragTarget::ScrollbarY`
                // on press (`click::pixel_to_click_target`'s minimap rung), so a
                // following move routes to `MouseDragRoute::ArmedTarget` above,
                // never here — re-running `apply_minimap_click` (an absolute
                // seek against the strip's own scroll-following window) on
                // every move was the crawl bug this issue fixes. See
                // `MouseDragRoute::Minimap`'s doc comment for when this arm can
                // still be reached at all.
            }
            render::MouseDragRoute::TerminalContent => {
                // Shared drag handler — tries forward_mouse(Move) when the
                // child has mouse reporting, falls back to local selection.
                render::apply_terminal_content_drag(
                    &mut self.engine.borrow_mut(),
                    x,
                    y,
                    bottom_metrics,
                );
            }
            render::MouseDragRoute::EditorText => {
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(ref layout) = *layout_ref {
                    let mut engine = self.engine.borrow_mut();
                    let drag_rc = backend.drag_state_handle();
                    handle_mouse_drag(
                        &mut engine,
                        backend,
                        x,
                        y,
                        // Use the real painted metrics: `painted_line_height()` and
                        // `painted_char_width()` are fresh every `render_content` frame,
                        // whereas the cached versions may be stale before the first tick.
                        self.painted_line_height(),
                        self.painted_char_width(),
                        layout,
                        &self.cached_group_tab_bar_layouts.borrow(),
                        self.cached_frame_hit_map.borrow().as_ref(),
                        &self.cached_tab_bar_zones.borrow(),
                        &mut drag_rc.borrow_mut(),
                    );
                }
            }
            render::MouseDragRoute::CommandLine => {
                // Same `command_line_click_char_idx` helper the press
                // rung uses — extends `cmd_sel`'s head via
                // `CommandLineLayout::hit_test`, mirroring TUI's identical
                // drag arm in `mouse::handle_mouse`.
                if let Some(mut sel) = self.engine.borrow().cmd_sel.get() {
                    let data = render::build_command_line(&self.engine.borrow());
                    // Same fix as the press rung above — use the real
                    // painted char width, not the click backend's never-seeded
                    // `current_char_width` default.
                    let char_width = self.painted_char_width() as f32;
                    let rect = self.engine.borrow().command_line_rect.get();
                    let point = quadraui::Point::new(x as f32, y as f32);
                    if let Some(char_idx) =
                        render::command_line_click_char_idx(rect, &data.text, char_width, point)
                    {
                        sel.1 = char_idx;
                        self.engine.borrow().cmd_sel.set(Some(sel));
                    }
                }
            }
            render::MouseDragRoute::ExplorerDnd => {
                // Shared with TUI's `mouse::handle_mouse` — the row under the pointer
                // is resolved against `explorer_tree_rect` and the row's own pixel/cell
                // pitch applied to get the correct row in the tree.
                let rect = self.engine.borrow().explorer_tree_rect.get();
                let row_height = (self.units.explorer_row_h)(self.cached_explorer_metrics.get().0);
                render::apply_explorer_drag_move(
                    &self.engine.borrow(),
                    rect,
                    row_height,
                    x,
                    y,
                    &mut self.explorer_drag_src,
                    &mut self.explorer_drag_active,
                );
            }
            // A drag inside an open modal with nothing armed is swallowed
            // so it cannot leak to the editor underneath.
            render::MouseDragRoute::ModalSwallow
            | render::MouseDragRoute::SidebarResize
            | render::MouseDragRoute::SidebarBody
            | render::MouseDragRoute::None => {}
        }
        self.draw_needed.set(true);
    }

    /// `width` is the live terminal-panel pixel width — the same
    /// `ctx.layout.main_content_bounds.width` `UiEvent::MouseMoved` already
    /// threads into `handle_mouse_drag_msg` — needed to finalize a
    /// terminal-split divider drag with the real pixel→cell conversion
    /// instead of a fixed guess.
    pub(crate) fn handle_mouse_up_msg(&mut self, backend: &dyn quadraui::Backend, width: f64) {
        // Clear debug toolbar pressed state.
        if self.engine.borrow().debug_button_pressed.is_some() {
            self.engine.borrow_mut().debug_button_pressed = None;
            self.draw_needed.set(true);
        }

        // Phase B.4: clear any active cross-backend drag state. The
        // dispatcher returns a MouseUp event we could forward to the
        // engine later, but today no consumer cares about mouse-up
        // beyond clearing drag state.
        {
            let drag_rc = backend.drag_state_handle();
            let mut drag = drag_rc.borrow_mut();
            if drag.is_active() {
                let stack_rc = backend.modal_stack_handle();
                let stack = stack_rc.borrow();
                let _events = quadraui::dispatch_mouse_up(
                    &stack,
                    &mut drag,
                    quadraui::Point { x: 0.0, y: 0.0 },
                    quadraui::MouseButton::Left,
                );
            }
        }

        // Tab drag drop — shared `handle_release` that both backends call.
        if self.tab_drag.handle_release(&mut self.engine.borrow_mut()) {
            self.draw_needed.set(true);
        }
        // Explorer drag-and-drop: execute the move on release. Also clears
        // `sidebar_pointer_captured` so a future unrelated press does not
        // start out already "dragging".
        if self.explorer_drag_src.take().is_some() || self.explorer_drag_active.is_some() {
            self.sidebar_pointer_captured.set(false);
        }
        if let Some((src_row, target_row)) = self.explorer_drag_active.take() {
            render::apply_explorer_drop(&mut self.engine.borrow_mut(), src_row, target_row);
            self.draw_needed.set(true);
        }
        if self.terminal_split_dragging {
            self.terminal_split_dragging = false;
            if self.cached_char_width > 0.0 {
                let engine = self.engine.borrow();
                let left_cols = if engine.terminal_split_left_cols > 0 {
                    engine.terminal_split_left_cols
                } else if !engine.terminal_panes.is_empty() {
                    engine.terminal_panes[0].session.cols()
                } else {
                    0
                };
                let rows = engine.session.terminal_panel_rows;
                drop(engine);
                if left_cols > 0 {
                    // Was `let da_w = 800.0;` — a fixed guess that
                    // only produced the right column count in a window that
                    // happened to be exactly 800px wide. `width` is the real
                    // live panel width the caller (`UiEvent::MouseUp`) reads
                    // off `ctx.layout.main_content_bounds`, same source
                    // `handle_mouse_drag_msg`'s `TerminalSplitDivider` arm
                    // already uses while the drag is in progress.
                    let total_cols = self.terminal_panel_cols(width);
                    let right_cols = total_cols.saturating_sub(left_cols);
                    self.engine
                        .borrow_mut()
                        .terminal_split_finalize_drag(left_cols, right_cols, rows);
                }
            }
        }
        if self.terminal_resize_dragging {
            self.terminal_resize_dragging = false;
            let rows = self.engine.borrow().session.terminal_panel_rows;
            // `width` is this handler's own live panel width.
            let cols = self.terminal_panel_cols(width);
            self.engine.borrow_mut().terminal_resize(cols, rows);
            let _ = self.engine.borrow().session.save();
        }
        self.divider_grab = None;
        self.engine.borrow().cmd_dragging.set(false);
        {
            let mut engine = self.engine.borrow_mut();
            engine.mouse_drag_active = false;
            engine.mouse_drag_origin_window = None;
            // Auto-copy terminal selection on mouse-release, mirroring
            // TUI.  terminal_autocopy_selection() is a no-op when the
            // terminal isn't focused or has no selection.
            engine.terminal_autocopy_selection();
        }
        self.draw_needed.set(true);
    }
}
