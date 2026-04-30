use super::*;

/// Compute the `grab_offset` to seed [`quadraui::DragTarget::ScrollbarY`]
/// at click-down time so the thumb doesn't jump out from under the cursor.
///
/// Mirrors the thumb math `dispatch_mouse_drag` uses: if `cursor_y` lands
/// inside the visible thumb (between `thumb_top` and `thumb_top + thumb_length`),
/// returns the cursor's offset from the thumb top — the cursor stays at
/// the same relative spot on the thumb during the drag.
///
/// If `cursor_y` is on the track outside the thumb (or above/below the
/// track entirely), returns `0.0` — the standard "click track to jump"
/// behavior where the thumb hops to put its top at the cursor.
fn scrollbar_grab_offset(
    cursor_y: f32,
    track_start: f32,
    track_length: f32,
    visible_rows: usize,
    total_items: usize,
    current_scroll: usize,
) -> f32 {
    if track_length <= 0.0 || total_items == 0 {
        return 0.0;
    }
    let thumb_ratio = (visible_rows as f32 / total_items as f32).min(1.0);
    let thumb_length = (track_length * thumb_ratio).max(1.0);
    let max_scroll = total_items.saturating_sub(visible_rows);
    let effective_track = (track_length - thumb_length).max(1.0);
    let scroll_ratio = if max_scroll == 0 {
        0.0
    } else {
        (current_scroll as f32 / max_scroll as f32).clamp(0.0, 1.0)
    };
    let thumb_top = track_start + scroll_ratio * effective_track;
    let dy = cursor_y - thumb_top;
    if dy >= 0.0 && dy < thumb_length {
        dy
    } else {
        0.0
    }
}

/// Look up the `(total_items - visible_rows)` for the currently active
/// `ScrollbarY` drag. Used by inverted scrollbars (terminal scrollback,
/// debug output) to flip a forward offset reported by
/// [`quadraui::dispatch_mouse_drag`] back into the "lines from bottom"
/// convention those panels store. Returns 0 if no drag is active or
/// the active drag isn't a `ScrollbarY`.
fn current_drag_max_scroll(drag_state: &quadraui::DragState) -> usize {
    if let Some(quadraui::DragTarget::ScrollbarY {
        total_items,
        visible_rows,
        ..
    }) = drag_state.target()
    {
        total_items.saturating_sub(*visible_rows)
    } else {
        0
    }
}

/// Run [`quadraui::dispatch_mouse_drag`] for an active drag and apply the
/// resulting `ScrollOffsetChanged` events to the matching scroll-state
/// fields. Returns `true` if any event was handled (caller can short-circuit).
///
/// Used by both the mouse-drag path (to handle continued drags) and the
/// mouse-down path (to apply the click-time offset using the same
/// thumb-aware math the drag will use, eliminating the visual "jump and
/// correct" jankiness when the click-down math differs from drag math).
fn apply_scrollbar_drag(
    drag_state: &quadraui::DragState,
    point: quadraui::Point,
    engine: &mut Engine,
    sidebar: &mut TuiSidebar,
    debug_output_scroll: &mut usize,
) -> bool {
    let events = quadraui::dispatch_mouse_drag(drag_state, point, Default::default());
    let mut handled = false;
    for ev in &events {
        if let quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset } = ev {
            let key = widget.as_str();
            match key {
                "explorer:sb" => {
                    sidebar.scroll_top = *new_offset;
                    handled = true;
                }
                "ext_panel:sb" => {
                    engine.ext_panel_scroll_top = *new_offset;
                    handled = true;
                }
                "editor_hover" => {
                    engine.editor_hover_set_scroll(*new_offset);
                    handled = true;
                }
                "tui:search_results" => {
                    sidebar.search_scroll_top = *new_offset;
                    handled = true;
                }
                "tui:settings" => {
                    engine.settings_scroll_top = *new_offset;
                    handled = true;
                }
                // Inverted scrollbars: top of track = max offset (oldest
                // content), bottom = 0 (newest). dispatch_mouse_drag
                // reports the raw forward offset; flip it here.
                "tui:terminal_scrollback" => {
                    let max = current_drag_max_scroll(drag_state);
                    if let Some(term) = engine.active_terminal_mut() {
                        term.set_scroll_offset(max.saturating_sub(*new_offset));
                    }
                    handled = true;
                }
                "tui:debug_output" => {
                    let max = current_drag_max_scroll(drag_state);
                    *debug_output_scroll = max.saturating_sub(*new_offset);
                    handled = true;
                }
                other if other.starts_with("tui:debug_sidebar:") => {
                    if let Some(idx_str) = other.strip_prefix("tui:debug_sidebar:") {
                        if let Ok(idx) = idx_str.parse::<usize>() {
                            if idx < engine.dap_sidebar_scroll.len() {
                                engine.dap_sidebar_scroll[idx] = *new_offset;
                                handled = true;
                            }
                        }
                    }
                }
                // Editor window scrollbars — widget id format
                // `tui:editor:<window_id>:<vsb|hsb>`. Apply-side parses the
                // window id and routes to the per-window scroll setters.
                other if other.starts_with("tui:editor:") => {
                    if let Some(rest) = other.strip_prefix("tui:editor:") {
                        if let Some((wid_str, axis)) = rest.split_once(':') {
                            if let Ok(wid) = wid_str.parse::<usize>() {
                                let window_id = crate::core::WindowId(wid);
                                match axis {
                                    "vsb" => {
                                        engine.set_scroll_top_for_window(window_id, *new_offset);
                                        engine.sync_scroll_binds();
                                        handled = true;
                                    }
                                    "hsb" => {
                                        engine.set_scroll_left_for_window(window_id, *new_offset);
                                        handled = true;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    handled
}

// ─── Mouse handling ───────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_mouse(
    ev: MouseEvent,
    sidebar: &mut TuiSidebar,
    engine: &mut Engine,
    terminal_size: &Option<Size>,
    sidebar_width: u16,
    dragging_sidebar: &mut bool,
    debug_output_scroll: &mut usize,
    dragging_terminal_resize: &mut bool,
    dragging_terminal_split: &mut bool,
    dragging_group_divider: &mut Option<usize>,
    drag_state: &mut quadraui::DragState,
    modal_stack: &mut quadraui::ModalStack,
    last_layout: Option<&render::ScreenLayout>,
    last_click_time: &mut Instant,
    last_click_pos: &mut (u16, u16),
    mouse_text_drag: &mut bool,
    folder_picker: &mut Option<FolderPickerState>,
    quit_confirm: &mut bool,
    quit_confirm_focus: &mut usize,
    close_tab_confirm: &mut bool,
    close_tab_confirm_focus: &mut usize,
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
    editor_hover_scrollbar: Option<crate::render::PopupScrollbarHit>,
    hover_selecting: &mut bool,
    fr_input_dragging: &mut bool,
) -> u16 {
    let col = ev.column;
    let row = ev.row;
    let term_height = terminal_size.map(|s| s.height).unwrap_or(24);

    // ── Quit-confirm overlay click interception ─────────────────────────────
    // Route clicks through DialogLayout::hit_test. Swallow all clicks while
    // the overlay is visible so they don't fall through to the editor.
    if *quit_confirm {
        if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
            let term_size =
                terminal_size.unwrap_or_else(|| ratatui::layout::Size::new(term_height, 80));
            let area = ratatui::layout::Rect {
                x: 0,
                y: 0,
                width: term_size.width,
                height: term_size.height,
            };
            // Focus index doesn't matter for hit-testing (button positions
            // don't depend on which one is focused); pass 0.
            let (_dialog, layout) = super::render_impl::build_quit_confirm_dialog(area, 0);
            match layout.hit_test(col as f32, row as f32) {
                quadraui::DialogHit::Button(id) => match id.as_str() {
                    "quit:save_all" => {
                        engine.save_all_dirty();
                        engine.cleanup_all_swaps();
                        engine.lsp_shutdown();
                        save_session(engine);
                        *quit_confirm = false;
                        *quit_confirm_focus = 0;
                        *should_quit = true;
                    }
                    "quit:force" => {
                        engine.cleanup_all_swaps();
                        engine.lsp_shutdown();
                        save_session(engine);
                        *quit_confirm = false;
                        *quit_confirm_focus = 0;
                        *should_quit = true;
                    }
                    "quit:cancel" => {
                        *quit_confirm = false;
                        *quit_confirm_focus = 0;
                    }
                    _ => {}
                },
                quadraui::DialogHit::Outside => {
                    // Click outside dialog: dismiss (same as pressing Escape).
                    *quit_confirm = false;
                    *quit_confirm_focus = 0;
                }
                quadraui::DialogHit::Body => {
                    // Click on dialog body (not a button): swallow.
                }
            }
        }
        // Swallow all other mouse events while the overlay is up.
        return sidebar_width;
    }

    // ── Close-tab confirm overlay click interception ────────────────────────
    // Route clicks through DialogLayout::hit_test. Swallow all clicks while
    // the overlay is visible so they don't fall through to the editor.
    if *close_tab_confirm {
        if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
            let term_size =
                terminal_size.unwrap_or_else(|| ratatui::layout::Size::new(term_height, 80));
            let area = ratatui::layout::Rect {
                x: 0,
                y: 0,
                width: term_size.width,
                height: term_size.height,
            };
            // Focus index doesn't matter for hit-testing (button positions
            // don't depend on which one is focused); pass 0.
            let (_dialog, layout) = super::render_impl::build_close_tab_dialog(area, 0);
            match layout.hit_test(col as f32, row as f32) {
                quadraui::DialogHit::Button(id) => match id.as_str() {
                    "close_tab:save" => {
                        // Save + quit. `execute_command("quit")` handles
                        // the last-window case (returns EngineAction::Quit)
                        // automatically. Since we just saved, the dirty
                        // check inside "quit" is satisfied.
                        engine.escape_to_normal();
                        let _ = engine.save();
                        let action = engine.execute_command("quit");
                        *close_tab_confirm = false;
                        if action == crate::core::engine::EngineAction::Quit
                            && handle_action(engine, action)
                        {
                            *should_quit = true;
                        }
                    }
                    "close_tab:discard" => {
                        // Force-quit semantics via `quit!`. Handles the
                        // last-window case (returns EngineAction::Quit)
                        // and drops the buffer regardless of dirty flag.
                        engine.escape_to_normal();
                        let action = engine.execute_command("quit!");
                        *close_tab_confirm = false;
                        if action == crate::core::engine::EngineAction::Quit
                            && handle_action(engine, action)
                        {
                            *should_quit = true;
                        }
                    }
                    "close_tab:cancel" => {
                        engine.escape_to_normal();
                        *close_tab_confirm = false;
                    }
                    _ => {}
                },
                quadraui::DialogHit::Outside => {
                    // Click outside dialog: dismiss (same as pressing Escape).
                    engine.escape_to_normal();
                    *close_tab_confirm = false;
                }
                quadraui::DialogHit::Body => {
                    // Click on dialog body (not a button): swallow.
                }
            }
        }
        // Swallow all other mouse events while the overlay is up.
        return sidebar_width;
    }

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

    // Bottom chrome rows: rows below the terminal panel.
    let has_separated = last_layout
        .as_ref()
        .is_some_and(|l| l.separated_status_line.is_some());
    let bottom_chrome: u16 = if engine.settings.window_status_line {
        1 // cmd only
    } else {
        2 // status + cmd
    };
    // Separated status row between terminal and cmd (when noslat + terminal open).
    let sep_status_rows: u16 = if has_separated { 1 } else { 0 };

    // Check if the mouse cursor is currently inside or adjacent to the hover
    // popup bounding rect. We include 1 column to the left (the sidebar
    // separator) so the popup doesn't dismiss while the mouse crosses to it.
    let mouse_on_hover_popup = hover_popup_rect.is_some_and(|(px, py, pw, ph)| {
        col >= px.saturating_sub(1) && col < px + pw && row >= py && row < py + ph
    });

    // Check if mouse is on the editor hover popup (exact bounds).
    let mouse_on_editor_hover = editor_hover_popup_rect
        .is_some_and(|(px, py, pw, ph)| col >= px && col < px + pw && row >= py && row < py + ph);

    // Reconcile the editor hover popup with the modal stack (#216).
    // Push whenever the popup is visible — even unfocused. Right-click
    // dispatch consults the stack's hit_test below so the editor's
    // context menu can't steal events from the popup.
    {
        let editor_hover_id = quadraui::WidgetId::new("editor_hover");
        match (engine.editor_hover.is_some(), editor_hover_popup_rect) {
            (true, Some((px, py, pw, ph))) => {
                modal_stack.push(
                    editor_hover_id,
                    quadraui::Rect {
                        x: px as f32,
                        y: py as f32,
                        width: pw as f32,
                        height: ph as f32,
                    },
                );
            }
            _ => {
                modal_stack.pop(&editor_hover_id);
            }
        }
    }

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

    // ── Find/replace overlay mouse handling ────────────────────────────────────
    if engine.find_replace_open {
        // Use hit regions from the last layout for accurate click dispatch.
        let fr_panel = last_layout.as_ref().and_then(|l| l.find_replace.as_ref());
        if let Some(panel) = fr_panel {
            let panel_w = panel.panel_width;
            let row_count: u16 = if panel.show_replace { 2 } else { 1 };
            let panel_h: u16 = row_count + 2; // +2 for borders

            // Compute panel screen position from group_bounds
            let gb = &panel.group_bounds;
            let gb_right = editor_left + gb.x as u16 + gb.width as u16;
            let panel_x = gb_right.saturating_sub(panel_w + 1).max(editor_left);
            let panel_y = (gb.y as u16).max(1);
            let content_x = panel_x + 1; // inside left border
            let find_y = panel_y + 1; // first content row

            let on_panel = col >= panel_x
                && col < panel_x + panel_w
                && row >= panel_y
                && row < panel_y + panel_h;

            // --- Drag-to-select in input fields ---
            if let MouseEventKind::Drag(MouseButton::Left) = ev.kind {
                if *fr_input_dragging && on_panel {
                    let rel_col = col.saturating_sub(content_x);
                    // Determine input bounds from hit regions
                    let input_region = panel.hit_regions.iter().find(|(r, t)| {
                        matches!(
                            t,
                            crate::core::engine::FindReplaceClickTarget::FindInput(_)
                                | crate::core::engine::FindReplaceClickTarget::ReplaceInput(_)
                        ) && r.row == if engine.find_replace_focus == 0 { 0 } else { 1 }
                    });
                    if let Some((region, _)) = input_region {
                        let char_pos = rel_col.saturating_sub(region.col) as usize;
                        let field_len = if engine.find_replace_focus == 0 {
                            engine.find_replace_query.chars().count()
                        } else {
                            engine.find_replace_replacement.chars().count()
                        };
                        engine.find_replace_cursor = char_pos.min(field_len);
                    }
                    return sidebar_width;
                }
            }

            // --- Mouse up: end drag ---
            if let MouseEventKind::Up(MouseButton::Left) = ev.kind {
                if *fr_input_dragging {
                    *fr_input_dragging = false;
                    // If cursor == anchor, clear selection
                    if engine.find_replace_sel_anchor == Some(engine.find_replace_cursor) {
                        engine.find_replace_sel_anchor = None;
                    }
                    return sidebar_width;
                }
            }

            // --- Click (Down) ---
            if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
                if on_panel {
                    // Double-click detection
                    let now = Instant::now();
                    let is_double = now.duration_since(*last_click_time)
                        < Duration::from_millis(400)
                        && *last_click_pos == (col, row);
                    *last_click_time = now;
                    *last_click_pos = (col, row);

                    // Translate to panel-relative coordinates
                    let rel_col = col.saturating_sub(content_x);
                    let rel_row = if row == find_y {
                        0u16
                    } else if row == find_y + 1 && panel.show_replace {
                        1u16
                    } else {
                        return sidebar_width; // on border, consume click
                    };

                    // Walk hit regions to find the target
                    let mut matched_target = None;
                    for (region, target) in &panel.hit_regions {
                        if region.row == rel_row
                            && rel_col >= region.col
                            && rel_col < region.col + region.width
                        {
                            matched_target = Some((*target, region.col));
                            break;
                        }
                    }

                    if let Some((target, region_col)) = matched_target {
                        use crate::core::engine::FindReplaceClickTarget::*;

                        // For input fields, compute the char offset
                        let target = match target {
                            FindInput(_) => {
                                let char_pos = rel_col.saturating_sub(region_col) as usize;
                                FindInput(char_pos)
                            }
                            ReplaceInput(_) => {
                                let char_pos = rel_col.saturating_sub(region_col) as usize;
                                ReplaceInput(char_pos)
                            }
                            other => other,
                        };

                        // Double-click word select in input fields
                        if is_double {
                            match target {
                                FindInput(pos) => {
                                    let (start, end) = crate::core::engine::find_word_boundaries(
                                        &engine.find_replace_query,
                                        pos,
                                    );
                                    engine.find_replace_focus = 0;
                                    engine.find_replace_sel_anchor = Some(start);
                                    engine.find_replace_cursor = end;
                                    return sidebar_width;
                                }
                                ReplaceInput(pos) => {
                                    let (start, end) = crate::core::engine::find_word_boundaries(
                                        &engine.find_replace_replacement,
                                        pos,
                                    );
                                    engine.find_replace_focus = 1;
                                    engine.find_replace_sel_anchor = Some(start);
                                    engine.find_replace_cursor = end;
                                    return sidebar_width;
                                }
                                _ => {}
                            }
                        }

                        // Start drag if clicking on an input field
                        if matches!(target, FindInput(_) | ReplaceInput(_)) {
                            *fr_input_dragging = true;
                        }

                        engine.handle_find_replace_click(target);
                    }
                    return sidebar_width;
                }
                // Click outside panel — fall through to other handlers
            }
        }
    }

    // ── Dialog popup click handling ─────────────────────────────────────────────
    if engine.dialog.is_some() {
        if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
            let term_cols = terminal_size.map(|s| s.width).unwrap_or(80);
            let dialog = engine.dialog.as_ref().unwrap();
            // Compute dialog layout (same formula as render_dialog_popup)
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

            let layout = crate::core::engine::DialogLayout {
                x: px,
                y: py,
                width,
                height,
                btn_y,
            };
            let result = crate::core::engine::resolve_dialog_click(
                &dialog.buttons,
                &layout,
                col,
                row,
                &|label, hotkey| render::format_button_label(label, hotkey),
            );
            use crate::core::engine::DialogClickResult;
            match result {
                DialogClickResult::Button(idx) => {
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
                DialogClickResult::Outside => {
                    engine.dialog = None;
                    engine.pending_move = None;
                }
                DialogClickResult::InsideDialog => {}
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
        // Active scrollbar drag — feed through the cross-backend
        // dispatcher. Same math GTK uses (0f3e0d0), same primitive
        // event type; TUI just supplies cell-unit track geometry
        // instead of pixels.
        if let MouseEventKind::Drag(MouseButton::Left) = ev.kind {
            if drag_state.is_active() {
                // Capture visible_rows before drop for the selection-clamp below.
                let visible_rows =
                    if let Some(quadraui::DragTarget::ScrollbarY { visible_rows, .. }) =
                        drag_state.target()
                    {
                        *visible_rows
                    } else {
                        0
                    };
                let events = quadraui::dispatch_mouse_drag(
                    drag_state,
                    quadraui::Point {
                        x: col as f32,
                        y: row as f32,
                    },
                    Default::default(),
                );
                for ev in &events {
                    if let quadraui::UiEvent::ScrollOffsetChanged { new_offset, .. } = ev {
                        engine.picker_scroll_top = *new_offset;
                        // `draw_palette` clamps its effective scroll
                        // offset to keep `picker_selected` on-screen, so
                        // a drag that leaves selection outside the new
                        // viewport would snap back visually. Pull
                        // selection to the nearest visible edge.
                        if engine.picker_selected < *new_offset {
                            engine.picker_selected = *new_offset;
                        } else if visible_rows > 0
                            && engine.picker_selected >= *new_offset + visible_rows
                        {
                            engine.picker_selected = *new_offset + visible_rows - 1;
                        }
                        engine.picker_load_preview();
                    }
                }
                return sidebar_width;
            }
        }
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
                let visible_rows = results_end.saturating_sub(results_start) as usize;
                let total_items = engine.picker_items.len();

                // Phase B.4: route the click through quadraui's modal
                // stack + dispatcher to decide in-modal vs backdrop.
                // Same shape GTK uses (0f3e0d0 + a02eff9): push the
                // popup's bounds, call `dispatch_mouse_down`, branch
                // on the returned `UiEvent`s. TUI uses whole cells as
                // the unit; GTK uses pixels; the dispatcher doesn't
                // care — it works in whichever f32 space the backend
                // supplies.
                let picker_id = quadraui::WidgetId::new("picker");
                modal_stack.push(
                    picker_id.clone(),
                    quadraui::Rect {
                        x: popup_x as f32,
                        y: popup_y as f32,
                        width: popup_w as f32,
                        height: popup_h as f32,
                    },
                );
                let events = quadraui::dispatch_mouse_down(
                    modal_stack,
                    quadraui::Point {
                        x: col as f32,
                        y: row as f32,
                    },
                    quadraui::MouseButton::Left,
                    quadraui::Modifiers::default(),
                );
                let mut hit_modal = false;
                let mut dismiss_modal = false;
                for ev in &events {
                    match ev {
                        quadraui::UiEvent::MouseDown {
                            widget: Some(wid), ..
                        } if *wid == picker_id => {
                            hit_modal = true;
                        }
                        quadraui::UiEvent::Palette(_, quadraui::PaletteEvent::Closed) => {
                            dismiss_modal = true;
                        }
                        _ => {}
                    }
                }

                if hit_modal {
                    // Click inside popup — inner hit-test (scrollbar,
                    // then result row) drives what the click does.
                    let has_scrollbar = !has_preview && total_items > visible_rows && popup_w >= 2;
                    let sb_col = popup_x + popup_w - 2;
                    let on_scrollbar = has_scrollbar
                        && col >= sb_col
                        && col < popup_x + popup_w
                        && row >= results_start
                        && row < results_end
                        && visible_rows > 0;

                    if on_scrollbar {
                        let grab_offset = scrollbar_grab_offset(
                            row as f32,
                            results_start as f32,
                            visible_rows as f32,
                            visible_rows,
                            total_items,
                            engine.picker_scroll_top,
                        );
                        drag_state.begin(quadraui::DragTarget::ScrollbarY {
                            widget: picker_id.clone(),
                            track_start: results_start as f32,
                            track_length: visible_rows as f32,
                            visible_rows,
                            total_items,
                            grab_offset,
                        });
                        let events = quadraui::dispatch_mouse_drag(
                            drag_state,
                            quadraui::Point {
                                x: col as f32,
                                y: row as f32,
                            },
                            Default::default(),
                        );
                        for ev in &events {
                            if let quadraui::UiEvent::ScrollOffsetChanged { new_offset, .. } = ev {
                                engine.picker_scroll_top = *new_offset;
                                if engine.picker_selected < *new_offset {
                                    engine.picker_selected = *new_offset;
                                } else if engine.picker_selected >= *new_offset + visible_rows {
                                    engine.picker_selected = *new_offset + visible_rows - 1;
                                }
                                engine.picker_load_preview();
                            }
                        }
                    } else if row >= results_start && row < results_end {
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
                    }
                }
                if dismiss_modal {
                    engine.close_picker();
                    modal_stack.pop(&picker_id);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                drag_state.end();
            }
            MouseEventKind::ScrollDown => {
                // Check if scroll is over the preview pane (right side).
                let term_cols = terminal_size.map(|s| s.width).unwrap_or(80);
                let has_preview = engine.picker_preview.is_some();
                let popup_w = if has_preview {
                    (term_cols * 4 / 5).max(60)
                } else {
                    (term_cols * 55 / 100).max(55)
                };
                let popup_x = (term_cols.saturating_sub(popup_w)) / 2;
                let left_w = if has_preview {
                    (popup_w as usize * 35 / 100) as u16
                } else {
                    0
                };
                if has_preview && col > popup_x + left_w {
                    // Scroll the preview pane.
                    let max = engine
                        .picker_preview
                        .as_ref()
                        .map(|p| p.lines.len())
                        .unwrap_or(0);
                    engine.picker_preview_scroll =
                        (engine.picker_preview_scroll + 3).min(max.saturating_sub(1));
                } else {
                    let step = 3;
                    let max = engine.picker_items.len().saturating_sub(1);
                    engine.picker_selected = (engine.picker_selected + step).min(max);
                    let visible = 20usize;
                    if engine.picker_selected >= engine.picker_scroll_top + visible {
                        engine.picker_scroll_top = engine.picker_selected + 1 - visible;
                    }
                    engine.picker_load_preview();
                }
            }
            MouseEventKind::ScrollUp => {
                let term_cols = terminal_size.map(|s| s.width).unwrap_or(80);
                let has_preview = engine.picker_preview.is_some();
                let popup_w = if has_preview {
                    (term_cols * 4 / 5).max(60)
                } else {
                    (term_cols * 55 / 100).max(55)
                };
                let popup_x = (term_cols.saturating_sub(popup_w)) / 2;
                let left_w = if has_preview {
                    (popup_w as usize * 35 / 100) as u16
                } else {
                    0
                };
                if has_preview && col > popup_x + left_w {
                    // Scroll the preview pane.
                    engine.picker_preview_scroll = engine.picker_preview_scroll.saturating_sub(3);
                } else {
                    let step = 3;
                    engine.picker_selected = engine.picker_selected.saturating_sub(step);
                    if engine.picker_selected < engine.picker_scroll_top {
                        engine.picker_scroll_top = engine.picker_selected;
                    }
                    engine.picker_load_preview();
                }
            }
            _ => {} // consume all other events
        }
        return sidebar_width;
    } else {
        // Picker isn't open but the modal stack may carry a stale
        // entry if the picker closed via keyboard (Esc / Enter) or
        // programmatic close_picker() without the mouse path knowing.
        // Keep them consistent.
        modal_stack.pop(&quadraui::WidgetId::new("picker"));
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
            // Phase B.4 Stage 5c: every scrollbar drag flows through the
            // shared `quadraui::DragState::ScrollbarY` + `dispatch_mouse_drag`.
            // Widget id routes the resulting `ScrollOffsetChanged` to the
            // matching scroll-state field. Sites covered:
            // - `explorer:sb`, `ext_panel:sb`, `editor_hover` (Stage 5a)
            // - `tui:search_results`, `tui:settings`, `tui:debug_sidebar:N` (5c)
            // - `tui:terminal_scrollback`, `tui:debug_output` (5c, inverted)
            if drag_state.is_active() {
                let point = quadraui::Point {
                    x: col as f32,
                    y: row as f32,
                };
                if apply_scrollbar_drag(drag_state, point, engine, sidebar, debug_output_scroll) {
                    return sidebar_width;
                }
            }
            // Terminal panel resize drag
            if *dragging_terminal_resize {
                let qf_h: u16 = if engine.quickfix_open { 6 } else { 0 };
                let available = term_height.saturating_sub(row + bottom_chrome + qf_h);
                // Leave at least 4 editor lines visible (+ menu/tab bar chrome)
                let mr: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                let min_editor_chrome = 4 + mr + 1; // 4 lines + menu + tab bar
                let max_rows = term_height
                    .saturating_sub(bottom_chrome + qf_h + min_editor_chrome + 2) // +2 for terminal tab bar + header
                    .max(5);
                let new_rows = available.saturating_sub(1).clamp(5, max_rows);
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
            // Phase B.4 Stage 5c: terminal scrollback + debug output
            // scrollbars are inverted (top of track = oldest content,
            // bottom = live view). Their drag math now lives in the
            // shared `if drag_state.is_active()` block above; the
            // receive site for `tui:terminal_scrollback` /
            // `tui:debug_output` flips the offset with `max - new_offset`
            // so `term.set_scroll_offset` / `*debug_output_scroll`
            // continue to mean "lines from the bottom".

            // Phase B.4 Stage 5d: editor-window scrollbar drag math now
            // lives in the shared `if drag_state.is_active()` block above
            // via `tui:editor:N:vsb` / `tui:editor:N:hsb` widget ids. The
            // legacy `dragging_scrollbar` local + `ScrollDragState` are
            // gone.
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
                // Editor drag moved outside all windows (e.g. into terminal area) —
                // stop processing so it doesn't bleed into other panels.
                if *mouse_text_drag {
                    return sidebar_width;
                }
            }
            // Terminal drag-to-select in content rows.
            // Only activate if the drag originated in the terminal (selection exists)
            // and the mouse is within the terminal panel bounds.
            {
                let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
                let strip_rows: u16 = if engine.terminal_open {
                    super::effective_terminal_panel_rows_tui(engine, term_height) + 1
                } else {
                    0
                };
                let term_strip_top =
                    term_height.saturating_sub(bottom_chrome + qf_rows + strip_rows);
                if engine.terminal_open
                    && strip_rows > 0
                    && col >= editor_left
                    && row > term_strip_top
                    && row < term_strip_top + strip_rows
                    && engine
                        .active_terminal()
                        .is_some_and(|t| t.selection.is_some())
                {
                    let term_row = row - term_strip_top - 1;
                    let term_col = col.saturating_sub(editor_left);
                    if let Some(term) = engine.active_terminal_mut() {
                        if let Some(ref mut sel) = term.selection {
                            sel.end_row = term_row;
                            sel.end_col = term_col;
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
            // Stage 5c+5d: scrollbar drags (search, settings, debug-sidebar,
            // terminal, debug-output, editor v/h scrollbars) clear via
            // `drag_state.end()` — single source of truth.
            drag_state.end();
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
                    super::effective_terminal_panel_rows_tui(engine, term_height) + 1
                } else {
                    0
                };
                let term_strip_top =
                    term_height.saturating_sub(bottom_chrome + qf_rows + strip_rows);
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
                    let panel_height =
                        super::effective_terminal_panel_rows_tui(engine, term_height) + 2;
                    let panel_y =
                        term_height.saturating_sub(bottom_chrome + dt_rows + panel_height);
                    let panel_end = term_height.saturating_sub(bottom_chrome + dt_rows);
                    if row >= panel_y && row < panel_end {
                        let content_rows =
                            super::effective_terminal_panel_rows_tui(engine, term_height) as usize;
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
        // Swallow if the click landed on a focused modal that wants
        // to consume it (#216 — editor hover popup). The modal stack
        // was reconciled at the top of this function.
        if modal_stack
            .hit_test(quadraui::Point {
                x: col as f32,
                y: row as f32,
            })
            .is_some()
        {
            return sidebar_width;
        }

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

        // Right-click on tab bar → open tab context menu.
        //
        // B5c.2: hit-test via primitive `bar.layout(...).hit_test(...)`
        // — same code path the rasteriser uses to paint, so click
        // resolution doesn't drift from the rendered positions.
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
                            let bar = render::build_tab_bar_primitive(
                                &gtb.tabs,
                                false,
                                gtb.diff_toolbar.as_ref(),
                                gtb.tab_scroll_offset,
                                None,
                            );
                            let tab_widths: Vec<usize> = gtb
                                .tabs
                                .iter()
                                .map(|t| t.name.chars().count() + render::TAB_CLOSE_COLS as usize)
                                .collect();
                            let bar_layout = bar.layout(
                                gw as f32,
                                1.0,
                                0.0,
                                |i| {
                                    quadraui::TabMeasure::new(
                                        tab_widths[i] as f32,
                                        render::TAB_CLOSE_COLS as f32,
                                    )
                                },
                                |i| {
                                    quadraui::SegmentMeasure::new(
                                        bar.right_segments[i].width_cells as f32,
                                    )
                                },
                            );
                            if let quadraui::TabBarHit::Tab(i) | quadraui::TabBarHit::TabClose(i) =
                                bar_layout.hit_test(local_col as f32, 0.0)
                            {
                                engine.open_tab_context_menu(gtb.group_id, i, col, row + 1);
                                return sidebar_width;
                            }
                            break;
                        }
                    }
                } else {
                    // Single-group tab bar (row == menu_rows)
                    if row == menu_rows && !engine.is_tab_bar_hidden(engine.active_group) {
                        let editor_col_width = terminal_size
                            .map(|s| s.width)
                            .unwrap_or(80)
                            .saturating_sub(editor_left);
                        let bar = render::build_tab_bar_primitive(
                            &layout.tab_bar,
                            true,
                            layout.diff_toolbar.as_ref(),
                            layout.tab_scroll_offset,
                            None,
                        );
                        let tab_widths: Vec<usize> = layout
                            .tab_bar
                            .iter()
                            .map(|t| t.name.chars().count() + render::TAB_CLOSE_COLS as usize)
                            .collect();
                        let bar_layout = bar.layout(
                            editor_col_width as f32,
                            1.0,
                            0.0,
                            |i| {
                                quadraui::TabMeasure::new(
                                    tab_widths[i] as f32,
                                    render::TAB_CLOSE_COLS as f32,
                                )
                            },
                            |i| {
                                quadraui::SegmentMeasure::new(
                                    bar.right_segments[i].width_cells as f32,
                                )
                            },
                        );
                        if let quadraui::TabBarHit::Tab(i) | quadraui::TabBarHit::TabClose(i) =
                            bar_layout.hit_test(rel_col as f32, 0.0)
                        {
                            engine.open_tab_context_menu(engine.active_group, i, col, row + 1);
                            return sidebar_width;
                        }
                    }
                }
            }
        }

        // Right-click on terminal panel → suppress (don't show editor context menu).
        {
            let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
            let strip_rows: u16 = if engine.terminal_open {
                super::effective_terminal_panel_rows_tui(engine, term_height) + 1
            } else {
                0
            };
            let term_strip_top = term_height.saturating_sub(bottom_chrome + qf_rows + strip_rows);
            if engine.terminal_open
                && strip_rows > 0
                && col >= editor_left
                && row >= term_strip_top
                && row < term_strip_top + strip_rows
            {
                return sidebar_width;
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
        if let Some(ref cm) = engine.context_menu {
            let term_w = terminal_size.map(|s| s.width).unwrap_or(80);
            let result = crate::core::engine::resolve_context_menu_click(
                &cm.items,
                cm.screen_x,
                cm.screen_y,
                term_w,
                term_height,
                col,
                row,
            );
            use crate::core::engine::ContextMenuClickResult;
            match result {
                ContextMenuClickResult::Item(idx) => {
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
                    return sidebar_width;
                }
                ContextMenuClickResult::InsidePopup => {
                    return sidebar_width;
                }
                ContextMenuClickResult::Outside => {
                    engine.close_context_menu();
                    // Fall through to process the click normally
                }
            }
        } else {
            engine.close_context_menu();
        }
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
    // ── Click on editor hover popup scrollbar → jump-scroll or arm drag ────
    // Same pattern as picker/explorer scrollbars (#215). Track click jumps
    // to that offset and begins a drag so the mouse-move dispatcher
    // updates the offset live; thumb click just begins the drag.
    if mouse_on_editor_hover {
        if let Some(sb_hit) = editor_hover_scrollbar {
            let cx = col as f32;
            let cy = row as f32;
            let on_thumb = cx >= sb_hit.thumb.x
                && cx < sb_hit.thumb.x + sb_hit.thumb.width
                && cy >= sb_hit.thumb.y
                && cy < sb_hit.thumb.y + sb_hit.thumb.height;
            let on_track = !on_thumb
                && cx >= sb_hit.track.x
                && cx < sb_hit.track.x + sb_hit.track.width
                && cy >= sb_hit.track.y
                && cy < sb_hit.track.y + sb_hit.track.height;
            if on_track || on_thumb {
                let grab_offset = if on_thumb { cy - sb_hit.thumb.y } else { 0.0 };
                drag_state.begin(quadraui::DragTarget::ScrollbarY {
                    widget: quadraui::WidgetId::new("editor_hover"),
                    track_start: sb_hit.track.y,
                    track_length: sb_hit.track.height,
                    visible_rows: sb_hit.visible_rows,
                    total_items: sb_hit.total,
                    grab_offset,
                });
                apply_scrollbar_drag(
                    drag_state,
                    quadraui::Point { x: cx, y: cy },
                    engine,
                    sidebar,
                    debug_output_scroll,
                );
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
                            *quit_confirm_focus = 0;
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
    // Resolve via the same StatusBar adapter the renderer uses, so the
    // hit math can never drift from the visible layout (was: duplicated
    // walk over DEBUG_BUTTONS that re-derived per-button widths and
    // separator gaps).
    if engine.debug_toolbar_visible {
        // Below the debug toolbar in the v_chunks layout (see
        // render_impl::draw_frame): sep_status, wildmenu,
        // global_status, cmd. Quickfix and bottom_panel are ABOVE
        // the toolbar, so they don't count here. (Legacy formula
        // mistakenly subtracted them and missed the row entirely
        // whenever a terminal/debug panel was open.)
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
        let toolbar_row = term_height.saturating_sub(
            1 // cmd
                + global_status_rows
                + wildmenu_rows
                + sep_status_rows
                + 1, // the toolbar row itself
        );
        if row == toolbar_row && col >= editor_left {
            let toolbar = render::DebugToolbarData {
                buttons: render::DEBUG_BUTTONS.to_vec(),
                session_active: engine.dap_session_active,
            };
            // Theme only feeds segment fg/bg here, which resolve_click
            // ignores — onedark stand-in is fine for hit-test only.
            let theme = Theme::onedark();
            let bar = render::debug_toolbar_to_quadraui_status_bar(&toolbar, &theme);
            // The toolbar is rendered into the right-column rect starting
            // at editor_left, so its local x=0 corresponds to that
            // absolute column. Convert the click into bar-local space
            // before resolving.
            let term_w = terminal_size.map(|s| s.width).unwrap_or(80);
            let bar_w = term_w.saturating_sub(editor_left) as usize;
            let local_col = col - editor_left;
            if let Some(id) = bar.resolve_click(local_col, bar_w) {
                if let Some(idx) = render::debug_toolbar_action_index(&id) {
                    if let Some(btn) = render::DEBUG_BUTTONS.get(idx) {
                        let _ = engine.execute_command(btn.action);
                    }
                }
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
            let panel_height = super::effective_terminal_panel_rows_tui(engine, term_height) + 2;
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
            let panel_height = super::effective_terminal_panel_rows_tui(engine, term_height) + 2;
            let panel_y = term_height.saturating_sub(bottom_chrome + dt_rows + panel_height);
            let panel_end = term_height.saturating_sub(bottom_chrome + dt_rows);
            if row >= panel_y && row < panel_end {
                let term_width = terminal_size.map(|s| s.width).unwrap_or(80);
                let sb_col = term_width.saturating_sub(1);
                let total = engine.dap_output_lines.len();
                let content_rows =
                    super::effective_terminal_panel_rows_tui(engine, term_height) as usize;
                if total > content_rows && col == sb_col && row >= panel_y + 2 {
                    // Click on scrollbar track — start drag through shared state.
                    // Inverted scrollbar: keep grab_offset = 0 for now (the
                    // visual thumb position uses different math than the
                    // standard renderer, so thumb-grab preservation here
                    // would require more work).
                    let track_start = panel_y + 2; // after tab-bar row + header row
                    let track_len = super::effective_terminal_panel_rows_tui(engine, term_height);
                    drag_state.begin(quadraui::DragTarget::ScrollbarY {
                        widget: quadraui::WidgetId::new("tui:debug_output"),
                        track_start: track_start as f32,
                        track_length: track_len as f32,
                        visible_rows: track_len as usize,
                        total_items: total,
                        grab_offset: 0.0,
                    });
                    // Apply the click-time offset using the same thumb-aware
                    // math the subsequent drags will use, so the thumb
                    // doesn't visually jump on the first drag event.
                    apply_scrollbar_drag(
                        drag_state,
                        quadraui::Point {
                            x: col as f32,
                            y: row as f32,
                        },
                        engine,
                        sidebar,
                        debug_output_scroll,
                    );
                }
                return sidebar_width;
            }
        }
    }
    // ── Separated status line click (above terminal) ────────────────────────
    if sep_status_rows > 0 {
        let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
        let strip_rows: u16 = if engine.terminal_open {
            super::effective_terminal_panel_rows_tui(engine, term_height) + 1
        } else {
            0
        };
        let term_strip_top = term_height.saturating_sub(bottom_chrome + qf_rows + strip_rows);
        // Separated status is 1 row above the terminal panel.
        let sep_row = term_strip_top.saturating_sub(sep_status_rows);
        if col >= editor_left && row == sep_row {
            if let Some(layout) = last_layout {
                if let Some(status) = &layout.separated_status_line {
                    let click_col = (col - editor_left) as usize;
                    let bar_width = terminal_size.map(|s| s.width).unwrap_or(80) as usize;
                    if let Some(action) = status_segment_hit_test(status, bar_width, click_col) {
                        if let Some(ea) = engine.handle_status_action(&action) {
                            use crate::core::engine::EngineAction;
                            match ea {
                                EngineAction::ToggleSidebar => {
                                    sidebar.visible = !sidebar.visible;
                                }
                                EngineAction::OpenTerminal => {
                                    let cols =
                                        terminal_size.as_ref().map(|s| s.width).unwrap_or(80);
                                    engine
                                        .terminal_new_tab(cols, engine.session.terminal_panel_rows);
                                }
                                _ => {}
                            }
                        }
                    }
                    return sidebar_width;
                }
            }
        }
    }
    // ── Terminal panel click ───────────────────────────────────────────────────
    {
        let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
        let strip_rows: u16 = if engine.terminal_open {
            super::effective_terminal_panel_rows_tui(engine, term_height) + 1
        } else {
            0
        };
        let term_strip_top = term_height.saturating_sub(bottom_chrome + qf_rows + strip_rows);
        if engine.terminal_open
            && strip_rows > 0
            && col >= editor_left
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
                    // Maximize button (2 cols left of close)
                    let screen_h = terminal_size.map(|s| s.height).unwrap_or(24);
                    let full_cols = terminal_size.map(|s| s.width).unwrap_or(80);
                    engine.toggle_terminal_maximize();
                    let target = super::terminal_target_maximize_rows_tui(engine, screen_h);
                    let effective = engine.effective_terminal_panel_rows(target);
                    engine.terminal_resize(full_cols, effective);
                } else if col >= term_width.saturating_sub(6) {
                    // Split button (2 cols left of maximize)
                    let full_cols = terminal_size.map(|s| s.width).unwrap_or(80);
                    let rows = engine.session.terminal_panel_rows;
                    engine.terminal_toggle_split(full_cols, rows);
                } else if col >= term_width.saturating_sub(8) {
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
                    // Scrollbar column — start drag through shared state.
                    let track_start = term_strip_top + 1;
                    let track_len = strip_rows.saturating_sub(1); // content rows
                                                                  // Cap total to one screenful (vt100 API limit) so the drag range
                                                                  // [0, total] exactly matches what set_scroll_offset can deliver.
                    let total = engine
                        .active_terminal()
                        .map(|t| t.history.len())
                        .unwrap_or(0);
                    // visible_rows: 0 — terminal's `set_scroll_offset` clamps
                    // to `history.len()` (not `history.len() - viewport`),
                    // and the renderer's thumb math uses `max_off = history.len()`.
                    // Setting visible_rows = 0 makes dispatch_mouse_drag's
                    // max_scroll = total, so inverting (`max - new_offset`)
                    // reaches the very top of scrollback.
                    drag_state.begin(quadraui::DragTarget::ScrollbarY {
                        widget: quadraui::WidgetId::new("tui:terminal_scrollback"),
                        track_start: track_start as f32,
                        track_length: track_len as f32,
                        visible_rows: 0,
                        total_items: total,
                        grab_offset: 0.0,
                    });
                    apply_scrollbar_drag(
                        drag_state,
                        quadraui::Point {
                            x: col as f32,
                            y: row as f32,
                        },
                        engine,
                        sidebar,
                        debug_output_scroll,
                    );
                } else {
                    // Content area — start a selection.
                    let term_row = row - term_strip_top - 1;
                    let term_col = col.saturating_sub(editor_left);
                    engine.terminal_scroll_reset();
                    if let Some(term) = engine.active_terminal_mut() {
                        term.selection = Some(crate::core::terminal::TermSelection {
                            start_row: term_row,
                            start_col: term_col,
                            end_row: term_row,
                            end_col: term_col,
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
        if row < menu_rows {
            return sidebar_width;
        }
        let bar_row = row - menu_rows;
        let bar_height = term_height.saturating_sub(menu_rows);
        // Resolve click target using shared function
        let mut ext_names: Vec<_> = engine.ext_panels.keys().cloned().collect();
        ext_names.sort();
        let ab_target =
            crate::core::engine::resolve_activity_bar_click(bar_row, bar_height, &ext_names);
        use crate::core::engine::{ActivityBarTarget, SidebarPanel};
        match ab_target {
            Some(ActivityBarTarget::MenuToggle) => {
                engine.toggle_menu_bar();
                return sidebar_width;
            }
            Some(ActivityBarTarget::ExtensionPanel(name)) => {
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
            _ => {}
        }
        // Map shared SidebarPanel to TUI-local TuiPanel
        let target_panel = match ab_target {
            Some(ActivityBarTarget::Panel(p)) => match p {
                SidebarPanel::Explorer => Some(TuiPanel::Explorer),
                SidebarPanel::Search => Some(TuiPanel::Search),
                SidebarPanel::Debug => Some(TuiPanel::Debug),
                SidebarPanel::Git => Some(TuiPanel::Git),
                SidebarPanel::Extensions => Some(TuiPanel::Extensions),
                SidebarPanel::Ai => Some(TuiPanel::Ai),
            },
            Some(ActivityBarTarget::Settings) => Some(TuiPanel::Settings),
            _ => None,
        };
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
            let content_h =
                term_height.saturating_sub(bottom_chrome + menu_rows + content_start) as usize;
            if col == sb_col && flat_len > content_h && sidebar_row >= content_start {
                let track_start = (content_start + menu_rows) as f32;
                let grab_offset = scrollbar_grab_offset(
                    row as f32,
                    track_start,
                    content_h as f32,
                    content_h,
                    flat_len,
                    engine.ext_panel_scroll_top,
                );
                drag_state.begin(quadraui::DragTarget::ScrollbarY {
                    widget: quadraui::WidgetId::new("ext_panel:sb"),
                    track_start,
                    track_length: content_h as f32,
                    visible_rows: content_h,
                    total_items: flat_len,
                    grab_offset,
                });
                apply_scrollbar_drag(
                    drag_state,
                    quadraui::Point {
                        x: col as f32,
                        y: row as f32,
                    },
                    engine,
                    sidebar,
                    debug_output_scroll,
                );
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
            // tree_height = total height - bottom chrome rows (no header)
            let tree_height = term_height.saturating_sub(bottom_chrome) as usize;
            let total_rows = sidebar.rows.len();

            // Click on the scrollbar column → arm drag, jump via shared dispatch.
            // Thumb-grab preservation: if cursor lands on the visible thumb,
            // grab_offset stores the cursor-to-thumb-top offset so the thumb
            // doesn't snap out from under the user's cursor.
            if col == sb_col && total_rows > tree_height {
                let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                let track_start = menu_rows as f32;
                let grab_offset = scrollbar_grab_offset(
                    row as f32,
                    track_start,
                    tree_height as f32,
                    tree_height,
                    total_rows,
                    sidebar.scroll_top,
                );
                drag_state.begin(quadraui::DragTarget::ScrollbarY {
                    widget: quadraui::WidgetId::new("explorer:sb"),
                    track_start,
                    track_length: tree_height as f32,
                    visible_rows: tree_height,
                    total_items: total_rows,
                    grab_offset,
                });
                apply_scrollbar_drag(
                    drag_state,
                    quadraui::Point {
                        x: col as f32,
                        y: row as f32,
                    },
                    engine,
                    sidebar,
                    debug_output_scroll,
                );
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
                            engine.dap_sidebar_section = *section;
                            // Phase B.4 Stage 5c: shared drag-state.
                            // Widget id encodes section index so the drag
                            // handler can route the offset to the right
                            // `dap_sidebar_scroll[idx]`. apply_scrollbar_drag
                            // computes the click-time offset using the same
                            // thumb-aware math as subsequent drags.
                            let track_start = (items_start + menu_rows) as f32;
                            let grab_offset = scrollbar_grab_offset(
                                row as f32,
                                track_start,
                                sec_height as f32,
                                sec_height as usize,
                                item_count,
                                engine.dap_sidebar_scroll[*sec_idx],
                            );
                            drag_state.begin(quadraui::DragTarget::ScrollbarY {
                                widget: quadraui::WidgetId::new(format!(
                                    "tui:debug_sidebar:{}",
                                    *sec_idx
                                )),
                                track_start,
                                track_length: sec_height as f32,
                                visible_rows: sec_height as usize,
                                total_items: item_count,
                                grab_offset,
                            });
                            apply_scrollbar_drag(
                                drag_state,
                                quadraui::Point {
                                    x: col as f32,
                                    y: row as f32,
                                },
                                engine,
                                sidebar,
                                debug_output_scroll,
                            );
                        } else {
                            // Hit-test through `TreeViewLayout::hit_test()` so
                            // the click path agrees with the paint path by
                            // construction (#281, mirrors the #280 / #210
                            // pattern). For flat 1-cell rows the result is
                            // arithmetically identical to
                            // `scroll_off + (sidebar_row - items_start)`,
                            // but routing through the primitive keeps the
                            // shape future-proof against scroll / wrapping
                            // changes.
                            let theme = Theme::onedark();
                            let screen =
                                render::build_screen_layout(engine, &theme, &[], 1.0, 1.0, true);
                            let sb = &screen.debug_sidebar;
                            let items_for_section = match section {
                                DebugSidebarSection::Variables => &sb.variables,
                                DebugSidebarSection::Watch => &sb.watch,
                                DebugSidebarSection::CallStack => &sb.frames,
                                DebugSidebarSection::Breakpoints => &sb.breakpoints,
                            };
                            let scroll_off = engine.dap_sidebar_scroll[*sec_idx];
                            let tree = render::debug_sidebar_section_to_tree_view(
                                items_for_section,
                                scroll_off,
                                sb.has_focus,
                                sb.session_active,
                                "click",
                            );
                            let layout =
                                tree.layout(sidebar_width as f32, sec_height as f32, |_| {
                                    quadraui::TreeRowMeasure::new(1.0)
                                });
                            let rel_y = (sidebar_row - items_start) as f32;
                            if let quadraui::TreeViewHit::Row(row_idx) = layout.hit_test(0.0, rel_y)
                            {
                                let path = tree.rows[row_idx].path.clone();
                                if let [item_idx_u16] = path.as_slice() {
                                    if *item_idx_u16 != u16::MAX {
                                        engine.dap_sidebar_section = *section;
                                        engine.dap_sidebar_selected = *item_idx_u16 as usize;
                                        engine.handle_debug_sidebar_key("Return", false);
                                    }
                                }
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
                // The SC panel stopped rendering a "(no changes)" placeholder
                // row for expanded-but-empty sections when the section
                // rendering migrated to the TreeView primitive (see the
                // NOTE in `render::source_control_to_tree_view`). Pass
                // `empty_section_hint: false` so the click math matches
                // the actual render — otherwise every row after an
                // expanded-but-empty section is off by +1 and clicking
                // any row highlights the one above it (#184).
                if let Some((flat_idx, is_header)) =
                    engine.sc_visual_row_to_flat(adjusted as usize, false)
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
                    // Phase B.4 Stage 5c: arm drag state via the shared
                    // `quadraui::DragState`. `apply_scrollbar_drag` runs
                    // dispatch_mouse_drag at click time so the click-time
                    // offset matches subsequent drags (no thumb jump).
                    let track_start = (5 + menu_rows) as f32;
                    let grab_offset = scrollbar_grab_offset(
                        row as f32,
                        track_start,
                        results_height as f32,
                        results_height,
                        total_display,
                        sidebar.search_scroll_top,
                    );
                    drag_state.begin(quadraui::DragTarget::ScrollbarY {
                        widget: quadraui::WidgetId::new("tui:search_results"),
                        track_start,
                        track_length: results_height as f32,
                        visible_rows: results_height,
                        total_items: total_display,
                        grab_offset,
                    });
                    apply_scrollbar_drag(
                        drag_state,
                        quadraui::Point {
                            x: col as f32,
                            y: row as f32,
                        },
                        engine,
                        sidebar,
                        debug_output_scroll,
                    );
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

            // Chrome rows match `render_ext_sidebar`: 0 = panel header,
            // 1 = search box. Rows 2+ are the `quadraui::TreeView` body.
            // Hit-test the body via `TreeViewLayout::hit_test()` so the
            // paint and click paths agree by construction (#280, mirrors
            // the #210/#211 pattern).
            if sidebar_row == 0 {
                // Header — no-op
            } else if sidebar_row == 1 {
                engine.ext_sidebar_input_active = true;
            } else {
                let theme = Theme::onedark();
                let screen = render::build_screen_layout(engine, &theme, &[], 1.0, 1.0, true);
                if let Some(ref ext) = screen.ext_sidebar {
                    let installed_count = ext.items_installed.len();
                    let tree = render::ext_sidebar_to_tree_view(ext);
                    // Use f32::MAX for height so hit_test returns Row(idx)
                    // for any visible row index. TUI rows are 1 cell each.
                    let layout = tree.layout(sidebar_width as f32, f32::MAX, |_| {
                        quadraui::TreeRowMeasure::new(1.0)
                    });
                    let rel_y = (sidebar_row - 2) as f32;
                    if let quadraui::TreeViewHit::Row(row_idx) = layout.hit_test(0.0, rel_y) {
                        let path = tree.rows[row_idx].path.clone();
                        let now = Instant::now();
                        let is_double = now.duration_since(*last_click_time)
                            < Duration::from_millis(400)
                            && *last_click_pos == (col, row);
                        *last_click_time = now;
                        *last_click_pos = (col, row);

                        match path.as_slice() {
                            [0] => {
                                engine.ext_sidebar_sections_expanded[0] =
                                    !engine.ext_sidebar_sections_expanded[0];
                            }
                            [1] => {
                                engine.ext_sidebar_sections_expanded[1] =
                                    !engine.ext_sidebar_sections_expanded[1];
                            }
                            [0, item_idx] if *item_idx != u16::MAX => {
                                engine.ext_sidebar_selected = *item_idx as usize;
                                if is_double {
                                    engine.ext_open_selected_readme();
                                }
                            }
                            [1, item_idx] if *item_idx != u16::MAX => {
                                engine.ext_sidebar_selected = installed_count + *item_idx as usize;
                                if is_double {
                                    engine.ext_open_selected_readme();
                                }
                            }
                            _ => {
                                // Empty-state row (item_idx == u16::MAX) or
                                // unknown shape — no-op.
                            }
                        }
                    }
                }
            }
        } else if sidebar.active_panel == TuiPanel::Settings {
            sidebar.has_focus = true;
            engine.settings_has_focus = true;

            // Row 0: header, Row 1: search input, Row 2+: scrollable content
            let content_height = term_height.saturating_sub(4) as usize; // header+search+status+cmd
            let flat_total = engine.settings_flat_list().len();

            // Scrollbar column → arm drag, jump via shared dispatch math
            if col == sb_col && sidebar_row >= 2 && flat_total > content_height {
                let track_start_row = row - (sidebar_row - 2);
                let track_len = content_height as u16;
                let grab_offset = scrollbar_grab_offset(
                    row as f32,
                    track_start_row as f32,
                    track_len as f32,
                    content_height,
                    flat_total,
                    engine.settings_scroll_top,
                );
                drag_state.begin(quadraui::DragTarget::ScrollbarY {
                    widget: quadraui::WidgetId::new("tui:settings"),
                    track_start: track_start_row as f32,
                    track_length: track_len as f32,
                    visible_rows: content_height,
                    total_items: flat_total,
                    grab_offset,
                });
                apply_scrollbar_drag(
                    drag_state,
                    quadraui::Point {
                        x: col as f32,
                        y: row as f32,
                    },
                    engine,
                    sidebar,
                    debug_output_scroll,
                );
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
                    // Theme only feeds segment fg/bg here, which resolve_click
                    // ignores — onedark stand-in is fine for hit-test only.
                    let theme = Theme::onedark();
                    let bar = render::breadcrumbs_to_quadraui_status_bar(
                        &bc.segments,
                        &theme,
                        engine.breadcrumb_focus,
                        engine.breadcrumb_selected,
                    );
                    let local_col = col - bc_x;
                    if let Some(id) = bar.resolve_click(local_col, bc_w as usize) {
                        if let Some(idx) = render::breadcrumb_action_index(&id) {
                            engine.rebuild_breadcrumb_segments();
                            engine.breadcrumb_selected = idx;
                            engine.breadcrumb_open_scoped();
                        }
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
                _bar_width,
                _group_tabs,
                _diff_toolbar_ref,
                _was_active,
                _scroll_offset,
            )) = matched_group
            {
                // Use pre-computed hit regions from the GroupTabBar.
                let hit_target = split
                    .group_tab_bars
                    .iter()
                    .find(|gtb| gtb.group_id == group_id)
                    .and_then(|gtb| {
                        crate::render::resolve_tab_bar_click(&gtb.hit_regions, local_col)
                    });
                if let Some(target) = hit_target {
                    use crate::core::engine::TabBarClickTarget;
                    match target {
                        TabBarClickTarget::Tab(_) => {
                            let needs_confirm = engine.handle_tab_bar_click(group_id, target);
                            if needs_confirm {
                                *close_tab_confirm = true;
                                *close_tab_confirm_focus = 0;
                            }
                            *tab_drag_start = Some((col, row));
                            if let Some(path) = engine.file_path().cloned() {
                                sidebar.reveal_path(&path, term_height.saturating_sub(4) as usize);
                            }
                        }
                        TabBarClickTarget::CloseTab(_) => {
                            let needs_confirm = engine.handle_tab_bar_click(group_id, target);
                            if needs_confirm {
                                *close_tab_confirm = true;
                                *close_tab_confirm_focus = 0;
                            }
                        }
                        TabBarClickTarget::ActionMenu => {
                            engine.active_group = group_id;
                            engine.open_editor_action_menu(group_id, col, row + 1);
                        }
                        _ => {
                            engine.handle_tab_bar_click(group_id, target);
                        }
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

            // B5c.2: hand-rolled tab/diff/split geometry replaced by the
            // primitive's `hit_test` so the click resolution uses the
            // exact same layout the rasteriser painted.
            let bar = render::build_tab_bar_primitive(
                &layout.tab_bar,
                true,
                layout.diff_toolbar.as_ref(),
                scroll_offset,
                None,
            );
            let tab_widths: Vec<usize> = layout
                .tab_bar
                .iter()
                .map(|t| t.name.chars().count() + render::TAB_CLOSE_COLS as usize)
                .collect();
            let bar_layout = bar.layout(
                bar_width as f32,
                1.0,
                0.0,
                |i| quadraui::TabMeasure::new(tab_widths[i] as f32, render::TAB_CLOSE_COLS as f32),
                |i| quadraui::SegmentMeasure::new(bar.right_segments[i].width_cells as f32),
            );
            match bar_layout.hit_test(local_col as f32, 0.0) {
                quadraui::TabBarHit::Tab(i) => {
                    if i < engine.active_group().tabs.len() {
                        engine.goto_tab(i);
                        *tab_drag_start = Some((col, row));
                        engine.lsp_ensure_active_buffer();
                        if let Some(path) = engine.file_path().cloned() {
                            sidebar.reveal_path(&path, term_height.saturating_sub(4) as usize);
                        }
                    }
                }
                quadraui::TabBarHit::TabClose(i) => {
                    if i < engine.active_group().tabs.len() {
                        engine.active_group_mut().active_tab = i;
                        engine.line_annotations.clear();
                        if engine.dirty() {
                            *close_tab_confirm = true;
                            *close_tab_confirm_focus = 0;
                        } else {
                            engine.close_tab();
                        }
                    }
                }
                quadraui::TabBarHit::RightSegment(id) => {
                    let has_win = engine.windows.contains_key(&engine.active_window_id());
                    match id.as_str() {
                        "tab:diff_prev" => {
                            if has_win {
                                engine.jump_prev_hunk();
                            }
                        }
                        "tab:diff_next" => {
                            if has_win {
                                engine.jump_next_hunk();
                            }
                        }
                        "tab:diff_toggle" => {
                            engine.diff_toggle_hide_unchanged();
                        }
                        "tab:split_right" => {
                            engine.open_editor_group(SplitDirection::Vertical);
                        }
                        "tab:split_down" => {
                            engine.open_editor_group(SplitDirection::Horizontal);
                        }
                        "tab:action_menu" => {
                            engine.open_editor_action_menu(engine.active_group, col, row + 1);
                        }
                        _ => {}
                    }
                }
                _ => {}
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

                // Per-window status line (when enabled) occupies the
                // bottom row of the window — render_window subtracts it
                // before computing viewport / scrollbar geometry. Mirror
                // that here so the click hit-tests match what's actually
                // drawn.
                let status_rows: u16 = if rw.status_line.is_some() && wh > 1 {
                    1
                } else {
                    0
                };
                let content_height = wh.saturating_sub(status_rows);
                let viewport_lines = content_height as usize;
                let has_v_scrollbar = rw.total_lines > viewport_lines;
                let gutter = rw.gutter_char_width as u16;
                let viewport_cols = (ww as usize)
                    .saturating_sub(gutter as usize + if has_v_scrollbar { 1 } else { 0 });
                let has_h_scrollbar = rw.max_col > viewport_cols && content_height > 1;

                // Vertical scrollbar click/drag-start (rightmost column)
                if has_v_scrollbar && rel_col == wx + ww - 1 {
                    // menu_rows = menu bar offset; wy already includes tab_bar_height
                    let track_abs_start = menu_rows + wy;
                    // V-track loses 1 row to each of: per-window status line,
                    // horizontal scrollbar (if either present).
                    let track_len =
                        content_height.saturating_sub(if has_h_scrollbar { 1 } else { 0 });
                    let track_visible = track_len as usize;
                    // Track-click vs thumb-click: page-jump on empty
                    // track, drag-start on thumb. Standard editor UX —
                    // clicking the empty track moves by one viewport
                    // toward the click direction; clicking the thumb
                    // begins a drag.
                    let (thumb_start, thumb_len) = quadraui::fit_thumb(
                        rw.scroll_top as f32,
                        rw.total_lines as f32,
                        track_visible as f32,
                        track_len as f32,
                        1.0,
                    );
                    let thumb_top = thumb_start.floor() as u16;
                    let thumb_size = thumb_len.ceil().max(1.0) as u16;
                    let cursor_offset = row.saturating_sub(track_abs_start);
                    if cursor_offset < thumb_top {
                        let new_scroll = rw.scroll_top.saturating_sub(track_visible);
                        engine.set_scroll_top_for_window(rw.window_id, new_scroll);
                        engine.sync_scroll_binds();
                        return sidebar_width;
                    } else if cursor_offset >= thumb_top.saturating_add(thumb_size) {
                        let max_scroll = rw.total_lines.saturating_sub(track_visible);
                        let new_scroll = (rw.scroll_top + track_visible).min(max_scroll);
                        engine.set_scroll_top_for_window(rw.window_id, new_scroll);
                        engine.sync_scroll_binds();
                        return sidebar_width;
                    }
                    // Phase B.4 Stage 5d: editor scrollbars on the shared
                    // `quadraui::DragState`. Widget id encodes the window id
                    // so the apply-side router can call
                    // `engine.set_scroll_*_for_window(...)` against the
                    // right window. `grab_offset` preserves cursor position
                    // on the thumb during drag — same UX every other
                    // migrated scrollbar gives.
                    let grab_offset = scrollbar_grab_offset(
                        row as f32,
                        track_abs_start as f32,
                        track_len as f32,
                        track_visible,
                        rw.total_lines,
                        rw.scroll_top,
                    );
                    drag_state.begin(quadraui::DragTarget::ScrollbarY {
                        widget: quadraui::WidgetId::new(format!(
                            "tui:editor:{}:vsb",
                            rw.window_id.0
                        )),
                        track_start: track_abs_start as f32,
                        track_length: track_len as f32,
                        visible_rows: track_visible,
                        total_items: rw.total_lines,
                        grab_offset,
                    });
                    apply_scrollbar_drag(
                        drag_state,
                        quadraui::Point {
                            x: col as f32,
                            y: row as f32,
                        },
                        engine,
                        sidebar,
                        debug_output_scroll,
                    );
                    engine.sync_scroll_binds();
                    return sidebar_width;
                }

                // Horizontal scrollbar click/drag-start.
                // The renderer reserves the bottommost row of the window
                // for a per-window status line when one is enabled, then
                // shrinks the content area and draws the h-scrollbar at
                // the last row of the *shrunken* area. So the h-scrollbar
                // sits at `wy + wh - 1` when no per-window status line
                // and `wy + wh - 2` when there is one.
                let h_sb_row = if rw.status_line.is_some() && wh > 1 {
                    wy + wh - 2
                } else {
                    wy + wh - 1
                };
                if has_h_scrollbar && editor_row == h_sb_row {
                    let track_x = wx + gutter;
                    let track_w = ww.saturating_sub(gutter + if has_v_scrollbar { 1 } else { 0 });
                    if rel_col >= track_x && rel_col < track_x + track_w && track_w > 0 {
                        let track_abs_start = editor_left + track_x;
                        let track_visible = viewport_cols;
                        // Track-click vs thumb-click: page-jump on the
                        // empty track, drag-start on the thumb (mirrors
                        // the v-scrollbar above).
                        let (thumb_start, thumb_len) = quadraui::fit_thumb(
                            rw.scroll_left as f32,
                            rw.max_col as f32,
                            track_visible as f32,
                            track_w as f32,
                            1.0,
                        );
                        let thumb_left = thumb_start.floor() as u16;
                        let thumb_size = thumb_len.ceil().max(1.0) as u16;
                        let cursor_offset = col.saturating_sub(track_abs_start);
                        if cursor_offset < thumb_left {
                            let new_left = rw.scroll_left.saturating_sub(track_visible);
                            engine.set_scroll_left_for_window(rw.window_id, new_left);
                            return sidebar_width;
                        } else if cursor_offset >= thumb_left.saturating_add(thumb_size) {
                            let max_left = rw.max_col.saturating_sub(track_visible);
                            let new_left = (rw.scroll_left + track_visible).min(max_left);
                            engine.set_scroll_left_for_window(rw.window_id, new_left);
                            return sidebar_width;
                        }
                        let grab_offset = scrollbar_grab_offset(
                            col as f32,
                            track_abs_start as f32,
                            track_w as f32,
                            track_visible,
                            rw.max_col,
                            rw.scroll_left,
                        );
                        drag_state.begin(quadraui::DragTarget::ScrollbarX {
                            widget: quadraui::WidgetId::new(format!(
                                "tui:editor:{}:hsb",
                                rw.window_id.0
                            )),
                            track_start: track_abs_start as f32,
                            track_length: track_w as f32,
                            visible_cols: track_visible,
                            total_cols: rw.max_col,
                            grab_offset,
                        });
                        apply_scrollbar_drag(
                            drag_state,
                            quadraui::Point {
                                x: col as f32,
                                y: row as f32,
                            },
                            engine,
                            sidebar,
                            debug_output_scroll,
                        );
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
///
/// Per D6: builds the StatusBar primitive and its layout, then calls
/// `StatusBarLayout::hit_test()`. Same layout math as the draw path
/// (`render_window_status_line`), so clicks on dropped (invisible)
/// segments can't fire — the layout's hit_regions only include segments
/// that actually rendered.
fn status_segment_hit_test(
    status: &crate::render::WindowStatusLine,
    width: usize,
    click_col: usize,
) -> Option<crate::render::StatusAction> {
    let bar = crate::render::window_status_line_to_status_bar(
        status,
        quadraui::WidgetId::new("status:window"),
    );
    // Must match the min_gap used in render_window_status_line.
    const MIN_GAP_CELLS: f32 = 2.0;
    let layout = bar.layout(width as f32, 1.0, MIN_GAP_CELLS, |seg| {
        quadraui::StatusSegmentMeasure::new(seg.text.chars().count() as f32)
    });
    match layout.hit_test(click_col as f32, 0.0) {
        quadraui::StatusBarHit::Segment(id) => crate::render::status_action_from_id(id.as_str()),
        quadraui::StatusBarHit::Empty => None,
    }
}
