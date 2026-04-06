use super::*;

// ─── Mouse handling ───────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_mouse(
    ev: MouseEvent,
    sidebar: &mut TuiSidebar,
    engine: &mut Engine,
    terminal_size: &Option<Size>,
    sidebar_width: u16,
    dragging_sidebar: &mut bool,
    dragging_scrollbar: &mut Option<ScrollDragState>,
    dragging_sidebar_search: &mut Option<SidebarScrollDrag>,
    dragging_debug_sb: &mut Option<DebugSidebarScrollDrag>,
    dragging_terminal_sb: &mut Option<(u16, u16, usize)>,
    debug_output_scroll: &mut usize,
    dragging_debug_output_sb: &mut Option<(u16, u16, usize)>,
    dragging_terminal_resize: &mut bool,
    dragging_terminal_split: &mut bool,
    dragging_group_divider: &mut Option<usize>,
    dragging_settings_sb: &mut Option<SidebarScrollDrag>,
    dragging_generic_sb: &mut Option<SidebarScrollDrag>,
    last_layout: Option<&render::ScreenLayout>,
    last_click_time: &mut Instant,
    last_click_pos: &mut (u16, u16),
    mouse_text_drag: &mut bool,
    folder_picker: &mut Option<FolderPickerState>,
    quit_confirm: &mut bool,
    close_tab_confirm: &mut bool,
    cmd_sel: &mut Option<(usize, usize)>,
    cmd_dragging: &mut bool,
    should_quit: &mut bool,
    explorer_drag_src: &mut Option<usize>,
    explorer_drag_active: &mut Option<(usize, Option<usize>)>,
    tab_drag_start: &mut Option<(u16, u16)>,
    tab_dragging: &mut bool,
    hover_link_rects: &[(u16, u16, u16, u16, String)],
    hover_popup_rect: Option<(u16, u16, u16, u16)>,
    editor_hover_popup_rect: Option<(u16, u16, u16, u16)>,
    editor_hover_link_rects: &[(u16, u16, u16, u16, String)],
    hover_selecting: &mut bool,
) -> u16 {
    let col = ev.column;
    let row = ev.row;
    let term_height = terminal_size.map(|s| s.height).unwrap_or(24);

    let ab_width = if engine.settings.autohide_panels && !sidebar.visible {
        0
    } else {
        ACTIVITY_BAR_WIDTH
    };
    let editor_left = ab_width
        + if sidebar.visible {
            sidebar_width + 1
        } else {
            0
        };

    // Check if the mouse cursor is currently inside or adjacent to the hover
    // popup bounding rect. We include 1 column to the left (the sidebar
    // separator) so the popup doesn't dismiss while the mouse crosses to it.
    let mouse_on_hover_popup = hover_popup_rect.is_some_and(|(px, py, pw, ph)| {
        col >= px.saturating_sub(1) && col < px + pw && row >= py && row < py + ph
    });

    // Check if mouse is on the editor hover popup (exact bounds).
    let mouse_on_editor_hover = editor_hover_popup_rect
        .is_some_and(|(px, py, pw, ph)| col >= px && col < px + pw && row >= py && row < py + ph);

    // ── Hover link click-to-copy ────────────────────────────────────────────────
    if !hover_link_rects.is_empty() {
        if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
            for &(lx, ly, lw, _lh, ref url) in hover_link_rects {
                if row == ly && col >= lx && col < lx + lw {
                    if url.starts_with("command:") {
                        engine.execute_command_uri(url);
                    } else {
                        tui_copy_to_clipboard(url, engine);
                    }
                    engine.dismiss_panel_hover_now();
                    return sidebar_width;
                }
            }
        }
    }

    // ── Dialog popup click handling ─────────────────────────────────────────────
    if engine.dialog.is_some() {
        if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
            let term_cols = terminal_size.map(|s| s.width).unwrap_or(80);
            // Recompute dialog geometry (same formula as render_dialog_popup).
            let dialog = engine.dialog.as_ref().unwrap();
            let body_max = dialog.body.iter().map(|l| l.len()).max().unwrap_or(0);
            let btn_row_len: usize = dialog
                .buttons
                .iter()
                .map(|b| render::format_button_label(&b.label, b.hotkey).len() + 4)
                .sum::<usize>()
                + 2;
            let content_width = body_max.max(dialog.title.len() + 4).max(btn_row_len);
            let width = (content_width as u16 + 4).clamp(40, term_cols.saturating_sub(4));
            let height = (3 + dialog.body.len() as u16 + 2 + 1).min(term_height.saturating_sub(4));
            let px = (term_cols.saturating_sub(width)) / 2;
            let py = (term_height.saturating_sub(height)) / 2;
            let btn_y = py + height - 2;

            if row == btn_y {
                // Walk the button positions to find which was clicked.
                let mut col_offset = px + 2;
                for (idx, btn) in dialog.buttons.iter().enumerate() {
                    let label = render::format_button_label(&btn.label, btn.hotkey);
                    let btn_w = label.len() as u16 + 4; // "  label  "
                    if col >= col_offset && col < col_offset + btn_w {
                        let action = engine.dialog_click_button(idx);
                        if engine.explorer_needs_refresh {
                            engine.explorer_needs_refresh = false;
                            sidebar.build_rows();
                        }
                        if handle_action(engine, action) {
                            *should_quit = true;
                        }
                        return sidebar_width;
                    }
                    col_offset += btn_w;
                }
            }
            // Click outside dialog — dismiss (Escape equivalent).
            if col < px || col >= px + width || row < py || row >= py + height {
                engine.dialog = None;
                engine.pending_move = None;
            }
        }
        return sidebar_width;
    }

    // ── Folder picker mouse handling ────────────────────────────────────────────
    if let Some(ref mut picker) = folder_picker {
        if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
            let term_cols = terminal_size.map(|s| s.width).unwrap_or(80);
            let term_rows = terminal_size.map(|s| s.height).unwrap_or(24);
            let popup_w = (term_cols * 3 / 5).max(50);
            let popup_h = (term_rows * 55 / 100).max(15);
            let popup_x = (term_cols.saturating_sub(popup_w)) / 2;
            let popup_y = (term_rows.saturating_sub(popup_h)) / 2;
            let results_start = popup_y + 3;
            let results_end = popup_y + popup_h - 1;

            if col >= popup_x
                && col < popup_x + popup_w
                && row >= results_start
                && row < results_end
            {
                let clicked_idx = picker.scroll_top + (row - results_start) as usize;
                if clicked_idx < picker.filtered.len() {
                    picker.selected = clicked_idx;
                }
            } else if col < popup_x
                || col >= popup_x + popup_w
                || row < popup_y
                || row >= popup_y + popup_h
            {
                // Click outside popup — dismiss
                *folder_picker = None;
            }
            return sidebar_width;
        }
    }

    // ── Unified picker mouse handling ────────────────────────────────────────
    if engine.picker_open {
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let term_cols = terminal_size.map(|s| s.width).unwrap_or(80);
                let term_rows = terminal_size.map(|s| s.height).unwrap_or(24);
                let has_preview = engine.picker_preview.is_some();
                let popup_w = if has_preview {
                    (term_cols * 4 / 5).max(60)
                } else {
                    (term_cols * 55 / 100).max(55)
                };
                let popup_h = if has_preview {
                    (term_rows * 65 / 100).max(18)
                } else {
                    (term_rows * 60 / 100).max(16)
                };
                let popup_x = (term_cols.saturating_sub(popup_w)) / 2;
                let popup_y = (term_rows.saturating_sub(popup_h)) / 2;
                let results_start = popup_y + 3;
                let results_end = popup_y + popup_h - 1;

                if col >= popup_x
                    && col < popup_x + popup_w
                    && row >= results_start
                    && row < results_end
                {
                    let clicked_idx = engine.picker_scroll_top + (row - results_start) as usize;
                    if clicked_idx < engine.picker_items.len() {
                        if engine.picker_selected == clicked_idx {
                            // Second click on same item — toggle expand or confirm
                            let in_tree_mode = engine.picker_source
                                == crate::core::engine::PickerSource::CommandCenter
                                && engine.picker_query == "@";
                            if in_tree_mode && engine.picker_toggle_expand() {
                                engine.picker_load_preview();
                            } else {
                                engine.picker_confirm();
                            }
                        } else {
                            engine.picker_selected = clicked_idx;
                            engine.picker_load_preview();
                        }
                    }
                } else if col < popup_x
                    || col >= popup_x + popup_w
                    || row < popup_y
                    || row >= popup_y + popup_h
                {
                    engine.close_picker();
                }
            }
            MouseEventKind::ScrollDown => {
                let step = 3;
                let max = engine.picker_items.len().saturating_sub(1);
                engine.picker_selected = (engine.picker_selected + step).min(max);
                let visible = 20usize;
                if engine.picker_selected >= engine.picker_scroll_top + visible {
                    engine.picker_scroll_top = engine.picker_selected + 1 - visible;
                }
                engine.picker_load_preview();
            }
            MouseEventKind::ScrollUp => {
                let step = 3;
                engine.picker_selected = engine.picker_selected.saturating_sub(step);
                if engine.picker_selected < engine.picker_scroll_top {
                    engine.picker_scroll_top = engine.picker_selected;
                }
                engine.picker_load_preview();
            }
            _ => {} // consume all other events
        }
        return sidebar_width;
    }

    // ── Sidebar separator drag (works anywhere, regardless of row) ────────────
    let sep_col = ab_width + if sidebar.visible { sidebar_width } else { 0 };
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) if sidebar.visible && col == sep_col => {
            *dragging_sidebar = true;
            return sidebar_width;
        }
        MouseEventKind::Drag(MouseButton::Left) if *dragging_sidebar => {
            let new_w = col.saturating_sub(ab_width);
            return new_w.clamp(15, 150);
        }
        MouseEventKind::Drag(MouseButton::Left) if *hover_selecting => {
            // Extend text selection in the editor hover popup
            if let Some((px, py, _pw, _ph)) = editor_hover_popup_rect {
                let scroll = engine
                    .editor_hover
                    .as_ref()
                    .map(|h| h.scroll_top)
                    .unwrap_or(0);
                let content_line = (row.saturating_sub(py + 1)) as usize + scroll;
                let content_col = col.saturating_sub(px + 2) as usize;
                engine.editor_hover_extend_selection(content_line, content_col);
            }
            return sidebar_width;
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            // Explorer drag-and-drop: activate or update target row.
            if explorer_drag_src.is_some() || explorer_drag_active.is_some() {
                let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                if sidebar.visible
                    && sidebar.active_panel == TuiPanel::Explorer
                    && col >= ab_width
                    && col < ab_width + sidebar_width
                {
                    let sidebar_row = row.saturating_sub(menu_rows);
                    if sidebar_row >= 1 {
                        let tree_row =
                            (sidebar_row as usize).saturating_sub(1) + sidebar.scroll_top;
                        if tree_row < sidebar.rows.len() {
                            if let Some(src_row) = *explorer_drag_src {
                                // Only activate drag if target differs from source.
                                if tree_row != src_row {
                                    *explorer_drag_active = Some((src_row, Some(tree_row)));
                                    *explorer_drag_src = None;
                                }
                            } else if let Some((src, _)) = explorer_drag_active {
                                *explorer_drag_active = Some((*src, Some(tree_row)));
                            }
                        }
                    }
                } else if let Some((src, _)) = explorer_drag_active {
                    // Mouse dragged outside sidebar — clear target but keep active.
                    *explorer_drag_active = Some((*src, None));
                }
                if explorer_drag_active.is_some() {
                    return sidebar_width;
                }
            }
            // Tab drag-and-drop: update drop zone while dragging.
            if *tab_dragging {
                engine.tab_drag_mouse = Some((col as f64, row as f64));
                engine.tab_drop_zone = compute_tui_tab_drop_zone(
                    engine,
                    col,
                    row,
                    editor_left,
                    last_layout,
                    *terminal_size,
                );
                return sidebar_width;
            }
            // Tab drag-and-drop: detect drag start (mouse moved far enough).
            if let Some((sx, sy)) = *tab_drag_start {
                let dx = col.abs_diff(sx);
                let dy = row.abs_diff(sy);
                if dx + dy >= 2 {
                    // Use the active group + active tab as the drag source.
                    let gid = engine.active_group;
                    let tidx = engine
                        .editor_groups
                        .get(&gid)
                        .map(|g| g.active_tab)
                        .unwrap_or(0);
                    engine.tab_drag_begin(gid, tidx);
                    engine.tab_drag_mouse = Some((col as f64, row as f64));
                    *tab_dragging = true;
                    *tab_drag_start = None;
                    return sidebar_width;
                }
                // Haven't moved enough yet — don't start any drag.
                return sidebar_width;
            }
            // Command-line text selection drag
            if *cmd_dragging {
                if let Some(ref mut sel) = *cmd_sel {
                    sel.1 = col as usize;
                }
                return sidebar_width;
            }
            // Debug sidebar section scrollbar drag
            if let Some(ref drag) = *dragging_debug_sb {
                if drag.track_len > 0 && drag.total > 0 {
                    let end = drag.track_abs_start + drag.track_len - 1;
                    let clamped = row.clamp(drag.track_abs_start, end);
                    let ratio = (clamped - drag.track_abs_start) as f64 / drag.track_len as f64;
                    let max_scroll = drag.total.saturating_sub(drag.track_len as usize);
                    engine.dap_sidebar_scroll[drag.sec_idx] =
                        (ratio * max_scroll as f64).round() as usize;
                }
                return sidebar_width;
            }
            // Sidebar search-results scrollbar drag
            if let Some(ref drag) = *dragging_sidebar_search {
                if drag.track_len > 0 && drag.total > 0 {
                    let end = drag.track_abs_start + drag.track_len - 1;
                    let clamped = row.clamp(drag.track_abs_start, end);
                    let ratio = (clamped - drag.track_abs_start) as f64 / drag.track_len as f64;
                    let new_scroll = (ratio * drag.total as f64) as usize;
                    sidebar.search_scroll_top =
                        new_scroll.min(drag.total.saturating_sub(drag.track_len as usize));
                }
                return sidebar_width;
            }
            // Generic sidebar scrollbar drag (explorer, ext panel, etc.)
            if let Some(ref drag) = *dragging_generic_sb {
                if drag.track_len > 0 && drag.total > 0 {
                    let end = drag.track_abs_start + drag.track_len - 1;
                    let clamped = row.clamp(drag.track_abs_start, end);
                    let ratio = (clamped - drag.track_abs_start) as f64 / drag.track_len as f64;
                    let new_scroll = (ratio * drag.total as f64) as usize;
                    let max_scroll = drag.total.saturating_sub(drag.track_len as usize);
                    // Route to the right panel's scroll state
                    if sidebar.ext_panel_name.is_some() {
                        engine.ext_panel_scroll_top = new_scroll.min(drag.total.saturating_sub(1));
                    } else if sidebar.active_panel == TuiPanel::Explorer {
                        sidebar.scroll_top = new_scroll.min(max_scroll);
                    }
                }
                return sidebar_width;
            }
            // Settings panel scrollbar drag
            if let Some(ref drag) = *dragging_settings_sb {
                if drag.track_len > 0 && drag.total > 0 {
                    let end = drag.track_abs_start + drag.track_len - 1;
                    let clamped = row.clamp(drag.track_abs_start, end);
                    let ratio = (clamped - drag.track_abs_start) as f64 / drag.track_len as f64;
                    let max_scroll = drag.total.saturating_sub(drag.track_len as usize);
                    engine.settings_scroll_top = (ratio * max_scroll as f64).round() as usize;
                }
                return sidebar_width;
            }
            // Terminal panel resize drag
            if *dragging_terminal_resize {
                let qf_h: u16 = if engine.quickfix_open { 6 } else { 0 };
                let available = term_height.saturating_sub(row + 2 + qf_h);
                let new_rows = available.saturating_sub(1).clamp(5, 30);
                engine.session.terminal_panel_rows = new_rows;
                return sidebar_width;
            }
            // Group divider drag — update ratio based on mouse position.
            if let Some(split_index) = *dragging_group_divider {
                if let Some(split) = last_layout.and_then(|l| l.editor_group_split.as_ref()) {
                    if let Some(div) = split.dividers.iter().find(|d| d.split_index == split_index)
                    {
                        let mr: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                        let editor_row = row.saturating_sub(mr);
                        let rel_col = col.saturating_sub(editor_left);
                        let mouse_pos = match div.direction {
                            crate::core::window::SplitDirection::Vertical => rel_col as f64,
                            crate::core::window::SplitDirection::Horizontal => editor_row as f64,
                        };
                        let new_ratio = (mouse_pos - div.axis_start) / div.axis_size;
                        engine
                            .group_layout
                            .set_ratio_at_index(split_index, new_ratio);
                    }
                }
                return sidebar_width;
            }
            // Terminal split divider drag — update visual column position (no PTY resize yet).
            if *dragging_terminal_split {
                let term_width = terminal_size.map(|s| s.width).unwrap_or(80);
                let sb_col = term_width.saturating_sub(1);
                let left_cols = col.clamp(5, sb_col.saturating_sub(5));
                engine.terminal_split_set_drag_cols(left_cols);
                return sidebar_width;
            }
            // Debug output panel scrollbar drag
            if let Some((track_start, track_len, total)) = *dragging_debug_output_sb {
                if track_len > 0 && total > 0 {
                    let offset_in_track = row.saturating_sub(track_start).min(track_len) as f64;
                    let ratio = offset_in_track / track_len as f64;
                    // ratio=0 (top) → max scroll (oldest); ratio=1 (bottom) → 0 (newest)
                    let max_scroll = total.saturating_sub(track_len as usize);
                    *debug_output_scroll = ((1.0 - ratio) * max_scroll as f64).round() as usize;
                }
                return sidebar_width;
            }
            // Terminal scrollbar drag
            if let Some((track_start, track_len, total)) = *dragging_terminal_sb {
                if track_len > 0 && total > 0 {
                    // Use saturating_sub + min(track_len) so ratio reaches exactly 1.0
                    // at the bottom of the track (allowing scroll_offset to reach 0).
                    let offset_in_track = row.saturating_sub(track_start).min(track_len) as f64;
                    let ratio = offset_in_track / track_len as f64;
                    // top (ratio=0) → max offset; bottom (ratio=1) → 0 (live view)
                    let new_offset = ((1.0 - ratio) * total as f64) as usize;
                    if let Some(term) = engine.active_terminal_mut() {
                        term.set_scroll_offset(new_offset);
                    }
                }
                return sidebar_width;
            }
            // Scrollbar thumb drag (vertical or horizontal)
            if let Some(ref drag) = *dragging_scrollbar {
                if drag.track_len > 0 && drag.total > 0 {
                    if drag.is_horizontal {
                        let end = drag.track_abs_start + drag.track_len - 1;
                        let clamped = col.clamp(drag.track_abs_start, end);
                        let ratio = (clamped - drag.track_abs_start) as f64 / drag.track_len as f64;
                        let new_left = (ratio * drag.total as f64) as usize;
                        engine.set_scroll_left_for_window(drag.window_id, new_left);
                    } else {
                        let end = drag.track_abs_start + drag.track_len - 1;
                        let clamped = row.clamp(drag.track_abs_start, end);
                        let ratio = (clamped - drag.track_abs_start) as f64 / drag.track_len as f64;
                        let new_top = (ratio * drag.total as f64) as usize;
                        engine.set_scroll_top_for_window(drag.window_id, new_top);
                        engine.sync_scroll_binds();
                    }
                }
                return sidebar_width;
            }
            // Text drag-to-select — find window under cursor and extend visual selection
            if col >= editor_left {
                if let Some(layout) = last_layout {
                    let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                    let editor_row = row.saturating_sub(menu_rows);
                    for rw in &layout.windows {
                        let wx = rw.rect.x as u16;
                        let wy = rw.rect.y as u16;
                        let ww = rw.rect.width as u16;
                        let wh = rw.rect.height as u16;
                        let gutter = rw.gutter_char_width as u16;
                        let rel_col = col - editor_left;
                        if rel_col >= wx
                            && rel_col < wx + ww
                            && editor_row >= wy
                            && editor_row < wy + wh
                        {
                            // Skip per-window status bar row
                            if rw.status_line.is_some() && wh > 1 && editor_row == wy + wh - 1 {
                                break;
                            }
                            let view_row = (editor_row - wy) as usize;
                            let drag_rl = rw.lines.get(view_row);
                            let buf_line = drag_rl
                                .map(|l| l.line_idx)
                                .unwrap_or_else(|| rw.scroll_top + view_row);
                            let seg_offset = drag_rl.map(|l| l.segment_col_offset).unwrap_or(0);
                            let col_in_text = (rel_col - wx).saturating_sub(gutter) as usize
                                + rw.scroll_left
                                + seg_offset;
                            engine.mouse_drag(rw.window_id, buf_line, col_in_text);
                            *mouse_text_drag = true;
                            return sidebar_width;
                        }
                    }
                }
            }
            // Terminal drag-to-select in content rows.
            {
                let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
                let strip_rows: u16 = if engine.terminal_open {
                    engine.session.terminal_panel_rows + 1
                } else {
                    0
                };
                let term_strip_top = term_height.saturating_sub(2 + qf_rows + strip_rows);
                if engine.terminal_open
                    && strip_rows > 0
                    && row > term_strip_top
                    && row < term_strip_top + strip_rows
                {
                    let term_row = row - term_strip_top - 1;
                    if let Some(term) = engine.active_terminal_mut() {
                        if let Some(ref mut sel) = term.selection {
                            sel.end_row = term_row;
                            sel.end_col = col;
                        }
                    }
                    return sidebar_width;
                }
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            // Tab drag-and-drop: execute drop on release.
            if *tab_dragging {
                *tab_dragging = false;
                *tab_drag_start = None;
                let zone = engine.tab_drop_zone;
                engine.tab_drag_drop(zone);
                return sidebar_width;
            }
            *tab_drag_start = None;
            // Explorer drag-and-drop: execute move on release.
            if let Some((src_row, Some(target_row))) = explorer_drag_active.take() {
                *explorer_drag_src = None;
                if src_row < sidebar.rows.len() && target_row < sidebar.rows.len() {
                    let src_path = sidebar.rows[src_row].path.clone();
                    let target = &sidebar.rows[target_row];
                    let dest_dir = if target.is_dir {
                        target.path.clone()
                    } else {
                        target
                            .path
                            .parent()
                            .unwrap_or(std::path::Path::new("."))
                            .to_path_buf()
                    };
                    engine.confirm_move_file(&src_path, &dest_dir);
                }
                return sidebar_width;
            }
            *explorer_drag_src = None;
            *explorer_drag_active = None;
            *dragging_sidebar = false;
            *dragging_scrollbar = None;
            *dragging_sidebar_search = None;
            *dragging_debug_sb = None;
            *dragging_terminal_sb = None;
            *dragging_debug_output_sb = None;
            *dragging_settings_sb = None;
            *dragging_generic_sb = None;
            *dragging_group_divider = None;
            *cmd_dragging = false;
            *hover_selecting = false;
            if *dragging_terminal_resize {
                *dragging_terminal_resize = false;
                let rows = engine.session.terminal_panel_rows;
                let cols = terminal_size.map(|s| s.width).unwrap_or(80);
                engine.terminal_resize(cols, rows);
                let _ = engine.session.save();
            }
            if *dragging_terminal_split {
                *dragging_terminal_split = false;
                let left_cols = engine.terminal_split_left_cols;
                if left_cols > 0 {
                    let term_width = terminal_size.map(|s| s.width).unwrap_or(80);
                    let sb_col = term_width.saturating_sub(1);
                    let right_cols = sb_col.saturating_sub(left_cols).saturating_sub(1);
                    let rows = engine.session.terminal_panel_rows;
                    engine.terminal_split_finalize_drag(left_cols, right_cols, rows);
                }
            }
            *mouse_text_drag = false;
            engine.mouse_drag_active = false;
            engine.mouse_drag_origin_window = None;
            // Auto-copy terminal selection to clipboard on mouse-release.
            if engine.terminal_has_focus {
                let text = engine.active_terminal().and_then(|t| t.selected_text());
                if let Some(ref text) = text {
                    if let Some(ref cb) = engine.clipboard_write {
                        let _ = cb(text);
                    }
                }
            }
            return sidebar_width;
        }
        // Scroll wheel — sidebar or editor
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            // Editor hover popup scroll wheel — scroll content without focusing
            if mouse_on_editor_hover && engine.editor_hover.is_some() {
                let delta = if matches!(ev.kind, MouseEventKind::ScrollUp) {
                    -3
                } else {
                    3
                };
                engine.editor_hover_scroll(delta);
                return sidebar_width;
            }
            // Sidebar scroll wheel
            if sidebar.visible && col >= ab_width && col < ab_width + sidebar_width {
                if sidebar.ext_panel_name.is_some() {
                    // Extension-provided panel (e.g. Git Log): scroll viewport
                    let flat_len = engine.ext_panel_flat_len();
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        engine.ext_panel_scroll_top = engine.ext_panel_scroll_top.saturating_sub(3);
                    } else {
                        engine.ext_panel_scroll_top =
                            (engine.ext_panel_scroll_top + 3).min(flat_len.saturating_sub(1));
                    }
                } else if sidebar.active_panel == TuiPanel::Explorer {
                    let tree_height = term_height.saturating_sub(3) as usize;
                    let total = sidebar.rows.len();
                    if total > tree_height {
                        if matches!(ev.kind, MouseEventKind::ScrollUp) {
                            sidebar.scroll_top = sidebar.scroll_top.saturating_sub(3);
                        } else {
                            sidebar.scroll_top =
                                (sidebar.scroll_top + 3).min(total.saturating_sub(tree_height));
                        }
                    }
                } else if sidebar.active_panel == TuiPanel::Search {
                    // Scroll the viewport directly; render will keep selection visible.
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        sidebar.search_scroll_top = sidebar.search_scroll_top.saturating_sub(3);
                    } else {
                        sidebar.search_scroll_top += 3; // clamped in render_search_panel
                    }
                } else if sidebar.active_panel == TuiPanel::Git {
                    // SC panel: scroll selection
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        engine.sc_selected = engine.sc_selected.saturating_sub(3);
                    } else {
                        let flat_len = engine.sc_flat_len();
                        engine.sc_selected =
                            (engine.sc_selected + 3).min(flat_len.saturating_sub(1));
                    }
                } else if sidebar.active_panel == TuiPanel::Debug {
                    use crate::core::engine::DebugSidebarSection;
                    // Determine which section the mouse is over.
                    let menu_offset = if engine.menu_bar_visible { 1u16 } else { 0 };
                    let sidebar_row = row.saturating_sub(menu_offset);
                    let sections = [
                        (DebugSidebarSection::Variables, 0usize),
                        (DebugSidebarSection::Watch, 1),
                        (DebugSidebarSection::CallStack, 2),
                        (DebugSidebarSection::Breakpoints, 3),
                    ];
                    let mut cur_row: u16 = 2;
                    let mut target_idx: Option<usize> = None;
                    for (_section, sec_idx) in &sections {
                        let sec_height = engine.dap_sidebar_section_heights[*sec_idx];
                        let section_end = cur_row + 1 + sec_height; // header + content
                        if sidebar_row >= cur_row && sidebar_row < section_end {
                            target_idx = Some(*sec_idx);
                            break;
                        }
                        cur_row = section_end;
                    }
                    if let Some(sec_idx) = target_idx {
                        let item_count = engine.dap_sidebar_section_item_count(sections[sec_idx].0);
                        let height = engine.dap_sidebar_section_heights[sec_idx] as usize;
                        let max_scroll = item_count.saturating_sub(height);
                        if matches!(ev.kind, MouseEventKind::ScrollUp) {
                            engine.dap_sidebar_scroll[sec_idx] =
                                engine.dap_sidebar_scroll[sec_idx].saturating_sub(3);
                        } else {
                            engine.dap_sidebar_scroll[sec_idx] =
                                (engine.dap_sidebar_scroll[sec_idx] + 3).min(max_scroll);
                        }
                    }
                } else if sidebar.active_panel == TuiPanel::Settings {
                    let flat = engine.settings_flat_list();
                    let content_height = term_height.saturating_sub(4) as usize; // header+search+status+cmd
                    let max_scroll = flat.len().saturating_sub(content_height);
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        engine.settings_scroll_top = engine.settings_scroll_top.saturating_sub(3);
                    } else {
                        engine.settings_scroll_top =
                            (engine.settings_scroll_top + 3).min(max_scroll);
                    }
                } else if sidebar.active_panel == TuiPanel::Extensions {
                    // Scroll selection up/down
                    let total = engine.ext_available_manifests().len();
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        engine.ext_sidebar_selected = engine.ext_sidebar_selected.saturating_sub(3);
                    } else {
                        engine.ext_sidebar_selected =
                            (engine.ext_sidebar_selected + 3).min(total.saturating_sub(1));
                    }
                }
                return sidebar_width;
            }
            // Terminal panel scroll (must check before editor scroll).
            {
                let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
                let strip_rows: u16 = if engine.terminal_open {
                    engine.session.terminal_panel_rows + 1
                } else {
                    0
                };
                let term_strip_top = term_height.saturating_sub(2 + qf_rows + strip_rows);
                if engine.terminal_open
                    && strip_rows > 0
                    && row >= term_strip_top
                    && row < term_strip_top + strip_rows
                {
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        engine.terminal_scroll_up(3);
                    } else {
                        engine.terminal_scroll_down(3);
                    }
                    return sidebar_width;
                }
            }
            // Debug output panel scroll wheel.
            {
                let debug_output_open = engine.bottom_panel_kind
                    == render::BottomPanelKind::DebugOutput
                    && !engine.dap_output_lines.is_empty();
                if debug_output_open {
                    let dt_rows: u16 = if engine.debug_toolbar_visible { 1 } else { 0 };
                    let panel_height = engine.session.terminal_panel_rows + 2;
                    let panel_y = term_height.saturating_sub(2 + dt_rows + panel_height);
                    let panel_end = term_height.saturating_sub(2 + dt_rows);
                    if row >= panel_y && row < panel_end {
                        let content_rows = engine.session.terminal_panel_rows as usize;
                        let total = engine.dap_output_lines.len();
                        let max_scroll = total.saturating_sub(content_rows);
                        if matches!(ev.kind, MouseEventKind::ScrollUp) {
                            *debug_output_scroll = (*debug_output_scroll + 3).min(max_scroll);
                        } else {
                            *debug_output_scroll = debug_output_scroll.saturating_sub(3);
                        }
                        return sidebar_width;
                    }
                }
            }

            if col >= editor_left && row + 2 < term_height {
                let rel_col = col - editor_left;
                let scroll_menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                let editor_row = row.saturating_sub(scroll_menu_rows);
                // Find which window the mouse is over; scroll that window
                let scrolled = last_layout.and_then(|layout| {
                    layout.windows.iter().find(|rw| {
                        let wx = rw.rect.x as u16;
                        let wy = rw.rect.y as u16;
                        let ww = rw.rect.width as u16;
                        let wh = rw.rect.height as u16;
                        rel_col >= wx
                            && rel_col < wx + ww
                            && editor_row >= wy
                            && editor_row < wy + wh
                    })
                });
                if let Some(rw) = scrolled {
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        engine.scroll_up_visible_for_window(rw.window_id, 3);
                    } else {
                        engine.scroll_down_visible_for_window(rw.window_id, 3);
                    }
                    engine.sync_scroll_binds();
                } else {
                    // Fallback: scroll active window
                    if matches!(ev.kind, MouseEventKind::ScrollUp) {
                        engine.scroll_up_visible(3);
                    } else {
                        engine.scroll_down_visible(3);
                    }
                    engine.ensure_cursor_visible();
                    engine.sync_scroll_binds();
                }
            }
            return sidebar_width;
        }
        _ => {}
    }

    // ── Right-click: open context menus ────────────────────────────────────────
    if ev.kind == MouseEventKind::Down(MouseButton::Right) {
        // Close any existing context menu first.
        engine.close_context_menu();

        let menu_rows = if engine.menu_bar_visible { 1_u16 } else { 0 };

        // Right-click on explorer sidebar → open explorer context menu
        if sidebar.visible && col >= ab_width && col < ab_width + sidebar_width {
            if sidebar.active_panel == TuiPanel::Explorer {
                let sidebar_row = row.saturating_sub(menu_rows);
                let tree_row = sidebar_row as usize + sidebar.scroll_top;
                if tree_row < sidebar.rows.len() {
                    sidebar.selected = tree_row;
                    let path = sidebar.rows[tree_row].path.clone();
                    let is_dir = sidebar.rows[tree_row].is_dir;
                    engine.open_explorer_context_menu(path, is_dir, col, row);
                } else {
                    // Empty space below last entry → context menu for root folder
                    let root = sidebar.root.clone();
                    engine.open_explorer_context_menu(root, true, col, row);
                }
            }
            return sidebar_width;
        }

        // Right-click on tab bar → open tab context menu
        if col >= editor_left {
            let rel_col = col - editor_left;
            if let Some(layout) = last_layout {
                if let Some(ref split) = layout.editor_group_split {
                    let click_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
                    for gtb in split.group_tab_bars.iter() {
                        let tab_bar_row =
                            menu_rows + (gtb.bounds.y as u16).saturating_sub(click_tbh);
                        let gx = gtb.bounds.x as u16;
                        let gw = gtb.bounds.width as u16;
                        if row == tab_bar_row && rel_col >= gx && rel_col < gx + gw {
                            let local_col = rel_col - gx;
                            let ov_cols: u16 = if gtb.tab_scroll_offset > 0 { 2 } else { 0 };
                            let mut x: u16 = ov_cols;
                            for (i, tab) in gtb.tabs.iter().enumerate().skip(gtb.tab_scroll_offset)
                            {
                                let name_w = tab.name.chars().count() as u16;
                                let tab_w = name_w + TAB_CLOSE_COLS;
                                if local_col >= x && local_col < x + tab_w {
                                    engine.open_tab_context_menu(gtb.group_id, i, col, row + 1);
                                    return sidebar_width;
                                }
                                x += tab_w;
                            }
                            break;
                        }
                    }
                } else {
                    // Single-group tab bar (row == menu_rows)
                    if row == menu_rows && !engine.is_tab_bar_hidden(engine.active_group) {
                        let sg_offset = layout.tab_scroll_offset;
                        let ov_cols: u16 = if sg_offset > 0 { 2 } else { 0 };
                        let mut x: u16 = ov_cols;
                        for (i, tab) in layout.tab_bar.iter().enumerate().skip(sg_offset) {
                            let name_w = tab.name.chars().count() as u16;
                            let tab_w = name_w + TAB_CLOSE_COLS;
                            if rel_col >= x && rel_col < x + tab_w {
                                engine.open_tab_context_menu(engine.active_group, i, col, row + 1);
                                return sidebar_width;
                            }
                            x += tab_w;
                        }
                    }
                }
            }
        }

        // Right-click on editor area → open editor context menu
        if col >= editor_left {
            engine.open_editor_context_menu(col, row + 1);
        }

        return sidebar_width;
    }

    // ── Context menu click intercept ────────────────────────────────────────────
    if engine.context_menu.is_some() && ev.kind == MouseEventKind::Down(MouseButton::Left) {
        // Check if click is inside the context menu popup
        if let Some(ref cm) = engine.context_menu {
            let sep_count = cm.items.iter().filter(|i| i.separator_after).count() as u16;
            let popup_h = cm.items.len() as u16 + sep_count + 2;
            let max_label = cm.items.iter().map(|i| i.label.len()).max().unwrap_or(4);
            let max_sc = cm.items.iter().map(|i| i.shortcut.len()).max().unwrap_or(0);
            let popup_w = (max_label + max_sc + 6).clamp(20, 50) as u16;
            let term_w = terminal_size.map(|s| s.width).unwrap_or(80);
            let px = cm.screen_x.min(term_w.saturating_sub(popup_w));
            let py = cm.screen_y.min(term_height.saturating_sub(popup_h));

            if col >= px && col < px + popup_w && row >= py && row < py + popup_h {
                // Click inside — map to item
                let inner_row = row - py;
                if inner_row >= 1 && inner_row < popup_h - 1 {
                    // Walk items + separators to find which was clicked
                    let mut visual_row: u16 = 1;
                    for (idx, item) in cm.items.iter().enumerate() {
                        if visual_row == inner_row {
                            if item.enabled {
                                engine.context_menu.as_mut().unwrap().selected = idx;
                                let ctx = engine.context_menu_target_path();
                                if let Some(act) = engine.context_menu_confirm() {
                                    if let Some((ctx_path, ctx_is_dir)) = ctx {
                                        handle_explorer_context_action(
                                            &act,
                                            engine,
                                            sidebar,
                                            *terminal_size,
                                            ctx_path,
                                            ctx_is_dir,
                                        );
                                    }
                                }
                            }
                            return sidebar_width;
                        }
                        visual_row += 1;
                        if item.separator_after {
                            if visual_row == inner_row {
                                // Clicked on separator line — ignore
                                return sidebar_width;
                            }
                            visual_row += 1;
                        }
                    }
                }
                return sidebar_width;
            }
        }
        // Click outside — close menu
        engine.close_context_menu();
        // Fall through to process the click normally
    }

    // ── Context menu mouse hover ──────────────────────────────────────────────
    if engine.context_menu.is_some() && matches!(ev.kind, MouseEventKind::Moved) {
        // Compute hit item by examining the menu geometry.
        let mut hit_idx: Option<usize> = None;
        if let Some(ref cm) = engine.context_menu {
            let sep_count = cm.items.iter().filter(|i| i.separator_after).count() as u16;
            let popup_h = cm.items.len() as u16 + sep_count + 2;
            let max_label = cm.items.iter().map(|i| i.label.len()).max().unwrap_or(4);
            let max_sc = cm.items.iter().map(|i| i.shortcut.len()).max().unwrap_or(0);
            let popup_w = (max_label + max_sc + 6).clamp(20, 50) as u16;
            let term_w = terminal_size.map(|s| s.width).unwrap_or(80);
            let px = cm.screen_x.min(term_w.saturating_sub(popup_w));
            let py = cm.screen_y.min(term_height.saturating_sub(popup_h));

            if col >= px && col < px + popup_w && row >= py && row < py + popup_h {
                let inner_row = row - py;
                if inner_row >= 1 && inner_row < popup_h - 1 {
                    let mut visual_row: u16 = 1;
                    for (idx, item) in cm.items.iter().enumerate() {
                        if visual_row == inner_row && item.enabled {
                            hit_idx = Some(idx);
                            break;
                        }
                        visual_row += 1;
                        if item.separator_after {
                            visual_row += 1;
                        }
                    }
                }
            }
        }
        if let Some(idx) = hit_idx {
            if let Some(ref mut cm) = engine.context_menu {
                cm.selected = idx;
            }
        }
        return sidebar_width;
    }

    // ── Cancel hover dismiss if mouse is on the popup ─────────────────────
    if matches!(ev.kind, MouseEventKind::Moved) && mouse_on_hover_popup {
        engine.cancel_panel_hover_dismiss();
    }
    // Cancel editor hover dismiss if mouse is on the editor hover popup
    if matches!(ev.kind, MouseEventKind::Moved) && mouse_on_editor_hover {
        engine.cancel_editor_hover_dismiss();
    }

    // ── SC button hover (mouse moved) ───────────────────────────────────────
    if matches!(ev.kind, MouseEventKind::Moved) {
        let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
        if sidebar.visible
            && sidebar.active_panel == TuiPanel::Git
            && col >= ab_width
            && col < ab_width + sidebar_width
        {
            let sidebar_row = row.saturating_sub(menu_rows);
            let commit_rows = engine.sc_commit_message.split('\n').count().max(1) as u16;
            let btn_row = 1 + commit_rows + 1; // header + commit + pad_above
            if sidebar_row == btn_row {
                let rel_col = col.saturating_sub(ab_width);
                let commit_w = sidebar_width / 2;
                let btn_idx = if rel_col < commit_w {
                    0
                } else {
                    let icon_w = (sidebar_width - commit_w) / 3;
                    (1 + ((rel_col - commit_w) / icon_w.max(1))).min(3) as usize
                };
                engine.sc_button_hovered = Some(btn_idx);
            } else {
                engine.sc_button_hovered = None;
                // SC item hover dwell tracking (sections area).
                let section_start = 4 + commit_rows; // btn + pad_below + 1
                if sidebar_row >= section_start {
                    let adjusted = sidebar_row - section_start + 3;
                    if let Some((flat_idx, _is_header)) =
                        engine.sc_visual_row_to_flat(adjusted as usize, true)
                    {
                        engine.panel_hover_mouse_move("source_control", "", flat_idx);
                    } else if !mouse_on_hover_popup {
                        engine.dismiss_panel_hover();
                    }
                } else if !mouse_on_hover_popup {
                    engine.dismiss_panel_hover();
                }
            }
        } else {
            engine.sc_button_hovered = None;
            // If we were showing an SC hover and mouse left Git panel, dismiss
            // — unless the mouse is over the popup itself.
            if engine.panel_hover.is_some() && !mouse_on_hover_popup {
                engine.dismiss_panel_hover();
            }
        }
    }

    // ── Ext panel hover (mouse moved) ───────────────────────────────────────
    if matches!(ev.kind, MouseEventKind::Moved) {
        let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
        if sidebar.visible
            && sidebar.ext_panel_name.is_some()
            && col >= ab_width
            && col < ab_width + sidebar_width
        {
            if let Some(ref panel_name) = sidebar.ext_panel_name.clone() {
                let sidebar_row = row.saturating_sub(menu_rows);
                // Row 0 is the header; content items start at row 1.
                if sidebar_row >= 1 {
                    let flat_idx =
                        engine.ext_panel_scroll_top + (sidebar_row as usize).saturating_sub(1);
                    engine.panel_hover_mouse_move(panel_name, "", flat_idx);
                } else if !mouse_on_hover_popup {
                    engine.dismiss_panel_hover();
                }
            }
        } else if sidebar.ext_panel_name.is_some() && !mouse_on_hover_popup {
            // Mouse moved outside the ext panel area — dismiss hover.
            engine.dismiss_panel_hover();
        }
    }

    // ── Tab hover tooltip (mouse moved over tab bar) ────────────────────────
    if matches!(ev.kind, MouseEventKind::Moved) {
        let mut tooltip: Option<String> = None;
        if col >= editor_left {
            if let Some(layout) = last_layout {
                let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                let rel_col = col - editor_left;

                if let Some(ref split) = layout.editor_group_split {
                    let click_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
                    for gtb in split.group_tab_bars.iter() {
                        let tab_bar_row =
                            menu_rows + (gtb.bounds.y as u16).saturating_sub(click_tbh);
                        let gx = gtb.bounds.x as u16;
                        let gw = gtb.bounds.width as u16;
                        if row == tab_bar_row && rel_col >= gx && rel_col < gx + gw {
                            let local_col = rel_col - gx;
                            tooltip = tab_tooltip_at_col(
                                engine,
                                gtb.group_id,
                                local_col,
                                &gtb.tabs,
                                gtb.tab_scroll_offset,
                            );
                            break;
                        }
                    }
                } else if row == menu_rows && !engine.is_tab_bar_hidden(engine.active_group) {
                    tooltip = tab_tooltip_at_col(
                        engine,
                        engine.active_group,
                        rel_col,
                        &layout.tab_bar,
                        layout.tab_scroll_offset,
                    );
                }
            }
        }
        if tooltip != engine.tab_hover_tooltip {
            engine.tab_hover_tooltip = tooltip;
        }
    }

    // ── Editor hover dwell (mouse moved over editor area) ───────────────────
    if matches!(ev.kind, MouseEventKind::Moved)
        && !mouse_on_editor_hover
        && col >= editor_left
        && engine.settings.hover_delay > 0
        && !engine.editor_hover_has_focus
        && (matches!(
            engine.mode,
            Mode::Normal | Mode::Visual | Mode::VisualLine | Mode::VisualBlock
        ) || engine.is_vscode_mode())
    {
        if let Some(layout) = last_layout {
            let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
            let editor_row = row.saturating_sub(menu_rows);
            let mut found = false;
            for rw in &layout.windows {
                let wx = rw.rect.x as u16;
                let wy = rw.rect.y as u16;
                let ww = rw.rect.width as u16;
                let wh = rw.rect.height as u16;
                let gutter = rw.gutter_char_width as u16;
                let rel_col = col - editor_left;
                if rel_col >= wx + gutter
                    && rel_col < wx + ww
                    && editor_row >= wy
                    && editor_row < wy + wh
                {
                    let view_row = (editor_row - wy) as usize;
                    let buf_line = rw
                        .lines
                        .get(view_row)
                        .map(|l| l.line_idx)
                        .unwrap_or_else(|| rw.scroll_top + view_row);
                    let text_col = (rel_col - wx).saturating_sub(gutter) as usize + rw.scroll_left;
                    engine.editor_hover_mouse_move(buf_line, text_col, mouse_on_editor_hover);
                    found = true;
                    break;
                }
            }
            if !found
                && engine.editor_hover.is_some()
                && !engine.editor_hover_has_focus
                && !mouse_on_editor_hover
            {
                engine.dismiss_editor_hover();
            }
        }
    }

    // Only process left-click presses from here on
    if ev.kind != MouseEventKind::Down(MouseButton::Left) {
        return sidebar_width;
    }

    // ── Click on editor hover popup link → execute command or copy URL ─────
    if mouse_on_editor_hover && !editor_hover_link_rects.is_empty() {
        for &(lx, ly, lw, _lh, ref url) in editor_hover_link_rects {
            if row == ly && col >= lx && col < lx + lw {
                if url.starts_with("command:") {
                    engine.execute_hover_goto(url);
                } else {
                    tui_copy_to_clipboard(url, engine);
                    engine.dismiss_editor_hover();
                }
                return sidebar_width;
            }
        }
    }
    // ── Click on editor hover popup → focus or start selection ─────────────
    if mouse_on_editor_hover && engine.editor_hover.is_some() {
        if engine.editor_hover_has_focus {
            // Already focused — start text selection
            if let Some((px, py, _pw, _ph)) = editor_hover_popup_rect {
                let scroll = engine
                    .editor_hover
                    .as_ref()
                    .map(|h| h.scroll_top)
                    .unwrap_or(0);
                let content_line = (row.saturating_sub(py + 1)) as usize + scroll;
                let content_col = col.saturating_sub(px + 2) as usize;
                engine.editor_hover_start_selection(content_line, content_col);
                *hover_selecting = true;
            }
        } else {
            engine.editor_hover_focus();
        }
        return sidebar_width;
    }
    // Click elsewhere dismisses editor hover but lets the click fall through
    // so the cursor moves to the clicked position (instead of requiring a second click).
    if engine.editor_hover.is_some() && !mouse_on_editor_hover {
        engine.dismiss_editor_hover();
    }

    // ── Command line click — start text selection ──────────────────────────────
    // Skip when click is in the activity bar column (settings button lives there).
    {
        use crate::core::Mode;
        if row + 1 == term_height
            && col >= ab_width
            && matches!(engine.mode, Mode::Command | Mode::Search)
        {
            let char_idx = col as usize;
            let buf_len = engine.command_buffer.chars().count();
            engine.command_cursor = char_idx.saturating_sub(1).min(buf_len);
            *cmd_sel = Some((char_idx, char_idx));
            *cmd_dragging = true;
            return sidebar_width;
        }
        // Also allow selection on the message/command line in Normal mode.
        if row + 1 == term_height
            && col >= ab_width
            && matches!(
                engine.mode,
                Mode::Normal | Mode::Visual | Mode::VisualLine | Mode::VisualBlock
            )
            && !engine.message.is_empty()
        {
            let char_idx = col as usize;
            *cmd_sel = Some((char_idx, char_idx));
            *cmd_dragging = true;
            debug_log!(
                "MSG_SEL start: col={} msg={:?}",
                char_idx,
                &engine.message[..engine.message.len().min(40)]
            );
            return sidebar_width;
        }
    }

    // ── Status bar branch click — open branch picker ───────────────────────
    // (only when global status bar exists — per-window status replaces it)
    // Skip when click is in the activity bar column (settings button lives there).
    if row + 2 == term_height && !engine.settings.window_status_line && col >= ab_width {
        if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
            if let Some(layout) = last_layout {
                if let Some((start, end)) = layout.status_branch_range {
                    let click_col = col as usize;
                    if click_col >= start && click_col < end {
                        engine.open_picker(crate::core::engine::PickerSource::GitBranches);
                        return sidebar_width;
                    }
                }
            }
        }
        return sidebar_width;
    }

    // Bottom row is cmd — ignore (but not in the activity bar column)
    if row + 1 >= term_height && col >= ab_width {
        return sidebar_width;
    }

    // ── Menu bar row click ─────────────────────────────────────────────────────
    if engine.menu_bar_visible && row == 0 {
        let mut col_pos: u16 = 1; // 1-cell left pad
        for (idx, (name, _, _)) in render::MENU_STRUCTURE.iter().enumerate() {
            let item_w = name.chars().count() as u16 + 2; // space + name + space
            if col >= col_pos && col < col_pos + item_w {
                if engine.menu_open_idx == Some(idx) {
                    engine.close_menu();
                } else {
                    engine.open_menu(idx);
                }
                return sidebar_width;
            }
            col_pos += item_w;
        }
        // Nav arrows + search box are centered between menu_end and right edge.
        let menu_end = col_pos;
        let arrows_w: u16 = 4; // "◀ ▶ "
        let title = engine
            .cwd
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "VimCode".to_string());
        let display = format!("\u{1f50d} {}", title);
        let text_len = display.chars().count() as u16;
        let box_width = if !title.is_empty() { text_len + 4 } else { 0 };
        let gap: u16 = if box_width > 0 { 1 } else { 0 };
        let total_unit = arrows_w + gap + box_width;
        let term_w = terminal_size.map(|r| r.width).unwrap_or(80);
        let available = term_w.saturating_sub(menu_end);
        if available >= total_unit + 2 {
            let unit_start = menu_end + (available - total_unit) / 2;
            // Back arrow at unit_start, forward at unit_start+2
            if col == unit_start {
                engine.tab_nav_back();
                return sidebar_width;
            }
            if col == unit_start + 2 {
                engine.tab_nav_forward();
                return sidebar_width;
            }
            // Search box area: from arrows_w past unit_start to end of box
            let search_start = unit_start + arrows_w + gap;
            let search_end = unit_start + total_unit;
            if col >= search_start && col < search_end {
                engine.open_command_center();
                return sidebar_width;
            }
        }
        engine.close_menu(); // click in empty area of menu bar
        return sidebar_width;
    }

    // ── Menu dropdown item click ───────────────────────────────────────────────
    if let Some(open_idx) = engine.menu_open_idx {
        if let Some((_, _, items)) = render::MENU_STRUCTURE.get(open_idx) {
            // Determine the dropdown anchor column (same formula as render_menu_dropdown)
            let mut popup_col: u16 = 1;
            for i in 0..open_idx {
                if let Some((name, _, _)) = render::MENU_STRUCTURE.get(i) {
                    popup_col += name.chars().count() as u16 + 2;
                }
            }
            let max_label = items.iter().map(|i| i.label.len()).max().unwrap_or(4);
            let max_shortcut = items.iter().map(|i| i.shortcut.len()).max().unwrap_or(0);
            let popup_width = (max_label + max_shortcut + 6).clamp(20, 50) as u16;
            let popup_x = popup_col.min(term_height.saturating_sub(popup_width));
            // Dropdown rows: border(1) + items
            let menu_bar_row: u16 = if engine.menu_bar_visible { 1 } else { 0 };
            let popup_y = menu_bar_row; // dropdown starts below menu bar
            if row > popup_y && col >= popup_x && col < popup_x + popup_width {
                let item_idx = (row - popup_y - 1) as usize;
                if item_idx < items.len() && !items[item_idx].separator && items[item_idx].enabled {
                    let action = items[item_idx].action.to_string();
                    if action == "open_file_dialog" {
                        engine.close_menu();
                        engine.open_picker(crate::core::engine::PickerSource::Files);
                    } else {
                        let act = engine.menu_activate_item(open_idx, item_idx, &action);
                        if act == EngineAction::OpenTerminal {
                            let cols = terminal_size.map(|s| s.width).unwrap_or(80);
                            engine.terminal_new_tab(cols, engine.session.terminal_panel_rows);
                        } else if let EngineAction::RunInTerminal(cmd) = act {
                            let cols = terminal_size.map(|s| s.width).unwrap_or(80);
                            engine.terminal_run_command(
                                &cmd,
                                cols,
                                engine.session.terminal_panel_rows,
                            );
                        } else if act == EngineAction::OpenFolderDialog {
                            *folder_picker = Some(FolderPickerState::new(
                                &engine.cwd.clone(),
                                FolderPickerMode::OpenFolder,
                                engine.settings.show_hidden_files,
                            ));
                        } else if act == EngineAction::OpenWorkspaceDialog {
                            // open_workspace_from_file() already ran in the engine;
                            // refresh the sidebar to reflect the new cwd.
                            *sidebar = TuiSidebar::new(engine.cwd.clone(), sidebar.visible);
                            sidebar.show_hidden_files = engine.settings.show_hidden_files;
                        } else if act == EngineAction::SaveWorkspaceAsDialog {
                            let ws_path = engine.cwd.join(".vimcode-workspace");
                            engine.save_workspace_as(&ws_path);
                        } else if act == EngineAction::OpenRecentDialog {
                            *folder_picker = Some(FolderPickerState::new_recent(
                                &engine.session.recent_workspaces,
                            ));
                        } else if act == EngineAction::QuitWithUnsaved {
                            *quit_confirm = true;
                        } else if act == EngineAction::ToggleSidebar {
                            sidebar.visible = !sidebar.visible;
                        } else if handle_action(engine, act) {
                            *should_quit = true;
                        }
                    }
                }
                return sidebar_width;
            }
            // Click outside dropdown — close it
            engine.close_menu();
        }
    }

    // ── Debug toolbar row click ────────────────────────────────────────────────
    if engine.debug_toolbar_visible {
        let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
        let strip_rows: u16 = if engine.terminal_open {
            engine.session.terminal_panel_rows + 1
        } else {
            0
        };
        let toolbar_row = term_height.saturating_sub(3 + qf_rows + strip_rows);
        if row == toolbar_row {
            let mut col_pos: u16 = 1;
            for (idx, btn) in render::DEBUG_BUTTONS.iter().enumerate() {
                if idx == 4 {
                    col_pos += 2; // separator gap
                }
                let btn_w = (btn.icon.chars().count() + btn.key_hint.chars().count() + 4) as u16;
                if col >= col_pos && col < col_pos + btn_w {
                    let _ = engine.execute_command(btn.action);
                    return sidebar_width;
                }
                col_pos += btn_w;
            }
            return sidebar_width; // click in toolbar row, consume event
        }
    }

    // ── Bottom panel tab bar click (shared row above Terminal / Debug Output) ──
    {
        let bottom_panel_visible = engine.terminal_open || engine.bottom_panel_open;
        if bottom_panel_visible && col >= editor_left {
            let dt_rows: u16 = if engine.debug_toolbar_visible { 1 } else { 0 };
            let wildmenu_rows: u16 = if !engine.wildmenu_items.is_empty() {
                1
            } else {
                0
            };
            let global_status_rows: u16 = if engine.settings.window_status_line {
                0
            } else {
                1
            };
            let panel_height = engine.session.terminal_panel_rows + 2;
            // Bottom panel y = term_height - cmd(1) - status - wildmenu - debug_toolbar - panel
            let tab_bar_row = term_height
                .saturating_sub(1 + global_status_rows + wildmenu_rows + dt_rows + panel_height);
            if row == tab_bar_row {
                let term_width = terminal_size.map(|s| s.width).unwrap_or(80);
                let rel_col = col - editor_left; // column relative to editor area
                                                 // Close button (×) at rightmost 2 cols of editor area
                if col >= term_width.saturating_sub(2) {
                    engine.bottom_panel_open = false;
                    engine.close_terminal();
                    return sidebar_width;
                }
                // Tab label click — switch between Terminal and Debug Output.
                // Labels: "  Terminal  " (12 chars), "  Debug Output  " (16 chars)
                if rel_col < 12 {
                    engine.bottom_panel_kind = render::BottomPanelKind::Terminal;
                } else if rel_col < 28 {
                    engine.bottom_panel_kind = render::BottomPanelKind::DebugOutput;
                }
                return sidebar_width;
            }
        }
    }

    // ── Debug output panel click (scrollbar) ──────────────────────────────────
    {
        let debug_output_open = engine.bottom_panel_kind == render::BottomPanelKind::DebugOutput
            && !engine.dap_output_lines.is_empty();
        if debug_output_open {
            let dt_rows: u16 = if engine.debug_toolbar_visible { 1 } else { 0 };
            let panel_height = engine.session.terminal_panel_rows + 2;
            let panel_y = term_height.saturating_sub(2 + dt_rows + panel_height);
            let panel_end = term_height.saturating_sub(2 + dt_rows);
            if row >= panel_y && row < panel_end {
                let term_width = terminal_size.map(|s| s.width).unwrap_or(80);
                let sb_col = term_width.saturating_sub(1);
                let total = engine.dap_output_lines.len();
                let content_rows = engine.session.terminal_panel_rows as usize;
                if total > content_rows && col == sb_col && row >= panel_y + 2 {
                    // Click on scrollbar track — start drag.
                    let track_start = panel_y + 2; // after tab-bar row + header row
                    let track_len = engine.session.terminal_panel_rows;
                    *dragging_debug_output_sb = Some((track_start, track_len, total));
                }
                return sidebar_width;
            }
        }
    }
    // ── Terminal panel click ───────────────────────────────────────────────────
    {
        let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
        let strip_rows: u16 = if engine.terminal_open {
            engine.session.terminal_panel_rows + 1
        } else {
            0
        };
        let term_strip_top = term_height.saturating_sub(2 + qf_rows + strip_rows);
        if engine.terminal_open
            && strip_rows > 0
            && row >= term_strip_top
            && row < term_strip_top + strip_rows
        {
            if row == term_strip_top {
                // Header row — tab switch, toolbar buttons, or resize drag.
                engine.terminal_has_focus = true;
                const TERMINAL_TAB_COLS: u16 = 4;
                let tab_count = engine.terminal_panes.len() as u16;
                let term_width = terminal_size.map(|s| s.width).unwrap_or(80);
                if tab_count > 0 && col < tab_count * TERMINAL_TAB_COLS {
                    engine.terminal_switch_tab((col / TERMINAL_TAB_COLS) as usize);
                } else if col >= term_width.saturating_sub(2) {
                    // Close icon (rightmost 2 cols)
                    engine.terminal_close_active_tab();
                } else if col >= term_width.saturating_sub(4) {
                    // Split button (2 cols left of close)
                    let full_cols = terminal_size.map(|s| s.width).unwrap_or(80);
                    let rows = engine.session.terminal_panel_rows;
                    engine.terminal_toggle_split(full_cols, rows);
                } else if col >= term_width.saturating_sub(6) {
                    // Add button (2 cols left of split)
                    let cols = terminal_size.map(|s| s.width).unwrap_or(80);
                    let rows = engine.session.terminal_panel_rows;
                    engine.terminal_new_tab(cols, rows);
                } else {
                    *dragging_terminal_resize = true;
                }
            } else {
                // Content row — focus split pane or start divider drag.
                if engine.terminal_split && engine.terminal_panes.len() >= 2 {
                    // Mirror render.rs: use drag-override if set, else actual PTY cols.
                    let div_col = if engine.terminal_split_left_cols > 0 {
                        engine.terminal_split_left_cols
                    } else {
                        engine.terminal_panes[0].cols
                    };
                    // Allow clicking within ±1 column of the divider to start a resize drag.
                    if col.abs_diff(div_col) <= 1 {
                        engine.terminal_has_focus = true;
                        *dragging_terminal_split = true;
                        return sidebar_width; // skip selection start
                    } else {
                        engine.terminal_active = if col < div_col { 0 } else { 1 };
                    }
                }
                // Check for scrollbar click first.
                let term_width = terminal_size.map(|s| s.width).unwrap_or(80);
                let sb_col = term_width.saturating_sub(1);
                engine.terminal_has_focus = true;
                if col == sb_col {
                    // Scrollbar column — start drag.
                    let track_start = term_strip_top + 1;
                    let track_len = strip_rows.saturating_sub(1); // content rows
                                                                  // Cap total to one screenful (vt100 API limit) so the drag range
                                                                  // [0, total] exactly matches what set_scroll_offset can deliver.
                    let total = engine
                        .active_terminal()
                        .map(|t| t.history.len())
                        .unwrap_or(0);
                    *dragging_terminal_sb = Some((track_start, track_len, total));
                } else {
                    // Content area — start a selection.
                    let term_row = row - term_strip_top - 1;
                    engine.terminal_scroll_reset();
                    if let Some(term) = engine.active_terminal_mut() {
                        term.selection = Some(crate::core::terminal::TermSelection {
                            start_row: term_row,
                            start_col: col,
                            end_row: term_row,
                            end_col: col,
                        });
                    }
                }
            }
            return sidebar_width;
        }
    }
    // Click landed outside the terminal panel — return focus to the editor.
    engine.terminal_has_focus = false;

    // ── Activity bar ──────────────────────────────────────────────────────────
    if col < ab_width {
        // Activity bar spans full height below the menu bar row (matching GTK layout).
        let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
        // Activity bar starts at row `menu_rows` in absolute terminal coordinates.
        if row < menu_rows {
            return sidebar_width; // click in menu bar area, ignore
        }
        let bar_row = row - menu_rows; // row relative to activity bar start
        let bar_height = term_height.saturating_sub(menu_rows);
        let settings_row = bar_height.saturating_sub(1);
        // Row 0: hamburger (menu bar toggle)
        if bar_row == 0 {
            engine.toggle_menu_bar();
            return sidebar_width;
        }
        let target_panel = match bar_row {
            1 => Some(TuiPanel::Explorer),
            2 => Some(TuiPanel::Search),
            3 => Some(TuiPanel::Debug),
            4 => Some(TuiPanel::Git),
            5 => Some(TuiPanel::Extensions),
            6 => Some(TuiPanel::Ai),
            r if r == settings_row && settings_row >= 7 => Some(TuiPanel::Settings),
            _ => None,
        };
        // Check if click is on an extension panel icon (rows 7+)
        if target_panel.is_none() && bar_row >= 7 {
            let ext_idx = (bar_row - 7) as usize;
            let mut ext_names: Vec<_> = engine.ext_panels.keys().cloned().collect();
            ext_names.sort();
            if ext_idx < ext_names.len() {
                let name = ext_names[ext_idx].clone();
                if sidebar.ext_panel_name.as_deref() == Some(&name) && sidebar.visible {
                    sidebar.visible = false;
                    sidebar.ext_panel_name = None;
                    engine.ext_panel_has_focus = false;
                    engine.ext_panel_active = None;
                } else {
                    sidebar.ext_panel_name = Some(name.clone());
                    sidebar.visible = true;
                    sidebar.has_focus = true;
                    engine.ext_panel_active = Some(name.clone());
                    engine.ext_panel_has_focus = true;
                    engine.ext_panel_selected = 0;
                    engine.plugin_event("panel_focus", &name);
                }
                engine.session.explorer_visible = sidebar.visible;
                let _ = engine.session.save();
                return sidebar_width;
            }
        }
        if let Some(panel) = target_panel {
            // Clear extension panel state when switching to a built-in panel
            sidebar.ext_panel_name = None;
            engine.ext_panel_has_focus = false;
            engine.ext_panel_active = None;
            if sidebar.active_panel == panel && sidebar.visible {
                sidebar.visible = false;
            } else {
                sidebar.active_panel = panel;
                sidebar.visible = true;
                if panel == TuiPanel::Search {
                    sidebar.has_focus = true;
                    sidebar.search_input_mode = true;
                }
                if panel == TuiPanel::Git {
                    engine.sc_refresh();
                }
                if panel == TuiPanel::Extensions {
                    engine.ext_sidebar_has_focus = true;
                    if engine.ext_registry.is_none() && !engine.ext_registry_fetching {
                        engine.ext_refresh();
                    }
                    sidebar.has_focus = true;
                }
                if panel == TuiPanel::Ai {
                    engine.ai_has_focus = true;
                    sidebar.has_focus = true;
                }
                if panel == TuiPanel::Settings {
                    engine.settings_has_focus = true;
                    sidebar.has_focus = true;
                }
            }
            engine.session.explorer_visible = sidebar.visible;
            let _ = engine.session.save();
        }
        return sidebar_width;
    }

    // ── Sidebar panel area ────────────────────────────────────────────────────
    if sidebar.visible && col < ab_width + sidebar_width {
        // Rightmost column of the sidebar is the scrollbar column.
        let sb_col = ab_width + sidebar_width - 1;
        // Account for menu bar: when visible it occupies absolute row 0, so the
        // sidebar's logical row 0 is at absolute terminal row `menu_rows`.
        let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
        let sidebar_row = row.saturating_sub(menu_rows);

        // Extension panel must be checked FIRST — ext_panel_name overrides active_panel
        if sidebar.ext_panel_name.is_some() {
            sidebar.has_focus = true;
            engine.ext_panel_has_focus = true;

            // Account for the search input row when it's visible
            let input_rows: u16 = if engine.ext_panel_input_active
                || engine
                    .ext_panel_active
                    .as_ref()
                    .and_then(|n| engine.ext_panel_input_text.get(n))
                    .map(|t| !t.is_empty())
                    .unwrap_or(false)
            {
                1
            } else {
                0
            };
            let content_start = 1 + input_rows; // header + optional input

            // Right-click fires panel_context_menu event.
            if ev.kind == MouseEventKind::Down(MouseButton::Right) {
                if sidebar_row >= content_start {
                    let flat_idx =
                        engine.ext_panel_scroll_top + (sidebar_row - content_start) as usize;
                    let flat_len = engine.ext_panel_flat_len();
                    if flat_idx < flat_len {
                        engine.ext_panel_selected = flat_idx;
                    }
                }
                engine.open_ext_panel_context_menu(col, row);
                return sidebar_width;
            }

            // Scrollbar click/drag → jump-scroll + arm drag
            let flat_len = engine.ext_panel_flat_len();
            let content_h = term_height.saturating_sub(2 + menu_rows + content_start) as usize;
            if col == sb_col && flat_len > content_h && sidebar_row >= content_start {
                let rel_row = (sidebar_row - content_start) as usize;
                let ratio = rel_row as f64 / content_h as f64;
                let new_top = (ratio * flat_len as f64) as usize;
                engine.ext_panel_scroll_top = new_top.min(flat_len.saturating_sub(1));
                *dragging_generic_sb = Some(SidebarScrollDrag {
                    track_abs_start: content_start + menu_rows,
                    track_len: content_h as u16,
                    total: flat_len,
                });
                return sidebar_width;
            }

            if sidebar_row == 0 {
                // Header — no-op
            } else if sidebar_row >= content_start {
                // Map sidebar_row to flat index
                let flat_idx = engine.ext_panel_scroll_top + (sidebar_row - content_start) as usize;
                if flat_idx < flat_len {
                    engine.ext_panel_selected = flat_idx;
                    // Check for double-click
                    let now = Instant::now();
                    let is_double = now.duration_since(*last_click_time)
                        < Duration::from_millis(400)
                        && *last_click_pos == (col, row);
                    *last_click_time = now;
                    *last_click_pos = (col, row);
                    if is_double {
                        engine.handle_ext_panel_double_click();
                    }
                    // Single-click toggles sections/expandable items
                    engine.handle_ext_panel_key("Return", false, None);
                }
            }
        } else if sidebar.active_panel == TuiPanel::Explorer {
            sidebar.has_focus = true;
            engine.explorer_has_focus = true;
            // tree_height = total height - 2 status rows (no header)
            let tree_height = term_height.saturating_sub(2) as usize;
            let total_rows = sidebar.rows.len();

            // Click on the scrollbar column → jump-scroll + arm drag
            if col == sb_col && total_rows > tree_height {
                let rel_row = sidebar_row as usize;
                let ratio = rel_row as f64 / tree_height as f64;
                let new_top = (ratio * total_rows as f64) as usize;
                sidebar.scroll_top = new_top.min(total_rows.saturating_sub(tree_height));
                let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                *dragging_generic_sb = Some(SidebarScrollDrag {
                    track_abs_start: menu_rows,
                    track_len: tree_height as u16,
                    total: total_rows,
                });
                return sidebar_width;
            }

            let tree_row = sidebar_row as usize + sidebar.scroll_top;
            if tree_row < sidebar.rows.len() {
                // Record potential drag source for DnD.
                *explorer_drag_src = Some(tree_row);
                if sidebar.rows[tree_row].is_dir {
                    sidebar.selected = tree_row;
                    sidebar.toggle_dir(tree_row);
                } else {
                    sidebar.selected = tree_row;
                    let path = sidebar.rows[tree_row].path.clone();
                    let now = Instant::now();
                    let is_double = now.duration_since(*last_click_time)
                        < Duration::from_millis(400)
                        && *last_click_pos == (col, row);
                    *last_click_time = now;
                    *last_click_pos = (col, row);
                    if is_double {
                        engine.open_file_in_tab(&path);
                    } else {
                        engine.open_file_preview(&path);
                    }
                }
            }
        } else if sidebar.active_panel == TuiPanel::Debug {
            use crate::core::engine::DebugSidebarSection;
            sidebar.has_focus = true;
            engine.dap_sidebar_has_focus = true;

            if sidebar_row == 0 {
                // Header row — no-op
            } else if sidebar_row == 1 {
                // Run/Stop button
                if engine.dap_session_active && engine.dap_stopped_thread.is_some() {
                    engine.dap_continue();
                } else if engine.dap_session_active {
                    engine.execute_command("stop");
                } else {
                    engine.execute_command("debug");
                }
            } else {
                // Walk sections using fixed-allocation layout:
                // row 2+ = [section_header(1) + content(height)]×4
                let sections = [
                    (DebugSidebarSection::Variables, 0usize),
                    (DebugSidebarSection::Watch, 1),
                    (DebugSidebarSection::CallStack, 2),
                    (DebugSidebarSection::Breakpoints, 3),
                ];
                let mut cur_row: u16 = 2;
                for (section, sec_idx) in &sections {
                    let sec_height = engine.dap_sidebar_section_heights[*sec_idx];
                    let section_header_row = cur_row;
                    let items_start = cur_row + 1;
                    let items_end = items_start + sec_height;

                    if sidebar_row == section_header_row {
                        engine.dap_sidebar_section = *section;
                        engine.dap_sidebar_selected = 0;
                        break;
                    } else if sidebar_row >= items_start && sidebar_row < items_end {
                        let item_count = engine.dap_sidebar_section_item_count(*section);
                        let height = sec_height as usize;
                        let sb_col = ab_width + sidebar_width - 1;
                        // Scrollbar click: rightmost column when items overflow.
                        if col == sb_col && item_count > height && height > 0 {
                            let rel_row = (sidebar_row - items_start) as usize;
                            let ratio = rel_row as f64 / height as f64;
                            let max_scroll = item_count.saturating_sub(height);
                            engine.dap_sidebar_scroll[*sec_idx] =
                                (ratio * max_scroll as f64) as usize;
                            engine.dap_sidebar_section = *section;
                            // Arm drag state for subsequent Drag events.
                            *dragging_debug_sb = Some(DebugSidebarScrollDrag {
                                sec_idx: *sec_idx,
                                track_abs_start: items_start + menu_rows,
                                track_len: sec_height,
                                total: item_count,
                            });
                        } else {
                            let scroll_off = engine.dap_sidebar_scroll[*sec_idx];
                            let row_offset = (sidebar_row - items_start) as usize;
                            let item_idx = scroll_off + row_offset;
                            if item_count > 0 && item_idx < item_count {
                                engine.dap_sidebar_section = *section;
                                engine.dap_sidebar_selected = item_idx;
                                engine.handle_debug_sidebar_key("Return", false);
                            }
                        }
                        break;
                    }
                    cur_row = items_end;
                }
            }
            return sidebar_width;
        } else if sidebar.active_panel == TuiPanel::Git {
            sidebar.has_focus = true;
            engine.sc_has_focus = true;

            // sidebar_row layout:
            //   0 = header
            //   1 .. commit_rows = commit input
            //   1+commit_rows = pad above
            //   2+commit_rows = button row
            //   3+commit_rows = pad below
            //   4+commit_rows .. = sections
            let commit_rows = engine.sc_commit_message.split('\n').count().max(1) as u16;
            let commit_end = 1 + commit_rows; // first row after commit input
            let btn_row = 2 + commit_rows; // pad_above + 1
            let section_start = 4 + commit_rows; // btn + pad_below + 1
            if sidebar_row == 0 {
                // Panel header — no-op
                engine.sc_commit_input_active = false;
            } else if sidebar_row >= 1 && sidebar_row < commit_end {
                // Commit input row(s) — enter commit mode
                engine.sc_commit_input_active = true;
                engine.sc_commit_cursor = engine.sc_commit_message.len();
            } else if sidebar_row == btn_row {
                engine.sc_commit_input_active = false;
                // Button row: Commit (~50%), Push/Pull/Sync (~17% each, icon-only).
                // Use column relative to the sidebar content area start.
                let rel_col = col.saturating_sub(ab_width);
                let commit_w = sidebar_width / 2;
                let btn_idx = if rel_col < commit_w {
                    0
                } else {
                    let icon_w = (sidebar_width - commit_w) / 3;
                    let x = rel_col - commit_w;
                    (1 + (x / icon_w.max(1))).min(3) as usize
                };
                engine.sc_activate_button(btn_idx);
            } else if sidebar_row >= section_start {
                engine.sc_commit_input_active = false;
                // Sections area — map to flat index.
                // sc_visual_row_to_flat expects: 0=header,1=commit,2=buttons,3+=sections.
                let adjusted = sidebar_row - section_start + 3;
                // TUI shows a "(no changes)" hint for expanded-but-empty sections
                // (extra visual row with no flat-index entry), so empty_section_hint = true.
                if let Some((flat_idx, is_header)) =
                    engine.sc_visual_row_to_flat(adjusted as usize, true)
                {
                    engine.sc_selected = flat_idx;
                    if is_header {
                        engine.handle_sc_key("Tab", false, None);
                    } else {
                        let now = Instant::now();
                        let is_double = now.duration_since(*last_click_time)
                            < Duration::from_millis(400)
                            && *last_click_pos == (col, row);
                        *last_click_time = now;
                        *last_click_pos = (col, row);
                        if is_double {
                            engine.sc_open_selected_async();
                            engine.sc_has_focus = true;
                            sidebar.has_focus = true;
                        }
                    }
                }
            }
            return sidebar_width;
        } else if sidebar.active_panel == TuiPanel::Search {
            sidebar.has_focus = true;
            // results_height = (total height - 2 status rows) - 5 panel header rows
            let results_height = term_height.saturating_sub(7) as usize;
            let results = &engine.project_search_results;

            // Click on the scrollbar column in the results area → jump-scroll
            if col == sb_col && !results.is_empty() && sidebar_row >= 5 {
                // Count total display rows (result rows + file header rows)
                let total_display = {
                    let mut count = 0usize;
                    let mut last_file: Option<&std::path::Path> = None;
                    for m in results.iter() {
                        if last_file != Some(m.file.as_path()) {
                            last_file = Some(m.file.as_path());
                            count += 1;
                        }
                        count += 1;
                    }
                    count
                };
                if total_display > results_height {
                    let rel_row = sidebar_row.saturating_sub(5) as usize;
                    let ratio = rel_row as f64 / results_height as f64;
                    let new_scroll = (ratio * total_display as f64) as usize;
                    sidebar.search_scroll_top =
                        new_scroll.min(total_display.saturating_sub(results_height));
                    // Arm drag state so subsequent Drag events continue scrolling.
                    // track_abs_start is the absolute terminal row of the track top.
                    *dragging_sidebar_search = Some(SidebarScrollDrag {
                        track_abs_start: 5 + menu_rows,
                        track_len: results_height as u16,
                        total: total_display,
                    });
                }
                return sidebar_width;
            }

            // sidebar_rows 0-2: header + search + replace inputs — clicking enters input mode
            if sidebar_row <= 2 {
                sidebar.search_input_mode = true;
                sidebar.replace_input_focused = sidebar_row == 2;
            } else {
                sidebar.search_input_mode = false;
                sidebar.replace_input_focused = false;
                // sidebar_row 3 = toggles, 4 = status line; 5+ = results area
                // Add scroll offset so clicks map to the correct result.
                let content_row =
                    (sidebar_row as usize).saturating_sub(5) + sidebar.search_scroll_top;
                if !results.is_empty() {
                    let selected = visual_row_to_result_idx(results, content_row);
                    if let Some(idx) = selected {
                        engine.project_search_selected = idx;
                        // Open the file immediately on click
                        let result = engine
                            .project_search_results
                            .get(idx)
                            .map(|m| (m.file.clone(), m.line));
                        if let Some((file, line)) = result {
                            engine.open_file_in_tab(&file);
                            let win_id = engine.active_window_id();
                            engine.set_cursor_for_window(win_id, line, 0);
                            engine.ensure_cursor_visible();
                            sidebar.has_focus = false;
                        }
                    }
                }
            }
        } else if sidebar.active_panel == TuiPanel::Extensions {
            sidebar.has_focus = true;
            engine.ext_sidebar_has_focus = true;

            // Row layout: 0=header, 1=search, 2=INSTALLED header, 3..=items/headers
            if sidebar_row == 0 {
                // Header — no-op
            } else if sidebar_row == 1 {
                // Search box — activate search input
                engine.ext_sidebar_input_active = true;
            } else {
                let installed = engine.ext_installed_items();
                let installed_len = if engine.ext_sidebar_sections_expanded[0] {
                    installed.len()
                } else {
                    0
                };
                let installed_header_row: u16 = 2;
                let installed_display =
                    installed_len.max(if engine.ext_sidebar_sections_expanded[0] {
                        1
                    } else {
                        0
                    });
                let available_header_row = installed_header_row + 1 + installed_display as u16;

                if sidebar_row == installed_header_row {
                    engine.ext_sidebar_sections_expanded[0] =
                        !engine.ext_sidebar_sections_expanded[0];
                } else if sidebar_row > installed_header_row
                    && sidebar_row < available_header_row
                    && installed_len > 0
                {
                    let idx = (sidebar_row - installed_header_row - 1) as usize;
                    if idx < installed_len {
                        let now = Instant::now();
                        let is_double = now.duration_since(*last_click_time)
                            < Duration::from_millis(400)
                            && *last_click_pos == (col, row);
                        *last_click_time = now;
                        *last_click_pos = (col, row);
                        engine.ext_sidebar_selected = idx;
                        if is_double {
                            engine.ext_open_selected_readme();
                        }
                    }
                } else if sidebar_row == available_header_row {
                    engine.ext_sidebar_sections_expanded[1] =
                        !engine.ext_sidebar_sections_expanded[1];
                } else if sidebar_row > available_header_row {
                    let avail_len = if engine.ext_sidebar_sections_expanded[1] {
                        engine.ext_available_items().len()
                    } else {
                        0
                    };
                    let avail_idx = (sidebar_row - available_header_row - 1) as usize;
                    if avail_idx < avail_len {
                        let now = Instant::now();
                        let is_double = now.duration_since(*last_click_time)
                            < Duration::from_millis(400)
                            && *last_click_pos == (col, row);
                        *last_click_time = now;
                        *last_click_pos = (col, row);
                        engine.ext_sidebar_selected = installed_len + avail_idx;
                        if is_double {
                            // Double-click opens README
                            engine.ext_open_selected_readme();
                        }
                        // Single-click just selects
                    }
                }
            }
        } else if sidebar.active_panel == TuiPanel::Settings {
            sidebar.has_focus = true;
            engine.settings_has_focus = true;

            // Row 0: header, Row 1: search input, Row 2+: scrollable content
            let content_height = term_height.saturating_sub(4) as usize; // header+search+status+cmd
            let flat_total = engine.settings_flat_list().len();

            // Scrollbar column → jump-scroll + start drag
            if col == sb_col && sidebar_row >= 2 && flat_total > content_height {
                let track_start = row - (sidebar_row - 2);
                let track_len = content_height as u16;
                let rel = (sidebar_row - 2) as f64;
                let ratio = rel / track_len as f64;
                let max_scroll = flat_total.saturating_sub(content_height);
                engine.settings_scroll_top = (ratio * max_scroll as f64).round() as usize;
                *dragging_settings_sb = Some(SidebarScrollDrag {
                    track_abs_start: track_start,
                    track_len,
                    total: flat_total,
                });
            } else if sidebar_row == 0 {
                // Header — no-op
            } else if sidebar_row == 1 {
                // Search box — activate search input
                engine.settings_input_active = true;
            } else {
                let content_row = sidebar_row.saturating_sub(2) as usize;
                let fi = engine.settings_scroll_top + content_row;
                if fi < flat_total {
                    engine.settings_selected = fi;
                    // Double-click toggles bools / expands categories
                    let now = Instant::now();
                    let is_double = now.duration_since(*last_click_time)
                        < Duration::from_millis(400)
                        && *last_click_pos == (col, row);
                    *last_click_time = now;
                    *last_click_pos = (col, row);
                    if is_double {
                        engine.handle_settings_key("Return", false, None);
                    }
                }
            }
        }
        return sidebar_width;
    }

    // ── Editor area ───────────────────────────────────────────────────────────
    sidebar.has_focus = false;
    sidebar.toolbar_focused = false;
    engine.sc_has_focus = false;
    engine.dap_sidebar_has_focus = false;
    engine.ext_sidebar_has_focus = false;
    engine.ai_has_focus = false;
    engine.settings_has_focus = false;
    engine.ext_panel_has_focus = false;
    if col < editor_left {
        return sidebar_width; // separator column
    }

    // The menu bar (if visible) occupies absolute row 0, pushing the tab bar
    // and editor content down by `menu_rows`.
    let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };

    // ── Breadcrumb click ────────────────────────────────────────────────────
    if engine.settings.breadcrumbs {
        if let Some(layout) = last_layout {
            for bc in &layout.breadcrumbs {
                // Match the renderer: bc_y = editor_area.y + bounds.y - 1
                // where editor_area.y == menu_rows in TUI coordinates.
                let bc_row = if bc.bounds.y >= 1.0 {
                    menu_rows + bc.bounds.y as u16 - 1
                } else {
                    menu_rows
                };
                let bc_x = editor_left + bc.bounds.x as u16;
                let bc_w = bc.bounds.width as u16;
                if row == bc_row && col >= bc_x && col < bc_x + bc_w {
                    if !matches!(ev.kind, MouseEventKind::Down(MouseButton::Left)) {
                        return sidebar_width; // consume non-click events on breadcrumb row
                    }
                    let local_col = (col - bc_x) as usize;
                    let sep_len = 3; // " › "
                    let mut x = 1usize; // match left padding in renderer
                    engine.rebuild_breadcrumb_segments();
                    for (i, seg) in bc.segments.iter().enumerate() {
                        let label_len = seg.label.chars().count();
                        if local_col >= x && local_col < x + label_len {
                            engine.breadcrumb_selected = i;
                            engine.breadcrumb_open_scoped();
                            return sidebar_width;
                        }
                        x += label_len + sep_len;
                    }
                    return sidebar_width;
                }
            }
        }
    }

    // ── Tab bar click ──────────────────────────────────────────────────────
    // For split groups, any group's tab bar row is clickable (not just the top row).
    if let Some(layout) = last_layout {
        let rel_col = col - editor_left;

        if let Some(ref split) = layout.editor_group_split {
            // Find which group's tab bar row matches the clicked row.
            // Tab bar sits tab_bar_height rows above the group's window content.
            let click_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
            let mut matched_group = None;
            for gtb in split.group_tab_bars.iter() {
                if engine.is_tab_bar_hidden(gtb.group_id) {
                    continue;
                }
                let tab_bar_row = menu_rows + (gtb.bounds.y as u16).saturating_sub(click_tbh);
                let gx = gtb.bounds.x as u16;
                let gw = gtb.bounds.width as u16;
                if row == tab_bar_row && rel_col >= gx && rel_col < gx + gw {
                    let was_active = gtb.group_id == split.active_group;
                    matched_group = Some((
                        gtb.group_id,
                        rel_col - gx,
                        gw,
                        &gtb.tabs,
                        gtb.diff_toolbar.as_ref(),
                        was_active,
                        gtb.tab_scroll_offset,
                    ));
                    break;
                }
            }
            if let Some((
                group_id,
                local_col,
                bar_width,
                group_tabs,
                diff_toolbar_ref,
                was_active,
                scroll_offset,
            )) = matched_group
            {
                engine.active_group = group_id;

                let mut x: u16 = 0;
                let mut tab_matched = false;
                // Collect tab hit info from immutable borrow, then apply mutably.
                let mut hit_info: Option<(usize, bool)> = None;
                for (i, tab) in group_tabs.iter().enumerate().skip(scroll_offset) {
                    let name_width = tab.name.chars().count() as u16;
                    let tab_width = name_width + TAB_CLOSE_COLS;
                    if local_col >= x && local_col < x + tab_width {
                        tab_matched = true;
                        let valid = engine
                            .editor_groups
                            .get(&group_id)
                            .is_some_and(|g| i < g.tabs.len());
                        if valid {
                            let is_close = local_col >= x + name_width;
                            hit_info = Some((i, is_close));
                        }
                        break;
                    }
                    x += tab_width;
                }
                if let Some((tab_idx, is_close)) = hit_info {
                    engine.active_group = group_id;
                    if is_close {
                        if let Some(g) = engine.editor_groups.get_mut(&group_id) {
                            g.active_tab = tab_idx;
                        }
                        engine.line_annotations.clear();
                        if engine.dirty() {
                            *close_tab_confirm = true;
                        } else {
                            engine.close_tab();
                        }
                    } else {
                        engine.goto_tab(tab_idx);
                        // Record drag start position for tab drag-and-drop.
                        *tab_drag_start = Some((col, row));
                        engine.lsp_ensure_active_buffer();
                        if let Some(path) = engine.file_path().cloned() {
                            sidebar.reveal_path(&path, term_height.saturating_sub(4) as usize);
                        }
                    }
                }
                if !tab_matched {
                    // Calculate diff toolbar zone (label + 3 buttons).
                    let diff_total_cols = if let Some(dt) = diff_toolbar_ref {
                        let label_cols = dt
                            .change_label
                            .as_ref()
                            .map(|l| l.len() as u16 + 1)
                            .unwrap_or(0);
                        DIFF_TOOLBAR_BTN_COLS + label_cols
                    } else {
                        0
                    };
                    // Split buttons exist on active group, or all groups in diff mode.
                    let had_split = was_active || engine.is_in_diff_view();
                    let split_cols = if had_split { TAB_SPLIT_BOTH_COLS } else { 0 };
                    let split_end = bar_width.saturating_sub(TAB_ACTION_BTN_COLS);
                    let split_start = split_end.saturating_sub(split_cols);
                    let diff_end = split_start;
                    let diff_start = diff_end.saturating_sub(diff_total_cols);
                    // Hit-test diff toolbar buttons FIRST (they sit left of
                    // split buttons, so check them before split to avoid
                    // boundary overlap).
                    if diff_total_cols > 0 && local_col >= diff_start && local_col < diff_end {
                        // Hit-test diff toolbar buttons (prev, next, fold).
                        // Layout: [label][prev][next][fold].
                        let in_diff = local_col - diff_start;
                        let label_cols = diff_total_cols - DIFF_TOOLBAR_BTN_COLS;
                        let in_btns = in_diff.saturating_sub(label_cols);
                        let has_win = engine.windows.contains_key(&engine.active_window_id());
                        if in_diff < label_cols {
                            // Clicked on the label — no-op.
                        } else if in_btns < DIFF_BTN_COLS {
                            if has_win {
                                engine.jump_prev_hunk();
                            }
                        } else if in_btns < DIFF_BTN_COLS * 2 {
                            if has_win {
                                engine.jump_next_hunk();
                            }
                        } else {
                            engine.diff_toggle_hide_unchanged();
                        }
                    } else if had_split
                        && local_col >= split_start
                        && local_col < split_start + TAB_SPLIT_BOTH_COLS
                        && bar_width >= TAB_SPLIT_BOTH_COLS
                    {
                        // Hit-test split buttons.
                        let in_split = local_col - split_start;
                        if in_split >= TAB_SPLIT_BTN_COLS {
                            engine.open_editor_group(SplitDirection::Horizontal);
                        } else {
                            engine.open_editor_group(SplitDirection::Vertical);
                        }
                    } else if local_col >= bar_width.saturating_sub(TAB_ACTION_BTN_COLS) {
                        // Editor action menu button ("…") at far right.
                        engine.open_editor_action_menu(group_id, col, row + 1);
                    }
                }
                return sidebar_width;
            }
        }
        // Single group: check top tab bar row only.
        if row == menu_rows
            && layout.editor_group_split.is_none()
            && !engine.is_tab_bar_hidden(engine.active_group)
        {
            let editor_col_width = terminal_size
                .map(|s| s.width)
                .unwrap_or(80)
                .saturating_sub(editor_left);
            let bar_width = editor_col_width;
            let local_col = rel_col;
            let scroll_offset = layout.tab_scroll_offset;

            let mut x: u16 = 0;
            let mut tab_matched = false;
            for (i, tab) in layout.tab_bar.iter().enumerate().skip(scroll_offset) {
                let name_width = tab.name.chars().count() as u16;
                let tab_width = name_width + TAB_CLOSE_COLS;
                if local_col >= x && local_col < x + tab_width {
                    tab_matched = true;
                    if i < engine.active_group().tabs.len() {
                        let close_col = x + name_width;
                        if local_col >= close_col {
                            engine.active_group_mut().active_tab = i;
                            engine.line_annotations.clear();
                            if engine.dirty() {
                                *close_tab_confirm = true;
                            } else {
                                engine.close_tab();
                            }
                        } else {
                            engine.goto_tab(i);
                            // Record drag start position for tab drag-and-drop.
                            *tab_drag_start = Some((col, row));
                            engine.lsp_ensure_active_buffer();
                            if let Some(path) = engine.file_path().cloned() {
                                sidebar.reveal_path(&path, term_height.saturating_sub(4) as usize);
                            }
                        }
                    }
                    break;
                }
                x += tab_width;
            }
            if !tab_matched {
                let diff_total_cols = if let Some(dt) = layout.diff_toolbar.as_ref() {
                    let label_cols = dt
                        .change_label
                        .as_ref()
                        .map(|l| l.len() as u16 + 1)
                        .unwrap_or(0);
                    DIFF_TOOLBAR_BTN_COLS + label_cols
                } else {
                    0
                };
                let split_end = bar_width.saturating_sub(TAB_ACTION_BTN_COLS);
                let split_start = split_end.saturating_sub(TAB_SPLIT_BOTH_COLS);
                let diff_end = split_start;
                let diff_start = diff_end.saturating_sub(diff_total_cols);
                // Check diff toolbar FIRST to avoid boundary overlap with split buttons.
                if diff_total_cols > 0 && local_col >= diff_start && local_col < diff_end {
                    let in_diff = local_col - diff_start;
                    let label_cols = diff_total_cols - DIFF_TOOLBAR_BTN_COLS;
                    let in_btns = in_diff.saturating_sub(label_cols);
                    let has_win = engine.windows.contains_key(&engine.active_window_id());
                    if in_diff < label_cols {
                        // Clicked on label — no-op.
                    } else if in_btns < DIFF_BTN_COLS {
                        if has_win {
                            engine.jump_prev_hunk();
                        }
                    } else if in_btns < DIFF_BTN_COLS * 2 {
                        if has_win {
                            engine.jump_next_hunk();
                        }
                    } else {
                        engine.diff_toggle_hide_unchanged();
                    }
                } else if local_col >= split_start
                    && local_col < split_start + TAB_SPLIT_BOTH_COLS
                    && bar_width >= TAB_SPLIT_BOTH_COLS
                {
                    let in_split = local_col - split_start;
                    if in_split >= TAB_SPLIT_BTN_COLS {
                        engine.open_editor_group(SplitDirection::Horizontal);
                    } else {
                        engine.open_editor_group(SplitDirection::Vertical);
                    }
                } else if local_col >= bar_width.saturating_sub(TAB_ACTION_BTN_COLS) {
                    // Editor action menu button ("…") at far right.
                    engine.open_editor_action_menu(engine.active_group, col, row + 1);
                }
            }
            return sidebar_width;
        }
    }

    let rel_col = col - editor_left;
    // editor_row is 0-indexed relative to the editor content area.
    // Window rects already include the tab_bar_height offset (y >= 1),
    // so we only subtract menu_rows here (not the tab bar row).
    let editor_row = row.saturating_sub(menu_rows);

    // ── Group divider click — start drag ──────────────────────────────────────
    if let Some(layout) = last_layout {
        if let Some(ref split) = layout.editor_group_split {
            for div in &split.dividers {
                let hit = match div.direction {
                    crate::core::window::SplitDirection::Vertical => {
                        let div_col = div.position.round() as u16;
                        rel_col == div_col
                            && (editor_row as f64) >= div.cross_start
                            && (editor_row as f64) < div.cross_start + div.cross_size
                    }
                    crate::core::window::SplitDirection::Horizontal => {
                        let div_row = div.position.round() as u16;
                        editor_row == div_row
                            && (rel_col as f64) >= div.cross_start
                            && (rel_col as f64) < div.cross_start + div.cross_size
                    }
                };
                if hit {
                    *dragging_group_divider = Some(div.split_index);
                    return sidebar_width;
                }
            }
        }
    }

    if let Some(layout) = last_layout {
        for rw in &layout.windows {
            let wx = rw.rect.x as u16;
            let wy = rw.rect.y as u16;
            let ww = rw.rect.width as u16;
            let wh = rw.rect.height as u16;

            if rel_col >= wx && rel_col < wx + ww && editor_row >= wy && editor_row < wy + wh {
                // Per-window status bar click — hit-test segments for actions.
                if rw.status_line.is_some() && wh > 1 && editor_row == wy + wh - 1 {
                    if let Some(ref status) = rw.status_line {
                        let click_col = (rel_col - wx) as usize;
                        if let Some(action) =
                            status_segment_hit_test(status, ww as usize, click_col)
                        {
                            if let Some(ea) = engine.handle_status_action(&action) {
                                use crate::core::engine::EngineAction;
                                match ea {
                                    EngineAction::ToggleSidebar => {
                                        sidebar.visible = !sidebar.visible;
                                    }
                                    EngineAction::OpenTerminal => {
                                        let cols =
                                            terminal_size.as_ref().map(|s| s.width).unwrap_or(80);
                                        engine.terminal_new_tab(
                                            cols,
                                            engine.session.terminal_panel_rows,
                                        );
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    return sidebar_width;
                }

                let viewport_lines = wh as usize;
                let has_v_scrollbar = rw.total_lines > viewport_lines;
                let gutter = rw.gutter_char_width as u16;
                let viewport_cols = (ww as usize)
                    .saturating_sub(gutter as usize + if has_v_scrollbar { 1 } else { 0 });
                let has_h_scrollbar = rw.max_col > viewport_cols && wh > 1;

                // Vertical scrollbar click/drag-start (rightmost column)
                if has_v_scrollbar && rel_col == wx + ww - 1 {
                    // menu_rows = menu bar offset; wy already includes tab_bar_height
                    let track_abs_start = menu_rows + wy;
                    // If there's also a h-scrollbar, v-track is 1 row shorter
                    let track_len = if has_h_scrollbar {
                        wh.saturating_sub(1)
                    } else {
                        wh
                    };
                    *dragging_scrollbar = Some(ScrollDragState {
                        window_id: rw.window_id,
                        is_horizontal: false,
                        track_abs_start,
                        track_len,
                        total: rw.total_lines,
                    });
                    let track_rel_row = editor_row.saturating_sub(wy);
                    let ratio = track_rel_row as f64 / track_len as f64;
                    let new_top = (ratio * rw.total_lines as f64) as usize;
                    engine.set_scroll_top_for_window(rw.window_id, new_top);
                    engine.sync_scroll_binds();
                    return sidebar_width;
                }

                // Horizontal scrollbar click/drag-start (bottom row)
                if has_h_scrollbar && editor_row == wy + wh - 1 {
                    let track_x = wx + gutter;
                    let track_w = ww.saturating_sub(gutter + if has_v_scrollbar { 1 } else { 0 });
                    if rel_col >= track_x && rel_col < track_x + track_w && track_w > 0 {
                        let track_abs_start = editor_left + track_x;
                        *dragging_scrollbar = Some(ScrollDragState {
                            window_id: rw.window_id,
                            is_horizontal: true,
                            track_abs_start,
                            track_len: track_w,
                            total: rw.max_col,
                        });
                        let ratio = (rel_col - track_x) as f64 / track_w as f64;
                        let new_left = (ratio * rw.max_col as f64) as usize;
                        engine.set_scroll_left_for_window(rw.window_id, new_left);
                        return sidebar_width;
                    }
                }

                // Check gutter area
                let view_row = (editor_row - wy) as usize;
                if gutter > 0 && rel_col >= wx && rel_col < wx + gutter {
                    if let Some(rl) = rw.lines.get(view_row) {
                        let gutter_col = (rel_col - wx) as usize;
                        let bp_offset: usize = if rw.has_breakpoints { 1 } else { 0 };
                        let git_col = if rw.has_git_diff {
                            bp_offset
                        } else {
                            usize::MAX
                        };

                        if rw.has_breakpoints && gutter_col == 0 {
                            // Breakpoint column (leftmost).
                            let file = engine
                                .windows
                                .get(&rw.window_id)
                                .and_then(|w| engine.buffer_manager.get(w.buffer_id))
                                .and_then(|bs| bs.file_path.as_ref())
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let bp_line = rl.line_idx as u64 + 1;
                            engine.dap_toggle_breakpoint(&file, bp_line);
                        } else if gutter_col == git_col {
                            // Git diff column — open diff peek popup.
                            engine.active_tab_mut().active_window = rw.window_id;
                            engine.view_mut().cursor.line = rl.line_idx;
                            engine.open_diff_peek();
                        } else if engine.has_diagnostic_on_line(rl.line_idx) {
                            // Diagnostic gutter indicator — show hover popup.
                            engine.active_tab_mut().active_window = rw.window_id;
                            engine.view_mut().cursor.line = rl.line_idx;
                            engine.trigger_editor_hover_for_line(rl.line_idx);
                        } else if engine.has_code_actions_on_line(rl.line_idx) {
                            // Code action lightbulb — show code actions popup.
                            engine.active_tab_mut().active_window = rw.window_id;
                            engine.view_mut().cursor.line = rl.line_idx;
                            engine.show_code_actions_popup();
                        } else {
                            let has_fold_indicator =
                                rl.gutter_text.chars().any(|c| c == '+' || c == '-');
                            if has_fold_indicator {
                                engine.toggle_fold_at_line(rl.line_idx);
                            }
                        }
                    }
                    return sidebar_width;
                }
                // Text area click — fold/wrap-aware row → buffer line mapping
                let clicked_rl = rw.lines.get(view_row);
                let buf_line = clicked_rl
                    .map(|l| l.line_idx)
                    .unwrap_or_else(|| rw.scroll_top + view_row);
                // For wrapped lines, add segment_col_offset so the click
                // targets the correct column within the full buffer line.
                let seg_offset = clicked_rl.map(|l| l.segment_col_offset).unwrap_or(0);
                let col_in_text = (rel_col - wx - gutter) as usize + rw.scroll_left + seg_offset;

                // Double-click detection
                let now = Instant::now();
                let is_double = now.duration_since(*last_click_time) < Duration::from_millis(400)
                    && *last_click_pos == (col, row);
                *last_click_time = now;
                *last_click_pos = (col, row);

                if ev.modifiers.contains(KeyModifiers::CONTROL)
                    || (ev.modifiers.contains(KeyModifiers::ALT) && engine.is_vscode_mode())
                {
                    engine.add_cursor_at_pos(buf_line, col_in_text);
                } else if is_double {
                    engine.mouse_double_click(rw.window_id, buf_line, col_in_text);
                } else {
                    // Clear selection on click in VSCode mode.
                    if engine.is_vscode_mode() {
                        engine.vscode_clear_selection();
                    }
                    engine.mouse_click(rw.window_id, buf_line, col_in_text);
                }
                // Fire cursor_move hook so plugins (e.g. git-insights blame) see
                // the new cursor position after a mouse click on a buffer line.
                engine.fire_cursor_move_hook();
                return sidebar_width;
            }
        }
    }

    sidebar_width
}

/// Walk status line segments and find which action (if any) is at `click_col`.
fn status_segment_hit_test(
    status: &crate::render::WindowStatusLine,
    width: usize,
    click_col: usize,
) -> Option<crate::render::StatusAction> {
    // Compute right-side total width
    let right_width: usize = status
        .right_segments
        .iter()
        .map(|s| s.text.chars().count())
        .sum();
    let right_start = width.saturating_sub(right_width);

    // Check left segments
    let mut col = 0;
    for seg in &status.left_segments {
        let seg_len = seg.text.chars().count();
        if click_col >= col && click_col < col + seg_len {
            return seg.action.clone();
        }
        col += seg_len;
    }

    // Check right segments
    let mut col = right_start;
    for seg in &status.right_segments {
        let seg_len = seg.text.chars().count();
        if click_col >= col && click_col < col + seg_len {
            return seg.action.clone();
        }
        col += seg_len;
    }

    None
}
