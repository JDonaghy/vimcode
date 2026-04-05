use super::*;

/// Pango font description string for UI panels (menu bar, sidebars, dropdown).
/// Matches VSCode's Linux font stack at 11pt ≈ 13px @ 96 dpi.
pub(super) const UI_FONT: &str = "Segoe UI, Ubuntu, Droid Sans, Sans 10";

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
    diff_btn_map_out: &Rc<RefCell<DiffBtnMap>>,
    split_btn_map_out: &Rc<RefCell<SplitBtnMap>>,
    action_btn_map_out: &Rc<RefCell<ActionBtnMap>>,
    dialog_btn_rects_out: &Rc<RefCell<DialogBtnRects>>,
    editor_hover_rect_out: &Rc<Cell<Option<(f64, f64, f64, f64)>>>,
    editor_hover_link_rects_out: &Rc<RefCell<Vec<(f64, f64, f64, f64, String)>>>,
    mouse_pos: (f64, f64),
    tab_visible_counts_out: &Rc<RefCell<Vec<(crate::core::window::GroupId, usize)>>>,
) {
    let theme = Theme::from_name(&engine.settings.colorscheme);

    // Clear cached button positions from previous frame.
    diff_btn_map_out.borrow_mut().clear();
    split_btn_map_out.borrow_mut().clear();
    action_btn_map_out.borrow_mut().clear();

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

    // Derive line height and char width from font metrics
    let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
    let line_height = (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;
    let char_width = font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;

    // Only send CacheFontMetrics when metrics actually change (e.g. on startup or font change).
    // Sending on every draw creates a feedback loop: draw → message → #[watch] → queue_draw → draw.
    let (last_lh, last_cw) = last_metrics.get();
    if (last_lh - line_height).abs() > 0.01 || (last_cw - char_width).abs() > 0.01 {
        last_metrics.set((line_height, char_width));
        sender
            .send(Msg::CacheFontMetrics(line_height, char_width))
            .ok();
    }

    // Calculate layout regions
    let tab_row_height = (line_height * 1.6).ceil();
    let tab_bar_height = if engine.settings.breadcrumbs {
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
    let global_status_rows = if per_window_status { 1.0 } else { 2.0 }; // cmd only vs status+cmd
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
    let term_px = if engine.terminal_open || engine.bottom_panel_open {
        (engine.session.terminal_panel_rows as usize + 2) as f64 * line_height
    } else {
        0.0
    };

    let debug_toolbar_px = if engine.debug_toolbar_visible {
        line_height
    } else {
        0.0
    };

    // Calculate window rects for all editor groups.
    // editor_bounds spans the full editor area from y=0; tab_bar_height is reserved per group.
    let editor_bounds = WindowRect::new(
        0.0,
        0.0,
        width as f64,
        height as f64 - status_bar_height - debug_toolbar_px - qf_px - term_px,
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
            let is_active = gtb.group_id == split.active_group;
            // In diff mode, show split buttons on all groups so clicking
            // an inactive group's toolbar doesn't cause a visual shift.
            let show_split = is_active || engine.is_in_diff_view();
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
            let accent = if is_active {
                Some(theme.tab_active_accent)
            } else {
                None
            };
            let (positions, dbp, sbp, vis_count, abp) = draw_tab_bar(
                cr,
                &layout,
                &theme,
                &gtb.tabs,
                tab_w,
                line_height,
                0.0,
                show_split,
                hover_idx,
                gtb.diff_toolbar.as_ref(),
                gtb.tab_scroll_offset,
                accent,
            );
            tab_slot_positions_out
                .borrow_mut()
                .insert(gtb.group_id.0, positions);
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
                .push((gtb.group_id, vis_count));
            cr.restore().ok();
        }
    } else if !engine.is_tab_bar_hidden(engine.active_group) {
        // Single group: draw tab bar at full width with split buttons.
        let hover_idx = tab_close_hover.map(|(_gid, tidx)| tidx);
        let (positions, dbp, sbp, vis_count, abp) = draw_tab_bar(
            cr,
            &layout,
            &theme,
            &screen.tab_bar,
            width as f64,
            line_height,
            0.0,
            true,
            hover_idx,
            screen.diff_toolbar.as_ref(),
            screen.tab_scroll_offset,
            Some(theme.tab_active_accent),
        );
        // Use group_id 0 for single-group mode
        tab_slot_positions_out
            .borrow_mut()
            .insert(engine.active_group.0, positions);
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
            .push((engine.active_group, vis_count));
    }

    // 4b. Draw breadcrumb bar(s) below tab bar(s)
    for bc in &screen.breadcrumbs {
        if bc.segments.is_empty() {
            continue;
        }
        // Breadcrumb bar sits one line_height above the window content (bc.bounds.y)
        // and one line_height below the tab bar.
        let bc_y = bc.bounds.y - line_height;
        let bc_x = bc.bounds.x;
        let bc_w = bc.bounds.width;
        cr.save().ok();
        cr.translate(bc_x, 0.0);
        draw_breadcrumb_bar(
            cr,
            &layout,
            &theme,
            &bc.segments,
            bc_w,
            line_height,
            bc_y,
            engine.breadcrumb_focus,
            engine.breadcrumb_selected,
        );
        cr.restore().ok();
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
        );
    }

    // 5b. Draw completion popup (on top of everything else)
    draw_completion_popup(cr, &layout, &screen, &theme, line_height, char_width);

    // 5c. Draw hover popup (on top of everything else)
    draw_hover_popup(cr, &layout, &screen, &theme, line_height, char_width);

    // 5c2. Draw signature-help popup (on top of everything else, shown in insert mode)
    draw_signature_popup(cr, &layout, &screen, &theme, line_height, char_width);

    // 5c3. Draw diff peek popup (inline git hunk preview)
    draw_diff_peek_popup(cr, &layout, &screen, &theme, line_height, char_width);

    // 5c4. Draw editor hover popup (gh key, diagnostic/annotation/plugin hovers)
    let (eh_rect, eh_links) =
        draw_editor_hover_popup(cr, &layout, &screen, &theme, line_height, char_width);
    editor_hover_rect_out.set(eh_rect);
    *editor_hover_link_rects_out.borrow_mut() = eh_links;

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

    // 5d. Draw unified picker modal (on top of everything else)
    draw_picker_popup(
        cr,
        &layout,
        &screen,
        &theme,
        width as f64,
        height as f64,
        line_height,
    );

    // 5e3. Draw tab switcher popup
    draw_tab_switcher_popup(
        cr,
        &screen,
        &theme,
        width as f64,
        height as f64,
        line_height,
    );

    // 5e4. Draw modal dialog (highest z-order)
    let btn_rects = draw_dialog_popup(
        cr,
        &layout,
        &screen,
        &theme,
        width as f64,
        height as f64,
        line_height,
    );
    *dialog_btn_rects_out.borrow_mut() = btn_rects;

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
        );
    }

    // 5g. Draw bottom panel (Terminal or Debug Output) with a tab bar.
    if term_px > 0.0 {
        let term_y = height as f64 - status_bar_height - debug_toolbar_px - term_px;
        // Tab bar row (1 line high) at the top of the bottom panel area.
        draw_bottom_panel_tabs(
            cr,
            &layout,
            &screen,
            &theme,
            0.0,
            term_y,
            width as f64,
            line_height,
            engine.terminal_open,
            !screen.bottom_tabs.output_lines.is_empty(),
        );
        match screen.bottom_tabs.active {
            render::BottomPanelKind::Terminal => {
                if let Some(ref term_panel) = screen.bottom_tabs.terminal {
                    draw_terminal_panel(
                        cr,
                        &layout,
                        term_panel,
                        &theme,
                        0.0,
                        term_y + line_height, // skip tab bar row
                        width as f64,
                        term_px - line_height,
                        line_height,
                        char_width,
                        sender,
                    );
                }
            }
            render::BottomPanelKind::DebugOutput => {
                draw_debug_output(
                    cr,
                    &layout,
                    &screen.bottom_tabs.output_lines,
                    &theme,
                    0.0,
                    term_y + line_height,
                    width as f64,
                    term_px - line_height,
                    line_height,
                );
            }
        }
    }

    // 5h. Draw debug toolbar strip if visible (above status bar)
    if let Some(ref toolbar) = screen.debug_toolbar {
        let toolbar_y = height as f64 - status_bar_height - debug_toolbar_px;
        draw_debug_toolbar(
            cr,
            toolbar,
            &theme,
            0.0,
            toolbar_y,
            width as f64,
            line_height,
        );
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
    if per_window_status {
        for rendered_window in &screen.windows {
            if let Some(ref status) = rendered_window.status_line {
                let wr = &rendered_window.rect;
                let bar_y = wr.y + wr.height - line_height;
                draw_window_status_bar(
                    cr,
                    &layout,
                    &theme,
                    status,
                    wr.x,
                    bar_y,
                    wr.width,
                    line_height,
                );
            }
        }
    }

    // 6. Status Line (global — only when per-window status is off)
    let status_y = height as f64 - status_bar_height;
    if !per_window_status {
        draw_status_line(
            cr,
            &layout,
            &theme,
            &screen.status_left,
            &screen.status_right,
            width as f64,
            status_y,
            line_height,
        );
    }

    // 6b. Wildmenu bar (between status and command line)
    let mut next_y = if per_window_status {
        status_y // no global status row — cmd line starts where status_y is
    } else {
        status_y + line_height // skip past the global status row
    };
    if let Some(ref wm) = screen.wildmenu {
        draw_wildmenu(cr, &layout, &theme, wm, width as f64, next_y, line_height);
        next_y += line_height;
    }

    // 7. Command Line (last line)
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
    for (window_id, rect) in window_rects {
        let Some((track_x, track_y, track_w, sb_height, thumb_x, thumb_w, _, _)) =
            h_scrollbar_geometry(engine, *window_id, rect, char_width, line_height)
        else {
            continue;
        };

        let is_active = dragging_window == Some(*window_id);

        // Track background (slightly darker when hovered/active)
        let track_alpha = if hovered || is_active { 0.35 } else { 0.20 };
        let (tr, tg, tb) = theme.scrollbar_track.to_cairo();
        cr.set_source_rgba(tr, tg, tb, track_alpha);
        cr.rectangle(track_x, track_y, track_w, sb_height);
        cr.fill().ok();

        // Thumb: brighter on hover, brighter still on active drag
        let thumb_alpha = if is_active {
            0.85
        } else if hovered {
            0.70
        } else {
            0.50
        };
        let (thr, thg, thb) = theme.scrollbar_thumb.to_cairo();
        cr.set_source_rgba(thr, thg, thb, thumb_alpha);
        cr.rectangle(thumb_x, track_y, thumb_w, sb_height);
        cr.fill().ok();
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
    layout: &pango::Layout,
) {
    use crate::core::window::{DropZone, SplitDirection, WindowRect};

    let tab_row_height = (line_height * 1.6).ceil();
    let tab_bar_height = if engine.settings.breadcrumbs {
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
    let global_status_rows = if per_window_status { 1.0 } else { 2.0 }; // cmd only vs status+cmd
    let status_bar_height = line_height * global_status_rows + wildmenu_px;
    let qf_px = if engine.quickfix_open {
        let n = engine.quickfix_items.len().clamp(1, 10) as f64;
        (n + 1.0) * line_height
    } else {
        0.0
    };
    let term_px = if engine.terminal_open || engine.bottom_panel_open {
        (engine.session.terminal_panel_rows as usize + 2) as f64 * line_height
    } else {
        0.0
    };
    let debug_toolbar_px = if engine.debug_toolbar_visible {
        line_height
    } else {
        0.0
    };
    let editor_bottom = height - status_bar_height - debug_toolbar_px - qf_px - term_px;
    let content_bounds = WindowRect::new(0.0, 0.0, width, editor_bottom);
    let mut group_rects = engine
        .group_layout
        .calculate_group_rects(content_bounds, tab_bar_height);
    engine.adjust_group_rects_for_hidden_tabs(&mut group_rects, tab_bar_height);

    // Helper: effective tab bar height for a group (0 if hidden).
    let eff_tbh = |gid: crate::core::window::GroupId| -> f64 {
        if engine.is_tab_bar_hidden(gid) {
            if engine.settings.breadcrumbs {
                tab_bar_height / 2.0
            } else {
                0.0
            }
        } else {
            tab_bar_height
        }
    };

    // Compute highlight rect from the drop zone.
    let zone = engine.tab_drop_zone;
    let highlight: Option<(f64, f64, f64, f64)> = match zone {
        DropZone::Center(gid) => group_rects.iter().find(|(g, _)| *g == gid).map(|(_, r)| {
            let tbh = eff_tbh(gid);
            (r.x, r.y - tbh, r.width, r.height + tbh)
        }),
        DropZone::Split(gid, dir, new_first) => {
            group_rects.iter().find(|(g, _)| *g == gid).map(|(_, r)| {
                let tbh = eff_tbh(gid);
                let full_y = r.y - tbh;
                let full_h = r.height + tbh;
                match (dir, new_first) {
                    (SplitDirection::Vertical, true) => (r.x, full_y, r.width / 2.0, full_h),
                    (SplitDirection::Vertical, false) => {
                        (r.x + r.width / 2.0, full_y, r.width / 2.0, full_h)
                    }
                    (SplitDirection::Horizontal, true) => (r.x, full_y, r.width, full_h / 2.0),
                    (SplitDirection::Horizontal, false) => {
                        (r.x, full_y + full_h / 2.0, r.width, full_h / 2.0)
                    }
                }
            })
        }
        DropZone::TabReorder(gid, _idx) => group_rects
            .iter()
            .find(|(g, _)| *g == gid)
            .map(|(_, r)| (r.x, r.y - eff_tbh(gid), r.width, line_height)),
        DropZone::None => None,
    };

    // Draw the highlight rectangle.
    if let Some((hx, hy, hw, hh)) = highlight {
        let (cr_r, cr_g, cr_b) = theme.cursor.to_cairo();
        cr.set_source_rgba(cr_r, cr_g, cr_b, 0.15);
        cr.rectangle(hx, hy, hw, hh);
        cr.fill().ok();
        cr.set_source_rgba(cr_r, cr_g, cr_b, 0.5);
        cr.set_line_width(2.0);
        cr.rectangle(hx, hy, hw, hh);
        cr.stroke().ok();
    }

    // Draw a small ghost label near the cursor.
    if let (Some((mx, my)), Some(ref drag)) = (engine.tab_drag_mouse, &engine.tab_drag) {
        let label = &drag.tab_name;
        if !label.is_empty() {
            layout.set_text(label);
            let (tw, th) = layout.pixel_size();
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
            pangocairo::show_layout(cr, layout);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_tab_bar(
    cr: &Context,
    layout: &pango::Layout,
    theme: &Theme,
    tabs: &[TabInfo],
    width: f64,
    line_height: f64,
    y_offset: f64,
    show_split_btn: bool,
    hovered_close_tab: Option<usize>,
    diff_toolbar: Option<&render::DiffToolbarData>,
    tab_scroll_offset: usize,
    accent_color: Option<render::Color>,
) -> TabBarDrawResult {
    // Tab row is taller than line_height for vertical padding.
    let tab_row_height = (line_height * 1.6).ceil();
    let text_y_offset = y_offset + (tab_row_height - line_height) / 2.0;

    // Tab bar background
    let (r, g, b) = theme.tab_bar_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(0.0, y_offset, width, tab_row_height);
    cr.fill().ok();

    // Clear any leftover Pango attributes (e.g. syntax highlighting from draw_window).
    layout.set_attributes(None);

    // Use sans-serif UI font for tabs (like VSCode)
    let saved_font = layout.font_description().unwrap_or_default();
    let ui_font_desc = FontDescription::from_string(UI_FONT);
    layout.set_font_description(Some(&ui_font_desc));

    let normal_font = ui_font_desc.clone();
    let mut italic_font = normal_font.clone();
    italic_font.set_style(pango::Style::Italic);

    // Measure both split buttons so tabs don't overlap them.
    let btn_right_s = format!(" {} ", icons::SPLIT_RIGHT.nerd);
    let btn_down_s = format!(" {} ", icons::SPLIT_DOWN.nerd);
    let btn_right_text = btn_right_s.as_str();
    let btn_down_text = btn_down_s.as_str();
    let (both_btns_px, btn_right_px) = if show_split_btn {
        layout.set_font_description(Some(&normal_font));
        layout.set_text(btn_right_text);
        let (wr, _) = layout.pixel_size();
        layout.set_text(btn_down_text);
        let (wd, _) = layout.pixel_size();
        (wr as f64 + wd as f64, wr as f64)
    } else {
        (0.0, 0.0)
    };
    // Measure diff toolbar buttons if present.
    let diff_prev_s = format!(" {}", icons::DIFF_PREV.nerd);
    let diff_next_s = format!(" {}", icons::DIFF_NEXT.nerd);
    let diff_fold_s = format!(" {}", icons::DIFF_FOLD.nerd);
    let diff_btn_prev_text = diff_prev_s.as_str();
    let diff_btn_next_text = diff_next_s.as_str();
    let diff_btn_fold_text = diff_fold_s.as_str();
    let (diff_btns_px, diff_label_px) = if let Some(dt) = diff_toolbar {
        layout.set_font_description(Some(&normal_font));
        layout.set_text(diff_btn_prev_text);
        let (wp, _) = layout.pixel_size();
        layout.set_text(diff_btn_next_text);
        let (wn, _) = layout.pixel_size();
        layout.set_text(diff_btn_fold_text);
        let (wf, _) = layout.pixel_size();
        let btns = wp as f64 + wn as f64 + wf as f64;
        let label = if let Some(lbl) = &dt.change_label {
            layout.set_text(&format!(" {lbl}"));
            let (wl, _) = layout.pixel_size();
            wl as f64
        } else {
            0.0
        };
        (btns, label)
    } else {
        (0.0, 0.0)
    };
    let diff_total_px = diff_btns_px + diff_label_px;

    // Measure the action menu button ("…").
    let action_btn_s = " \u{22EF} "; // " ⋯ " (midline ellipsis)
    layout.set_font_description(Some(&normal_font));
    layout.set_text(action_btn_s);
    let (action_w_i, _) = layout.pixel_size();
    let action_btn_px = action_w_i as f64;

    let tab_area_width = width - both_btns_px - diff_total_px - action_btn_px;

    // Measure the close button (×) once for use in every tab.
    layout.set_font_description(Some(&normal_font));
    layout.set_text("×");
    let (close_w_i, _) = layout.pixel_size();
    let close_w = close_w_i as f64;
    // Gap between tab name and ×, and gap between tabs.
    let tab_pad = 14.0; // horizontal padding inside each tab
    let tab_inner_gap = 10.0; // space between name and ×
    let tab_outer_gap = 1.0; // space between tabs

    let mut x = 0.0_f64;
    let effective_tab_area = tab_area_width;

    let mut slot_positions: Vec<(f64, f64)> = Vec::with_capacity(tabs.len());
    // Fill slots for hidden tabs (before scroll offset) with zero-width entries
    // so that slot_positions indices match tab indices.
    for _ in 0..tab_scroll_offset.min(tabs.len()) {
        slot_positions.push((0.0, 0.0));
    }
    for (tab_idx, tab) in tabs.iter().enumerate().skip(tab_scroll_offset) {
        // Use italic font for preview tabs
        if tab.preview {
            layout.set_font_description(Some(&italic_font));
        } else {
            layout.set_font_description(Some(&normal_font));
        }

        layout.set_text(&tab.name);
        let (tab_width, _) = layout.pixel_size();
        let tab_w = tab_width as f64;
        // Total per-tab slot: pad + name + gap + × + pad + outer_gap
        let tab_content_w = tab_pad + tab_w + tab_inner_gap + close_w + tab_pad;
        let slot_w = tab_content_w + tab_outer_gap;

        // Stop drawing tabs if they would overrun the available area.
        if x + slot_w > effective_tab_area {
            break;
        }
        slot_positions.push((x, x + slot_w));

        // Tab background (covers pad + name + gap + × + pad)
        let bg = if tab.active {
            theme.tab_active_bg
        } else {
            theme.tab_bar_bg
        };
        let (br, bg_g, bb) = bg.to_cairo();
        cr.set_source_rgb(br, bg_g, bb);
        cr.rectangle(x, y_offset, tab_content_w, tab_row_height);
        cr.fill().ok();

        // Accent line at top of active tab in focused group.
        if tab.active {
            if let Some(accent) = accent_color {
                let (ar, ag, ab) = accent.to_cairo();
                cr.set_source_rgb(ar, ag, ab);
                cr.rectangle(x, y_offset, tab_content_w, 2.0);
                cr.fill().ok();
            }
        }

        // Tab text — dimmed colours for preview tabs
        cr.move_to(x + tab_pad, text_y_offset);
        let fg = if tab.preview {
            if tab.active {
                theme.tab_preview_active_fg
            } else {
                theme.tab_preview_inactive_fg
            }
        } else if tab.active {
            theme.tab_active_fg
        } else {
            theme.tab_inactive_fg
        };
        let (fr, fg_g, fb) = fg.to_cairo();
        cr.set_source_rgb(fr, fg_g, fb);
        layout.set_font_description(Some(
            &(if tab.preview {
                italic_font.clone()
            } else {
                normal_font.clone()
            }),
        ));
        pangocairo::show_layout(cr, layout);

        // Close (×) button — dim on inactive, matches active fg on the active tab.
        let close_x = x + tab_pad + tab_w + tab_inner_gap;
        let is_close_hovered = hovered_close_tab == Some(tab_idx);
        if is_close_hovered {
            // Draw a subtle rounded background behind the × on hover.
            let pad = 2.0;
            let rx = close_x - pad;
            let ry = text_y_offset + pad;
            let rw = close_w + pad * 2.0;
            let rh = line_height - pad * 2.0;
            let (hr, hg, hb) = theme.foreground.to_cairo();
            cr.set_source_rgba(hr, hg, hb, 0.15);
            let radius = 3.0;
            cr.new_path();
            cr.arc(
                rx + rw - radius,
                ry + radius,
                radius,
                -std::f64::consts::FRAC_PI_2,
                0.0,
            );
            cr.arc(
                rx + rw - radius,
                ry + rh - radius,
                radius,
                0.0,
                std::f64::consts::FRAC_PI_2,
            );
            cr.arc(
                rx + radius,
                ry + rh - radius,
                radius,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::PI,
            );
            cr.arc(
                rx + radius,
                ry + radius,
                radius,
                std::f64::consts::PI,
                3.0 * std::f64::consts::FRAC_PI_2,
            );
            cr.close_path();
            cr.fill().ok();
        }
        // Show ● (modified dot) when dirty and not hovered, × otherwise (VSCode style).
        let close_glyph = if tab.dirty && !is_close_hovered {
            "●"
        } else {
            "×"
        };
        let (xr, xg, xb) = if tab.dirty && !is_close_hovered {
            // White/foreground dot for modified indicator
            theme.foreground.to_cairo()
        } else if is_close_hovered {
            theme.foreground.to_cairo()
        } else if tab.active {
            theme.tab_inactive_fg.to_cairo()
        } else {
            theme.separator.to_cairo()
        };
        cr.set_source_rgb(xr, xg, xb);
        layout.set_font_description(Some(&normal_font));
        layout.set_text(close_glyph);
        cr.move_to(close_x, text_y_offset);
        pangocairo::show_layout(cr, layout);

        x += slot_w;
    }

    // Draw diff toolbar buttons (to the left of split buttons).
    let diff_btn_pos: Option<(f64, f64, f64, f64, f64, f64)> = if let Some(dt) = diff_toolbar {
        layout.set_font_description(Some(&normal_font));
        let (fr, fg_g, fb) = theme.tab_inactive_fg.to_cairo();
        let mut dx = width - both_btns_px - diff_total_px - action_btn_px;
        // Change label (e.g. " 2 of 5")
        if let Some(lbl) = &dt.change_label {
            let (fr2, fg2, fb2) = theme.foreground.to_cairo();
            cr.set_source_rgb(fr2, fg2, fb2);
            layout.set_text(&format!(" {lbl}"));
            cr.move_to(dx, text_y_offset);
            pangocairo::show_layout(cr, layout);
            dx += diff_label_px;
        }
        // Prev button
        let prev_start = dx;
        cr.set_source_rgb(fr, fg_g, fb);
        layout.set_text(diff_btn_prev_text);
        cr.move_to(dx, text_y_offset);
        pangocairo::show_layout(cr, layout);
        let (wp, _) = layout.pixel_size();
        dx += wp as f64;
        let prev_end = dx;
        // Next button
        let next_start = dx;
        layout.set_text(diff_btn_next_text);
        cr.move_to(dx, text_y_offset);
        pangocairo::show_layout(cr, layout);
        let (wn, _) = layout.pixel_size();
        dx += wn as f64;
        let next_end = dx;
        // Fold toggle (highlighted when active)
        let fold_start = dx;
        if dt.unchanged_hidden {
            let (ar, ag, ab) = theme.tab_active_fg.to_cairo();
            cr.set_source_rgb(ar, ag, ab);
        }
        layout.set_text(diff_btn_fold_text);
        cr.move_to(dx, text_y_offset);
        pangocairo::show_layout(cr, layout);
        let (wf, _) = layout.pixel_size();
        let fold_end = dx + wf as f64;
        Some((
            prev_start, prev_end, next_start, next_end, fold_start, fold_end,
        ))
    } else {
        None
    };

    // Draw split-right then split-down buttons at the right edge.
    if show_split_btn && both_btns_px > 0.0 {
        layout.set_font_description(Some(&normal_font));
        let (fr, fg_g, fb) = theme.tab_inactive_fg.to_cairo();
        cr.set_source_rgb(fr, fg_g, fb);
        // Split-right button (shifted left to make room for action button)
        layout.set_text(btn_right_text);
        cr.move_to(width - both_btns_px - action_btn_px, text_y_offset);
        pangocairo::show_layout(cr, layout);
        // Split-down button
        layout.set_text(btn_down_text);
        cr.move_to(
            width - both_btns_px - action_btn_px + btn_right_px,
            text_y_offset,
        );
        pangocairo::show_layout(cr, layout);
    }

    let split_btn_info = if show_split_btn && both_btns_px > 0.0 {
        Some((both_btns_px, btn_right_px))
    } else {
        None
    };

    // Measure average character width, then report tab bar width in
    // character-column equivalents so the engine can compute tab fits
    // using char-based tab name widths.
    layout.set_font_description(Some(&normal_font));
    layout.set_text("M");
    let (char_px, _) = layout.pixel_size();
    let char_w = (char_px as f64).max(1.0);
    let available_cols = (effective_tab_area / char_w).floor().max(0.0) as usize;

    // Draw the editor action menu button ("…") at the far right.
    let action_btn_info = {
        layout.set_font_description(Some(&normal_font));
        let (fr, fg_g, fb) = theme.tab_inactive_fg.to_cairo();
        cr.set_source_rgb(fr, fg_g, fb);
        let ax = width - action_btn_px;
        layout.set_text(action_btn_s);
        cr.move_to(ax, text_y_offset);
        pangocairo::show_layout(cr, layout);
        Some((ax, width))
    };

    // Restore original editor font for subsequent rendering
    layout.set_font_description(Some(&saved_font));
    (
        slot_positions,
        diff_btn_pos,
        split_btn_info,
        available_cols,
        action_btn_info,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_breadcrumb_bar(
    cr: &Context,
    layout: &pango::Layout,
    theme: &Theme,
    segments: &[render::BreadcrumbSegment],
    width: f64,
    line_height: f64,
    y_offset: f64,
    focus_active: bool,
    focus_selected: usize,
) {
    // Background
    let (r, g, b) = theme.breadcrumb_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(0.0, y_offset, width, line_height);
    cr.fill().ok();

    let separator = " \u{203A} "; // " › "
    let mut x = 4.0; // small left padding

    for (i, seg) in segments.iter().enumerate() {
        // Separator before all but the first
        if x > 5.0 {
            let (sr, sg, sb) = theme.breadcrumb_fg.to_cairo();
            cr.set_source_rgb(sr, sg, sb);
            layout.set_text(separator);
            cr.move_to(x, y_offset);
            pangocairo::show_layout(cr, layout);
            let (sw, _) = layout.pixel_size();
            x += sw as f64;
        }

        // Measure label width for highlight rect
        layout.set_text(&seg.label);
        let (lw, _) = layout.pixel_size();

        // Draw highlight background for focused segment
        let is_focused = focus_active && i == focus_selected;
        if is_focused {
            let (hr, hg, hb) = theme.breadcrumb_active_fg.to_cairo();
            cr.set_source_rgb(hr, hg, hb);
            cr.rectangle(x - 2.0, y_offset, lw as f64 + 4.0, line_height);
            cr.fill().ok();
        }

        // Segment label
        let fg = if is_focused {
            theme.breadcrumb_bg
        } else if seg.is_last {
            theme.breadcrumb_active_fg
        } else {
            theme.breadcrumb_fg
        };
        let (fr, fg_g, fb) = fg.to_cairo();
        cr.set_source_rgb(fr, fg_g, fb);
        cr.move_to(x, y_offset);
        pangocairo::show_layout(cr, layout);
        x += lw as f64;

        if x > width {
            break;
        }
    }
}

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
    let rect = &rw.rect;

    // Gutter pixel width
    let gutter_width = rw.gutter_char_width as f64 * char_width;

    // Apply horizontal scroll offset
    let h_scroll_offset = rw.scroll_left as f64 * char_width;
    let text_x_offset = rect.x + gutter_width - h_scroll_offset;

    // Window background
    let bg = if rw.show_active_bg {
        theme.active_background
    } else {
        theme.background
    };
    let (br, bg_g, bb) = bg.to_cairo();
    cr.set_source_rgb(br, bg_g, bb);
    cr.rectangle(rect.x, rect.y, rect.width, rect.height);
    cr.fill().ok();

    // Cursorline / Diff / DAP stopped-line background (drawn before selection so selection is on top)
    for (view_idx, rl) in rw.lines.iter().enumerate() {
        let y = rect.y + view_idx as f64 * line_height;
        let bg_color = if rl.is_dap_current {
            Some(theme.dap_stopped_bg)
        } else if let Some(diff_status) = rl.diff_status {
            use crate::core::engine::DiffLine;
            match diff_status {
                DiffLine::Added => Some(theme.diff_added_bg),
                DiffLine::Removed => Some(theme.diff_removed_bg),
                DiffLine::Padding => Some(theme.diff_padding_bg),
                DiffLine::Same => None,
            }
        } else if rl.is_current_line && rw.is_active && rw.cursorline {
            Some(theme.cursorline_bg)
        } else {
            None
        };
        if let Some(color) = bg_color {
            let (dr, dg, db) = color.to_cairo();
            cr.set_source_rgb(dr, dg, db);
            cr.rectangle(rect.x, y, rect.width, line_height);
            cr.fill().ok();
        }
    }

    // Visual selection highlight (drawn before text so text renders on top)
    if let Some(sel) = &rw.selection {
        draw_visual_selection(
            cr,
            layout,
            sel,
            &rw.lines,
            rect,
            line_height,
            rw.scroll_top,
            text_x_offset,
            theme.selection,
            theme.selection_alpha,
        );
    }

    // Extra selections (Ctrl+D multi-cursor word highlights)
    for esel in &rw.extra_selections {
        draw_visual_selection(
            cr,
            layout,
            esel,
            &rw.lines,
            rect,
            line_height,
            rw.scroll_top,
            text_x_offset,
            theme.selection,
            theme.selection_alpha,
        );
    }

    // Yank highlight (brief flash after yank)
    if let Some(yh) = &rw.yank_highlight {
        draw_visual_selection(
            cr,
            layout,
            yh,
            &rw.lines,
            rect,
            line_height,
            rw.scroll_top,
            text_x_offset,
            theme.yank_highlight_bg,
            theme.yank_highlight_alpha,
        );
    }

    // Render gutter (bp marker + git marker + fold indicators + optional line numbers)
    if rw.gutter_char_width > 0 {
        for (view_idx, rl) in rw.lines.iter().enumerate() {
            let y = rect.y + view_idx as f64 * line_height;

            // Track how many left-aligned marker chars have been rendered.
            let mut char_offset = 0usize;

            // Breakpoint column — leftmost when any breakpoints/session active.
            if rw.has_breakpoints {
                let bp_ch: String = rl.gutter_text.chars().take(1).collect();
                let bp_color = if rl.is_dap_current || rl.is_breakpoint {
                    theme.diagnostic_error
                } else {
                    theme.line_number_fg
                };
                layout.set_text(&bp_ch);
                layout.set_attributes(None);
                let (br, bg_c, bb) = bp_color.to_cairo();
                cr.set_source_rgb(br, bg_c, bb);
                cr.move_to(rect.x + 3.0, y);
                pangocairo::show_layout(cr, layout);
                char_offset += 1;
            }

            // Git marker column.
            if rw.has_git_diff {
                let git_ch: String = rl.gutter_text.chars().skip(char_offset).take(1).collect();
                let git_color = match rl.git_diff {
                    Some(GitLineStatus::Added) => theme.git_added,
                    Some(GitLineStatus::Modified) => theme.git_modified,
                    Some(GitLineStatus::Deleted) => theme.git_deleted,
                    None => theme.line_number_fg,
                };
                layout.set_text(&git_ch);
                layout.set_attributes(None);
                let (gr, gg, gb) = git_color.to_cairo();
                cr.set_source_rgb(gr, gg, gb);
                cr.move_to(rect.x + char_offset as f64 * char_width + 3.0, y);
                pangocairo::show_layout(cr, layout);
                char_offset += 1;

                // Fold+numbers portion right-aligned.
                let rest: String = rl.gutter_text.chars().skip(char_offset).collect();
                layout.set_text(&rest);
                layout.set_attributes(None);
            } else if char_offset > 0 {
                // bp column only — rest is fold+numbers.
                let rest: String = rl.gutter_text.chars().skip(char_offset).collect();
                layout.set_text(&rest);
                layout.set_attributes(None);
            } else {
                // No marker columns.
                layout.set_text(&rl.gutter_text);
                layout.set_attributes(None);
            }

            let (num_width, _) = layout.pixel_size();
            let num_x = rect.x + gutter_width - num_width as f64 - char_width + 3.0;

            let num_color = if rw.is_active && rl.is_current_line {
                theme.line_number_active_fg
            } else {
                theme.line_number_fg
            };
            let (nr, ng, nb) = num_color.to_cairo();
            cr.set_source_rgb(nr, ng, nb);
            cr.move_to(num_x, y);
            pangocairo::show_layout(cr, layout);

            // Diagnostic gutter icon (colored dot at leftmost gutter position)
            if let Some(severity) = rw.diagnostic_gutter.get(&rl.line_idx) {
                let diag_color = match severity {
                    DiagnosticSeverity::Error => theme.diagnostic_error,
                    DiagnosticSeverity::Warning => theme.diagnostic_warning,
                    DiagnosticSeverity::Information => theme.diagnostic_info,
                    DiagnosticSeverity::Hint => theme.diagnostic_hint,
                };
                let (dr, dg, db) = diag_color.to_cairo();
                cr.set_source_rgb(dr, dg, db);
                let dot_r = line_height * 0.2;
                let dot_cx = rect.x + 3.0 + dot_r;
                let dot_cy = y + line_height * 0.5;
                cr.arc(dot_cx, dot_cy, dot_r, 0.0, 2.0 * std::f64::consts::PI);
                cr.fill().ok();
            } else if !rl.is_wrap_continuation && rw.code_action_lines.contains(&rl.line_idx) {
                // Code action lightbulb gutter icon
                let (lr, lg, lb) = theme.lightbulb.to_cairo();
                cr.set_source_rgb(lr, lg, lb);
                let bulb_layout = layout.clone();
                bulb_layout.set_text(icons::LIGHTBULB.nerd);
                cr.move_to(rect.x + 1.0, y);
                pangocairo::show_layout(cr, &bulb_layout);
            }
        }
    } // end gutter rendering block

    // Clip text area (excluding gutter)
    cr.save().ok();
    cr.rectangle(
        rect.x + gutter_width,
        rect.y,
        rect.width - gutter_width,
        rect.height,
    );
    cr.clip();

    // Render each visible line
    for (view_idx, rl) in rw.lines.iter().enumerate() {
        let y = rect.y + view_idx as f64 * line_height;

        layout.set_text(&rl.raw_text);

        let attrs = build_pango_attrs(&rl.spans);
        layout.set_attributes(Some(&attrs));

        let (fr, fg_g, fb) = theme.foreground.to_cairo();
        cr.set_source_rgb(fr, fg_g, fb);
        cr.move_to(text_x_offset, y);
        pangocairo::show_layout(cr, layout);

        // Ghost continuation lines — full line drawn in ghost colour.
        if rl.is_ghost_continuation {
            if let Some(ghost) = &rl.ghost_suffix {
                let (gr, gg, gb) = theme.ghost_text_fg.to_cairo();
                cr.set_source_rgb(gr, gg, gb);
                cr.move_to(text_x_offset, y);
                layout.set_text(ghost);
                layout.set_attributes(None);
                pangocairo::show_layout(cr, layout);
            }
        }

        // Inline annotation / virtual text (e.g. git blame)
        if let Some(ann) = &rl.annotation {
            let text_pixel_width = layout.pixel_size().0 as f64;
            let ann_x = text_x_offset + text_pixel_width + char_width * 2.0;
            let (ar, ag, ab) = theme.annotation_fg.to_cairo();
            cr.set_source_rgb(ar, ag, ab);
            cr.move_to(ann_x, y);
            layout.set_text(ann);
            layout.set_attributes(None);
            pangocairo::show_layout(cr, layout);
        }

        // Indent guides: thin vertical lines at each guide column
        if !rl.indent_guides.is_empty() {
            cr.set_line_width(1.0);
            for &guide_col in &rl.indent_guides {
                let is_active = rw.active_indent_col == Some(guide_col);
                let (gr, gg, gb) = if is_active {
                    theme.indent_guide_active_fg.to_cairo()
                } else {
                    theme.indent_guide_fg.to_cairo()
                };
                cr.set_source_rgb(gr, gg, gb);
                let gx = text_x_offset + guide_col as f64 * char_width;
                cr.move_to(gx, y);
                cr.line_to(gx, y + line_height);
                cr.stroke().ok();
            }
        }

        // Bracket match highlighting
        for &(bm_view_line, bm_col) in &rw.bracket_match_positions {
            if bm_view_line == view_idx {
                let (br, bg_c, bb) = theme.bracket_match_bg.to_cairo();
                cr.set_source_rgba(br, bg_c, bb, 0.6);
                let bx = text_x_offset + bm_col as f64 * char_width;
                cr.rectangle(bx, y, char_width, line_height);
                cr.fill().ok();
            }
        }

        // Restore layout to match rendered text (needed for correct
        // index_to_pos when font_scale != 1.0, e.g. markdown headings).
        layout.set_text(&rl.raw_text);
        let line_attrs = build_pango_attrs(&rl.spans);
        layout.set_attributes(Some(&line_attrs));

        // Diagnostic underlines (wavy squiggles)
        for dm in &rl.diagnostics {
            let diag_color = match dm.severity {
                DiagnosticSeverity::Error => theme.diagnostic_error,
                DiagnosticSeverity::Warning => theme.diagnostic_warning,
                DiagnosticSeverity::Information => theme.diagnostic_info,
                DiagnosticSeverity::Hint => theme.diagnostic_hint,
            };
            let (dr, dg, db) = diag_color.to_cairo();
            cr.set_source_rgb(dr, dg, db);
            cr.set_line_width(1.0);

            let start_byte = rl
                .raw_text
                .char_indices()
                .nth(dm.start_col)
                .map(|(i, _)| i)
                .unwrap_or(rl.raw_text.len());
            let end_byte = rl
                .raw_text
                .char_indices()
                .nth(dm.end_col)
                .map(|(i, _)| i)
                .unwrap_or(rl.raw_text.len());

            let start_pos = layout.index_to_pos(start_byte as i32);
            let end_pos = layout.index_to_pos(end_byte as i32);
            let x0 = text_x_offset + start_pos.x() as f64 / pango::SCALE as f64;
            let x1 = text_x_offset + end_pos.x() as f64 / pango::SCALE as f64;
            let underline_y = y + line_height - 2.0;

            // Draw wavy underline
            let wave_h = 1.5;
            let wave_len = 4.0;
            cr.move_to(x0, underline_y);
            let mut wx = x0;
            let mut up = true;
            while wx < x1 {
                let next_x = (wx + wave_len).min(x1);
                let cy = if up {
                    underline_y - wave_h
                } else {
                    underline_y + wave_h
                };
                cr.curve_to(
                    wx + (next_x - wx) * 0.5,
                    cy,
                    wx + (next_x - wx) * 0.5,
                    cy,
                    next_x,
                    underline_y,
                );
                wx = next_x;
                up = !up;
            }
            cr.stroke().ok();
        }

        // Spell error underlines (dotted underline in spell_error color)
        for sm in &rl.spell_errors {
            let (sr, sg, sb) = theme.spell_error.to_cairo();
            cr.set_source_rgb(sr, sg, sb);
            cr.set_line_width(1.0);

            let start_byte = rl
                .raw_text
                .char_indices()
                .nth(sm.start_col)
                .map(|(i, _)| i)
                .unwrap_or(rl.raw_text.len());
            let end_byte = rl
                .raw_text
                .char_indices()
                .nth(sm.end_col)
                .map(|(i, _)| i)
                .unwrap_or(rl.raw_text.len());

            let start_pos = layout.index_to_pos(start_byte as i32);
            let end_pos = layout.index_to_pos(end_byte as i32);
            let x0 = text_x_offset + start_pos.x() as f64 / pango::SCALE as f64;
            let x1 = text_x_offset + end_pos.x() as f64 / pango::SCALE as f64;
            let underline_y = y + line_height - 2.0;

            // Draw dotted underline
            let dot_spacing = 3.0;
            let mut dx = x0;
            while dx < x1 {
                cr.rectangle(dx, underline_y, 1.0, 1.0);
                dx += dot_spacing;
            }
            cr.fill().ok();
        }
    }

    cr.restore().ok();
    // Render cursor
    if let Some((cursor_pos, cursor_shape)) = &rw.cursor {
        if let Some(rl) = rw.lines.get(cursor_pos.view_line) {
            layout.set_text(&rl.raw_text);
            let cursor_attrs = build_pango_attrs(&rl.spans);
            layout.set_attributes(Some(&cursor_attrs));

            // When Ctrl+D selections are active, draw bar at right edge (col+1)
            let render_col = if !rw.extra_selections.is_empty() && *cursor_shape == CursorShape::Bar
            {
                cursor_pos.col + 1
            } else {
                cursor_pos.col
            };
            let byte_offset: usize = rl
                .raw_text
                .char_indices()
                .nth(render_col)
                .map(|(i, _)| i)
                .unwrap_or(rl.raw_text.len());

            let pos = layout.index_to_pos(byte_offset as i32);
            let cursor_x = text_x_offset + pos.x() as f64 / pango::SCALE as f64;
            let char_w = pos.width() as f64 / pango::SCALE as f64;
            let cursor_y = rect.y + cursor_pos.view_line as f64 * line_height;

            let (cr_r, cr_g, cr_b) = theme.cursor.to_cairo();
            let char_w = if char_w > 0.0 {
                char_w
            } else {
                font_metrics.approximate_char_width() as f64 / pango::SCALE as f64
            };
            match cursor_shape {
                CursorShape::Block => {
                    cr.set_source_rgba(cr_r, cr_g, cr_b, theme.cursor_normal_alpha);
                    cr.rectangle(cursor_x, cursor_y, char_w, line_height);
                    cr.fill().ok();
                }
                CursorShape::Bar => {
                    cr.set_source_rgb(cr_r, cr_g, cr_b);
                    cr.rectangle(cursor_x, cursor_y, 2.0, line_height);
                    cr.fill().ok();
                }
                CursorShape::Underline => {
                    cr.set_source_rgb(cr_r, cr_g, cr_b);
                    let bar_h = (line_height * 0.12).max(2.0);
                    cr.rectangle(cursor_x, cursor_y + line_height - bar_h, char_w, bar_h);
                    cr.fill().ok();
                }
            }
        }
    }

    // AI ghost text — draw after cursor so it's clearly a suggestion.
    if let Some((cursor_pos, _)) = &rw.cursor {
        if let Some(rl) = rw.lines.get(cursor_pos.view_line) {
            if let Some(ghost) = &rl.ghost_suffix {
                layout.set_text(&rl.raw_text);
                let ghost_line_attrs = build_pango_attrs(&rl.spans);
                layout.set_attributes(Some(&ghost_line_attrs));
                let byte_offset: usize = rl
                    .raw_text
                    .char_indices()
                    .nth(cursor_pos.col)
                    .map(|(i, _)| i)
                    .unwrap_or(rl.raw_text.len());
                let pos = layout.index_to_pos(byte_offset as i32);
                let ghost_x = text_x_offset + pos.x() as f64 / pango::SCALE as f64;
                let ghost_y = rect.y + cursor_pos.view_line as f64 * line_height;
                let (gr, gg, gb) = theme.ghost_text_fg.to_cairo();
                cr.set_source_rgb(gr, gg, gb);
                cr.move_to(ghost_x, ghost_y);
                layout.set_text(ghost);
                layout.set_attributes(None);
                pangocairo::show_layout(cr, layout);
            }
        }
    }

    // Secondary cursors — same shape as primary cursor (bar in Insert/VSCode, block in Normal).
    let extra_cursor_shape = rw
        .cursor
        .as_ref()
        .map(|(_, s)| *s)
        .unwrap_or(CursorShape::Bar);
    let has_extra_sels = !rw.extra_selections.is_empty();
    let fallback_char_w = font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;
    let (cr_r, cr_g, cr_b) = theme.cursor.to_cairo();
    for extra_pos in &rw.extra_cursors {
        if let Some(rl) = rw.lines.get(extra_pos.view_line) {
            layout.set_text(&rl.raw_text);
            let extra_attrs = build_pango_attrs(&rl.spans);
            layout.set_attributes(Some(&extra_attrs));
            // When Ctrl+D selections are active, draw bar at right edge (col+1)
            let render_col = if has_extra_sels && extra_cursor_shape == CursorShape::Bar {
                extra_pos.col + 1
            } else {
                extra_pos.col
            };
            let byte_offset: usize = rl
                .raw_text
                .char_indices()
                .nth(render_col)
                .map(|(i, _)| i)
                .unwrap_or(rl.raw_text.len());
            let pos = layout.index_to_pos(byte_offset as i32);
            let ex = text_x_offset + pos.x() as f64 / pango::SCALE as f64;
            let ew = {
                let w = pos.width() as f64 / pango::SCALE as f64;
                if w > 0.0 {
                    w
                } else {
                    fallback_char_w
                }
            };
            let ey = rect.y + extra_pos.view_line as f64 * line_height;
            match extra_cursor_shape {
                CursorShape::Bar => {
                    cr.set_source_rgb(cr_r, cr_g, cr_b);
                    cr.rectangle(ex, ey, 2.0, line_height);
                    cr.fill().ok();
                }
                CursorShape::Block => {
                    cr.set_source_rgba(cr_r, cr_g, cr_b, theme.cursor_normal_alpha);
                    cr.rectangle(ex, ey, ew, line_height);
                    cr.fill().ok();
                }
                CursorShape::Underline => {
                    cr.set_source_rgb(cr_r, cr_g, cr_b);
                    let bar_h = (line_height * 0.12).max(2.0);
                    cr.rectangle(ex, ey + line_height - bar_h, ew, bar_h);
                    cr.fill().ok();
                }
            }
        }
    }
}

/// Convert a slice of [`StyledSpan`]s into a Pango [`AttrList`].
pub(super) fn build_pango_attrs(spans: &[StyledSpan]) -> AttrList {
    let attrs = AttrList::new();
    for span in spans {
        let (fr, fg_g, fb) = span.style.fg.to_pango_u16();
        let mut fg_attr = AttrColor::new_foreground(fr, fg_g, fb);
        fg_attr.set_start_index(span.start_byte as u32);
        fg_attr.set_end_index(span.end_byte as u32);
        attrs.insert(fg_attr);

        if let Some(bg) = span.style.bg {
            let (br, bg_g, bb) = bg.to_pango_u16();
            let mut bg_attr = AttrColor::new_background(br, bg_g, bb);
            bg_attr.set_start_index(span.start_byte as u32);
            bg_attr.set_end_index(span.end_byte as u32);
            attrs.insert(bg_attr);
        }
        if span.style.bold {
            let mut w = pango::AttrInt::new_weight(pango::Weight::Bold);
            w.set_start_index(span.start_byte as u32);
            w.set_end_index(span.end_byte as u32);
            attrs.insert(w);
        }
        if span.style.italic {
            let mut s = pango::AttrInt::new_style(pango::Style::Italic);
            s.set_start_index(span.start_byte as u32);
            s.set_end_index(span.end_byte as u32);
            attrs.insert(s);
        }
        if (span.style.font_scale - 1.0).abs() > f64::EPSILON {
            let mut sc = pango::AttrFloat::new_scale(span.style.font_scale);
            sc.set_start_index(span.start_byte as u32);
            sc.set_end_index(span.end_byte as u32);
            attrs.insert(sc);
        }
    }
    attrs
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_visual_selection(
    cr: &Context,
    layout: &pango::Layout,
    sel: &SelectionRange,
    lines: &[render::RenderedLine],
    rect: &WindowRect,
    line_height: f64,
    scroll_top: usize,
    text_x_offset: f64,
    color: render::Color,
    alpha: f64,
) {
    let visible_lines = lines.len();
    let (sr, sg, sb) = color.to_cairo();
    cr.set_source_rgba(sr, sg, sb, alpha);

    match sel.kind {
        SelectionKind::Line => {
            for line_idx in sel.start_line..=sel.end_line {
                if line_idx >= scroll_top && line_idx < scroll_top + visible_lines {
                    let view_idx = line_idx - scroll_top;
                    let y = rect.y + view_idx as f64 * line_height;
                    let highlight_width = rect.width - (text_x_offset - rect.x);
                    cr.rectangle(text_x_offset, y, highlight_width, line_height);
                }
            }
            cr.fill().ok();
        }
        SelectionKind::Char => {
            if sel.start_line == sel.end_line {
                // Single-line selection
                if sel.start_line >= scroll_top && sel.start_line < scroll_top + visible_lines {
                    let view_idx = sel.start_line - scroll_top;
                    let y = rect.y + view_idx as f64 * line_height;
                    let line_text = &lines[view_idx].raw_text;

                    layout.set_text(line_text);
                    layout.set_attributes(None);

                    let start_byte = line_text
                        .char_indices()
                        .nth(sel.start_col)
                        .map(|(i, _)| i)
                        .unwrap_or(line_text.len());
                    let start_pos = layout.index_to_pos(start_byte as i32);
                    let start_x = text_x_offset + start_pos.x() as f64 / pango::SCALE as f64;

                    let end_col = (sel.end_col + 1).min(line_text.chars().count());
                    let end_byte = line_text
                        .char_indices()
                        .nth(end_col)
                        .map(|(i, _)| i)
                        .unwrap_or(line_text.len());
                    let end_pos = layout.index_to_pos(end_byte as i32);
                    let end_x = text_x_offset + end_pos.x() as f64 / pango::SCALE as f64;

                    cr.rectangle(start_x, y, end_x - start_x, line_height);
                    cr.fill().ok();
                }
            } else {
                // Multi-line selection
                for line_idx in sel.start_line..=sel.end_line {
                    if line_idx >= scroll_top && line_idx < scroll_top + visible_lines {
                        let view_idx = line_idx - scroll_top;
                        let y = rect.y + view_idx as f64 * line_height;
                        let line_text = &lines[view_idx].raw_text;

                        layout.set_text(line_text);
                        layout.set_attributes(None);

                        if line_idx == sel.start_line {
                            let start_byte = line_text
                                .char_indices()
                                .nth(sel.start_col)
                                .map(|(i, _)| i)
                                .unwrap_or(line_text.len());
                            let start_pos = layout.index_to_pos(start_byte as i32);
                            let start_x =
                                text_x_offset + start_pos.x() as f64 / pango::SCALE as f64;
                            let (line_width, _) = layout.pixel_size();
                            cr.rectangle(
                                start_x,
                                y,
                                text_x_offset + line_width as f64 - start_x,
                                line_height,
                            );
                            cr.fill().ok();
                        } else if line_idx == sel.end_line {
                            let end_col = (sel.end_col + 1).min(line_text.chars().count());
                            let end_byte = line_text
                                .char_indices()
                                .nth(end_col)
                                .map(|(i, _)| i)
                                .unwrap_or(line_text.len());
                            let end_pos = layout.index_to_pos(end_byte as i32);
                            let end_x = text_x_offset + end_pos.x() as f64 / pango::SCALE as f64;
                            cr.rectangle(text_x_offset, y, end_x - text_x_offset, line_height);
                            cr.fill().ok();
                        } else {
                            let (line_width, _) = layout.pixel_size();
                            cr.rectangle(text_x_offset, y, line_width as f64, line_height);
                            cr.fill().ok();
                        }
                    }
                }
            }
        }
        SelectionKind::Block => {
            for line_idx in sel.start_line..=sel.end_line {
                if line_idx >= scroll_top && line_idx < scroll_top + visible_lines {
                    let view_idx = line_idx - scroll_top;
                    let y = rect.y + view_idx as f64 * line_height;
                    let line_text = &lines[view_idx].raw_text;
                    let line_len = line_text.chars().count();

                    layout.set_text(line_text);
                    layout.set_attributes(None);

                    if sel.start_col < line_len {
                        let start_byte = line_text
                            .char_indices()
                            .nth(sel.start_col)
                            .map(|(i, _)| i)
                            .unwrap_or(line_text.len());
                        let start_pos = layout.index_to_pos(start_byte as i32);
                        let start_x = text_x_offset + start_pos.x() as f64 / pango::SCALE as f64;

                        let block_end_col = (sel.end_col + 1).min(line_len);
                        let end_byte = line_text
                            .char_indices()
                            .nth(block_end_col)
                            .map(|(i, _)| i)
                            .unwrap_or(line_text.len());
                        let end_pos = layout.index_to_pos(end_byte as i32);
                        let end_x = text_x_offset + end_pos.x() as f64 / pango::SCALE as f64;

                        cr.rectangle(start_x, y, end_x - start_x, line_height);
                    }
                }
            }
            cr.fill().ok();
        }
    }
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

pub(super) fn draw_completion_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    line_height: f64,
    char_width: f64,
) {
    let Some(menu) = &screen.completion else {
        return;
    };
    let Some(active_win) = screen
        .windows
        .iter()
        .find(|w| w.window_id == screen.active_window_id)
    else {
        return;
    };
    let Some((cursor_pos, _)) = &active_win.cursor else {
        return;
    };

    // Anchor popup below the cursor cell, to the right of the gutter.
    let gutter_width = active_win.gutter_char_width as f64 * char_width;
    let h_scroll_offset = active_win.scroll_left as f64 * char_width;
    let popup_x =
        active_win.rect.x + gutter_width + cursor_pos.col as f64 * char_width - h_scroll_offset;
    let popup_y = active_win.rect.y + (cursor_pos.view_line + 1) as f64 * line_height;

    let visible = menu.candidates.len().min(10);
    let popup_w = ((menu.max_width + 2) as f64 * char_width).max(100.0);
    let popup_h = visible as f64 * line_height;

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

    // Items
    for (i, candidate) in menu.candidates.iter().enumerate().take(visible) {
        let item_y = popup_y + i as f64 * line_height;

        // Selected row highlight
        if i == menu.selected_idx {
            let (r, g, b) = theme.completion_selected_bg.to_cairo();
            cr.set_source_rgb(r, g, b);
            cr.rectangle(popup_x, item_y, popup_w, line_height);
            cr.fill().ok();
        }

        // Candidate text
        let (r, g, b) = theme.completion_fg.to_cairo();
        cr.set_source_rgb(r, g, b);
        let display = format!(" {}", candidate);
        layout.set_text(&display);
        layout.set_attributes(None);
        cr.move_to(popup_x, item_y);
        pangocairo::show_layout(cr, layout);
    }
}

pub(super) fn draw_hover_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    line_height: f64,
    char_width: f64,
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

    // Position above the anchor line
    let gutter_width = active_win.gutter_char_width as f64 * char_width;
    let h_scroll_offset = active_win.scroll_left as f64 * char_width;
    let anchor_view_line = hover.anchor_line.saturating_sub(active_win.scroll_top);
    let popup_x =
        active_win.rect.x + gutter_width + hover.anchor_col as f64 * char_width - h_scroll_offset;

    // Split text into lines and measure
    let text_lines: Vec<&str> = hover.text.lines().collect();
    let num_lines = text_lines.len().min(20) as f64;
    let max_line_len = text_lines.iter().map(|l| l.len()).max().unwrap_or(10);
    let popup_w = ((max_line_len + 2) as f64 * char_width).max(100.0);
    let popup_h = num_lines * line_height + 4.0;

    // Place above cursor if possible, otherwise below
    let popup_y = if anchor_view_line as f64 * line_height > popup_h {
        active_win.rect.y + anchor_view_line as f64 * line_height - popup_h
    } else {
        active_win.rect.y + (anchor_view_line as f64 + 1.0) * line_height
    };

    // Background
    let (r, g, b) = theme.hover_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border
    let (r, g, b) = theme.hover_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Text
    let (r, g, b) = theme.hover_fg.to_cairo();
    cr.set_source_rgb(r, g, b);
    for (i, text_line) in text_lines.iter().enumerate().take(20) {
        let display = format!(" {}", text_line);
        layout.set_text(&display);
        layout.set_attributes(None);
        cr.move_to(popup_x, popup_y + 2.0 + i as f64 * line_height);
        pangocairo::show_layout(cr, layout);
    }
}

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
) {
    use crate::core::markdown::MdStyle;

    let empty = (None, Vec::new());
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
    let lines = &eh.rendered.lines;
    if lines.is_empty() {
        return empty;
    }

    let padding = 4.0;
    let scroll = eh.scroll_top;

    // Popup width: use a comfortable reading width, clamped to editor area
    let da_width = active_win.rect.x + active_win.rect.width;
    let popup_w = ((eh.popup_width + 2) as f64 * char_width)
        .clamp(100.0, (da_width - active_win.rect.x) * 0.9)
        .min(80.0 * char_width);
    let text_w = popup_w - padding * 2.0;
    let pango_text_w = (text_w * pango::SCALE as f64) as i32;

    // Pre-compute wrapped height of each logical line using Pango word wrap
    let mut line_heights: Vec<f64> = Vec::with_capacity(lines.len());
    for text_line in lines {
        let display = format!(" {}", text_line);
        layout.set_text(&display);
        layout.set_width(pango_text_w);
        layout.set_wrap(pango::WrapMode::WordChar);
        let (_pw, ph) = layout.pixel_size();
        line_heights.push(ph as f64);
    }
    layout.set_width(-1); // reset for later use

    // Determine which lines are visible within a max pixel height
    let max_popup_content_h = 20.0 * line_height;
    let total_content_h: f64 = line_heights.iter().sum();
    let can_scroll = total_content_h > max_popup_content_h;
    let scrollbar_w = if can_scroll { char_width } else { 0.0 };
    let popup_w = popup_w + scrollbar_w;

    // Calculate visible content height (lines from scroll onward, capped)
    let mut visible_content_h = 0.0;
    let mut visible_end = scroll;
    for (i, h) in line_heights.iter().enumerate().skip(scroll) {
        let h = *h;
        if visible_content_h + h > max_popup_content_h && visible_end > scroll {
            break;
        }
        visible_content_h += h;
        visible_end = i + 1;
    }
    visible_content_h = visible_content_h.min(max_popup_content_h);

    let focus_bar_h = if eh.has_focus { line_height } else { 0.0 };
    let popup_h = visible_content_h + padding * 2.0 + focus_bar_h;

    let gutter_width = active_win.gutter_char_width as f64 * char_width;
    // Use frozen scroll offsets so the popup stays fixed on screen
    let h_scroll_offset = eh.frozen_scroll_left as f64 * char_width;
    let anchor_view_line = eh.anchor_line.saturating_sub(eh.frozen_scroll_top);
    let mut popup_x =
        active_win.rect.x + gutter_width + eh.anchor_col as f64 * char_width - h_scroll_offset;

    // Prefer above the word (like VSCode); below only near the top
    let space_above = anchor_view_line as f64 * line_height;
    let space_below = active_win.rect.height - (anchor_view_line as f64 + 1.0) * line_height;
    let popup_y = if space_above >= popup_h {
        active_win.rect.y + anchor_view_line as f64 * line_height - popup_h
    } else if space_below >= popup_h {
        active_win.rect.y + (anchor_view_line as f64 + 1.0) * line_height
    } else {
        // Neither fits perfectly — use the larger space
        if space_above >= space_below {
            (active_win.rect.y + anchor_view_line as f64 * line_height - popup_h).max(0.0)
        } else {
            active_win.rect.y + (anchor_view_line as f64 + 1.0) * line_height
        }
    };
    // Keep popup on-screen horizontally
    if popup_x + popup_w > da_width {
        popup_x = (da_width - popup_w).max(active_win.rect.x);
    }

    // Background
    let (r, g, b) = theme.hover_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border — use link color when focused to indicate keyboard mode
    let border_color = if eh.has_focus {
        theme.md_link
    } else {
        theme.hover_border
    };
    let (r, g, b) = border_color.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(if eh.has_focus { 2.0 } else { 1.0 });
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Clip rendering to popup bounds so text doesn't spill outside
    cr.save().ok();
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.clip();

    // Render styled markdown lines with word wrapping
    let mut link_rects: Vec<(f64, f64, f64, f64, String)> = Vec::new();
    let mut y_offset = 0.0;
    for (li, text_line) in lines.iter().enumerate().take(visible_end).skip(scroll) {
        let display = format!(" {}", text_line);
        let actual_line = li;
        let line_spans = eh.rendered.spans.get(actual_line);
        let code_hl = eh.rendered.code_highlights.get(actual_line);
        let has_code_hl = code_hl.is_some_and(|h| !h.is_empty());

        let attrs = pango::AttrList::new();
        if has_code_hl {
            // Use tree-sitter syntax highlighting for code block lines.
            for hl in code_hl.unwrap() {
                let start = hl.start_byte as u32 + 1; // +1 for leading space
                let end = hl.end_byte as u32 + 1;
                let color = theme.scope_color(&hl.scope);
                let (r, g, b) = color.to_pango_u16();
                let mut attr = pango::AttrColor::new_foreground(r, g, b);
                attr.set_start_index(start);
                attr.set_end_index(end);
                attrs.insert(attr);
            }
        } else if let Some(spans) = line_spans {
            for sp in spans {
                let start = sp.start_byte as u32 + 1; // +1 for leading space
                let end = sp.end_byte as u32 + 1;
                match sp.style {
                    MdStyle::Heading(1) => {
                        let (r, g, b) = theme.md_heading1.to_pango_u16();
                        let mut attr = pango::AttrColor::new_foreground(r, g, b);
                        attr.set_start_index(start);
                        attr.set_end_index(end);
                        attrs.insert(attr);
                        let mut attr = pango::AttrInt::new_weight(pango::Weight::Bold);
                        attr.set_start_index(start);
                        attr.set_end_index(end);
                        attrs.insert(attr);
                    }
                    MdStyle::Heading(2) => {
                        let (r, g, b) = theme.md_heading2.to_pango_u16();
                        let mut attr = pango::AttrColor::new_foreground(r, g, b);
                        attr.set_start_index(start);
                        attr.set_end_index(end);
                        attrs.insert(attr);
                        let mut attr = pango::AttrInt::new_weight(pango::Weight::Bold);
                        attr.set_start_index(start);
                        attr.set_end_index(end);
                        attrs.insert(attr);
                    }
                    MdStyle::Heading(_) => {
                        let (r, g, b) = theme.md_heading3.to_pango_u16();
                        let mut attr = pango::AttrColor::new_foreground(r, g, b);
                        attr.set_start_index(start);
                        attr.set_end_index(end);
                        attrs.insert(attr);
                    }
                    MdStyle::Bold | MdStyle::BoldItalic => {
                        let mut attr = pango::AttrInt::new_weight(pango::Weight::Bold);
                        attr.set_start_index(start);
                        attr.set_end_index(end);
                        attrs.insert(attr);
                    }
                    MdStyle::Code | MdStyle::CodeBlock => {
                        let (r, g, b) = theme.md_code.to_pango_u16();
                        let mut attr = pango::AttrColor::new_foreground(r, g, b);
                        attr.set_start_index(start);
                        attr.set_end_index(end);
                        attrs.insert(attr);
                    }
                    MdStyle::Link | MdStyle::LinkUrl => {
                        let (r, g, b) = theme.md_link.to_pango_u16();
                        let mut attr = pango::AttrColor::new_foreground(r, g, b);
                        attr.set_start_index(start);
                        attr.set_end_index(end);
                        attrs.insert(attr);
                        // Underline focused link
                        if eh.has_focus {
                            if let Some(focused) = eh.focused_link {
                                if let Some(&(link_line, sb, eb, _)) = eh.links.get(focused) {
                                    if link_line == actual_line
                                        && sp.start_byte >= sb
                                        && sp.end_byte <= eb
                                    {
                                        let mut attr =
                                            pango::AttrInt::new_underline(pango::Underline::Single);
                                        attr.set_start_index(start);
                                        attr.set_end_index(end);
                                        attrs.insert(attr);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Selection highlight via Pango background attribute
        if let Some((sl, sc, el, ec)) = eh.selection {
            let line_chars: Vec<char> = text_line.chars().collect();
            let in_sel = if sl == el {
                actual_line == sl
            } else {
                actual_line >= sl && actual_line <= el
            };
            if in_sel {
                let sel_start_char = if actual_line == sl { sc } else { 0 };
                let sel_end_char = if actual_line == el {
                    ec
                } else {
                    line_chars.len()
                };
                if sel_start_char < sel_end_char && sel_start_char < line_chars.len() {
                    // Convert char offsets to byte offsets in text_line
                    let byte_start: usize = line_chars[..sel_start_char]
                        .iter()
                        .map(|c| c.len_utf8())
                        .sum();
                    let byte_end: usize = line_chars[..sel_end_char.min(line_chars.len())]
                        .iter()
                        .map(|c| c.len_utf8())
                        .sum();
                    // +1 for the leading space in `display`
                    let (sr, sg, sb) = theme.selection.to_pango_u16();
                    let mut bg_attr = pango::AttrColor::new_background(sr, sg, sb);
                    bg_attr.set_start_index((byte_start + 1) as u32);
                    bg_attr.set_end_index((byte_end + 1) as u32);
                    attrs.insert(bg_attr);
                }
            }
        }

        layout.set_text(&display);
        layout.set_attributes(Some(&attrs));
        layout.set_width(pango_text_w);
        layout.set_wrap(pango::WrapMode::WordChar);
        let (r, g, b) = theme.hover_fg.to_cairo();
        cr.set_source_rgb(r, g, b);
        let line_draw_x = popup_x + padding;
        let line_draw_y = popup_y + padding + y_offset;
        cr.move_to(line_draw_x, line_draw_y);
        pangocairo::show_layout(cr, layout);

        // Compute pixel rects for any clickable links on this line.
        for (link_line, sb, eb, url) in &eh.links {
            if *link_line == actual_line {
                // +1 for the leading space added to `display`
                let start_idx = (*sb + 1) as i32;
                let end_idx = (*eb + 1) as i32;
                let start_rect = layout.index_to_pos(start_idx);
                let end_rect = layout.index_to_pos(end_idx.saturating_sub(1));
                let lx = line_draw_x + start_rect.x() as f64 / pango::SCALE as f64;
                let ly = line_draw_y + start_rect.y() as f64 / pango::SCALE as f64;
                let lw =
                    (end_rect.x() + end_rect.width() - start_rect.x()) as f64 / pango::SCALE as f64;
                let lh = start_rect.height() as f64 / pango::SCALE as f64;
                link_rects.push((lx, ly, lw.max(char_width), lh, url.clone()));
            }
        }

        let (_pw, ph) = layout.pixel_size();
        y_offset += ph as f64;
    }
    layout.set_width(-1);
    layout.set_attributes(None);

    // Scrollbar when content overflows
    if can_scroll {
        let track_h = visible_content_h;
        let visible_ratio = visible_content_h / total_content_h;
        let thumb_h = (track_h * visible_ratio).max(line_height);
        let scroll_range_h: f64 = line_heights.iter().take(scroll).sum();
        let thumb_top = if total_content_h > visible_content_h {
            scroll_range_h / (total_content_h - visible_content_h) * (track_h - thumb_h)
        } else {
            0.0
        };
        let sb_x = popup_x + popup_w - char_width;
        // Track background
        let (r, g, b) = theme.hover_border.to_cairo();
        cr.set_source_rgba(r, g, b, 0.2);
        cr.rectangle(sb_x, popup_y + padding, char_width, track_h);
        cr.fill().ok();
        // Thumb
        cr.set_source_rgba(r, g, b, 0.6);
        cr.rectangle(sb_x, popup_y + padding + thumb_top, char_width, thumb_h);
        cr.fill().ok();
    }

    // Focus indicator
    if eh.has_focus {
        let indicator = "y:copy  Tab:links  Esc:close";
        let (r, g, b) = theme.line_number_fg.to_cairo();
        cr.set_source_rgb(r, g, b);
        layout.set_text(indicator);
        layout.set_attributes(None);
        cr.move_to(popup_x + padding, popup_y + popup_h - line_height - 2.0);
        pangocairo::show_layout(cr, layout);
    }

    // Restore clip
    cr.restore().ok();

    (Some((popup_x, popup_y, popup_w, popup_h)), link_rects)
}

pub(super) fn draw_diff_peek_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    line_height: f64,
    char_width: f64,
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

    // Dimensions.
    let max_line_len = peek.hunk_lines.iter().map(|l| l.len()).max().unwrap_or(10);
    let action_bar_lines = 1;
    let num_lines = (peek.hunk_lines.len() + action_bar_lines).min(30);
    let popup_w = ((max_line_len + 4) as f64 * char_width).max(200.0);
    let popup_h = num_lines as f64 * line_height + 6.0;

    // Position below the anchor line.
    let popup_x = active_win.rect.x + gutter_width;
    let popup_y = active_win.rect.y + (anchor_view_line as f64 + 1.0) * line_height;

    // Background.
    let (r, g, b) = theme.hover_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border.
    let (r, g, b) = theme.hover_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Diff lines with color coding.
    for (i, hline) in peek.hunk_lines.iter().enumerate().take(29) {
        let (r, g, b) = if hline.starts_with('+') {
            theme.git_added.to_cairo()
        } else if hline.starts_with('-') {
            theme.git_deleted.to_cairo()
        } else {
            theme.hover_fg.to_cairo()
        };
        cr.set_source_rgb(r, g, b);
        let display = format!(" {}", hline);
        layout.set_text(&display);
        layout.set_attributes(None);
        cr.move_to(popup_x, popup_y + 2.0 + i as f64 * line_height);
        pangocairo::show_layout(cr, layout);
    }

    // Action bar at bottom.
    let action_y = popup_y + 2.0 + peek.hunk_lines.len().min(29) as f64 * line_height;
    let labels = ["[s] Stage", "[r] Revert", "[q] Close"];
    let mut ax = popup_x + char_width;
    let (r, g, b) = theme.hover_fg.to_cairo();
    for label in &labels {
        cr.set_source_rgb(r, g, b);
        layout.set_text(label);
        layout.set_attributes(None);
        cr.move_to(ax, action_y);
        pangocairo::show_layout(cr, layout);
        ax += (label.len() as f64 + 2.0) * char_width;
    }
}

pub(super) fn draw_signature_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    line_height: f64,
    char_width: f64,
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
    let popup_x =
        active_win.rect.x + gutter_width + sig.anchor_col as f64 * char_width - h_scroll_offset;

    let popup_w = ((sig.label.len() + 4) as f64 * char_width).max(120.0);
    let popup_h = line_height + 4.0;

    // Place above the cursor if space allows, otherwise below.
    let popup_y = if anchor_view_line as f64 * line_height > popup_h {
        active_win.rect.y + anchor_view_line as f64 * line_height - popup_h
    } else {
        active_win.rect.y + (anchor_view_line as f64 + 1.0) * line_height
    };

    // Background
    let (r, g, b) = theme.hover_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border
    let (r, g, b) = theme.hover_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Build Pango attr list: active parameter in keyword color, rest in hover_fg.
    let display = format!(" {}", sig.label);
    let offset = 1usize; // accounts for the leading space

    let attrs = AttrList::new();
    let (fr, fg_g, fb) = theme.hover_fg.to_pango_u16();
    let mut base_attr = AttrColor::new_foreground(fr, fg_g, fb);
    base_attr.set_start_index(0);
    base_attr.set_end_index(display.len() as u32);
    attrs.insert(base_attr);

    if let Some(idx) = sig.active_param {
        if let Some(&(start, end)) = sig.params.get(idx) {
            let (kr, kg, kb) = theme.keyword.to_pango_u16();
            let mut kw_attr = AttrColor::new_foreground(kr, kg, kb);
            kw_attr.set_start_index((offset + start) as u32);
            kw_attr.set_end_index((offset + end) as u32);
            attrs.insert(kw_attr);
        }
    }

    layout.set_text(&display);
    layout.set_attributes(Some(&attrs));
    cr.move_to(popup_x, popup_y + 2.0);
    pangocairo::show_layout(cr, layout);
    layout.set_attributes(None);
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
) {
    let Some(picker) = &screen.picker else {
        return;
    };

    let has_preview = picker.preview.is_some();

    // Size adapts based on whether we have a preview pane
    let popup_w = if has_preview {
        (editor_width * 0.8).max(600.0)
    } else {
        (editor_width * 0.55).max(500.0)
    };
    let popup_h = if has_preview {
        (editor_height * 0.65).max(400.0)
    } else {
        (editor_height * 0.60).max(350.0)
    };

    let popup_x = (editor_width - popup_w) / 2.0;
    let popup_y = (editor_height - popup_h) / 2.0;

    // Background
    let (r, g, b) = theme.fuzzy_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border
    let (r, g, b) = theme.fuzzy_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Title row
    let title = format!(
        "  {}  ({}/{})",
        picker.title,
        picker.items.len(),
        picker.total_count
    );
    let (r, g, b) = theme.fuzzy_title_fg.to_cairo();
    cr.set_source_rgb(r, g, b);
    layout.set_text(&title);
    layout.set_attributes(None);
    cr.move_to(popup_x, popup_y);
    pangocairo::show_layout(cr, layout);

    // Query row
    let query_text = format!("> {}_", picker.query);
    let (r, g, b) = theme.fuzzy_query_fg.to_cairo();
    cr.set_source_rgb(r, g, b);
    layout.set_text(&query_text);
    layout.set_attributes(None);
    cr.move_to(popup_x, popup_y + line_height);
    pangocairo::show_layout(cr, layout);

    // Horizontal separator
    let sep_y = popup_y + 2.0 * line_height;
    let (r, g, b) = theme.fuzzy_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.move_to(popup_x, sep_y);
    cr.line_to(popup_x + popup_w, sep_y);
    cr.stroke().ok();

    // Two-pane layout
    let left_pane_w = if has_preview { popup_w * 0.4 } else { popup_w };

    if has_preview {
        // Vertical separator
        cr.move_to(popup_x + left_pane_w, sep_y);
        cr.line_to(popup_x + left_pane_w, popup_y + popup_h);
        cr.stroke().ok();
    }

    let rows_area_h = popup_h - 2.0 * line_height - 2.0;
    let visible_rows = (rows_area_h / line_height) as usize;

    // Scrollbar geometry (single-pane only)
    let total_items = picker.items.len();
    let has_scrollbar = !has_preview && total_items > visible_rows;
    const SB_W: f64 = 6.0;
    let content_w = if has_scrollbar {
        left_pane_w - SB_W
    } else {
        left_pane_w
    };

    // Left pane: result rows — clipped
    cr.save().ok();
    cr.rectangle(popup_x, sep_y, content_w, rows_area_h + 2.0);
    cr.clip();

    let has_tree = picker.items.iter().any(|i| i.expandable || i.depth > 0);

    for i in 0..visible_rows {
        let result_idx = picker.scroll_top + i;
        let Some(item) = picker.items.get(result_idx) else {
            break;
        };
        let item_y = sep_y + 1.0 + i as f64 * line_height;
        let is_selected = result_idx == picker.selected_idx;

        // Selected row highlight
        if is_selected {
            let (r, g, b) = theme.fuzzy_selected_bg.to_cairo();
            cr.set_source_rgb(r, g, b);
            cr.rectangle(popup_x, item_y, content_w, line_height);
            cr.fill().ok();
        }

        // Build pango attributed string with match highlighting
        let sel_prefix = if is_selected { "▶ " } else { "  " };
        let indent: String = "  ".repeat(item.depth);
        let arrow = if item.expandable {
            if item.expanded {
                "▼ "
            } else {
                "▷ "
            }
        } else if has_tree {
            "  "
        } else {
            ""
        };
        let prefix = format!("{}{}{}", sel_prefix, indent, arrow);
        let full_text = format!("{}{}", prefix, item.display);
        let prefix_bytes = prefix.len();

        // Create attributes for match highlighting
        let attr_list = pango::AttrList::new();

        // Default color
        let (r, g, b) = theme.fuzzy_fg.to_cairo();
        let mut attr_fg = pango::AttrColor::new_foreground(
            (r * 65535.0) as u16,
            (g * 65535.0) as u16,
            (b * 65535.0) as u16,
        );
        attr_fg.set_start_index(0);
        attr_fg.set_end_index(full_text.len() as u32);
        attr_list.insert(attr_fg);

        // Match highlight color for matched positions
        if !item.match_positions.is_empty() {
            let (mr, mg, mb) = theme.fuzzy_match_fg.to_cairo();
            for &pos in &item.match_positions {
                // pos is a byte index into the display text; offset by prefix length
                let start = prefix_bytes + pos;
                if start < full_text.len() {
                    // Find the byte length of the char at this position
                    let end = start
                        + full_text[start..]
                            .chars()
                            .next()
                            .map(|c| c.len_utf8())
                            .unwrap_or(1);
                    let mut attr_match = pango::AttrColor::new_foreground(
                        (mr * 65535.0) as u16,
                        (mg * 65535.0) as u16,
                        (mb * 65535.0) as u16,
                    );
                    attr_match.set_start_index(start as u32);
                    attr_match.set_end_index(end as u32);
                    attr_list.insert(attr_match);
                }
            }
        }

        layout.set_text(&full_text);
        layout.set_attributes(Some(&attr_list));
        cr.move_to(popup_x, item_y);
        pangocairo::show_layout(cr, layout);

        // Right-aligned detail (shortcut) — single-pane only
        if !has_preview {
            if let Some(ref detail) = item.detail {
                if !detail.is_empty() {
                    let detail_text = format!("{}  ", detail);
                    let (r, g, b) = theme.fuzzy_border.to_cairo();
                    cr.set_source_rgb(r, g, b);
                    layout.set_text(&detail_text);
                    layout.set_attributes(None);
                    let (sc_w, _) = layout.pixel_size();
                    cr.move_to(popup_x + content_w - sc_w as f64, item_y);
                    pangocairo::show_layout(cr, layout);
                }
            }
        }
    }

    cr.restore().ok();

    // Right pane: preview lines (two-pane only)
    if has_preview {
        let right_pane_x = popup_x + left_pane_w + 1.0;
        let right_pane_w = popup_w - left_pane_w - 1.0;

        cr.save().ok();
        cr.rectangle(right_pane_x, sep_y, right_pane_w, rows_area_h + 2.0);
        cr.clip();

        if let Some(ref preview) = picker.preview {
            for (i, (lineno, text, is_match)) in preview.iter().enumerate().take(visible_rows) {
                let item_y = sep_y + 1.0 + i as f64 * line_height;
                let preview_text = format!("{:4}: {}", lineno, text);

                if *is_match {
                    let (r, g, b) = theme.fuzzy_title_fg.to_cairo();
                    cr.set_source_rgb(r, g, b);
                } else {
                    let (r, g, b) = theme.fuzzy_fg.to_cairo();
                    cr.set_source_rgb(r, g, b);
                }

                layout.set_text(&preview_text);
                layout.set_attributes(None);
                cr.move_to(right_pane_x, item_y);
                pangocairo::show_layout(cr, layout);
            }
        }

        cr.restore().ok();
    }

    // Scrollbar (single-pane only)
    if has_scrollbar && visible_rows > 0 {
        let sb_x = popup_x + popup_w - SB_W;
        let sb_track_y = sep_y + 1.0;
        let sb_track_h = rows_area_h;

        let (tr, tg, tb) = theme.fuzzy_bg.to_cairo();
        cr.set_source_rgb(tr * 0.7, tg * 0.7, tb * 0.7);
        cr.rectangle(sb_x, sb_track_y, SB_W, sb_track_h);
        cr.fill().ok();

        let thumb_ratio = visible_rows as f64 / total_items as f64;
        let thumb_h = (sb_track_h * thumb_ratio).max(8.0);
        let max_scroll = total_items.saturating_sub(visible_rows) as f64;
        let scroll_frac = if max_scroll > 0.0 {
            picker.scroll_top as f64 / max_scroll
        } else {
            0.0
        };
        let thumb_y = sb_track_y + scroll_frac * (sb_track_h - thumb_h);

        let (br, bg_c, bb) = theme.fuzzy_border.to_cairo();
        cr.set_source_rgb(br, bg_c, bb);
        cr.rectangle(sb_x + 1.0, thumb_y, SB_W - 2.0, thumb_h);
        cr.fill().ok();
    }
}

/// Draw the tab switcher popup (Ctrl+Tab MRU list).
pub(super) fn draw_tab_switcher_popup(
    cr: &Context,
    screen: &render::ScreenLayout,
    theme: &Theme,
    editor_width: f64,
    editor_height: f64,
    line_height: f64,
) {
    let Some(ts) = &screen.tab_switcher else {
        return;
    };
    if ts.items.is_empty() {
        return;
    }

    let item_count = ts.items.len();
    let max_visible = ((editor_height * 0.6) / line_height) as usize;
    let visible = item_count.min(max_visible).min(20);

    let popup_w = (editor_width * 0.40).clamp(350.0, 600.0);
    let popup_h = (visible as f64 + 1.5) * line_height; // items + title

    let popup_x = (editor_width - popup_w) / 2.0;
    let popup_y = (editor_height - popup_h) / 2.0;

    // Background
    let (r, g, b) = theme.fuzzy_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border
    let (r, g, b) = theme.fuzzy_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Use sans-serif UI font (same as VSCode tabs)
    let pango_ctx = pangocairo::create_context(cr);
    let ui_font_desc = FontDescription::from_string(UI_FONT);
    let layout = pango::Layout::new(&pango_ctx);
    layout.set_font_description(Some(&ui_font_desc));

    // Title
    let title = " Open Tabs";
    let (r, g, b) = theme.fuzzy_title_fg.to_cairo();
    cr.set_source_rgb(r, g, b);
    layout.set_text(title);
    layout.set_attributes(None);
    cr.move_to(popup_x + 4.0, popup_y);
    pangocairo::show_layout(cr, &layout);

    // Scroll offset
    let scroll = if ts.selected_idx >= visible {
        ts.selected_idx - visible + 1
    } else {
        0
    };

    let items_y = popup_y + line_height * 1.2;
    for i in 0..visible {
        let item_idx = scroll + i;
        if item_idx >= item_count {
            break;
        }
        let item_y = items_y + i as f64 * line_height;
        let is_selected = item_idx == ts.selected_idx;

        if is_selected {
            let (r, g, b) = theme.fuzzy_selected_bg.to_cairo();
            cr.set_source_rgb(r, g, b);
            cr.rectangle(popup_x + 1.0, item_y, popup_w - 2.0, line_height);
            cr.fill().ok();
        }

        let (name, path, dirty) = &ts.items[item_idx];
        let dirty_mark = if *dirty { " \u{25cf}" } else { "" }; // ●
        let prefix = if is_selected { "\u{25b6} " } else { "  " }; // ▶
        let label = format!("{}{}{}", prefix, name, dirty_mark);

        let (r, g, b) = theme.fuzzy_fg.to_cairo();
        cr.set_source_rgb(r, g, b);
        layout.set_text(&label);
        layout.set_attributes(None);
        cr.move_to(popup_x + 4.0, item_y);
        pangocairo::show_layout(cr, &layout);

        // Path right-aligned (dimmed)
        if !path.is_empty() {
            let (r, g, b) = theme.fuzzy_border.to_cairo();
            cr.set_source_rgb(r, g, b);
            layout.set_text(path);
            layout.set_attributes(None);
            let (pw, _) = layout.pixel_size();
            cr.move_to(popup_x + popup_w - pw as f64 - 8.0, item_y);
            pangocairo::show_layout(cr, &layout);
        }
    }
}

/// Draw a modal dialog popup centered on the screen.
#[allow(clippy::too_many_arguments)]
/// Returns button hit-rects `(x, y, w, h)` for each dialog button.
pub(super) fn draw_dialog_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    editor_width: f64,
    editor_height: f64,
    line_height: f64,
) -> Vec<(f64, f64, f64, f64)> {
    let Some(dialog) = &screen.dialog else {
        return Vec::new();
    };

    let pango_ctx = pangocairo::create_context(cr);
    let ui_font_desc = FontDescription::from_string(UI_FONT);
    let ui_layout = pango::Layout::new(&pango_ctx);
    ui_layout.set_font_description(Some(&ui_font_desc));

    // Measure button widths.
    let mut btn_total_w = 8.0; // padding
    let mut btn_max_w = 0.0f64;
    for (label, _) in &dialog.buttons {
        ui_layout.set_text(&format!("  {}  ", label));
        let (w, _) = ui_layout.pixel_size();
        btn_total_w += w as f64 + 4.0;
        btn_max_w = btn_max_w.max(w as f64 + 4.0);
    }

    // Measure body width.
    let mut body_max_w = 0.0f64;
    for line in &dialog.body {
        layout.set_text(line);
        let (w, _) = layout.pixel_size();
        body_max_w = body_max_w.max(w as f64);
    }

    // Title width.
    ui_layout.set_text(&dialog.title);
    let (title_w, _) = ui_layout.pixel_size();

    let has_input = dialog.input.is_some();
    let input_rows = if has_input { 1.0 } else { 0.0 };
    let effective_btn_w = if dialog.vertical_buttons {
        btn_max_w + 24.0
    } else {
        btn_total_w
    };
    let content_w = body_max_w.max(title_w as f64 + 16.0).max(effective_btn_w);
    let popup_w = (content_w + 32.0).clamp(350.0, editor_width - 40.0);
    let btn_rows = if dialog.vertical_buttons {
        dialog.buttons.len() as f64
    } else {
        1.0
    };
    let popup_h = ((3.0 + dialog.body.len() as f64 + input_rows + btn_rows + 1.0) * line_height)
        .min(editor_height - 40.0);

    let popup_x = (editor_width - popup_w) / 2.0;
    let popup_y = (editor_height - popup_h) / 2.0;

    // Background.
    let (r, g, b) = theme.fuzzy_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border.
    let (r, g, b) = theme.fuzzy_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Title.
    let (r, g, b) = theme.fuzzy_title_fg.to_cairo();
    cr.set_source_rgb(r, g, b);
    ui_layout.set_text(&dialog.title);
    ui_layout.set_attributes(None);
    cr.move_to(popup_x + 12.0, popup_y + line_height * 0.3);
    pangocairo::show_layout(cr, &ui_layout);

    // Body lines.
    let body_y = popup_y + line_height * 1.8;
    let (r, g, b) = theme.fuzzy_fg.to_cairo();
    cr.set_source_rgb(r, g, b);
    for (i, line) in dialog.body.iter().enumerate() {
        layout.set_text(line);
        layout.set_attributes(None);
        cr.move_to(popup_x + 12.0, body_y + i as f64 * line_height);
        pangocairo::show_layout(cr, layout);
    }

    // Input field (if present).
    if let Some(ref input) = dialog.input {
        let input_y = body_y + dialog.body.len() as f64 * line_height + line_height * 0.3;
        // Draw input background.
        let (ibg_r, ibg_g, ibg_b) = theme.completion_bg.to_cairo();
        cr.set_source_rgb(ibg_r, ibg_g, ibg_b);
        cr.rectangle(popup_x + 12.0, input_y, popup_w - 24.0, line_height);
        cr.fill().ok();
        // Draw input border.
        let (br_r, br_g, br_b) = theme.fuzzy_border.to_cairo();
        cr.set_source_rgb(br_r, br_g, br_b);
        cr.rectangle(popup_x + 12.0, input_y, popup_w - 24.0, line_height);
        cr.stroke().ok();
        // Draw input text.
        let (r, g, b) = theme.fuzzy_fg.to_cairo();
        cr.set_source_rgb(r, g, b);
        layout.set_text(&format!(" {}", input.display));
        layout.set_attributes(None);
        let (_, ilh) = layout.pixel_size();
        cr.move_to(popup_x + 14.0, input_y + (line_height - ilh as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
    }

    // Buttons — vertical list or horizontal row.
    let mut rects = Vec::with_capacity(dialog.buttons.len());
    if dialog.vertical_buttons {
        let btn_start_y = popup_y + popup_h - (btn_rows + 0.5) * line_height;
        for (i, (label, is_selected)) in dialog.buttons.iter().enumerate() {
            let by = btn_start_y + i as f64 * line_height;
            let row_w = popup_w - 24.0;
            rects.push((popup_x + 12.0, by, row_w, line_height));

            if *is_selected {
                let (r, g, b) = theme.fuzzy_selected_bg.to_cairo();
                cr.set_source_rgb(r, g, b);
                cr.rectangle(popup_x + 12.0, by, row_w, line_height);
                cr.fill().ok();
            }

            let prefix = if *is_selected { "▸ " } else { "  " };
            let btn_text = format!("{}{}", prefix, label);
            let (r, g, b) = theme.fuzzy_fg.to_cairo();
            cr.set_source_rgb(r, g, b);
            ui_layout.set_text(&btn_text);
            ui_layout.set_attributes(None);
            cr.move_to(popup_x + 14.0, by);
            pangocairo::show_layout(cr, &ui_layout);
        }
    } else {
        let btn_y = popup_y + popup_h - line_height * 1.5;
        let mut bx = popup_x + 12.0;
        for (label, is_selected) in &dialog.buttons {
            let btn_text = format!("  {}  ", label);
            ui_layout.set_text(&btn_text);
            let (bw, bh) = ui_layout.pixel_size();
            let bw = bw as f64;
            let bh = bh as f64;

            rects.push((bx, btn_y, bw, bh));

            if *is_selected {
                let (r, g, b) = theme.fuzzy_selected_bg.to_cairo();
                cr.set_source_rgb(r, g, b);
                cr.rectangle(bx, btn_y, bw, bh);
                cr.fill().ok();
            }

            let (r, g, b) = theme.fuzzy_fg.to_cairo();
            cr.set_source_rgb(r, g, b);
            ui_layout.set_attributes(None);
            cr.move_to(bx, btn_y);
            pangocairo::show_layout(cr, &ui_layout);

            bx += bw + 4.0;
        }
    }
    rects
}

/// Draw the tab bar for the bottom panel (Terminal / Debug Output).
/// One row high at `(x, y)`, full width `w`.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_bottom_panel_tabs(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    line_height: f64,
    has_terminal: bool,
    has_debug_output: bool,
) {
    let (br, bg, bb) = theme.tab_bar_bg.to_cairo();
    let (fr, fg2, fb) = theme.status_fg.to_cairo();
    let (ar, ag, ab) = theme.tab_active_fg.to_cairo();

    // Background — use tab bar bg to match the editor tab bar.
    cr.set_source_rgb(br, bg, bb);
    cr.rectangle(x, y, w, line_height);
    cr.fill().ok();

    // Thin separator line at the top.
    let (sr, sg, sb) = theme.separator.to_cairo();
    cr.set_source_rgb(sr, sg, sb);
    cr.rectangle(x, y, w, 1.0);
    cr.fill().ok();

    // Use sans-serif UI font (like VSCode panel tabs).
    let saved_font = layout.font_description().unwrap_or_default();
    let ui_font_desc = FontDescription::from_string(UI_FONT);
    layout.set_font_description(Some(&ui_font_desc));
    layout.set_attributes(None);

    let all_tabs: &[(&str, render::BottomPanelKind, bool)] = &[
        ("TERMINAL", render::BottomPanelKind::Terminal, has_terminal),
        (
            "DEBUG CONSOLE",
            render::BottomPanelKind::DebugOutput,
            has_debug_output,
        ),
    ];

    let padding = 12.0;
    let mut cursor_x = x + padding;
    for (label, kind, visible) in all_tabs {
        if !visible {
            continue;
        }
        let is_active = screen.bottom_tabs.active == *kind;
        let (lr, lg, lb) = if is_active {
            (ar, ag, ab)
        } else {
            (fr, fg2, fb)
        };
        cr.set_source_rgb(lr, lg, lb);
        layout.set_text(label);
        cr.move_to(cursor_x, y);
        pangocairo::show_layout(cr, layout);
        let extents = layout.pixel_extents().1;
        let tab_w = extents.width() as f64;
        // Underline the active tab.
        if is_active {
            cr.set_source_rgb(ar, ag, ab);
            cr.rectangle(cursor_x, y + line_height - 2.0, tab_w, 2.0);
            cr.fill().ok();
        }
        cursor_x += tab_w + padding * 2.0;
    }

    // Close button (×) at right edge
    let close_x = x + w - padding - 10.0;
    cr.set_source_rgb(fr, fg2, fb);
    layout.set_text("\u{00d7}"); // ×
    cr.move_to(close_x, y);
    pangocairo::show_layout(cr, layout);

    // Restore the original monospace font.
    layout.set_font_description(Some(&saved_font));
}

/// Draw debug output lines (read-only scrolling log).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_debug_output(
    cr: &Context,
    layout: &pango::Layout,
    output_lines: &[String],
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    line_height: f64,
) {
    let (br, bg_g, bb) = theme.completion_bg.to_cairo();
    cr.set_source_rgb(br, bg_g, bb);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    let (fr, fg, fb) = theme.fuzzy_fg.to_cairo();
    cr.set_source_rgb(fr, fg, fb);
    layout.set_attributes(None);

    let visible_rows = (h / line_height) as usize;
    let start = output_lines.len().saturating_sub(visible_rows);
    for (row, line_text) in output_lines.iter().skip(start).enumerate() {
        let ry = y + row as f64 * line_height;
        let text = format!("  {line_text}");
        layout.set_text(&text);
        cr.move_to(x, ry);
        pangocairo::show_layout(cr, layout);
    }
}

/// Draw the VSCode-style debug sidebar content.
///
/// Shows four sections stacked vertically:
///   • VARIABLES (with ▶/▼ expansion)
///   • WATCH (expressions + values)
///   • CALL STACK (frames, active highlighted)
///   • BREAKPOINTS (file:line list)
///
/// A 2-row header at the top shows the session status and a Run/Stop button.
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
) {
    let sidebar = &screen.debug_sidebar;

    let (bg_r, bg_g, bg_b) = theme.tab_bar_bg.to_cairo();
    let (hdr_r, hdr_g, hdr_b) = theme.status_bg.to_cairo();
    let (hdr_fg_r, hdr_fg_g, hdr_fg_b) = theme.status_fg.to_cairo();
    let (dim_r, dim_g, dim_b) = theme.line_number_fg.to_cairo();
    let (sel_r, sel_g, sel_b) = theme.fuzzy_selected_bg.to_cairo();
    let (act_r, act_g, act_b) = theme.tab_active_fg.to_cairo();

    // Paint sidebar background.
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    layout.set_attributes(None);

    // ── Row 0: header strip ─────────────────────────────────────────────────
    cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
    cr.rectangle(x, y, w, line_height);
    cr.fill().ok();

    let cfg_name = sidebar.launch_config_name.as_deref().unwrap_or("no config");
    let header_text = format!("  {} DEBUG  |  {cfg_name}", icons::DEBUG.nerd);
    cr.set_source_rgb(hdr_fg_r, hdr_fg_g, hdr_fg_b);
    layout.set_text(&header_text);
    cr.move_to(x + 4.0, y);
    pangocairo::show_layout(cr, layout);

    // ── Row 1: Run/Stop button ───────────────────────────────────────────────
    let btn_y = y + line_height;
    cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
    cr.rectangle(x, btn_y, w, line_height);
    cr.fill().ok();

    let continue_label = format!("{}  Continue", icons::DBG_PLAY.nerd);
    let stop_label = format!("{}  Stop", icons::DBG_STOP_ALT.nerd);
    let start_label = format!("{}  Start Debugging", icons::DBG_PLAY.nerd);
    let (btn_label, btn_color) = if sidebar.session_active && sidebar.stopped {
        (continue_label.as_str(), (0.38_f64, 0.73_f64, 0.45_f64))
    } else if sidebar.session_active {
        (stop_label.as_str(), (0.86_f64, 0.27_f64, 0.22_f64))
    } else {
        (start_label.as_str(), (0.38_f64, 0.73_f64, 0.45_f64))
    };
    cr.set_source_rgb(btn_color.0, btn_color.1, btn_color.2);
    layout.set_text(btn_label);
    cr.move_to(x + 8.0, btn_y);
    pangocairo::show_layout(cr, layout);

    // ── Sections with fixed-height allocation + per-section scrolling ──────
    let sections: [(
        &str,
        &[render::DebugSidebarItem],
        render::DebugSidebarSection,
        usize,
    ); 4] = [
        (
            &format!("{} VARIABLES", icons::DBG_VARIABLES.nerd),
            &sidebar.variables,
            render::DebugSidebarSection::Variables,
            0,
        ),
        (
            &format!("{} WATCH", icons::DBG_WATCH.nerd),
            &sidebar.watch,
            render::DebugSidebarSection::Watch,
            1,
        ),
        (
            &format!("{} CALL STACK", icons::DBG_CALL_STACK.nerd),
            &sidebar.frames,
            render::DebugSidebarSection::CallStack,
            2,
        ),
        (
            &format!("{} BREAKPOINTS", icons::DBG_BREAKPOINTS.nerd),
            &sidebar.breakpoints,
            render::DebugSidebarSection::Breakpoints,
            3,
        ),
    ];

    // Compute per-section content heights (equal share of remaining space).
    // Available px after header(1) + button(1) = 2 line_heights.
    // Each section has 1 header row (4 total), so content px = h - 6*line_height.
    let content_px = (h - 6.0 * line_height).max(0.0);
    let sec_content_h = (content_px / 4.0).floor();

    let mut cursor_y = btn_y + line_height;
    let max_y = y + h;

    let (sb_r, sb_g, sb_b) = (0.5_f64, 0.5_f64, 0.5_f64); // scrollbar thumb color

    for (section_label, items, section_kind, sec_idx) in &sections {
        if cursor_y >= max_y {
            break;
        }

        // Section header row.
        let is_active_section = sidebar.active_section == *section_kind;
        let (shr, shg, shb) = if is_active_section {
            (act_r, act_g, act_b)
        } else {
            (hdr_fg_r, hdr_fg_g, hdr_fg_b)
        };
        cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
        cr.rectangle(x, cursor_y, w, line_height);
        cr.fill().ok();
        cr.set_source_rgb(shr, shg, shb);
        layout.set_text(section_label);
        cr.move_to(x + 4.0, cursor_y);
        pangocairo::show_layout(cr, layout);
        cursor_y += line_height;

        let scroll_off = sidebar.scroll_offsets[*sec_idx];
        let sec_height = sidebar.section_heights[*sec_idx] as usize;
        let visible_rows = if sec_height > 0 {
            sec_height
        } else {
            (sec_content_h / line_height).floor() as usize
        };

        // Clip to section content area.
        let section_start_y = cursor_y;
        let section_end_y = (cursor_y + visible_rows as f64 * line_height).min(max_y);

        cr.save().ok();
        cr.rectangle(x, section_start_y, w, section_end_y - section_start_y);
        cr.clip();

        // Render items within the allocated height.
        for row_offset in 0..visible_rows {
            let item_y = section_start_y + row_offset as f64 * line_height;
            if item_y >= section_end_y {
                break;
            }
            let item_idx = scroll_off + row_offset;
            if items.is_empty() && row_offset == 0 {
                // Empty hint.
                cr.set_source_rgb(dim_r, dim_g, dim_b);
                let hint = if sidebar.session_active {
                    "  (empty)"
                } else {
                    "  (not running)"
                };
                layout.set_text(hint);
                cr.move_to(x + 4.0, item_y);
                pangocairo::show_layout(cr, layout);
            } else if item_idx < items.len() {
                let item = &items[item_idx];
                if item.is_selected {
                    cr.set_source_rgb(sel_r, sel_g, sel_b);
                    cr.rectangle(x, item_y, w, line_height);
                    cr.fill().ok();
                    cr.set_source_rgb(hdr_fg_r, hdr_fg_g, hdr_fg_b);
                } else {
                    cr.set_source_rgb(dim_r, dim_g, dim_b);
                }
                let indent_px = item.indent as f64 * 12.0;
                layout.set_text(&item.text);
                cr.move_to(x + 4.0 + indent_px, item_y);
                pangocairo::show_layout(cr, layout);
            }
        }

        // Draw scrollbar if items exceed visible height.
        if items.len() > visible_rows && visible_rows > 0 {
            let sb_w = 4.0_f64;
            let sb_x = x + w - sb_w;
            let track_h = visible_rows as f64 * line_height;
            let total_items = items.len();
            let thumb_h = ((visible_rows as f64 / total_items as f64) * track_h)
                .ceil()
                .max(line_height * 0.5);
            let max_scroll = total_items - visible_rows;
            let thumb_top = if max_scroll > 0 {
                (scroll_off as f64 / max_scroll as f64) * (track_h - thumb_h)
            } else {
                0.0
            };
            // Track background.
            let (st_r, st_g, st_b) = theme.scrollbar_track.to_cairo();
            cr.set_source_rgba(st_r, st_g, st_b, 0.3);
            cr.rectangle(sb_x, section_start_y, sb_w, track_h);
            cr.fill().ok();
            // Thumb.
            cr.set_source_rgb(sb_r, sb_g, sb_b);
            cr.rectangle(sb_x, section_start_y + thumb_top, sb_w, thumb_h);
            cr.fill().ok();
        }

        cr.restore().ok();
        cursor_y = section_start_y + visible_rows as f64 * line_height;
    }
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
) {
    let Some(qf) = &screen.quickfix else {
        return;
    };

    // Header row
    let (hr, hg, hb) = theme.status_bg.to_cairo();
    cr.set_source_rgb(hr, hg, hb);
    cr.rectangle(editor_x, editor_y, editor_w, line_height);
    cr.fill().ok();
    let focus_mark = if qf.has_focus { " [FOCUS]" } else { "" };
    let title = format!("  QUICKFIX  ({} items){}", qf.total_items, focus_mark);
    let (fr, fg, fb) = theme.status_fg.to_cairo();
    cr.set_source_rgb(fr, fg, fb);
    layout.set_attributes(None);
    layout.set_text(&title);
    cr.move_to(editor_x, editor_y);
    pangocairo::show_layout(cr, layout);

    // Result rows
    let visible_rows = ((qf_px / line_height) as usize).saturating_sub(1);
    let scroll_top = (qf.selected_idx + 1).saturating_sub(visible_rows);
    for row_idx in 0..visible_rows {
        let item_idx = scroll_top + row_idx;
        if item_idx >= qf.items.len() {
            break;
        }
        let ry = editor_y + line_height * (row_idx + 1) as f64;
        let is_selected = item_idx == qf.selected_idx;
        if is_selected {
            let (sr, sg, sb) = theme.fuzzy_selected_bg.to_cairo();
            cr.set_source_rgb(sr, sg, sb);
            cr.rectangle(editor_x, ry, editor_w, line_height);
            cr.fill().ok();
        }
        let prefix = if is_selected { "▶ " } else { "  " };
        let text = format!("{}{}", prefix, qf.items[item_idx]);
        let (ir, ig, ib) = theme.fuzzy_fg.to_cairo();
        cr.set_source_rgb(ir, ig, ib);
        layout.set_text(&text);
        cr.move_to(editor_x, ry);
        pangocairo::show_layout(cr, layout);
    }
}

/// Nerd Font icons for the terminal panel toolbar.
pub(super) const NF_CLOSE: &str = "󰅖"; // nf-md-close_box
pub(super) const NF_SPLIT: &str = "󰤼"; // nf-md-view_split_vertical

/// Draw the integrated terminal bottom panel.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_terminal_panel(
    cr: &Context,
    layout: &pango::Layout,
    panel: &render::TerminalPanel,
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    term_px: f64,
    line_height: f64,
    char_width: f64,
    sender: &relm4::Sender<Msg>,
) {
    // Toolbar row (header) — use sans-serif UI font like VSCode.
    let saved_font = layout.font_description().unwrap_or_default();
    let ui_font_desc = FontDescription::from_string(UI_FONT);

    let (hr, hg, hb) = theme.status_bg.to_cairo();
    cr.set_source_rgb(hr, hg, hb);
    cr.rectangle(x, y, w, line_height);
    cr.fill().ok();

    let (fr, fg2, fb) = theme.status_fg.to_cairo();
    layout.set_font_description(Some(&ui_font_desc));
    layout.set_attributes(None);

    if panel.find_active {
        // Find bar mode: replace tab strip with query + match count
        let match_info = if panel.find_match_count == 0 {
            if panel.find_query.is_empty() {
                String::new()
            } else {
                "  (no matches)".to_string()
            }
        } else {
            format!(
                "  ({}/{})",
                panel.find_selected_idx + 1,
                panel.find_match_count
            )
        };
        let find_text = format!(" FIND: {}█{}", panel.find_query, match_info);
        cr.set_source_rgb(fr, fg2, fb);
        layout.set_text(&find_text);
        cr.move_to(x, y);
        pangocairo::show_layout(cr, layout);
        // Close icon right-aligned
        layout.set_text(NF_CLOSE);
        let (cw, _) = layout.pixel_size();
        cr.move_to(x + w - cw as f64 - 4.0, y);
        pangocairo::show_layout(cr, layout);
    } else {
        // Tab strip — each tab is 4 chars: "[N] "
        const TERMINAL_TAB_COLS: usize = 4;
        let mut tab_x = x;
        for i in 0..panel.tab_count {
            let label = format!("[{}] ", i + 1);
            if i == panel.active_tab {
                // Active tab: inverted colors (cursor background)
                let (ar, ag, ab) = theme.cursor.to_cairo();
                cr.set_source_rgb(ar, ag, ab);
                cr.rectangle(tab_x, y, char_width * TERMINAL_TAB_COLS as f64, line_height);
                cr.fill().ok();
                let (br, bg_, bb) = theme.background.to_cairo();
                cr.set_source_rgb(br, bg_, bb);
            } else {
                cr.set_source_rgb(fr, fg2, fb);
            }
            layout.set_text(&label);
            cr.move_to(tab_x, y);
            pangocairo::show_layout(cr, layout);
            tab_x += char_width * TERMINAL_TAB_COLS as f64;
        }

        // If no tabs yet (panel open but spawning), show a minimal title
        if panel.tab_count == 0 {
            cr.set_source_rgb(fr, fg2, fb);
            layout.set_text("  TERMINAL");
            cr.move_to(x, y);
            pangocairo::show_layout(cr, layout);
        }

        // Right-aligned toolbar buttons: + ⊞ ×  (each ~2 chars wide)
        cr.set_source_rgb(fr, fg2, fb);
        let btn_text = format!("+ {} {}", NF_SPLIT, NF_CLOSE);
        layout.set_text(&btn_text);
        let (btn_w, _) = layout.pixel_size();
        cr.move_to(x + w - btn_w as f64 - 4.0, y);
        pangocairo::show_layout(cr, layout);
    }

    // close_x / split_x used by click detection in MouseClick handler
    let _ = sender; // click detection handled in MouseClick

    // Restore monospace font for terminal content rendering.
    layout.set_font_description(Some(&saved_font));

    // Scrollbar geometry
    const SB_W: f64 = 6.0;
    let content_y = y + line_height;
    let content_h = term_px - line_height;
    let rows_to_draw = ((term_px / line_height) as usize).saturating_sub(1);
    let total = panel.scrollback_rows + rows_to_draw;
    let (thumb_top_px, thumb_bot_px) = if panel.scrollback_rows == 0 {
        (0.0, content_h) // no scrollback → full bar
    } else {
        let thumb_h = ((rows_to_draw as f64 / total as f64) * content_h).max(4.0);
        let max_off = panel.scrollback_rows as f64;
        let frac = if panel.scroll_offset == 0 {
            1.0 // at live bottom → thumb at bottom
        } else {
            1.0 - (panel.scroll_offset as f64 / max_off).min(1.0)
        };
        let thumb_t = frac * (content_h - thumb_h);
        (thumb_t, thumb_t + thumb_h)
    };

    // Draw scrollbar track (right edge of whole panel)
    let sb_x = x + w - SB_W;
    let (tbr, tbg, tbb) = theme.status_bg.to_cairo();
    cr.set_source_rgb(tbr * 1.4, tbg * 1.4, tbb * 1.4); // slightly lighter than header
    cr.rectangle(sb_x, content_y, SB_W, content_h);
    cr.fill().ok();
    // Draw scrollbar thumb
    let (fr, fg2, fb) = theme.status_fg.to_cairo();
    cr.set_source_rgba(fr, fg2, fb, 0.5);
    cr.rectangle(
        sb_x + 1.0,
        content_y + thumb_top_px,
        SB_W - 2.0,
        thumb_bot_px - thumb_top_px,
    );
    cr.fill().ok();

    // ── Split view: draw left pane + divider + right pane ─────────────────────
    if let Some(ref left_rows) = panel.split_left_rows {
        let half_w = panel.split_left_cols as f64 * char_width;
        let div_x = x + half_w;

        // Fill both halves with terminal default bg.
        let (tbgr, tbgg, tbgb) = theme.terminal_bg.to_cairo();
        cr.set_source_rgb(tbgr, tbgg, tbgb);
        cr.rectangle(x, content_y, w - SB_W, content_h);
        cr.fill().ok();

        // Draw left pane cells.
        draw_terminal_cells(
            cr,
            layout,
            left_rows,
            x,
            content_y,
            half_w,
            line_height,
            char_width,
            theme,
        );

        // Draw divider (1px vertical line).
        let (dr, dg, db) = theme.separator.to_cairo();
        cr.set_source_rgb(dr, dg, db);
        cr.rectangle(div_x, content_y, 1.0, content_h);
        cr.fill().ok();

        // Draw right pane cells.
        draw_terminal_cells(
            cr,
            layout,
            &panel.rows,
            div_x + 1.0,
            content_y,
            half_w - 1.0,
            line_height,
            char_width,
            theme,
        );
        return;
    }

    // ── Normal single-pane view ────────────────────────────────────────────────
    // Content rows (terminal cells)
    let cell_area_w = w - SB_W;

    // Fill the entire content area with the default terminal background first.
    let (tbgr, tbgg, tbgb) = theme.terminal_bg.to_cairo();
    cr.set_source_rgb(tbgr, tbgg, tbgb);
    cr.rectangle(x, content_y, cell_area_w, content_h);
    cr.fill().ok();

    draw_terminal_cells(
        cr,
        layout,
        &panel.rows,
        x,
        content_y,
        cell_area_w,
        line_height,
        char_width,
        theme,
    );
}

/// Draw a grid of terminal cells into a rectangular region.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_terminal_cells(
    cr: &Context,
    layout: &pango::Layout,
    rows: &[Vec<render::TerminalCell>],
    x: f64,
    content_y: f64,
    cell_area_w: f64,
    line_height: f64,
    char_width: f64,
    theme: &Theme,
) {
    for (row_idx, row) in rows.iter().enumerate() {
        let row_y = content_y + row_idx as f64 * line_height;
        let mut cell_x = x;
        for cell in row {
            if cell_x + char_width > x + cell_area_w {
                break;
            }
            let (br, bg, bb) = cell.bg;
            let (fr, fg2, fb) = cell.fg;

            // Cell background
            let (draw_br, draw_bg, draw_bb) = if cell.is_cursor {
                // Cursor: inverted colors (white on normal bg)
                (fr, fg2, fb)
            } else if cell.is_find_active {
                // Active find match: orange background
                (255u8, 165u8, 0u8)
            } else if cell.is_find_match {
                // Other find matches: dark amber background
                (100u8, 80u8, 20u8)
            } else if cell.selected {
                // Selection highlight (use theme selection color)
                let (sr, sg, sb) = theme.selection.to_cairo();
                ((sr * 255.0) as u8, (sg * 255.0) as u8, (sb * 255.0) as u8)
            } else {
                (br, bg, bb)
            };
            cr.set_source_rgb(
                draw_br as f64 / 255.0,
                draw_bg as f64 / 255.0,
                draw_bb as f64 / 255.0,
            );
            cr.rectangle(cell_x, row_y, char_width, line_height);
            cr.fill().ok();

            // Cell foreground text
            let ch_str = cell.ch.to_string();
            if cell.ch != ' ' {
                let (draw_fr, draw_fg, draw_fb) = if cell.is_cursor {
                    (br, bg, bb) // inverted for cursor
                } else if cell.is_find_active {
                    (0u8, 0u8, 0u8) // black text on orange
                } else {
                    (fr, fg2, fb)
                };
                cr.set_source_rgb(
                    draw_fr as f64 / 255.0,
                    draw_fg as f64 / 255.0,
                    draw_fb as f64 / 255.0,
                );

                // Apply bold/italic via Pango attributes if needed
                let attrs = AttrList::new();
                if cell.bold {
                    attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
                }
                if cell.italic {
                    attrs.insert(pango::AttrInt::new_style(pango::Style::Italic));
                }
                if cell.underline {
                    attrs.insert(pango::AttrInt::new_underline(pango::Underline::Single));
                }
                layout.set_attributes(Some(&attrs));
                layout.set_text(&ch_str);
                cr.move_to(cell_x, row_y);
                pangocairo::show_layout(cr, layout);
                layout.set_attributes(None);
            }

            cell_x += char_width;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_status_line(
    cr: &Context,
    layout: &pango::Layout,
    theme: &Theme,
    left: &str,
    right: &str,
    width: f64,
    y: f64,
    line_height: f64,
) {
    let (br, bg, bb) = theme.status_bg.to_cairo();
    cr.set_source_rgb(br, bg, bb);
    cr.rectangle(0.0, y, width, line_height);
    cr.fill().ok();

    layout.set_attributes(None);

    let (fr, fg, fb) = theme.status_fg.to_cairo();
    cr.set_source_rgb(fr, fg, fb);

    // Measure right text first so we can clamp left text width.
    layout.set_width(-1); // reset to natural width
    layout.set_ellipsize(pango::EllipsizeMode::None);
    layout.set_text(right);
    let (right_w, _) = layout.pixel_size();

    // Draw left text, truncated to not overlap right text.
    let left_max = (width - right_w as f64 - 8.0).max(0.0);
    layout.set_text(left);
    layout.set_width((left_max * pango::SCALE as f64) as i32);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    cr.move_to(0.0, y);
    pangocairo::show_layout(cr, layout);
    layout.set_width(-1);
    layout.set_ellipsize(pango::EllipsizeMode::None);

    // Draw right text, right-aligned.
    layout.set_text(right);
    cr.move_to(width - right_w as f64, y);
    pangocairo::show_layout(cr, layout);
}

/// Draw a per-window status bar with styled segments.
#[allow(clippy::too_many_arguments)]
fn draw_window_status_bar(
    cr: &Context,
    layout: &pango::Layout,
    theme: &Theme,
    status: &render::WindowStatusLine,
    x: f64,
    y: f64,
    width: f64,
    line_height: f64,
) {
    // Fill background using the first segment's bg (derived from theme, not status_bg)
    let fill_bg = status
        .left_segments
        .first()
        .or(status.right_segments.first())
        .map(|s| s.bg)
        .unwrap_or(theme.background);
    let (br, bg, bb) = fill_bg.to_cairo();
    cr.set_source_rgb(br, bg, bb);
    cr.rectangle(x, y, width, line_height);
    cr.fill().ok();

    layout.set_attributes(None);
    layout.set_width(-1);
    layout.set_ellipsize(pango::EllipsizeMode::None);

    // Draw left segments
    let mut cx = x;
    for seg in &status.left_segments {
        let (sr, sg, sb) = seg.bg.to_cairo();
        cr.set_source_rgb(sr, sg, sb);
        // Measure segment width
        layout.set_text(&seg.text);
        if seg.bold {
            let attrs = pango::AttrList::new();
            attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
            layout.set_attributes(Some(&attrs));
        } else {
            layout.set_attributes(None);
        }
        let (seg_w, _) = layout.pixel_size();
        let seg_w = seg_w as f64;
        // Draw segment background
        cr.rectangle(cx, y, seg_w, line_height);
        cr.fill().ok();
        // Draw segment text
        let (fr, fg, fb) = seg.fg.to_cairo();
        cr.set_source_rgb(fr, fg, fb);
        cr.move_to(cx, y);
        pangocairo::show_layout(cr, layout);
        cx += seg_w;
        if cx >= x + width {
            break;
        }
    }

    // Draw right segments, right-aligned
    let mut right_total_w = 0.0;
    for seg in &status.right_segments {
        layout.set_text(&seg.text);
        if seg.bold {
            let attrs = pango::AttrList::new();
            attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
            layout.set_attributes(Some(&attrs));
        } else {
            layout.set_attributes(None);
        }
        let (seg_w, _) = layout.pixel_size();
        right_total_w += seg_w as f64;
    }
    let mut rx = (x + width - right_total_w).max(cx);
    for seg in &status.right_segments {
        let (sr, sg, sb) = seg.bg.to_cairo();
        cr.set_source_rgb(sr, sg, sb);
        layout.set_text(&seg.text);
        if seg.bold {
            let attrs = pango::AttrList::new();
            attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
            layout.set_attributes(Some(&attrs));
        } else {
            layout.set_attributes(None);
        }
        let (seg_w, _) = layout.pixel_size();
        let seg_w = seg_w as f64;
        cr.rectangle(rx, y, seg_w, line_height);
        cr.fill().ok();
        let (fr, fg, fb) = seg.fg.to_cairo();
        cr.set_source_rgb(fr, fg, fb);
        cr.move_to(rx, y);
        pangocairo::show_layout(cr, layout);
        rx += seg_w;
    }

    layout.set_attributes(None);
}

pub(super) fn draw_wildmenu(
    cr: &Context,
    layout: &pango::Layout,
    theme: &Theme,
    wm: &render::WildmenuData,
    width: f64,
    y: f64,
    line_height: f64,
) {
    // Fill background
    let (br, bg, bb) = theme.wildmenu_bg.to_cairo();
    cr.set_source_rgb(br, bg, bb);
    cr.rectangle(0.0, y, width, line_height);
    cr.fill().ok();

    layout.set_attributes(None);
    layout.set_width(-1);
    layout.set_ellipsize(pango::EllipsizeMode::None);

    let mut x = 0.0;
    for (i, item) in wm.items.iter().enumerate() {
        if x >= width {
            break;
        }
        let is_selected = wm.selected == Some(i);
        let label = format!(" {} ", item);
        layout.set_text(&label);
        let (item_w, _) = layout.pixel_size();

        if is_selected {
            // Draw selected item background
            let (sbr, sbg, sbb) = theme.wildmenu_sel_bg.to_cairo();
            cr.set_source_rgb(sbr, sbg, sbb);
            cr.rectangle(x, y, item_w as f64, line_height);
            cr.fill().ok();
            // Selected item foreground
            let (sfr, sfg, sfb) = theme.wildmenu_sel_fg.to_cairo();
            cr.set_source_rgb(sfr, sfg, sfb);
        } else {
            let (fr, fg, fb) = theme.wildmenu_fg.to_cairo();
            cr.set_source_rgb(fr, fg, fb);
        }

        cr.move_to(x, y);
        pangocairo::show_layout(cr, layout);
        x += item_w as f64;
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

/// Returns `(back_x, back_end, fwd_x, fwd_end, unit_end)` — pixel hit rects for nav arrows
/// and the right edge of the entire interactive area (arrows + search box).
pub(super) fn draw_menu_bar(
    cr: &Context,
    data: &render::MenuBarData,
    theme: &Theme,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> (f64, f64, f64, f64, f64) {
    // Title bar background: use tab_bar_bg (adapts to light/dark themes).
    let (tbr, tbg, tbb) = theme.tab_bar_bg.to_cairo();
    cr.set_source_rgb(tbr, tbg, tbb);
    cr.rectangle(x, y, width, height);
    let _ = cr.fill();

    let pango_ctx = pangocairo::create_context(cr);
    let font_desc = pango::FontDescription::from_string(UI_FONT);
    let layout = pango::Layout::new(&pango_ctx);
    layout.set_font_description(Some(&font_desc));

    let (fr, fg, fb) = theme.foreground.to_cairo();
    cr.set_source_rgb(fr, fg, fb);

    // Menu labels
    let mut cursor_x = x + 8.0;

    for (idx, (name, _, _)) in render::MENU_STRUCTURE.iter().enumerate() {
        let is_open = data.open_menu_idx == Some(idx);
        if is_open {
            let (ar, ag, ab) = theme.keyword.to_cairo();
            cr.set_source_rgb(ar, ag, ab);
        } else {
            cr.set_source_rgb(fr, fg, fb);
        }
        layout.set_text(name);
        let (_lw, lh) = layout.pixel_size();
        cr.move_to(cursor_x, y + (height - lh as f64) / 2.0);
        pangocairo::show_layout(cr, &layout);
        // Use same metric as click/hover handlers: 7px/char + 10px padding.
        cursor_x += name.len() as f64 * 7.0 + 10.0;
    }

    // Centered nav arrows + search box (like VSCode Command Center).
    // The entire unit is centered between the menu labels and the right edge.
    let menu_end_x = cursor_x;

    // Measure arrow widths.
    layout.set_text("\u{25C0}"); // ◀
    let (back_w, _) = layout.pixel_size();
    layout.set_text("\u{25B6}"); // ▶
    let (fwd_w, _) = layout.pixel_size();
    let arrow_gap = 6.0;
    let arrows_w = back_w as f64 + arrow_gap + fwd_w as f64;

    // Measure search box text.
    let display = if data.title.is_empty() {
        String::new()
    } else {
        format!("\u{1f50d}  {}", data.title)
    };
    let box_pad = 12.0;
    let min_box_w = 280.0; // minimum search bar width to match VSCode proportions
    let (box_text_w, _) = if !display.is_empty() {
        layout.set_text(&display);
        layout.pixel_size()
    } else {
        (0, 0)
    };
    let box_w = if !display.is_empty() {
        (box_text_w as f64 + box_pad * 2.0).max(min_box_w)
    } else {
        0.0
    };
    let gap_between = if box_w > 0.0 { 10.0 } else { 0.0 };
    let total_unit_w = arrows_w + gap_between + box_w;

    // Center the unit between menu_end_x and right edge.
    let available = x + width - menu_end_x;
    let unit_x = (menu_end_x + (available - total_unit_w) / 2.0).max(menu_end_x + 8.0);

    // Draw back arrow.
    let dim_fg = theme.line_number_fg;
    let back_color = if data.nav_back_enabled {
        theme.foreground
    } else {
        dim_fg
    };
    let (br2, bg2, bb2) = back_color.to_cairo();
    cr.set_source_rgb(br2, bg2, bb2);
    layout.set_text("\u{25C0}");
    let (_, bh) = layout.pixel_size();
    cr.move_to(unit_x, y + (height - bh as f64) / 2.0);
    pangocairo::show_layout(cr, &layout);

    // Draw forward arrow.
    let fwd_color = if data.nav_forward_enabled {
        theme.foreground
    } else {
        dim_fg
    };
    let (fr2, fg2, fb2) = fwd_color.to_cairo();
    cr.set_source_rgb(fr2, fg2, fb2);
    layout.set_text("\u{25B6}");
    let (_, fh) = layout.pixel_size();
    cr.move_to(
        unit_x + back_w as f64 + arrow_gap,
        y + (height - fh as f64) / 2.0,
    );
    pangocairo::show_layout(cr, &layout);

    // Draw search box.
    if !display.is_empty() {
        let bx = unit_x + arrows_w + gap_between;
        let by = y + 3.0;
        let bh_box = height - 6.0;
        let radius = 4.0;
        // Border
        let (sr, sg, sb) = theme.separator.to_cairo();
        cr.set_source_rgb(sr, sg, sb);
        cr.new_path();
        cr.arc(
            bx + box_w - radius,
            by + radius,
            radius,
            -std::f64::consts::FRAC_PI_2,
            0.0,
        );
        cr.arc(
            bx + box_w - radius,
            by + bh_box - radius,
            radius,
            0.0,
            std::f64::consts::FRAC_PI_2,
        );
        cr.arc(
            bx + radius,
            by + bh_box - radius,
            radius,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
        );
        cr.arc(
            bx + radius,
            by + radius,
            radius,
            std::f64::consts::PI,
            3.0 * std::f64::consts::FRAC_PI_2,
        );
        cr.close_path();
        cr.set_line_width(1.0);
        let _ = cr.stroke();
        // Text inside box — same color as menu labels (foreground)
        cr.set_source_rgb(fr, fg, fb);
        layout.set_text(&display);
        let (_, th) = layout.pixel_size();
        cr.move_to(bx + box_pad, y + (height - th as f64) / 2.0);
        pangocairo::show_layout(cr, &layout);
    }

    // Return pixel hit rects for back and forward arrows + interactive area end.
    let fwd_x = unit_x + back_w as f64 + arrow_gap;
    let unit_end = unit_x + total_unit_w;
    (
        unit_x,
        unit_x + back_w as f64,
        fwd_x,
        fwd_x + fwd_w as f64,
        unit_end,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_menu_dropdown(
    cr: &Context,
    data: &render::MenuBarData,
    theme: &Theme,
    x: f64,
    anchor_y: f64,
    _width: f64,
    _height: f64,
    line_height: f64,
) {
    if data.open_items.is_empty() {
        return;
    }

    let pango_ctx = pangocairo::create_context(cr);
    let font_desc = pango::FontDescription::from_string(UI_FONT);
    let layout = pango::Layout::new(&pango_ctx);
    layout.set_font_description(Some(&font_desc));

    // Compute popup_x using the same metric as the click/hover handlers.
    let mut popup_x = x + 8.0;
    if let Some(midx) = data.open_menu_idx {
        for i in 0..midx {
            if let Some((name, _, _)) = render::MENU_STRUCTURE.get(i) {
                popup_x += name.len() as f64 * 7.0 + 10.0;
            }
        }
    }
    let item_count = data.open_items.len() as f64;
    let popup_width = 220.0_f64;
    let pad = 4.0; // small top/bottom padding
    let popup_height = item_count * line_height + pad * 2.0;
    let popup_y = anchor_y;

    // Background — use hover_bg (adapts to light/dark themes).
    let (hbr, hbg, hbb) = theme.hover_bg.to_cairo();
    cr.set_source_rgb(hbr, hbg, hbb);
    cr.rectangle(popup_x, popup_y, popup_width, popup_height);
    let _ = cr.fill();

    // Border
    let (bdr, bdg, bdb) = theme.hover_border.to_cairo();
    cr.set_source_rgb(bdr, bdg, bdb);
    cr.rectangle(popup_x, popup_y, popup_width, popup_height);
    let _ = cr.stroke();

    // Items — each occupies one line_height row starting at popup_y + pad.
    let (fr, fg_c, fb) = theme.foreground.to_cairo();
    let (sr, sg, sb) = theme.line_number_fg.to_cairo();
    cr.set_source_rgb(fr, fg_c, fb);
    for (i, item) in data.open_items.iter().enumerate() {
        let row_top = popup_y + pad + i as f64 * line_height;
        if item.separator {
            cr.set_source_rgb(sr, sg, sb);
            let sep_y = row_top + line_height * 0.5;
            cr.move_to(popup_x + 4.0, sep_y);
            cr.line_to(popup_x + popup_width - 4.0, sep_y);
            let _ = cr.stroke();
            cr.set_source_rgb(fr, fg_c, fb);
        } else {
            // Draw highlight bar for hovered/keyboard-selected item.
            if data.highlighted_item_idx == Some(i) {
                let (slr, slg, slb) = theme.sidebar_sel_bg.to_cairo();
                cr.set_source_rgb(slr, slg, slb);
                cr.rectangle(popup_x + 1.0, row_top, popup_width - 2.0, line_height);
                let _ = cr.fill();
            }
            layout.set_text(item.label);
            let (_, lh) = layout.pixel_size();
            let text_y = row_top + (line_height - lh as f64) * 0.5;
            cr.set_source_rgb(fr, fg_c, fb);
            cr.move_to(popup_x + 8.0, text_y);
            pangocairo::show_layout(cr, &layout);
            let sc = if data.is_vscode_mode && !item.vscode_shortcut.is_empty() {
                item.vscode_shortcut
            } else {
                item.shortcut
            };
            if !sc.is_empty() {
                layout.set_text(sc);
                let (sc_w, _) = layout.pixel_size();
                cr.set_source_rgb(sr, sg, sb);
                cr.move_to(popup_x + popup_width - sc_w as f64 - 8.0, text_y);
                pangocairo::show_layout(cr, &layout);
                cr.set_source_rgb(fr, fg_c, fb);
            }
        }
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
    let (sel_r, sel_g, sel_b) = theme.fuzzy_selected_bg.to_cairo();
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

    // Helper to draw a section — uses y_off (float) for vertical positioning.
    let item_height = (line_height * 1.4).round();
    let draw_section = |cr: &Context,
                        layout: &pango::Layout,
                        title: &str,
                        items: &[String],
                        expanded: bool,
                        y_off: &mut f64,
                        flat_start: usize,
                        selected: usize| {
        let arrow = if expanded { "▼" } else { "▶" };
        let header_text = format!("  {} {} ({})", arrow, title, items.len());
        cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
        cr.rectangle(x, *y_off, w, line_height);
        cr.fill().ok();
        cr.set_source_rgb(hdr_fg_r, hdr_fg_g, hdr_fg_b);
        layout.set_text(&header_text);
        let (_, lh) = layout.pixel_size();
        cr.move_to(x + 2.0, *y_off + (line_height - lh as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
        *y_off += line_height;

        if expanded {
            for (i, item) in items.iter().enumerate() {
                let flat_idx = flat_start + 1 + i; // +1 for section header
                let is_sel = flat_idx == selected;
                if is_sel {
                    cr.set_source_rgb(sel_r, sel_g, sel_b);
                    cr.rectangle(x, *y_off, w, item_height);
                    cr.fill().ok();
                }
                cr.set_source_rgb(
                    if is_sel { hdr_fg_r } else { dim_r },
                    if is_sel { hdr_fg_g } else { dim_g },
                    if is_sel { hdr_fg_b } else { dim_b },
                );
                layout.set_text(&format!("    {}", item));
                let (_, lh) = layout.pixel_size();
                cr.move_to(x + 2.0, *y_off + (item_height - lh as f64) / 2.0);
                pangocairo::show_layout(cr, layout);
                *y_off += item_height;
                if *y_off > y + h {
                    break;
                }
            }
        }
    };

    // Compute flat start offsets
    let staged_items: Vec<String> = sc
        .staged
        .iter()
        .map(|f| format!("{} {}", f.status_char, f.path))
        .collect();
    let unstaged_items: Vec<String> = sc
        .unstaged
        .iter()
        .map(|f| format!("{} {}", f.status_char, f.path))
        .collect();
    let wt_items: Vec<String> = sc
        .worktrees
        .iter()
        .map(|wt| {
            let marker = if wt.is_current { "\u{2714} " } else { "  " };
            format!("{}{} {}", marker, wt.branch, wt.path)
        })
        .collect();

    let staged_flat_start = 0usize;
    let unstaged_flat_start = 1 + if sc.sections_expanded[0] {
        sc.staged.len()
    } else {
        0
    };
    let wt_flat_start = unstaged_flat_start
        + 1
        + if sc.sections_expanded[1] {
            sc.unstaged.len()
        } else {
            0
        };
    let show_worktrees = sc.worktrees.len() > 1;
    let log_flat_start = if show_worktrees {
        wt_flat_start
            + 1
            + if sc.sections_expanded[2] {
                sc.worktrees.len()
            } else {
                0
            }
    } else {
        wt_flat_start
    };

    // Track vertical position for sections (float, since item_height != line_height).
    let mut y_off = y_commit;

    // Draw staged section
    if y_off < y + h {
        draw_section(
            cr,
            layout,
            "STAGED CHANGES",
            &staged_items,
            sc.sections_expanded[0],
            &mut y_off,
            staged_flat_start,
            sc.selected,
        );
    }

    // Color hint for diff-add
    let _ = (add_r, add_g, add_b, del_r, del_g, del_b);

    // Draw unstaged section
    if y_off < y + h {
        draw_section(
            cr,
            layout,
            "CHANGES",
            &unstaged_items,
            sc.sections_expanded[1],
            &mut y_off,
            unstaged_flat_start,
            sc.selected,
        );
    }

    // Draw worktrees section (only when there are linked worktrees beyond the main one).
    if y_off < y + h && show_worktrees {
        draw_section(
            cr,
            layout,
            "WORKTREES",
            &wt_items,
            sc.sections_expanded[2],
            &mut y_off,
            wt_flat_start,
            sc.selected,
        );
    }

    // Draw log section (RECENT COMMITS) — always present.
    if y_off < y + h {
        let log_items: Vec<String> = sc
            .log
            .iter()
            .map(|e| format!("{} {}", e.hash, e.message))
            .collect();
        draw_section(
            cr,
            layout,
            &format!("{} RECENT COMMITS", icons::GIT_HISTORY.nerd),
            &log_items,
            sc.sections_expanded[3],
            &mut y_off,
            log_flat_start,
            sc.selected,
        );
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
) {
    let Some(ref ext) = screen.ext_sidebar else {
        return;
    };

    let (bg_r, bg_g, bg_b) = theme.tab_bar_bg.to_cairo();
    let (hdr_r, hdr_g, hdr_b) = theme.status_bg.to_cairo();
    let (hdr_fg_r, hdr_fg_g, hdr_fg_b) = theme.status_fg.to_cairo();
    let (fg_r, fg_g, fg_b) = theme.foreground.to_cairo();
    let (dim_r, dim_g, dim_b) = theme.line_number_fg.to_cairo();
    let (sel_r, sel_g, sel_b) = theme.fuzzy_selected_bg.to_cairo();

    // Background
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    // Item rows get extra vertical padding for readability.
    let item_height = (line_height * 1.4).ceil();
    let pad = (item_height - line_height) / 2.0;

    layout.set_attributes(None);
    let mut ry: f64 = 0.0;

    // Helper: draw a text row with optional right-aligned hint (only on selected).
    // Returns the row height used.
    #[allow(clippy::too_many_arguments)]
    fn draw_item_row(
        cr: &Context,
        layout: &pango::Layout,
        x: f64,
        y: f64,
        ry: f64,
        w: f64,
        item_height: f64,
        pad: f64,
        is_selected: bool,
        sel_rgb: (f64, f64, f64),
        fg_rgb: (f64, f64, f64),
        sel_fg_rgb: (f64, f64, f64),
        dim_rgb: (f64, f64, f64),
        name_text: &str,
        hint: &str,
    ) {
        if is_selected {
            cr.set_source_rgb(sel_rgb.0, sel_rgb.1, sel_rgb.2);
            cr.rectangle(x, y + ry, w, item_height);
            cr.fill().ok();
        }
        // Measure hint width.
        let hint_w = if is_selected && !hint.is_empty() {
            layout.set_text(hint);
            layout.pixel_size().0
        } else {
            0
        };
        // Draw name with ellipsis if needed.
        let name_max = (w - 6.0 - hint_w as f64).max(20.0) as i32;
        let text_rgb = if is_selected { sel_fg_rgb } else { fg_rgb };
        cr.set_source_rgb(text_rgb.0, text_rgb.1, text_rgb.2);
        layout.set_text(name_text);
        layout.set_width(name_max * pango::SCALE);
        layout.set_ellipsize(pango::EllipsizeMode::End);
        let (_, text_h) = layout.pixel_size();
        cr.move_to(
            x + 2.0,
            y + ry + pad + (item_height - pad * 2.0 - text_h as f64) / 2.0,
        );
        pangocairo::show_layout(cr, layout);
        layout.set_width(-1);
        layout.set_ellipsize(pango::EllipsizeMode::None);
        // Right-aligned hint.
        if hint_w > 0 {
            cr.set_source_rgb(dim_rgb.0, dim_rgb.1, dim_rgb.2);
            layout.set_text(hint);
            cr.move_to(
                x + w - hint_w as f64 - 4.0,
                y + ry + pad + (item_height - pad * 2.0 - text_h as f64) / 2.0,
            );
            pangocairo::show_layout(cr, layout);
        }
    }

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

    // ── INSTALLED section ─────────────────────────────────────────────────────
    let installed_count = ext.items_installed.len();
    if ry < h {
        let arrow = if ext.sections_expanded[0] {
            "▼"
        } else {
            "▶"
        };
        let sec_hdr = format!("  {} INSTALLED ({})", arrow, installed_count);
        cr.set_source_rgb(hdr_r * 0.85, hdr_g * 0.85, hdr_b * 0.85);
        cr.rectangle(x, y + ry, w, line_height);
        cr.fill().ok();
        cr.set_source_rgb(dim_r, dim_g, dim_b);
        layout.set_text(&sec_hdr);
        let (_, lh3) = layout.pixel_size();
        cr.move_to(x + 2.0, y + ry + (line_height - lh3 as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
        ry += line_height;
    }

    if ext.sections_expanded[0] {
        for (idx, item) in ext.items_installed.iter().enumerate() {
            if ry >= h {
                break;
            }
            let is_selected = ext.has_focus && ext.selected == idx;
            let name_text = if item.update_available {
                format!("  ● {} \u{2191}", item.display_name)
            } else {
                format!("  ● {}", item.display_name)
            };
            let hint = if !is_selected {
                ""
            } else if item.update_available {
                "[u]update"
            } else {
                "[d]remove"
            };
            draw_item_row(
                cr,
                layout,
                x,
                y,
                ry,
                w,
                item_height,
                pad,
                is_selected,
                (sel_r, sel_g, sel_b),
                (fg_r, fg_g, fg_b),
                (hdr_fg_r, hdr_fg_g, hdr_fg_b),
                (dim_r, dim_g, dim_b),
                &name_text,
                hint,
            );
            ry += item_height;
        }
        if installed_count == 0 && ry < h {
            cr.set_source_rgb(dim_r, dim_g, dim_b);
            layout.set_text("    (none installed)");
            let (_, lhn) = layout.pixel_size();
            cr.move_to(x + 2.0, y + ry + (item_height - lhn as f64) / 2.0);
            pangocairo::show_layout(cr, layout);
            ry += item_height;
        }
    }

    // ── AVAILABLE section ─────────────────────────────────────────────────────
    let available_count = ext.items_available.len();
    if ry < h {
        let arrow = if ext.sections_expanded[1] {
            "▼"
        } else {
            "▶"
        };
        let sec_hdr = format!("  {} AVAILABLE ({})", arrow, available_count);
        cr.set_source_rgb(hdr_r * 0.85, hdr_g * 0.85, hdr_b * 0.85);
        cr.rectangle(x, y + ry, w, line_height);
        cr.fill().ok();
        cr.set_source_rgb(dim_r, dim_g, dim_b);
        layout.set_text(&sec_hdr);
        let (_, lh5) = layout.pixel_size();
        cr.move_to(x + 2.0, y + ry + (line_height - lh5 as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
        ry += line_height;
    }

    if ext.sections_expanded[1] {
        for (idx, item) in ext.items_available.iter().enumerate() {
            if ry >= h {
                break;
            }
            let flat_idx = installed_count + idx;
            let is_selected = ext.has_focus && ext.selected == flat_idx;
            let name_text = format!("  ○ {}", item.display_name);
            let hint = if is_selected { "[i]install" } else { "" };
            draw_item_row(
                cr,
                layout,
                x,
                y,
                ry,
                w,
                item_height,
                pad,
                is_selected,
                (sel_r, sel_g, sel_b),
                (fg_r, fg_g, fg_b),
                (hdr_fg_r, hdr_fg_g, hdr_fg_b),
                (dim_r, dim_g, dim_b),
                &name_text,
                hint,
            );
            ry += item_height;
        }
        if available_count == 0 && ry < h {
            let msg = if ext.fetching {
                "    Fetching registry…"
            } else {
                "    (all extensions installed)"
            };
            cr.set_source_rgb(dim_r, dim_g, dim_b);
            layout.set_text(msg);
            let (_, lhn) = layout.pixel_size();
            cr.move_to(x + 2.0, y + ry + (item_height - lhn as f64) / 2.0);
            pangocairo::show_layout(cr, layout);
            ry += item_height;
        }
    }

    // Focus border
    if ext.has_focus {
        let (kr, kg, kb) = theme.keyword.to_cairo();
        cr.set_source_rgb(kr, kg, kb);
        cr.set_line_width(1.5);
        cr.rectangle(x + 0.75, y + 0.75, w - 1.5, h - 1.5);
        cr.stroke().ok();
    }

    let _ = ry;
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
    let (user_r, user_g, user_b) = theme.keyword.to_cairo();
    let (asst_r, asst_g, asst_b) = theme.string_lit.to_cairo();

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
    let mut vis_rows: Vec<(String, f64, f64, f64, f64)> = Vec::new();
    for msg in &ai.messages {
        let is_user = msg.role == "user";
        let (role_label, rr, rg, rb) = if is_user {
            ("You:", user_r, user_g, user_b)
        } else {
            ("AI:", asst_r, asst_g, asst_b)
        };
        vis_rows.push((role_label.to_string(), rr, rg, rb, 4.0));
        for line in msg.content.lines() {
            if line.is_empty() {
                vis_rows.push((" ".to_string(), fg_r, fg_g, fg_b, 12.0));
                continue;
            }
            let chars: Vec<char> = line.chars().collect();
            let mut pos = 0;
            while pos < chars.len() {
                let end = (pos + wrap_cols).min(chars.len());
                let chunk: String = chars[pos..end].iter().collect();
                vis_rows.push((chunk, fg_r, fg_g, fg_b, 12.0));
                pos = end;
            }
        }
        vis_rows.push((" ".to_string(), dim_r, dim_g, dim_b, 0.0));
    }

    let scroll = ai.scroll_top.min(vis_rows.len().saturating_sub(1));
    for (i, (text, rr, rg, rb, xi)) in vis_rows.iter().enumerate().skip(scroll) {
        let vrow = (i - scroll + 1) as f64;
        let ry = vrow * line_height;
        if ry >= max_row_y {
            break;
        }
        cr.set_source_rgb(*rr, *rg, *rb);
        layout.set_text(text);
        let (_, lh) = layout.pixel_size();
        cr.move_to(x + xi, y + ry + (line_height - lh as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
    }

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
    let cursor_line = if input_content_w > 0 {
        cursor / input_content_w
    } else {
        0
    };
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

pub(super) fn draw_debug_toolbar(
    cr: &Context,
    toolbar: &render::DebugToolbarData,
    theme: &Theme,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) {
    let (r, g, b) = theme.status_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(x, y, width, height);
    let _ = cr.fill();

    let (fr, fg_c, fb) = if toolbar.session_active {
        theme.status_fg.to_cairo()
    } else {
        theme.line_number_fg.to_cairo()
    };
    cr.set_source_rgb(fr, fg_c, fb);

    let mut cursor_x = x + 8.0;
    for (idx, btn) in toolbar.buttons.iter().enumerate() {
        if idx == 4 {
            // Separator
            let (dr, dg, db) = theme.line_number_fg.to_cairo();
            cr.set_source_rgb(dr, dg, db);
            cr.move_to(cursor_x, y + 2.0);
            cr.line_to(cursor_x, y + height - 2.0);
            let _ = cr.stroke();
            cr.set_source_rgb(fr, fg_c, fb);
            cursor_x += 8.0;
        }
        cr.move_to(cursor_x, y + height * 0.7);
        let text = format!("{} ({}) ", btn.label, btn.key_hint);
        let _ = cr.show_text(&text);
        cursor_x += text.len() as f64 * 7.0;
    }
}
