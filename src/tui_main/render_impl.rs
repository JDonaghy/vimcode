use super::*;

// ─── Screen layout bridging ───────────────────────────────────────────────────

pub(super) fn build_screen_for_tui(
    engine: &Engine,
    theme: &Theme,
    area: Rect,
    _sidebar: &TuiSidebar,
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
    let sv = engine.app_shell.sidebar_visible();
    let sidebar_cols = if sv { sidebar_width + 1 } else { 0 }; // +1 sep
    let ab_width = if engine.settings.autohide_panels && !sv {
        0
    } else {
        ACTIVITY_BAR_WIDTH
    };
    let content_cols = area.width.saturating_sub(ab_width + sidebar_cols);
    // #550: window rects are absolute terminal-screen coordinates, matching
    // GTK's convention, rather than relative to the editor content area's own
    // top-left. `editor_area`'s origin here must match the `Layout` split
    // `draw_frame` performs on the same `area` (menu bar row, then activity
    // bar + sidebar columns) — see the mirrored computation there.
    let editor_origin_x = area.x as f64 + ab_width as f64 + sidebar_cols as f64;
    let editor_origin_y = area.y as f64 + menu_height as f64;
    let content_bounds = WindowRect::new(
        editor_origin_x,
        editor_origin_y,
        content_cols as f64,
        content_rows as f64,
    );
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
    folder_picker: Option<&FolderPickerState>,
    cmd_sel: Option<(usize, usize)>,
    explorer_drop_target: Option<usize>,
    hover_link_rects_out: &mut Vec<(u16, u16, u16, u16, String)>,
    hover_popup_rect_out: &mut Option<(u16, u16, u16, u16)>,
    editor_hover_popup_rect_out: &mut Option<(u16, u16, u16, u16)>,
    editor_hover_link_rects_out: &mut Vec<(u16, u16, u16, u16, String)>,
    editor_hover_scrollbar_out: &mut Option<render::PopupScrollbarHit>,
    tab_visible_counts_out: &mut Vec<(GroupId, usize)>,
    debug_toolbar_rect_out: &mut quadraui::Rect,
    completion_layout_out: &mut Option<quadraui::CompletionsLayout>,
    context_menu_layout_out: &mut Option<quadraui::ContextMenuLayout>,
    dialog_layout_out: &mut Option<quadraui::DialogLayout>,
    // Phase B.4 Stage 2: backend handle for migrated `draw_*` calls.
    // Set once per frame by the caller (cached theme); the migrated
    // call sites wrap their access in `backend.enter_frame_scope`.
    backend: &mut super::backend::TuiBackend,
    tab_drag_source: Option<(crate::core::window::GroupId, usize)>,
    tab_drag_cursor: Option<(f64, f64)>,
    tab_drop_zone: &crate::core::window::DropZone,
) {
    let area = frame.area();

    engine.scroll_surfaces.borrow_mut().clear();

    // ── Top-level: [menu] / [content_area] ──
    let menu_bar_height: u16 = if screen.menu_bar_visible { 1 } else { 0 };
    let top_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(menu_bar_height), Constraint::Min(0)])
        .split(area);
    let menu_bar_area = top_chunks[0];
    let content_area = top_chunks[1];

    // ── Horizontal split: [activity_bar] [sidebar?] [editor_col] ─
    // Activity bar and sidebar span full height (like GTK layout).
    let sv2 = engine.app_shell.sidebar_visible();
    let ab_width = if engine.settings.autohide_panels && !sv2 {
        0
    } else {
        ACTIVITY_BAR_WIDTH
    };
    let sidebar_constraint = if sv2 {
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
    if screen.menu_bar_visible {
        let q_theme = super::quadraui_tui::q_theme(theme);
        let bar = engine.menu_system.borrow().menu_bar();
        let bar_rect = quadraui::Rect::new(
            menu_bar_area.x as f32,
            menu_bar_area.y as f32,
            menu_bar_area.width as f32,
            menu_bar_area.height as f32,
        );
        let mb_layout = backend.enter_frame_scope(frame, |b| {
            use quadraui::Backend;
            b.draw_menu_bar(bar_rect, &bar)
        });

        let menu_end: u16 = mb_layout
            .visible_items
            .last()
            .map(|vi| menu_bar_area.x + (vi.bounds.x + vi.bounds.width).round() as u16)
            .unwrap_or(menu_bar_area.x);

        let title = engine
            .cwd
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "VimCode".to_string());
        let cc = render::build_command_center_view(
            engine.tab_nav_can_go_back(),
            engine.tab_nav_can_go_forward(),
            &title,
        );
        let cc_area = Rect {
            x: menu_end,
            y: menu_bar_area.y,
            width: menu_bar_area
                .width
                .saturating_sub(menu_end - menu_bar_area.x),
            height: menu_bar_area.height,
        };
        let cc_layout =
            quadraui::tui::draw_command_center(frame.buffer_mut(), cc_area, &cc, &q_theme);
        engine.command_center_layout.replace(Some(cc_layout));
        // Note: dropdown is rendered LAST (after all content) so it draws on top.
    } else {
        engine.command_center_layout.replace(None);
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
    if engine.app_shell.sidebar_visible() && sidebar_sep_area.width > 1 {
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
    // Skip-condition + rect math (including the zero-width fallback filter
    // and the reserved-height subtraction that recovers the tab row's own
    // top edge from `GroupTabBar::bounds.y`) come from
    // `render::tab_bar_draw_targets`, shared with GTK, so the two backends
    // can't drift apart (#549, follow-up to #547's `breadcrumb_draw_targets`).
    // Each arm below still does its own draw call + `tab_visible_counts_out`
    // bookkeeping, and picks where in the window-render order to draw (split
    // draws windows first so tab bars paint on top at group boundaries;
    // single draws the tab bar first then windows below it).
    let tui_tbh: f64 = if engine.settings.breadcrumbs && !engine.terminal_maximized {
        2.0
    } else {
        1.0
    };
    let tab_bar_targets = render::tab_bar_draw_targets(
        engine,
        screen,
        1.0,
        tui_tbh,
        (
            editor_area.x as f64,
            editor_area.y as f64,
            editor_area.width as f64,
        ),
    );
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
        render_all_windows(backend, frame, &screen.windows, theme);
        // Draw each group's tab bar.  Tab bar sits tab_bar_height rows above
        // the group's window content (bounds.y - tab_bar_height).
        for target in &tab_bar_targets {
            let g_tab = Rect {
                x: target.rect.x as u16,
                y: target.rect.y as u16,
                width: target.rect.width as u16,
                height: 1,
            };
            let vis = render_tab_bar(backend, frame, g_tab, target.bar, theme);
            tab_visible_counts_out.push((target.group_id, vis));
        }
        // Draw breadcrumb bars (below each group's tab bar). Hidden while the
        // terminal panel is maximized so it can claim the row. Skip
        // conditions + rect math (including the zero-width fallback filter)
        // come from `render::breadcrumb_draw_targets`, shared with GTK, so
        // the two backends can't drift apart (#547).
        for t in render::breadcrumb_draw_targets(screen, engine.terminal_maximized, 1.0) {
            let bc_rect = Rect {
                x: t.rect.x as u16,
                y: t.rect.y as u16,
                width: t.rect.width as u16,
                height: 1,
            };
            let layout = draw_breadcrumb_bar(backend, frame, bc_rect, t.bar, theme);
            *t.draw_layout.borrow_mut() = Some(layout);
        }
        // Draw divider lines (vertical only — horizontal splits use the tab bar as divider).
        // `div.position`/`.cross_start` are already absolute terminal-screen
        // coordinates (#550), matching `editor_area`'s own coordinate space —
        // no offset addition needed.
        let sep_fg = rc(theme.separator);
        let sep_bg = rc(theme.background);
        for div in &split.dividers {
            if div.direction == SplitDirection::Vertical {
                let div_x = div.position as u16;
                let y_start = div.cross_start as u16;
                let y_end = y_start + div.cross_size as u16;
                for y in y_start..y_end {
                    if div_x < editor_area.x + editor_area.width {
                        // #481: the window immediately to the left already
                        // renders its own separator in the column right before
                        // the divider (`div_x - 1`) — either its vertical
                        // scrollbar (via `quadraui::tui::draw_editor`, glyphs
                        // `█`/`░`) when it overflows, or a plain divider line
                        // (`│`, painted by `render_separators`) when it does
                        // not. Either way that column already visually
                        // separates the two groups, so painting a second
                        // divider glyph beside it produces a phantom
                        // "duplicate scrollbar"/double-line bar in multi-tab-
                        // group layouts. Skip the divider on rows where such a
                        // separator already occupies `div_x - 1`.
                        if div_x > editor_area.x {
                            let left = frame.buffer_mut()[(div_x - 1, y)].symbol();
                            if left == "█" || left == "░" || left == "│" {
                                continue;
                            }
                        }
                        set_cell(frame.buffer_mut(), div_x, y, '│', sep_fg, sep_bg);
                    }
                }
            }
        }
    } else {
        // Single group: tab bar at row 0 of editor_area, windows at row 1+.
        for target in &tab_bar_targets {
            let tab_rect = Rect {
                x: target.rect.x as u16,
                y: target.rect.y as u16,
                width: target.rect.width as u16,
                height: 1,
            };
            let vis = render_tab_bar(backend, frame, tab_rect, target.bar, theme);
            tab_visible_counts_out.push((target.group_id, vis));
        }
        // Draw breadcrumb bar for the single group. Hidden while the terminal
        // panel is maximized. `bc.bounds.y` (via `breadcrumb_draw_targets`)
        // already accounts for a hidden tab bar — `calculate_group_window_rects`
        // → `adjust_group_rects_for_hidden_tabs` shifts the window rect (and
        // therefore the derived breadcrumb bounds) up by one row in that case,
        // so this no longer needs its own `is_tab_bar_hidden` special case (#547).
        for t in render::breadcrumb_draw_targets(screen, engine.terminal_maximized, 1.0) {
            let bc_rect = Rect {
                x: t.rect.x as u16,
                y: t.rect.y as u16,
                width: t.rect.width as u16,
                height: 1,
            };
            let layout = draw_breadcrumb_bar(backend, frame, bc_rect, t.bar, theme);
            *t.draw_layout.borrow_mut() = Some(layout);
        }
        render_all_windows(backend, frame, &screen.windows, theme);
    }

    // Register the editor viewport as a scroll surface so dispatch_scroll
    // routes scroll wheel events to it (per-window routing done in handler).
    engine
        .scroll_surfaces
        .borrow_mut()
        .push(quadraui::ScrollSurface {
            id: quadraui::WidgetId::new("tui:editor_viewport"),
            bounds: quadraui::Rect::new(
                editor_area.x as f32,
                editor_area.y as f32,
                editor_area.width as f32,
                editor_area.height as f32,
            ),
            scrollbar: None,
        });

    // ── Tab drag overlay ────────────────────────────────────────────────────
    if tab_drag_source.is_some() {
        render_tab_drag_overlay(
            frame,
            engine,
            editor_area,
            screen,
            theme,
            tab_drag_source,
            tab_drag_cursor,
            tab_drop_zone,
        );
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
                let win_x = active_win.rect.x as u16;
                let win_y = active_win.rect.y as u16;
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
                *completion_layout_out = Some(layout);
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
            let win_x = active_win.rect.x as u16;
            let win_y = active_win.rect.y as u16;
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
            let win_x = active_win.rect.x as u16;
            let win_y = active_win.rect.y as u16;
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
            let win_x = active_win.rect.x as u16;
            let win_y = active_win.rect.y as u16;
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
            let win_x = active_win.rect.x as u16;
            let win_y = active_win.rect.y as u16;
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
        engine
            .bottom_panel_geometry
            .replace(Some(crate::core::engine::BottomPanelGeometry {
                top_y: bottom_panel_area.y as f64,
                height: bottom_panel_area.height as f64,
                toolbar_y: 1.0,
                content_y: 2.0,
                content_row_h: 1.0,
            }));
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
        let hits = render_bottom_panel_tabs(
            backend,
            frame,
            tab_bar_area,
            &engine.bottom_panel_kind,
            engine.terminal_open,
            !screen.bottom_tabs.output_lines.is_empty(),
            theme,
        );
        engine.bottom_tab_bar_hits.replace(Some(hits));
        match engine.bottom_panel_kind {
            render::BottomPanelKind::Terminal => {
                if let Some(ref term) = screen.bottom_tabs.terminal {
                    let toolbar_area = Rect {
                        x: content_area.x,
                        y: content_area.y,
                        width: content_area.width,
                        height: 1,
                    };
                    let hits = render_terminal_toolbar(backend, frame, toolbar_area, term, theme);
                    engine.terminal_toolbar_hits.replace(Some(hits));
                    let term_content = Rect {
                        x: content_area.x,
                        y: content_area.y + 1,
                        width: content_area.width,
                        height: content_area.height.saturating_sub(1),
                    };
                    render_terminal_panel(frame, backend, term_content, term, theme, engine);
                    // Register terminal content area as a scroll surface.
                    engine
                        .scroll_surfaces
                        .borrow_mut()
                        .push(quadraui::ScrollSurface {
                            id: quadraui::WidgetId::new("terminal_scrollback"),
                            bounds: quadraui::Rect::new(
                                term_content.x as f32,
                                term_content.y as f32,
                                term_content.width as f32,
                                term_content.height as f32,
                            ),
                            scrollbar: None,
                        });
                }
            }
            render::BottomPanelKind::DebugOutput => {
                let td = render::debug_output_to_text_display(
                    &screen.bottom_tabs.output_lines,
                    engine.debug_output_scroll,
                    engine.debug_output_auto_scroll,
                );
                let q_rect = quadraui::Rect::new(
                    content_area.x as f32,
                    content_area.y as f32,
                    content_area.width as f32,
                    content_area.height as f32,
                );
                backend.set_current_theme(super::quadraui_tui::q_theme(theme));
                let td_layout = {
                    use quadraui::Backend;
                    backend.text_display_layout(q_rect, &td)
                };
                backend.enter_frame_scope(frame, |b| {
                    use quadraui::Backend;
                    b.draw_text_display(q_rect, &td);
                });
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

    // ── Debug toolbar strip (if visible) ────────────────────────────────────
    if screen.debug_toolbar.is_some() {
        // Route through `Backend::draw_toolbar` (#510). Layout is cached on
        // `engine.debug_toolbar_layout` for click/hover hit-testing.
        let q_rect = quadraui::Rect::new(
            debug_toolbar_area.x as f32,
            debug_toolbar_area.y as f32,
            debug_toolbar_area.width as f32,
            debug_toolbar_area.height as f32,
        );
        *debug_toolbar_rect_out = q_rect;
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        backend.enter_frame_scope(frame, |b| {
            render::draw_debug_toolbar(b, engine, q_rect);
        });
    }

    // ── Wildmenu bar (command Tab completion) ─────────────────────────────────
    if let Some(ref wm) = screen.wildmenu {
        let bar = render::wildmenu_to_status_bar(wm, theme);
        let q_rect = quadraui::Rect::new(
            wildmenu_area.x as f32,
            wildmenu_area.y as f32,
            wildmenu_area.width as f32,
            wildmenu_area.height as f32,
        );
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        backend.enter_frame_scope(frame, |b| {
            use quadraui::Backend;
            b.draw_status_bar(q_rect, &bar, None, None);
        });
    }

    // ── Status / command ──────────────────────────────────────────────────────
    if let Some(ref bar) = screen.global_status_bar {
        let q_rect = quadraui::Rect::new(
            status_area.x as f32,
            status_area.y as f32,
            status_area.width as f32,
            status_area.height as f32,
        );
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        backend.enter_frame_scope(frame, |b| {
            use quadraui::Backend;
            b.draw_status_bar(q_rect, bar, None, None);
        });
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
    if engine.app_shell.sidebar_visible() && sidebar_sep_area.width > 1 {
        let sep_x = sidebar_sep_area.x + sidebar_sep_area.width - 1;
        if sidebar.ext_panel_name.is_some() || engine.active_panel_is(PANEL_GIT) {
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
        // #550: `find_replace.group_bounds` is derived from `window_rects`
        // (render.rs's `active_group_bounds`) and is now absolute
        // terminal-screen space, not content-relative. quadraui's shared
        // `draw_find_replace(..., editor_left)` still expects to translate a
        // content-relative `group_bounds` by `editor_left` internally (it's
        // TUI-only — GTK never calls this path, so there's no established
        // absolute-input convention to lean on there); passing `0` here
        // keeps that internal translation a no-op instead of double-
        // counting the origin now baked into `group_bounds` itself.
        super::quadraui_tui::draw_find_replace(frame.buffer_mut(), area, find_replace, theme);
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
        // The layout describes the INNER items region; the rasteriser draws
        // a 1-cell box border around it, so the anchor/viewport passed to
        // `context_menu_generic_layout` are inset by (1, 1) and shrunk by 2 —
        // otherwise the right/bottom border can extend past the screen on
        // narrow windows. `char_width`/`line_height` are both 1.0 (one
        // screen cell) on TUI; GTK's ShellApp `render_content` passes its
        // real pixel metrics through the same shared function (#546).
        let inner_viewport = quadraui::Rect::new(
            (area.x + 1) as f32,
            (area.y + 1) as f32,
            area.width.saturating_sub(2) as f32,
            area.height.saturating_sub(2) as f32,
        );
        let inset_panel = render::ContextMenuPanel {
            screen_col: ctx_menu.screen_col + 1,
            screen_row: ctx_menu.screen_row + 1,
            ..ctx_menu.clone()
        };
        let (menu, layout) =
            render::context_menu_generic_layout(&inset_panel, inner_viewport, 1.0, 1.0, 1.0);
        super::quadraui_tui::draw_context_menu(frame.buffer_mut(), &menu, &layout, theme);
        *context_menu_layout_out = Some(layout);
    }

    // ── Modal dialog (highest z-order after quit confirm) ────────────────────
    if let Some(ref dialog) = screen.dialog {
        let viewport = quadraui::Rect::new(
            area.x as f32,
            area.y as f32,
            area.width as f32,
            area.height as f32,
        );
        let (q_dialog, layout) = render::dialog_generic_layout(dialog, viewport, 1.0, 1.0);
        super::quadraui_tui::draw_dialog(frame.buffer_mut(), &q_dialog, &layout, theme);
        *dialog_layout_out = Some(layout);
    } else {
        *dialog_layout_out = None;
    }

    // ── Menu dropdown — rendered last so it draws on top of everything ────────
    if screen.menu_bar_visible {
        let bar_rect = quadraui::Rect::new(
            menu_bar_area.x as f32,
            menu_bar_area.y as f32,
            menu_bar_area.width as f32,
            menu_bar_area.height as f32,
        );
        backend.enter_frame_scope(frame, |b| {
            engine.menu_system.borrow().render(b, bar_rect);
        });
    }

    // Toast overlay (#450) — drawn LAST so it sits on top of every other
    // surface. Bottom-right corner; transient (auto-dismissed after
    // TOAST_LIFETIME via `engine.prune_toasts()` from poll_idle). The
    // returned layout is cached on the engine so click handlers can run
    // hit_test → handle_toast_hit (× close, action buttons).
    if let Some(stack) = render::build_toast_stack(engine) {
        let q_theme = super::quadraui_tui::q_theme(theme);
        let toast_area = frame.area();
        let layout =
            quadraui::tui::draw_toast_stack(frame.buffer_mut(), toast_area, &stack, &q_theme);
        engine.toast_layout.replace(Some(layout));
    } else {
        engine.toast_layout.replace(None);
    }
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
                depth: 0,
                expandable: false,
                expanded: false,
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
        show_query: true,
        create_label: None,
        preview: None,
        mode: quadraui::PaletteMode::List,
    }
}

// ─── Tab bar constants ───────────────────────────────────────────────────────

/// Terminal columns used by each tab's close button (the × itself + trailing space).
/// The glyph itself lives in `quadraui::tui::TAB_CLOSE_CHAR` since the public
/// rasteriser owns the painting.
///
/// #477: re-export of `render::TAB_CLOSE_COLS` — was a duplicate literal
/// here, now the shared constant is the single source of truth.
pub(super) use render::TAB_CLOSE_COLS;

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

/// Extract per-tab drag-and-drop slot bounds — `(x_start, x_end)` pairs in
/// absolute screen-column units, ordered by tab index — from a tab bar's
/// hit regions. `base_x` is the absolute left edge the region columns are
/// relative to (bar left edge = column 0).
///
/// #477: hit regions are the single source of truth already used for mouse
/// click routing (`render::resolve_tab_bar_click`); this just re-slices
/// them into the `(f32, f32)` shape the drag overlay / drop-zone geometry
/// expects, instead of hand-rolling `name.chars().count() + TAB_CLOSE_COLS`
/// per tab (which had drifted from the real per-tab close-button width and
/// from an obsolete "+2 for the scroll indicator" adjustment that the
/// quadraui TUI tab bar rasteriser doesn't actually reserve space for).
fn tab_drag_slots_from_hit_regions(
    hit_regions: &[(
        crate::core::engine::TabBarHitRegion,
        crate::core::engine::TabBarClickTarget,
    )],
    base_x: f32,
) -> Vec<(f32, f32)> {
    use crate::core::engine::TabBarClickTarget;
    let mut tabs: Vec<(usize, f32, f32)> = hit_regions
        .iter()
        .filter_map(|(region, target)| match target {
            TabBarClickTarget::Tab(idx) => {
                let start = base_x + region.col as f32;
                Some((*idx, start, start + region.width as f32))
            }
            _ => None,
        })
        .collect();
    tabs.sort_unstable_by_key(|(idx, ..)| *idx);
    tabs.into_iter().map(|(_, s, e)| (s, e)).collect()
}

/// Build the per-group tab-drag slot map consumed by the drag overlay and
/// drop-zone hit testing. Both branches reuse hit regions already cached on
/// `ScreenLayout` by `render::build_screen_layout()` (from
/// `compute_tab_bar_hit_regions()`) instead of recomputing tab positions:
/// multi-group splits use each group's `GroupTabBar::hit_regions`, and the
/// single-group bar uses `ScreenLayout::tab_bar_hit_regions` (#515).
fn build_tui_tab_slots(
    screen: &render::ScreenLayout,
    engine: &Engine,
    editor_x: f32,
) -> std::collections::HashMap<usize, Vec<(f32, f32)>> {
    let mut map = std::collections::HashMap::new();
    if let Some(ref split) = screen.editor_group_split {
        for gtb in &split.group_tab_bars {
            // #550: `gtb.bounds` is already absolute terminal-screen space
            // (same convention as GTK), so no `editor_x` offset addition.
            let abs_x = gtb.bounds.x as f32;
            map.insert(
                gtb.group_id.0,
                tab_drag_slots_from_hit_regions(&gtb.hit_regions, abs_x),
            );
        }
    } else {
        // Single-group bar spans the editor area's own left edge; hit
        // regions are bar-relative offsets, not window-rect-derived, so
        // `editor_x` is still the correct base here.
        map.insert(
            engine.active_group.0,
            tab_drag_slots_from_hit_regions(&screen.tab_bar_hit_regions, editor_x),
        );
    }
    map
}

/// Render the tab drag overlay for the TUI path.
///
/// `tab_drag_source` is the (GroupId, tab_index) captured when the drag started.
/// `tab_drag_cursor` is the current cursor position during the drag.
/// `tab_drop_zone` is the most recently computed drop zone.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_tab_drag_overlay(
    frame: &mut ratatui::Frame,
    engine: &Engine,
    editor_area: Rect,
    screen: &render::ScreenLayout,
    theme: &render::Theme,
    tab_drag_source: Option<(crate::core::window::GroupId, usize)>,
    tab_drag_cursor: Option<(f64, f64)>,
    tab_drop_zone: &crate::core::window::DropZone,
) {
    let tab_slots = build_tui_tab_slots(screen, engine, editor_area.x as f32);
    let tbh_f = if engine.settings.breadcrumbs {
        2.0f32
    } else {
        1.0
    };
    // #550/#515: in split mode `gtb.bounds` is already absolute (built from
    // absolute window rects, matching GTK), so the origin here must be
    // (0,0) — adding `editor_area`'s origin again would double-count it,
    // the same bug GTK's #515 fix addressed for its own call site.
    // Single-group mode returns the origin directly (no `gtb.bounds` to
    // derive it from), so it still needs the real editor origin.
    let drop_origin = if screen.editor_group_split.is_some() {
        (0.0, 0.0)
    } else {
        (editor_area.x as f32, editor_area.y as f32)
    };
    let bounds = render::screen_to_drop_group_bounds(
        screen,
        engine,
        drop_origin,
        (editor_area.width as f32, editor_area.height as f32),
        tbh_f,
    );
    let (groups, tbh) = render::build_tab_drop_groups(&bounds, engine, tbh_f, &tab_slots);
    let cursor = tab_drag_cursor
        .map(|(mx, my)| (mx as f32, my as f32))
        .unwrap_or((0.0, 0.0));
    let overlay =
        match render::compute_tab_drop_overlay(tab_drop_zone, &groups, cursor, tbh, 1.0, 2.0) {
            Some(o) => o,
            None => return,
        };

    {
        let q_theme = super::quadraui_tui::q_theme(theme);
        let q_overlay = quadraui::DropOverlay {
            highlight: overlay.highlight,
            insertion_bar: overlay.insertion_bar,
            ghost_position: Some(overlay.ghost_position),
        };
        quadraui::tui::draw_drop_overlay(frame.buffer_mut(), &q_overlay, &q_theme);
    }

    // Look up the tab label from engine using the captured drag source.
    let drag_label: String = if let Some((src_gid, src_tab_idx)) = tab_drag_source {
        engine
            .editor_groups
            .get(&src_gid)
            .and_then(|g| g.tabs.get(src_tab_idx))
            .and_then(|t| {
                let win = engine.windows.get(&t.active_window)?;
                let state = engine.buffer_manager.get(win.buffer_id)?;
                Some(state.display_name().to_string())
            })
            .unwrap_or_default()
    } else {
        String::new()
    };

    if tab_drag_cursor.is_some() && !drag_label.is_empty() {
        let label = &drag_label;
        if !label.is_empty() {
            let gx = overlay.ghost_position.0 as u16;
            let gy = overlay.ghost_position.1 as u16;
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
    let layout = match last_layout {
        Some(l) => l,
        None => return crate::core::window::DropZone::None,
    };
    if col < editor_left {
        return crate::core::window::DropZone::None;
    }
    let ts = match terminal_size {
        Some(s) => s,
        None => return crate::core::window::DropZone::None,
    };
    let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
    let editor_w = ts.width.saturating_sub(editor_left);
    let editor_h = ts.height.saturating_sub(menu_rows + 2);
    let tab_slots = build_tui_tab_slots(layout, engine, editor_left as f32);
    let tbh_f = if engine.settings.breadcrumbs {
        2.0f32
    } else {
        1.0
    };
    // #550/#515: same double-count hazard as `render_tab_drag_overlay` above —
    // split-mode `gtb.bounds` is already absolute, so the origin must be (0,0).
    let drop_origin = if layout.editor_group_split.is_some() {
        (0.0, 0.0)
    } else {
        (editor_left as f32, menu_rows as f32)
    };
    let bounds = render::screen_to_drop_group_bounds(
        layout,
        engine,
        drop_origin,
        (editor_w as f32, editor_h as f32),
        tbh_f,
    );
    let (groups, tbh) = render::build_tab_drop_groups(&bounds, engine, tbh_f, &tab_slots);
    render::compute_tab_drop_zone(col as f32, row as f32, &groups, tbh)
}

/// Render the tab bar via `Backend::draw_tab_bar`. Returns the
/// **tab-bar content width in cells** — what the engine stores via
/// `set_tab_visible_count` (misnamed; it's the bar width used by
/// `ensure_active_tab_visible` to derive scroll offsets).
///
/// The pre-built `quadraui::TabBar` primitive comes from `ScreenLayout`
/// (built by `render::build_screen_layout`).
pub(super) fn render_tab_bar(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    area: Rect,
    bar: &quadraui::TabBar,
    theme: &Theme,
) -> usize {
    let q_rect = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    let hits = backend.enter_frame_scope(frame, |b| {
        use quadraui::Backend;
        b.draw_tab_bar(q_rect, bar, None)
    });
    hits.available_cols
}

/// Draw the breadcrumb bar via the D6 StatusBar pipeline.
///
/// The pre-built `quadraui::StatusBar` primitive comes from
/// `ScreenLayout` (built by `render::build_screen_layout`).
/// Returns the `StatusBarLayout` for click-time hit testing.
pub(super) fn draw_breadcrumb_bar(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    area: Rect,
    bar: &quadraui::StatusBar,
    theme: &Theme,
) -> quadraui::StatusBarLayout {
    let q_rect = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    backend.enter_frame_scope(frame, |b| {
        use quadraui::Backend;
        b.draw_status_bar(q_rect, bar, None, None)
    })
}

// ─── Editor windows ───────────────────────────────────────────────────────────

pub(super) fn render_all_windows(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    windows: &[RenderedWindow],
    theme: &Theme,
) {
    for window in windows {
        // #550: `window.rect` is already absolute terminal-screen coordinates.
        let win_rect = Rect {
            x: window.rect.x as u16,
            y: window.rect.y as u16,
            width: window.rect.width as u16,
            height: window.rect.height as u16,
        };
        render_window(backend, frame, win_rect, window, theme);
    }
    render_separators(frame.buffer_mut(), windows, theme);
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
    let has_preview = picker.preview.is_some();
    let geo = render::PickerGeometry::compute(
        term_area.width as f32,
        term_area.height as f32,
        has_preview,
        &render::TUI_PICKER_SIZING,
    );
    let palette = render::picker_panel_to_palette(picker);
    let q_rect = quadraui::Rect::new(geo.popup_x, geo.popup_y, geo.popup_w, geo.popup_h);
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    backend.enter_frame_scope(frame, |b| {
        use quadraui::Backend;
        b.draw_palette(q_rect, &palette);
    });
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
        render_window_status_line(
            backend,
            frame,
            editor_area.x,
            sy,
            editor_area.width,
            status,
            theme,
        );
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
        let _ = b.draw_status_bar(q_rect, &bar, None, None);
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
    windows: &[RenderedWindow],
    theme: &Theme,
) {
    if windows.len() <= 1 {
        return;
    }
    let sep_fg = rc(theme.separator);
    let sep_bg = rc(theme.background);

    for i in 0..windows.len() {
        for j in (i + 1)..windows.len() {
            let a = &windows[i];
            let b = &windows[j];

            // Vertical separator: window a is the left pane, b is the right pane.
            // The boundary sits in the last column of a (`sep_x - 1`).
            // Also require vertical overlap — windows from different groups may
            // share an x edge but not overlap in y (e.g. 2×2 grid).
            let v_overlap =
                a.rect.y.max(b.rect.y) < (a.rect.y + a.rect.height).min(b.rect.y + b.rect.height);
            if (a.rect.x + a.rect.width - b.rect.x).abs() < 1.0 && v_overlap {
                // #550: `a.rect`/`b.rect` are already absolute terminal-screen
                // coordinates, so no `editor_area` offset addition needed.
                let sep_x = (a.rect.x + a.rect.width) as u16;
                let y_start = a.rect.y.max(b.rect.y) as u16;
                let y_end = (a.rect.y + a.rect.height).min(b.rect.y + b.rect.height) as u16;

                // #481 (iter4): `quadraui::tui::draw_editor` already paints
                // window `a`'s own vertical scrollbar in this exact column
                // (its last column) whenever it overflows — see
                // `render_window` → `draw_editor`, which runs for every window
                // *before* this pass. Re-drawing a second scrollbar here was
                // pure redundancy AND buggy: this pass computed the track from
                // `a.rect.height` (which includes the per-window status-line
                // row) whereas `draw_editor` reserves that row, so the repaint
                // came out one row taller and bled a stray track glyph onto the
                // status bar — reading as a slightly-longer "duplicate"
                // scrollbar jammed against the real one at tab-group
                // boundaries. Let `draw_editor`'s scrollbar own the column; it
                // doubles as the visual separator. Only when the left window
                // has NO scrollbar do we draw a plain divider line.
                //
                // Match `draw_editor`'s overflow test exactly (it reserves the
                // status-line row from the viewport) so we draw the '│' in
                // precisely the cases where it drew no scrollbar.
                let text_rows = (a.rect.height as usize).saturating_sub(
                    if a.status_line.is_some() && a.rect.height > 1.0 {
                        1
                    } else {
                        0
                    },
                );
                let has_scroll = a.total_lines > text_rows && y_end > y_start;

                if !has_scroll {
                    for dy in 0..y_end.saturating_sub(y_start) {
                        let y = y_start + dy;
                        set_cell(buf, sep_x.saturating_sub(1), y, '│', sep_fg, sep_bg);
                    }
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
                let sep_y = (a.rect.y + a.rect.height) as u16;
                let x_start = a.rect.x.max(b.rect.x) as u16;
                let x_end = (a.rect.x + a.rect.width).min(b.rect.x + b.rect.width) as u16;
                for x in x_start..x_end.max(x_start) {
                    set_cell(buf, x, sep_y.saturating_sub(1), '─', sep_fg, sep_bg);
                }
            }
        }
    }
}

// ─── Activity bar ─────────────────────────────────────────────────────────────

// ─── Menu bar rendering ─────────────────────────────────────────────────────
// (Now handled by MenuSystem::render() — see draw_frame menu dropdown block.)

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
        // #439: Engine::new() reads git::current_branch(cwd), so without
        // this clear the snapshot tests leak whatever branch name the
        // test runner happens to be on into the status bar fixture.
        // Likewise sc_ahead/sc_behind get computed from the surrounding
        // repo state. Reset all three so snapshots are reproducible.
        e.git_branch = None;
        e.sc_ahead = 0;
        e.sc_behind = 0;
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
        let mut sidebar = TuiSidebar::new();
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
        let mut dbg_toolbar_rect = quadraui::Rect::default();
        let mut completion_layout = None;
        let mut context_menu_layout = None;
        let mut dialog_layout = None;
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
                    0,    // quickfix_scroll_top
                    None, // folder_picker
                    None, // cmd_sel
                    None, // explorer_drop_target
                    &mut hover_link_rects,
                    &mut hover_popup_rect,
                    &mut editor_hover_popup_rect,
                    &mut editor_hover_link_rects,
                    &mut editor_hover_scrollbar,
                    &mut tab_visible_counts,
                    &mut dbg_toolbar_rect,
                    &mut completion_layout,
                    &mut context_menu_layout,
                    &mut dialog_layout,
                    &mut backend,
                    None,                                 // tab_drag_source
                    None,                                 // tab_drag_cursor
                    &crate::core::window::DropZone::None, // tab_drop_zone
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

    /// #477 fix iteration 1 regression test: dragging a tab within a single
    /// (unsplit) tab group and dropping it over the tab bar must resolve to
    /// a same-group `TabReorder`, never a `Split`.
    ///
    /// Root cause was in `render::screen_to_drop_group_bounds`'s no-split
    /// branch: it passed the whole-editor origin/size (top-left at the
    /// global tab bar's row, per the "tab bar at row 0 of editor_area"
    /// convention) straight through as `DropGroupBounds` content bounds,
    /// which `build_tab_drop_groups` then shifted *up* by `tab_bar_height`
    /// again to reconstruct the full rect. That double-shift made the
    /// computed tab-bar band sit one row above the screen (`bounds.y`
    /// negative), so a cursor sitting on the real tab-bar row (row 0)
    /// tested as being *above* the bar — landing in the `Split(Top)`
    /// branch of `quadraui::compute_drop_zone` instead of `TabReorder`.
    #[test]
    fn test_tui_single_group_tab_drag_reorder_not_split_477() {
        let mut e = test_engine("content\n");
        e.new_tab(None);
        e.new_tab(None);
        e.new_tab(None);
        assert_eq!(e.active_group().tabs.len(), 4);
        assert_eq!(
            e.editor_groups.len(),
            1,
            "test setup must stay a single tab group"
        );

        let theme = crate::render::Theme::onedark();
        let sidebar = TuiSidebar::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let screen = build_screen_for_tui(&e, &theme, area, &sidebar, 0);
        assert!(
            screen.editor_group_split.is_none(),
            "test setup must stay a single tab group"
        );

        let slots = build_tui_tab_slots(&screen, &e, 0.0);
        let group_slots = slots
            .get(&e.active_group.0)
            .expect("single-group tab slots must be keyed by the active group id");
        assert_eq!(group_slots.len(), 4, "expected 4 visible tab slots");

        // Cursor over the middle of tab index 2 (3rd tab), row 0 — the tab
        // bar's own row. A drop here reorders within the group; it must
        // never be resolved as a split.
        let (s2, e2) = group_slots[2];
        let cursor_col = ((s2 + e2) / 2.0).round() as u16;
        let zone = compute_tui_tab_drop_zone(
            &e,
            cursor_col,
            0,
            0,
            Some(&screen),
            Some(Size {
                width: 80,
                height: 24,
            }),
        );
        match zone {
            crate::core::window::DropZone::TabReorder(gid, _) => {
                assert_eq!(gid, e.active_group, "reorder must target the source group");
            }
            other => {
                panic!("expected DropZone::TabReorder for a drop on the tab bar, got {other:?}")
            }
        }
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

    /// Render a full frame and return the raw `Buffer` so tests can inspect
    /// individual cells (symbol / column) at the tab-group boundary.
    fn render_tui_buffer(engine: &Engine, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = crate::render::Theme::onedark();
        let mut sidebar = TuiSidebar::new();
        let area = Rect {
            x: 0,
            y: 0,
            width,
            height,
        };
        let screen = build_screen_for_tui(engine, &theme, area, &sidebar, 0);
        let mut hlr = Vec::new();
        let mut hpr = None;
        let mut ehpr = None;
        let mut ehlr = Vec::new();
        let mut ehs = None;
        let mut tvc: Vec<(GroupId, usize)> = Vec::new();
        let mut dtr = quadraui::Rect::default();
        let mut cl = None;
        let mut cml = None;
        let mut dl = None;
        let mut backend2 = super::backend::TuiBackend::new();
        terminal
            .draw(|frame| {
                draw_frame(
                    frame,
                    &screen,
                    &theme,
                    &mut sidebar,
                    engine,
                    0,
                    0,
                    None,
                    None,
                    None,
                    &mut hlr,
                    &mut hpr,
                    &mut ehpr,
                    &mut ehlr,
                    &mut ehs,
                    &mut tvc,
                    &mut dtr,
                    &mut cl,
                    &mut cml,
                    &mut dl,
                    &mut backend2,
                    None,
                    None,
                    &crate::core::window::DropZone::None,
                );
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// #481 iteration 4 regression: two vertically-split tab groups whose
    /// windows both overflow (so each shows a scrollbar) must render exactly
    /// ONE vertical bar at the group boundary — the left window's own
    /// `draw_editor` scrollbar. The pre-fix `render_separators` redundantly
    /// repainted a second scrollbar in the same column using `a.rect.height`
    /// (which includes the per-window status-line row) as the track height,
    /// so the repaint ran one row taller and bled a stray track glyph onto
    /// the status bar — the operator saw this as a slightly-longer "duplicate"
    /// scrollbar jammed against the real one.
    #[test]
    fn test_tui_two_groups_single_boundary_scrollbar_481() {
        let mut text = String::new();
        for i in 0..100 {
            text.push_str(&format!("line {i}\n"));
        }
        let mut e = test_engine(&text);
        e.open_editor_group(crate::core::window::SplitDirection::Vertical);
        // Operator flow: scroll the LEFT group down, the RIGHT group to a
        // different position, so both windows overflow and show scrollbars.
        e.focus_window_direction(crate::core::window::SplitDirection::Vertical, false);
        e.handle_key("G", Some('G'), false);
        e.focus_window_direction(crate::core::window::SplitDirection::Vertical, true);
        e.handle_key("g", Some('g'), false);
        e.handle_key("g", Some('g'), false);

        let width = 80u16;
        let height = 24u16;
        let buf = render_tui_buffer(&e, width, height);

        // Recover the boundary column: the left window's last column, where
        // `draw_editor` paints its scrollbar. Locate the single boundary by
        // scanning for a column that is entirely scrollbar glyphs across the
        // editor body (rows 2..21 in this fixture) and is NOT the far-right
        // scrollbar of the right pane.
        let scroll_glyph = |s: &str| s == "█" || s == "░";
        let mut boundary_cols: Vec<u16> = Vec::new();
        for x in 0..width {
            let mut scroll_rows = 0;
            for y in 2..22u16 {
                if scroll_glyph(buf[(x, y)].symbol()) {
                    scroll_rows += 1;
                }
            }
            // A scrollbar column is (nearly) all scroll glyphs down the body.
            if scroll_rows >= 18 {
                boundary_cols.push(x);
            }
        }
        // Exactly two scrollbar columns overall: the left pane's (at the group
        // boundary) and the right pane's (far right edge). Crucially they must
        // not be adjacent — no "two jammed together" at the boundary.
        assert_eq!(
            boundary_cols.len(),
            2,
            "expected exactly 2 scrollbar columns (one per pane), got {boundary_cols:?}"
        );
        assert!(
            boundary_cols[1] - boundary_cols[0] > 2,
            "the two panes' scrollbars must be far apart, not jammed together: {boundary_cols:?}"
        );

        // The group-boundary scrollbar column must not have an adjacent
        // second vertical bar (scrollbar glyph or '│') immediately to its
        // right — that was the duplicate the operator reported.
        let sep_col = boundary_cols[0];
        for y in 2..22u16 {
            let right = buf[(sep_col + 1, y)].symbol();
            assert!(
                !(scroll_glyph(right) || right == "│"),
                "row {y}: found a duplicate separator glyph {right:?} at col {} right beside the boundary scrollbar",
                sep_col + 1
            );
        }

        // The boundary scrollbar must NOT bleed onto the per-window status
        // row (row 22): `draw_editor` reserves that row, and the old
        // `render_separators` repaint (one row too tall) painted a stray
        // track glyph there.
        let status_row = 22u16;
        assert!(
            !scroll_glyph(buf[(sep_col, status_row)].symbol()),
            "boundary scrollbar bled a glyph onto the status row at ({sep_col}, {status_row})"
        );
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

    /// #481 regression: in multi-tab-group vertical layouts, a group whose
    /// left window overflows renders that window's scrollbar in the column
    /// immediately before the group divider. The divider glyph must NOT be
    /// painted next to that scrollbar — doing so produced a phantom
    /// "duplicate scrollbar" bar. The scrollbar column doubles as the group
    /// separator, so no `│` may sit immediately to the right of a scrollbar
    /// glyph anywhere in the grid.
    #[test]
    fn test_tui_no_phantom_divider_beside_scrollbar_481() {
        let content: String = (1..=200).map(|i| format!("line {i}\n")).collect();
        let mut e = test_engine(&content);
        // Two tab groups side by side; scroll the right one so it overflows.
        e.open_editor_group(crate::core::window::SplitDirection::Vertical);
        e.active_window_mut().view.scroll_top = 40;
        e.active_window_mut().view.cursor.line = 45;
        // A third group, scrolled to yet another position.
        e.open_editor_group(crate::core::window::SplitDirection::Vertical);
        e.active_window_mut().view.scroll_top = 80;
        e.active_window_mut().view.cursor.line = 85;

        let lines = render_tui(&e, 80, 24);
        for (y, l) in lines.iter().enumerate() {
            let chars: Vec<char> = l.chars().collect();
            for x in 1..chars.len() {
                let left = chars[x - 1];
                let cur = chars[x];
                if (left == '█' || left == '░') && cur == '│' {
                    panic!(
                        "phantom divider '│' at row {y}, col {x} sits immediately \
                         right of scrollbar glyph '{left}' — duplicate-scrollbar bug (#481)\n\
                         row: {l}"
                    );
                }
            }
        }
    }

    /// #481 guard: a group divider is still drawn between groups when the
    /// left window does NOT overflow (no scrollbar to double as separator).
    #[test]
    fn test_tui_divider_present_without_scrollbar_481() {
        // Short file: no overflow, so no per-window scrollbar.
        let content = "a\nb\nc\n";
        let mut e = test_engine(content);
        e.open_editor_group(crate::core::window::SplitDirection::Vertical);
        let lines = render_tui(&e, 80, 24);
        let has_divider = lines.iter().any(|l| l.contains('│'));
        assert!(
            has_divider,
            "vertical group divider '│' must be drawn when the left window has no scrollbar"
        );
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

    // ── :help render regression tests (#596) ─────────────────────────────────

    /// Drive `:help` through the full event→handle→render path and assert that
    /// (a) the engine does not panic, and (b) the help content appears in the
    /// rendered output.
    ///
    /// Also exercises the case where the cursor is at a non-zero line before
    /// the split (the new help window inherits the view, but the help buffer
    /// is shorter — verifying that the render handles an out-of-range cursor).
    #[test]
    fn test_tui_help_no_panic_and_renders_content() {
        // Use a 50-line buffer and move the cursor down so the view is at a
        // non-zero position before :help is invoked.
        let long_text: String = (0..50).map(|i| format!("line {}\n", i)).collect();
        let mut e = test_engine(&long_text);
        // Move cursor to line ~30 so view.scroll_top and cursor.line are non-zero.
        for _ in 0..30 {
            e.handle_key("j", Some('j'), false);
        }
        // Enter command mode and type 'help', then submit.
        e.handle_key(":", Some(':'), false);
        e.handle_key("h", Some('h'), false);
        e.handle_key("e", Some('e'), false);
        e.handle_key("l", Some('l'), false);
        e.handle_key("p", Some('p'), false);
        e.handle_key("Return", None, false);

        // After :help, there should be a split (2 windows in the layout).
        let win_count = e.active_tab().layout.window_ids().len();
        assert_eq!(win_count, 2, ":help should open a vsplit (2 windows)");

        // Rendering must not panic.
        let lines = render_tui(&e, 80, 24);

        // The help content should appear somewhere in the rendered output.
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("VimCode Help") || rendered.contains("topics"),
            "rendered output should contain help content, got:\n{rendered}"
        );
    }

    /// `:help topics` — named topic must open a split and render content.
    #[test]
    fn test_tui_help_topics_no_panic() {
        let mut e = test_engine("");
        e.handle_key(":", Some(':'), false);
        for ch in "help topics".chars() {
            e.handle_key(&ch.to_string(), Some(ch), false);
        }
        e.handle_key("Return", None, false);
        let lines = render_tui(&e, 80, 24);
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("VimCode Help") || rendered.contains("topics"),
            "help topics should render, got:\n{rendered}"
        );
    }

    /// `:help keys` — must not panic, and must render key reference content.
    #[test]
    fn test_tui_help_keys_no_panic() {
        let mut e = test_engine("");
        e.handle_key(":", Some(':'), false);
        for ch in "help keys".chars() {
            e.handle_key(&ch.to_string(), Some(ch), false);
        }
        e.handle_key("Return", None, false);
        let lines = render_tui(&e, 80, 24);
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("Normal Mode") || rendered.contains("Motion"),
            "help keys should render key reference, got:\n{rendered}"
        );
    }

    /// `:help commands` — must not panic, and must render command reference.
    #[test]
    fn test_tui_help_commands_no_panic() {
        let mut e = test_engine("");
        e.handle_key(":", Some(':'), false);
        for ch in "help commands".chars() {
            e.handle_key(&ch.to_string(), Some(ch), false);
        }
        e.handle_key("Return", None, false);
        let lines = render_tui(&e, 80, 24);
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("Command Mode") || rendered.contains(":w"),
            "help commands should render command reference, got:\n{rendered}"
        );
    }

    /// `:help explorer` — must not panic, and must render explorer keys.
    #[test]
    fn test_tui_help_explorer_no_panic() {
        let mut e = test_engine("");
        e.handle_key(":", Some(':'), false);
        for ch in "help explorer".chars() {
            e.handle_key(&ch.to_string(), Some(ch), false);
        }
        e.handle_key("Return", None, false);
        let lines = render_tui(&e, 80, 24);
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("Explorer") || rendered.contains("sidebar"),
            "help explorer should render explorer reference, got:\n{rendered}"
        );
    }

    /// `:help bogus` (unknown topic) — must NOT open a split and must show
    /// the "No help for..." message without crashing.
    #[test]
    fn test_tui_help_unknown_topic_no_panic() {
        let mut e = test_engine("");
        let win_count_before = e.active_tab().layout.window_ids().len();
        e.handle_key(":", Some(':'), false);
        for ch in "help bogus".chars() {
            e.handle_key(&ch.to_string(), Some(ch), false);
        }
        e.handle_key("Return", None, false);

        // Unknown topic must NOT create a new split.
        let win_count_after = e.active_tab().layout.window_ids().len();
        assert_eq!(
            win_count_after, win_count_before,
            "unknown :help topic must not open a split"
        );
        assert!(
            e.message.contains("No help for"),
            "engine.message should say 'No help for', got: {:?}",
            e.message
        );

        // Render must not panic either.
        let _lines = render_tui(&e, 80, 24);
    }
}
