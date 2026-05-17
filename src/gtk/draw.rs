use super::*;

/// Pango font family for UI panels (menu bar, sidebars, dropdown,
/// dialogs, hover popups). Size is appended at use via [`UI_FONT`]
/// from the configured `settings.ui_font_size` (#217).
pub(super) const UI_FONT_FAMILY: &str = "Segoe UI, Ubuntu, Droid Sans, Sans";

/// Process-global UI font size (points). Synced from
/// `settings.ui_font_size` at the start of each frame by
/// [`sync_ui_font_size`]. Read everywhere a Pango font description
/// is built — avoids threading `&Settings` through ~20 draw
/// functions for what's effectively one shared knob (#217).
static UI_FONT_SIZE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(10);

/// Update the process-global UI font size from `settings`. Called
/// once per frame at the top of [`draw_editor`].
pub(super) fn sync_ui_font_size(settings: &core::settings::Settings) {
    UI_FONT_SIZE.store(
        settings.ui_font_size.max(6),
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// Pango font description string for UI chrome at the currently
/// configured size. Drop-in replacement for the legacy `UI_FONT`
/// const — call sites do `FontDescription::from_string(&UI_FONT())`.
#[allow(non_snake_case)]
pub(super) fn UI_FONT() -> String {
    format!(
        "{} {}",
        UI_FONT_FAMILY,
        UI_FONT_SIZE.load(std::sync::atomic::Ordering::Relaxed)
    )
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn draw_editor(
    cr: &Context,
    engine: &Engine,
    width: i32,
    height: i32,
    sender: &relm4::Sender<Msg>,
    h_sb_hovered: bool,
    tab_close_hover: Option<(usize, usize)>,
    h_sb_dragging_window: Option<core::WindowId>,
    last_metrics: &std::rc::Rc<std::cell::Cell<(f64, f64)>>,
    tab_slot_positions_out: &Rc<RefCell<TabSlotMap>>,
    tab_close_bounds_out: &Rc<RefCell<TabCloseMap>>,
    diff_btn_map_out: &Rc<RefCell<DiffBtnMap>>,
    split_btn_map_out: &Rc<RefCell<SplitBtnMap>>,
    action_btn_map_out: &Rc<RefCell<ActionBtnMap>>,
    dialog_btn_rects_out: &Rc<RefCell<DialogBtnRects>>,
    dialog_popup_rect_out: &Rc<Cell<Option<(f64, f64, f64, f64)>>>,
    editor_hover_rect_out: &Rc<Cell<Option<(f64, f64, f64, f64)>>>,
    completion_layout_out: &Rc<RefCell<Option<quadraui::CompletionsLayout>>>,
    context_menu_layout_out: &Rc<RefCell<Option<quadraui::ContextMenuLayout>>>,
    tab_switcher_popup_rect_out: &Rc<Cell<Option<(f64, f64, f64, f64)>>>,
    editor_hover_link_rects_out: &Rc<RefCell<Vec<(f64, f64, f64, f64, String)>>>,
    editor_hover_scrollbar_out: &Rc<Cell<Option<render::PopupScrollbarHit>>>,
    mouse_pos: (f64, f64),
    tab_visible_counts_out: &Rc<RefCell<Vec<(crate::core::window::GroupId, usize, usize)>>>,
    status_segment_map_out: &Rc<RefCell<StatusSegmentMap>>,
    screen_layout_out: &Rc<RefCell<Option<render::ScreenLayout>>>,
    debug_toolbar_layout_out: &Rc<RefCell<Option<quadraui::StatusBarLayout>>>,
    debug_toolbar_y_offset_out: &Rc<Cell<f64>>,
    debug_toolbar_height_out: &Rc<Cell<f64>>,
    // Phase B.5 Stage 3: shared `quadraui::Backend` impl. Routed
    // through to draw paths that go through the trait. Today only
    // the quickfix panel uses it as a pilot — the rest still call
    // `quadraui_gtk::draw_*` shims directly.
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    debug_toolbar_hovered_id: Option<&quadraui::WidgetId>,
    debug_toolbar_pressed_id: Option<&quadraui::WidgetId>,
) {
    let theme = Theme::from_name(&engine.settings.colorscheme);

    // Sync the UI font size atomic from settings so all
    // `UI_FONT()` callers below see the configured size (#217).
    sync_ui_font_size(&engine.settings);

    // Clear cached button positions from previous frame.
    diff_btn_map_out.borrow_mut().clear();
    split_btn_map_out.borrow_mut().clear();
    action_btn_map_out.borrow_mut().clear();
    status_segment_map_out.borrow_mut().clear();
    engine.scroll_surfaces.borrow_mut().clear();

    // 1. Background
    let (bg_r, bg_g, bg_b) = theme.background.to_cairo();
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.paint().ok();

    // 2. Setup Pango
    let pango_ctx = pangocairo::create_context(cr);
    let layout = pango::Layout::new(&pango_ctx);

    // Use configurable font from settings
    let font_str = format!(
        "{} {}",
        engine.settings.font_family, engine.settings.font_size
    );
    let font_desc = FontDescription::from_string(&font_str);
    layout.set_font_description(Some(&font_desc));

    // Derive line height and char width from font metrics.
    // Use Pango layout measurement for char_width (not approximate_char_width)
    // so the value matches actual glyph positioning in draw_editor.
    let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
    let line_height = (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;
    layout.set_text("0");
    let char_width = layout.pixel_size().0 as f64;

    // Only send CacheFontMetrics when metrics actually change (e.g. on startup or font change).
    // Sending on every draw creates a feedback loop: draw → message → #[watch] → queue_draw → draw.
    let (last_lh, last_cw) = last_metrics.get();
    if (last_lh - line_height).abs() > 0.01 || (last_cw - char_width).abs() > 0.01 {
        last_metrics.set((line_height, char_width));
        sender
            .send(Msg::CacheFontMetrics(line_height, char_width))
            .ok();
    }

    // Calculate layout regions.
    // When terminal is maximized, breadcrumbs are suppressed — the editor
    // area is reduced to the tab row only so the panel can fill the rest.
    let show_breadcrumbs = engine.settings.breadcrumbs && !engine.terminal_maximized;
    let tab_row_height = (line_height * 1.6).ceil();
    let tab_bar_height = if show_breadcrumbs {
        tab_row_height + line_height
    } else {
        tab_row_height
    };
    let wildmenu_px = if engine.wildmenu_items.is_empty() {
        0.0
    } else {
        line_height
    };
    let per_window_status = engine.settings.window_status_line;
    let bottom_panel_open_early = engine.terminal_open || engine.bottom_panel_open;
    let has_separated =
        per_window_status && !engine.settings.status_line_above_terminal && bottom_panel_open_early;
    // Bottom chrome: cmd line + optional global status + wildmenu.
    let global_status_rows = if per_window_status { 1.0 } else { 2.0 };
    let status_bar_height = line_height * global_status_rows + wildmenu_px;

    // Reserve space for the quickfix panel when open
    const QUICKFIX_ROWS: usize = 6; // 1 header + 5 result rows
    let qf_px = if engine.quickfix_open && !engine.quickfix_items.is_empty() {
        QUICKFIX_ROWS as f64 * line_height
    } else {
        0.0
    };

    // Reserve space for the bottom panel when open (1 tab-bar row + content rows).
    // Triggered by either a live terminal OR the debug output panel being shown.
    // When maximized, the effective row count is derived from the current DA
    // height each frame so window resizes take effect immediately.
    let term_px =
        crate::render::compute_editor_layout(engine, height as f64, line_height, false).terminal_h;

    let debug_toolbar_px = if engine.debug_toolbar_visible {
        line_height
    } else {
        0.0
    };

    // Separated status: extracted from windows whenever terminal is open.
    // When noslat + terminal: separated status row goes below terminal.
    let separated_status_px = if has_separated {
        line_height // status row below terminal (cmd already in status_bar_height)
    } else {
        0.0
    };

    // Calculate window rects for all editor groups.
    // editor_bounds spans the full editor area from y=0; tab_bar_height is reserved per group.
    let editor_bounds = WindowRect::new(
        0.0,
        0.0,
        width as f64,
        height as f64
            - status_bar_height
            - debug_toolbar_px
            - qf_px
            - term_px
            - separated_status_px,
    );
    let (window_rects, _group_dividers) =
        engine.calculate_group_window_rects(editor_bounds, tab_bar_height);

    // Build the platform-agnostic screen layout
    let screen = build_screen_layout(
        engine,
        &theme,
        &window_rects,
        line_height,
        char_width,
        false,
    );

    // 3b. Draw each window (before tab bars so tabs paint on top)
    for rendered_window in &screen.windows {
        draw_window(
            cr,
            &layout,
            &font_metrics,
            &theme,
            rendered_window,
            char_width,
            line_height,
        );
    }

    // 3c. Draw window separators
    draw_window_separators(cr, &window_rects, &theme);

    // 4. Draw tab bar(s) ON TOP of windows — one per editor group.
    // (Drawn after windows so tab bars are never overwritten by window backgrounds.)
    {
        let mut slots = tab_slot_positions_out.borrow_mut();
        slots.clear();
    }
    if let Some(ref split) = screen.editor_group_split {
        // Draw divider lines first (behind tab bars).
        let (sr, sg, sb) = theme.separator.to_cairo();
        cr.set_source_rgb(sr, sg, sb);
        cr.set_line_width(1.0);
        for div in &split.dividers {
            match div.direction {
                crate::core::window::SplitDirection::Vertical => {
                    cr.move_to(div.position, div.cross_start);
                    cr.line_to(div.position, div.cross_start + div.cross_size);
                    cr.stroke().ok();
                }
                crate::core::window::SplitDirection::Horizontal => {
                    cr.move_to(div.cross_start, div.position);
                    cr.line_to(div.cross_start + div.cross_size, div.position);
                    cr.stroke().ok();
                }
            }
        }
        // Draw each group's tab bar ON TOP of dividers.
        for gtb in &split.group_tab_bars {
            if engine.is_tab_bar_hidden(gtb.group_id) {
                continue;
            }
            let tab_y = gtb.bounds.y - tab_bar_height;
            let tab_x = gtb.bounds.x;
            let tab_w = gtb.bounds.width;
            cr.save().ok();
            cr.rectangle(tab_x, tab_y, tab_w, tab_row_height);
            cr.clip();
            cr.translate(tab_x, tab_y);
            let hover_idx = tab_close_hover.and_then(|(gid, tidx)| {
                if gid == gtb.group_id.0 {
                    Some(tidx)
                } else {
                    None
                }
            });
            let (positions, close_b, dbp, sbp, vis_count, abp, correct_offset) = draw_tab_bar(
                backend,
                cr,
                &layout,
                &theme,
                &gtb.bar,
                tab_w,
                line_height,
                0.0,
                hover_idx,
            );
            tab_slot_positions_out
                .borrow_mut()
                .insert(gtb.group_id.0, positions);
            tab_close_bounds_out
                .borrow_mut()
                .insert(gtb.group_id.0, close_b);
            if let Some(dp) = dbp {
                diff_btn_map_out.borrow_mut().insert(gtb.group_id.0, dp);
            }
            if let Some(sp) = sbp {
                split_btn_map_out.borrow_mut().insert(gtb.group_id.0, sp);
            }
            if let Some(ap) = abp {
                action_btn_map_out.borrow_mut().insert(gtb.group_id.0, ap);
            }
            tab_visible_counts_out
                .borrow_mut()
                .push((gtb.group_id, vis_count, correct_offset));
            cr.restore().ok();
        }
    } else if !engine.is_tab_bar_hidden(engine.active_group) {
        // Single group: draw tab bar at full width with split buttons.
        let hover_idx = tab_close_hover.map(|(_gid, tidx)| tidx);
        let (positions, close_b, dbp, sbp, vis_count, abp, correct_offset) = draw_tab_bar(
            backend,
            cr,
            &layout,
            &theme,
            &screen.tab_bar_primitive,
            width as f64,
            line_height,
            0.0,
            hover_idx,
        );
        // Use group_id 0 for single-group mode
        tab_slot_positions_out
            .borrow_mut()
            .insert(engine.active_group.0, positions);
        tab_close_bounds_out
            .borrow_mut()
            .insert(engine.active_group.0, close_b);
        if let Some(dp) = dbp {
            diff_btn_map_out
                .borrow_mut()
                .insert(engine.active_group.0, dp);
        }
        if let Some(sp) = sbp {
            split_btn_map_out
                .borrow_mut()
                .insert(engine.active_group.0, sp);
        }
        if let Some(ap) = abp {
            action_btn_map_out
                .borrow_mut()
                .insert(engine.active_group.0, ap);
        }
        tab_visible_counts_out
            .borrow_mut()
            .push((engine.active_group, vis_count, correct_offset));
    }

    // 4b. Draw breadcrumb bar(s) below tab bar(s). Skipped while the terminal
    // is maximized so the panel can claim the breadcrumb row.
    for bc in &screen.breadcrumbs {
        if bc.segments.is_empty() || engine.terminal_maximized {
            continue;
        }
        let bc_y = bc.bounds.y;
        let bc_x = bc.bounds.x;
        let bc_w = bc.bounds.width;
        cr.save().ok();
        cr.translate(bc_x, 0.0);
        let bar_layout = draw_breadcrumb_bar(
            backend,
            cr,
            &layout,
            &theme,
            &bc.bar,
            bc_w,
            line_height,
            bc_y,
        );
        cr.restore().ok();
        *bc.draw_layout.borrow_mut() = Some(bar_layout);
    }

    // 5. Draw tab drag overlay (drop zone highlight + ghost label).
    if engine.tab_drag.is_some() {
        draw_tab_drag_overlay(
            cr,
            engine,
            &theme,
            width as f64,
            height as f64,
            line_height,
            char_width,
            &layout,
            &tab_slot_positions_out.borrow(),
        );
    }

    // 5b. Draw completion popup (on top of everything else). Cache
    //     the layout so the click handler can hit-test items.
    *completion_layout_out.borrow_mut() =
        draw_completion_popup(cr, &layout, &screen, &theme, line_height, char_width);

    // 5c. Draw hover popup (on top of everything else)
    draw_hover_popup(
        cr,
        &layout,
        &screen,
        &theme,
        line_height,
        char_width,
        width as f64,
        height as f64,
    );

    // 5c2. Draw signature-help popup (on top of everything else, shown in insert mode)
    draw_signature_popup(
        cr,
        &layout,
        &screen,
        &theme,
        line_height,
        char_width,
        width as f64,
        height as f64,
    );

    // 5c3. Draw diff peek popup (inline git hunk preview)
    draw_diff_peek_popup(
        cr,
        &layout,
        &screen,
        &theme,
        line_height,
        char_width,
        width as f64,
        height as f64,
    );

    // 5c4. Draw editor hover popup (gh key, diagnostic/annotation/plugin hovers)
    let (eh_rect, eh_links, eh_sb) =
        draw_editor_hover_popup(cr, &layout, &screen, &theme, line_height, char_width);
    editor_hover_rect_out.set(eh_rect);
    *editor_hover_link_rects_out.borrow_mut() = eh_links;
    editor_hover_scrollbar_out.set(eh_sb);

    // 5c5. Draw tab hover tooltip (small popup below hovered tab).
    if let Some(ref tooltip_text) = screen.tab_tooltip {
        let (mx, _my) = mouse_pos;
        if mx >= 0.0 {
            let tab_row_h = (line_height * 1.4).ceil();
            let tab_bar_h = if !screen.breadcrumbs.is_empty() {
                tab_row_h + line_height
            } else {
                tab_row_h
            };
            let tooltip_y = tab_bar_h + 2.0;
            let padding = 6.0;
            layout.set_text(tooltip_text);
            layout.set_width(-1);
            let (text_w, text_h) = layout.pixel_size();
            let box_w = text_w as f64 + padding * 2.0;
            let box_h = text_h as f64 + padding * 2.0;
            let tooltip_x = mx.max(0.0).min((width as f64 - box_w).max(0.0));

            // Background
            cr.set_source_rgba(
                theme.hover_bg.r as f64 / 255.0,
                theme.hover_bg.g as f64 / 255.0,
                theme.hover_bg.b as f64 / 255.0,
                0.95,
            );
            cr.rectangle(tooltip_x, tooltip_y, box_w, box_h);
            let _ = cr.fill();

            // Border
            cr.set_source_rgba(
                theme.hover_border.r as f64 / 255.0,
                theme.hover_border.g as f64 / 255.0,
                theme.hover_border.b as f64 / 255.0,
                0.8,
            );
            cr.rectangle(tooltip_x, tooltip_y, box_w, box_h);
            cr.set_line_width(1.0);
            let _ = cr.stroke();

            // Text
            cr.set_source_rgb(
                theme.hover_fg.r as f64 / 255.0,
                theme.hover_fg.g as f64 / 255.0,
                theme.hover_fg.b as f64 / 255.0,
            );
            cr.move_to(tooltip_x + padding, tooltip_y + padding);
            pangocairo::show_layout(cr, &layout);
        }
    }

    // 5f2. Draw quickfix panel (persistent bottom strip above status bar)
    if qf_px > 0.0 {
        let qf_y = height as f64 - status_bar_height - debug_toolbar_px - qf_px - term_px;
        draw_quickfix_panel(
            cr,
            &layout,
            &screen,
            &theme,
            0.0,
            qf_y,
            width as f64,
            qf_px,
            line_height,
            backend,
        );
    }

    // 5g. Draw bottom panel (Terminal or Debug Output) with a tab bar.
    if term_px > 0.0 {
        // When maximized, snap the panel up to the editor tab bar's bottom
        // edge. Without this, the row-based `PanelChromeDesc` helper reserves
        // `ceil(1.6) = 2` row-units for the editor tab bar while GTK's
        // actual `tab_bar_height = ceil(1.6 * line_height)` is only 1.6 lh —
        // the 0.4 lh slack leaks through as a strip of editor content above
        // the terminal, and shows up as a partial first line ("25 …").
        let (term_y, term_px) = if engine.terminal_maximized {
            let snapped_y = tab_bar_height;
            let snapped_px = (height as f64
                - status_bar_height
                - debug_toolbar_px
                - separated_status_px
                - snapped_y)
                .max(line_height);
            (snapped_y, snapped_px)
        } else {
            let y = height as f64 - status_bar_height - debug_toolbar_px - term_px;
            (y, term_px)
        };
        engine
            .bottom_panel_geometry
            .replace(Some(crate::core::engine::BottomPanelGeometry {
                top_y: term_y,
                height: term_px,
                toolbar_y: line_height,
                content_y: 2.0 * line_height,
                content_row_h: line_height,
            }));
        // Tab bar row (1 line high) at the top of the bottom panel area.
        let hits = draw_bottom_panel_tabs(
            backend,
            cr,
            &layout,
            &screen.bottom_tabs.active,
            &theme,
            0.0,
            term_y,
            width as f64,
            line_height,
            engine.terminal_open,
            !screen.bottom_tabs.output_lines.is_empty(),
        );
        engine.bottom_tab_bar_hits.replace(Some(hits));
        match screen.bottom_tabs.active {
            render::BottomPanelKind::Terminal => {
                if let Some(ref term_panel) = screen.bottom_tabs.terminal {
                    let toolbar_y = term_y + line_height;
                    let hits = draw_terminal_toolbar(
                        backend,
                        cr,
                        &layout,
                        term_panel,
                        &theme,
                        0.0,
                        toolbar_y,
                        width as f64,
                        line_height,
                    );
                    engine.terminal_toolbar_hits.replace(Some(hits));
                    let content_h = (term_px - 2.0 * line_height).max(0.0);
                    if content_h > 0.0 {
                        let content_y = toolbar_y + line_height;
                        let visible_rows = (content_h / line_height) as usize;
                        let q_theme = super::quadraui_gtk::q_theme(&theme);
                        let (tbgr, tbgg, tbgb) = theme.terminal_bg.to_cairo();
                        cr.set_source_rgb(tbgr, tbgg, tbgb);
                        cr.rectangle(0.0, content_y, width as f64, content_h);
                        cr.fill().ok();
                        let q_area = quadraui::Rect::new(
                            0.0,
                            content_y as f32,
                            width as f32,
                            content_h as f32,
                        );
                        let td = render::build_terminal_draw_data(
                            term_panel,
                            q_area,
                            char_width as f32,
                            line_height as f32,
                            visible_rows,
                            Some(6),
                        );
                        engine.terminal_split_layout.replace(td.split);
                        {
                            let mut b = backend.borrow_mut();
                            b.set_current_theme(q_theme);
                            b.set_current_line_height(line_height);
                            b.set_current_char_width(char_width);
                        }
                        if let Some(split) = &td.split {
                            let left = td.left.as_ref().unwrap();
                            let right = td.right.as_ref().unwrap();
                            let sl = *split;
                            backend.borrow_mut().enter_frame_scope(cr, &layout, |b| {
                                use quadraui::Backend;
                                b.draw_terminal(sl.left, left);
                                b.draw_terminal(sl.right, right);
                            });
                            quadraui::gtk::draw_terminal_divider(
                                cr,
                                split.divider_x as f64,
                                content_y,
                                content_h,
                                &q_theme,
                            );
                        } else if let Some(ref term) = td.single {
                            backend.borrow_mut().enter_frame_scope(cr, &layout, |b| {
                                use quadraui::Backend;
                                b.draw_terminal(q_area, term);
                            });
                        }
                        // Register scroll surface for dispatch.
                        let geom = render::terminal_scrollbar_geometry(term_panel, visible_rows);
                        let surface_sb = geom.map(|g| {
                            let sb_w = 6.0;
                            let sb_x = width as f64 - sb_w;
                            let thumb_t = g.thumb_top_frac * content_h;
                            let thumb_h = (g.thumb_height_frac * content_h).max(4.0);
                            quadraui::SurfaceScrollbar {
                                axis: quadraui::ScrollAxis::Vertical,
                                track_bounds: quadraui::Rect::new(
                                    sb_x as f32,
                                    content_y as f32,
                                    sb_w as f32,
                                    content_h as f32,
                                ),
                                thumb_bounds: quadraui::Rect::new(
                                    (sb_x + 1.0) as f32,
                                    (content_y + thumb_t) as f32,
                                    (sb_w - 2.0) as f32,
                                    thumb_h as f32,
                                ),
                                total_items: g.total_items,
                                visible_items: g.visible_items,
                                scroll_offset: term_panel.scroll_offset,
                                inverted: true,
                            }
                        });
                        engine
                            .scroll_surfaces
                            .borrow_mut()
                            .push(quadraui::ScrollSurface {
                                id: quadraui::WidgetId::new("terminal_scrollback"),
                                bounds: quadraui::Rect::new(
                                    0.0,
                                    content_y as f32,
                                    width as f32,
                                    content_h as f32,
                                ),
                                scrollbar: surface_sb,
                            });
                    }
                }
            }
            render::BottomPanelKind::DebugOutput => {
                let td = render::debug_output_to_text_display(
                    &screen.bottom_tabs.output_lines,
                    engine.debug_output_scroll,
                    engine.debug_output_auto_scroll,
                );
                let q_rect = quadraui::Rect::new(
                    0.0,
                    (term_y + line_height) as f32,
                    width as f32,
                    (term_px - line_height) as f32,
                );
                let td_layout = {
                    use quadraui::Backend;
                    let mut b = backend.borrow_mut();
                    b.set_current_theme(super::quadraui_gtk::q_theme(&theme));
                    b.set_current_line_height(line_height);
                    let layout_result = b.text_display_layout(q_rect, &td);
                    b.enter_frame_scope(cr, &layout, |b| {
                        b.draw_text_display(q_rect, &td);
                    });
                    layout_result
                };
                let scrollbar =
                    td_layout
                        .scrollbar_bounds
                        .zip(td_layout.thumb_bounds)
                        .map(|(track, thumb)| {
                            let offset_y = q_rect.y;
                            quadraui::SurfaceScrollbar {
                                axis: quadraui::ScrollAxis::Vertical,
                                track_bounds: quadraui::Rect::new(
                                    q_rect.x + track.x,
                                    offset_y + track.y,
                                    track.width,
                                    track.height,
                                ),
                                thumb_bounds: quadraui::Rect::new(
                                    q_rect.x + thumb.x,
                                    offset_y + thumb.y,
                                    thumb.width,
                                    thumb.height,
                                ),
                                total_items: td.lines.len(),
                                visible_items: td_layout.visible_lines.len(),
                                scroll_offset: td_layout.resolved_scroll_offset,
                                inverted: false,
                            }
                        });
                engine
                    .scroll_surfaces
                    .borrow_mut()
                    .push(quadraui::ScrollSurface {
                        id: quadraui::WidgetId::new("debug_output"),
                        bounds: q_rect,
                        scrollbar,
                    });
            }
        }
    } else {
        engine.bottom_panel_geometry.replace(None);
    }

    // 5h. Draw debug toolbar strip if visible (above status bar)
    if let Some(ref toolbar) = screen.debug_toolbar {
        let toolbar_y = height as f64 - status_bar_height - debug_toolbar_px;
        draw_debug_toolbar(
            backend,
            cr,
            toolbar,
            &theme,
            0.0,
            toolbar_y,
            width as f64,
            line_height,
            debug_toolbar_layout_out,
            debug_toolbar_hovered_id,
            debug_toolbar_pressed_id,
        );
        debug_toolbar_y_offset_out.set(toolbar_y);
        debug_toolbar_height_out.set(line_height);
    } else {
        debug_toolbar_y_offset_out.set(0.0);
        debug_toolbar_height_out.set(0.0);
    }

    // 5i. Draw horizontal scrollbars in Cairo (VSCode-style overlay on window bottom)
    draw_h_scrollbars(
        cr,
        engine,
        &theme,
        &window_rects,
        char_width,
        line_height,
        h_sb_hovered,
        h_sb_dragging_window,
    );

    // 5j. Draw per-window status bars (after scrollbars so they paint on top)
    //     When terminal_maximized, the editor area is collapsed to just the
    //     tab bar; drawing the per-window status line there would overlap the
    //     terminal toolbar below, so we skip it entirely.
    if per_window_status && !engine.terminal_maximized {
        for rendered_window in &screen.windows {
            if let Some(ref status) = rendered_window.status_line {
                let wr = &rendered_window.rect;
                let bar_y = wr.y + wr.height - line_height;
                let mut zones = Vec::new();
                draw_window_status_bar(
                    backend,
                    cr,
                    &layout,
                    &theme,
                    status,
                    wr.x,
                    bar_y,
                    wr.width,
                    line_height,
                    &mut zones,
                );
                status_segment_map_out
                    .borrow_mut()
                    .insert(rendered_window.window_id.0, zones);
            }
        }
    }

    // 5k–7. Status line, wildmenu, and command line.
    if let Some(ref status) = screen.separated_status_line {
        // noslat + terminal: [terminal][debug] ... [sep_status][wildmenu?][cmd]
        let status_y = height as f64 - status_bar_height - separated_status_px;
        let mut zones = Vec::new();
        draw_window_status_bar(
            backend,
            cr,
            &layout,
            &theme,
            status,
            0.0,
            status_y,
            width as f64,
            line_height,
            &mut zones,
        );
        status_segment_map_out
            .borrow_mut()
            .insert(screen.active_window_id.0, zones);
        let mut next_y = status_y + line_height;
        if let Some(ref wm) = screen.wildmenu {
            let bar = render::wildmenu_to_status_bar(wm, &theme);
            {
                use quadraui::Backend;
                backend.borrow_mut().enter_frame_scope(cr, &layout, |b| {
                    b.set_current_theme(super::quadraui_gtk::q_theme(&theme));
                    b.set_current_line_height(line_height);
                    b.draw_status_bar(
                        quadraui::Rect::new(0.0, next_y as f32, width as f32, line_height as f32),
                        &bar,
                        None,
                        None,
                    );
                });
            }
            next_y += line_height;
        }
        draw_command_line(
            cr,
            &layout,
            &theme,
            &screen.command,
            width as f64,
            next_y,
            line_height,
        );
    } else {
        // No terminal: original layout with per-window or global status at bottom
        let status_y = height as f64 - status_bar_height;
        if let Some(ref bar) = screen.global_status_bar {
            use quadraui::Backend;
            backend.borrow_mut().enter_frame_scope(cr, &layout, |b| {
                b.set_current_theme(super::quadraui_gtk::q_theme(&theme));
                b.set_current_line_height(line_height);
                b.draw_status_bar(
                    quadraui::Rect::new(0.0, status_y as f32, width as f32, line_height as f32),
                    bar,
                    None,
                    None,
                );
            });
        }
        let mut next_y = if per_window_status {
            status_y
        } else {
            status_y + line_height
        };
        if let Some(ref wm) = screen.wildmenu {
            let bar = render::wildmenu_to_status_bar(wm, &theme);
            {
                use quadraui::Backend;
                backend.borrow_mut().enter_frame_scope(cr, &layout, |b| {
                    b.set_current_theme(super::quadraui_gtk::q_theme(&theme));
                    b.set_current_line_height(line_height);
                    b.draw_status_bar(
                        quadraui::Rect::new(0.0, next_y as f32, width as f32, line_height as f32),
                        &bar,
                        None,
                        None,
                    );
                });
            }
            next_y += line_height;
        }
        draw_command_line(
            cr,
            &layout,
            &theme,
            &screen.command,
            width as f64,
            next_y,
            line_height,
        );
    }

    // 8. Popups and modals — drawn last so they appear on top of everything.
    draw_find_replace_popup(
        cr,
        &layout,
        &screen,
        &theme,
        width as f64,
        height as f64,
        line_height,
        char_width,
    );

    draw_picker_popup(
        cr,
        &layout,
        &screen,
        &theme,
        width as f64,
        height as f64,
        line_height,
        backend,
    );

    tab_switcher_popup_rect_out.set(draw_tab_switcher_popup_list(
        &screen,
        width as f64,
        height as f64,
        line_height,
        backend,
        cr,
        &layout,
    ));

    let (btn_rects, popup_rect) = draw_dialog_popup(
        cr,
        &layout,
        &screen,
        &theme,
        width as f64,
        height as f64,
        line_height,
    );
    *dialog_btn_rects_out.borrow_mut() = btn_rects;
    dialog_popup_rect_out.set(popup_rect);

    *context_menu_layout_out.borrow_mut() = draw_context_menu_popup(
        cr,
        &layout,
        &screen,
        &theme,
        width as f64,
        height as f64,
        char_width,
        line_height,
        mouse_pos,
    );

    // Cache the ScreenLayout for click handlers (#344). Moves, not clones.
    *screen_layout_out.borrow_mut() = Some(screen);
}

/// Draw thin Cairo horizontal scrollbars that overlay the bottom of each editor
/// window (VSCode style). Only shown when content is wider than the viewport.
/// `hovered` — mouse is over any scrollbar track (brightens the thumb).
/// `dragging_window` — window being dragged (shows the active/dragging colour).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_h_scrollbars(
    cr: &Context,
    engine: &Engine,
    theme: &Theme,
    window_rects: &[(core::WindowId, core::WindowRect)],
    char_width: f64,
    line_height: f64,
    hovered: bool,
    dragging_window: Option<core::WindowId>,
) {
    let q_theme = super::quadraui_gtk::q_theme(theme);
    for (window_id, rect) in window_rects {
        let Some((track_x, track_y, track_w, sb_height, thumb_x, thumb_w, _, _)) =
            h_scrollbar_geometry(engine, *window_id, rect, char_width, line_height)
        else {
            continue;
        };

        let scrollbar = quadraui::Scrollbar {
            id: quadraui::WidgetId::new("gtk:editor:h_scrollbar"),
            axis: quadraui::ScrollAxis::Horizontal,
            track: quadraui::Rect::new(
                track_x as f32,
                track_y as f32,
                track_w as f32,
                sb_height as f32,
            ),
            thumb_start: (thumb_x - track_x) as f32,
            thumb_len: thumb_w as f32,
            hovered,
            dragging: dragging_window == Some(*window_id),
        };
        quadraui::gtk::draw_scrollbar(cr, &scrollbar, &q_theme);
    }
}

/// Draw the tab drag overlay: a semi-transparent highlight over the drop zone
/// and a ghost label near the cursor.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_tab_drag_overlay(
    cr: &Context,
    engine: &Engine,
    theme: &Theme,
    width: f64,
    height: f64,
    line_height: f64,
    _char_width: f64,
    pango_layout: &pango::Layout,
    tab_slot_positions: &TabSlotMap,
) {
    let (bounds, tbh, slots_map) =
        super::click::build_gtk_tab_slots(engine, width, height, line_height, tab_slot_positions);
    let (groups, eff_tbh) = render::build_tab_drop_groups(&bounds, engine, tbh, &slots_map);
    let tbh = eff_tbh;
    let cursor = engine
        .tab_drag_mouse
        .map(|(mx, my)| (mx as f32, my as f32))
        .unwrap_or((0.0, 0.0));
    let overlay = match render::compute_tab_drop_overlay(
        &engine.tab_drop_zone,
        &groups,
        cursor,
        tbh,
        2.0,
        12.0,
    ) {
        Some(o) => o,
        None => return,
    };

    if let Some(h) = overlay.highlight {
        let (cr_r, cr_g, cr_b) = theme.cursor.to_cairo();
        cr.set_source_rgba(cr_r, cr_g, cr_b, 0.15);
        cr.rectangle(h.x as f64, h.y as f64, h.width as f64, h.height as f64);
        cr.fill().ok();
        cr.set_source_rgba(cr_r, cr_g, cr_b, 0.5);
        cr.set_line_width(2.0);
        cr.rectangle(h.x as f64, h.y as f64, h.width as f64, h.height as f64);
        cr.stroke().ok();
    }

    if let Some(bar) = overlay.insertion_bar {
        let (cr_r, cr_g, cr_b) = theme.cursor.to_cairo();
        cr.set_source_rgb(cr_r, cr_g, cr_b);
        cr.rectangle(
            bar.x as f64,
            bar.y as f64,
            bar.width as f64,
            bar.height as f64,
        );
        cr.fill().ok();
    }

    if let (Some((mx, my)), Some(ref drag)) = (engine.tab_drag_mouse, &engine.tab_drag) {
        let label = &drag.tab_name;
        if !label.is_empty() {
            pango_layout.set_text(label);
            let (tw, th) = pango_layout.pixel_size();
            let gx = mx + 12.0;
            let gy = my - th as f64 / 2.0;
            let pad = 4.0;
            let (gbr, gbg, gbb) = theme.background.to_cairo();
            cr.set_source_rgba(gbr, gbg, gbb, 0.85);
            cr.rectangle(
                gx - pad,
                gy - pad,
                tw as f64 + pad * 2.0,
                th as f64 + pad * 2.0,
            );
            cr.fill().ok();
            let (gfr, gfg, gfb) = theme.foreground.to_cairo();
            cr.set_source_rgba(gfr, gfg, gfb, 0.9);
            cr.move_to(gx, gy);
            pangocairo::show_layout(cr, pango_layout);
        }
    }
}

/// GTK tab bar renders via `Backend::draw_tab_bar`.
///
/// Takes a pre-built `quadraui::TabBar` primitive (from `ScreenLayout`)
/// and reshapes `TabBarHits.right_segment_bounds` (keyed by `WidgetId`)
/// into the vimcode-specific (diff_btns, split_btns, action_btn)
/// groupings the click handler consumes. The vimcode UI font is set
/// on the Pango layout before the trait call and restored afterwards
/// (the rasteriser uses whatever font is on the layout at call time).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_tab_bar(
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    cr: &Context,
    layout: &pango::Layout,
    theme: &Theme,
    bar: &quadraui::TabBar,
    width: f64,
    line_height: f64,
    y_offset: f64,
    hovered_close_tab: Option<usize>,
) -> TabBarDrawResult {
    use pango::FontDescription;

    // The rasteriser uses whatever font is on the layout. Vimcode renders
    // tabs in the UI sans-serif, not the editor monospace; set it before
    // the trait call and restore the caller's font after.
    let saved_font = layout.font_description().unwrap_or_default();
    let ui_font_desc = FontDescription::from_string(&UI_FONT());
    layout.set_font_description(Some(&ui_font_desc));

    use quadraui::Backend;
    let hits = backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
        b.set_current_theme(super::quadraui_gtk::q_theme(theme));
        b.set_current_line_height(line_height);
        let tab_row_h = (line_height * 1.6).ceil();
        b.draw_tab_bar(
            quadraui::Rect::new(0.0, y_offset as f32, width as f32, tab_row_h as f32),
            bar,
            hovered_close_tab,
        )
    });

    layout.set_font_description(Some(&saved_font));

    // Reshape `hits.right_segment_bounds` into vimcode's app-specific
    // (diff_btns, split_btns, action_btn) groupings using the
    // `WidgetId`s emitted by `build_tab_bar_primitive`.
    let mut prev: Option<(f64, f64)> = None;
    let mut next: Option<(f64, f64)> = None;
    let mut fold: Option<(f64, f64)> = None;
    let mut split_right: Option<(f64, f64)> = None;
    let mut split_down: Option<(f64, f64)> = None;
    let mut action: Option<(f64, f64)> = None;
    for (i, seg) in bar.right_segments.iter().enumerate() {
        let Some(bounds) = hits.right_segment_bounds.get(i).copied() else {
            continue;
        };
        if let Some(ref id) = seg.id {
            match id.as_str() {
                "tab:diff_prev" => prev = Some(bounds),
                "tab:diff_next" => next = Some(bounds),
                "tab:diff_toggle" => fold = Some(bounds),
                "tab:split_right" => split_right = Some(bounds),
                "tab:split_down" => split_down = Some(bounds),
                "tab:action_menu" => action = Some(bounds),
                _ => {}
            }
        }
    }
    let diff_btns = match (prev, next, fold) {
        (Some(p), Some(n), Some(f)) => Some((p.0, p.1, n.0, n.1, f.0, f.1)),
        _ => None,
    };
    // Preserve the legacy `(both_btns_px, btn_right_px)` contract.
    let split_btns = match (split_right, split_down) {
        (Some(sr), Some(sd)) => {
            let sr_w = sr.1 - sr.0;
            let sd_w = sd.1 - sd.0;
            Some((sr_w + sd_w, sr_w))
        }
        _ => None,
    };

    (
        hits.slot_positions,
        hits.close_bounds,
        diff_btns,
        split_btns,
        hits.available_cols,
        action,
        hits.correct_scroll_offset,
    )
}

/// Draw the breadcrumb bar via `Backend::draw_status_bar`.
///
/// The pre-built `quadraui::StatusBar` primitive comes from
/// `ScreenLayout` (built by `render::build_screen_layout`).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_breadcrumb_bar(
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    cr: &Context,
    layout: &pango::Layout,
    theme: &Theme,
    bar: &quadraui::StatusBar,
    width: f64,
    line_height: f64,
    y_offset: f64,
) -> quadraui::StatusBarLayout {
    use quadraui::Backend;
    let mut result = quadraui::StatusBarLayout {
        bar_width: 0.0,
        bar_height: 0.0,
        visible_segments: Vec::new(),
        hit_regions: Vec::new(),
        resolved_right_start: 0,
    };
    backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
        b.set_current_theme(super::quadraui_gtk::q_theme(theme));
        b.set_current_line_height(line_height);
        result = b.draw_status_bar(
            quadraui::Rect::new(0.0, y_offset as f32, width as f32, line_height as f32),
            bar,
            None,
            None,
        );
    });
    result
}

/// Render one editor window (pane) onto `cr`.
///
/// Phase C Stage 1D (#276) collapsed the body of this function to a
/// thin delegator. The actual paint code lives in
/// `quadraui::gtk::draw_editor`, fed by `render::to_q_editor` (the
/// boundary adapter that converts the engine-side `RenderedWindow`
/// IR into the cross-backend `quadraui::Editor` primitive). GTK
/// scrollbars are painted elsewhere (`draw_h_scrollbars` for
/// horizontal, native gtk4 widgets for vertical) — preserved by this
/// delegator since the rasteriser explicitly excludes scrollbar paint
/// on GTK. The per-window status line is also painted separately (it
/// was lifted in Session 241).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_window(
    cr: &Context,
    layout: &pango::Layout,
    font_metrics: &pango::FontMetrics,
    theme: &Theme,
    rw: &RenderedWindow,
    char_width: f64,
    line_height: f64,
) {
    let editor = render::to_q_editor(rw);
    let q_theme = super::quadraui_gtk::q_theme(theme);
    quadraui::gtk::draw_editor(
        cr,
        layout,
        font_metrics,
        &editor,
        &q_theme,
        char_width,
        line_height,
    );
}

pub(super) fn draw_window_separators(
    cr: &Context,
    window_rects: &[(core::WindowId, WindowRect)],
    theme: &Theme,
) {
    if window_rects.len() <= 1 {
        return;
    }

    let (sr, sg, sb) = theme.separator.to_cairo();
    cr.set_source_rgb(sr, sg, sb);
    cr.set_line_width(1.0);

    // Draw separators between adjacent windows
    for i in 0..window_rects.len() {
        for j in (i + 1)..window_rects.len() {
            let (_, rect_a) = &window_rects[i];
            let (_, rect_b) = &window_rects[j];

            // Check if they share a horizontal edge
            if (rect_a.y + rect_a.height - rect_b.y).abs() < 2.0 {
                let x_start = rect_a.x.max(rect_b.x);
                let x_end = (rect_a.x + rect_a.width).min(rect_b.x + rect_b.width);
                if x_end > x_start {
                    cr.move_to(x_start, rect_a.y + rect_a.height);
                    cr.line_to(x_end, rect_a.y + rect_a.height);
                    cr.stroke().ok();
                }
            }

            // Check if they share a vertical edge
            if (rect_a.x + rect_a.width - rect_b.x).abs() < 2.0 {
                let y_start = rect_a.y.max(rect_b.y);
                let y_end = (rect_a.y + rect_a.height).min(rect_b.y + rect_b.height);
                if y_end > y_start {
                    cr.move_to(rect_a.x + rect_a.width, y_start);
                    cr.line_to(rect_a.x + rect_a.width, y_end);
                    cr.stroke().ok();
                }
            }
        }
    }
}

/// Returns the popup's `(x, y, w, h)` if drawn, `None` otherwise. The
/// caller writes the rect into `App.completion_popup_rect` so the
/// click handler can register it on the modal stack (B.5b Stage 5).
///
/// Body is a thin delegator over `quadraui::gtk::draw_completions`
/// (#285). Builds the `quadraui::Completions` description via the
/// shared `render::completion_menu_to_quadraui_completions` adapter,
/// computes the popup placement via `Completions::layout()`, then
/// forwards to the lifted rasteriser through the
/// `quadraui_gtk::draw_completions` shim. Returns the resolved
/// `CompletionsLayout` so the click handler can hit-test items.
pub(super) fn draw_completion_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    line_height: f64,
    char_width: f64,
) -> Option<quadraui::CompletionsLayout> {
    let menu = screen.completion.as_ref()?;
    let active_win = screen
        .windows
        .iter()
        .find(|w| w.window_id == screen.active_window_id)?;
    let (cursor_pos, _) = active_win.cursor.as_ref()?;

    // Anchor: cell below the cursor cell, to the right of the gutter.
    let gutter_width = active_win.gutter_char_width as f64 * char_width;
    let h_scroll_offset = active_win.scroll_left as f64 * char_width;
    let cursor_x =
        active_win.rect.x + gutter_width + cursor_pos.col as f64 * char_width - h_scroll_offset;
    let cursor_y = active_win.rect.y + cursor_pos.view_line as f64 * line_height;

    // Popup width matches the pre-lift bespoke math: longest candidate
    // + 2 cells of padding/border, floored at 100 px.
    let popup_w = ((menu.max_width + 2) as f64 * char_width).max(100.0);
    // Cap visible rows at 10 — same ceiling as the pre-lift code.
    let max_popup_h = 10.0 * line_height;

    let completions = render::completion_menu_to_quadraui_completions(menu);
    let viewport = quadraui::Rect::new(
        active_win.rect.x as f32,
        active_win.rect.y as f32,
        active_win.rect.width as f32,
        active_win.rect.height as f32,
    );
    let line_height_f = line_height as f32;
    let q_layout = completions.layout(
        cursor_x as f32,
        cursor_y as f32,
        line_height_f,
        viewport,
        popup_w as f32,
        max_popup_h as f32,
        |_| quadraui::CompletionItemMeasure::new(line_height_f),
    );

    super::quadraui_gtk::draw_completions(cr, layout, &completions, &q_layout, theme);

    Some(q_layout)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_hover_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    line_height: f64,
    char_width: f64,
    viewport_w: f64,
    viewport_h: f64,
) {
    let Some(hover) = &screen.hover else {
        return;
    };
    let Some(active_win) = screen
        .windows
        .iter()
        .find(|w| w.window_id == screen.active_window_id)
    else {
        return;
    };

    let gutter_width = active_win.gutter_char_width as f64 * char_width;
    let h_scroll_offset = active_win.scroll_left as f64 * char_width;
    let anchor_view_line = hover.anchor_line.saturating_sub(active_win.scroll_top);
    let anchor_x =
        active_win.rect.x + gutter_width + hover.anchor_col as f64 * char_width - h_scroll_offset;
    let anchor_y = active_win.rect.y + anchor_view_line as f64 * line_height;

    let text_lines: Vec<&str> = hover.text.lines().take(20).collect();
    let num_lines = text_lines.len() as f64;
    let max_line_len = text_lines.iter().map(|l| l.len()).max().unwrap_or(10);
    let measured_w = ((max_line_len + 2) as f64 * char_width).max(100.0);
    let measured_h = num_lines * line_height + 4.0;

    let tooltip = quadraui::Tooltip {
        id: quadraui::WidgetId::new("lsp:hover"),
        text: text_lines.join("\n"),
        styled_lines: None,
        placement: quadraui::TooltipPlacement::Top,
        bg: None,
        fg: None,
    };
    let tip_layout = tooltip.layout(
        quadraui::Rect::new(
            anchor_x as f32,
            anchor_y as f32,
            char_width as f32,
            line_height as f32,
        ),
        quadraui::Rect::new(0.0, 0.0, viewport_w as f32, viewport_h as f32),
        quadraui::TooltipMeasure::new(measured_w as f32, measured_h as f32),
        0.0,
    );

    super::quadraui_gtk::draw_tooltip(
        cr,
        layout,
        &tooltip,
        &tip_layout,
        line_height,
        char_width,
        theme,
    );
}

/// Draw the LSP/editor hover popup via the `quadraui::RichTextPopup`
/// primitive. Returns `(popup_bounds, link_rects, scrollbar_hit)` —
/// scrollbar geometry feeds the click + drag handlers in `mod.rs`
/// (#215).
#[allow(clippy::type_complexity)]
pub(super) fn draw_editor_hover_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    line_height: f64,
    char_width: f64,
) -> (
    Option<(f64, f64, f64, f64)>,
    Vec<(f64, f64, f64, f64, String)>,
    Option<render::PopupScrollbarHit>,
) {
    let empty = (None, Vec::new(), None);
    let Some(eh) = screen.editor_hover.as_ref() else {
        return empty;
    };
    let Some(active_win) = screen
        .windows
        .iter()
        .find(|w| w.window_id == screen.active_window_id)
    else {
        return empty;
    };
    if eh.rendered.lines.is_empty() {
        return empty;
    }

    // Anchor in pixels from window origin + frozen scroll.
    let gutter_w = active_win.gutter_char_width as f64 * char_width;
    let anchor_view = eh.anchor_line.saturating_sub(eh.frozen_scroll_top) as f64;
    let vis_col = eh.anchor_col.saturating_sub(eh.frozen_scroll_left) as f64;
    let anchor_x = active_win.rect.x + gutter_w + vis_col * char_width;
    let anchor_y = active_win.rect.y + anchor_view * line_height;

    // Content width — clamp to a comfortable reading width inside the window.
    let da_width = active_win.rect.x + active_win.rect.width;
    let popup_w = ((eh.popup_width + 2) as f64 * char_width)
        .clamp(100.0, (da_width - active_win.rect.x) * 0.9)
        .min(80.0 * char_width);
    let content_w = popup_w - 2.0;

    let popup = render::editor_hover_to_quadraui_rich_text(eh, theme);
    let viewport = quadraui::Rect::new(
        active_win.rect.x as f32,
        active_win.rect.y as f32,
        active_win.rect.width as f32,
        active_win.rect.height as f32,
    );
    let measure = quadraui::RichTextPopupMeasure::new(content_w as f32, line_height as f32);
    let cw = char_width as f32;
    let popup_layout = popup.layout(
        anchor_x as f32,
        anchor_y as f32,
        viewport,
        measure,
        |line_idx, start_byte, end_byte| {
            popup
                .line_text
                .get(line_idx)
                .map(|t| {
                    t[start_byte.min(t.len())..end_byte.min(t.len())]
                        .chars()
                        .count() as f32
                        * cw
                })
                .unwrap_or(0.0)
        },
    );

    let link_rects = super::quadraui_gtk::draw_rich_text_popup(
        cr,
        layout,
        &popup,
        &popup_layout,
        line_height,
        char_width,
        theme,
    );
    let popup_rect = Some((
        popup_layout.bounds.x as f64,
        popup_layout.bounds.y as f64,
        popup_layout.bounds.width as f64,
        popup_layout.bounds.height as f64,
    ));
    // The GTK rasteriser paints the scrollbar wider than the layout's
    // 1px border (so it's actually visible + clickable). Mirror that
    // geometry here so hit-test matches what the user sees (#215).
    let scrollbar_hit = popup_layout.scrollbar.map(|sb| {
        let sb_w = super::quadraui_gtk::RICH_TEXT_POPUP_SB_WIDTH as f32;
        let inset = super::quadraui_gtk::RICH_TEXT_POPUP_SB_INSET as f32;
        let bw = popup_layout.bounds.width;
        let bx = popup_layout.bounds.x;
        let track_x = bx + bw - sb_w - inset;
        // Thumb: rasteriser fills `(sb_x + 1, ..., sb_w - 2, ...)`.
        let thumb_x = track_x + 1.0;
        let thumb_w = (sb_w - 2.0).max(1.0);
        render::PopupScrollbarHit {
            track: quadraui::Rect::new(track_x, sb.track.y, sb_w, sb.track.height),
            thumb: quadraui::Rect::new(thumb_x, sb.thumb.y, thumb_w, sb.thumb.height),
            visible_rows: render::EDITOR_HOVER_MAX_ROWS,
            total: popup.lines.len(),
        }
    });
    (popup_rect, link_rects, scrollbar_hit)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_diff_peek_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    line_height: f64,
    char_width: f64,
    viewport_w: f64,
    viewport_h: f64,
) {
    let Some(peek) = &screen.diff_peek else {
        return;
    };
    let Some(active_win) = screen
        .windows
        .iter()
        .find(|w| w.window_id == screen.active_window_id)
    else {
        return;
    };

    let gutter_width = active_win.gutter_char_width as f64 * char_width;
    let anchor_view_line = peek.anchor_line.saturating_sub(active_win.scroll_top);
    let anchor_x = active_win.rect.x + gutter_width;
    let anchor_y = active_win.rect.y + anchor_view_line as f64 * line_height;

    let visible: Vec<&String> = peek.hunk_lines.iter().take(29).collect();
    let action_text = "[s] Stage  [r] Revert  [q] Close";
    let max_chars = visible
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
        .max(action_text.chars().count());
    let measured_w = ((max_chars + 4) as f64 * char_width).max(200.0);
    let measured_h = (visible.len() + 1) as f64 * line_height + 6.0;

    // Build styled rows: per-line +/- colour for diff content, default fg
    // for context and the action bar. Same colour logic as the legacy
    // renderer; same as the TUI adapter at `render::diff_peek_to_quadraui_tooltip`.
    let added = render::to_quadraui_color(theme.git_added);
    let deleted = render::to_quadraui_color(theme.git_deleted);
    let fg = render::to_quadraui_color(theme.hover_fg);

    let mut styled_lines: Vec<quadraui::StyledText> = Vec::with_capacity(visible.len() + 1);
    for hline in &visible {
        let line_fg = if hline.starts_with('+') {
            added
        } else if hline.starts_with('-') {
            deleted
        } else {
            fg
        };
        styled_lines.push(quadraui::StyledText {
            spans: vec![quadraui::StyledSpan::with_fg(hline.as_str(), line_fg)],
        });
    }
    styled_lines.push(quadraui::StyledText {
        spans: vec![quadraui::StyledSpan::with_fg(action_text, fg)],
    });

    let tooltip = quadraui::Tooltip {
        id: quadraui::WidgetId::new("diff_peek"),
        text: String::new(),
        styled_lines: Some(styled_lines),
        // Legacy diff peek always rendered below the anchor line —
        // mirror that with placement=Bottom (with primitive fallback
        // to Top when there's no room below).
        placement: quadraui::TooltipPlacement::Bottom,
        bg: None,
        fg: None,
    };
    let tip_layout = tooltip.layout(
        quadraui::Rect::new(
            anchor_x as f32,
            anchor_y as f32,
            measured_w as f32,
            line_height as f32,
        ),
        quadraui::Rect::new(0.0, 0.0, viewport_w as f32, viewport_h as f32),
        quadraui::TooltipMeasure::new(measured_w as f32, measured_h as f32),
        0.0,
    );

    super::quadraui_gtk::draw_tooltip(
        cr,
        layout,
        &tooltip,
        &tip_layout,
        line_height,
        char_width,
        theme,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_signature_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    line_height: f64,
    char_width: f64,
    viewport_w: f64,
    viewport_h: f64,
) {
    let Some(sig) = &screen.signature_help else {
        return;
    };
    let Some(active_win) = screen
        .windows
        .iter()
        .find(|w| w.window_id == screen.active_window_id)
    else {
        return;
    };

    let gutter_width = active_win.gutter_char_width as f64 * char_width;
    let h_scroll_offset = active_win.scroll_left as f64 * char_width;
    let anchor_view_line = sig.anchor_line.saturating_sub(active_win.scroll_top);
    let anchor_x =
        active_win.rect.x + gutter_width + sig.anchor_col as f64 * char_width - h_scroll_offset;
    let anchor_y = active_win.rect.y + anchor_view_line as f64 * line_height;

    let measured_w = ((sig.label.len() + 4) as f64 * char_width).max(120.0);
    let measured_h = line_height + 4.0;

    // Build a single styled line: pre-active text, active parameter (in
    // keyword colour), post-active text. The active parameter byte range
    // is in `sig.params[active_param]` and indexes into `sig.label`.
    let kw_color = render::to_quadraui_color(theme.keyword);
    let mut spans: Vec<quadraui::StyledSpan> = Vec::new();
    let active_range = sig
        .active_param
        .and_then(|idx| sig.params.get(idx).copied());
    if let Some((start, end)) = active_range {
        if start < sig.label.len() && end <= sig.label.len() && start < end {
            if start > 0 {
                spans.push(quadraui::StyledSpan::plain(sig.label[..start].to_string()));
            }
            spans.push(quadraui::StyledSpan::with_fg(
                sig.label[start..end].to_string(),
                kw_color,
            ));
            if end < sig.label.len() {
                spans.push(quadraui::StyledSpan::plain(sig.label[end..].to_string()));
            }
        }
    }
    if spans.is_empty() {
        spans.push(quadraui::StyledSpan::plain(sig.label.clone()));
    }
    let styled_line = quadraui::StyledText { spans };

    let tooltip = quadraui::Tooltip {
        id: quadraui::WidgetId::new("lsp:signature"),
        text: String::new(),
        styled_lines: Some(vec![styled_line]),
        placement: quadraui::TooltipPlacement::Top,
        bg: None,
        fg: None,
    };
    let tip_layout = tooltip.layout(
        quadraui::Rect::new(
            anchor_x as f32,
            anchor_y as f32,
            char_width as f32,
            line_height as f32,
        ),
        quadraui::Rect::new(0.0, 0.0, viewport_w as f32, viewport_h as f32),
        quadraui::TooltipMeasure::new(measured_w as f32, measured_h as f32),
        0.0,
    );

    super::quadraui_gtk::draw_tooltip(
        cr,
        layout,
        &tooltip,
        &tip_layout,
        line_height,
        char_width,
        theme,
    );
}

/// Draw the inline find/replace overlay at the top-right of the editor.
///
/// #196: paint layout is driven by `compute_find_replace_hit_regions`
/// — the same cell-unit region list TUI walks in its rasteriser and
/// the click handler in `src/gtk/mod.rs` resolves clicks against.
/// Paint and hit-test derive from the same source of truth, so the
/// toggle-button misalignment bug can't recur by construction.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_find_replace_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    _editor_width: f64,
    _editor_height: f64,
    line_height: f64,
    char_width: f64,
) {
    let Some(panel) = &screen.find_replace else {
        return;
    };
    quadraui::gtk::draw_find_replace(
        cr,
        layout,
        panel,
        &super::quadraui_gtk::q_theme(theme),
        line_height,
        char_width,
    );
}

/// Draw the unified picker modal (supports single-pane and two-pane with preview).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_picker_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    editor_width: f64,
    editor_height: f64,
    line_height: f64,
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
) {
    let Some(picker) = &screen.picker else {
        return;
    };

    let has_preview = picker.preview.is_some();
    let geo = render::PickerGeometry::compute(
        editor_width as f32,
        editor_height as f32,
        has_preview,
        &render::gtk_picker_sizing(line_height as f32),
    );
    let palette = render::picker_panel_to_palette(picker);
    use quadraui::Backend;
    backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
        b.set_current_theme(super::quadraui_gtk::q_theme(theme));
        b.set_current_line_height(line_height);
        b.draw_palette(
            quadraui::Rect::new(geo.popup_x, geo.popup_y, geo.popup_w, geo.popup_h),
            &palette,
        );
    });
}

/// Draw the tab switcher popup (Ctrl+Tab MRU list). Returns the
/// popup's `(x, y, w, h)` if drawn, `None` otherwise — the caller
/// caches this for `ModalStack` registration in the click handler
/// (B.5b Stage 7).
fn draw_tab_switcher_popup_list(
    screen: &render::ScreenLayout,
    editor_width: f64,
    editor_height: f64,
    line_height: f64,
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    cr: &Context,
    layout: &pango::Layout,
) -> Option<(f64, f64, f64, f64)> {
    let ts = screen.tab_switcher.as_ref()?;
    if ts.items.is_empty() {
        return None;
    }

    let max_visible = ((editor_height * 0.6) / line_height) as usize;
    let visible = ts.items.len().min(max_visible).min(20);
    let popup_w = (editor_width * 0.40).clamp(350.0, 600.0);
    let popup_h = (visible as f64 + 1.5) * line_height;
    let popup_x = (editor_width - popup_w) / 2.0;
    let popup_y = (editor_height - popup_h) / 2.0;

    let list = render::tab_switcher_to_quadraui_list_view(ts, visible);
    let q_rect = quadraui::Rect::new(
        popup_x as f32,
        popup_y as f32,
        popup_w as f32,
        popup_h as f32,
    );
    backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
        use quadraui::Backend;
        b.draw_list(q_rect, &list);
    });

    Some((popup_x, popup_y, popup_w, popup_h))
}

/// Draw a modal dialog popup centered on the screen.
///
/// Returns `(btn_rects, popup_rect)` where:
/// - `btn_rects` — `(x, y, w, h)` for each dialog button (same as
///   pre-B5b.13).
/// - `popup_rect` — the dialog box's resolved bounds in DA-local
///   pixels, or `None` if no dialog is being drawn. The click
///   handler caches this in `App.dialog_popup_rect` for `ModalStack`
///   registration; previously it derived bounds from the button
///   rects with a fixed-min-width fudge that overshot the actual
///   popup width on small dialogs (e.g. `:about`), causing
///   click-outside-to-dismiss to mis-fire.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn draw_dialog_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    editor_width: f64,
    editor_height: f64,
    line_height: f64,
) -> (Vec<(f64, f64, f64, f64)>, Option<(f64, f64, f64, f64)>) {
    let Some(panel) = &screen.dialog else {
        return (Vec::new(), None);
    };

    let pango_ctx = pangocairo::create_context(cr);
    let ui_font_desc = FontDescription::from_string(&UI_FONT());
    let ui_layout = pango::Layout::new(&pango_ctx);
    ui_layout.set_font_description(Some(&ui_font_desc));

    // Convert engine-side panel → quadraui::Dialog (synthesises button
    // ids, lifts is_selected → is_default). Same adapter TUI uses.
    let dialog = render::dialog_panel_to_quadraui_dialog(panel);

    // Measure each region in pixels so the primitive's `layout()` can
    // place sub-bounds without owning Pango itself.
    let title_text = dialog
        .title
        .spans
        .iter()
        .map(|s| s.text.as_str())
        .collect::<String>();
    ui_layout.set_text(&title_text);
    let (title_w, _) = ui_layout.pixel_size();

    let body_lines: Vec<String> = dialog
        .body
        .iter()
        .map(|st| st.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();
    let mut body_max_w = 0.0f64;
    for line in &body_lines {
        layout.set_text(line);
        let (w, _) = layout.pixel_size();
        body_max_w = body_max_w.max(w as f64);
    }
    let body_line_count = body_lines.len();

    // Measure horizontal-button widths uniformly (max of formatted
    // labels) so layout's `button_width` is consistent.
    let mut btn_max_w = 0.0f64;
    for btn in &dialog.buttons {
        ui_layout.set_text(&format!("  {}  ", btn.label));
        let (w, _) = ui_layout.pixel_size();
        btn_max_w = btn_max_w.max(w as f64);
    }

    let padding = 12.0;
    let button_gap = 4.0;
    let button_row_height = if dialog.vertical_buttons {
        line_height * dialog.buttons.len() as f64
    } else {
        line_height * 1.5
    };
    let title_height = if title_text.is_empty() {
        0.0
    } else {
        line_height * 1.5
    };
    let body_height = body_line_count as f64 * line_height;
    let input_height = if dialog.input.is_some() {
        line_height + 4.0
    } else {
        0.0
    };

    let total_btns_w = if dialog.vertical_buttons {
        btn_max_w + 24.0
    } else {
        dialog.buttons.len() as f64 * btn_max_w
            + (dialog.buttons.len().saturating_sub(1)) as f64 * button_gap
    };
    let content_w = body_max_w.max(title_w as f64 + 16.0).max(total_btns_w);
    let popup_w = (content_w + padding * 2.0).clamp(350.0, editor_width - 40.0);

    let measure = quadraui::DialogMeasure {
        width: popup_w as f32,
        title_height: title_height as f32,
        body_height: body_height as f32,
        input_height: input_height as f32,
        button_row_height: button_row_height as f32,
        button_width: btn_max_w as f32,
        button_gap: button_gap as f32,
        padding: padding as f32,
    };
    let viewport = quadraui::Rect::new(0.0, 0.0, editor_width as f32, editor_height as f32);
    let dialog_layout = dialog.layout(viewport, measure);

    let popup_rect = (
        dialog_layout.bounds.x as f64,
        dialog_layout.bounds.y as f64,
        dialog_layout.bounds.width as f64,
        dialog_layout.bounds.height as f64,
    );

    let btn_rects =
        super::quadraui_gtk::draw_dialog(cr, layout, &dialog, &dialog_layout, line_height, theme);
    (btn_rects, Some(popup_rect))
}

/// Draw an engine-driven context menu popup on the DrawingArea.
/// Uses the same data as TUI/Win-GUI for visual consistency.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_context_menu_popup(
    cr: &Context,
    _layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    editor_width: f64,
    editor_height: f64,
    char_width: f64,
    line_height: f64,
    _mouse_pos: (f64, f64),
) -> Option<quadraui::ContextMenuLayout> {
    let Some(cm) = &screen.context_menu else {
        return None;
    };
    if cm.items.is_empty() {
        return None;
    }

    let pango_ctx = pangocairo::create_context(cr);
    let ui_font_desc = FontDescription::from_string(&UI_FONT());
    let ui_layout = pango::Layout::new(&pango_ctx);
    ui_layout.set_font_description(Some(&ui_font_desc));

    // Convert engine-side panel → quadraui::ContextMenu (synthesises
    // `context:N` ids, lifts separator_after into separator rows). Same
    // adapter TUI uses.
    let menu = render::context_menu_panel_to_quadraui_context_menu(cm);

    // Each non-separator row is `line_height`; separators get a
    // half-line slot to render as a thin rule.
    // Uniform `line_height` per row (separators included) so the row
    // index a backend mouse-handler computes from `(y / line_height)`
    // matches the row index this layout produces. Separator rendering
    // (a thin rule centred in the row) happens inside `draw_context_menu`.
    let item_height = |_i: usize| quadraui::ContextMenuItemMeasure::new(line_height as f32);

    // Width: budget to fit longest label + longest shortcut + padding.
    let max_label = cm.items.iter().map(|i| i.label.len()).max().unwrap_or(4);
    let max_sc = cm.items.iter().map(|i| i.shortcut.len()).max().unwrap_or(0);
    let content_cols = (max_label + max_sc + 6).clamp(20, 50);
    let menu_w = content_cols as f64 * char_width;

    let anchor_x = cm.screen_col as f64 * char_width;
    let anchor_y = cm.screen_row as f64 * line_height;
    let trigger_height_px = cm.trigger_height as f64 * line_height;
    let viewport = quadraui::Rect::new(0.0, 0.0, editor_width as f32, editor_height as f32);
    let menu_layout = menu.layout_at(
        quadraui::Rect::new(
            anchor_x as f32,
            anchor_y as f32,
            0.0,
            trigger_height_px as f32,
        ),
        viewport,
        menu_w as f32,
        item_height,
    );

    // #435: do NOT override menu.selected_idx from mouse_pos at draw time.
    // The motion handler in `mod.rs` already updates
    // `engine.context_menu.selected` from mouse position on actual motion
    // events; the adapter copies that into `menu.selected_idx` above. Doing
    // a redundant hover override here clobbered keyboard navigation: every
    // queue_draw triggered by `j`/`k` would re-snap selection back to the
    // item under the (stationary) cursor.

    super::quadraui_gtk::draw_context_menu(cr, &ui_layout, &menu, &menu_layout, line_height, theme);

    Some(menu_layout)
}

/// Draw the tab bar for the bottom panel (Terminal / Debug Output) via
/// `quadraui::Backend::draw_tab_bar`. Returns `TabBarHits` for the
/// click handler (caller caches on `engine.bottom_tab_bar_hits`).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_bottom_panel_tabs(
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    cr: &Context,
    layout: &pango::Layout,
    active: &render::BottomPanelKind,
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    line_height: f64,
    has_terminal: bool,
    has_debug_output: bool,
) -> quadraui::TabBarHits {
    use pango::FontDescription;

    let bar = render::build_bottom_panel_tab_bar(active, has_terminal, has_debug_output);

    let saved_font = layout.font_description().unwrap_or_default();
    let ui_font_desc = FontDescription::from_string(&UI_FONT());
    layout.set_font_description(Some(&ui_font_desc));

    use quadraui::Backend;
    let hits = backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
        b.set_current_theme(super::quadraui_gtk::q_theme(theme));
        b.set_current_line_height(line_height);
        b.draw_tab_bar(
            quadraui::Rect::new(x as f32, y as f32, w as f32, line_height as f32),
            &bar,
            None,
        )
    });

    layout.set_font_description(Some(&saved_font));
    hits
}

/// Draw the VSCode-style debug sidebar content.
///
/// Shows four sections stacked vertically:
///   - VARIABLES (with chevron expansion)
///   - WATCH (expressions + values)
///   - CALL STACK (frames, active highlighted)
///   - BREAKPOINTS (file:line list)
///
/// A 2-row header at the top shows the session status and a Run/Stop
/// button.
///
/// Migrated to four `quadraui::TreeView` instances (#281), one per
/// section. Panel header + Run/Stop button + per-section title rows +
/// per-section scrollbar overlays stay panel-specific chrome; item
/// rendering goes through `Backend::draw_tree`.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_debug_sidebar(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    line_height: f64,
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    engine: &crate::core::engine::Engine,
) -> quadraui::StatusBarLayout {
    let sidebar = &screen.debug_sidebar;

    let (bg_r, bg_g, bg_b) = theme.tab_bar_bg.to_cairo();

    // Paint sidebar background.
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    // ── Chrome rows via StatusBar primitive. ──
    let (title_bar, action_bar) = render::debug_sidebar_chrome_to_status_bars(sidebar, theme);
    let title_rect = quadraui::Rect::new(x as f32, y as f32, w as f32, line_height as f32);
    let btn_y = y + line_height;
    let action_rect = quadraui::Rect::new(x as f32, btn_y as f32, w as f32, line_height as f32);
    let action_hits;
    {
        use quadraui::Backend;
        action_hits = backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
            b.set_current_theme(super::quadraui_gtk::q_theme(theme));
            b.set_current_line_height(line_height);
            let _ = b.draw_status_bar(title_rect, &title_bar, None, None);
            b.draw_status_bar(action_rect, &action_bar, None, None)
        });
    }

    // ── SidebarSystem body (the four sections). ──
    let msv_y = btn_y + line_height;
    let msv_h = (h - 2.0 * line_height).max(0.0);
    if msv_h > 0.0 {
        let body_rect = quadraui::Rect::new(x as f32, msv_y as f32, w as f32, msv_h as f32);
        engine.dap_sidebar_body_rect.set(body_rect);
        backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
            b.set_current_theme(super::quadraui_gtk::q_theme(theme));
            b.set_current_line_height(line_height);
            engine.dap_sidebar_system.borrow().render(b, body_rect);
        });
    }
    action_hits
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_quickfix_panel(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    editor_x: f64,
    editor_y: f64,
    editor_w: f64,
    qf_px: f64,
    line_height: f64,
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
) {
    let Some(qf) = &screen.quickfix else {
        return;
    };

    // Phase A.5b migration: quickfix now renders through the shared
    // `quadraui::ListView` primitive. The adapter produces a ListView
    // with a `QUICKFIX (N items)` header; `Backend::draw_list`
    // renders header + rows with selection indicator.
    //
    // Phase B.5 Stage 3 pilot: this is the first GTK draw site routed
    // through the `quadraui::Backend` trait — `enter_frame_scope`
    // stashes the cairo context + pango layout for the closure
    // duration; `b.draw_list(rect, &list)` reaches them via the
    // backend's frame-scope accessor. The same generic
    // `paint::<B: Backend>` shape now drives both backends.
    //
    // Scroll-to-selection: reserve one row for the header, then keep the
    // selected item within the remaining visible rows. Matches prior
    // GTK behaviour.
    let visible_rows = ((qf_px / line_height) as usize).saturating_sub(1);
    let scroll_top = if visible_rows == 0 {
        0
    } else {
        (qf.selected_idx + 1).saturating_sub(visible_rows)
    };
    let mut list = render::quickfix_to_list_view(qf);
    list.scroll_offset = scroll_top;

    use quadraui::Backend;
    backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
        b.set_current_theme(super::quadraui_gtk::q_theme(theme));
        b.set_current_line_height(line_height);
        b.draw_list(
            quadraui::Rect::new(
                editor_x as f32,
                editor_y as f32,
                editor_w as f32,
                qf_px as f32,
            ),
            &list,
        );
    });
}

/// Draw the terminal toolbar row (find bar or tab strip) through
/// quadraui primitives. Returns cached hit data for click dispatch.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_terminal_toolbar(
    backend: &std::rc::Rc<std::cell::RefCell<super::backend::GtkBackend>>,
    cr: &Context,
    layout: &pango::Layout,
    panel: &render::TerminalPanel,
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    line_height: f64,
) -> crate::core::engine::TerminalToolbarHits {
    use crate::core::engine::TerminalToolbarHits;
    use pango::FontDescription;

    let toolbar = render::build_terminal_toolbar(panel, theme);

    let saved_font = layout.font_description().unwrap_or_default();
    let ui_font_desc = FontDescription::from_string(&UI_FONT());
    layout.set_font_description(Some(&ui_font_desc));

    let result = match toolbar {
        render::TerminalToolbar::FindBar(bar) => {
            use quadraui::Backend;
            let q_rect = quadraui::Rect::new(x as f32, y as f32, w as f32, line_height as f32);
            backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
                b.set_current_theme(super::quadraui_gtk::q_theme(theme));
                b.set_current_line_height(line_height);
                b.draw_status_bar(q_rect, &bar, None, None);
            });
            let sb_layout = bar.layout(w as f32, line_height as f32, 16.0, |seg| {
                layout.set_text(&seg.text);
                quadraui::StatusSegmentMeasure::new(layout.pixel_size().0.max(0) as f32)
            });
            TerminalToolbarHits::FindBar {
                layout: sb_layout,
                origin_x: x,
            }
        }
        render::TerminalToolbar::TabStrip(bar) => {
            use quadraui::Backend;
            let q_rect = quadraui::Rect::new(x as f32, y as f32, w as f32, line_height as f32);
            let hits = backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
                b.set_current_theme(super::quadraui_gtk::q_theme(theme));
                b.set_current_line_height(line_height);
                b.draw_tab_bar(q_rect, &bar, None)
            });
            TerminalToolbarHits::TabStrip(hits)
        }
    };

    layout.set_font_description(Some(&saved_font));
    result
}

/// Draw a per-window status bar with styled segments.
#[allow(clippy::too_many_arguments)]
/// Render a per-window / separated status bar row (A.6b).
///
/// Routes through `Backend::draw_status_bar` via the
/// `window_status_line_to_status_bar` adapter. `StatusAction` is decoded from
/// the primitive's `WidgetId`s so the existing per-window
/// `status_segment_map` stays on `StatusAction` and the click handler in
/// `src/gtk/click.rs` is unchanged.
fn draw_window_status_bar(
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    cr: &Context,
    layout: &pango::Layout,
    theme: &Theme,
    status: &render::WindowStatusLine,
    x: f64,
    y: f64,
    width: f64,
    line_height: f64,
    segment_zones: &mut Vec<(f64, f64, crate::core::engine::StatusAction)>,
) {
    let bar =
        render::window_status_line_to_status_bar(status, quadraui::WidgetId::new("status:window"));
    use quadraui::Backend;
    let bar_layout = backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
        b.set_current_theme(super::quadraui_gtk::q_theme(theme));
        b.set_current_line_height(line_height);
        b.draw_status_bar(
            quadraui::Rect::new(x as f32, y as f32, width as f32, line_height as f32),
            &bar,
            None,
            None,
        )
    });

    segment_zones.clear();
    for (rect, hit) in &bar_layout.hit_regions {
        if let quadraui::StatusBarHit::Segment(ref id) = hit {
            if let Some(action) = render::status_action_from_id(id.as_str()) {
                let start = rect.x as f64;
                let end = start + rect.width as f64;
                segment_zones.push((start, end, action));
            }
        }
    }
}

pub(super) fn draw_command_line(
    cr: &Context,
    layout: &pango::Layout,
    theme: &Theme,
    cmd: &CommandLineData,
    width: f64,
    y: f64,
    line_height: f64,
) {
    let (br, bg, bb) = theme.command_bg.to_cairo();
    cr.set_source_rgb(br, bg, bb);
    cr.rectangle(0.0, y, width, line_height);
    cr.fill().ok();

    if !cmd.text.is_empty() {
        layout.set_text(&cmd.text);
        layout.set_attributes(None);

        let (fr, fg, fb) = theme.command_fg.to_cairo();
        cr.set_source_rgb(fr, fg, fb);

        if cmd.right_align {
            let (text_w, _) = layout.pixel_size();
            cr.move_to(width - text_w as f64, y);
        } else {
            cr.move_to(0.0, y);
        }
        pangocairo::show_layout(cr, layout);
    }

    // Command-line insert cursor
    if cmd.show_cursor {
        layout.set_text(&cmd.cursor_anchor_text);
        layout.set_attributes(None);
        let (text_w, _) = layout.pixel_size();
        let (cr_r, cr_g, cr_b) = theme.cursor.to_cairo();
        cr.set_source_rgb(cr_r, cr_g, cr_b);
        cr.rectangle(text_w as f64, y, 2.0, line_height);
        cr.fill().ok();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_source_control_panel(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    line_height: f64,
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    engine: &Engine,
) {
    let Some(ref sc) = screen.source_control else {
        return;
    };

    // Draw hint bar at bottom when focused.
    let h = if sc.has_focus && h > line_height * 3.0 {
        let hint_y = y + h - line_height;
        let (hdr_r, hdr_g, hdr_b) = theme.status_bg.to_cairo();
        cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
        cr.rectangle(x, hint_y, w, line_height);
        cr.fill().ok();
        let hint_text = " Press '?' for help";
        let (dim_r, dim_g, dim_b) = theme.line_number_fg.to_cairo();
        cr.set_source_rgb(dim_r, dim_g, dim_b);
        layout.set_text(hint_text);
        layout.set_attributes(None);
        let (_, lh) = layout.pixel_size();
        cr.move_to(x + 2.0, hint_y + (line_height - lh as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
        h - line_height
    } else {
        h
    };

    let (bg_r, bg_g, bg_b) = theme.tab_bar_bg.to_cairo();
    let (hdr_r, hdr_g, hdr_b) = theme.status_bg.to_cairo();
    let (hdr_fg_r, hdr_fg_g, hdr_fg_b) = theme.status_fg.to_cairo();
    let (fg_r, fg_g, fg_b) = theme.foreground.to_cairo();
    let (dim_r, dim_g, dim_b) = theme.line_number_fg.to_cairo();
    let (add_r, add_g, add_b) = theme.diff_added_bg.to_cairo();
    let (del_r, del_g, del_b) = theme.diff_removed_bg.to_cairo();

    // Background
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    layout.set_attributes(None);

    let mut row: usize = 0;

    // ── Row 0: header "SOURCE CONTROL" ──────────────────────────────────────
    cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
    cr.rectangle(x, y + row as f64 * line_height, w, line_height);
    cr.fill().ok();

    let branch_str = format!(
        "  {} SOURCE CONTROL   {}  ↑{}↓{}",
        icons::GIT_BRANCH.nerd,
        sc.branch,
        sc.ahead,
        sc.behind
    );
    cr.set_source_rgb(hdr_fg_r, hdr_fg_g, hdr_fg_b);
    layout.set_text(&branch_str);
    let (lw, lh) = layout.pixel_size();
    cr.move_to(
        x + 2.0,
        y + row as f64 * line_height + (line_height - lh as f64) / 2.0,
    );
    pangocairo::show_layout(cr, layout);
    let _ = (lw, lh);
    row += 1;

    // Vertical gap after header.
    let gap = (line_height * 0.3).round();

    // ── Row 1+: commit input row(s) ─────────────────────────────────────────
    // Use float y_commit to track position with gaps.
    let mut y_commit = y + row as f64 * line_height + gap;
    if y_commit < y + h {
        let lines: Vec<&str> = sc.commit_message.split('\n').collect();
        let commit_rows = lines.len().max(1);
        let commit_h = commit_rows as f64 * line_height;
        let (inp_bg_r, inp_bg_g, inp_bg_b) = if sc.commit_input_active {
            theme.fuzzy_selected_bg.to_cairo()
        } else {
            theme.completion_bg.to_cairo()
        };
        // Draw background for all commit input rows with horizontal margin.
        let margin = 4.0;
        cr.set_source_rgb(inp_bg_r, inp_bg_g, inp_bg_b);
        cr.rectangle(x + margin, y_commit, w - margin * 2.0, commit_h);
        cr.fill().ok();

        let (prompt_r, prompt_g, prompt_b) = if sc.commit_input_active {
            (fg_r, fg_g, fg_b)
        } else {
            (dim_r, dim_g, dim_b)
        };
        cr.set_source_rgb(prompt_r, prompt_g, prompt_b);

        // Compute cursor line/col for active input.
        let (cursor_line, cursor_col) = if sc.commit_input_active {
            let before_cursor = &sc.commit_message[..sc.commit_cursor.min(sc.commit_message.len())];
            let cl = before_cursor.matches('\n').count();
            let line_start = before_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
            (cl, before_cursor[line_start..].chars().count())
        } else {
            (0, 0)
        };
        let prefix_s = format!(" {}  ", icons::GIT_EDIT.nerd);
        let prefix = prefix_s.as_str();
        let pad_str = "    "; // 4 spaces — same visual width as prefix

        if sc.commit_message.is_empty() && !sc.commit_input_active {
            let prompt = format!("{}Message (press c to type)", prefix);
            layout.set_text(&prompt);
            let (_, lh2) = layout.pixel_size();
            cr.move_to(
                x + margin + 2.0,
                y_commit + (line_height - lh2 as f64) / 2.0,
            );
            pangocairo::show_layout(cr, layout);
        } else {
            for (i, line) in lines.iter().enumerate() {
                let pfx = if i == 0 { prefix } else { pad_str };
                let text = format!("{}{}", pfx, line);
                layout.set_text(&text);
                let (_, lh2) = layout.pixel_size();
                let row_y = y_commit + i as f64 * line_height + (line_height - lh2 as f64) / 2.0;
                cr.move_to(x + margin + 2.0, row_y);
                pangocairo::show_layout(cr, layout);

                // Draw beam cursor (thin vertical line) at cursor position.
                if sc.commit_input_active && i == cursor_line {
                    let pfx_len = pfx.chars().count();
                    let before_cursor_text: String =
                        text.chars().take(pfx_len + cursor_col).collect();
                    layout.set_text(&before_cursor_text);
                    let (cursor_px, _) = layout.pixel_size();
                    cr.set_source_rgb(fg_r, fg_g, fg_b);
                    cr.rectangle(
                        x + margin + 2.0 + cursor_px as f64,
                        y_commit + i as f64 * line_height,
                        1.5,
                        line_height,
                    );
                    cr.fill().ok();
                    cr.set_source_rgb(prompt_r, prompt_g, prompt_b);
                }
            }
        }
        y_commit += commit_h;
    }

    // ── Action buttons (with padding above and below) ──────────────────────
    {
        let btn_pad = gap; // same gap as after header
        let btn_y_base = y_commit + btn_pad;
        let btn_h = line_height;
        let margin = 4.0;
        let btn_x = x + margin;
        let btn_w = w - margin * 2.0;

        // Commit gets ~50% of the width (with label text).
        // Push / Pull / Sync get equal shares of the remaining width, icon only.
        let commit_w = btn_w / 2.0;
        let remain_w = btn_w - commit_w;
        let icon_w = remain_w / 3.0;

        // Button background color (slightly contrasting).
        let (btn_bg_r, btn_bg_g, btn_bg_b) = theme.status_bg.to_cairo();
        // Hover: lighten the button bg slightly.
        let lighten = |c: f64| (c + 0.08).min(1.0);
        let (hover_bg_r, hover_bg_g, hover_bg_b) =
            (lighten(btn_bg_r), lighten(btn_bg_g), lighten(btn_bg_b));

        // Helper: fill and label one button segment.
        let draw_btn = |bx: f64, seg_w: f64, text: &str, focused: bool, hovered: bool| {
            let (fill_r, fill_g, fill_b) = if focused {
                (hdr_r, hdr_g, hdr_b)
            } else if hovered {
                (hover_bg_r, hover_bg_g, hover_bg_b)
            } else {
                (btn_bg_r, btn_bg_g, btn_bg_b)
            };
            cr.set_source_rgb(fill_r, fill_g, fill_b);
            cr.rectangle(bx, btn_y_base, seg_w, btn_h);
            cr.fill().ok();
            cr.set_source_rgb(hdr_fg_r, hdr_fg_g, hdr_fg_b);
            layout.set_text(text);
            let (_, lh_btn) = layout.pixel_size();
            cr.move_to(bx + 2.0, btn_y_base + (btn_h - lh_btn as f64) / 2.0);
            pangocairo::show_layout(cr, layout);
        };

        let commit_lbl = format!(" {} Commit", icons::GIT_COMMIT.nerd);
        let push_lbl = format!(" {}", icons::GIT_PUSH.nerd);
        let pull_lbl = format!(" {}", icons::GIT_PULL.nerd);
        let sync_lbl = format!(" {}", icons::GIT_SYNC.nerd);
        for (i, (bx, bw, label)) in [
            (btn_x, commit_w, commit_lbl.as_str()),
            (btn_x + commit_w, icon_w, push_lbl.as_str()),
            (btn_x + commit_w + icon_w, icon_w, pull_lbl.as_str()),
            (
                btn_x + commit_w + icon_w * 2.0,
                btn_w - (commit_w + icon_w * 2.0),
                sync_lbl.as_str(),
            ),
        ]
        .iter()
        .enumerate()
        {
            draw_btn(
                *bx,
                *bw,
                label,
                sc.button_focused == Some(i),
                sc.button_hovered == Some(i),
            );
        }

        y_commit = btn_y_base + btn_h + btn_pad;
    }

    // Section rendering — migrated to SidebarSystem (#321).
    let _ = (add_r, add_g, add_b, del_r, del_g, del_b);
    let sections_h = (y + h - y_commit).max(0.0);
    let body_rect = quadraui::Rect::new(x as f32, y_commit as f32, w as f32, sections_h as f32);
    engine.sc_sidebar_body_rect.set(body_rect);
    render::populate_sc_sidebar_system(engine, theme);
    {
        backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
            b.set_current_theme(super::quadraui_gtk::q_theme(theme));
            b.set_current_line_height(line_height);
            engine.sc_sidebar_system.borrow().render(b, body_rect);
        });
    }

    // ── Branch picker / create overlay ───────────────────────────────────────
    if let Some(ref bp) = sc.branch_picker {
        let popup_w = w.min(300.0);
        let popup_h = if bp.create_mode {
            line_height * 3.0
        } else {
            (line_height * (bp.results.len() as f64 + 3.0)).min(h - line_height * 2.0)
        };
        let popup_x = x + (w - popup_w) / 2.0;
        let popup_y = y + line_height * 2.0;

        // Background
        let (r, g, b) = theme.completion_bg.to_cairo();
        cr.set_source_rgb(r, g, b);
        cr.rectangle(popup_x, popup_y, popup_w, popup_h);
        cr.fill().ok();
        // Border
        let (r, g, b) = theme.completion_border.to_cairo();
        cr.set_source_rgb(r, g, b);
        cr.set_line_width(1.0);
        cr.rectangle(popup_x, popup_y, popup_w, popup_h);
        cr.stroke().ok();

        // Title
        let title = if bp.create_mode {
            "New Branch"
        } else {
            "Switch Branch"
        };
        let (r, g, b) = theme.completion_fg.to_cairo();
        cr.set_source_rgb(r, g, b);
        layout.set_text(title);
        layout.set_attributes(None);
        cr.move_to(popup_x + 8.0, popup_y);
        pangocairo::show_layout(cr, layout);

        if bp.create_mode {
            let input_text = format!("Name: {}▏", bp.create_input);
            layout.set_text(&input_text);
            cr.move_to(popup_x + 8.0, popup_y + line_height);
            pangocairo::show_layout(cr, layout);
        } else {
            // Query row
            let query_text = format!("{} {}", icons::SEARCH.nerd, bp.query);
            let (r, g, b) = theme.completion_fg.to_cairo();
            cr.set_source_rgb(r, g, b);
            layout.set_text(&query_text);
            layout.set_attributes(None);
            cr.move_to(popup_x + 8.0, popup_y + line_height);
            pangocairo::show_layout(cr, layout);

            // Branch list
            for (i, (name, is_current)) in bp.results.iter().enumerate() {
                let ry = popup_y + line_height * (i as f64 + 2.0);
                if ry + line_height > popup_y + popup_h {
                    break;
                }
                // Selection highlight
                if i == bp.selected {
                    let (r, g, b) = theme.completion_selected_bg.to_cairo();
                    cr.set_source_rgb(r, g, b);
                    cr.rectangle(popup_x + 1.0, ry, popup_w - 2.0, line_height);
                    cr.fill().ok();
                }
                let marker = if *is_current { "● " } else { "  " };
                let display = format!("{marker}{name}");
                let (r, g, b) = theme.completion_fg.to_cairo();
                cr.set_source_rgb(r, g, b);
                layout.set_text(&display);
                layout.set_attributes(None);
                cr.move_to(popup_x + 8.0, ry);
                pangocairo::show_layout(cr, layout);
            }
        }
    }

    // ── Help dialog overlay ──────────────────────────────────────────────────
    if sc.help_open {
        let bindings: &[(&str, &str)] = &[
            ("j/k", "Navigate"),
            ("s", "Stage / unstage"),
            ("S", "Stage all"),
            ("d", "Discard file"),
            ("D", "Discard all unstaged"),
            ("c", "Commit message"),
            ("b", "Switch branch"),
            ("B", "Create branch"),
            ("p", "Push"),
            ("P", "Pull"),
            ("f", "Fetch"),
            ("r", "Refresh"),
            ("Tab", "Expand / collapse"),
            ("Enter", "Open file"),
            ("q/Esc", "Close panel"),
        ];
        let popup_w = w.min(280.0);
        let popup_h = line_height * (bindings.len() as f64 + 2.0);
        let popup_x = x + (w - popup_w) / 2.0;
        let popup_y = y + (h - popup_h) / 2.0;

        let (r, g, b) = theme.completion_bg.to_cairo();
        cr.set_source_rgb(r, g, b);
        cr.rectangle(popup_x, popup_y, popup_w, popup_h);
        cr.fill().ok();
        let (r, g, b) = theme.completion_border.to_cairo();
        cr.set_source_rgb(r, g, b);
        cr.set_line_width(1.0);
        cr.rectangle(popup_x, popup_y, popup_w, popup_h);
        cr.stroke().ok();

        // Title + close hint
        let (r, g, b) = theme.completion_fg.to_cairo();
        cr.set_source_rgb(r, g, b);
        layout.set_text("Keybindings");
        layout.set_attributes(None);
        cr.move_to(popup_x + 8.0, popup_y);
        pangocairo::show_layout(cr, layout);

        layout.set_text("x");
        cr.move_to(popup_x + popup_w - 16.0, popup_y);
        pangocairo::show_layout(cr, layout);

        // Bindings
        for (i, (key, desc)) in bindings.iter().enumerate() {
            let ry = popup_y + line_height * (i as f64 + 1.0);
            let (r, g, b) = theme.function.to_cairo();
            cr.set_source_rgb(r, g, b);
            layout.set_text(key);
            layout.set_attributes(None);
            cr.move_to(popup_x + 12.0, ry);
            pangocairo::show_layout(cr, layout);

            let (r, g, b) = theme.completion_fg.to_cairo();
            cr.set_source_rgb(r, g, b);
            layout.set_text(desc);
            layout.set_attributes(None);
            cr.move_to(popup_x + 100.0, ry);
            pangocairo::show_layout(cr, layout);
        }
    }
}

// ─── Settings sidebar panel ───────────────────────────────────────────────────

/// Phase A.3c-2: settings panel renders into a `DrawingArea` via the
/// shared `quadraui::Form` primitive. Layout from top to bottom:
///   - Row 0: header bar (status-bar styling, "  SETTINGS")
///   - Row 1: search input row (`/ <query>` with cursor when active)
///   - Body: scrollable form (`quadraui_gtk::draw_form`) + scrollbar column
///   - Bottom row: "Open settings.json" footer button (status-bar styling)
///
/// **Geometry contract:** the click handler in
/// `App::handle_settings_msg` mirrors these row positions exactly
/// (header @ 0..line_height, search @ line_height..2*line_height, body
/// rows of `(line_height * 1.4).round()` starting at `2*line_height`,
/// footer at `panel_h - line_height`). Update both sites together.
///
/// Inline-edit mode (Integer / String) overlays the editing row's value
/// plus cursor on top of the form rendering, since `settings_to_form`
/// does not yet emit `TextInput` for active-edit fields (tracked as a
/// future adapter refinement; matches the TUI fallback in `panels.rs`).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_settings_panel(
    cr: &Context,
    layout: &pango::Layout,
    engine: &Engine,
    theme: &Theme,
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    line_height: f64,
) {
    use crate::core::engine::SettingsRow;
    use crate::core::settings::SETTING_DEFS;

    if w <= 0.0 || h <= 0.0 {
        return;
    }

    // Geometry — keep in sync with `App::handle_settings_msg`.
    let row_h = (line_height * 1.4).round();
    let footer_h = line_height;

    let (bg_r, bg_g, bg_b) = theme.tab_bar_bg.to_cairo();
    let (hdr_r, hdr_g, hdr_b) = theme.status_bg.to_cairo();
    let (hdr_fg_r, hdr_fg_g, hdr_fg_b) = theme.status_fg.to_cairo();
    let (fg_r, fg_g, fg_b) = theme.foreground.to_cairo();
    let (dim_r, dim_g, dim_b) = theme.line_number_fg.to_cairo();
    let (sel_r, sel_g, sel_b) = theme.fuzzy_selected_bg.to_cairo();
    let (accent_r, accent_g, accent_b) = theme.cursor.to_cairo();

    // Background.
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();
    layout.set_attributes(None);

    // ── Rows 0–1: header + search input chrome ──────────────────────────────
    quadraui::gtk::draw_settings_chrome(
        cr,
        layout,
        x,
        y,
        w,
        line_height,
        "  SETTINGS",
        &engine.settings_query,
        "Search settings…",
        engine.settings_input_active,
        &super::quadraui_gtk::q_theme(theme),
    );
    let (_, header_lh) = layout.pixel_size();

    // ── Body: form + scrollbar; bottom row reserved for footer ──────────────
    let body_y = y + line_height * 2.0;
    let body_h = (y + h - body_y - footer_h).max(0.0);
    if body_h <= 0.0 {
        return;
    }

    let total = engine.settings_flat_list().len();
    let visible_rows = (body_h / row_h).floor() as usize;
    let need_sb = visible_rows > 0 && total > visible_rows;
    let sb_w = if need_sb { 8.0 } else { 0.0 };
    let form_w = (w - sb_w).max(0.0);

    // Form + scrollbar via FormController.
    render::populate_settings_form_controller(engine);
    {
        let q_rect = quadraui::Rect::new(x as f32, body_y as f32, w as f32, body_h as f32);
        backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
            b.set_current_theme(super::quadraui_gtk::q_theme(theme));
            b.set_current_line_height(line_height);
            engine
                .settings_form_controller
                .borrow_mut()
                .render_and_cache(b, q_rect);
        });
    }

    // ── Inline-edit overlay ─────────────────────────────────────────────────
    let editing_idx: Option<usize> = if let Some(def_idx) = engine.settings_editing {
        engine
            .settings_flat_list()
            .iter()
            .position(|r| matches!(r, SettingsRow::CoreSetting(i) if *i == def_idx))
    } else if let Some((ext_name, ext_key)) = engine.ext_settings_editing.clone() {
        engine.settings_flat_list().iter().position(
            |r| matches!(r, SettingsRow::ExtSetting(en, ek) if en == &ext_name && ek == &ext_key),
        )
    } else {
        None
    };
    if let Some(flat_idx) = editing_idx {
        let scroll = engine.settings_scroll_top;
        if flat_idx >= scroll {
            let local = flat_idx - scroll;
            let ey = body_y + local as f64 * row_h;
            if ey + row_h <= body_y + body_h {
                let buf = &engine.settings_edit_buf;
                let value_max_w = (form_w * 0.6).max(80.0);
                let input_right = x + form_w - 8.0;

                // Highlight the row.
                cr.set_source_rgb(sel_r, sel_g, sel_b);
                cr.rectangle(x, ey, form_w, row_h);
                cr.fill().ok();

                // Re-draw label.
                let flat = engine.settings_flat_list();
                let label_text = match &flat[flat_idx] {
                    SettingsRow::CoreSetting(i) => SETTING_DEFS[*i].label.to_string(),
                    SettingsRow::ExtSetting(_, k) => k.clone(),
                    _ => String::new(),
                };
                cr.set_source_rgb(fg_r, fg_g, fg_b);
                layout.set_text(&label_text);
                let (_, lh2) = layout.pixel_size();
                cr.move_to(x + 6.0, ey + (row_h - lh2 as f64) / 2.0);
                pangocairo::show_layout(cr, layout);

                // Bracketed editable value with cursor at end of buffer.
                layout.set_text(buf);
                let (bw, _) = layout.pixel_size();
                let draw_w = (bw as f64).min(value_max_w);
                let ix = input_right - draw_w - 14.0;

                cr.set_source_rgb(dim_r, dim_g, dim_b);
                layout.set_text("[");
                cr.move_to(ix, ey + (row_h - lh2 as f64) / 2.0);
                pangocairo::show_layout(cr, layout);

                cr.set_source_rgb(fg_r, fg_g, fg_b);
                layout.set_text(buf);
                cr.move_to(ix + 8.0, ey + (row_h - lh2 as f64) / 2.0);
                pangocairo::show_layout(cr, layout);

                cr.set_source_rgb(dim_r, dim_g, dim_b);
                layout.set_text("]");
                cr.move_to(ix + 8.0 + draw_w + 2.0, ey + (row_h - lh2 as f64) / 2.0);
                pangocairo::show_layout(cr, layout);

                // Cursor at end of buf.
                cr.set_source_rgb(accent_r, accent_g, accent_b);
                cr.rectangle(ix + 8.0 + bw as f64, ey + 3.0, 1.5, row_h - 6.0);
                cr.fill().ok();
            }
        }
    }

    // ── Footer: "Open settings.json" ───────────────────────────────────────
    let footer_y = y + h - footer_h;
    cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
    cr.rectangle(x, footer_y, w, footer_h);
    cr.fill().ok();
    cr.set_source_rgb(hdr_fg_r, hdr_fg_g, hdr_fg_b);
    layout.set_text("  Open settings.json");
    cr.move_to(x + 4.0, footer_y + (footer_h - header_lh as f64) / 2.0);
    pangocairo::show_layout(cr, layout);
}

// ─── Extension-provided panel (e.g. git-insights GIT LOG) ─────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_ext_dyn_panel(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    line_height: f64,
) {
    use crate::core::plugin::ExtPanelStyle;

    let Some(ref panel) = screen.ext_panel else {
        return;
    };

    let (bg_r, bg_g, bg_b) = theme.tab_bar_bg.to_cairo();
    let (hdr_r, hdr_g, hdr_b) = theme.status_bg.to_cairo();
    let (hdr_fg_r, hdr_fg_g, hdr_fg_b) = theme.status_fg.to_cairo();
    let (fg_r, fg_g, fg_b) = theme.foreground.to_cairo();
    let (item_r, item_g, item_b) = theme.foreground.to_cairo();
    let (dim_r, dim_g, dim_b) = theme.line_number_fg.to_cairo();
    let (accent_r, accent_g, accent_b) = theme.keyword.to_cairo();
    let (sel_r, sel_g, sel_b) = theme.fuzzy_selected_bg.to_cairo();

    // Background
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    layout.set_attributes(None);
    let mut ry: f64 = 0.0;

    // ── Row 0: panel header ─────────────────────────────────────────────────
    cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
    cr.rectangle(x, y + ry, w, line_height);
    cr.fill().ok();
    let hdr_text = format!("  {}", panel.title);
    cr.set_source_rgb(hdr_fg_r, hdr_fg_g, hdr_fg_b);
    layout.set_text(&hdr_text);
    let (_, lh) = layout.pixel_size();
    cr.move_to(x + 2.0, y + ry + (line_height - lh as f64) / 2.0);
    pangocairo::show_layout(cr, layout);
    ry += line_height;

    if ry >= h {
        return;
    }

    // ── Search input row (when active or has text) ──────────────────────────
    if panel.input_active || !panel.input_text.is_empty() {
        cr.set_source_rgb(bg_r, bg_g, bg_b);
        cr.rectangle(x, y + ry, w, line_height);
        cr.fill().ok();
        cr.set_source_rgb(dim_r, dim_g, dim_b);
        layout.set_text(" / ");
        cr.move_to(x, y + ry);
        pangocairo::show_layout(cr, layout);
        let (prefix_w, _) = layout.pixel_size();
        if panel.input_active {
            cr.set_source_rgb(item_r, item_g, item_b);
        } else {
            cr.set_source_rgb(dim_r, dim_g, dim_b);
        }
        layout.set_text(&panel.input_text);
        cr.move_to(x + prefix_w as f64, y + ry);
        pangocairo::show_layout(cr, layout);
        if panel.input_active {
            let (tw, _) = layout.pixel_size();
            cr.set_source_rgb(item_r, item_g, item_b);
            cr.rectangle(x + prefix_w as f64 + tw as f64, y + ry, 1.5, line_height);
            cr.fill().ok();
        }
        ry += line_height;
        if ry >= h {
            return;
        }
    }

    // ── Build flat list of rows ─────────────────────────────────────────────
    struct FlatRow {
        text: String,
        hint: String,
        is_header: bool,
        style: ExtPanelStyle,
        is_separator: bool,
        badges: Vec<crate::core::plugin::ExtPanelBadge>,
        actions: Vec<crate::core::plugin::ExtPanelAction>,
    }
    let mut flat_rows: Vec<FlatRow> = Vec::new();
    for section in &panel.sections {
        let arrow = if section.expanded { "▼" } else { "▶" };
        flat_rows.push(FlatRow {
            text: format!(" {} {}", arrow, section.name),
            hint: String::new(),
            is_header: true,
            style: ExtPanelStyle::Header,
            is_separator: false,
            badges: Vec::new(),
            actions: Vec::new(),
        });
        if section.expanded {
            for item in &section.items {
                if item.is_separator {
                    flat_rows.push(FlatRow {
                        text: String::new(),
                        hint: String::new(),
                        is_header: false,
                        style: ExtPanelStyle::Dim,
                        is_separator: true,
                        badges: Vec::new(),
                        actions: Vec::new(),
                    });
                    continue;
                }
                let indent = "  ".repeat(item.indent as usize + 1);
                let chevron = if item.expandable {
                    if item.expanded {
                        "▼ "
                    } else {
                        "▶ "
                    }
                } else {
                    ""
                };
                let icon_part = if item.icon.is_empty() {
                    String::new()
                } else {
                    format!("{} ", item.icon)
                };
                flat_rows.push(FlatRow {
                    text: format!("{}{}{}{}", indent, chevron, icon_part, item.text),
                    hint: item.hint.clone(),
                    style: item.style,
                    is_header: false,
                    is_separator: false,
                    badges: item.badges.clone(),
                    actions: item.actions.clone(),
                });
            }
        }
    }

    // ── Render visible rows with scroll offset ──────────────────────────────
    let content_h = h - ry;
    let max_rows = (content_h / line_height) as usize;
    let scroll = panel.scroll_top;
    let visible = &flat_rows[scroll.min(flat_rows.len())..];

    for (ri, row) in visible.iter().enumerate().take(max_rows) {
        let row_y = y + ry + ri as f64 * line_height;
        let is_sel = (scroll + ri) == panel.selected;

        // Separator: horizontal line
        if row.is_separator {
            let sep_y = row_y + line_height / 2.0;
            cr.set_source_rgb(dim_r, dim_g, dim_b);
            cr.set_line_width(1.0);
            cr.move_to(x + 8.0, sep_y);
            cr.line_to(x + w - 8.0, sep_y);
            cr.stroke().ok();
            continue;
        }

        // Selection highlight
        if is_sel && panel.has_focus {
            cr.set_source_rgb(sel_r, sel_g, sel_b);
            cr.rectangle(x, row_y, w, line_height);
            cr.fill().ok();
        }

        // Choose foreground color based on style
        let (text_r, text_g, text_b) = if row.is_header {
            (fg_r, fg_g, fg_b)
        } else {
            match row.style {
                ExtPanelStyle::Header => (fg_r, fg_g, fg_b),
                ExtPanelStyle::Dim => (dim_r, dim_g, dim_b),
                ExtPanelStyle::Accent => (accent_r, accent_g, accent_b),
                ExtPanelStyle::Normal => (item_r, item_g, item_b),
            }
        };

        // Measure right-side decorations: badges + actions + hint
        let mut right_w: f64 = 0.0;

        // Measure badges
        for badge in &row.badges {
            let badge_text = format!(" {} ", badge.text);
            layout.set_text(&badge_text);
            right_w += layout.pixel_size().0 as f64 + 4.0;
        }

        // Measure actions (only shown on selected row)
        if is_sel && panel.has_focus {
            for action in &row.actions {
                let action_text = format!(" {} ", action.label);
                layout.set_text(&action_text);
                right_w += layout.pixel_size().0 as f64 + 4.0;
            }
        }

        // Measure hint
        let hint_w = if !row.hint.is_empty() {
            layout.set_text(&row.hint);
            let pw = layout.pixel_size().0 as f64;
            right_w += pw + 4.0;
            pw
        } else {
            0.0
        };

        // Draw text with ellipsis
        let name_max = (w - 6.0 - right_w).max(20.0) as i32;
        cr.set_source_rgb(text_r, text_g, text_b);
        layout.set_text(&row.text);
        layout.set_width(name_max * pango::SCALE);
        layout.set_ellipsize(pango::EllipsizeMode::End);
        let (_, text_h) = layout.pixel_size();
        cr.move_to(x + 2.0, row_y + (line_height - text_h as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
        layout.set_width(-1);
        layout.set_ellipsize(pango::EllipsizeMode::None);

        // Draw right-side decorations from right to left
        let mut rx = x + w - 4.0;
        let text_y = row_y + (line_height - text_h as f64) / 2.0;

        // Hint (rightmost)
        if hint_w > 0.0 {
            rx -= hint_w;
            cr.set_source_rgb(dim_r, dim_g, dim_b);
            layout.set_text(&row.hint);
            cr.move_to(rx, text_y);
            pangocairo::show_layout(cr, layout);
            rx -= 4.0;
        }

        // Actions (only on selected row)
        if is_sel && panel.has_focus {
            for action in row.actions.iter().rev() {
                let action_text = format!(" {} ", action.label);
                layout.set_text(&action_text);
                let aw = layout.pixel_size().0 as f64;
                rx -= aw;
                // Draw action button background
                cr.set_source_rgb(accent_r, accent_g, accent_b);
                cr.rectangle(rx, row_y + 2.0, aw, line_height - 4.0);
                cr.fill().ok();
                cr.set_source_rgb(bg_r, bg_g, bg_b);
                cr.move_to(rx, text_y);
                pangocairo::show_layout(cr, layout);
                rx -= 4.0;
            }
        }

        // Badges
        for badge in row.badges.iter().rev() {
            let badge_text = format!(" {} ", badge.text);
            layout.set_text(&badge_text);
            let bw = layout.pixel_size().0 as f64;
            rx -= bw;
            // Parse badge color (try hex, fallback to dim)
            let (br, bg, bb) = parse_badge_color(&badge.color).unwrap_or((dim_r, dim_g, dim_b));
            // Draw badge pill background (slightly transparent)
            cr.set_source_rgba(br, bg, bb, 0.25);
            cr.rectangle(rx, row_y + 2.0, bw, line_height - 4.0);
            cr.fill().ok();
            // Badge text in badge color
            cr.set_source_rgb(br, bg, bb);
            cr.move_to(rx, text_y);
            pangocairo::show_layout(cr, layout);
            rx -= 4.0;
        }
    }

    // ── Scrollbar ───────────────────────────────────────────────────────────
    let total = flat_rows.len();
    if total > max_rows && max_rows > 0 {
        let track_h = content_h;
        let thumb_h = (track_h * max_rows as f64 / total as f64).max(4.0);
        let thumb_top = scroll as f64 * track_h / total as f64;
        let sb_x = x + w - 5.0;
        cr.set_source_rgb(dim_r, dim_g, dim_b);
        cr.rectangle(sb_x, y + ry + thumb_top, 4.0, thumb_h);
        cr.fill().ok();
    }

    // ── Help popup overlay ──────────────────────────────────────────────────
    if panel.help_open && !panel.help_bindings.is_empty() {
        let bindings = &panel.help_bindings;
        let popup_w = w.min(280.0);
        let popup_h = line_height * (bindings.len() as f64 + 2.0);
        let popup_x = x + (w - popup_w) / 2.0;
        let popup_y = y + (h - popup_h) / 2.0;

        let (r, g, b) = theme.completion_bg.to_cairo();
        cr.set_source_rgb(r, g, b);
        cr.rectangle(popup_x, popup_y, popup_w, popup_h);
        cr.fill().ok();
        let (r, g, b) = theme.completion_border.to_cairo();
        cr.set_source_rgb(r, g, b);
        cr.set_line_width(1.0);
        cr.rectangle(popup_x, popup_y, popup_w, popup_h);
        cr.stroke().ok();

        // Title + close hint
        let (r, g, b) = theme.completion_fg.to_cairo();
        cr.set_source_rgb(r, g, b);
        layout.set_text("Keybindings");
        layout.set_attributes(None);
        cr.move_to(popup_x + 8.0, popup_y);
        pangocairo::show_layout(cr, layout);

        layout.set_text("x");
        cr.move_to(popup_x + popup_w - 16.0, popup_y);
        pangocairo::show_layout(cr, layout);

        // Bindings
        for (i, (key, desc)) in bindings.iter().enumerate() {
            let bind_y = popup_y + line_height * (i as f64 + 1.0);
            let (r, g, b) = theme.function.to_cairo();
            cr.set_source_rgb(r, g, b);
            layout.set_text(key);
            layout.set_attributes(None);
            cr.move_to(popup_x + 12.0, bind_y);
            pangocairo::show_layout(cr, layout);

            let (r, g, b) = theme.completion_fg.to_cairo();
            cr.set_source_rgb(r, g, b);
            layout.set_text(desc);
            layout.set_attributes(None);
            cr.move_to(popup_x + 100.0, bind_y);
            pangocairo::show_layout(cr, layout);
        }
    }
}

/// Parse a badge color string (hex like "#4ec9b0" or named colors) to cairo RGB.
pub(super) fn parse_badge_color(color: &str) -> Option<(f64, f64, f64)> {
    if let Some(hex) = color.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()? as f64 / 255.0;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()? as f64 / 255.0;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()? as f64 / 255.0;
            return Some((r, g, b));
        }
    }
    match color {
        "red" => Some((0.9, 0.3, 0.3)),
        "green" => Some((0.3, 0.8, 0.4)),
        "blue" => Some((0.3, 0.5, 0.9)),
        "yellow" => Some((0.9, 0.8, 0.3)),
        "orange" => Some((0.9, 0.6, 0.2)),
        "cyan" => Some((0.3, 0.8, 0.8)),
        "magenta" | "purple" => Some((0.7, 0.3, 0.8)),
        _ => None,
    }
}

// ─── Panel hover popup (sidebar item dwell tooltip with markdown) ──────────────

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn draw_panel_hover_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    sidebar_right_x: f64,
    _sidebar_y: f64,
    window_w: f64,
    window_h: f64,
    line_height: f64,
    is_native: bool,
) -> (
    Vec<(f64, f64, f64, f64, String, bool)>,
    Option<(f64, f64, f64, f64)>,
) {
    use crate::core::markdown::MdStyle;

    let Some(ref hover) = screen.panel_hover else {
        return (vec![], None);
    };

    let rendered = &hover.rendered;
    if rendered.lines.is_empty() {
        return (vec![], None);
    }

    // Measure popup dimensions.
    const MAX_POPUP_W: f64 = 400.0;
    const MAX_POPUP_H: f64 = 300.0;
    const PADDING: f64 = 6.0;

    let num_lines = rendered.lines.len().min(20);

    // Measure the widest line to set popup width.
    let mut max_line_w = 0i32;
    for line_text in rendered.lines.iter().take(num_lines) {
        let display = format!(" {} ", line_text);
        layout.set_text(&display);
        layout.set_attributes(None);
        let (w, _) = layout.pixel_size();
        if w > max_line_w {
            max_line_w = w;
        }
    }
    let popup_w = (max_line_w as f64 + PADDING * 2.0).clamp(80.0, MAX_POPUP_W);
    let popup_h = (num_lines as f64 * line_height + PADDING * 2.0).min(MAX_POPUP_H);

    // Y position: align with the hovered item row.
    let item_row_y = if hover.panel_name == "source_control" {
        // SC layout: header(lh) + gap + commit(lh*rows) + gap + buttons(lh) + gap + sections
        // The flat index maps into the sections area. Use SC-specific geometry.
        let gap = (line_height * 0.3).round();
        let item_height = (line_height * 1.4).round();
        let commit_rows = screen
            .source_control
            .as_ref()
            .map(|sc| sc.commit_message.split('\n').count().max(1))
            .unwrap_or(1) as f64;
        let section_top = line_height + gap + commit_rows * line_height + gap + line_height + gap;
        // Walk sections to find the accumulated Y offset for the hovered flat_idx.
        // Headers use line_height, items use item_height.
        // Staged + Unstaged always show; Worktrees only when > 1; Log always shows.
        if let Some(ref sc) = screen.source_control {
            let show_worktrees = sc.worktrees.len() > 1;
            let mut sections: Vec<(usize, bool)> = vec![
                (sc.staged.len(), sc.sections_expanded[0]),
                (sc.unstaged.len(), sc.sections_expanded[1]),
            ];
            if show_worktrees {
                sections.push((sc.worktrees.len(), sc.sections_expanded[2]));
            }
            sections.push((sc.log.len(), sc.sections_expanded[3]));

            let mut y_off = section_top;
            let mut fi = 0usize;
            'outer: for &(count, expanded) in &sections {
                if fi == hover.item_index {
                    break;
                }
                y_off += line_height; // section header
                fi += 1;
                if expanded {
                    for _ in 0..count {
                        if fi == hover.item_index {
                            break 'outer;
                        }
                        y_off += item_height;
                        fi += 1;
                    }
                }
            }
            y_off
        } else {
            section_top + hover.item_index as f64 * line_height
        }
    } else {
        // Ext panels: header row + item_index rows (uniform line_height)
        line_height + hover.item_index as f64 * line_height
    };
    let popup_y = if item_row_y + popup_h <= window_h {
        item_row_y
    } else {
        (item_row_y + line_height - popup_h).max(0.0)
    };

    // X position: right edge of sidebar, extending into the editor area.
    // Clamp width so it doesn't extend past the window.
    let avail_w = (window_w - sidebar_right_x).max(0.0);
    let popup_w = popup_w.min(avail_w);
    if popup_w < 40.0 {
        return (vec![], None);
    }
    let popup_x = sidebar_right_x;

    // Background.
    let (bg_r, bg_g, bg_b) = theme.hover_bg.to_cairo();
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border.
    let (br, bg_brd, bb) = theme.hover_border.to_cairo();
    cr.set_source_rgb(br, bg_brd, bb);
    cr.set_line_width(1.0);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Render each line with markdown styling.
    let max_text_w = (popup_w - PADDING * 2.0).max(20.0) as i32;
    let mut link_rects: Vec<(f64, f64, f64, f64, String, bool)> = Vec::new();
    for (line_idx, line_text) in rendered.lines.iter().enumerate().take(num_lines) {
        let row_y = popup_y + PADDING + line_idx as f64 * line_height;
        if row_y + line_height > popup_y + popup_h {
            break;
        }

        let display = format!(" {}", line_text);
        let line_spans = rendered
            .spans
            .get(line_idx)
            .map(|s| s.as_slice())
            .unwrap_or(&[]);
        let code_hl = rendered.code_highlights.get(line_idx);
        let has_code_hl = code_hl.is_some_and(|h| !h.is_empty());

        // Build pango attr list for this line.
        let attrs = AttrList::new();

        // Base foreground color for the whole line.
        let (base_r, base_g, base_b) = theme.hover_fg.to_pango_u16();
        let mut base_fg = AttrColor::new_foreground(base_r, base_g, base_b);
        base_fg.set_start_index(0);
        base_fg.set_end_index(display.len() as u32 + 1);
        attrs.insert(base_fg);

        // Apply span styles (offset by 1 byte for the leading space).
        const OFFSET: u32 = 1;

        if has_code_hl {
            // Use tree-sitter syntax highlighting for code block lines.
            for hl in code_hl.unwrap() {
                let start = hl.start_byte as u32 + OFFSET;
                let end = (hl.end_byte as u32 + OFFSET).min(display.len() as u32 + 1);
                if start >= end {
                    continue;
                }
                let color = theme.scope_color(&hl.scope);
                let (r, g, b) = color.to_pango_u16();
                let mut fg_attr = AttrColor::new_foreground(r, g, b);
                fg_attr.set_start_index(start);
                fg_attr.set_end_index(end);
                attrs.insert(fg_attr);
            }
        } else {
            for span in line_spans {
                let start = span.start_byte as u32 + OFFSET;
                let end = (span.end_byte as u32 + OFFSET).min(display.len() as u32 + 1);
                if start >= end {
                    continue;
                }

                let (fg_color, bold, italic): (render::Color, bool, bool) = match span.style {
                    MdStyle::Heading(1) => (theme.md_heading1, true, false),
                    MdStyle::Heading(2) => (theme.md_heading2, true, false),
                    MdStyle::Heading(_) => (theme.md_heading3, true, false),
                    MdStyle::Bold => (theme.hover_fg, true, false),
                    MdStyle::Italic => (theme.hover_fg, false, true),
                    MdStyle::BoldItalic => (theme.hover_fg, true, true),
                    MdStyle::Code | MdStyle::CodeBlock => (theme.md_code, false, false),
                    MdStyle::Link => (theme.md_link, false, false),
                    MdStyle::LinkUrl => (theme.md_link, false, true),
                    MdStyle::BlockQuote => (theme.md_heading3, false, true),
                    MdStyle::ListBullet => (theme.md_heading1, true, false),
                    MdStyle::HorizontalRule => (theme.line_number_fg, false, false),
                    MdStyle::Image => (theme.md_link, false, true),
                };

                let (fr, fg_g, fb) = fg_color.to_pango_u16();
                let mut fg_attr = AttrColor::new_foreground(fr, fg_g, fb);
                fg_attr.set_start_index(start);
                fg_attr.set_end_index(end);
                attrs.insert(fg_attr);

                if bold {
                    let mut w = pango::AttrInt::new_weight(pango::Weight::Bold);
                    w.set_start_index(start);
                    w.set_end_index(end);
                    attrs.insert(w);
                }
                if italic {
                    let mut s = pango::AttrInt::new_style(pango::Style::Italic);
                    s.set_start_index(start);
                    s.set_end_index(end);
                    attrs.insert(s);
                }
                if span.style == MdStyle::Link {
                    let mut u = pango::AttrInt::new_underline(pango::Underline::Single);
                    u.set_start_index(start);
                    u.set_end_index(end);
                    attrs.insert(u);
                }
            }
        }

        layout.set_text(&display);
        layout.set_attributes(Some(&attrs));
        layout.set_width(max_text_w * pango::SCALE);
        layout.set_ellipsize(pango::EllipsizeMode::End);
        let (_, text_h) = layout.pixel_size();
        cr.move_to(
            popup_x + PADDING,
            row_y + (line_height - text_h as f64) / 2.0,
        );
        pangocairo::show_layout(cr, layout);

        // Compute link hit rects for clickable URLs in this line.
        for link in &hover.links {
            if link.0 != line_idx {
                continue;
            }
            // link = (line_idx, start_byte, end_byte, url)
            // Measure the pixel position of the link text within the layout.
            // The display string has a 1-byte " " prefix, so offset by 1.
            let link_start = link.1 + 1; // byte offset in display string
            let link_end = link.2 + 1;
            // Use pango to convert byte indices to pixel positions.
            let start_idx = layout.index_to_pos(link_start as i32);
            let end_idx = layout.index_to_pos(link_end.min(display.len()) as i32);
            let lx = popup_x + PADDING + start_idx.x() as f64 / pango::SCALE as f64;
            let ly = row_y;
            let lw = (end_idx.x() - start_idx.x()) as f64 / pango::SCALE as f64;
            let lh = line_height;
            link_rects.push((lx, ly, lw, lh, link.3.clone(), is_native));
        }

        layout.set_attributes(None);
        layout.set_width(-1);
        layout.set_ellipsize(pango::EllipsizeMode::None);
    }
    (link_rects, Some((popup_x, popup_y, popup_w, popup_h)))
}

/// Migrated to `quadraui::MultiSectionView` (#293).
///
/// Panel header + search input + focus border stay panel-specific
/// chrome; the two "INSTALLED" / "AVAILABLE" sections become a
/// `MultiSectionView` built by
/// `render::ext_sidebar_to_multi_section_view` and rasterised via
/// `quadraui::gtk::draw_multi_section_view`. Both paint and
/// `Msg::ExtSidebarClick` consult the same `MultiSectionViewLayout`
/// (via `quadraui::gtk::gtk_msv_layout`), so per-section
/// drift is impossible by construction (the structural fix for the
/// #281 bug classes).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_ext_sidebar(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    line_height: f64,
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    engine: &crate::core::engine::Engine,
) {
    let Some(ref ext) = screen.ext_sidebar else {
        return;
    };

    let (bg_r, bg_g, bg_b) = theme.tab_bar_bg.to_cairo();
    let (hdr_r, hdr_g, hdr_b) = theme.status_bg.to_cairo();
    let (hdr_fg_r, hdr_fg_g, hdr_fg_b) = theme.status_fg.to_cairo();
    let (fg_r, fg_g, fg_b) = theme.foreground.to_cairo();
    let (dim_r, dim_g, dim_b) = theme.line_number_fg.to_cairo();

    // Background
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    layout.set_attributes(None);
    let mut ry: f64 = 0.0;

    // ── Row 0: panel header ──────────────────────────────────────────────────
    cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
    cr.rectangle(x, y + ry, w, line_height);
    cr.fill().ok();
    let hdr_text = if ext.fetching {
        format!("  {} EXTENSIONS  (fetching…)", icons::EXTENSIONS.nerd)
    } else {
        format!("  {} EXTENSIONS", icons::EXTENSIONS.nerd)
    };
    cr.set_source_rgb(hdr_fg_r, hdr_fg_g, hdr_fg_b);
    layout.set_text(&hdr_text);
    let (_, lh) = layout.pixel_size();
    cr.move_to(x + 2.0, y + ry + (line_height - lh as f64) / 2.0);
    pangocairo::show_layout(cr, layout);
    ry += line_height;

    // ── Row 1: search box ─────────────────────────────────────────────────────
    if ry < h {
        let (inp_bg_r, inp_bg_g, inp_bg_b) = if ext.input_active {
            theme.fuzzy_selected_bg.to_cairo()
        } else {
            theme.completion_bg.to_cairo()
        };
        cr.set_source_rgb(inp_bg_r, inp_bg_g, inp_bg_b);
        cr.rectangle(x, y + ry, w, line_height);
        cr.fill().ok();
        let si = icons::SEARCH.nerd;
        let search_text = if ext.input_active {
            format!(" {}  {}|", si, ext.query)
        } else if ext.query.is_empty() {
            format!(" {}  Search extensions (press /)", si)
        } else {
            format!(" {}  {}", si, ext.query)
        };
        let (text_r, text_g, text_b) = if ext.input_active || !ext.query.is_empty() {
            (fg_r, fg_g, fg_b)
        } else {
            (dim_r, dim_g, dim_b)
        };
        cr.set_source_rgb(text_r, text_g, text_b);
        layout.set_text(&search_text);
        let (_, lh2) = layout.pixel_size();
        cr.move_to(x + 2.0, y + ry + (line_height - lh2 as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
        ry += line_height;
    }

    // ── SidebarSystem body: rest of the panel ──────────────────────────────
    let body_h = (h - ry).max(0.0);
    if body_h > 0.0 {
        let body_rect = quadraui::Rect::new(x as f32, (y + ry) as f32, w as f32, body_h as f32);
        engine.ext_sidebar_body_rect.set(body_rect);
        render::populate_ext_sidebar_system(engine);
        backend.borrow_mut().enter_frame_scope(cr, layout, |b| {
            b.set_current_theme(super::quadraui_gtk::q_theme(theme));
            b.set_current_line_height(line_height);
            engine.ext_sidebar_system.borrow().render(b, body_rect);
        });
    }

    // Focus border (drawn last so it sits on top of bg + section paint)
    if ext.has_focus {
        let (kr, kg, kb) = theme.keyword.to_cairo();
        cr.set_source_rgb(kr, kg, kb);
        cr.set_line_width(1.5);
        cr.rectangle(x + 0.75, y + 0.75, w - 1.5, h - 1.5);
        cr.stroke().ok();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_ai_sidebar(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    line_height: f64,
) {
    let Some(ref ai) = screen.ai_panel else {
        return;
    };

    let (bg_r, bg_g, bg_b) = theme.tab_bar_bg.to_cairo();
    let (hdr_r, hdr_g, hdr_b) = theme.status_bg.to_cairo();
    let (hdr_fg_r, hdr_fg_g, hdr_fg_b) = theme.status_fg.to_cairo();
    let (fg_r, fg_g, fg_b) = theme.foreground.to_cairo();
    let (dim_r, dim_g, dim_b) = theme.line_number_fg.to_cairo();

    // Background
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    layout.set_attributes(None);
    let mut row: usize = 0;

    // ── Row 0: header ─────────────────────────────────────────────────────────
    {
        cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
        cr.rectangle(x, y + row as f64 * line_height, w, line_height);
        cr.fill().ok();
        let ai_icon = icons::AI_CHAT.nerd;
        let hdr_thinking = format!("  {} AI ASSISTANT  (thinking…)", ai_icon);
        let hdr_idle = format!("  {} AI ASSISTANT", ai_icon);
        let hdr_text = if ai.streaming {
            hdr_thinking.as_str()
        } else {
            hdr_idle.as_str()
        };
        cr.set_source_rgb(hdr_fg_r, hdr_fg_g, hdr_fg_b);
        layout.set_text(hdr_text);
        let (_, lh) = layout.pixel_size();
        cr.move_to(
            x + 2.0,
            y + row as f64 * line_height + (line_height - lh as f64) / 2.0,
        );
        pangocairo::show_layout(cr, layout);
        row += 1;
    }

    // ── Measure char width and compute input/message layout ───────────────────
    layout.set_text("W");
    let (char_w_px, _) = layout.pixel_size();
    let wrap_cols = if char_w_px > 0 {
        ((w - 20.0) / char_w_px as f64).floor() as usize
    } else {
        40
    }
    .max(10);

    // Input: content width = wrap_cols - 1 (for " > " prefix equivalent)
    let input_content_w = wrap_cols.saturating_sub(1).max(1);
    let input_chars: Vec<char> = ai.input.chars().collect();
    let input_line_count = {
        let raw = if input_chars.is_empty() {
            1
        } else {
            input_chars.len().div_ceil(input_content_w)
        };
        // cap at half the panel height so messages are still visible
        let max_lines = ((h / line_height) as usize / 2).max(1);
        raw.min(max_lines).max(1)
    };
    // separator (1 line) + input lines
    let input_height = (input_line_count + 1) as f64 * line_height;
    let input_y = h - input_height;
    let max_row_y = input_y; // messages render above this

    // ── Message history ───────────────────────────────────────────────────────
    let q_user_fg = render::to_quadraui_color(theme.keyword);
    let q_asst_fg = render::to_quadraui_color(theme.string_lit);
    let q_default_fg = render::to_quadraui_color(theme.foreground);
    let q_dim_fg = render::to_quadraui_color(theme.line_number_fg);
    let mut rows: Vec<quadraui::MessageRow> = Vec::new();
    for msg in &ai.messages {
        let is_user = msg.role == "user";
        let (role_label, role_fg) = if is_user {
            ("You:", q_user_fg)
        } else {
            ("AI:", q_asst_fg)
        };
        rows.push(quadraui::MessageRow::new(role_label, role_fg, 4.0));
        for line in msg.content.lines() {
            if line.is_empty() {
                rows.push(quadraui::MessageRow::new(" ", q_default_fg, 12.0));
                continue;
            }
            let chars: Vec<char> = line.chars().collect();
            let mut pos = 0;
            while pos < chars.len() {
                let end = (pos + wrap_cols).min(chars.len());
                let chunk: String = chars[pos..end].iter().collect();
                rows.push(quadraui::MessageRow::new(chunk, q_default_fg, 12.0));
                pos = end;
            }
        }
        rows.push(quadraui::MessageRow::new(" ", q_dim_fg, 0.0));
    }
    let scroll = ai.scroll_top.min(rows.len().saturating_sub(1));
    let msg_list = quadraui::MessageList {
        id: quadraui::WidgetId::new("gtk:ai:messages"),
        rows,
        scroll_top: scroll,
    };
    quadraui::gtk::draw_message_list(
        cr,
        layout,
        &msg_list,
        x,
        y + line_height,
        w,
        y + max_row_y,
        line_height,
    );

    // ── Input area (grows with content) ───────────────────────────────────────
    // Separator line
    cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
    cr.rectangle(x, y + input_y, w, 1.0);
    cr.fill().ok();

    // Input background
    let (inp_bg_r, inp_bg_g, inp_bg_b) = if ai.input_active {
        theme.fuzzy_selected_bg.to_cairo()
    } else {
        theme.completion_bg.to_cairo()
    };
    cr.set_source_rgb(inp_bg_r, inp_bg_g, inp_bg_b);
    cr.rectangle(x, y + input_y + 1.0, w, input_height - 1.0);
    cr.fill().ok();

    let cursor = ai.input_cursor.min(input_chars.len());
    let cursor_line = cursor.checked_div(input_content_w).unwrap_or(0);
    let cursor_col = if input_content_w > 0 {
        cursor % input_content_w
    } else {
        cursor
    };

    if ai.input_active || !ai.input.is_empty() {
        // Split input into visual chunks and render each line
        let chunks: Vec<Vec<char>> = if input_chars.is_empty() {
            vec![vec![]]
        } else {
            input_chars
                .chunks(input_content_w)
                .map(|c| c.to_vec())
                .collect()
        };

        // Measure the pixel width of the " > " prefix once
        layout.set_text(" > ");
        let (pfx_px, _) = layout.pixel_size();
        let pfx_w = pfx_px as f64;

        for (line_idx, chunk) in chunks.iter().enumerate().take(input_line_count) {
            let ly = y + input_y + 1.0 + line_idx as f64 * line_height;
            let pfx = if line_idx == 0 { " > " } else { "   " };

            // Prefix
            let (txt_r, txt_g, txt_b) = if ai.input_active {
                (fg_r, fg_g, fg_b)
            } else {
                (dim_r, dim_g, dim_b)
            };
            cr.set_source_rgb(txt_r, txt_g, txt_b);
            layout.set_text(pfx);
            let (_, lh) = layout.pixel_size();
            cr.move_to(x + 2.0, ly + (line_height - lh as f64) / 2.0);
            pangocairo::show_layout(cr, layout);

            // Content
            if !chunk.is_empty() {
                let s: String = chunk.iter().collect();
                layout.set_text(&s);
                let (_, lh) = layout.pixel_size();
                cr.move_to(x + 2.0 + pfx_w, ly + (line_height - lh as f64) / 2.0);
                pangocairo::show_layout(cr, layout);
            }

            // Cursor bar on the active cursor line
            if ai.input_active && line_idx == cursor_line {
                let before: String = chunk.iter().take(cursor_col).collect();
                layout.set_text(&before);
                let (before_px, lh) = layout.pixel_size();
                let cx = x + 2.0 + pfx_w + before_px as f64;
                let text_y = ly + (line_height - lh as f64) / 2.0;
                cr.set_source_rgb(fg_r, fg_g, fg_b);
                cr.set_line_width(1.5);
                cr.move_to(cx, text_y);
                cr.line_to(cx, text_y + lh as f64);
                cr.stroke().ok();
            }
        }
    } else {
        // Placeholder
        let placeholder = if ai.streaming {
            " (waiting for response…)"
        } else {
            " Press i to type a message…"
        };
        layout.set_text(placeholder);
        let (_, lh) = layout.pixel_size();
        cr.set_source_rgb(dim_r, dim_g, dim_b);
        cr.move_to(x + 2.0, y + input_y + 1.0 + (line_height - lh as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
    }

    // Focus border
    if ai.has_focus {
        let (kr, kg, kb) = theme.keyword.to_cairo();
        cr.set_source_rgb(kr, kg, kb);
        cr.set_line_width(1.5);
        cr.rectangle(x + 0.75, y + 0.75, w - 1.5, h - 1.5);
        cr.stroke().ok();
    }

    let _ = row;
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_debug_toolbar(
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    cr: &Context,
    toolbar: &render::DebugToolbarData,
    theme: &Theme,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    hit_layout_out: &Rc<RefCell<Option<quadraui::StatusBarLayout>>>,
    hovered_id: Option<&quadraui::WidgetId>,
    pressed_id: Option<&quadraui::WidgetId>,
) {
    let pango_ctx = pangocairo::create_context(cr);
    let ui_font_desc = FontDescription::from_string(&UI_FONT());
    let ui_layout = pango::Layout::new(&pango_ctx);
    ui_layout.set_font_description(Some(&ui_font_desc));

    let bar = render::debug_toolbar_to_quadraui_status_bar(toolbar, theme);
    use quadraui::Backend;
    let bar_layout = backend.borrow_mut().enter_frame_scope(cr, &ui_layout, |b| {
        b.set_current_theme(super::quadraui_gtk::q_theme(theme));
        b.set_current_line_height(height);
        b.draw_status_bar(
            quadraui::Rect::new(x as f32, y as f32, width as f32, height as f32),
            &bar,
            hovered_id,
            pressed_id,
        )
    });
    *hit_layout_out.borrow_mut() = Some(bar_layout);
}
