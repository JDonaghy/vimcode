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
        engine.session.terminal_panel_rows + 2 // 1 tab bar row + 1 header row + content
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
    let tui_tab_bar_height = if engine.settings.breadcrumbs {
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
    close_tab_confirm: bool,
    cmd_sel: Option<(usize, usize)>,
    explorer_drop_target: Option<usize>,
    hover_link_rects_out: &mut Vec<(u16, u16, u16, u16, String)>,
    hover_popup_rect_out: &mut Option<(u16, u16, u16, u16)>,
    editor_hover_popup_rect_out: &mut Option<(u16, u16, u16, u16)>,
    editor_hover_link_rects_out: &mut Vec<(u16, u16, u16, u16, String)>,
    tab_visible_counts_out: &mut Vec<(GroupId, usize)>,
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
        engine.session.terminal_panel_rows + 2
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
            frame.buffer_mut(),
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
        render_all_windows(frame, editor_area, &screen.windows, theme);
        // Draw each group's tab bar.  Tab bar sits tab_bar_height rows above
        // the group's window content (bounds.y - tab_bar_height).
        let tui_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
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
                    frame.buffer_mut(),
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
        // Draw breadcrumb bars (below each group's tab bar).
        for bc in &screen.breadcrumbs {
            if bc.segments.is_empty() {
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
                render_breadcrumb_bar(
                    frame.buffer_mut(),
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
                frame.buffer_mut(),
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
        // Draw breadcrumb bar for the single group.
        if let Some(bc) = screen.breadcrumbs.first() {
            if !bc.segments.is_empty() {
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
                render_breadcrumb_bar(
                    frame.buffer_mut(),
                    bc_rect,
                    &bc.segments,
                    theme,
                    engine.breadcrumb_focus,
                    engine.breadcrumb_selected,
                );
            }
        }
        render_all_windows(frame, editor_area, &screen.windows, theme);
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
                render_completion_popup(frame, menu, popup_x, popup_y, frame.area(), theme);
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
            render_hover_popup(frame, hover, popup_x, popup_y, frame.area(), theme);
        }
    }

    // ── Editor hover popup (rich markdown, triggered by gh or mouse dwell) ─
    *editor_hover_popup_rect_out = None; // Clear stale rect before rendering
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
            let (eh_links, eh_rect) =
                render_editor_hover_popup(frame, eh, popup_x, popup_y, frame.area(), theme);
            *editor_hover_link_rects_out = eh_links;
            *editor_hover_popup_rect_out = eh_rect;
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
            let popup_y = win_y + anchor_view + 1; // below anchor line
            render_diff_peek_popup(frame, peek, popup_x, popup_y, frame.area(), theme);
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
            render_signature_popup(frame, sig, popup_x, popup_y, frame.area(), theme);
        }
    }

    // ── Quickfix panel (persistent bottom strip) ──────────────────────────────
    if let Some(ref qf) = screen.quickfix {
        render_quickfix_panel(
            frame.buffer_mut(),
            quickfix_area,
            qf,
            quickfix_scroll_top,
            theme,
        );
    }

    // ── Separated status line (above terminal, when status_line_above_terminal is active) ──
    if let Some(ref status) = screen.separated_status_line {
        render_window_status_line(
            frame.buffer_mut(),
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
        render_debug_toolbar(frame.buffer_mut(), debug_toolbar_area, toolbar, theme);
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
        render_folder_picker(frame, picker, area, theme);
    }

    // ── Find/replace overlay (top-right of active group) ───────────────────
    if let Some(ref find_replace) = screen.find_replace {
        let editor_left = h_chunks[0].width + h_chunks[1].width;
        render_find_replace_popup(frame.buffer_mut(), area, find_replace, theme, editor_left);
    }

    // ── Unified picker modal (above terminal/status so it's fully visible) ──
    if let Some(ref picker) = screen.picker {
        render_picker_popup(frame, picker, area, theme);
    }

    // ── Tab switcher popup ───────────────────────────────────────────────────
    if let Some(ref ts) = screen.tab_switcher {
        render_tab_switcher_popup(frame.buffer_mut(), area, ts, theme);
    }

    // ── Context menu popup (above status/command line) ─────────────────────
    if let Some(ref ctx_menu) = screen.context_menu {
        render_context_menu(frame.buffer_mut(), area, ctx_menu, theme);
    }

    // ── Modal dialog (highest z-order after quit confirm) ────────────────────
    if let Some(ref dialog) = screen.dialog {
        render_dialog_popup(frame.buffer_mut(), area, dialog, theme);
    }

    // ── Menu dropdown — rendered last so it draws on top of everything ────────
    if let Some(ref menu_data) = screen.menu_bar {
        if menu_data.open_menu_idx.is_some() {
            render_menu_dropdown(frame.buffer_mut(), area, menu_data, theme);
        }
    }

    // ── Quit confirm overlay — rendered on top of absolutely everything ───────
    if quit_confirm {
        render_quit_confirm_overlay(frame.buffer_mut(), area, theme);
    }

    // ── Close-tab confirm overlay ──────────────────────────────────────────────
    if close_tab_confirm {
        render_close_tab_confirm_overlay(frame.buffer_mut(), area, theme);
    }
}

// ─── Tab bar constants ───────────────────────────────────────────────────────

/// Close-tab × button character (shown on every tab).
pub(super) const TAB_CLOSE_CHAR: char = '×'; // U+00D7 MULTIPLICATION SIGN
/// Terminal columns used by each tab's close button (the × itself + trailing space).
pub(super) const TAB_CLOSE_COLS: u16 = 2;

// Split button glyphs: \u{F0932} (split-right), \u{f0d7} (caret-down / split-down).
/// Terminal columns occupied by each split button (1 space + 2-wide NF glyph).
pub(super) const TAB_SPLIT_BTN_COLS: u16 = 3;
/// Total columns reserved for both split buttons.
pub(super) const TAB_SPLIT_BOTH_COLS: u16 = TAB_SPLIT_BTN_COLS * 2;

/// Terminal columns for the editor action menu button ("…").
pub(super) const TAB_ACTION_BTN_COLS: u16 = 3;

/// Terminal columns per diff toolbar button (1 space + 1 char + 1 space).
pub(super) const DIFF_BTN_COLS: u16 = 3;
/// Total columns for all three diff toolbar buttons.
pub(super) const DIFF_TOOLBAR_BTN_COLS: u16 = DIFF_BTN_COLS * 3;

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

/// Render the tab bar.  Returns the number of tabs that were actually drawn
/// (used to update `EditorGroup::tab_visible_count`).
#[allow(clippy::too_many_arguments)]
pub(super) fn render_tab_bar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    tabs: &[render::TabInfo],
    theme: &Theme,
    show_split_btns: bool,
    diff_toolbar: Option<&render::DiffToolbarData>,
    tab_scroll_offset: usize,
    focused_accent: Option<ratatui::style::Color>,
) -> usize {
    let bar_bg = rc(theme.tab_bar_bg);

    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', bar_bg, bar_bg);
    }

    // Calculate total reserved columns at the right edge.
    let diff_cols = if diff_toolbar.is_some() {
        // 3 buttons + up to 6 chars for label like "2 of 5" + 1 space
        let label_cols = diff_toolbar
            .and_then(|d| d.change_label.as_ref())
            .map(|l| l.len() as u16 + 1)
            .unwrap_or(0);
        DIFF_TOOLBAR_BTN_COLS + label_cols
    } else {
        0
    };
    let split_cols = if show_split_btns {
        TAB_SPLIT_BOTH_COLS
    } else {
        0
    };
    let action_cols = TAB_ACTION_BTN_COLS;
    let reserved = diff_cols + split_cols + action_cols;

    // Reserve columns at the right edge for buttons.
    let tab_end = if area.width >= reserved {
        area.x + area.width - reserved
    } else {
        area.x + area.width
    };

    let mut x = area.x;
    let tab_end_for_content = tab_end;

    for tab in tabs.iter().skip(tab_scroll_offset) {
        let (fg, bg) = match (tab.active, tab.preview) {
            (true, true) => (rc(theme.tab_preview_active_fg), rc(theme.tab_active_bg)),
            (true, false) => (rc(theme.tab_active_fg), rc(theme.tab_active_bg)),
            (false, true) => (rc(theme.tab_preview_inactive_fg), rc(theme.tab_bar_bg)),
            (false, false) => (rc(theme.tab_inactive_fg), rc(theme.tab_bar_bg)),
        };
        let modifier = if tab.active && focused_accent.is_some() {
            if tab.preview {
                Modifier::ITALIC | Modifier::UNDERLINED
            } else {
                Modifier::UNDERLINED
            }
        } else if tab.preview {
            Modifier::ITALIC
        } else {
            Modifier::empty()
        };

        // Check if this tab would overflow the available space.
        let name_w = tab.name.chars().count() as u16;
        let tab_w = name_w + TAB_CLOSE_COLS;
        if x + tab_w > tab_end_for_content {
            break;
        }

        // Find where the filename starts (after the " N: " prefix) so the
        // underline accent only covers the filename, not the number prefix.
        let prefix_len = tab.name.find(": ").map(|p| p + 2).unwrap_or(0);
        let prefix_mod = if tab.preview {
            Modifier::ITALIC
        } else {
            Modifier::empty()
        };
        for (ci, ch) in tab.name.chars().enumerate() {
            if x >= tab_end_for_content {
                break;
            }
            let in_filename = ci >= prefix_len;
            let cell_mod = if in_filename { modifier } else { prefix_mod };
            let ul_color = if in_filename && tab.active {
                focused_accent
            } else {
                None
            };
            set_cell_styled(buf, x, area.y, ch, fg, bg, cell_mod, ul_color);
            x += 1;
        }
        // Show ● (modified dot) when dirty, × otherwise (VSCode style).
        if x < tab_end_for_content {
            let (close_ch, close_fg) = if tab.dirty {
                ('●', rc(theme.foreground))
            } else if tab.active {
                (TAB_CLOSE_CHAR, rc(theme.tab_active_fg))
            } else {
                (TAB_CLOSE_CHAR, rc(theme.separator))
            };
            set_cell(buf, x, area.y, close_ch, close_fg, bg);
            x += 1;
        }
        // Trailing separator space.
        if x < tab_end_for_content {
            set_cell(buf, x, area.y, ' ', bar_bg, bar_bg);
            x += 1;
        }
    }

    // Draw diff toolbar buttons (to the left of split buttons).
    if let Some(dt) = diff_toolbar {
        if area.width >= reserved {
            let mut bx = area.x + area.width - reserved;
            let btn_fg = rc(theme.tab_inactive_fg);
            let active_fg = rc(theme.tab_active_fg);
            // Change label (e.g. "2/5")
            if let Some(label) = &dt.change_label {
                let label_fg = rc(theme.foreground);
                set_cell(buf, bx, area.y, ' ', label_fg, bar_bg);
                bx += 1;
                for ch in label.chars() {
                    set_cell(buf, bx, area.y, ch, label_fg, bar_bg);
                    bx += 1;
                }
            }
            // Prev button (space + 2-wide NF glyph = 3 cols)
            set_cell(buf, bx, area.y, ' ', btn_fg, bar_bg);
            set_cell_wide(buf, bx + 1, area.y, '\u{F0143}', btn_fg, bar_bg);
            bx += DIFF_BTN_COLS;
            // Next button
            set_cell(buf, bx, area.y, ' ', btn_fg, bar_bg);
            set_cell_wide(buf, bx + 1, area.y, '\u{F0140}', btn_fg, bar_bg);
            bx += DIFF_BTN_COLS;
            // Fold toggle button (highlighted when active)
            let fold_fg = if dt.unchanged_hidden {
                active_fg
            } else {
                btn_fg
            };
            set_cell(buf, bx, area.y, ' ', fold_fg, bar_bg);
            set_cell_wide(buf, bx + 1, area.y, '\u{F0233}', fold_fg, bar_bg);
            bx += DIFF_BTN_COLS;
        }
    }

    // Draw split-right then split-down buttons, then the action menu button.
    if show_split_btns && area.width >= split_cols + action_cols {
        let btn_fg = rc(theme.tab_inactive_fg);
        let mut bx = area.x + area.width - split_cols - action_cols;
        // Split-right button (space + 2-wide NF glyph = 3 cols)
        set_cell(buf, bx, area.y, ' ', btn_fg, bar_bg);
        set_cell_wide(buf, bx + 1, area.y, '\u{F0932}', btn_fg, bar_bg);
        bx += TAB_SPLIT_BTN_COLS;
        // Split-down button (caret-down ▾)
        set_cell(buf, bx, area.y, ' ', btn_fg, bar_bg);
        set_cell(
            buf,
            bx + 1,
            area.y,
            crate::icons::SPLIT_DOWN.c(),
            btn_fg,
            bar_bg,
        );
        set_cell(buf, bx + 2, area.y, ' ', btn_fg, bar_bg);
    }

    // Draw the editor action menu button ("…") at the far right.
    if area.width >= action_cols {
        let btn_fg = rc(theme.tab_inactive_fg);
        let bx = area.x + area.width - action_cols;
        set_cell(buf, bx, area.y, ' ', btn_fg, bar_bg);
        set_cell(buf, bx + 1, area.y, '\u{22EF}', btn_fg, bar_bg); // ⋯
        set_cell(buf, bx + 2, area.y, ' ', btn_fg, bar_bg);
    }

    // Return the available tab bar width in columns so the engine can compute
    // how many tabs fit.  (Returning a count caused a feedback-loop bug where
    // the engine treated the count as column width, shrinking tabs each frame.)
    (tab_end_for_content - area.x) as usize
}

pub(super) fn render_breadcrumb_bar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    segments: &[render::BreadcrumbSegment],
    theme: &Theme,
    focus_active: bool,
    focus_selected: usize,
) {
    let bg = rc(theme.breadcrumb_bg);
    let sel_bg = rc(theme.breadcrumb_active_fg);
    // Fill the row with breadcrumb bg
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', bg, bg);
    }

    let separator = " \u{203A} "; // " › "
    let mut x = area.x + 1; // small left padding

    for (i, seg) in segments.iter().enumerate() {
        // Separator before all but the first
        if x > area.x + 2 {
            let sep_fg = rc(theme.breadcrumb_fg);
            for ch in separator.chars() {
                if x >= area.x + area.width {
                    return;
                }
                set_cell(buf, x, area.y, ch, sep_fg, bg);
                x += 1;
            }
        }

        // Segment label — highlight selected segment in focus mode
        let is_focused = focus_active && i == focus_selected;
        let (fg, segment_bg) = if is_focused {
            (rc(theme.breadcrumb_bg), sel_bg)
        } else if seg.is_last {
            (rc(theme.breadcrumb_active_fg), bg)
        } else {
            (rc(theme.breadcrumb_fg), bg)
        };
        for ch in seg.label.chars() {
            if x >= area.x + area.width {
                return;
            }
            set_cell(buf, x, area.y, ch, fg, segment_bg);
            x += 1;
        }
    }
}

// ─── Editor windows ───────────────────────────────────────────────────────────

pub(super) fn render_all_windows(
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
        render_window(frame, win_rect, window, theme);
    }
    render_separators(frame.buffer_mut(), editor_area, windows, theme);
}

pub(super) fn render_completion_popup(
    frame: &mut ratatui::Frame,
    menu: &CompletionMenu,
    popup_x: u16,
    popup_y: u16,
    term_area: Rect,
    theme: &Theme,
) {
    let visible = menu.candidates.len().min(10) as u16;
    if visible == 0 {
        return;
    }
    let width = (menu.max_width as u16 + 4).max(12);

    // Clamp so popup doesn't go off the right/bottom edge
    let x = popup_x.min(term_area.width.saturating_sub(width));
    let y = popup_y.min(term_area.height.saturating_sub(visible));

    let bg_color = rc(theme.completion_bg);
    let sel_bg_color = rc(theme.completion_selected_bg);
    let fg_color = rc(theme.completion_fg);
    let border_color = rc(theme.completion_border);

    let buf = frame.buffer_mut();
    for (i, candidate) in menu.candidates.iter().enumerate().take(visible as usize) {
        let row_y = y + i as u16;
        let row_bg = if i == menu.selected_idx {
            sel_bg_color
        } else {
            bg_color
        };
        // Fill the row background
        for col in 0..width {
            let cell_x = x + col;
            if cell_x < term_area.width && row_y < term_area.height {
                let cell = &mut buf[(cell_x, row_y)];
                cell.set_bg(row_bg).set_fg(fg_color);
                // Draw border chars on leftmost/rightmost or blank fill
                let ch = if col == 0 || col == width - 1 {
                    '│'
                } else {
                    ' '
                };
                cell.set_char(ch).set_fg(border_color);
            }
        }
        // Render candidate text starting at col 1
        let display = format!(" {}", candidate);
        for (j, ch) in display.chars().enumerate() {
            let cell_x = x + 1 + j as u16;
            if cell_x + 1 < x + width && cell_x < term_area.width && row_y < term_area.height {
                let cell = &mut buf[(cell_x, row_y)];
                cell.set_char(ch).set_fg(fg_color).set_bg(row_bg);
            }
        }
    }
}

pub(super) fn render_hover_popup(
    frame: &mut ratatui::Frame,
    hover: &render::HoverPopup,
    popup_x: u16,
    popup_y: u16,
    term_area: Rect,
    theme: &Theme,
) {
    let text_lines: Vec<&str> = hover.text.lines().collect();
    let num_lines = text_lines.len().min(20) as u16;
    if num_lines == 0 {
        return;
    }
    let max_len = text_lines.iter().map(|l| l.len()).max().unwrap_or(10);
    let width = (max_len as u16 + 4).max(12);

    // Place above cursor if possible, otherwise below
    let y = if popup_y > num_lines {
        popup_y - num_lines
    } else {
        popup_y + 1
    };

    // Clamp to screen bounds
    let x = popup_x.min(term_area.width.saturating_sub(width));
    let y = y.min(term_area.height.saturating_sub(num_lines));

    let bg_color = rc(theme.hover_bg);
    let fg_color = rc(theme.hover_fg);
    let border_color = rc(theme.hover_border);

    let buf = frame.buffer_mut();
    for (i, text_line) in text_lines.iter().enumerate().take(num_lines as usize) {
        let row_y = y + i as u16;
        // Fill row background
        for col in 0..width {
            let cell_x = x + col;
            if cell_x < term_area.width && row_y < term_area.height {
                let cell = &mut buf[(cell_x, row_y)];
                cell.set_bg(bg_color);
                let ch = if col == 0 || col == width - 1 {
                    '│'
                } else {
                    ' '
                };
                cell.set_char(ch).set_fg(border_color);
            }
        }
        // Render text starting at col 1
        let display = format!(" {}", text_line);
        for (j, ch) in display.chars().enumerate() {
            let cell_x = x + 1 + j as u16;
            if cell_x + 1 < x + width && cell_x < term_area.width && row_y < term_area.height {
                let cell = &mut buf[(cell_x, row_y)];
                cell.set_char(ch).set_fg(fg_color).set_bg(bg_color);
            }
        }
    }
}

pub(super) fn render_diff_peek_popup(
    frame: &mut ratatui::Frame,
    peek: &render::DiffPeekPopup,
    popup_x: u16,
    popup_y: u16,
    term_area: Rect,
    theme: &Theme,
) {
    let action_bar_lines = 1_u16;
    let num_lines = (peek.hunk_lines.len() as u16 + action_bar_lines).min(30);
    if num_lines == 0 {
        return;
    }
    let max_len = peek.hunk_lines.iter().map(|l| l.len()).max().unwrap_or(10);
    let width = (max_len as u16 + 4).max(20);

    // Clamp to screen bounds.
    let x = popup_x.min(term_area.width.saturating_sub(width));
    let y = popup_y.min(term_area.height.saturating_sub(num_lines));

    let bg_color = rc(theme.hover_bg);
    let fg_color = rc(theme.hover_fg);
    let border_color = rc(theme.hover_border);
    let added_fg = rc(theme.git_added);
    let deleted_fg = rc(theme.git_deleted);

    let buf = frame.buffer_mut();

    // Draw diff lines.
    for (i, hline) in peek.hunk_lines.iter().enumerate().take(29) {
        let row_y = y + i as u16;
        if row_y >= term_area.height {
            break;
        }
        // Fill background.
        for col in 0..width {
            let cell_x = x + col;
            if cell_x < term_area.width {
                let cell = &mut buf[(cell_x, row_y)];
                cell.set_bg(bg_color);
                let ch = if col == 0 || col == width - 1 {
                    '│'
                } else {
                    ' '
                };
                cell.set_char(ch).set_fg(border_color);
            }
        }
        // Render text.
        let line_fg = if hline.starts_with('+') {
            added_fg
        } else if hline.starts_with('-') {
            deleted_fg
        } else {
            fg_color
        };
        let display = format!(" {}", hline);
        for (j, ch) in display.chars().enumerate() {
            let cell_x = x + 1 + j as u16;
            if cell_x + 1 < x + width && cell_x < term_area.width {
                buf[(cell_x, row_y)]
                    .set_char(ch)
                    .set_fg(line_fg)
                    .set_bg(bg_color);
            }
        }
    }

    // Action bar at bottom.
    let action_row = y + peek.hunk_lines.len().min(29) as u16;
    if action_row < term_area.height {
        // Fill background.
        for col in 0..width {
            let cell_x = x + col;
            if cell_x < term_area.width {
                let cell = &mut buf[(cell_x, action_row)];
                cell.set_bg(bg_color);
                let ch = if col == 0 || col == width - 1 {
                    '│'
                } else {
                    ' '
                };
                cell.set_char(ch).set_fg(border_color);
            }
        }
        let labels = ["[s] Stage", "[r] Revert", "[q] Close"];
        let mut cx = x + 2;
        for label in &labels {
            for ch in label.chars() {
                if cx + 1 < x + width && cx < term_area.width {
                    buf[(cx, action_row)]
                        .set_char(ch)
                        .set_fg(fg_color)
                        .set_bg(bg_color);
                }
                cx += 1;
            }
            cx += 2; // spacing between labels
        }
    }
}

pub(super) fn render_signature_popup(
    frame: &mut ratatui::Frame,
    sig: &render::SignatureHelp,
    popup_x: u16,
    popup_y: u16,
    term_area: Rect,
    theme: &Theme,
) {
    let label = &sig.label;
    if label.is_empty() {
        return;
    }
    let display = format!(" {} ", label);
    let width = (display.len() as u16 + 2).max(12);

    // Place above the cursor line if possible, otherwise below.
    let y = if popup_y > 1 {
        popup_y - 1
    } else {
        popup_y + 1
    };
    let x = popup_x.min(term_area.width.saturating_sub(width));
    let y = y.min(term_area.height.saturating_sub(1));

    let bg_color = rc(theme.hover_bg);
    let fg_color = rc(theme.hover_fg);
    let kw_color = rc(theme.keyword);
    let border_color = rc(theme.hover_border);

    // Compute which char indices are in the active parameter (byte → char mapping).
    let active_char_range: Option<(usize, usize)> = sig.active_param.and_then(|idx| {
        sig.params.get(idx).map(|&(start_byte, end_byte)| {
            let char_start = label[..start_byte].chars().count() + 1; // +1 for leading space
            let char_end = label[..end_byte].chars().count() + 1;
            (char_start, char_end)
        })
    });

    let buf = frame.buffer_mut();
    // Draw background row
    for col in 0..width {
        let cell_x = x + col;
        if cell_x < term_area.width && y < term_area.height {
            let cell = &mut buf[(cell_x, y)];
            cell.set_bg(bg_color);
            let ch = if col == 0 || col == width - 1 {
                '│'
            } else {
                ' '
            };
            cell.set_char(ch).set_fg(border_color);
        }
    }
    // Draw each character of the display string with appropriate color.
    for (j, ch) in display.chars().enumerate() {
        let cell_x = x + 1 + j as u16;
        if cell_x + 1 < x + width && cell_x < term_area.width && y < term_area.height {
            let in_active = active_char_range
                .map(|(s, e)| j >= s && j < e)
                .unwrap_or(false);
            let color = if in_active { kw_color } else { fg_color };
            let cell = &mut buf[(cell_x, y)];
            cell.set_char(ch).set_fg(color).set_bg(bg_color);
        }
    }
}

pub(super) fn render_folder_picker(
    frame: &mut ratatui::Frame,
    picker: &FolderPickerState,
    term_area: Rect,
    theme: &Theme,
) {
    let term_cols = term_area.width;
    let term_rows = term_area.height;

    // Same proportions as the fuzzy popup
    let width = (term_cols * 3 / 5).max(50);
    let height = (term_rows * 55 / 100).max(15);
    let x = (term_cols.saturating_sub(width)) / 2;
    let y = (term_rows.saturating_sub(height)) / 2;

    let bg_color = rc(theme.fuzzy_bg);
    let sel_bg_color = rc(theme.fuzzy_selected_bg);
    let fg_color = rc(theme.fuzzy_fg);
    let query_fg = rc(theme.fuzzy_query_fg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);

    let buf = frame.buffer_mut();

    // Clear popup background so stale characters don't persist.
    for row in y..y + height {
        for col in x..x + width {
            if col < term_area.width && row < term_area.height {
                set_cell(buf, col, row, ' ', fg_color, bg_color);
            }
        }
    }

    // Title varies by mode; for folder modes show the current root for orientation
    let root_display = if picker.mode != FolderPickerMode::OpenRecent {
        let r = picker.root.to_string_lossy();
        // Truncate from left if too long
        let max = (width as usize).saturating_sub(30).max(10);
        if r.len() > max {
            format!("…{}", &r[r.len() - max..])
        } else {
            r.into_owned()
        }
    } else {
        String::new()
    };
    let title_text = match picker.mode {
        FolderPickerMode::OpenFolder => format!(
            " Open Folder {}  {}/{} ",
            root_display,
            picker.filtered.len(),
            picker.all_entries.len()
        ),
        FolderPickerMode::OpenRecent => format!(" Open Recent  {} ", picker.filtered.len()),
    };

    // Row 0: top border ╭─ Title ── N/M ──╮
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

    // Row 1: query line │ > query_ │
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

    // Row 2: separator ├───────┤
    let row2 = y + 2;
    if row2 < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '├'
                } else if col == width - 1 {
                    '┤'
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

    for row_idx in 0..visible_rows {
        let result_idx = picker.scroll_top + row_idx;
        let ry = results_start + row_idx as u16;
        if ry >= results_end || ry >= term_area.height {
            break;
        }
        set_cell(buf, x, ry, '│', border_fg, bg_color);
        if x + width - 1 < term_area.width {
            set_cell(buf, x + width - 1, ry, '│', border_fg, bg_color);
        }
        let is_selected = result_idx == picker.selected;
        let row_bg = if is_selected { sel_bg_color } else { bg_color };
        for col in 1..width - 1 {
            let cx = x + col;
            if cx < term_area.width {
                set_cell(buf, cx, ry, ' ', fg_color, row_bg);
            }
        }
        if let Some(entry) = picker.filtered.get(result_idx) {
            // Show workspace files differently with a marker
            let display = entry.to_string_lossy();
            let is_workspace = entry
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == ".vimcode-workspace")
                .unwrap_or(false);
            let prefix = if is_selected { "▶ " } else { "  " };
            let marker = if is_workspace { "⚙ " } else { "📁 " };
            let row_text = format!("{}{}{}", prefix, marker, display);
            for (j, ch) in row_text.chars().enumerate() {
                let cx = x + 1 + j as u16;
                if cx + 1 < x + width && cx < term_area.width {
                    set_cell(buf, cx, ry, ch, fg_color, row_bg);
                }
            }
        }
    }

    // Bottom border ╰───────╯
    let bottom = y + height - 1;
    if bottom < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '╰'
                } else if col == width - 1 {
                    '╯'
                } else {
                    '─'
                };
                set_cell(buf, cx, bottom, ch, border_fg, bg_color);
            }
        }
    }
}

/// Render the unified picker popup. Supports single-pane (no preview) and
/// two-pane (with preview) layouts, fuzzy match highlighting, and scrollbar.
pub(super) fn render_picker_popup(
    frame: &mut ratatui::Frame,
    picker: &render::PickerPanel,
    term_area: Rect,
    theme: &Theme,
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
        super::quadraui_tui::draw_palette(frame.buffer_mut(), area, &palette, theme);
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

pub(super) fn render_tab_switcher_popup(
    buf: &mut ratatui::buffer::Buffer,
    term_area: Rect,
    ts: &render::TabSwitcherPanel,
    theme: &Theme,
) {
    if ts.items.is_empty() {
        return;
    }
    let item_count = ts.items.len();
    // Size: 45% width (min 40, max 80), height = items + 2 (borders)
    let width = (term_area.width * 45 / 100).clamp(40, 80);
    let max_visible = (term_area.height as usize).saturating_sub(4).min(20);
    let visible = item_count.min(max_visible);
    let height = visible as u16 + 2; // top + bottom border

    // Centered
    let x = (term_area.width.saturating_sub(width)) / 2;
    let y = (term_area.height.saturating_sub(height)) / 2;

    let bg = rc(theme.fuzzy_bg);
    let fg = rc(theme.fuzzy_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);

    // Top border
    if y < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '╭'
                } else if col == width - 1 {
                    '╮'
                } else {
                    '─'
                };
                set_cell(buf, cx, y, ch, border_fg, bg);
            }
        }
        // Title overlay
        let title = " Open Tabs ";
        for (i, ch) in title.chars().enumerate() {
            let cx = x + 2 + i as u16;
            if cx + 1 < x + width && cx < term_area.width {
                set_cell(buf, cx, y, ch, title_fg, bg);
            }
        }
    }

    // Scroll offset so selected item is always visible
    let scroll = if ts.selected_idx >= visible {
        ts.selected_idx - visible + 1
    } else {
        0
    };

    // Items
    let inner_w = (width - 2) as usize;
    for i in 0..visible {
        let item_idx = scroll + i;
        if item_idx >= item_count {
            break;
        }
        let ry = y + 1 + i as u16;
        if ry >= term_area.height {
            break;
        }
        let is_selected = item_idx == ts.selected_idx;
        let row_bg = if is_selected { sel_bg } else { bg };

        // Clear row
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 || col == width - 1 {
                    '│'
                } else {
                    ' '
                };
                let c = if col == 0 || col == width - 1 {
                    border_fg
                } else {
                    fg
                };
                set_cell(
                    buf,
                    cx,
                    ry,
                    ch,
                    c,
                    if col == 0 || col == width - 1 {
                        bg
                    } else {
                        row_bg
                    },
                );
            }
        }

        let (name, path, dirty) = &ts.items[item_idx];
        let dirty_mark = if *dirty { " ●" } else { "" };
        let prefix = if is_selected { "▶ " } else { "  " };
        let label = format!("{}{}{}", prefix, name, dirty_mark);

        // Draw label
        for (j, ch) in label.chars().enumerate() {
            let cx = x + 1 + j as u16;
            if cx + 1 < x + width && cx < term_area.width {
                set_cell(buf, cx, ry, ch, fg, row_bg);
            }
        }

        // Draw path right-aligned (dimmed)
        if !path.is_empty() && inner_w > label.chars().count() + 4 {
            let available = inner_w - label.chars().count() - 2;
            let display_path = if path.len() > available {
                &path[path.len() - available..]
            } else {
                path.as_str()
            };
            let path_start = inner_w - display_path.len();
            for (j, ch) in display_path.chars().enumerate() {
                let cx = x + 1 + (path_start + j) as u16;
                if cx + 1 < x + width && cx < term_area.width {
                    set_cell(buf, cx, ry, ch, border_fg, row_bg);
                }
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
                } else {
                    '─'
                };
                set_cell(buf, cx, bottom, ch, border_fg, bg);
            }
        }
    }
}

pub(super) fn render_quit_confirm_overlay(
    buf: &mut ratatui::buffer::Buffer,
    term_area: Rect,
    theme: &Theme,
) {
    // Lines of content (title, blank, message, blank, 3 options, blank, bottom)
    let lines: &[(&str, bool)] = &[
        ("  You have unsaved changes.", false),
        ("", false),
        ("  [S]   Save All & Quit", true),
        ("  [Q]   Quit Without Saving", true),
        ("  [Esc] Cancel", true),
    ];
    let title = " Unsaved Changes ";
    let width: u16 = 42;
    // top border + blank + content rows + blank + bottom border
    let height: u16 = 2 + 1 + lines.len() as u16 + 1;

    let x = (term_area.width.saturating_sub(width)) / 2;
    let y = (term_area.height.saturating_sub(height)) / 2;

    let bg_color = rc(theme.fuzzy_bg);
    let fg_color = rc(theme.fuzzy_fg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);
    let key_fg = rc(theme.fuzzy_query_fg);

    // Top border row ╭─ Unsaved Changes ─╮
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 {
            '╭'
        } else if col == width - 1 {
            '╮'
        } else {
            '─'
        };
        set_cell(buf, cx, y, ch, border_fg, bg_color);
    }
    // Overlay title
    for (i, ch) in title.chars().enumerate() {
        let cx = x + 2 + i as u16;
        if cx + 1 < x + width {
            set_cell(buf, cx, y, ch, title_fg, bg_color);
        }
    }

    // Blank row after title
    let blank_row = y + 1;
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 || col == width - 1 {
            '│'
        } else {
            ' '
        };
        let fg = if col == 0 || col == width - 1 {
            border_fg
        } else {
            fg_color
        };
        set_cell(buf, cx, blank_row, ch, fg, bg_color);
    }

    // Content rows
    for (row_i, (text, is_key_row)) in lines.iter().enumerate() {
        let ry = y + 2 + row_i as u16;
        for col in 0..width {
            let cx = x + col;
            let ch = if col == 0 || col == width - 1 {
                '│'
            } else {
                ' '
            };
            let fg = if col == 0 || col == width - 1 {
                border_fg
            } else {
                fg_color
            };
            set_cell(buf, cx, ry, ch, fg, bg_color);
        }
        let row_fg = if *is_key_row { key_fg } else { fg_color };
        for (j, ch) in text.chars().enumerate() {
            let cx = x + 1 + j as u16;
            if cx + 1 < x + width {
                set_cell(buf, cx, ry, ch, row_fg, bg_color);
            }
        }
    }

    // Blank row before bottom border
    let pre_bottom = y + 2 + lines.len() as u16;
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 || col == width - 1 {
            '│'
        } else {
            ' '
        };
        let fg = if col == 0 || col == width - 1 {
            border_fg
        } else {
            fg_color
        };
        set_cell(buf, cx, pre_bottom, ch, fg, bg_color);
    }

    // Bottom border ╰──────╯
    let bottom = y + height - 1;
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 {
            '╰'
        } else if col == width - 1 {
            '╯'
        } else {
            '─'
        };
        set_cell(buf, cx, bottom, ch, border_fg, bg_color);
    }
}

pub(super) fn render_close_tab_confirm_overlay(
    buf: &mut ratatui::buffer::Buffer,
    term_area: Rect,
    theme: &Theme,
) {
    let lines: &[(&str, bool)] = &[
        ("  This file has unsaved changes.", false),
        ("", false),
        ("  [S]   Save & Close Tab", true),
        ("  [D]   Discard & Close Tab", true),
        ("  [Esc] Cancel", true),
    ];
    let title = " Unsaved Changes ";
    let width: u16 = 42;
    let height: u16 = 2 + 1 + lines.len() as u16 + 1;

    let x = (term_area.width.saturating_sub(width)) / 2;
    let y = (term_area.height.saturating_sub(height)) / 2;

    let bg_color = rc(theme.fuzzy_bg);
    let fg_color = rc(theme.fuzzy_fg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);
    let key_fg = rc(theme.fuzzy_query_fg);

    // Top border row
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 {
            '╭'
        } else if col == width - 1 {
            '╮'
        } else {
            '─'
        };
        set_cell(buf, cx, y, ch, border_fg, bg_color);
    }
    for (i, ch) in title.chars().enumerate() {
        let cx = x + 2 + i as u16;
        if cx + 1 < x + width {
            set_cell(buf, cx, y, ch, title_fg, bg_color);
        }
    }

    // Blank row after title
    let blank_row = y + 1;
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 || col == width - 1 {
            '│'
        } else {
            ' '
        };
        let fg = if col == 0 || col == width - 1 {
            border_fg
        } else {
            fg_color
        };
        set_cell(buf, cx, blank_row, ch, fg, bg_color);
    }

    // Content rows
    for (row_i, (text, is_key_row)) in lines.iter().enumerate() {
        let ry = y + 2 + row_i as u16;
        for col in 0..width {
            let cx = x + col;
            let ch = if col == 0 || col == width - 1 {
                '│'
            } else {
                ' '
            };
            let fg = if col == 0 || col == width - 1 {
                border_fg
            } else {
                fg_color
            };
            set_cell(buf, cx, ry, ch, fg, bg_color);
        }
        let row_fg = if *is_key_row { key_fg } else { fg_color };
        for (j, ch) in text.chars().enumerate() {
            let cx = x + 1 + j as u16;
            if cx + 1 < x + width {
                set_cell(buf, cx, ry, ch, row_fg, bg_color);
            }
        }
    }

    // Blank row before bottom border
    let pre_bottom = y + 2 + lines.len() as u16;
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 || col == width - 1 {
            '│'
        } else {
            ' '
        };
        let fg = if col == 0 || col == width - 1 {
            border_fg
        } else {
            fg_color
        };
        set_cell(buf, cx, pre_bottom, ch, fg, bg_color);
    }

    // Bottom border
    let bottom = y + height - 1;
    for col in 0..width {
        let cx = x + col;
        let ch = if col == 0 {
            '╰'
        } else if col == width - 1 {
            '╯'
        } else {
            '─'
        };
        set_cell(buf, cx, bottom, ch, border_fg, bg_color);
    }
}

pub(super) fn render_dialog_popup(
    buf: &mut ratatui::buffer::Buffer,
    term_area: Rect,
    dialog: &render::DialogPanel,
    theme: &Theme,
) {
    let bg = rc(theme.fuzzy_bg);
    let fg = rc(theme.fuzzy_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);

    // Compute dimensions: widest line of body or title, at least 40.
    let body_max = dialog.body.iter().map(|l| l.len()).max().unwrap_or(0);
    let btn_max_label = dialog
        .buttons
        .iter()
        .map(|(lbl, _)| lbl.len() + 4)
        .max()
        .unwrap_or(0);
    let btn_row_len: usize = if dialog.vertical_buttons {
        btn_max_label + 2
    } else {
        dialog
            .buttons
            .iter()
            .map(|(lbl, _)| lbl.len() + 4)
            .sum::<usize>()
            + 2
    };
    let content_width = body_max.max(dialog.title.len() + 4).max(btn_row_len);
    let width = (content_width as u16 + 4).clamp(40, term_area.width.saturating_sub(4));
    let has_input = dialog.input.is_some();
    let input_rows: u16 = if has_input { 1 } else { 0 };
    let btn_rows: u16 = if dialog.vertical_buttons {
        dialog.buttons.len() as u16
    } else {
        1
    };
    // Height: top border + title + blank + body lines + input + blank + button rows + bottom border.
    let height = (3 + dialog.body.len() as u16 + input_rows + 1 + btn_rows + 1)
        .min(term_area.height.saturating_sub(4));

    let x = (term_area.width.saturating_sub(width)) / 2;
    let y = (term_area.height.saturating_sub(height)) / 2;

    // Clear background.
    for row in y..y + height {
        for col in x..x + width {
            if col < term_area.width && row < term_area.height {
                set_cell(buf, col, row, ' ', fg, bg);
            }
        }
    }

    // Top border.
    for col in 0..width {
        let cx = x + col;
        if cx < term_area.width {
            let ch = if col == 0 {
                '╭'
            } else if col == width - 1 {
                '╮'
            } else {
                '─'
            };
            set_cell(buf, cx, y, ch, border_fg, bg);
        }
    }
    // Title overlay.
    let title = format!(" {} ", dialog.title);
    for (i, ch) in title.chars().enumerate() {
        let cx = x + 2 + i as u16;
        if cx + 1 < x + width && cx < term_area.width {
            set_cell(buf, cx, y, ch, title_fg, bg);
        }
    }

    // Left/right borders for content rows.
    for row in (y + 1)..(y + height - 1) {
        if row < term_area.height {
            if x < term_area.width {
                set_cell(buf, x, row, '│', border_fg, bg);
            }
            let rx = x + width - 1;
            if rx < term_area.width {
                set_cell(buf, rx, row, '│', border_fg, bg);
            }
        }
    }

    // Body lines.
    let body_y = y + 2;
    for (i, line) in dialog.body.iter().enumerate() {
        let row = body_y + i as u16;
        if row >= y + height - 2 {
            break;
        }
        for (j, ch) in line.chars().enumerate() {
            let cx = x + 2 + j as u16;
            if cx + 1 < x + width && cx < term_area.width && row < term_area.height {
                set_cell(buf, cx, row, ch, fg, bg);
            }
        }
    }

    // Input field (if present) — between body and buttons.
    if let Some(ref input) = dialog.input {
        let input_y = body_y + dialog.body.len() as u16 + 1;
        if input_y < y + height - 2 && input_y < term_area.height {
            // Draw input background.
            let inp_bg = rc(theme.completion_bg);
            for col_i in (x + 2)..(x + width - 1).min(term_area.width) {
                set_cell(buf, col_i, input_y, ' ', fg, inp_bg);
            }
            // Draw input text.
            let display = format!(" {}", input.display);
            for (j, ch) in display.chars().enumerate() {
                let cx = x + 2 + j as u16;
                if cx + 1 < x + width && cx < term_area.width {
                    set_cell(buf, cx, input_y, ch, fg, inp_bg);
                }
            }
        }
    }

    // Buttons — vertical list or horizontal row depending on mode.
    if dialog.vertical_buttons {
        let btn_start_y = y + height - 1 - btn_rows;
        for (i, (label, is_selected)) in dialog.buttons.iter().enumerate() {
            let row = btn_start_y + i as u16;
            if row >= y + height - 1 || row >= term_area.height {
                break;
            }
            let btn_bg = if *is_selected { sel_bg } else { bg };
            // Clear the row background for selection highlight.
            for col_i in (x + 1)..(x + width - 1).min(term_area.width) {
                set_cell(buf, col_i, row, ' ', fg, btn_bg);
            }
            let prefix = if *is_selected { "▸ " } else { "  " };
            let btn_text = format!("{}{}", prefix, label);
            for (j, ch) in btn_text.chars().enumerate() {
                let cx = x + 2 + j as u16;
                if cx + 1 < x + width && cx < term_area.width {
                    set_cell(buf, cx, row, ch, fg, btn_bg);
                }
            }
        }
    } else {
        let btn_y = y + height - 2;
        if btn_y < term_area.height {
            let mut col_offset = 2u16;
            for (label, is_selected) in &dialog.buttons {
                let btn_text = format!("  {}  ", label);
                let btn_bg = if *is_selected { sel_bg } else { bg };
                for ch in btn_text.chars() {
                    let cx = x + col_offset;
                    if cx + 1 < x + width && cx < term_area.width {
                        set_cell(buf, cx, btn_y, ch, fg, btn_bg);
                    }
                    col_offset += 1;
                }
            }
        }
    }

    // Bottom border.
    let bottom = y + height - 1;
    if bottom < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx < term_area.width {
                let ch = if col == 0 {
                    '╰'
                } else if col == width - 1 {
                    '╯'
                } else {
                    '─'
                };
                set_cell(buf, cx, bottom, ch, border_fg, bg);
            }
        }
    }
}

pub(super) fn render_window(
    frame: &mut ratatui::Frame,
    area: Rect,
    window: &RenderedWindow,
    theme: &Theme,
) {
    // Reserve bottom row for per-window status line when present
    let status_bar_row = if window.status_line.is_some() && area.height > 1 {
        Some(area.y + area.height - 1)
    } else {
        None
    };
    let area = if status_bar_row.is_some() {
        Rect {
            height: area.height - 1,
            ..area
        }
    } else {
        area
    };

    let window_bg = rc(if window.show_active_bg {
        theme.active_background
    } else {
        theme.background
    });
    let default_fg = rc(theme.foreground);
    let gutter_w = window.gutter_char_width as u16;
    let viewport_lines = area.height as usize;
    let has_scrollbar = window.total_lines > viewport_lines && area.width > gutter_w + 1;
    let viewport_cols =
        (area.width as usize).saturating_sub(gutter_w as usize + if has_scrollbar { 1 } else { 0 });
    let has_h_scrollbar = window.max_col > viewport_cols && area.height > 1;

    // Fill background
    for row in 0..area.height {
        for col in 0..area.width {
            set_cell(
                frame.buffer_mut(),
                area.x + col,
                area.y + row,
                ' ',
                default_fg,
                window_bg,
            );
        }
    }

    for (row_idx, line) in window.lines.iter().enumerate() {
        let screen_y = area.y + row_idx as u16;
        if screen_y >= area.y + area.height {
            break;
        }

        // Cursorline / Diff / DAP stopped-line background.
        let line_bg = if line.is_dap_current {
            rc(theme.dap_stopped_bg)
        } else {
            match line.diff_status {
                Some(DiffLine::Added) => rc(theme.diff_added_bg),
                Some(DiffLine::Removed) => rc(theme.diff_removed_bg),
                Some(DiffLine::Padding) => rc(theme.diff_padding_bg),
                _ if line.is_current_line && window.is_active && window.cursorline => {
                    rc(theme.cursorline_bg)
                }
                _ => window_bg,
            }
        };
        if line_bg != window_bg {
            for col in 0..area.width {
                set_cell(
                    frame.buffer_mut(),
                    area.x + col,
                    screen_y,
                    ' ',
                    default_fg,
                    line_bg,
                );
            }
        }

        // Gutter
        if gutter_w > 0 {
            let line_num_fg = rc(if line.is_current_line {
                theme.line_number_active_fg
            } else {
                theme.line_number_fg
            });
            // The bp column offset: 1 when has_breakpoints, else 0.
            // The git column offset: bp_offset + 1 when has_git_diff, else bp_offset.
            let bp_offset = if window.has_breakpoints { 1 } else { 0 };
            let git_offset = if window.has_git_diff {
                bp_offset + 1
            } else {
                bp_offset
            };
            for (i, ch) in line.gutter_text.chars().enumerate() {
                let gx = area.x + i as u16;
                if gx >= area.x + gutter_w {
                    break;
                }
                let fg = if window.has_breakpoints && i == 0 {
                    // Breakpoint column: red when active, dimmed otherwise.
                    if line.is_dap_current || line.is_breakpoint {
                        rc(theme.diagnostic_error)
                    } else {
                        line_num_fg
                    }
                } else if window.has_git_diff && i == bp_offset {
                    // Git column.
                    rc(match line.git_diff {
                        Some(GitLineStatus::Added) => theme.git_added,
                        Some(GitLineStatus::Modified) => theme.git_modified,
                        Some(GitLineStatus::Deleted) => theme.git_deleted,
                        None => theme.line_number_fg,
                    })
                } else {
                    let _ = git_offset; // suppress unused-variable warning
                    line_num_fg
                };
                set_cell(frame.buffer_mut(), gx, screen_y, ch, fg, line_bg);
            }
            // Diagnostic gutter icon (overwrite leftmost gutter char)
            if let Some(severity) = window.diagnostic_gutter.get(&line.line_idx) {
                let (diag_ch, diag_color) = match severity {
                    DiagnosticSeverity::Error => ('●', rc(theme.diagnostic_error)),
                    DiagnosticSeverity::Warning => ('●', rc(theme.diagnostic_warning)),
                    DiagnosticSeverity::Information => ('●', rc(theme.diagnostic_info)),
                    DiagnosticSeverity::Hint => ('●', rc(theme.diagnostic_hint)),
                };
                set_cell(
                    frame.buffer_mut(),
                    area.x,
                    screen_y,
                    diag_ch,
                    diag_color,
                    line_bg,
                );
            } else if !line.is_wrap_continuation
                && window.code_action_lines.contains(&line.line_idx)
            {
                // Code action lightbulb (only when no diagnostic icon)
                set_cell(
                    frame.buffer_mut(),
                    area.x,
                    screen_y,
                    crate::icons::LIGHTBULB.c(),
                    rc(theme.lightbulb),
                    line_bg,
                );
            }
        }

        // Text (narrowed by 1 when scrollbar is shown)
        let text_area_x = area.x + gutter_w;
        let text_width = area
            .width
            .saturating_sub(gutter_w)
            .saturating_sub(if has_scrollbar { 1 } else { 0 });
        render_text_line(
            frame.buffer_mut(),
            text_area_x,
            screen_y,
            text_width,
            line,
            window.scroll_left,
            theme,
            line_bg,
            window.tabstop,
        );

        // Indent guides: draw │ at guide columns where the cell is a space
        if !line.indent_guides.is_empty() {
            let guide_fg = rc(theme.indent_guide_fg);
            let active_fg = rc(theme.indent_guide_active_fg);
            for &guide_col in &line.indent_guides {
                if guide_col < window.scroll_left {
                    continue;
                }
                let vis_col = (guide_col - window.scroll_left) as u16;
                if vis_col >= text_width {
                    break;
                }
                let cx = text_area_x + vis_col;
                if cx < area.x + area.width && screen_y < area.y + area.height {
                    let cell = &mut frame.buffer_mut()[(cx, screen_y)];
                    // Only draw guide if the cell is a space (don't overwrite text)
                    if cell.symbol() == " " {
                        let is_active = window.active_indent_col == Some(guide_col);
                        let fg = if is_active { active_fg } else { guide_fg };
                        cell.set_char('│');
                        cell.set_fg(fg);
                    }
                }
            }
        }

        // Color columns: tint background at specified column positions
        if !line.colorcolumns.is_empty() {
            let cc_bg = rc(theme.colorcolumn_bg);
            for &cc_col in &line.colorcolumns {
                if cc_col < window.scroll_left {
                    continue;
                }
                let vis_col = (cc_col - window.scroll_left) as u16;
                if vis_col >= text_width {
                    break;
                }
                let cx = text_area_x + vis_col;
                if cx < area.x + area.width && screen_y < area.y + area.height {
                    let cell = &mut frame.buffer_mut()[(cx, screen_y)];
                    cell.set_bg(cc_bg);
                }
            }
        }

        // Ghost continuation lines — draw full line in ghost colour.
        if line.is_ghost_continuation {
            if let Some(ghost) = &line.ghost_suffix {
                let ghost_fg = rc(theme.ghost_text_fg);
                for (i, ch) in ghost.chars().enumerate() {
                    let gx = text_area_x + i as u16;
                    if gx >= text_area_x + text_width {
                        break;
                    }
                    set_cell(frame.buffer_mut(), gx, screen_y, ch, ghost_fg, line_bg);
                }
            }
        }

        // Diagnostic underlines (UNDERLINED modifier on diagnostic spans)
        for dm in &line.diagnostics {
            let diag_fg = rc(match dm.severity {
                DiagnosticSeverity::Error => theme.diagnostic_error,
                DiagnosticSeverity::Warning => theme.diagnostic_warning,
                DiagnosticSeverity::Information => theme.diagnostic_info,
                DiagnosticSeverity::Hint => theme.diagnostic_hint,
            });
            let vis_start = char_col_to_visual(&line.raw_text, dm.start_col, window.tabstop);
            let vis_end = char_col_to_visual(&line.raw_text, dm.end_col, window.tabstop);
            for vcol in vis_start..vis_end {
                if vcol < window.scroll_left {
                    continue;
                }
                let vis_col = (vcol - window.scroll_left) as u16;
                if vis_col >= text_width {
                    break;
                }
                let cx = text_area_x + vis_col;
                if cx < area.x + area.width && screen_y < area.y + area.height {
                    let cell = &mut frame.buffer_mut()[(cx, screen_y)];
                    cell.set_fg(diag_fg);
                    cell.modifier |= Modifier::UNDERLINED;
                    cell.underline_color = diag_fg;
                }
            }
        }

        // Spell error underlines
        let spell_fg = rc(theme.spell_error);
        for sm in &line.spell_errors {
            let vis_start = char_col_to_visual(&line.raw_text, sm.start_col, window.tabstop);
            let vis_end = char_col_to_visual(&line.raw_text, sm.end_col, window.tabstop);
            for vcol in vis_start..vis_end {
                if vcol < window.scroll_left {
                    continue;
                }
                let vis_col = (vcol - window.scroll_left) as u16;
                if vis_col >= text_width {
                    break;
                }
                let cx = text_area_x + vis_col;
                if cx < area.x + area.width && screen_y < area.y + area.height {
                    let cell = &mut frame.buffer_mut()[(cx, screen_y)];
                    cell.set_fg(spell_fg);
                    cell.modifier |= Modifier::UNDERLINED;
                    cell.underline_color = spell_fg;
                }
            }
        }

        // Bracket match highlighting
        let bracket_bg = rc(theme.bracket_match_bg);
        for &(view_line, col) in &window.bracket_match_positions {
            if view_line == row_idx {
                let vis = char_col_to_visual(&line.raw_text, col, window.tabstop);
                if vis < window.scroll_left {
                    continue;
                }
                let vis_col = (vis - window.scroll_left) as u16;
                if vis_col >= text_width {
                    continue;
                }
                let cx = text_area_x + vis_col;
                if cx < area.x + area.width && screen_y < area.y + area.height {
                    let cell = &mut frame.buffer_mut()[(cx, screen_y)];
                    cell.set_bg(bracket_bg);
                }
            }
        }
    }

    // Selection overlay
    if let Some(sel) = &window.selection {
        render_selection(
            frame.buffer_mut(),
            area,
            window,
            sel,
            window_bg,
            theme.selection,
            rc(theme.foreground),
        );
    }

    // Extra selections (Ctrl+D multi-cursor word highlights)
    for esel in &window.extra_selections {
        render_selection(
            frame.buffer_mut(),
            area,
            window,
            esel,
            window_bg,
            theme.selection,
            rc(theme.foreground),
        );
    }

    // Yank highlight overlay (brief flash after yank)
    if let Some(yh) = &window.yank_highlight {
        render_selection(
            frame.buffer_mut(),
            area,
            window,
            yh,
            window_bg,
            theme.yank_highlight_bg,
            rc(theme.foreground),
        );
    }

    // Vertical scrollbar
    if has_scrollbar {
        render_scrollbar(
            frame.buffer_mut(),
            area,
            window.scroll_top,
            window.total_lines,
            viewport_lines,
            has_h_scrollbar,
            theme,
        );
    }

    // Horizontal scrollbar
    if has_h_scrollbar {
        render_h_scrollbar(
            frame.buffer_mut(),
            area,
            window.scroll_left,
            window.max_col,
            viewport_cols,
            gutter_w,
            has_scrollbar,
            theme,
        );
    }

    // Cursor
    if let Some((cursor_pos, cursor_shape)) = &window.cursor {
        let cursor_screen_y = area.y + cursor_pos.view_line as u16;
        let raw = window
            .lines
            .get(cursor_pos.view_line)
            .map(|l| l.raw_text.as_str())
            .unwrap_or("");
        let vis_col = char_col_to_visual(raw, cursor_pos.col, window.tabstop)
            .saturating_sub(window.scroll_left) as u16;
        let cursor_screen_x = area.x + gutter_w + vis_col;

        let buf = frame.buffer_mut();
        let buf_area = buf.area;

        match cursor_shape {
            CursorShape::Block => {
                if cursor_screen_x < buf_area.x + buf_area.width
                    && cursor_screen_y < buf_area.y + buf_area.height
                {
                    let cell = &mut buf[(cursor_screen_x, cursor_screen_y)];
                    let old_fg = cell.fg;
                    let old_bg = cell.bg;
                    cell.set_fg(old_bg).set_bg(old_fg);
                }
            }
            CursorShape::Bar | CursorShape::Underline => {
                frame.set_cursor_position((cursor_screen_x, cursor_screen_y));
            }
        }
    }

    // AI ghost text — draw after cursor at cursor position in muted colour.
    if let Some((cursor_pos, _)) = &window.cursor {
        if let Some(rl) = window.lines.get(cursor_pos.view_line) {
            if let Some(ghost) = &rl.ghost_suffix {
                let ghost_screen_y = area.y + cursor_pos.view_line as u16;
                let vis_col = char_col_to_visual(&rl.raw_text, cursor_pos.col, window.tabstop)
                    .saturating_sub(window.scroll_left) as u16;
                let ghost_start_x = area.x + gutter_w + vis_col;
                let ghost_fg = rc(theme.ghost_text_fg);
                let buf = frame.buffer_mut();
                for (i, ch) in ghost.chars().enumerate() {
                    let gx = ghost_start_x + i as u16;
                    if gx >= area.x + area.width {
                        break;
                    }
                    set_cell(buf, gx, ghost_screen_y, ch, ghost_fg, window_bg);
                }
            }
        }
    }

    // Secondary cursors (multi-cursor) — render with cursor color background.
    let cursor_color = ratatui::style::Color::Rgb(theme.cursor.r, theme.cursor.g, theme.cursor.b);
    let has_extra_sels = !window.extra_selections.is_empty();
    for extra_pos in &window.extra_cursors {
        let sy = area.y + extra_pos.view_line as u16;
        // When Ctrl+D selections are active, show cursor at col+1 (right of selection)
        let col = if has_extra_sels {
            extra_pos.col + 1
        } else {
            extra_pos.col
        };
        let raw = window
            .lines
            .get(extra_pos.view_line)
            .map(|l| l.raw_text.as_str())
            .unwrap_or("");
        let vis_col =
            char_col_to_visual(raw, col, window.tabstop).saturating_sub(window.scroll_left) as u16;
        let sx = area.x + gutter_w + vis_col;
        let buf = frame.buffer_mut();
        if sx < buf.area.x + buf.area.width && sy < buf.area.y + buf.area.height {
            let cell = &mut buf[(sx, sy)];
            cell.set_bg(cursor_color).set_fg(ratatui::style::Color::Rgb(
                theme.background.r,
                theme.background.g,
                theme.background.b,
            ));
        }
    }

    // ── Per-window status bar ────────────────────────────────────────────────
    if let (Some(status), Some(sy)) = (&window.status_line, status_bar_row) {
        render_window_status_line(frame.buffer_mut(), area.x, sy, area.width, status, theme);
    }
}

/// Draw a per-window status line into the given row.
fn render_window_status_line(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    width: u16,
    status: &crate::render::WindowStatusLine,
    theme: &crate::render::Theme,
) {
    // A.6a: status line rendering delegates to the quadraui `StatusBar`
    // primitive. The adapter encodes engine-side `StatusAction` values as
    // opaque `WidgetId` strings; `status_segment_hit_test` (in mouse.rs)
    // decodes them back to `StatusAction` via `status_action_from_id`.
    let bar = crate::render::window_status_line_to_status_bar(
        status,
        quadraui::WidgetId::new("status:window"),
    );
    let area = ratatui::layout::Rect {
        x,
        y,
        width,
        height: 1,
    };
    super::quadraui_tui::draw_status_bar(buf, area, &bar, theme);
}

pub(super) fn render_scrollbar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    scroll_top: usize,
    total_lines: usize,
    viewport_lines: usize,
    // When true, leave the last row for the horizontal scrollbar (don't draw there)
    has_h_scrollbar: bool,
    theme: &Theme,
) {
    if area.height == 0 || total_lines == 0 {
        return;
    }
    let track_fg = rc(theme.separator);
    let thumb_fg = rc(theme.scrollbar_thumb);
    let sb_bg = rc(theme.background);
    // Track height: reserve last row for h-scrollbar if present
    let track_h = if has_h_scrollbar {
        area.height.saturating_sub(1)
    } else {
        area.height
    };
    if track_h == 0 {
        return;
    }
    let h = track_h as f64;
    let thumb_size = ((viewport_lines as f64 / total_lines as f64) * h)
        .ceil()
        .max(1.0) as u16;
    let thumb_top = ((scroll_top as f64 / total_lines as f64) * h).floor() as u16;
    let sb_x = area.x + area.width - 1;
    for dy in 0..track_h {
        let y = area.y + dy;
        let in_thumb = dy >= thumb_top && dy < thumb_top + thumb_size;
        let ch = if in_thumb { '█' } else { '░' };
        let fg = if in_thumb { thumb_fg } else { track_fg };
        set_cell(buf, sb_x, y, ch, fg, sb_bg);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_h_scrollbar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    scroll_left: usize,
    max_col: usize,
    viewport_cols: usize,
    gutter_w: u16,
    has_v_scrollbar: bool,
    theme: &Theme,
) {
    if area.height == 0 || max_col == 0 || viewport_cols == 0 {
        return;
    }
    let thumb_fg = rc(theme.scrollbar_thumb);
    let sb_bg = rc(theme.background);
    let corner_fg = rc(theme.separator);

    let sb_y = area.y + area.height - 1;
    let track_x = area.x + gutter_w;
    // Leave the rightmost cell for the v-scrollbar corner / separator
    let track_w = area
        .width
        .saturating_sub(gutter_w + if has_v_scrollbar { 1 } else { 0 });
    if track_w == 0 {
        return;
    }

    let w = track_w as f64;
    let thumb_size = ((viewport_cols as f64 / max_col as f64) * w)
        .ceil()
        .max(1.0) as u16;
    let thumb_left = ((scroll_left as f64 / max_col as f64) * w).floor() as u16;

    for dx in 0..track_w {
        let x = track_x + dx;
        let in_thumb = dx >= thumb_left && dx < thumb_left + thumb_size;
        let ch = if in_thumb { '▄' } else { ' ' };
        let fg = if in_thumb { thumb_fg } else { sb_bg };
        set_cell(buf, x, sb_y, ch, fg, sb_bg);
    }
    // Corner cell (intersection of v-scrollbar column and h-scrollbar row)
    if has_v_scrollbar {
        set_cell(buf, area.x + area.width - 1, sb_y, '┘', corner_fg, sb_bg);
    }
}

/// Convert a character-index column to a visual column, expanding tabs.
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

#[allow(clippy::too_many_arguments)]
pub(super) fn render_text_line(
    buf: &mut ratatui::buffer::Buffer,
    x_start: u16,
    y: u16,
    max_width: u16,
    line: &RenderedLine,
    scroll_left: usize,
    theme: &Theme,
    window_bg: RColor,
    tabstop: usize,
) {
    let raw = &line.raw_text;
    let chars: Vec<char> = raw.chars().filter(|&c| c != '\n' && c != '\r').collect();

    let mut char_fgs: Vec<Color> = vec![theme.foreground; chars.len()];
    let mut char_bgs: Vec<Option<Color>> = vec![None; chars.len()];
    let mut char_mods: Vec<Modifier> = vec![Modifier::empty(); chars.len()];

    for span in &line.spans {
        let start = byte_to_char_idx(raw, span.start_byte);
        let end = byte_to_char_idx(raw, span.end_byte).min(chars.len());
        for i in start..end {
            char_fgs[i] = span.style.fg;
            char_bgs[i] = span.style.bg;
            let mut m = Modifier::empty();
            if span.style.bold {
                m |= Modifier::BOLD;
            }
            if span.style.italic {
                m |= Modifier::ITALIC;
            }
            char_mods[i] = m;
        }
    }

    // Expand characters to visual columns, handling tabs.
    // Each entry: (visual_col, char_idx) for non-tab chars, or multiple
    // space entries for a single tab.
    let tabstop = tabstop.max(1);
    let mut vis_col: usize = 0;
    // Build a flat list of (visual_column, char_to_draw, char_index_for_style)
    let mut cells: Vec<(usize, char, usize)> = Vec::with_capacity(chars.len());
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '\t' {
            let next_stop = ((vis_col / tabstop) + 1) * tabstop;
            while vis_col < next_stop {
                cells.push((vis_col, ' ', i));
                vis_col += 1;
            }
        } else {
            cells.push((vis_col, ch, i));
            vis_col += 1;
        }
    }
    let total_vis_cols = vis_col;

    for &(vcol, ch, ci) in &cells {
        if vcol < scroll_left {
            continue;
        }
        let col = (vcol - scroll_left) as u16;
        if col >= max_width {
            break;
        }
        let fg = rc(char_fgs[ci]);
        let bg = char_bgs[ci].map(rc).unwrap_or(window_bg);
        if char_mods[ci].is_empty() {
            set_cell(buf, x_start + col, y, ch, fg, bg);
        } else {
            set_cell_styled(buf, x_start + col, y, ch, fg, bg, char_mods[ci], None);
        }
    }

    // Inline annotation / virtual text (e.g. git blame)
    if let Some(ann) = &line.annotation {
        let visible_cols = total_vis_cols.saturating_sub(scroll_left);
        let ann_start = x_start + visible_cols.min(max_width as usize) as u16;
        let ann_fg = rc(theme.annotation_fg);
        for (i, ch) in ann.chars().enumerate() {
            let col = ann_start + i as u16;
            if col >= x_start + max_width {
                break;
            }
            set_cell(buf, col, y, ch, ann_fg, window_bg);
        }
    }
}

pub(super) fn render_selection(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    window: &RenderedWindow,
    sel: &render::SelectionRange,
    window_bg: RColor,
    color: render::Color,
    default_fg: RColor,
) {
    let sel_bg = rc(color);
    let gutter_w = window.gutter_char_width as u16;
    let text_area_x = area.x + gutter_w;
    let text_width = area.width.saturating_sub(gutter_w) as usize;

    for (row_idx, line) in window.lines.iter().enumerate() {
        let buffer_line = line.line_idx;
        if buffer_line < sel.start_line || buffer_line > sel.end_line {
            continue;
        }
        let screen_y = area.y + row_idx as u16;
        let seg_offset = line.segment_col_offset;

        // Compute selection column range in buffer coordinates, then adjust
        // to segment-local coordinates for wrapped lines.
        let (buf_col_start, buf_col_end) = match sel.kind {
            SelectionKind::Line => (0usize, usize::MAX),
            SelectionKind::Char => {
                let cs = if buffer_line == sel.start_line {
                    sel.start_col
                } else {
                    0
                };
                let ce = if buffer_line == sel.end_line {
                    sel.end_col + 1
                } else {
                    usize::MAX
                };
                (cs, ce)
            }
            SelectionKind::Block => (sel.start_col, sel.end_col + 1),
        };

        let char_count = line.raw_text.chars().filter(|&c| c != '\n').count().max(1);
        let seg_end = seg_offset + char_count;

        // Skip segments that don't overlap the selection column range.
        if buf_col_start >= seg_end && buf_col_end != usize::MAX {
            continue;
        }
        if buf_col_end <= seg_offset {
            continue;
        }

        // Convert buffer columns to segment-local columns.
        let col_start = buf_col_start.saturating_sub(seg_offset);
        let col_end = if buf_col_end == usize::MAX {
            usize::MAX
        } else {
            buf_col_end.saturating_sub(seg_offset)
        };
        let effective_end = col_end.min(char_count);

        // Convert char-index column range to visual columns accounting for tabs.
        let vis_start = char_col_to_visual(&line.raw_text, col_start, window.tabstop);
        let vis_end = char_col_to_visual(&line.raw_text, effective_end, window.tabstop);

        for vis in vis_start..vis_end {
            if vis < window.scroll_left {
                continue;
            }
            let screen_col = (vis - window.scroll_left) as u16;
            if screen_col >= text_width as u16 {
                break;
            }
            let sx = text_area_x + screen_col;
            let buf_area = buf.area;
            if sx < buf_area.x + buf_area.width && screen_y < buf_area.y + buf_area.height {
                let cell = &mut buf[(sx, screen_y)];
                let old_fg = cell.fg;
                cell.set_bg(sel_bg);
                // Keep text visible against selection background
                if old_fg == window_bg {
                    cell.set_fg(default_fg);
                }
            }
        }
    }
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

pub(super) fn render_menu_dropdown(
    buf: &mut ratatui::buffer::Buffer,
    full_area: Rect,
    data: &render::MenuBarData,
    theme: &Theme,
) {
    let Some(midx) = data.open_menu_idx else {
        return;
    };
    if data.open_items.is_empty() {
        return;
    }

    let popup_bg = rc(theme.tab_bar_bg);
    let popup_fg = rc(theme.foreground);
    let sep_fg = rc(theme.line_number_fg);
    let shortcut_fg = rc(theme.line_number_fg);

    let total_rows = data.open_items.len() as u16 + 2; // border top/bottom
    let max_label = data
        .open_items
        .iter()
        .map(|i| i.label.len())
        .max()
        .unwrap_or(4);
    let max_shortcut = data
        .open_items
        .iter()
        .map(|i| {
            if data.is_vscode_mode && !i.vscode_shortcut.is_empty() {
                i.vscode_shortcut.len()
            } else {
                i.shortcut.len()
            }
        })
        .max()
        .unwrap_or(0);
    let popup_width = (max_label + max_shortcut + 6).clamp(20, 50) as u16;
    let anchor_col = data.open_menu_col + full_area.x;
    let popup_x = anchor_col.min(full_area.x + full_area.width.saturating_sub(popup_width));
    // Dropdown appears just below the menu bar row (y=1)
    let popup_y = full_area.y + 1;
    let popup_height = total_rows.min(full_area.height.saturating_sub(popup_y));

    // Draw border + background
    for dy in 0..popup_height {
        for dx in 0..popup_width {
            let x = popup_x + dx;
            let y = popup_y + dy;
            if x >= full_area.x + full_area.width || y >= full_area.y + full_area.height {
                continue;
            }
            let ch = if dy == 0 {
                if dx == 0 {
                    '\u{250c}'
                } else if dx == popup_width - 1 {
                    '\u{2510}'
                } else {
                    '\u{2500}'
                }
            } else if dy == popup_height - 1 {
                if dx == 0 {
                    '\u{2514}'
                } else if dx == popup_width - 1 {
                    '\u{2518}'
                } else {
                    '\u{2500}'
                }
            } else if dx == 0 || dx == popup_width - 1 {
                '\u{2502}'
            } else {
                ' '
            };
            set_cell(buf, x, y, ch, popup_fg, popup_bg);
        }
    }

    // Draw items
    let mut row: u16 = popup_y + 1;
    for (item_idx, item) in data.open_items.iter().enumerate() {
        if row >= popup_y + popup_height - 1 {
            break;
        }
        let is_highlighted = data.highlighted_item_idx == Some(item_idx);
        let (item_fg, item_bg) = if is_highlighted {
            (popup_bg, popup_fg) // invert for highlighted row
        } else {
            (popup_fg, popup_bg)
        };
        if item.separator {
            // Separator line (never highlighted)
            for dx in 1..popup_width - 1 {
                let x = popup_x + dx;
                if x < full_area.x + full_area.width {
                    set_cell(buf, x, row, '\u{2500}', sep_fg, popup_bg);
                }
            }
        } else {
            // Fill highlighted row background
            if is_highlighted {
                for dx in 1..popup_width - 1 {
                    let x = popup_x + dx;
                    if x < full_area.x + full_area.width {
                        set_cell(buf, x, row, ' ', item_fg, item_bg);
                    }
                }
            }
            // Label
            let label_x = popup_x + 2;
            for (i, ch) in item.label.chars().enumerate() {
                let x = label_x + i as u16;
                if x >= popup_x + popup_width - 1 {
                    break;
                }
                set_cell(buf, x, row, ch, item_fg, item_bg);
            }
            // Right-aligned shortcut (use VSCode variant when in VSCode mode)
            let sc = if data.is_vscode_mode && !item.vscode_shortcut.is_empty() {
                item.vscode_shortcut
            } else {
                item.shortcut
            };
            if !sc.is_empty() {
                let sc_fg = if is_highlighted { item_fg } else { shortcut_fg };
                let sc_len = sc.len() as u16;
                let sc_x = popup_x + popup_width - 1 - sc_len - 1;
                for (i, ch) in sc.chars().enumerate() {
                    let x = sc_x + i as u16;
                    if x < full_area.x + full_area.width {
                        set_cell(buf, x, row, ch, sc_fg, item_bg);
                    }
                }
            }
        }
        row += 1;
    }
    let _ = midx; // suppress unused warning
}

// ─── Context menu popup rendering ───────────────────────────────────────────────────────

pub(super) fn render_context_menu(
    buf: &mut ratatui::buffer::Buffer,
    full_area: Rect,
    data: &render::ContextMenuPanel,
    theme: &Theme,
) {
    if data.items.is_empty() {
        return;
    }

    let popup_bg = rc(theme.tab_bar_bg);
    let popup_fg = rc(theme.foreground);
    let sep_fg = rc(theme.line_number_fg);
    let shortcut_fg = rc(theme.line_number_fg);
    let disabled_fg = rc(theme.line_number_fg);

    // Count visual rows: items + separator lines after items that have separator_after
    let separator_count = data.items.iter().filter(|i| i.separator_after).count() as u16;
    let total_rows = data.items.len() as u16 + separator_count + 2; // +2 for borders

    let max_label = data.items.iter().map(|i| i.label.len()).max().unwrap_or(4);
    let max_shortcut = data
        .items
        .iter()
        .map(|i| i.shortcut.len())
        .max()
        .unwrap_or(0);
    let popup_width = (max_label + max_shortcut + 6).clamp(20, 50) as u16;

    // Clamp position to stay within terminal
    let popup_x = data
        .screen_col
        .min(full_area.x + full_area.width.saturating_sub(popup_width));
    let popup_y = data
        .screen_row
        .min(full_area.y + full_area.height.saturating_sub(total_rows));
    let popup_height = total_rows.min(full_area.y + full_area.height - popup_y);

    // Draw border + background
    for dy in 0..popup_height {
        for dx in 0..popup_width {
            let x = popup_x + dx;
            let y = popup_y + dy;
            if x >= full_area.x + full_area.width || y >= full_area.y + full_area.height {
                continue;
            }
            let ch = if dy == 0 {
                if dx == 0 {
                    '\u{250c}'
                } else if dx == popup_width - 1 {
                    '\u{2510}'
                } else {
                    '\u{2500}'
                }
            } else if dy == popup_height - 1 {
                if dx == 0 {
                    '\u{2514}'
                } else if dx == popup_width - 1 {
                    '\u{2518}'
                } else {
                    '\u{2500}'
                }
            } else if dx == 0 || dx == popup_width - 1 {
                '\u{2502}'
            } else {
                ' '
            };
            set_cell(buf, x, y, ch, popup_fg, popup_bg);
        }
    }

    // Draw items
    let mut row: u16 = popup_y + 1;
    for (item_idx, item) in data.items.iter().enumerate() {
        if row >= popup_y + popup_height - 1 {
            break;
        }
        let is_selected = item_idx == data.selected_idx;
        let (item_fg, item_bg) = if is_selected && item.enabled {
            (popup_bg, popup_fg) // invert for selected row
        } else if !item.enabled {
            (disabled_fg, popup_bg)
        } else {
            (popup_fg, popup_bg)
        };

        // Fill row background
        if is_selected && item.enabled {
            for dx in 1..popup_width - 1 {
                let x = popup_x + dx;
                if x < full_area.x + full_area.width {
                    set_cell(buf, x, row, ' ', item_fg, item_bg);
                }
            }
        }
        // Label
        let label_x = popup_x + 2;
        for (i, ch) in item.label.chars().enumerate() {
            let x = label_x + i as u16;
            if x >= popup_x + popup_width - 1 {
                break;
            }
            set_cell(buf, x, row, ch, item_fg, item_bg);
        }
        // Right-aligned shortcut
        if !item.shortcut.is_empty() {
            let sc_fg = if is_selected && item.enabled {
                item_fg
            } else {
                shortcut_fg
            };
            let sc_len = item.shortcut.len() as u16;
            let sc_x = popup_x + popup_width - 1 - sc_len - 1;
            for (i, ch) in item.shortcut.chars().enumerate() {
                let x = sc_x + i as u16;
                if x < full_area.x + full_area.width {
                    set_cell(buf, x, row, ch, sc_fg, item_bg);
                }
            }
        }
        row += 1;

        // Draw separator after this item if needed
        if item.separator_after && row < popup_y + popup_height - 1 {
            for dx in 1..popup_width - 1 {
                let x = popup_x + dx;
                if x < full_area.x + full_area.width {
                    set_cell(buf, x, row, '\u{2500}', sep_fg, popup_bg);
                }
            }
            row += 1;
        }
    }
}

// ─── Debug toolbar rendering ────────────────────────────────────────────────────────────

pub(super) fn render_debug_toolbar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    toolbar: &render::DebugToolbarData,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let bar_bg = rc(theme.status_bg);
    let bar_fg = rc(theme.status_fg);
    let dim_fg = rc(theme.line_number_fg);
    let y = area.y;

    // Fill background
    for x in area.x..area.x + area.width {
        set_cell(buf, x, y, ' ', bar_fg, bar_bg);
    }

    let mut col = area.x + 1;
    for (idx, btn) in toolbar.buttons.iter().enumerate() {
        // Separator between index 3 and 4
        if idx == 4 {
            if col < area.x + area.width {
                set_cell(buf, col, y, '\u{2502}', dim_fg, bar_bg);
                col += 1;
            }
            if col < area.x + area.width {
                set_cell(buf, col, y, ' ', bar_fg, bar_bg);
                col += 1;
            }
        }
        let fg = if toolbar.session_active {
            bar_fg
        } else {
            dim_fg
        };
        // Icon
        for ch in btn.icon.chars() {
            if col >= area.x + area.width {
                break;
            }
            set_cell(buf, col, y, ch, fg, bar_bg);
            col += 1;
        }
        // Key hint in parens
        if col < area.x + area.width {
            set_cell(buf, col, y, '(', dim_fg, bar_bg);
            col += 1;
        }
        for ch in btn.key_hint.chars() {
            if col >= area.x + area.width {
                break;
            }
            set_cell(buf, col, y, ch, dim_fg, bar_bg);
            col += 1;
        }
        if col < area.x + area.width {
            set_cell(buf, col, y, ')', dim_fg, bar_bg);
            col += 1;
        }
        // Space separator
        if col < area.x + area.width {
            set_cell(buf, col, y, ' ', bar_fg, bar_bg);
            col += 1;
        }
    }
}

// ─── Find/replace overlay ────────────────────────────────────────────────────

pub(super) fn render_find_replace_popup(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    panel: &render::FindReplacePanel,
    theme: &Theme,
    editor_left: u16,
) {
    use super::set_cell;

    let bg = rc(theme.fuzzy_bg);
    let fg = rc(theme.fuzzy_fg);
    let border_fg = rc(theme.fuzzy_border);
    let accent_bg = rc(theme.tab_active_accent);

    // Dimensions
    let panel_w: u16 = 50.min(area.width.saturating_sub(2));
    let row_count: u16 = if panel.show_replace { 2 } else { 1 };
    let panel_h: u16 = row_count + 2; // +2 for top/bottom borders

    // Position: top-right of active editor group.
    // group_bounds is in row/col units (content-relative), offset by editor_left.
    let gb = &panel.group_bounds;
    let gb_right = editor_left + gb.x as u16 + gb.width as u16;
    let x = gb_right.saturating_sub(panel_w + 1).max(editor_left);
    let y = (gb.y as u16).max(1);

    // Clear background
    for row in y..y + panel_h {
        for col in x..x + panel_w {
            if col < area.width && row < area.height {
                set_cell(buf, col, row, ' ', fg, bg);
            }
        }
    }

    // Top border
    for col in x..x + panel_w {
        set_cell(buf, col, y, '─', border_fg, bg);
    }
    set_cell(buf, x, y, '┌', border_fg, bg);
    if x + panel_w > 0 {
        set_cell(buf, x + panel_w - 1, y, '┐', border_fg, bg);
    }

    // Bottom border
    let bot = y + panel_h - 1;
    for col in x..x + panel_w {
        set_cell(buf, col, bot, '─', border_fg, bg);
    }
    set_cell(buf, x, bot, '└', border_fg, bg);
    if x + panel_w > 0 {
        set_cell(buf, x + panel_w - 1, bot, '┘', border_fg, bg);
    }

    // Side borders
    for row in y + 1..bot {
        set_cell(buf, x, row, '│', border_fg, bg);
        if x + panel_w > 0 {
            set_cell(buf, x + panel_w - 1, row, '│', border_fg, bg);
        }
    }

    // --- Find row: [▶] [query...] [Aa][ab][.*] [N of M] [↑][↓][≡][×] ---
    let find_y = y + 1;
    let content_x = x + 1;
    let right_edge = x + panel_w - 1;

    // Chevron
    let chevron = if panel.show_replace { '▼' } else { '▶' };
    set_cell(buf, content_x, find_y, chevron, fg, bg);

    // Find input (after chevron)
    let input_start = content_x + 2;
    // Reserve space for right-side buttons: toggles(9) + count(dynamic) + gap + nav(8)
    let info_len = (panel.match_info.len() as u16).max(5);
    let right_side_w: u16 = 9 + info_len + 1 + 8;
    let input_w = panel_w.saturating_sub(2 + 2 + right_side_w);
    for (i, ch) in panel.query.chars().enumerate() {
        let cx = input_start + i as u16;
        if cx < input_start + input_w && cx < right_edge {
            set_cell(buf, cx, find_y, ch, fg, bg);
        }
    }
    if panel.focus == 0 {
        // Selection highlight
        if let Some(anchor) = panel.sel_anchor {
            let s = anchor.min(panel.cursor) as u16;
            let e = anchor.max(panel.cursor) as u16;
            let sel_bg = rc(theme.selection);
            for i in s..e {
                let cx = input_start + i;
                if cx < input_start + input_w && cx < right_edge {
                    let ch = panel.query.chars().nth(i as usize).unwrap_or(' ');
                    set_cell(buf, cx, find_y, ch, fg, sel_bg);
                }
            }
        }
        // Cursor
        let cursor_col = input_start + panel.cursor as u16;
        if cursor_col < input_start + input_w && cursor_col < right_edge {
            let ch = panel.query.chars().nth(panel.cursor).unwrap_or(' ');
            set_cell(buf, cursor_col, find_y, ch, bg, fg);
        }
    }

    // Toggle buttons: [Aa] [ab] [.*]
    let mut tx = input_start + input_w + 1;
    for (label, active) in [
        ("Aa", panel.case_sensitive),
        ("ab", panel.whole_word),
        (".*", panel.use_regex),
    ] {
        let (t_fg, t_bg) = if active { (bg, accent_bg) } else { (fg, bg) };
        for ch in label.chars() {
            if tx < right_edge {
                set_cell(buf, tx, find_y, ch, t_fg, t_bg);
                tx += 1;
            }
        }
        tx += 1;
    }

    // Match count
    let info = &panel.match_info;
    for (i, ch) in info.chars().enumerate() {
        let cx = tx + i as u16;
        if cx < right_edge {
            set_cell(buf, cx, find_y, ch, fg, bg);
        }
    }
    tx += (info.len() as u16).max(5) + 1;

    // Nav buttons: ↑ ↓ ≡ ×
    let nav_items = [
        ('↑', false),
        ('↓', false),
        ('≡', panel.in_selection),
        ('×', false),
    ];
    for (ch, active) in nav_items {
        if tx < right_edge {
            let (n_fg, n_bg) = if active { (bg, accent_bg) } else { (fg, bg) };
            set_cell(buf, tx, find_y, ch, n_fg, n_bg);
            tx += 2;
        }
    }

    // --- Replace row: [  ] [replacement...] [AB] [⇄] [⇉] ---
    if panel.show_replace && row_count >= 2 {
        let rep_y = find_y + 1;

        // Replacement text (aligned with find input)
        for (i, ch) in panel.replacement.chars().enumerate() {
            let cx = input_start + i as u16;
            if cx < input_start + input_w && cx < right_edge {
                set_cell(buf, cx, rep_y, ch, fg, bg);
            }
        }
        if panel.focus == 1 {
            // Selection highlight
            if let Some(anchor) = panel.sel_anchor {
                let s = anchor.min(panel.cursor) as u16;
                let e = anchor.max(panel.cursor) as u16;
                let sel_bg = rc(theme.selection);
                for i in s..e {
                    let cx = input_start + i;
                    if cx < input_start + input_w && cx < right_edge {
                        let ch = panel.replacement.chars().nth(i as usize).unwrap_or(' ');
                        set_cell(buf, cx, rep_y, ch, fg, sel_bg);
                    }
                }
            }
            // Cursor
            let cursor_col = input_start + panel.cursor as u16;
            if cursor_col < input_start + input_w && cursor_col < right_edge {
                let ch = panel.replacement.chars().nth(panel.cursor).unwrap_or(' ');
                set_cell(buf, cursor_col, rep_y, ch, bg, fg);
            }
        }

        // AB (preserve case toggle)
        let mut bx = input_start + input_w + 1;
        let (ab_fg, ab_bg) = if panel.preserve_case {
            (bg, accent_bg)
        } else {
            (fg, bg)
        };
        for ch in "AB".chars() {
            if bx < right_edge {
                set_cell(buf, bx, rep_y, ch, ab_fg, ab_bg);
                bx += 1;
            }
        }
        bx += 1;

        // Replace current / Replace all (icon or fallback)
        let sel_bg = rc(theme.fuzzy_selected_bg);
        for label in [
            crate::icons::FIND_REPLACE.s(),
            crate::icons::FIND_REPLACE_ALL.s(),
        ] {
            for ch in label.chars() {
                if bx < right_edge {
                    set_cell(buf, bx, rep_y, ch, fg, sel_bg);
                    bx += 1;
                }
            }
            bx += 1;
        }
    }
}

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
        let mut tab_visible_counts: Vec<(GroupId, usize)> = Vec::new();

        terminal
            .draw(|frame| {
                draw_frame(
                    frame,
                    &screen,
                    &theme,
                    &mut sidebar,
                    engine,
                    sidebar_width,
                    0,    // quickfix_scroll_top
                    0,    // debug_output_scroll
                    None, // folder_picker
                    false,
                    false,
                    None, // cmd_sel
                    None, // explorer_drop_target
                    &mut hover_link_rects,
                    &mut hover_popup_rect,
                    &mut editor_hover_popup_rect,
                    &mut editor_hover_link_rects,
                    &mut tab_visible_counts,
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
