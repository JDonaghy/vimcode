use super::*;

// ─── Screen layout bridging ───────────────────────────────────────────────────

pub(super) fn build_screen_for_tui(
    engine: &Engine,
    theme: &Theme,
    area: Rect,
    sidebar: &TuiSidebar,
    sidebar_width: u16,
) -> render::ScreenLayout {
    // Global bottom rows: status(1) + cmd(1).  The tab bar row is included in
    // content_bounds and handled by calculate_group_window_rects (tab_bar_height=1).
    // Must match draw_frame's vertical layout exactly.
    let qf_height: u16 = if engine.quickfix_open { 6 } else { 0 };
    let bottom_panel_open = engine.terminal_open || engine.bottom_panel_open;
    let term_height: u16 = if bottom_panel_open {
        let target = super::terminal_target_maximize_rows_tui(engine, area.height);
        engine.effective_terminal_panel_rows(target) + 2 // tab bar + header + content
    } else {
        0
    };
    let menu_height: u16 = if engine.menu_bar_visible { 1 } else { 0 };
    let dbg_height: u16 = if engine.debug_toolbar_visible { 1 } else { 0 };
    let wildmenu_height: u16 = if !engine.wildmenu_items.is_empty() {
        1
    } else {
        0
    };
    let per_window_status = engine.settings.window_status_line;
    let global_status_rows: u16 = if per_window_status { 0 } else { 1 };
    let separate_status =
        per_window_status && !engine.settings.status_line_above_terminal && bottom_panel_open;
    let separated_status_rows: u16 = if separate_status { 1 } else { 0 };
    let content_rows = area.height.saturating_sub(
        1 + global_status_rows
            + qf_height
            + term_height
            + menu_height
            + dbg_height
            + wildmenu_height
            + separated_status_rows,
    ); // cmd(1) + optional status(1) + panels + separated status
    let sidebar_cols = if sidebar.visible {
        sidebar_width + 1
    } else {
        0
    }; // +1 sep
    let ab_width = if engine.settings.autohide_panels && !sidebar.visible {
        0
    } else {
        ACTIVITY_BAR_WIDTH
    };
    let content_cols = area.width.saturating_sub(ab_width + sidebar_cols);
    let content_bounds = WindowRect::new(0.0, 0.0, content_cols as f64, content_rows as f64);
    let tui_tab_bar_height = if engine.settings.breadcrumbs && !engine.terminal_maximized {
        2.0
    } else {
        1.0
    };
    let (window_rects, _dividers) =
        engine.calculate_group_window_rects(content_bounds, tui_tab_bar_height);
    debug_log!(
        "build_screen: content_rows={} content_cols={} groups={} window_rects={}",
        content_rows,
        content_cols,
        engine.group_layout.leaf_count(),
        window_rects.len()
    );
    for (wid, r) in &window_rects {
        debug_log!(
            "  window {:?}: x={:.1} y={:.1} w={:.1} h={:.1}",
            wid,
            r.x,
            r.y,
            r.width,
            r.height
        );
    }
    let bsl_t0 = std::time::Instant::now();
    let result = build_screen_layout(engine, theme, &window_rects, 1.0, 1.0, true);
    let bsl_elapsed = bsl_t0.elapsed();
    if bsl_elapsed.as_millis() > 10 {
        debug_log!(
            "PERF build_screen_layout: {:.1}ms",
            bsl_elapsed.as_secs_f64() * 1000.0
        );
    }
    result
}

// ─── Frame rendering ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_frame(
    frame: &mut ratatui::Frame,
    screen: &render::ScreenLayout,
    theme: &Theme,
    sidebar: &mut TuiSidebar,
    engine: &Engine,
    sidebar_width: u16,
    quickfix_scroll_top: usize,
    debug_output_scroll: usize,
    folder_picker: Option<&FolderPickerState>,
    quit_confirm: bool,
    quit_confirm_focus: usize,
    close_tab_confirm: bool,
    close_tab_confirm_focus: usize,
    cmd_sel: Option<(usize, usize)>,
    explorer_drop_target: Option<usize>,
    hover_link_rects_out: &mut Vec<(u16, u16, u16, u16, String)>,
    hover_popup_rect_out: &mut Option<(u16, u16, u16, u16)>,
    editor_hover_popup_rect_out: &mut Option<(u16, u16, u16, u16)>,
    editor_hover_link_rects_out: &mut Vec<(u16, u16, u16, u16, String)>,
    editor_hover_scrollbar_out: &mut Option<render::PopupScrollbarHit>,
    tab_visible_counts_out: &mut Vec<(GroupId, usize)>,
    // Phase B.4 Stage 2: backend handle for migrated `draw_*` calls.
    // Set once per frame by the caller (cached theme); the migrated
    // call sites wrap their access in `backend.enter_frame_scope`.
    backend: &mut super::backend::TuiBackend,
) {
    let area = frame.area();

    // ── Top-level: [menu] / [content_area] ──
    let menu_bar_height: u16 = if screen.menu_bar.is_some() { 1 } else { 0 };
    let top_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(menu_bar_height), Constraint::Min(0)])
        .split(area);
    let menu_bar_area = top_chunks[0];
    let content_area = top_chunks[1];

    // ── Horizontal split: [activity_bar] [sidebar?] [editor_col] ─
    // Activity bar and sidebar span full height (like GTK layout).
    let ab_width = if engine.settings.autohide_panels && !sidebar.visible {
        0
    } else {
        ACTIVITY_BAR_WIDTH
    };
    let sidebar_constraint = if sidebar.visible {
        Constraint::Length(sidebar_width + 1) // +1 for separator
    } else {
        Constraint::Length(0)
    };
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(ab_width),
            sidebar_constraint,
            Constraint::Min(0),
        ])
        .split(content_area);
    let activity_area = h_chunks[0];
    let sidebar_sep_area = h_chunks[1];
    let right_col = h_chunks[2];

    // ── Vertical split of editor column: [editor] / [qf?] / [bottom?] / [dbg?] / [wildmenu?] / [status?] / [cmd] ──
    let qf_height: u16 = if screen.quickfix.is_some() { 6 } else { 0 };
    let bottom_panel_open = engine.terminal_open || engine.bottom_panel_open;
    let bottom_panel_height: u16 = if bottom_panel_open {
        let target = super::terminal_target_maximize_rows_tui(engine, area.height);
        engine.effective_terminal_panel_rows(target) + 2
    } else {
        0
    };
    let debug_toolbar_height: u16 = if screen.debug_toolbar.is_some() { 1 } else { 0 };
    let wildmenu_height: u16 = if screen.wildmenu.is_some() { 1 } else { 0 };
    let per_window_status = engine.settings.window_status_line;
    let global_status_height: u16 = if per_window_status { 0 } else { 1 };
    let has_separated = screen.separated_status_line.is_some();
    let separated_status_height: u16 = if has_separated { 1 } else { 0 };

    // Layout: [editor][qf][terminal][debug][sep_status?][wildmenu][global_status][cmd]
    // When noslat + terminal open, sep_status(1) shows between debug and wildmenu.
    // When slat (default) or no terminal, sep_status is 0 and per-window bars are inside windows.
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),                          // 0: editor
            Constraint::Length(qf_height),               // 1: quickfix
            Constraint::Length(bottom_panel_height),     // 2: terminal
            Constraint::Length(debug_toolbar_height),    // 3: debug toolbar
            Constraint::Length(separated_status_height), // 4: separated status (0 or 1)
            Constraint::Length(wildmenu_height),         // 5: wildmenu
            Constraint::Length(global_status_height),    // 6: global status
            Constraint::Length(1),                       // 7: cmd
        ])
        .split(right_col);
    let editor_col = v_chunks[0];
    let quickfix_area = v_chunks[1];
    let bottom_panel_area = v_chunks[2];
    let debug_toolbar_area = v_chunks[3];
    let separated_status_area = v_chunks[4];
    let wildmenu_area = v_chunks[5];
    let status_area = v_chunks[6];
    let cmd_area = v_chunks[7];

    // The editor column includes the tab bar row(s).  Window rects from
    // calculate_group_window_rects already have y >= 1 (tab_bar_height offset),
    // so the tab bar occupies row 0 and windows start at row 1 automatically.
    let editor_area = editor_col;

    // ── Render menu bar strip (if visible) ───────────────────────────────────
    if let Some(ref menu_data) = screen.menu_bar {
        render_menu_bar(frame.buffer_mut(), menu_bar_area, menu_data, theme);
        // Note: dropdown is rendered LAST (after all content) so it draws on top.
    }

    // ── Render activity bar ───────────────────────────────────────────────────
    render_activity_bar(
        frame.buffer_mut(),
        activity_area,
        sidebar,
        theme,
        engine.menu_bar_visible,
        engine,
    );

    // ── Render sidebar + separator ────────────────────────────────────────────
    if sidebar.visible && sidebar_sep_area.width > 1 {
        let sidebar_area = Rect {
            x: sidebar_sep_area.x,
            y: sidebar_sep_area.y,
            width: sidebar_sep_area.width - 1,
            height: sidebar_sep_area.height,
        };
        let sep_x = sidebar_sep_area.x + sidebar_sep_area.width - 1;

        render_sidebar(
            backend,
            frame,
            sidebar_area,
            sidebar,
            engine,
            theme,
            explorer_drop_target,
        );
        // Note: render_sidebar / render_search_panel write back scroll_top to sidebar

        // Separator column
        let sep_fg = rc(theme.separator);
        let sep_bg = rc(theme.background);
        for y in sidebar_sep_area.y..sidebar_sep_area.y + sidebar_sep_area.height {
            set_cell(frame.buffer_mut(), sep_x, y, '│', sep_fg, sep_bg);
        }
    }

    // ── Render editor ─────────────────────────────────────────────────────────
    if let Some(ref split) = screen.editor_group_split {
        debug_log!(
            "draw_frame split: editor_area=({},{},{}x{}) groups={}",
            editor_area.x,
            editor_area.y,
            editor_area.width,
            editor_area.height,
            split.group_tab_bars.len()
        );
        for (idx, gtb) in split.group_tab_bars.iter().enumerate() {
            debug_log!(
                "  group[{}] id={:?} bounds=({:.1},{:.1},{:.1}x{:.1}) tabs={}",
                idx,
                gtb.group_id,
                gtb.bounds.x,
                gtb.bounds.y,
                gtb.bounds.width,
                gtb.bounds.height,
                gtb.tabs.len()
            );
        }
        // Render windows first so tab bars draw on top (prevents window content
        // from overwriting an adjacent group's tab bar in horizontal splits).
        render_all_windows(backend, frame, editor_area, &screen.windows, theme);
        // Draw each group's tab bar.  Tab bar sits tab_bar_height rows above
        // the group's window content (bounds.y - tab_bar_height).
        let tui_tbh: u16 = if engine.settings.breadcrumbs && !engine.terminal_maximized {
            2
        } else {
            1
        };
        for gtb in split.group_tab_bars.iter() {
            if engine.is_tab_bar_hidden(gtb.group_id) {
                continue;
            }
            let tab_x = gtb.bounds.x as u16 + editor_area.x;
            let tab_w = gtb.bounds.width as u16;
            let is_active = gtb.group_id == split.active_group;
            let show_split = is_active;
            if tab_w > 0 {
                let bar_y = editor_area.y + (gtb.bounds.y as u16).saturating_sub(tui_tbh);
                let g_tab = Rect {
                    x: tab_x,
                    y: bar_y,
                    width: tab_w,
                    height: 1,
                };
                let accent = if is_active {
                    Some(rc(theme.tab_active_accent))
                } else {
                    None
                };
                let vis = render_tab_bar(
                    backend,
                    frame,
                    g_tab,
                    &gtb.tabs,
                    theme,
                    show_split,
                    gtb.diff_toolbar.as_ref(),
                    gtb.tab_scroll_offset,
                    accent,
                );
                tab_visible_counts_out.push((gtb.group_id, vis));
            }
        }
        // Draw breadcrumb bars (below each group's tab bar). Hidden while the
        // terminal panel is maximized so it can claim the row.
        for bc in &screen.breadcrumbs {
            if bc.segments.is_empty() || engine.terminal_maximized {
                continue;
            }
            let bc_x = bc.bounds.x as u16 + editor_area.x;
            let bc_w = bc.bounds.width as u16;
            // Breadcrumb bar is one row above the window content (bounds.y - 1 in
            // breadcrumb coordinates, which is one row below the tab bar).
            let bc_y = editor_area.y + bc.bounds.y as u16;
            // In multi-group with breadcrumbs, bounds.y points to the breadcrumb row
            // (tab_bar_height=2 means row 0=tab, row 1=breadcrumb, row 2+=windows).
            // The breadcrumb bounds.y is window min_y, so the bc sits 1 above.
            let bc_y = bc_y.saturating_sub(1);
            if bc_w > 0 {
                let bc_rect = Rect {
                    x: bc_x,
                    y: bc_y,
                    width: bc_w,
                    height: 1,
                };
                draw_breadcrumb_bar(
                    backend,
                    frame,
                    bc_rect,
                    &bc.segments,
                    theme,
                    engine.breadcrumb_focus,
                    engine.breadcrumb_selected,
                );
            }
        }
        // Draw divider lines (vertical only — horizontal splits use the tab bar as divider).
        let sep_fg = rc(theme.separator);
        let sep_bg = rc(theme.background);
        for div in &split.dividers {
            if div.direction == SplitDirection::Vertical {
                let div_x = editor_area.x + div.position as u16;
                let y_start = editor_area.y + div.cross_start as u16;
                let y_end = y_start + div.cross_size as u16;
                for y in y_start..y_end {
                    if div_x < editor_area.x + editor_area.width {
                        set_cell(frame.buffer_mut(), div_x, y, '│', sep_fg, sep_bg);
                    }
                }
            }
        }
    } else {
        // Single group: tab bar at row 0 of editor_area, windows at row 1+.
        if !engine.is_tab_bar_hidden(engine.active_group) {
            let tab_rect = Rect {
                x: editor_area.x,
                y: editor_area.y,
                width: editor_area.width,
                height: 1,
            };
            let vis = render_tab_bar(
                backend,
                frame,
                tab_rect,
                &screen.tab_bar,
                theme,
                true,
                screen.diff_toolbar.as_ref(),
                screen.tab_scroll_offset,
                Some(rc(theme.tab_active_accent)),
            );
            tab_visible_counts_out.push((engine.active_group, vis));
        }
        // Draw breadcrumb bar for the single group. Hidden while the terminal
        // panel is maximized.
        if let Some(bc) = screen.breadcrumbs.first() {
            if !bc.segments.is_empty() && !engine.terminal_maximized {
                let bc_y = if engine.is_tab_bar_hidden(engine.active_group) {
                    editor_area.y
                } else {
                    editor_area.y + 1
                };
                let bc_rect = Rect {
                    x: editor_area.x,
                    y: bc_y,
                    width: editor_area.width,
                    height: 1,
                };
                draw_breadcrumb_bar(
                    backend,
                    frame,
                    bc_rect,
                    &bc.segments,
                    theme,
                    engine.breadcrumb_focus,
                    engine.breadcrumb_selected,
                );
            }
        }
        render_all_windows(backend, frame, editor_area, &screen.windows, theme);
    }

    // ── Tab drag overlay ────────────────────────────────────────────────────
    if engine.tab_drag.is_some() {
        render_tab_drag_overlay(frame, engine, editor_area, screen, theme);
    }

    // ── Tab hover tooltip (rendered on top of editor, below tab bar) ──────
    if let Some(ref tooltip_text) = screen.tab_tooltip {
        let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
        let tooltip_row = menu_rows + 1; // just below the tab bar row
        let len = tooltip_text.chars().count() as u16;
        // Position at the right edge of the editor area, or where the tooltip fits.
        let tooltip_x = editor_area.x;
        let tooltip_w = len.min(editor_area.width);
        let fg = rc(theme.hover_fg);
        let bg = rc(theme.hover_bg);
        for dx in 0..tooltip_w {
            let ch = tooltip_text.chars().nth(dx as usize).unwrap_or(' ');
            set_cell(frame.buffer_mut(), tooltip_x + dx, tooltip_row, ch, fg, bg);
        }
    }

    // ── Completion popup (rendered on top of editor) ───────────────────────
    if let Some(ref menu) = screen.completion {
        if let Some(active_win) = screen
            .windows
            .iter()
            .find(|w| w.window_id == screen.active_window_id)
        {
            if let Some((cursor_pos, _)) = &active_win.cursor {
                let gutter_w = active_win.gutter_char_width as u16;
                let win_x = editor_area.x + active_win.rect.x as u16;
                let win_y = editor_area.y + active_win.rect.y as u16;
                let raw = active_win
                    .lines
                    .get(cursor_pos.view_line)
                    .map(|l| l.raw_text.as_str())
                    .unwrap_or("");
                let vis_col = char_col_to_visual(raw, cursor_pos.col, active_win.tabstop)
                    .saturating_sub(active_win.scroll_left) as u16;
                let popup_x = win_x + gutter_w + vis_col;
                let popup_y = win_y + cursor_pos.view_line as u16 + 1;
                // Per D6: build quadraui::Completions + layout + rasterise.
                let completions = render::completion_menu_to_quadraui_completions(menu);
                let area = frame.area();
                let viewport = quadraui::Rect::new(
                    area.x as f32,
                    area.y as f32,
                    area.width as f32,
                    area.height as f32,
                );
                let popup_width = (menu.max_width as f32 + 4.0).max(12.0);
                let max_popup_height = 10.0;
                let layout = completions.layout(
                    popup_x as f32,
                    popup_y as f32 - 1.0, // cursor y; layout adds line_height below
                    1.0,
                    viewport,
                    popup_width,
                    max_popup_height,
                    |_| quadraui::CompletionItemMeasure::new(1.0),
                );
                super::quadraui_tui::draw_completions(
                    frame.buffer_mut(),
                    &completions,
                    &layout,
                    theme,
                );
            }
        }
    }

    // ── Hover popup (rendered on top of editor) ──────────────────────────────
    if let Some(ref hover) = screen.hover {
        if let Some(active_win) = screen
            .windows
            .iter()
            .find(|w| w.window_id == screen.active_window_id)
        {
            let gutter_w = active_win.gutter_char_width as u16;
            let win_x = editor_area.x + active_win.rect.x as u16;
            let win_y = editor_area.y + active_win.rect.y as u16;
            let anchor_view = hover.anchor_line.saturating_sub(active_win.scroll_top) as u16;
            let vis_col = hover.anchor_col.saturating_sub(active_win.scroll_left) as u16;
            let popup_x = win_x + gutter_w + vis_col;
            let popup_y = win_y + anchor_view;
            // Per D6: build quadraui::Tooltip + layout + rasterise.
            let area = frame.area();
            let viewport = quadraui::Rect::new(
                area.x as f32,
                area.y as f32,
                area.width as f32,
                area.height as f32,
            );
            let (tooltip, layout) =
                render::hover_popup_to_quadraui_tooltip(hover, popup_x, popup_y, viewport);
            super::quadraui_tui::draw_tooltip(frame.buffer_mut(), &tooltip, &layout, theme);
        }
    }

    // ── Editor hover popup (rich markdown, triggered by gh or mouse dwell) ─
    *editor_hover_popup_rect_out = None; // Clear stale rect before rendering
    *editor_hover_scrollbar_out = None;
    if let Some(ref eh) = screen.editor_hover {
        if let Some(active_win) = screen
            .windows
            .iter()
            .find(|w| w.window_id == screen.active_window_id)
        {
            let gutter_w = active_win.gutter_char_width as u16;
            let win_x = editor_area.x + active_win.rect.x as u16;
            let win_y = editor_area.y + active_win.rect.y as u16;
            // Use frozen scroll offsets so the popup stays fixed on screen
            let anchor_view = eh.anchor_line.saturating_sub(eh.frozen_scroll_top) as u16;
            let vis_col = eh.anchor_col.saturating_sub(eh.frozen_scroll_left) as u16;
            let popup_x = win_x + gutter_w + vis_col;
            let popup_y = win_y + anchor_view;
            let (eh_links, eh_rect, eh_sb) =
                render_editor_hover_popup(frame, eh, popup_x, popup_y, frame.area(), theme);
            *editor_hover_link_rects_out = eh_links;
            *editor_hover_popup_rect_out = eh_rect;
            *editor_hover_scrollbar_out = eh_sb;
        }
    }

    // ── Diff peek popup (inline git hunk preview) ──────────────────────────
    if let Some(ref peek) = screen.diff_peek {
        if let Some(active_win) = screen
            .windows
            .iter()
            .find(|w| w.window_id == screen.active_window_id)
        {
            let gutter_w = active_win.gutter_char_width as u16;
            let win_x = editor_area.x + active_win.rect.x as u16;
            let win_y = editor_area.y + active_win.rect.y as u16;
            let anchor_view = peek.anchor_line.saturating_sub(active_win.scroll_top) as u16;
            let popup_x = win_x + gutter_w;
            // anchor at the cursor's own row; placement=Bottom (with
            // primitive fallback to Top) puts the popup just below it.
            let popup_y = win_y + anchor_view;
            // Per D6: build quadraui::Tooltip + layout + rasterise.
            let area = frame.area();
            let viewport = quadraui::Rect::new(
                area.x as f32,
                area.y as f32,
                area.width as f32,
                area.height as f32,
            );
            let (tooltip, layout) =
                render::diff_peek_to_quadraui_tooltip(peek, popup_x, popup_y, viewport, theme);
            super::quadraui_tui::draw_tooltip(frame.buffer_mut(), &tooltip, &layout, theme);
        }
    }

    // ── Signature-help popup (shown in insert mode when cursor is inside a call) ─
    if let Some(ref sig) = screen.signature_help {
        if let Some(active_win) = screen
            .windows
            .iter()
            .find(|w| w.window_id == screen.active_window_id)
        {
            let gutter_w = active_win.gutter_char_width as u16;
            let win_x = editor_area.x + active_win.rect.x as u16;
            let win_y = editor_area.y + active_win.rect.y as u16;
            let anchor_view = sig.anchor_line.saturating_sub(active_win.scroll_top) as u16;
            let vis_col = sig.anchor_col.saturating_sub(active_win.scroll_left) as u16;
            let popup_x = win_x + gutter_w + vis_col;
            let popup_y = win_y + anchor_view;
            // Per D6: build quadraui::Tooltip + layout + rasterise.
            let area = frame.area();
            let viewport = quadraui::Rect::new(
                area.x as f32,
                area.y as f32,
                area.width as f32,
                area.height as f32,
            );
            let (tooltip, layout) =
                render::signature_help_to_quadraui_tooltip(sig, popup_x, popup_y, viewport, theme);
            super::quadraui_tui::draw_tooltip(frame.buffer_mut(), &tooltip, &layout, theme);
        }
    }

    // ── Quickfix panel (persistent bottom strip) ──────────────────────────────
    if let Some(ref qf) = screen.quickfix {
        render_quickfix_panel(
            frame,
            quickfix_area,
            qf,
            quickfix_scroll_top,
            theme,
            backend,
        );
    }

    // ── Separated status line (above terminal, when status_line_above_terminal is active) ──
    if let Some(ref status) = screen.separated_status_line {
        render_window_status_line(
            backend,
            frame,
            separated_status_area.x,
            separated_status_area.y,
            separated_status_area.width,
            status,
            theme,
        );
    }

    // ── Bottom panel (tab bar + terminal or debug output) ────────────────────
    if bottom_panel_area.height > 0 {
        // Tab bar (first row)
        let tab_bar_area = Rect {
            x: bottom_panel_area.x,
            y: bottom_panel_area.y,
            width: bottom_panel_area.width,
            height: 1,
        };
        let content_area = Rect {
            x: bottom_panel_area.x,
            y: bottom_panel_area.y + 1,
            width: bottom_panel_area.width,
            height: bottom_panel_area.height.saturating_sub(1),
        };
        render_bottom_panel_tabs(
            frame.buffer_mut(),
            tab_bar_area,
            engine.bottom_panel_kind.clone(),
            engine.terminal_open,
            !screen.bottom_tabs.output_lines.is_empty(),
            theme,
        );
        match engine.bottom_panel_kind {
            render::BottomPanelKind::Terminal => {
                if let Some(ref term) = screen.bottom_tabs.terminal {
                    render_terminal_panel(frame.buffer_mut(), content_area, term, theme);
                }
            }
            render::BottomPanelKind::DebugOutput => {
                render_debug_output(
                    frame.buffer_mut(),
                    content_area,
                    &screen.bottom_tabs.output_lines,
                    debug_output_scroll,
                    theme,
                );
            }
        }
    }

    // ── Debug toolbar strip (if visible) ────────────────────────────────────
    if let Some(ref toolbar) = screen.debug_toolbar {
        // B5c.1: route through the `Backend` trait. `draw_status_bar`
        // computes layout internally with the cell measurer.
        let bar = render::debug_toolbar_to_quadraui_status_bar(toolbar, theme);
        let q_rect = quadraui::Rect::new(
            debug_toolbar_area.x as f32,
            debug_toolbar_area.y as f32,
            debug_toolbar_area.width as f32,
            debug_toolbar_area.height as f32,
        );
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        backend.enter_frame_scope(frame, |b| {
            use quadraui::Backend;
            let _ = b.draw_status_bar(q_rect, &bar);
        });
    }

    // ── Wildmenu bar (command Tab completion) ─────────────────────────────────
    if let Some(ref wm) = screen.wildmenu {
        render_wildmenu(frame.buffer_mut(), wildmenu_area, wm, theme);
    }

    // ── Status / command ──────────────────────────────────────────────────────
    if !per_window_status {
        render_status_line(
            frame.buffer_mut(),
            status_area,
            &screen.status_left,
            &screen.status_right,
            theme,
        );
    }

    render_command_line(frame.buffer_mut(), cmd_area, &screen.command, theme);
    // Highlight command-line mouse selection (invert fg/bg for selected cells)
    if let Some((start, end)) = cmd_sel {
        let lo = start.min(end);
        let hi = start.max(end);
        let buf = frame.buffer_mut();
        for i in lo..=hi {
            let cx = cmd_area.x + i as u16;
            if cx < cmd_area.x + cmd_area.width {
                let cell = &mut buf[(cx, cmd_area.y)];
                let old_fg = cell.fg;
                let old_bg = cell.bg;
                cell.set_fg(old_bg).set_bg(old_fg);
            }
        }
    }

    // ── Panel hover popup (drawn after editor so it's not overwritten) ─────
    hover_link_rects_out.clear();
    *hover_popup_rect_out = None;
    if sidebar.visible && sidebar_sep_area.width > 1 {
        let sep_x = sidebar_sep_area.x + sidebar_sep_area.width - 1;
        if sidebar.ext_panel_name.is_some() || sidebar.active_panel == TuiPanel::Git {
            let (rects, popup_rect) = render_panel_hover_popup(
                frame,
                screen,
                theme,
                sep_x + 1,
                sidebar_sep_area.y,
                sidebar_sep_area.height,
                area,
            );
            *hover_link_rects_out = rects;
            *hover_popup_rect_out = popup_rect;
        }
    }

    // ── Folder / workspace picker modal ──────────────────────────────────────
    if let Some(picker) = folder_picker {
        // Sizing identical to the legacy popup: 60% of viewport
        // width clamped to >= 50; 55% of viewport height clamped to >= 15.
        let term_cols = area.width;
        let term_rows = area.height;
        let width = (term_cols * 3 / 5).max(50);
        let height = (term_rows * 55 / 100).max(15);
        let popup_x = (term_cols.saturating_sub(width)) / 2;
        let popup_y = (term_rows.saturating_sub(height)) / 2;
        let popup_area = Rect {
            x: popup_x,
            y: popup_y,
            width,
            height,
        };
        // Per D6: build quadraui::Palette + draw_palette.
        // Phase B.4 Stage 2: route through `Backend::draw_palette`.
        let palette = folder_picker_to_palette(picker, width as usize);
        let q_rect = quadraui::Rect::new(
            popup_area.x as f32,
            popup_area.y as f32,
            popup_area.width as f32,
            popup_area.height as f32,
        );
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        backend.enter_frame_scope(frame, |b| {
            use quadraui::Backend;
            b.draw_palette(q_rect, &palette);
        });
    }

    // ── Find/replace overlay (top-right of active group) ───────────────────
    if let Some(ref find_replace) = screen.find_replace {
        let editor_left = h_chunks[0].width + h_chunks[1].width;
        super::quadraui_tui::draw_find_replace(
            frame.buffer_mut(),
            area,
            find_replace,
            theme,
            editor_left,
        );
    }

    // ── Unified picker modal (above terminal/status so it's fully visible) ──
    if let Some(ref picker) = screen.picker {
        render_picker_popup(frame, picker, area, theme, backend);
    }

    // ── Tab switcher popup ───────────────────────────────────────────────────
    if let Some(ref ts) = screen.tab_switcher {
        if !ts.items.is_empty() {
            // Sizing identical to the legacy popup: 45% of viewport
            // width clamped to [40, 80]; height = visible_items + 2
            // (top + bottom border rows). The bordered ListView's own
            // layout reserves rows 0 and N-1 for borders.
            let term_w = area.width;
            let term_h = area.height;
            let width = (term_w * 45 / 100).clamp(40, 80);
            let max_visible = (term_h as usize).saturating_sub(4).min(20);
            let visible = ts.items.len().min(max_visible);
            let height = visible as u16 + 2;
            let x = (term_w.saturating_sub(width)) / 2;
            let y = (term_h.saturating_sub(height)) / 2;
            let popup_area = Rect {
                x,
                y,
                width,
                height,
            };
            // Per D6: build quadraui::ListView (bordered) + draw_list.
            // Phase B.4 Stage 2: route through `Backend::draw_list`.
            let list = render::tab_switcher_to_quadraui_list_view(ts, max_visible);
            let q_rect = quadraui::Rect::new(
                popup_area.x as f32,
                popup_area.y as f32,
                popup_area.width as f32,
                popup_area.height as f32,
            );
            backend.set_current_theme(super::quadraui_tui::q_theme(theme));
            backend.enter_frame_scope(frame, |b| {
                use quadraui::Backend;
                b.draw_list(q_rect, &list);
            });
        }
    }

    // ── Context menu popup (above status/command line) ─────────────────────
    if let Some(ref ctx_menu) = screen.context_menu {
        // Per D6: build quadraui::ContextMenu, ask for layout, rasterise.
        // The layout describes the INNER items region; the rasteriser
        // draws a 1-cell box border around it, so we inset the anchor
        // by (1, 1) and shrink the menu_width by 2.
        let menu = render::context_menu_panel_to_quadraui_context_menu(ctx_menu);
        // Shrink the viewport by 1 on each side so layout clamping
        // accounts for the 1-cell border chrome the rasteriser draws
        // outside layout.bounds — otherwise the right/bottom border
        // can extend past the screen on narrow windows.
        let inner_viewport = quadraui::Rect::new(
            (area.x + 1) as f32,
            (area.y + 1) as f32,
            area.width.saturating_sub(2) as f32,
            area.height.saturating_sub(2) as f32,
        );
        let max_label = ctx_menu
            .items
            .iter()
            .map(|i| i.label.len())
            .max()
            .unwrap_or(4);
        let max_shortcut = ctx_menu
            .items
            .iter()
            .map(|i| i.shortcut.len())
            .max()
            .unwrap_or(0);
        let outer_width = (max_label + max_shortcut + 6).clamp(20, 50) as f32;
        let inner_width = (outer_width - 2.0).max(1.0);
        let layout = menu.layout(
            ctx_menu.screen_col as f32 + 1.0,
            ctx_menu.screen_row as f32 + 1.0,
            inner_viewport,
            inner_width,
            |_| quadraui::ContextMenuItemMeasure::new(1.0),
        );
        super::quadraui_tui::draw_context_menu(frame.buffer_mut(), &menu, &layout, theme);
    }

    // ── Modal dialog (highest z-order after quit confirm) ────────────────────
    if let Some(ref dialog) = screen.dialog {
        // Per D6: build quadraui::Dialog primitive, ask it for a layout,
        // then hand both to the rasteriser.
        let q_dialog = render::dialog_panel_to_quadraui_dialog(dialog);
        let viewport = quadraui::Rect::new(
            area.x as f32,
            area.y as f32,
            area.width as f32,
            area.height as f32,
        );
        // Compute dimensions the same way the legacy draw did.
        let body_max = dialog
            .body
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0);
        let btn_max_label = dialog
            .buttons
            .iter()
            .map(|(lbl, _)| lbl.chars().count() + 4)
            .max()
            .unwrap_or(0);
        let btn_row_len: usize = if dialog.vertical_buttons {
            btn_max_label + 2
        } else {
            dialog
                .buttons
                .iter()
                .map(|(lbl, _)| lbl.chars().count() + 4)
                .sum::<usize>()
                + 2
        };
        let content_width = body_max
            .max(dialog.title.chars().count() + 4)
            .max(btn_row_len);
        let width = (content_width as u16 + 4).clamp(40, area.width.saturating_sub(4)) as f32;
        let measure = quadraui::DialogMeasure {
            width,
            title_height: 1.0,
            body_height: dialog.body.len() as f32,
            input_height: if dialog.input.is_some() { 2.0 } else { 0.0 },
            button_row_height: if dialog.vertical_buttons {
                dialog.buttons.len() as f32
            } else {
                1.0
            },
            button_width: btn_max_label as f32,
            button_gap: 0.0,
            padding: 1.0,
        };
        let layout = q_dialog.layout(viewport, measure);
        super::quadraui_tui::draw_dialog(frame.buffer_mut(), &q_dialog, &layout, theme);
    }

    // ── Menu dropdown — rendered last so it draws on top of everything ────────
    if let Some(ref menu_data) = screen.menu_bar {
        if let Some(menu) = render::menu_dropdown_to_quadraui_context_menu(menu_data) {
            // Sizing matches legacy: max(label) + max(shortcut) + 6 padding,
            // clamped to [20, 50]. Inner width = outer - 2 for borders.
            let max_label = menu_data
                .open_items
                .iter()
                .map(|i| i.label.len())
                .max()
                .unwrap_or(4);
            let max_shortcut = menu_data
                .open_items
                .iter()
                .map(|i| {
                    if menu_data.is_vscode_mode && !i.vscode_shortcut.is_empty() {
                        i.vscode_shortcut.len()
                    } else {
                        i.shortcut.len()
                    }
                })
                .max()
                .unwrap_or(0);
            let outer_width = (max_label + max_shortcut + 6).clamp(20, 50) as f32;
            let inner_width = (outer_width - 2.0).max(1.0);
            // The inner anchor sits 1 cell inside the outer box. Outer box
            // starts at column `open_menu_col` (under the menu label) on
            // the row directly below the menu-bar strip.
            let anchor_x = menu_data.open_menu_col as f32 + 1.0;
            let menu_bar_bottom = menu_bar_area.y + menu_bar_area.height;
            let anchor_y = menu_bar_bottom as f32 + 1.0;
            // Shrink the viewport by 1 on each side so layout clamping
            // accounts for the 1-cell border chrome the rasteriser draws
            // outside layout.bounds.
            let inner_viewport = quadraui::Rect::new(
                (area.x + 1) as f32,
                (area.y + 1) as f32,
                area.width.saturating_sub(2) as f32,
                area.height.saturating_sub(2) as f32,
            );
            let layout = menu.layout(anchor_x, anchor_y, inner_viewport, inner_width, |_| {
                quadraui::ContextMenuItemMeasure::new(1.0)
            });
            super::quadraui_tui::draw_context_menu(frame.buffer_mut(), &menu, &layout, theme);
        }
    }

    // ── Quit confirm overlay — rendered on top of absolutely everything ───────
    if quit_confirm {
        let (dialog, layout) = build_quit_confirm_dialog(area, quit_confirm_focus);
        super::quadraui_tui::draw_dialog(frame.buffer_mut(), &dialog, &layout, theme);
    }

    // ── Close-tab confirm overlay ──────────────────────────────────────────────
    if close_tab_confirm {
        let (dialog, layout) = build_close_tab_dialog(area, close_tab_confirm_focus);
        super::quadraui_tui::draw_dialog(frame.buffer_mut(), &dialog, &layout, theme);
    }
}

/// Build the close-tab-confirm Dialog primitive + its resolved layout.
/// Shared between the draw site (`render_window` above) and the mouse
/// hit-test site (`mouse.rs`) so a button's visual rect and its click
/// rect are identical by construction.
/// Button indices in the close-tab confirm dialog. Tab / arrow keys
/// cycle `focus_idx` through these in order.
pub(super) const CLOSE_TAB_BTN_COUNT: usize = 3;

/// Button indices in the quit-confirm dialog (unsaved-changes-on-exit).
/// Tab / arrow keys cycle `focus_idx` through these in order:
/// 0 = Save All & Quit, 1 = Quit Anyway, 2 = Cancel.
pub(super) const QUIT_CONFIRM_BTN_COUNT: usize = 3;

/// Build the quit-confirm Dialog primitive + its resolved layout.
/// Shared between the draw site (`render_window` above) and the mouse
/// hit-test site (`mouse.rs`) so a button's visual rect and its click
/// rect are identical by construction.
pub(super) fn build_quit_confirm_dialog(
    area: Rect,
    focus_idx: usize,
) -> (quadraui::Dialog, quadraui::DialogLayout) {
    let focus = focus_idx.min(QUIT_CONFIRM_BTN_COUNT - 1);
    let dialog = quadraui::Dialog {
        id: quadraui::WidgetId::new("quit_confirm"),
        title: quadraui::StyledText::plain("Unsaved Changes"),
        body: quadraui::StyledText::plain("You have unsaved changes. Quit anyway?"),
        buttons: vec![
            quadraui::DialogButton {
                id: quadraui::WidgetId::new("quit:save_all"),
                label: "[S] Save All & Quit".to_string(),
                is_default: focus == 0,
                is_cancel: false,
                tint: None,
            },
            quadraui::DialogButton {
                id: quadraui::WidgetId::new("quit:force"),
                label: "[Q] Quit Anyway".to_string(),
                is_default: focus == 1,
                is_cancel: false,
                tint: None,
            },
            quadraui::DialogButton {
                id: quadraui::WidgetId::new("quit:cancel"),
                label: "[Esc] Cancel".to_string(),
                is_default: focus == 2,
                is_cancel: true,
                tint: None,
            },
        ],
        severity: Some(quadraui::DialogSeverity::Warning),
        vertical_buttons: false,
        input: None,
    };
    let viewport = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    let measure = quadraui::DialogMeasure {
        width: 58.0,
        title_height: 1.0,
        body_height: 1.0,
        input_height: 0.0,
        button_row_height: 1.0,
        button_width: 22.0,
        button_gap: 2.0,
        padding: 1.0,
    };
    let layout = dialog.layout(viewport, measure);
    (dialog, layout)
}

pub(super) fn build_close_tab_dialog(
    area: Rect,
    focus_idx: usize,
) -> (quadraui::Dialog, quadraui::DialogLayout) {
    let focus = focus_idx.min(CLOSE_TAB_BTN_COUNT - 1);
    let dialog = quadraui::Dialog {
        id: quadraui::WidgetId::new("close_tab_confirm"),
        title: quadraui::StyledText::plain("Unsaved Changes"),
        body: quadraui::StyledText::plain("This file has unsaved changes."),
        buttons: vec![
            quadraui::DialogButton {
                id: quadraui::WidgetId::new("close_tab:save"),
                label: "[S] Save".to_string(),
                is_default: focus == 0,
                is_cancel: false,
                tint: None,
            },
            quadraui::DialogButton {
                id: quadraui::WidgetId::new("close_tab:discard"),
                label: "[D] Discard".to_string(),
                is_default: focus == 1,
                is_cancel: false,
                tint: None,
            },
            quadraui::DialogButton {
                id: quadraui::WidgetId::new("close_tab:cancel"),
                label: "[Esc] Cancel".to_string(),
                is_default: focus == 2,
                is_cancel: true,
                tint: None,
            },
        ],
        severity: Some(quadraui::DialogSeverity::Warning),
        vertical_buttons: false,
        input: None,
    };
    let viewport = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    let measure = quadraui::DialogMeasure {
        width: 54.0,
        title_height: 1.0,
        body_height: 1.0,
        input_height: 0.0,
        button_row_height: 1.0,
        button_width: 14.0,
        button_gap: 2.0,
        padding: 1.0,
    };
    let layout = dialog.layout(viewport, measure);
    (dialog, layout)
}

/// Convert a TUI-local `FolderPickerState` into a `quadraui::Palette`.
///
/// FolderPickerState lives in the TUI module (it's not portable across
/// backends yet), so this adapter is also TUI-local instead of in
/// `render.rs`. Title format mirrors the legacy popup:
///
/// - `OpenFolder`: `" Open Folder <truncated-root>  N/M "`
/// - `OpenRecent`: `" Open Recent  N "`
///
/// Each entry becomes a `PaletteItem` with an icon (📁 for folders,
/// ⚙ for `.vimcode-workspace` files) and the path as the primary text.
/// `query_cursor` is set to the end of the query (no internal-edit
/// cursor model in the TUI picker yet). `total_count` enables the
/// `N/M` chip in the title via `draw_palette`.
fn folder_picker_to_palette(picker: &FolderPickerState, popup_width: usize) -> quadraui::Palette {
    use quadraui::{Icon, Palette, PaletteItem, StyledText, WidgetId};

    // Build title — matches the legacy folder-picker title format.
    let title = match picker.mode {
        FolderPickerMode::OpenFolder => {
            let r = picker.root.to_string_lossy();
            // Truncate from the left if too long. Reserve ~30 cells of
            // chrome for the borders + count chip + padding.
            let max = popup_width.saturating_sub(30).max(10);
            let root_display = if r.len() > max {
                format!("…{}", &r[r.len() - max..])
            } else {
                r.into_owned()
            };
            format!("Open Folder {}", root_display)
        }
        FolderPickerMode::OpenRecent => "Open Recent".to_string(),
    };

    let folder_icon = Icon {
        glyph: "📁".to_string(),
        fallback: "📁".to_string(),
    };
    let workspace_icon = Icon {
        glyph: "⚙".to_string(),
        fallback: "⚙".to_string(),
    };

    let items: Vec<PaletteItem> = picker
        .filtered
        .iter()
        .map(|entry| {
            let is_workspace = entry
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == ".vimcode-workspace")
                .unwrap_or(false);
            PaletteItem {
                text: StyledText::plain(entry.to_string_lossy().to_string()),
                detail: None,
                icon: Some(if is_workspace {
                    workspace_icon.clone()
                } else {
                    folder_icon.clone()
                }),
                match_positions: Vec::new(),
            }
        })
        .collect();

    Palette {
        id: WidgetId::new("folder_picker"),
        title,
        query: picker.query.clone(),
        query_cursor: picker.query.len(),
        items,
        selected_idx: picker.selected,
        scroll_offset: picker.scroll_top,
        total_count: picker.all_entries.len(),
        has_focus: true,
    }
}

// ─── Tab bar constants ───────────────────────────────────────────────────────

/// Terminal columns used by each tab's close button (the × itself + trailing space).
/// The glyph itself lives in `quadraui::tui::TAB_CLOSE_CHAR` since the public
/// rasteriser owns the painting.
pub(super) const TAB_CLOSE_COLS: u16 = 2;

/// Given a column within a group's tab bar, return the shortened file path of
/// the tab at that column, or `None` if the column doesn't hit a tab with a file.
pub(super) fn tab_tooltip_at_col(
    engine: &Engine,
    group_id: GroupId,
    local_col: u16,
    tabs: &[render::TabInfo],
    tab_scroll_offset: usize,
) -> Option<String> {
    let overflow_cols: u16 = if tab_scroll_offset > 0 { 2 } else { 0 };
    let mut x: u16 = overflow_cols;
    for (i, tab) in tabs.iter().enumerate().skip(tab_scroll_offset) {
        let name_width = tab.name.chars().count() as u16;
        let tab_width = name_width + TAB_CLOSE_COLS;
        if local_col >= x && local_col < x + tab_width {
            // Found the tab — look up its file path.
            let group = engine.editor_groups.get(&group_id)?;
            let tab_data = group.tabs.get(i)?;
            let window = engine.windows.get(&tab_data.active_window)?;
            let state = engine.buffer_manager.get(window.buffer_id)?;
            let raw_path = state.file_path.as_ref()?;
            let path = crate::core::paths::strip_unc_prefix(raw_path);
            let home = crate::core::paths::home_dir();
            if let Ok(rest) = path.strip_prefix(&home) {
                return Some(format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()));
            }
            return Some(path.display().to_string());
        }
        x += tab_width;
    }
    None
}

/// Draw the tab drag-and-drop overlay (highlight drop zone + ghost label).
pub(super) fn render_tab_drag_overlay(
    frame: &mut ratatui::Frame,
    engine: &Engine,
    editor_area: Rect,
    screen: &render::ScreenLayout,
    theme: &render::Theme,
) {
    use crate::core::window::{DropZone, SplitDirection};

    let tui_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
    let zone = engine.tab_drop_zone;

    // Accent color for the drop zone highlight.
    let highlight_bg = RColor::Indexed(24); // dark blue

    // Compute the highlight rectangle in absolute terminal coordinates.
    let highlight: Option<(u16, u16, u16, u16)> = if let Some(ref split) = screen.editor_group_split
    {
        match zone {
            DropZone::Center(gid) => {
                split
                    .group_tab_bars
                    .iter()
                    .find(|g| g.group_id == gid)
                    .map(|g| {
                        let x = editor_area.x + g.bounds.x as u16;
                        let y = editor_area.y + (g.bounds.y as u16).saturating_sub(tui_tbh);
                        let w = g.bounds.width as u16;
                        let h = g.bounds.height as u16 + tui_tbh;
                        (x, y, w, h)
                    })
            }
            DropZone::Split(gid, dir, new_first) => split
                .group_tab_bars
                .iter()
                .find(|g| g.group_id == gid)
                .map(|g| {
                    let x = editor_area.x + g.bounds.x as u16;
                    let full_y = editor_area.y + (g.bounds.y as u16).saturating_sub(tui_tbh);
                    let w = g.bounds.width as u16;
                    let full_h = g.bounds.height as u16 + tui_tbh;
                    match (dir, new_first) {
                        (SplitDirection::Vertical, true) => (x, full_y, w / 2, full_h),
                        (SplitDirection::Vertical, false) => (x + w / 2, full_y, w - w / 2, full_h),
                        (SplitDirection::Horizontal, true) => (x, full_y, w, full_h / 2),
                        (SplitDirection::Horizontal, false) => {
                            (x, full_y + full_h / 2, w, full_h - full_h / 2)
                        }
                    }
                }),
            DropZone::TabReorder(gid, _) => split
                .group_tab_bars
                .iter()
                .find(|g| g.group_id == gid)
                .map(|g| {
                    let x = editor_area.x + g.bounds.x as u16;
                    let y = editor_area.y + (g.bounds.y as u16).saturating_sub(tui_tbh);
                    let w = g.bounds.width as u16;
                    (x, y, w, 1)
                }),
            DropZone::None => None,
        }
    } else {
        let x = editor_area.x;
        let y = editor_area.y;
        let w = editor_area.width;
        let h = editor_area.height;
        match zone {
            DropZone::Center(_) => Some((x, y, w, h)),
            DropZone::Split(_, dir, new_first) => Some(match (dir, new_first) {
                (SplitDirection::Vertical, true) => (x, y, w / 2, h),
                (SplitDirection::Vertical, false) => (x + w / 2, y, w - w / 2, h),
                (SplitDirection::Horizontal, true) => (x, y, w, h / 2),
                (SplitDirection::Horizontal, false) => (x, y + h / 2, w, h - h / 2),
            }),
            DropZone::TabReorder(_, _) => Some((x, y, w, 1)),
            DropZone::None => None,
        }
    };

    // Draw the highlight area.
    if let Some((hx, hy, hw, hh)) = highlight {
        let buf = frame.buffer_mut();
        for dy in 0..hh {
            for dx in 0..hw {
                let cx = hx + dx;
                let cy = hy + dy;
                let area = buf.area;
                if cx < area.x + area.width && cy < area.y + area.height {
                    buf[(cx, cy)].set_bg(highlight_bg);
                }
            }
        }
    }

    // For TabReorder, draw a vertical insertion bar at the target position.
    if let DropZone::TabReorder(gid, idx) = zone {
        let tab_bar_info: Option<(u16, u16, &[render::TabInfo], usize)> =
            if let Some(ref split) = screen.editor_group_split {
                split
                    .group_tab_bars
                    .iter()
                    .find(|g| g.group_id == gid)
                    .map(|g| {
                        let x = editor_area.x + g.bounds.x as u16;
                        let y = editor_area.y + (g.bounds.y as u16).saturating_sub(tui_tbh);
                        (x, y, g.tabs.as_slice(), g.tab_scroll_offset)
                    })
            } else {
                Some((
                    editor_area.x,
                    editor_area.y,
                    screen.tab_bar.as_slice(),
                    screen.tab_scroll_offset,
                ))
            };

        if let Some((bar_x, bar_y, tabs, scroll_off)) = tab_bar_info {
            let ov_cols: u16 = if scroll_off > 0 { 2 } else { 0 };
            let mut insert_x: u16 = ov_cols;
            for (i, tab) in tabs.iter().enumerate().skip(scroll_off) {
                if i == idx {
                    break;
                }
                insert_x += tab.name.chars().count() as u16 + TAB_CLOSE_COLS;
            }
            let abs_x = bar_x + insert_x;
            set_cell_styled(
                frame.buffer_mut(),
                abs_x,
                bar_y,
                '▎',
                RColor::Indexed(39),
                rc(theme.tab_bar_bg),
                Modifier::empty(),
                None,
            );
        }
    }

    // Draw ghost label near cursor.
    if let (Some((mx, my)), Some(ref drag)) = (engine.tab_drag_mouse, &engine.tab_drag) {
        let label = &drag.tab_name;
        if !label.is_empty() {
            let gx = (mx as u16) + 2;
            let gy = my as u16;
            let ghost_fg = RColor::White;
            let ghost_bg = RColor::Indexed(238);
            let buf = frame.buffer_mut();
            for (i, ch) in label.chars().enumerate() {
                let cx = gx + i as u16;
                let area = buf.area;
                if cx < area.x + area.width && gy < area.y + area.height {
                    buf[(cx, gy)].set_char(ch).set_fg(ghost_fg).set_bg(ghost_bg);
                }
            }
        }
    }
}

/// Compute the drop zone for a tab drag in TUI based on cursor cell position.
pub(super) fn compute_tui_tab_drop_zone(
    engine: &Engine,
    col: u16,
    row: u16,
    editor_left: u16,
    last_layout: Option<&render::ScreenLayout>,
    terminal_size: Option<Size>,
) -> crate::core::window::DropZone {
    use crate::core::window::{DropZone, SplitDirection};

    let layout = match last_layout {
        Some(l) => l,
        None => return DropZone::None,
    };

    let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
    let click_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };

    if col < editor_left {
        return DropZone::None;
    }
    let rel_col = col - editor_left;

    if let Some(ref split) = layout.editor_group_split {
        // Multi-group mode: check each group's tab bar and content area.
        for gtb in split.group_tab_bars.iter() {
            let tab_bar_row = menu_rows + (gtb.bounds.y as u16).saturating_sub(click_tbh);
            let gx = gtb.bounds.x as u16;
            let gw = gtb.bounds.width as u16;
            let group_id = gtb.group_id;

            // Tab bar region — determine reorder insertion index.
            if row == tab_bar_row && rel_col >= gx && rel_col < gx + gw {
                let local_col = rel_col - gx;
                let ov_cols: u16 = if gtb.tab_scroll_offset > 0 { 2 } else { 0 };
                let mut x: u16 = ov_cols;
                for (i, tab) in gtb.tabs.iter().enumerate().skip(gtb.tab_scroll_offset) {
                    let name_w = tab.name.chars().count() as u16;
                    let tab_w = name_w + TAB_CLOSE_COLS;
                    let mid = x + tab_w / 2;
                    if local_col < mid {
                        return DropZone::TabReorder(group_id, i);
                    }
                    x += tab_w;
                }
                return DropZone::TabReorder(group_id, gtb.tabs.len());
            }

            // Content area — edge zones for split, center for merge.
            let content_top = menu_rows + gtb.bounds.y as u16;
            let content_left = gx;
            let content_right = gx + gw;
            let content_h = gtb.bounds.height as u16;
            let content_bottom = content_top + content_h;
            if rel_col >= content_left
                && rel_col < content_right
                && row >= content_top
                && row < content_bottom
            {
                let w = gw;
                let h = content_h;
                let rx = rel_col - content_left;
                let ry = row - content_top;
                // Edge zones: ~20% of each dimension, minimum 3 cells.
                let edge_w = (w / 5).max(3).min(w / 2);
                let edge_h = (h / 5).max(2).min(h / 2);

                if rx < edge_w {
                    return DropZone::Split(group_id, SplitDirection::Vertical, true);
                }
                if rx >= w - edge_w {
                    return DropZone::Split(group_id, SplitDirection::Vertical, false);
                }
                if ry < edge_h {
                    return DropZone::Split(group_id, SplitDirection::Horizontal, true);
                }
                if ry >= h - edge_h {
                    return DropZone::Split(group_id, SplitDirection::Horizontal, false);
                }
                return DropZone::Center(group_id);
            }
        }
    } else {
        // Single-group mode: tab bar reorder + content area edge zones for split.
        let group_id = engine.active_group;
        if row == menu_rows {
            let local_col = rel_col;
            let sg_offset = layout.tab_scroll_offset;
            let ov_cols: u16 = if sg_offset > 0 { 2 } else { 0 };
            let mut x: u16 = ov_cols;
            for (i, tab) in layout.tab_bar.iter().enumerate().skip(sg_offset) {
                let name_w = tab.name.chars().count() as u16;
                let tab_w = name_w + TAB_CLOSE_COLS;
                let mid = x + tab_w / 2;
                if local_col < mid {
                    return DropZone::TabReorder(group_id, i);
                }
                x += tab_w;
            }
            return DropZone::TabReorder(group_id, layout.tab_bar.len());
        }

        // Content area — edge zones for split, center for merge.
        if let Some(ts) = terminal_size {
            let content_top = menu_rows + click_tbh;
            let editor_w = ts.width.saturating_sub(editor_left);
            // status + command = 2 rows at bottom
            let content_bottom = ts.height.saturating_sub(2);
            if row >= content_top && row < content_bottom && rel_col < editor_w {
                let w = editor_w;
                let h = content_bottom - content_top;
                let rx = rel_col;
                let ry = row - content_top;
                let edge_w = (w / 5).max(3).min(w / 2);
                let edge_h = (h / 5).max(2).min(h / 2);

                if rx < edge_w {
                    return DropZone::Split(group_id, SplitDirection::Vertical, true);
                }
                if rx >= w - edge_w {
                    return DropZone::Split(group_id, SplitDirection::Vertical, false);
                }
                if ry < edge_h {
                    return DropZone::Split(group_id, SplitDirection::Horizontal, true);
                }
                if ry >= h - edge_h {
                    return DropZone::Split(group_id, SplitDirection::Horizontal, false);
                }
                return DropZone::Center(group_id);
            }
        }
    }

    DropZone::None
}

/// Render the tab bar via `Backend::draw_tab_bar`. Returns the
/// **tab-bar content width in cells** — what the engine stores via
/// `set_tab_visible_count` (misnamed; it's the bar width used by
/// `ensure_active_tab_visible` to derive scroll offsets).
///
/// B5c.2: routes through the trait. The Backend impl computes layout
/// internally with the cell measurer.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_tab_bar(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    area: Rect,
    tabs: &[render::TabInfo],
    theme: &Theme,
    show_split_btns: bool,
    diff_toolbar: Option<&render::DiffToolbarData>,
    tab_scroll_offset: usize,
    focused_accent: Option<ratatui::style::Color>,
) -> usize {
    let accent = focused_accent.and_then(|c| match c {
        ratatui::style::Color::Rgb(r, g, b) => Some(quadraui::Color::rgb(r, g, b)),
        _ => None,
    });
    let bar = render::build_tab_bar_primitive(
        tabs,
        show_split_btns,
        diff_toolbar,
        tab_scroll_offset,
        accent,
    );
    let q_rect = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    let hits = backend.enter_frame_scope(frame, |b| {
        use quadraui::Backend;
        b.draw_tab_bar(q_rect, &bar, None)
    });
    hits.available_cols
}

/// Draw the breadcrumb bar via the D6 StatusBar pipeline.
///
/// B5c.1: routes through `Backend::draw_status_bar`. Breadcrumbs have
/// no right segments so the trait method's internal `MIN_GAP_CELLS` is
/// inert.
pub(super) fn draw_breadcrumb_bar(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    area: Rect,
    segments: &[render::BreadcrumbSegment],
    theme: &Theme,
    focus_active: bool,
    focus_selected: usize,
) {
    let bar =
        render::breadcrumbs_to_quadraui_status_bar(segments, theme, focus_active, focus_selected);
    let q_rect = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    backend.enter_frame_scope(frame, |b| {
        use quadraui::Backend;
        let _ = b.draw_status_bar(q_rect, &bar);
    });
}

// ─── Editor windows ───────────────────────────────────────────────────────────

pub(super) fn render_all_windows(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    editor_area: Rect,
    windows: &[RenderedWindow],
    theme: &Theme,
) {
    for window in windows {
        let win_rect = Rect {
            x: editor_area.x + window.rect.x as u16,
            y: editor_area.y + window.rect.y as u16,
            width: window.rect.width as u16,
            height: window.rect.height as u16,
        };
        render_window(backend, frame, win_rect, window, theme);
    }
    render_separators(frame.buffer_mut(), editor_area, windows, theme);
}

/// Render the unified picker popup. Supports single-pane (no preview) and
/// two-pane (with preview) layouts, fuzzy match highlighting, and scrollbar.
pub(super) fn render_picker_popup(
    frame: &mut ratatui::Frame,
    picker: &render::PickerPanel,
    term_area: Rect,
    theme: &Theme,
    backend: &mut super::backend::TuiBackend,
) {
    let term_cols = term_area.width;
    let term_rows = term_area.height;
    let has_preview = picker.preview.is_some();

    // Phase A.4 migration: flat-list palettes (no preview pane, no tree
    // depth) render through the shared `quadraui::Palette` primitive.
    // File and symbol pickers fall through to the legacy renderer below
    // because the primitive doesn't carry preview / tree indent yet.
    if let Some(palette) = render::picker_panel_to_palette(picker) {
        let width = (term_cols * 55 / 100).max(55);
        let height = (term_rows * 60 / 100).max(16);
        let x = (term_cols.saturating_sub(width)) / 2;
        let y = (term_rows.saturating_sub(height)) / 2;
        let area = Rect {
            x,
            y,
            width,
            height,
        };
        // Phase B.4 Stage 2: route through `Backend::draw_palette`.
        let q_rect = quadraui::Rect::new(
            area.x as f32,
            area.y as f32,
            area.width as f32,
            area.height as f32,
        );
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        backend.enter_frame_scope(frame, |b| {
            use quadraui::Backend;
            b.draw_palette(q_rect, &palette);
        });
        return;
    }

    // Size adapts based on whether we have a preview pane
    let width = if has_preview {
        (term_cols * 4 / 5).max(60)
    } else {
        (term_cols * 55 / 100).max(55)
    };
    let height = if has_preview {
        (term_rows * 65 / 100).max(18)
    } else {
        (term_rows * 60 / 100).max(16)
    };

    let x = (term_cols.saturating_sub(width)) / 2;
    let y = (term_rows.saturating_sub(height)) / 2;

    let bg_color = rc(theme.fuzzy_bg);
    let sel_bg_color = rc(theme.fuzzy_selected_bg);
    let fg_color = rc(theme.fuzzy_fg);
    let query_fg = rc(theme.fuzzy_query_fg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);
    let match_fg = rc(theme.fuzzy_match_fg);

    let buf = frame.buffer_mut();

    // Clear popup background so stale characters from previous content
    // (e.g. cycling through files with different preview lengths) don't persist.
    for row in y..y + height {
        for col in x..x + width {
            if col < term_area.width && row < term_area.height {
                set_cell(buf, col, row, ' ', fg_color, bg_color);
            }
        }
    }

    // Left pane width for two-pane mode
    let left_w = if has_preview {
        (width as usize * 35 / 100) as u16
    } else {
        0
    };

    // Row 0: top border ╭─ Title ── N/M ──╮
    let title_text = format!(
        " {}  {}/{} ",
        picker.title,
        picker.items.len(),
        picker.total_count
    );
    for col in 0..width {
        let cx = x + col;
        if cx < term_area.width && y < term_area.height {
            let ch = if col == 0 {
                '╭'
            } else if col == width - 1 {
                '╮'
            } else {
                '─'
            };
            set_cell(buf, cx, y, ch, border_fg, bg_color);
        }
    }
    for (i, ch) in title_text.chars().enumerate() {
        let cx = x + 2 + i as u16;
        if cx + 1 < x + width && cx < term_area.width && y < term_area.height {
            set_cell(buf, cx, y, ch, title_fg, bg_color);
        }
    }

    // Row 1: query line │ > query▌ │
    let row1 = y + 1;
    if row1 < term_area.height {
        set_cell(buf, x, row1, '│', border_fg, bg_color);
        if x + width - 1 < term_area.width {
            set_cell(buf, x + width - 1, row1, '│', border_fg, bg_color);
        }
        for col in 1..width - 1 {
            let cx = x + col;
            if cx < term_area.width {
                set_cell(buf, cx, row1, ' ', fg_color, bg_color);
            }
        }
        let query_display = format!("> {}", picker.query);
        for (i, ch) in query_display.chars().enumerate() {
            let cx = x + 1 + i as u16;
            if cx + 1 < x + width && cx < term_area.width {
                set_cell(buf, cx, row1, ch, query_fg, bg_color);
            }
        }
        let cursor_col = x + 1 + query_display.chars().count() as u16;
        if cursor_col + 1 < x + width && cursor_col < term_area.width {
            set_cell(buf, cursor_col, row1, '▌', query_fg, bg_color);
        }
    }

    // Row 2: separator
    let row2 = y + 2;
    if row2 < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '├'
                } else if col == width - 1 {
                    '┤'
                } else if has_preview && col == left_w {
                    '┬'
                } else {
                    '─'
                };
                set_cell(buf, cx, row2, ch, border_fg, bg_color);
            }
        }
    }

    // Result rows
    let results_start = y + 3;
    let results_end = y + height - 1;
    let visible_rows = (results_end.saturating_sub(results_start)) as usize;

    // Determine effective item width for the left pane
    let item_end_col = if has_preview { left_w } else { width - 1 };

    // Scrollbar when content overflows (single-pane only)
    let total_items = picker.items.len();
    let has_scrollbar = !has_preview && total_items > visible_rows;
    let content_end = if has_scrollbar {
        item_end_col.saturating_sub(1)
    } else {
        item_end_col
    };

    // Detect tree mode: any item with depth or expand arrows means we're in tree view
    let has_tree = picker.items.iter().any(|i| i.expandable || i.depth > 0);

    for row_idx in 0..visible_rows {
        let result_idx = picker.scroll_top + row_idx;
        let ry = results_start + row_idx as u16;
        if ry >= results_end || ry >= term_area.height {
            break;
        }

        // Borders
        set_cell(buf, x, ry, '│', border_fg, bg_color);
        if x + width - 1 < term_area.width {
            set_cell(buf, x + width - 1, ry, '│', border_fg, bg_color);
        }
        if has_preview && x + left_w < term_area.width {
            set_cell(buf, x + left_w, ry, '│', border_fg, bg_color);
        }

        let is_selected = result_idx == picker.selected_idx;
        let row_bg = if is_selected { sel_bg_color } else { bg_color };

        // Fill left pane background
        let fill_end = if has_preview { left_w } else { width - 1 };
        for col in 1..fill_end {
            let cx = x + col;
            if cx < term_area.width {
                set_cell(buf, cx, ry, ' ', fg_color, row_bg);
            }
        }

        // Right pane background is cleared in the preview section below.

        // Left pane: item text with fuzzy match highlighting
        if let Some(item) = picker.items.get(result_idx) {
            let inner_cols = (content_end.saturating_sub(1)) as usize;

            // Build prefix: selection indicator + tree indentation + expand arrow
            let sel_prefix = if is_selected { "▶ " } else { "  " };
            let indent: String = "  ".repeat(item.depth);
            let arrow = if item.expandable {
                if item.expanded {
                    "▼ "
                } else {
                    "▷ "
                }
            } else if has_tree {
                "  " // Align with expandable siblings
            } else {
                ""
            };
            let full_prefix = format!("{}{}{}", sel_prefix, indent, arrow);
            let prefix_len = full_prefix.chars().count();

            // Draw prefix
            for (j, ch) in full_prefix.chars().enumerate() {
                let cx = x + 1 + j as u16;
                if cx < x + content_end && cx < term_area.width {
                    set_cell(buf, cx, ry, ch, fg_color, row_bg);
                }
            }

            // Draw display text with match highlighting
            for (j, ch) in item.display.chars().enumerate() {
                let col_pos = prefix_len + j;
                if col_pos >= inner_cols {
                    break;
                }
                let cx = x + 1 + col_pos as u16;
                if cx < x + content_end && cx < term_area.width {
                    let char_fg = if item.match_positions.contains(&j) {
                        match_fg
                    } else {
                        fg_color
                    };
                    set_cell(buf, cx, ry, ch, char_fg, row_bg);
                }
            }

            // Right-aligned detail (shortcut) in single-pane mode
            if !has_preview {
                if let Some(ref detail) = item.detail {
                    let detail_padded = format!("{}  ", detail);
                    let detail_len = detail_padded.chars().count();
                    let sc_start = inner_cols.saturating_sub(detail_len);
                    for (j, ch) in detail_padded.chars().enumerate() {
                        let cx = x + 1 + (sc_start + j) as u16;
                        let limit = x + 1 + content_end - 1;
                        if cx < limit && cx < term_area.width {
                            set_cell(buf, cx, ry, ch, border_fg, row_bg);
                        }
                    }
                }
            }
        }

        // Right pane: preview line
        if has_preview {
            let right_start = x + left_w + 1;
            let right_inner = (width - left_w - 2) as usize;
            // Always clear the full right pane row first (prevents stale chars
            // from previous preview when the new file has shorter/fewer lines).
            for j in 0..right_inner {
                let cx = right_start + j as u16;
                if cx + 1 < x + width && cx < term_area.width {
                    set_cell(buf, cx, ry, ' ', fg_color, bg_color);
                }
            }
            if let Some(ref preview) = picker.preview {
                let preview_idx = row_idx + picker.preview_scroll;
                if let Some((lineno, text, is_match)) = preview.get(preview_idx) {
                    // Replace tabs with spaces so each character occupies exactly one cell.
                    let sanitized = text.replace('\t', "    ");
                    let preview_text = format!("{:4}: {}", lineno, sanitized);
                    let preview_fg = if *is_match { title_fg } else { fg_color };
                    for (j, ch) in preview_text.chars().enumerate().take(right_inner) {
                        let cx = right_start + j as u16;
                        if cx + 1 < x + width && cx < term_area.width {
                            set_cell(buf, cx, ry, ch, preview_fg, bg_color);
                        }
                    }
                }
            }
        }
    }

    // Scrollbar (single-pane only)
    if has_scrollbar && visible_rows > 0 {
        let sb_col = x + width - 2;
        let track_len = visible_rows;
        let thumb_size = ((visible_rows * visible_rows) / total_items).max(1);
        let max_scroll = total_items.saturating_sub(visible_rows);
        let thumb_offset = (picker.scroll_top * (track_len.saturating_sub(thumb_size)))
            .checked_div(max_scroll)
            .unwrap_or(0);
        for row_off in 0..track_len {
            let ry = results_start + row_off as u16;
            if ry >= y + height - 1 || ry >= term_area.height {
                break;
            }
            let in_thumb = row_off >= thumb_offset && row_off < thumb_offset + thumb_size;
            let sb_char = if in_thumb { '█' } else { '░' };
            if sb_col < term_area.width {
                set_cell(buf, sb_col, ry, sb_char, border_fg, bg_color);
            }
        }
    }

    // Bottom border
    let bottom = y + height - 1;
    if bottom < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '╰'
                } else if col == width - 1 {
                    '╯'
                } else if has_preview && col == left_w {
                    '┴'
                } else {
                    '─'
                };
                set_cell(buf, cx, bottom, ch, border_fg, bg_color);
            }
        }
    }
}

/// Render one editor window (pane) into `frame`.
///
/// Phase C Stage 1C (#276) collapsed the body of this function to a
/// thin delegator. The actual paint code lives in
/// `quadraui::tui::draw_editor`, fed by `render::to_q_editor` (the
/// boundary adapter that converts the engine-side `RenderedWindow`
/// IR into the cross-backend `quadraui::Editor` primitive). This
/// function handles only the bits the rasteriser deliberately
/// excludes: per-window status-line row reservation + paint, and
/// applying the rasteriser's returned cursor position when the shape
/// is `Bar` / `Underline` (which sets `Frame`-level cursor state and
/// can't live inside a `Buffer`-only rasteriser).
pub(super) fn render_window(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    area: Rect,
    window: &RenderedWindow,
    theme: &Theme,
) {
    // Reserve the bottom row for the per-window status line when present.
    let status_bar_row = if window.status_line.is_some() && area.height > 1 {
        Some(area.y + area.height - 1)
    } else {
        None
    };
    let editor_area = if status_bar_row.is_some() {
        Rect {
            height: area.height - 1,
            ..area
        }
    } else {
        area
    };

    let editor = render::to_q_editor(window);
    let q_theme = super::quadraui_tui::q_theme(theme);
    let result = quadraui::tui::draw_editor(frame.buffer_mut(), editor_area, &editor, &q_theme);

    if let Some(pos) = result.cursor_position {
        frame.set_cursor_position(pos);
    }

    if let (Some(status), Some(sy)) = (&window.status_line, status_bar_row) {
        render_window_status_line(backend, frame, editor_area.x, sy, editor_area.width, status, theme);
    }
}

/// Draw a per-window status line into the given row.
///
/// B5c.1: routes through `Backend::draw_status_bar`. The trait impl
/// computes layout internally with `MIN_GAP_CELLS = 2.0` so right
/// segments priority-drop on narrow bars (#159).
///
/// `StatusBar` adapter encodes engine-side `StatusAction` values as
/// opaque `WidgetId` strings; `status_segment_hit_test` (in mouse.rs)
/// decodes them back to `StatusAction` via `status_action_from_id`
/// after the layout's hit_test() resolves a click — TUI doesn't
/// consume the hit regions returned by `draw_status_bar` because the
/// click handler runs the layout on demand against current bar width.
fn render_window_status_line(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    x: u16,
    y: u16,
    width: u16,
    status: &crate::render::WindowStatusLine,
    theme: &crate::render::Theme,
) {
    let bar = crate::render::window_status_line_to_status_bar(
        status,
        quadraui::WidgetId::new("status:window"),
    );
    let q_rect = quadraui::Rect::new(x as f32, y as f32, width as f32, 1.0);
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    backend.enter_frame_scope(frame, |b| {
        use quadraui::Backend;
        let _ = b.draw_status_bar(q_rect, &bar);
    });
}

/// Convert a character-index column to a visual column, expanding tabs.
/// Used by mouse hit-tests outside the editor paint path; the
/// in-rasteriser callers were lifted to `quadraui::tui::editor` in
/// Stage 1C of #276.
pub(super) fn char_col_to_visual(raw_text: &str, char_col: usize, tabstop: usize) -> usize {
    let tabstop = tabstop.max(1);
    let mut vis = 0usize;
    for (i, ch) in raw_text.chars().enumerate() {
        if ch == '\n' || ch == '\r' {
            break;
        }
        if i >= char_col {
            break;
        }
        if ch == '\t' {
            vis = ((vis / tabstop) + 1) * tabstop;
        } else {
            vis += 1;
        }
    }
    vis
}

pub(super) fn render_separators(
    buf: &mut ratatui::buffer::Buffer,
    editor_area: Rect,
    windows: &[RenderedWindow],
    theme: &Theme,
) {
    if windows.len() <= 1 {
        return;
    }
    let sep_fg = rc(theme.separator);
    let thumb_fg = rc(theme.scrollbar_thumb);
    let track_fg = sep_fg;
    let sep_bg = rc(theme.background);

    for i in 0..windows.len() {
        for j in (i + 1)..windows.len() {
            let a = &windows[i];
            let b = &windows[j];

            // Vertical separator: window a is the left pane, b is the right pane.
            // The separator is drawn in the last column of a. We draw scrollbar
            // chars there so the user can see and interact with a's scroll position.
            // Also require vertical overlap — windows from different groups may
            // share an x edge but not overlap in y (e.g. 2×2 grid).
            let v_overlap =
                a.rect.y.max(b.rect.y) < (a.rect.y + a.rect.height).min(b.rect.y + b.rect.height);
            if (a.rect.x + a.rect.width - b.rect.x).abs() < 1.0 && v_overlap {
                let sep_x = editor_area.x + (a.rect.x + a.rect.width) as u16;
                let y_start = editor_area.y + a.rect.y.max(b.rect.y) as u16;
                let y_end =
                    editor_area.y + (a.rect.y + a.rect.height).min(b.rect.y + b.rect.height) as u16;
                let track_h = y_end.saturating_sub(y_start) as usize;
                let viewport_lines = a.rect.height as usize;
                let has_scroll = a.total_lines > viewport_lines && track_h > 0;

                let (thumb_top, thumb_size) = if has_scroll {
                    let h = track_h as f64;
                    let size = ((viewport_lines as f64 / a.total_lines as f64) * h)
                        .ceil()
                        .max(1.0) as usize;
                    let top = ((a.scroll_top as f64 / a.total_lines as f64) * h).floor() as usize;
                    (top, size)
                } else {
                    (0, track_h)
                };

                for dy in 0..y_end.saturating_sub(y_start) {
                    let y = y_start + dy;
                    let (ch, fg) = if has_scroll {
                        let in_thumb =
                            (dy as usize) >= thumb_top && (dy as usize) < thumb_top + thumb_size;
                        if in_thumb {
                            ('█', thumb_fg)
                        } else {
                            ('░', track_fg)
                        }
                    } else {
                        ('│', sep_fg)
                    };
                    set_cell(buf, sep_x.saturating_sub(1), y, ch, fg, sep_bg);
                }
            }

            // Horizontal separator — also require horizontal overlap.
            // Skip when the upper window has a per-window status bar (it replaces the separator).
            let h_overlap =
                a.rect.x.max(b.rect.x) < (a.rect.x + a.rect.width).min(b.rect.x + b.rect.width);
            let upper_has_status = if (a.rect.y + a.rect.height - b.rect.y).abs() < 1.0 {
                a.status_line.is_some()
            } else if (b.rect.y + b.rect.height - a.rect.y).abs() < 1.0 {
                b.status_line.is_some()
            } else {
                false
            };
            if (a.rect.y + a.rect.height - b.rect.y).abs() < 1.0 && h_overlap && !upper_has_status {
                let sep_y = editor_area.y + (a.rect.y + a.rect.height) as u16;
                let x_start = editor_area.x + a.rect.x.max(b.rect.x) as u16;
                let x_end =
                    editor_area.x + (a.rect.x + a.rect.width).min(b.rect.x + b.rect.width) as u16;
                for x in x_start..x_end.max(x_start) {
                    set_cell(buf, x, sep_y.saturating_sub(1), '─', sep_fg, sep_bg);
                }
            }
        }
    }
}

// ─── Activity bar ─────────────────────────────────────────────────────────────

// ─── Menu bar rendering ───────────────────────────────────────────────────────────────────

pub(super) fn render_menu_bar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    data: &render::MenuBarData,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let bar_bg = rc(theme.status_bg);
    let bar_fg = rc(theme.status_fg);
    let y = area.y;

    // Fill background
    for x in area.x..area.x + area.width {
        set_cell(buf, x, y, ' ', bar_fg, bar_bg);
    }

    // Menu labels (no hamburger here — it lives in the activity bar below)
    let mut col = area.x + 1; // one-cell left pad

    for (idx, (name, _, _)) in render::MENU_STRUCTURE.iter().enumerate() {
        let is_open = data.open_menu_idx == Some(idx);
        let (fg, bg) = if is_open {
            (bar_bg, bar_fg) // reversed for open
        } else {
            (bar_fg, bar_bg)
        };
        // Space before name
        if col < area.x + area.width {
            set_cell(buf, col, y, ' ', bar_fg, bar_bg);
            col += 1;
        }
        // Name chars
        for ch in name.chars() {
            if col >= area.x + area.width {
                break;
            }
            set_cell(buf, col, y, ch, fg, bg);
            col += 1;
        }
        // Space after name
        if col < area.x + area.width {
            set_cell(buf, col, y, ' ', fg, bar_bg);
            col += 1;
        }
    }

    // Center nav arrows + search box as one unit between menu labels and right edge.
    let dim_fg = rc(theme.line_number_fg);
    let active_fg = bar_fg;
    let menu_end = col;

    // Compute total unit width: "◀ ▶ [ 🔍 title ]"
    let arrows_w: u16 = 4; // "◀ ▶" = 4 cols (arrow + space + arrow + space)
    let display = if data.title.is_empty() {
        String::new()
    } else {
        format!("\u{1f50d} {}", data.title)
    };
    let display_chars: Vec<char> = display.chars().collect();
    let text_len = display_chars.len() as u16;
    // Box = [ space text space ] = text + 4
    let box_width = if !display.is_empty() { text_len + 4 } else { 0 };
    let gap = if box_width > 0 { 1u16 } else { 0 };
    let total_unit = arrows_w + gap + box_width;
    let right_edge = area.x + area.width;
    let available = right_edge.saturating_sub(menu_end);

    if available >= total_unit + 2 {
        let unit_start = menu_end + (available - total_unit) / 2;

        // Draw arrows.
        let mut ax = unit_start;
        let back_fg = if data.nav_back_enabled {
            active_fg
        } else {
            dim_fg
        };
        set_cell(buf, ax, y, '◀', back_fg, bar_bg);
        ax += 1;
        set_cell(buf, ax, y, ' ', bar_bg, bar_bg);
        ax += 1;
        let fwd_fg = if data.nav_forward_enabled {
            active_fg
        } else {
            dim_fg
        };
        set_cell(buf, ax, y, '▶', fwd_fg, bar_bg);
        ax += 1;
        set_cell(buf, ax, y, ' ', bar_bg, bar_bg);
        ax += 1;

        // Draw search box.
        if !display.is_empty() {
            ax += gap;
            let box_start = ax;
            let box_end = box_start + box_width;
            // Use bar_fg (same as menu text) for box border and text
            if box_start < right_edge {
                set_cell(buf, box_start, y, '[', dim_fg, bar_bg);
            }
            if box_start + 1 < right_edge {
                set_cell(buf, box_start + 1, y, ' ', bar_fg, bar_bg);
            }
            for (i, ch) in display_chars.iter().enumerate() {
                let cx = box_start + 2 + i as u16;
                if cx < right_edge {
                    set_cell(buf, cx, y, *ch, bar_fg, bar_bg);
                }
            }
            if box_end >= 2 && box_end - 2 < right_edge {
                set_cell(buf, box_end - 2, y, ' ', bar_fg, bar_bg);
            }
            if box_end >= 1 && box_end - 1 < right_edge {
                set_cell(buf, box_end - 1, y, ']', dim_fg, bar_bg);
            }
        }
    }
}

// ─── Context menu popup rendering ───────────────────────────────────────────────────────

// ─── Debug toolbar rendering ────────────────────────────────────────────────────────────

// ─── Find/replace overlay ────────────────────────────────────────────────────

// ─── TUI rendering regression tests ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::window::GroupId;
    use ratatui::backend::TestBackend;

    /// Create a hermetic engine for rendering tests.
    fn test_engine(text: &str) -> Engine {
        crate::core::session::suppress_disk_saves();
        let mut e = Engine::new();
        e.settings = crate::core::settings::Settings::default();
        e.extension_state = crate::core::session::ExtensionState::default();
        e.ext_registry = None;
        e.mode = crate::core::Mode::Normal;
        e.rebuild_user_keymaps();
        if !text.is_empty() {
            e.buffer_mut().insert(0, text);
        }
        e
    }

    /// Render the TUI and return the character buffer as a Vec of lines.
    fn render_tui(engine: &Engine, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = crate::render::Theme::onedark();
        let mut sidebar = TuiSidebar {
            visible: false,
            has_focus: false,
            active_panel: TuiPanel::Explorer,
            selected: 0,
            scroll_top: 0,
            rows: Vec::new(),
            root: std::path::PathBuf::from("/tmp"),
            expanded: std::collections::HashSet::new(),
            search_input_mode: false,
            replace_input_focused: false,
            search_scroll_top: 0,
            show_hidden_files: false,
            sort_case_insensitive: false,
            toolbar_focused: false,
            toolbar_selected: 0,
            pending_ctrl_w: false,
            ext_panel_name: None,
        };
        let sidebar_width = 0u16;
        let area = Rect {
            x: 0,
            y: 0,
            width,
            height,
        };
        let screen = build_screen_for_tui(engine, &theme, area, &sidebar, sidebar_width);

        let mut hover_link_rects = Vec::new();
        let mut hover_popup_rect = None;
        let mut editor_hover_popup_rect = None;
        let mut editor_hover_link_rects = Vec::new();
        let mut editor_hover_scrollbar = None;
        let mut tab_visible_counts: Vec<(GroupId, usize)> = Vec::new();
        let mut backend = super::backend::TuiBackend::new();

        terminal
            .draw(|frame| {
                draw_frame(
                    frame,
                    &screen,
                    &theme,
                    &mut sidebar,
                    engine,
                    sidebar_width,
                    0,     // quickfix_scroll_top
                    0,     // debug_output_scroll
                    None,  // folder_picker
                    false, // quit_confirm
                    0,     // quit_confirm_focus
                    false, // close_tab_confirm
                    0,     // close_tab_confirm_focus
                    None,  // cmd_sel
                    None,  // explorer_drop_target
                    &mut hover_link_rects,
                    &mut hover_popup_rect,
                    &mut editor_hover_popup_rect,
                    &mut editor_hover_link_rects,
                    &mut editor_hover_scrollbar,
                    &mut tab_visible_counts,
                    &mut backend,
                );
            })
            .unwrap();

        // Extract the rendered buffer as lines of text
        let buf = terminal.backend().buffer();
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                let cell = &buf[(x, y)];
                line.push_str(cell.symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines
    }

    /// Assert that a specific row in the rendered output contains a substring.
    fn assert_row_contains(lines: &[String], row: usize, substr: &str) {
        assert!(
            row < lines.len(),
            "row {row} out of bounds (have {} lines)",
            lines.len()
        );
        assert!(
            lines[row].contains(substr),
            "row {row}: expected {substr:?} in {:?}",
            lines[row]
        );
    }

    // ── Tests ─────────────────────────────────────────────────────────────

    #[test]
    fn test_tui_renders_file_content() {
        let e = test_engine("Hello, world!\nSecond line\n");
        let lines = render_tui(&e, 80, 24);

        // Content should appear somewhere in the rendered output
        let has_hello = lines.iter().any(|l| l.contains("Hello, world!"));
        assert!(has_hello, "rendered output should contain file content");

        let has_second = lines.iter().any(|l| l.contains("Second line"));
        assert!(has_second, "rendered output should contain second line");
    }

    #[test]
    fn test_tui_renders_tab_bar() {
        let e = test_engine("content\n");
        let lines = render_tui(&e, 80, 24);

        // Tab bar is the first line; should show "[No Name]" for unsaved buffer
        assert_row_contains(&lines, 0, "No Name");
    }

    #[test]
    fn test_tui_renders_command_line() {
        let e = test_engine("content\n");
        let lines = render_tui(&e, 80, 24);

        // Last line is the command line — should not contain normal text content.
        // Activity bar icons (nerd font glyphs) may appear in the leftmost columns.
        let last = &lines[23];
        assert!(
            !last.contains("content") && !last.contains("NORMAL"),
            "command line should not contain editor content or status, got: {last:?}"
        );
    }

    #[test]
    fn test_tui_renders_status_bar() {
        let e = test_engine("content\n");
        let lines = render_tui(&e, 80, 24);

        // Per-window status bar should show NORMAL mode
        let has_normal = lines
            .iter()
            .any(|l| l.contains("NORMAL") || l.contains("NOR"));
        assert!(has_normal, "status bar should show normal mode");
    }

    #[test]
    fn test_tui_split_renders_two_panes() {
        let mut e = test_engine("left pane\n");
        e.open_editor_group(crate::core::window::SplitDirection::Vertical);
        let lines = render_tui(&e, 80, 24);

        // Both panes should have a tab bar with "[No Name]"
        // Count occurrences of "No Name" across all lines
        let tab_count: usize = lines.iter().filter(|l| l.contains("No Name")).count();
        assert!(
            tab_count >= 2,
            "split should produce two tab bars, found {tab_count} 'No Name' occurrences"
        );
    }

    #[test]
    fn test_tui_dirty_indicator() {
        let mut e = test_engine("clean\n");
        e.handle_key("i", Some('i'), false);
        e.handle_key("x", Some('x'), false);
        e.handle_key("Escape", None, false);
        let lines = render_tui(&e, 80, 24);

        // Dirty buffer shows a dot indicator in the tab bar
        let has_dot = lines[0].contains('●') || lines[0].contains('•') || lines[0].contains('+');
        assert!(
            has_dot,
            "dirty buffer should show indicator in tab bar: {:?}",
            lines[0]
        );
    }

    #[test]
    fn test_tui_insert_mode_status() {
        let mut e = test_engine("hello\n");
        e.handle_key("i", Some('i'), false);
        let lines = render_tui(&e, 80, 24);

        let has_insert = lines
            .iter()
            .any(|l| l.contains("INSERT") || l.contains("INS"));
        assert!(has_insert, "insert mode should show in status bar");
    }

    #[test]
    fn test_tui_visual_mode_status() {
        let mut e = test_engine("hello\n");
        e.handle_key("v", Some('v'), false);
        let lines = render_tui(&e, 80, 24);

        let has_visual = lines
            .iter()
            .any(|l| l.contains("VISUAL") || l.contains("VIS"));
        assert!(has_visual, "visual mode should show in status bar");
    }

    #[test]
    fn test_tui_dimensions_respected() {
        let e = test_engine("content\n");
        // Small terminal
        let lines = render_tui(&e, 40, 10);
        assert_eq!(lines.len(), 10, "should render exactly 10 rows");

        // All lines should fit in 40 display columns.
        // Note: multi-byte nerd font glyphs may make .len() > 40 but the
        // ratatui buffer guarantees 40 cell columns. Check cell count instead.
        // (The render_tui helper already indexes by cell coordinates.)
    }

    #[test]
    fn test_tui_long_file_scroll() {
        // Create a file longer than the viewport
        let content: String = (1..=50).map(|i| format!("line {i}\n")).collect();
        let e = test_engine(&content);
        let lines = render_tui(&e, 80, 15);

        // Should show "line 1" at the top (we're at scroll position 0)
        let has_line1 = lines.iter().any(|l| l.contains("line 1"));
        assert!(has_line1, "scrolled-to-top should show line 1");

        // Should NOT show "line 50" (too far down)
        let has_line50 = lines.iter().any(|l| l.contains("line 50"));
        assert!(!has_line50, "should not show line 50 in 15-row viewport");
    }

    // ── Snapshot tests (golden reference) ────────────────────────────────
    //
    // These capture the full rendered grid. Any visual change causes a
    // test failure until the snapshot is reviewed and accepted with:
    //   cargo insta review
    //
    // First run creates the snapshot files automatically.
    //
    // The `prepend_module_path(false)` setting ensures both the `vimcode`
    // and `vcd` binaries share the same snapshot files.

    fn snap_settings() -> insta::Settings {
        let mut s = insta::Settings::clone_current();
        s.set_prepend_module_to_snapshot(false);
        s.set_snapshot_path("snapshots");
        s
    }

    #[test]
    fn snapshot_normal_mode() {
        let e = test_engine("fn main() {\n    println!(\"hello\");\n}\n");
        let lines = render_tui(&e, 60, 12);
        snap_settings().bind(|| insta::assert_snapshot!("normal_mode", lines.join("\n")));
    }

    #[test]
    fn snapshot_insert_mode() {
        let mut e = test_engine("hello world\n");
        e.handle_key("i", Some('i'), false);
        let lines = render_tui(&e, 60, 12);
        snap_settings().bind(|| insta::assert_snapshot!("insert_mode", lines.join("\n")));
    }

    #[test]
    fn snapshot_visual_selection() {
        let mut e = test_engine("select this text\nand this too\n");
        e.handle_key("v", Some('v'), false);
        for _ in 0..10 {
            e.handle_key("l", Some('l'), false);
        }
        let lines = render_tui(&e, 60, 12);
        snap_settings().bind(|| insta::assert_snapshot!("visual_selection", lines.join("\n")));
    }

    #[test]
    fn snapshot_command_line() {
        let mut e = test_engine("buffer content\n");
        e.handle_key(":", Some(':'), false);
        e.handle_key("s", Some('s'), false);
        e.handle_key("e", Some('e'), false);
        e.handle_key("t", Some('t'), false);
        let lines = render_tui(&e, 60, 12);
        snap_settings().bind(|| insta::assert_snapshot!("command_line", lines.join("\n")));
    }

    #[test]
    fn snapshot_split_panes() {
        let mut e = test_engine("left pane content\n");
        e.open_editor_group(crate::core::window::SplitDirection::Vertical);
        let lines = render_tui(&e, 80, 16);
        snap_settings().bind(|| insta::assert_snapshot!("split_panes", lines.join("\n")));
    }

    #[test]
    fn snapshot_line_numbers() {
        let mut e = test_engine("alpha\nbeta\ngamma\ndelta\nepsilon\n");
        e.settings.line_numbers = crate::core::settings::LineNumberMode::Absolute;
        let lines = render_tui(&e, 60, 12);
        snap_settings().bind(|| insta::assert_snapshot!("line_numbers", lines.join("\n")));
    }
}
