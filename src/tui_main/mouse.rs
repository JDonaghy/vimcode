use super::*;

use crate::core::engine::TabBarClickTarget;

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
    _sidebar: &mut TuiSidebar,
) -> bool {
    let events = quadraui::dispatch_mouse_drag(drag_state, point, Default::default());
    let mut handled = false;
    for ev in &events {
        if let quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset } = ev {
            let key = widget.as_str();
            match key {
                "explorer:sb" => {
                    engine
                        .explorer_tree
                        .borrow_mut()
                        .set_scroll_offset(*new_offset);
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
                    handled = true;
                }
                // Inverted scrollbars: top of track = max offset (oldest
                // content), bottom = 0 (newest). dispatch_mouse_drag
                // reports the raw forward offset; flip it here.
                "terminal_scrollback" => {
                    if let Some(term) = engine.active_terminal_mut() {
                        term.set_scroll_offset(*new_offset);
                    }
                    handled = true;
                }
                "tui:debug_output" => {
                    engine.debug_output_scroll = *new_offset;
                    engine.debug_output_auto_scroll = false;
                    handled = true;
                }
                // debug_sidebar:N — no longer needed, SidebarSystem handles scrollbar internally
                other if other.starts_with("debug_sidebar:") => {
                    handled = true;
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

/// Encode a text-drag `WidgetId` for the given editor window, following
/// the `tui:editor:<window_id>:<vsb|hsb>` scrollbar convention above (see
/// `apply_scrollbar_drag`). Used to arm a [`quadraui::DragTarget::TextSelection`]
/// at mouse-down so the subsequent `Drag` events can recover which window
/// "owns" the in-progress visual-selection drag (#565).
fn text_drag_widget_id(window_id: crate::core::WindowId) -> quadraui::WidgetId {
    quadraui::WidgetId::new(format!("tui:editor:{}:text", window_id.0))
}

/// Inverse of [`text_drag_widget_id`]: recover the origin window id from a
/// `DragTarget::TextSelection` region, if it matches the expected format.
/// Returns `None` for anything else (defensive — falls back to the
/// engine's own `mouse_drag_origin_window` guard in that case).
fn text_drag_origin_window(region: &quadraui::WidgetId) -> Option<crate::core::WindowId> {
    region
        .as_str()
        .strip_prefix("tui:editor:")
        .and_then(|rest| rest.strip_suffix(":text"))
        .and_then(|wid_str| wid_str.parse::<usize>().ok())
        .map(crate::core::WindowId)
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
    dragging_terminal_resize: &mut bool,
    dragging_terminal_split: &mut bool,
    dragging_group_divider: &mut Option<usize>,
    dragging_window_divider: &mut Option<(crate::core::window::GroupId, usize)>,
    drag_state: &mut quadraui::DragState,
    modal_stack: &mut quadraui::ModalStack,
    last_layout: Option<&render::ScreenLayout>,
    last_click_time: &mut Instant,
    last_click_pos: &mut (u16, u16),
    folder_picker: &mut Option<FolderPickerState>,
    cmd_sel: &mut Option<(usize, usize)>,
    cmd_dragging: &mut bool,
    should_quit: &mut bool,
    explorer_drag_src: &mut Option<usize>,
    explorer_drag_active: &mut Option<(usize, Option<usize>)>,
    tab_drag_start: &mut Option<(u16, u16)>,
    tab_dragging: &mut bool,
    tab_drag_source: &mut Option<(crate::core::window::GroupId, usize)>,
    tab_drag_cursor: &mut Option<(f64, f64)>,
    tab_drop_zone: &mut crate::core::window::DropZone,
    hover_link_rects: &[(u16, u16, u16, u16, String)],
    hover_popup_rect: Option<(u16, u16, u16, u16)>,
    editor_hover_popup_rect: Option<(u16, u16, u16, u16)>,
    editor_hover_link_rects: &[(u16, u16, u16, u16, String)],
    editor_hover_scrollbar: Option<crate::render::PopupScrollbarHit>,
    hover_selecting: &mut bool,
    fr_input_dragging: &mut bool,
    completion_layout: Option<&quadraui::CompletionsLayout>,
    context_menu_layout: Option<&quadraui::ContextMenuLayout>,
    dialog_layout: Option<&quadraui::DialogLayout>,
) -> u16 {
    let col = ev.column;
    let row = ev.row;
    let term_height = terminal_size.map(|s| s.height).unwrap_or(24);

    // ── Quit-confirm overlay click interception ─────────────────────────────
    // Route clicks through DialogLayout::hit_test. Swallow all clicks while
    // the overlay is visible so they don't fall through to the editor.
    let sb_visible = engine.app_shell.sidebar_visible();
    let ab_width = if engine.settings.autohide_panels && !sb_visible {
        0
    } else {
        ACTIVITY_BAR_WIDTH
    };
    let editor_left = ab_width + if sb_visible { sidebar_width + 1 } else { 0 };

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

    // Reconcile context menu with modal stack (#459).
    // Push the menu's outer bounds whenever a context menu is open so
    // panel intercepts in mod.rs can call modal_stack.hit_test() instead
    // of the per-backend engine.context_menu.is_none() gate.
    {
        let ctx_menu_id = quadraui::WidgetId::new("context_menu");
        match context_menu_layout {
            Some(layout) => modal_stack.push(ctx_menu_id, layout.bounds),
            None => {
                modal_stack.pop(&ctx_menu_id);
            }
        }
    }

    // Reconcile stale picker modal: if the picker closed (keyboard
    // Escape / confirm) without a backdrop-dismiss click, the "picker"
    // entry lingers on the stack and swallows all dispatch_scroll events.
    if !engine.picker_open {
        modal_stack.pop(&quadraui::WidgetId::new("picker"));
    }

    // ── Toast click (× dismiss / action) ───────────────────────────────────────
    // #450: toasts overlay the editor in the bottom-right. Run hit_test
    // before any underlying handler so clicking × dismisses the toast
    // instead of falling through to whatever sits underneath.
    if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
        let toast_hit = engine
            .toast_layout
            .borrow()
            .as_ref()
            .map(|layout| layout.hit_test(col as f32, row as f32));
        if let Some(hit) = toast_hit {
            if engine.handle_toast_hit(hit) {
                return sidebar_width;
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

            // Compute panel screen position from group_bounds. #550:
            // `group_bounds` is already absolute terminal-screen space (see
            // the matching comment at the `draw_find_replace` call site in
            // render_impl.rs, which now passes `editor_left=0` into
            // quadraui's rasteriser since the offset is baked into
            // `group_bounds` already). This hit-test math must mirror that
            // paint math exactly (mismatched math here is the "column drift
            // bug" class this overlay's doc comment warns about) — so no
            // `editor_left +`/`.max(editor_left)` here either, matching
            // quadraui's now-effectively-zero clamp.
            let gb = &panel.group_bounds;
            let gb_right = gb.x as u16 + gb.width as u16;
            let panel_x = gb_right.saturating_sub(panel_w + 1);
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
    // #546: resolve clicks via the `DialogLayout` cached from the render step
    // (`render::dialog_generic_layout`, called once in `draw_frame`) instead
    // of recomputing an independently-formulated layout here — the two
    // copies could (and did, subtly: `.len()` byte count vs `.chars().count()`)
    // drift apart. Mirrors the context-menu handling just below, which
    // already consumes its render-cached layout the same way.
    if engine.dialog.is_some() {
        if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
            // No cached layout yet (dialog opened this frame, not yet
            // painted) — treat as an inside/no-op click rather than risk
            // dismissing a dialog the user hasn't even seen painted.
            let hit = dialog_layout
                .map(|dl| dl.hit_test(col as f32, row as f32))
                .unwrap_or(quadraui::DialogHit::Body);
            match hit {
                quadraui::DialogHit::Button(id) => {
                    if let Some(idx) = id
                        .as_str()
                        .strip_prefix("dialog:btn:")
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        let action = engine.dialog_click_button(idx);
                        if engine.explorer_needs_refresh {
                            engine.explorer_needs_refresh = false;
                            engine.explorer_rebuild_rows();
                        }
                        if handle_action(engine, action) {
                            *should_quit = true;
                        }
                        return sidebar_width;
                    }
                }
                quadraui::DialogHit::BodyToolbarButton(_) | quadraui::DialogHit::Body => {}
                quadraui::DialogHit::Outside => {
                    engine.dialog = None;
                    engine.pending_move = None;
                }
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
                let visible_rows =
                    if let Some(quadraui::DragTarget::ScrollbarY { track_length, .. }) =
                        drag_state.target()
                    {
                        *track_length as usize
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
                let geo = render::PickerGeometry::compute(
                    term_cols as f32,
                    term_rows as f32,
                    has_preview,
                    &render::TUI_PICKER_SIZING,
                );
                let popup_x = geo.popup_x.round() as u16;
                let popup_y = geo.popup_y.round() as u16;
                let popup_w = geo.popup_w.round() as u16;
                let popup_h = geo.popup_h.round() as u16;
                let total_items = engine.picker_items.len();

                let list_w = if has_preview {
                    ((popup_w as f32) * 0.4).round() as u16
                } else {
                    popup_w
                };
                let items_row0 = popup_y + 3;
                let items_row_end = popup_y + popup_h - 1;
                let visible_rows = items_row_end.saturating_sub(items_row0) as usize;

                let max_offset = total_items.saturating_sub(visible_rows);
                let effective_offset = if visible_rows == 0 {
                    0
                } else if engine.picker_selected < engine.picker_scroll_top {
                    engine.picker_selected
                } else if engine.picker_selected >= engine.picker_scroll_top + visible_rows {
                    engine.picker_selected + 1 - visible_rows
                } else {
                    engine.picker_scroll_top
                }
                .min(max_offset);

                let picker_id = quadraui::WidgetId::new("picker");
                modal_stack.push(
                    picker_id.clone(),
                    quadraui::Rect {
                        x: geo.popup_x,
                        y: geo.popup_y,
                        width: geo.popup_w,
                        height: geo.popup_h,
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
                    let has_scrollbar = total_items > visible_rows;
                    let sb_col = popup_x + list_w - 1;
                    let on_scrollbar = has_scrollbar
                        && col == sb_col
                        && row >= items_row0
                        && row < items_row_end
                        && visible_rows > 0;

                    if on_scrollbar {
                        let tl = visible_rows as f32;
                        let thumb_len =
                            (tl * visible_rows as f32 / total_items.max(1) as f32).max(1.0);
                        let max_scroll = total_items.saturating_sub(visible_rows);
                        let grab_offset = scrollbar_grab_offset(
                            row as f32,
                            items_row0 as f32,
                            tl,
                            visible_rows,
                            total_items,
                            effective_offset,
                        );
                        let on_thumb = grab_offset > 0.0 || {
                            let eff_track = (tl - thumb_len).max(1.0);
                            let ratio = if max_scroll == 0 {
                                0.0
                            } else {
                                effective_offset as f32 / max_scroll as f32
                            };
                            let thumb_top = items_row0 as f32 + ratio * eff_track;
                            let dy = row as f32 - thumb_top;
                            dy >= 0.0 && dy < thumb_len
                        };

                        if on_thumb {
                            drag_state.begin(quadraui::DragTarget::ScrollbarY {
                                widget: picker_id.clone(),
                                track_start: items_row0 as f32,
                                track_length: tl,
                                thumb_length: thumb_len,
                                max_scroll,
                                grab_offset,
                                inverted: false,
                            });
                        } else {
                            let click_above_thumb = {
                                let eff_track = (tl - thumb_len).max(1.0);
                                let ratio = if max_scroll == 0 {
                                    0.0
                                } else {
                                    effective_offset as f32 / max_scroll as f32
                                };
                                let thumb_top = items_row0 as f32 + ratio * eff_track;
                                (row as f32) < thumb_top
                            };
                            let page = visible_rows.max(1);
                            let new_offset = if click_above_thumb {
                                effective_offset.saturating_sub(page)
                            } else {
                                (effective_offset + page).min(max_scroll)
                            };
                            engine.picker_scroll_top = new_offset;
                            if engine.picker_selected < new_offset {
                                engine.picker_selected = new_offset;
                            } else if engine.picker_selected >= new_offset + visible_rows {
                                engine.picker_selected = new_offset + visible_rows - 1;
                            }
                            engine.picker_load_preview();
                        }
                    } else if row >= items_row0 && row < items_row_end {
                        let clicked_idx = effective_offset + (row - items_row0) as usize;
                        if clicked_idx < engine.picker_items.len() {
                            if engine.picker_selected == clicked_idx {
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
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let term_cols = terminal_size.map(|s| s.width).unwrap_or(80);
                let term_rows = terminal_size.map(|s| s.height).unwrap_or(24);
                let has_preview = engine.picker_preview.is_some();
                let geo = render::PickerGeometry::compute(
                    term_cols as f32,
                    term_rows as f32,
                    has_preview,
                    &render::TUI_PICKER_SIZING,
                );
                let popup_x = geo.popup_x.round() as u16;
                let left_w = geo.left_pane_w.round() as u16;
                let scroll_down = matches!(ev.kind, MouseEventKind::ScrollDown);
                if has_preview && col > popup_x + left_w {
                    if scroll_down {
                        let max = engine
                            .picker_preview
                            .as_ref()
                            .map(|p| p.lines.len())
                            .unwrap_or(0);
                        engine.picker_preview_scroll =
                            (engine.picker_preview_scroll + 3).min(max.saturating_sub(1));
                    } else {
                        engine.picker_preview_scroll =
                            engine.picker_preview_scroll.saturating_sub(3);
                    }
                } else {
                    let delta = if scroll_down { 3 } else { -3 };
                    engine.picker_scroll(delta, geo.visible_rows);
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
    let sep_col = ab_width + if sb_visible { sidebar_width } else { 0 };
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) if sb_visible && col == sep_col => {
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
        MouseEventKind::Drag(MouseButton::Left)
            if sb_visible
                && col >= ab_width
                && col < ab_width + sidebar_width
                && engine.active_panel_is(PANEL_SEARCH) =>
        {
            let move_ev = quadraui::UiEvent::MouseMoved {
                position: quadraui::Point::new(col as f32, row as f32),
                buttons: quadraui::ButtonMask {
                    left: true,
                    right: false,
                    middle: false,
                },
            };
            engine.handle_search_sidebar_ui_event(move_ev);
            return sidebar_width;
        }
        MouseEventKind::Drag(MouseButton::Left)
            if sb_visible
                && engine.active_panel_is(PANEL_SETTINGS)
                && col >= ab_width
                && col < ab_width + sidebar_width =>
        {
            let content_start = 2_u16;
            let content_height = term_height.saturating_sub(4);
            let q_rect = quadraui::Rect::new(
                ab_width as f32,
                content_start as f32,
                sidebar_width as f32,
                content_height as f32,
            );
            let move_ev = quadraui::UiEvent::MouseMoved {
                position: quadraui::Point::new(col as f32, row as f32),
                buttons: quadraui::ButtonMask {
                    left: true,
                    middle: false,
                    right: false,
                },
            };
            render::populate_settings_form_controller(engine);
            let result = engine
                .settings_form_controller
                .borrow_mut()
                .handle_cached(&move_ev, q_rect);
            if !matches!(result, quadraui::FormControllerEvent::Ignored) {
                engine.settings_scroll_top =
                    engine.settings_form_controller.borrow().scroll_offset();
            }
            return sidebar_width;
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            // Explorer drag-and-drop: activate or update target row.
            if explorer_drag_src.is_some() || explorer_drag_active.is_some() {
                let menu_rows: u16 = if engine.menu_bar_visible { 1 } else { 0 };
                if sb_visible
                    && engine.active_panel_is(PANEL_EXPLORER)
                    && col >= ab_width
                    && col < ab_width + sidebar_width
                {
                    let sidebar_row = row.saturating_sub(menu_rows);
                    if sidebar_row >= 1 {
                        let tree_row = (sidebar_row as usize).saturating_sub(1)
                            + engine.explorer_tree.borrow().scroll_offset();
                        if tree_row < engine.explorer_rows.len() {
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
                *tab_drag_cursor = Some((col as f64, row as f64));
                *tab_drop_zone = compute_tui_tab_drop_zone(
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
                    *tab_drag_source = Some((gid, tidx));
                    *tab_drag_cursor = Some((col as f64, row as f64));
                    *tab_drop_zone = crate::core::window::DropZone::None;
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
            // - `tui:search_results`, `tui:debug_sidebar:N` (5c)
            // - `terminal_scrollback`, `tui:debug_output` (5c, inverted)
            if drag_state.is_active() {
                let point = quadraui::Point {
                    x: col as f32,
                    y: row as f32,
                };
                if apply_scrollbar_drag(drag_state, point, engine, sidebar) {
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
            // #550: `div.axis_start`/`.axis_size` are already absolute
            // terminal-screen coordinates, so `col`/`row` compare directly
            // with no editor-origin subtraction.
            if let Some(split_index) = *dragging_group_divider {
                if let Some(split) = last_layout.and_then(|l| l.editor_group_split.as_ref()) {
                    if let Some(div) = split.dividers.iter().find(|d| d.split_index == split_index)
                    {
                        let new_ratio = render::divider_ratio_from_pos(div, col as f64, row as f64);
                        engine
                            .group_layout
                            .set_ratio_at_index(split_index, new_ratio);
                    }
                }
                return sidebar_width;
            }
            // Window-split divider drag — update ratio based on mouse position (#582).
            if let Some((group_id, split_index)) = *dragging_window_divider {
                if let Some(layout) = last_layout {
                    if let Some(div) = layout
                        .window_dividers
                        .iter()
                        .find(|d| d.group_id == group_id && d.split_index == split_index)
                    {
                        let new_ratio = render::divider_ratio_from_pos(div, col as f64, row as f64);
                        if let Some(group) = engine.editor_groups.get_mut(&group_id) {
                            group
                                .active_tab_mut()
                                .layout
                                .set_ratio_at_index(split_index, new_ratio);
                        }
                    }
                }
                return sidebar_width;
            }
            // Terminal split divider drag — update visual column position (no PTY resize yet).
            if *dragging_terminal_split {
                let panel_col = col.saturating_sub(editor_left);
                let screen_w = terminal_size.map(|s| s.width).unwrap_or(80);
                let panel_w = screen_w.saturating_sub(editor_left);
                let left_cols = panel_col.clamp(5, panel_w.saturating_sub(6));
                engine.terminal_split_set_drag_cols(left_cols);
                return sidebar_width;
            }
            // Phase B.4 Stage 5c: terminal scrollback + debug output
            // scrollbars are inverted (top of track = oldest content,
            // bottom = live view). Their drag math now lives in the
            // shared `if drag_state.is_active()` block above; the
            // receive site for `terminal_scrollback` /
            // `tui:debug_output` flips the offset with `max - new_offset`
            // so `term.set_scroll_offset` / `engine.debug_output_scroll`
            // continue to mean "lines from the bottom".

            // Phase B.4 Stage 5d: editor-window scrollbar drag math now
            // lives in the shared `if drag_state.is_active()` block above
            // via `tui:editor:N:vsb` / `tui:editor:N:hsb` widget ids. The
            // legacy `dragging_scrollbar` local + `ScrollDragState` are
            // gone.
            // Text drag-to-select — find window under cursor and extend visual
            // selection. #565: the drag-origin arbitration (which window this
            // gesture "belongs to", so a drag can't leak into or be hijacked
            // by another split) now flows through the `DragTarget::TextSelection`
            // armed at mouse-down, mirroring how scrollbar drags carry their
            // owning widget id — replacing the old bespoke `mouse_text_drag`
            // bool. The document-model hit-testing below
            // (`window_zone_hit_test` → `buf_line`/`col` → `engine.mouse_drag`)
            // is unchanged.
            let text_drag_origin = match drag_state.target() {
                Some(quadraui::DragTarget::TextSelection { region, .. }) => {
                    text_drag_origin_window(region)
                }
                _ => None,
            };
            let text_drag_armed = matches!(
                drag_state.target(),
                Some(quadraui::DragTarget::TextSelection { .. })
            );
            if col >= editor_left {
                if let Some(layout) = last_layout {
                    // #550: `rw.rect` (and everything `find_window_at`/
                    // `window_zone_hit_test` compare it against) is already
                    // absolute terminal-screen space, so the raw event
                    // `col`/`row` are used directly — no editor-area-relative
                    // translation.
                    if let Some(idx) = render::find_window_at(layout, col as f64, row as f64) {
                        let rw = &layout.windows[idx];
                        let zone = render::window_zone_hit_test(
                            rw,
                            (col as f64) - rw.rect.x,
                            (row as f64) - rw.rect.y,
                            1.0,
                            1.0,
                        );
                        if let render::WindowZone::TextArea {
                            view_row, buf_line, ..
                        } = zone
                        {
                            // Cross-split guard: if this drag started in a
                            // different window, ignore it here — matches the
                            // engine's own `mouse_drag_origin_window` guard
                            // (kept as defense-in-depth) but decided earlier,
                            // from the arbitrated drag target.
                            let cross_split = text_drag_origin.is_some_and(|w| w != rw.window_id);
                            if !cross_split {
                                // #560: resolve via the shared quadraui
                                // text-layout inverse (`EditorLayout::col_at_x`)
                                // instead of hand-rolled cell math, so TUI and
                                // GTK column resolution can never diverge.
                                // `col_at_x` takes an absolute x matching
                                // `editor.rect`'s space (mirrors GTK's
                                // `editor_col_at_x` call, see gtk/click.rs).
                                let (editor, editor_layout) =
                                    render::editor_text_layout(rw, 1.0, 1.0);
                                let col_in_text =
                                    editor_layout.col_at_x(&editor, view_row, col as f32);
                                engine.mouse_drag(rw.window_id, buf_line, col_in_text);
                            }
                            return sidebar_width;
                        }
                    }
                }
                // Editor drag moved outside all windows (e.g. into terminal area) —
                // stop processing so it doesn't bleed into other panels.
                if text_drag_armed {
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
                    // #444: pane-relative col. Click uses
                    // TerminalSplitLayout::hit_test which returns
                    // 0-based col from the active pane's left edge.
                    // Drag must match, otherwise right-pane drag
                    // overshoots by left_pane_cols. Left pane is
                    // unaffected because left.x == editor_left.
                    let split_layout = engine.terminal_split_layout.borrow();
                    let active_pane_x = if let Some(ref sl) = *split_layout {
                        if engine.terminal_active == 1 {
                            sl.right.x as u16
                        } else {
                            sl.left.x as u16
                        }
                    } else {
                        editor_left
                    };
                    drop(split_layout);
                    let term_col = col.saturating_sub(active_pane_x);
                    // #533: shared drag handler — tries forward_mouse(Move)
                    // when the child has mouse reporting, falls back to
                    // local selection update.
                    engine.handle_terminal_pane_drag(term_col, term_row);
                    return sidebar_width;
                }
            }
        }
        MouseEventKind::Up(MouseButton::Left)
            if sb_visible && engine.active_panel_is(PANEL_SEARCH) =>
        {
            let up_ev = quadraui::UiEvent::MouseUp {
                widget: None,
                button: quadraui::MouseButton::Left,
                position: quadraui::Point::new(col as f32, row as f32),
            };
            engine.handle_search_sidebar_ui_event(up_ev);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            // Tab drag-and-drop: execute drop on release.
            if *tab_dragging {
                *tab_dragging = false;
                *tab_drag_start = None;
                if let Some((src_gid, src_tab_idx)) = tab_drag_source.take() {
                    let zone = *tab_drop_zone;
                    *tab_drop_zone = crate::core::window::DropZone::None;
                    engine.apply_tab_drop_zone(src_gid, src_tab_idx, zone);
                }
                *tab_drag_cursor = None;
                return sidebar_width;
            }
            *tab_drag_start = None;
            // Explorer drag-and-drop: execute move on release.
            if let Some((src_row, Some(target_row))) = explorer_drag_active.take() {
                *explorer_drag_src = None;
                if src_row < engine.explorer_rows.len() && target_row < engine.explorer_rows.len() {
                    let src_path = engine.explorer_rows[src_row].path.clone();
                    let target = &engine.explorer_rows[target_row];
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
            // `drag_state.end()` — single source of truth. #565: text-selection
            // drags (`DragTarget::TextSelection`) clear here too, replacing the
            // old separate `mouse_text_drag = false` reset.
            drag_state.end();
            *dragging_group_divider = None;
            *dragging_window_divider = None;
            *cmd_dragging = false;
            *hover_selecting = false;
            if *dragging_terminal_resize {
                *dragging_terminal_resize = false;
                let rows = engine.session.terminal_panel_rows;
                let screen_w = terminal_size.map(|s| s.width).unwrap_or(80);
                let cols = screen_w.saturating_sub(editor_left);
                engine.terminal_resize(cols, rows);
                let _ = engine.session.save();
            }
            if *dragging_terminal_split {
                *dragging_terminal_split = false;
                let left_cols = engine.terminal_split_left_cols;
                if left_cols > 0 {
                    let screen_w = terminal_size.map(|s| s.width).unwrap_or(80);
                    let panel_w = screen_w.saturating_sub(editor_left);
                    let right_cols = panel_w.saturating_sub(left_cols).saturating_sub(1);
                    let rows = engine.session.terminal_panel_rows;
                    engine.terminal_split_finalize_drag(left_cols, right_cols, rows);
                }
            }
            engine.mouse_drag_active = false;
            engine.mouse_drag_origin_window = None;
            // #533: auto-copy terminal selection to clipboard on
            // mouse-release via shared engine method (mirrors GTK).
            engine.terminal_autocopy_selection();
            return sidebar_width;
        }
        // Scroll wheel — sidebar or editor
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let scroll_up = matches!(ev.kind, MouseEventKind::ScrollUp);
            // When an extension panel is showing, its surface bounds (registered
            // by `render_ext_panel`) own the sidebar area — the activity-bar
            // `active_panel_id` is unchanged from before the ext panel opened,
            // so without this guard the wheel scrolls whatever the underlying
            // panel was (#485 was explorer routing).
            let ext_panel_showing = sidebar.ext_panel_name.is_some();
            if sb_visible
                && col >= ab_width
                && col < ab_width + sidebar_width
                && !ext_panel_showing
                && engine.active_panel_is(PANEL_EXPLORER)
            {
                let delta = if scroll_up { -3_isize } else { 3 };
                engine.explorer_scroll(delta);
                return sidebar_width;
            }
            if sb_visible
                && col >= ab_width
                && col < ab_width + sidebar_width
                && !ext_panel_showing
                && engine.active_panel_is(PANEL_GIT)
            {
                let scroll_ev = quadraui::UiEvent::Scroll {
                    widget: None,
                    delta: quadraui::ScrollDelta::new(0.0, if scroll_up { 3.0 } else { -3.0 }),
                    position: quadraui::Point::new(col as f32, row as f32),
                };
                engine.handle_sc_sidebar_ui_event(scroll_ev);
                return sidebar_width;
            }
            if sb_visible
                && col >= ab_width
                && col < ab_width + sidebar_width
                && !ext_panel_showing
                && engine.active_panel_is(PANEL_SEARCH)
            {
                let scroll_ev = quadraui::UiEvent::Scroll {
                    widget: None,
                    delta: quadraui::ScrollDelta::new(0.0, if scroll_up { 3.0 } else { -3.0 }),
                    position: quadraui::Point::new(col as f32, row as f32),
                };
                engine.handle_search_sidebar_ui_event(scroll_ev);
                return sidebar_width;
            }
            if sb_visible
                && col >= ab_width
                && col < ab_width + sidebar_width
                && !ext_panel_showing
                && engine.active_panel_is(PANEL_SETTINGS)
            {
                let content_start = 2_u16;
                let content_height = term_height.saturating_sub(4);
                let q_rect = quadraui::Rect::new(
                    ab_width as f32,
                    content_start as f32,
                    sidebar_width as f32,
                    content_height as f32,
                );
                let scroll_ev = quadraui::UiEvent::Scroll {
                    widget: None,
                    delta: quadraui::ScrollDelta::new(0.0, if scroll_up { 3.0 } else { -3.0 }),
                    position: quadraui::Point::new(col as f32, row as f32),
                };
                render::populate_settings_form_controller(engine);
                let result = engine
                    .settings_form_controller
                    .borrow_mut()
                    .handle_cached(&scroll_ev, q_rect);
                if !matches!(result, quadraui::FormControllerEvent::Ignored) {
                    engine.settings_scroll_top =
                        engine.settings_form_controller.borrow().scroll_offset();
                }
                return sidebar_width;
            }
            // Terminal panel scroll now routes through dispatch_scroll
            // via the registered "terminal_scrollback" surface.
            // Scroll-surface wheel dispatch — routes to registered surfaces.
            {
                let surfaces = engine.scroll_surfaces.borrow();
                let delta_y = if matches!(ev.kind, MouseEventKind::ScrollUp) {
                    -1.0
                } else {
                    1.0
                };
                let scroll_events = quadraui::dispatch_scroll(
                    modal_stack,
                    &surfaces,
                    quadraui::Point {
                        x: col as f32,
                        y: row as f32,
                    },
                    quadraui::ScrollDelta::new(0.0, delta_y),
                );
                drop(surfaces);
                for sev in &scroll_events {
                    if let quadraui::UiEvent::Scroll {
                        widget: Some(id),
                        delta,
                        ..
                    } = sev
                    {
                        let step = (delta.y.abs() * 3.0).round() as usize;
                        let down = delta.y > 0.0;
                        match id.as_str() {
                            "editor_hover" => {
                                let signed = if down { step as i32 } else { -(step as i32) };
                                engine.editor_hover_scroll(signed);
                                return sidebar_width;
                            }
                            "debug_output" => {
                                engine.handle_debug_output_scroll(delta.y);
                                return sidebar_width;
                            }
                            "explorer:sb" => {
                                let delta = if down {
                                    step as isize
                                } else {
                                    -(step as isize)
                                };
                                engine.explorer_scroll(delta);
                                return sidebar_width;
                            }
                            "ext_panel:sb" => {
                                let flat_len = engine.ext_panel_flat_len();
                                if down {
                                    engine.ext_panel_scroll_top = (engine.ext_panel_scroll_top
                                        + step)
                                        .min(flat_len.saturating_sub(1));
                                } else {
                                    engine.ext_panel_scroll_top =
                                        engine.ext_panel_scroll_top.saturating_sub(step);
                                }
                                return sidebar_width;
                            }
                            "tui:search_results" => {
                                // SidebarSystem handles scroll internally
                                return sidebar_width;
                            }
                            other if other.starts_with("debug_sidebar:") => {
                                // SidebarSystem handles scroll internally
                                return sidebar_width;
                            }
                            "terminal_scrollback" => {
                                // #533: single shared scroll entry point.
                                // delta.y < 0 = up (into history); > 0 = down
                                // (toward live).  Policy + forwarding live in
                                // Engine::handle_terminal_scroll.
                                engine.handle_terminal_scroll(delta.y);
                                return sidebar_width;
                            }
                            "tui:editor_viewport" => {
                                // #550: `find_window_at` compares against
                                // already-absolute `rw.rect`, so the raw
                                // event `col`/`row` are used directly.
                                let target = last_layout.and_then(|layout| {
                                    render::find_window_at(layout, col as f64, row as f64)
                                        .map(|idx| &layout.windows[idx])
                                });
                                if let Some(rw) = target {
                                    let dir = if down { 1 } else { -1 };
                                    engine.scroll_viewport_with_cursor_for_window(
                                        rw.window_id,
                                        dir,
                                        step,
                                    );
                                    engine.sync_scroll_binds();
                                }
                                return sidebar_width;
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Editor viewport scroll is now handled via dispatch_scroll
            // "tui:editor_viewport" surface above — fallback to active window
            // for scroll events that don't hit any registered surface.
            if col >= editor_left && row + 2 < term_height {
                let dir = if matches!(ev.kind, MouseEventKind::ScrollUp) {
                    -1
                } else {
                    1
                };
                engine.scroll_viewport_with_cursor(dir, 3);
                engine.sync_scroll_binds();
            }
            return sidebar_width;
        }
        _ => {}
    }

    // ── Right-click: open context menus ────────────────────────────────────────
    // #451: Alacritty + crossterm 0.28 only emit `Up(MouseButton::Right)` for a
    // right-click (no preceding `Down(Right)` event). Other terminals send both.
    // Matching `Up` instead of `Down` is also the standard ctx-menu trigger in
    // most GUI toolkits — menus open on release. Either-or makes both terminal
    // conventions work; the `close_context_menu` at the top of the handler
    // would re-close on the Up if both fired, but in practice every terminal
    // we've seen drops one or the other.
    if matches!(
        ev.kind,
        MouseEventKind::Down(MouseButton::Right) | MouseEventKind::Up(MouseButton::Right)
    ) {
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

        // Right-click on explorer sidebar → open explorer context menu.
        // #575: strictly gate on the Explorer actually being the active
        // panel. #451 relaxed this to `active_panel_is(PANEL_EXPLORER) ||
        // has_rows` on the theory that `explorer_rows` being non-empty meant
        // "the explorer is showing" — but `explorer_rows` is populated by
        // `explorer_rebuild_rows()` on workspace-open and a 2s auto-refresh
        // timer, and is never cleared on panel switch. So once a folder is
        // open, `has_rows` stays true forever, and this hijacked every
        // right-click in the sidebar column range for Debug/Search/Git —
        // wrong menu opened instead of no menu / a panel-appropriate one.
        // #451's actual root cause (confirmed via its issue history) was the
        // separate Up(Right)-only terminal quirk handled above, not this
        // gate — reverting the relaxation is safe. Clicks in the sidebar
        // still consume here (no-op) for non-Explorer panels, matching the
        // pre-#451 behavior and issue #575's "or no menu, if one isn't
        // implemented yet for that panel" expectation.
        if sb_visible && col >= ab_width && col < ab_width + sidebar_width {
            if engine.active_panel_is(PANEL_EXPLORER) {
                let sidebar_row = row.saturating_sub(menu_rows);
                let tree_row = sidebar_row as usize + engine.explorer_tree.borrow().scroll_offset();
                if tree_row < engine.explorer_rows.len() {
                    engine
                        .explorer_tree
                        .borrow_mut()
                        .set_selected_path(Some(vec![tree_row as u16]));
                    let path = engine.explorer_rows[tree_row].path.clone();
                    let is_dir = engine.explorer_rows[tree_row].is_dir;
                    engine.open_explorer_context_menu(path, is_dir, col, row);
                } else {
                    // Empty space below last entry → context menu for root folder
                    let root = engine.cwd.clone();
                    engine.open_explorer_context_menu(root, true, col, row);
                }
            }
            return sidebar_width;
        }

        // Right-click on tab bar → open tab context menu.
        //
        // #654: hit-test via the cached `hit_regions` (built once by
        // `render::compute_tab_bar_hit_regions` during
        // `build_screen_layout`) rather than rebuilding the primitive and
        // re-measuring every tab here. Left-click routing (below) and the
        // drag-slot map already read those regions, so all three now share
        // one geometry — the duplicate `name.chars().count() +
        // TAB_CLOSE_COLS` measurers this replaced had already drifted once
        // (#477) and were the last hand-rolled tab widths in the TUI.
        if col >= editor_left {
            let rel_col = col - editor_left;
            if let Some(layout) = last_layout {
                if let Some(ref split) = layout.editor_group_split {
                    let click_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
                    for gtb in split.group_tab_bars.iter() {
                        // #550: `gtb.bounds` is already absolute
                        // terminal-screen space, so no `menu_rows`/
                        // `editor_left` offset addition — compare directly
                        // against the raw event `col`/`row`.
                        let tab_bar_row = (gtb.bounds.y as u16).saturating_sub(click_tbh);
                        let gx = gtb.bounds.x as u16;
                        let gw = gtb.bounds.width as u16;
                        if row == tab_bar_row && col >= gx && col < gx + gw {
                            let local_col = col - gx;
                            if let Some(
                                TabBarClickTarget::Tab(i) | TabBarClickTarget::CloseTab(i),
                            ) = render::resolve_tab_bar_click(&gtb.hit_regions, local_col)
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
                        if let Some(TabBarClickTarget::Tab(i) | TabBarClickTarget::CloseTab(i)) =
                            render::resolve_tab_bar_click(&layout.tab_bar_hit_regions, rel_col)
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

    // ── Completion popup click intercept ──────────────────────────────────────────
    if engine.completion_idx.is_some() && ev.kind == MouseEventKind::Down(MouseButton::Left) {
        let hit = completion_layout
            .map(|cl| cl.hit_test(col as f32, row as f32))
            .unwrap_or(quadraui::CompletionsHit::Empty);
        if engine.handle_completion_click(hit) {
            return sidebar_width;
        }
    }

    // ── Context menu click intercept ────────────────────────────────────────────
    // #456: accept both `Down(Left)` and `Up(Left)`.  Some terminals (alacritty
    // via SGR mouse mode + tmux, and apparently gnome-terminal in certain
    // configurations) drop `Down(Left)` and only emit `Up(Left)` for clicks
    // — the same quirk #451 handled for right-clicks.  Without this the
    // menu item action never fires and the menu stays open until the user
    // clicks somewhere outside.
    //
    // Safety: when the terminal sends both `Down` + `Up`, the `Down` runs
    // first (action fires + menu closes via `context_menu_confirm` /
    // `close_context_menu`), then the `Up` sees `context_menu.is_none()`
    // and falls through — no double-fire.
    if engine.context_menu.is_some()
        && matches!(
            ev.kind,
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
        )
    {
        if let Some(cl) = context_menu_layout {
            let hit = cl.hit_test(col as f32, row as f32);
            match hit {
                quadraui::ContextMenuHit::Item(_) => {
                    if let Some(idx) = crate::core::engine::context_menu_hit_to_idx(&hit) {
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
                quadraui::ContextMenuHit::Inert => {
                    return sidebar_width;
                }
                quadraui::ContextMenuHit::Empty => {
                    // Check if click is on the 1-cell border around the
                    // inner layout bounds (TUI rasteriser draws border
                    // outside layout.bounds).
                    let b = &cl.bounds;
                    let on_border = col as f32 >= b.x - 1.0
                        && (col as f32) < b.x + b.width + 1.0
                        && row as f32 >= b.y - 1.0
                        && (row as f32) < b.y + b.height + 1.0;
                    if on_border {
                        return sidebar_width;
                    }
                    engine.close_context_menu();
                }
            }
        } else {
            engine.close_context_menu();
        }
    }

    // ── Context menu mouse hover ──────────────────────────────────────────────
    if engine.context_menu.is_some() && matches!(ev.kind, MouseEventKind::Moved) {
        if let Some(cl) = context_menu_layout {
            let hit = cl.hit_test(col as f32, row as f32);
            if let Some(idx) = crate::core::engine::context_menu_hit_to_idx(&hit) {
                if let Some(ref mut cm) = engine.context_menu {
                    cm.selected = idx;
                }
            }
        }
        return sidebar_width;
    }

    // Menu bar hover-to-switch is handled by MenuSystem::handle() in the
    // UiEvent intercept (mod.rs).

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
        if sb_visible
            && engine.active_panel_is(PANEL_GIT)
            && col >= ab_width
            && col < ab_width + sidebar_width
        {
            // Route via cached SidebarPanelLayout (#509) — no per-frame
            // arithmetic; hit_test uses absolute terminal coordinates.
            let hit = {
                let layout = engine.sc_panel_layout.borrow();
                layout.as_ref().map(|l| l.hit_test(col as f32, row as f32))
            };
            match hit {
                Some(quadraui::SidebarPanelHit::ToolbarButton(_))
                | Some(quadraui::SidebarPanelHit::ToolbarEmpty) => {
                    engine.sc_button_hovered = engine.sc_button_hit(col as f32, row as f32);
                    if !mouse_on_hover_popup {
                        engine.dismiss_panel_hover();
                    }
                }
                Some(quadraui::SidebarPanelHit::Content { y: content_y, .. }) => {
                    engine.sc_button_hovered = None;
                    // content_y is content-local (row 0 = first section row).
                    let content_row = content_y as usize;
                    if let Some((flat_idx, _is_header)) =
                        engine.sc_content_row_to_flat(content_row, true)
                    {
                        engine.panel_hover_mouse_move("source_control", "", flat_idx);
                    } else if !mouse_on_hover_popup {
                        engine.dismiss_panel_hover();
                    }
                }
                _ => {
                    engine.sc_button_hovered = None;
                    if !mouse_on_hover_popup {
                        engine.dismiss_panel_hover();
                    }
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
        if sb_visible
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
                        // #550: `gtb.bounds` is already absolute — compare
                        // against raw `col`, not `rel_col`/`menu_rows`.
                        let tab_bar_row = (gtb.bounds.y as u16).saturating_sub(click_tbh);
                        let gx = gtb.bounds.x as u16;
                        let gw = gtb.bounds.width as u16;
                        if row == tab_bar_row && col >= gx && col < gx + gw {
                            let local_col = col - gx;
                            tooltip = tab_tooltip_at_col(
                                engine,
                                gtb.group_id,
                                local_col,
                                &gtb.hit_regions,
                            );
                            break;
                        }
                    }
                } else if row == menu_rows && !engine.is_tab_bar_hidden(engine.active_group) {
                    tooltip = tab_tooltip_at_col(
                        engine,
                        engine.active_group,
                        rel_col,
                        &layout.tab_bar_hit_regions,
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
            // #550: `rw.rect` is already absolute — use raw `col`/`row`.
            let mut found = false;
            if let Some(idx) = render::find_window_at(layout, col as f64, row as f64) {
                let rw = &layout.windows[idx];
                let zone = render::window_zone_hit_test(
                    rw,
                    (col as f64) - rw.rect.x,
                    (row as f64) - rw.rect.y,
                    1.0,
                    1.0,
                );
                if let render::WindowZone::TextArea {
                    view_row, buf_line, ..
                } = zone
                {
                    // #560: shared quadraui text-layout inverse (see the
                    // drag handler above for the full rationale).
                    let (editor, editor_layout) = render::editor_text_layout(rw, 1.0, 1.0);
                    let text_col = editor_layout.col_at_x(&editor, view_row, col as f32);
                    engine.editor_hover_mouse_move(buf_line, text_col, mouse_on_editor_hover);
                    found = true;
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
    // DEBUG: trace every left-click
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
                    thumb_length: sb_hit.thumb.height,
                    max_scroll: sb_hit.total.saturating_sub(sb_hit.visible_rows),
                    grab_offset,
                    inverted: false,
                });
                apply_scrollbar_drag(
                    drag_state,
                    quadraui::Point { x: cx, y: cy },
                    engine,
                    sidebar,
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

    // Global status bar row — consume click (no interactive segments).
    if row + 2 == term_height && !engine.settings.window_status_line && col >= ab_width {
        return sidebar_width;
    }

    // Bottom row is cmd — ignore (but not in the activity bar column)
    if row + 1 >= term_height && col >= ab_width {
        return sidebar_width;
    }

    // ── Menu bar row click — command center only ──────────────────────────────
    // Menu bar item clicks and dropdown clicks are handled by
    // MenuSystem::handle() in the UiEvent intercept (mod.rs).
    // The command center (nav arrows + search box) is still separate.
    if engine.menu_bar_visible && row == 0 {
        let cc_hit = engine
            .command_center_layout
            .borrow()
            .as_ref()
            .map(|l| l.hit_test(col as f32, row as f32 + 0.5));
        match cc_hit {
            Some(quadraui::CommandCenterHit::Back) => {
                engine.tab_nav_back();
                return sidebar_width;
            }
            Some(quadraui::CommandCenterHit::Forward) => {
                engine.tab_nav_forward();
                return sidebar_width;
            }
            Some(quadraui::CommandCenterHit::SearchBox) => {
                engine.open_command_center();
                return sidebar_width;
            }
            _ => {}
        }
        // Click on menu bar area outside command center / menu items is
        // handled by MenuSystem (it closes the dropdown if open).
        return sidebar_width;
    }

    // ── Bottom panel tab bar click (shared row above Terminal / Debug Output) ──
    // Geometry is cached at paint time on engine.bottom_panel_geometry (#418).
    if col >= editor_left
        && matches!(
            engine.resolve_bottom_panel_zone(row as f64),
            Some(crate::core::engine::BottomPanelZone::TabBar)
        )
    {
        engine.handle_bottom_tab_bar_click(col as f64);
        return sidebar_width;
    }

    // ── Scroll-surface click dispatch (scrollbar thumb-drag + track-page). ──
    {
        let surfaces = engine.scroll_surfaces.borrow();
        let click_events = quadraui::dispatch_click(
            modal_stack,
            &surfaces,
            &[],
            drag_state,
            quadraui::Point {
                x: col as f32,
                y: row as f32,
            },
            quadraui::MouseButton::Left,
            Default::default(),
        );
        drop(surfaces);
        for cev in &click_events {
            match cev {
                quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset } => {
                    match widget.as_str() {
                        "debug_output" => {
                            engine.debug_output_scroll = *new_offset;
                            engine.debug_output_auto_scroll = false;
                            return sidebar_width;
                        }
                        "explorer:sb" => {
                            engine
                                .explorer_tree
                                .borrow_mut()
                                .set_scroll_offset(*new_offset);
                            return sidebar_width;
                        }
                        "ext_panel:sb" => {
                            engine.ext_panel_scroll_top = *new_offset;
                            return sidebar_width;
                        }
                        "tui:settings" => {
                            engine.settings_scroll_top = *new_offset;
                            return sidebar_width;
                        }
                        other if other.starts_with("debug_sidebar:") => {
                            // SidebarSystem handles scrollbar internally
                            return sidebar_width;
                        }
                        _ => {}
                    }
                }
                quadraui::UiEvent::MouseDown {
                    widget: Some(id), ..
                } if id.as_str() == "debug_output" => {
                    return sidebar_width;
                }
                quadraui::UiEvent::MouseDown {
                    widget: Some(id), ..
                } if matches!(id.as_str(), "explorer:sb" | "ext_panel:sb")
                    && drag_state.is_active() =>
                {
                    return sidebar_width;
                }
                quadraui::UiEvent::MouseDown {
                    widget: Some(id), ..
                } if id.as_str().starts_with("debug_sidebar:") && drag_state.is_active() => {
                    return sidebar_width;
                }
                _ => {}
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
                                    // Engine handles this internally now.
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
    // Zone resolved from cached geometry written at paint time (#418). Toolbar
    // and content rows live inside the bottom panel area; their absolute y
    // (e.g. for the scrollbar track) is recovered from the cached top_y.
    if engine.terminal_open && col >= editor_left {
        let zone = engine.resolve_bottom_panel_zone(row as f64);
        let geom = *engine.bottom_panel_geometry.borrow();
        if let (Some(zone), Some(geom)) = (zone, geom) {
            use crate::core::engine::BottomPanelZone;
            // Tab bar was already dispatched above; only Toolbar / Content land here.
            if matches!(zone, BottomPanelZone::Toolbar) {
                // Header row — dispatch through cached toolbar hit regions.
                engine.terminal_has_focus = true;
                let action = engine.resolve_terminal_toolbar_click(col as f64);
                let screen_h = terminal_size.map(|s| s.height).unwrap_or(24);
                let panel_cols = terminal_size
                    .map(|s| s.width)
                    .unwrap_or(80)
                    .saturating_sub(editor_left);
                let ctx = crate::core::engine::UiEventContext {
                    terminal_cols: panel_cols,
                    terminal_max_rows: super::terminal_target_maximize_rows_tui(engine, screen_h),
                };
                if !engine.execute_terminal_toolbar_action(action, ctx)
                    && matches!(
                        action,
                        crate::core::engine::TerminalToolbarAction::StartResize
                    )
                {
                    *dragging_terminal_resize = true;
                }
            } else if let BottomPanelZone::Content { row_offset } = zone {
                // Use cached TerminalSplitLayout for divider/pane/scrollbar
                // detection (#430). Non-split fallback uses row_offset directly.
                let split_layout = engine.terminal_split_layout.borrow();
                if let Some(ref sl) = *split_layout {
                    let abs_y = geom.top_y + geom.content_y + row_offset as f64;
                    let hit = sl.hit_test(col as f32, abs_y as f32);
                    drop(split_layout);
                    match hit {
                        quadraui::TerminalSplitHit::Scrollbar => {
                            let track_start = (geom.top_y + geom.content_y) as u16;
                            let track_len = (geom.height - geom.content_y).max(0.0) as u16;
                            let total = engine
                                .active_terminal()
                                .map(|t| t.history_len())
                                .unwrap_or(0);
                            let tl = track_len as f32;
                            drag_state.begin(quadraui::DragTarget::ScrollbarY {
                                widget: quadraui::WidgetId::new("terminal_scrollback"),
                                track_start: track_start as f32,
                                track_length: tl,
                                thumb_length: (tl / total.max(1) as f32).max(1.0),
                                max_scroll: total,
                                grab_offset: 0.0,
                                inverted: true,
                            });
                            apply_scrollbar_drag(
                                drag_state,
                                quadraui::Point {
                                    x: col as f32,
                                    y: row as f32,
                                },
                                engine,
                                sidebar,
                            );
                        }
                        _ => {
                            // #533: pass button/mods so split click can
                            // forward_mouse(Press) to the child when it
                            // has mouse reporting enabled.
                            if engine.handle_terminal_split_click(
                                hit,
                                quadraui::MouseButton::Left,
                                quadraui::Modifiers::default(),
                            ) {
                                *dragging_terminal_split = true;
                            }
                        }
                    }
                } else {
                    drop(split_layout);
                    // #429/#533: focus + scroll reset + selection / mouse
                    // forwarding are owned by the engine.  TUI still does
                    // the col conversion (panel is offset by the
                    // sidebar/activity-bar on the left).
                    let term_col = col.saturating_sub(editor_left);
                    engine.handle_terminal_pane_press(
                        term_col,
                        row_offset,
                        quadraui::MouseButton::Left,
                        quadraui::Modifiers::default(),
                    );
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
                if !engine.menu_bar_visible {
                    // Close the dropdown. MenuSystem::close() needs &mut Backend,
                    // but the mouse handler only has (drag_state, modal_stack).
                    // Pop the modal directly and reset the MenuSystem state by
                    // re-creating it with the same menu definitions.
                    modal_stack.pop(&quadraui::WidgetId::new("menu-system-dropdown"));
                    let menus = crate::render::build_menu_defs(engine.is_vscode_mode());
                    *engine.menu_system.borrow_mut() = quadraui::MenuSystem::new(menus);
                }
                return sidebar_width;
            }
            Some(ActivityBarTarget::ExtensionPanel(name)) => {
                if sidebar.ext_panel_name.as_deref() == Some(&name)
                    && engine.app_shell.sidebar_visible()
                {
                    engine.app_shell.hide_sidebar();
                    sidebar.ext_panel_name = None;
                    engine.ext_panel_has_focus = false;
                    engine.ext_panel_active = None;
                } else {
                    // #637: a plugin panel taking over the sidebar body
                    // must drop whatever panel's focus flag (and, for
                    // Extensions, `active_panel_id`-derived state like
                    // `ext_sidebar_has_focus`) was left set from before —
                    // `app_shell`'s active-panel id is deliberately left
                    // untouched here (this isn't a `toggle_sidebar_panel`
                    // switch), so nothing else clears it. A stale
                    // `ext_sidebar_has_focus = true` left over from a
                    // previous visit to the Extensions marketplace panel
                    // otherwise keeps `active_panel_is(PANEL_EXTENSIONS)`'s
                    // SidebarSystem intercept looking "focused" even though
                    // this plugin panel is what's actually on screen.
                    engine.clear_sidebar_focus();
                    sidebar.ext_panel_name = Some(name.clone());
                    if !engine.app_shell.sidebar_visible() {
                        engine.toggle_sidebar();
                    }
                    sidebar.has_focus = true;
                    engine.ext_panel_active = Some(name.clone());
                    engine.ext_panel_has_focus = true;
                    engine.ext_panel_selected = 0;
                    engine.plugin_event("panel_focus", &name);
                }
                engine.session.explorer_visible = engine.app_shell.sidebar_visible();
                let _ = engine.session.save();
                return sidebar_width;
            }
            _ => {}
        }
        let target_panel_id = match ab_target {
            Some(ActivityBarTarget::Panel(p)) => Some(match p {
                SidebarPanel::Explorer => PANEL_EXPLORER,
                SidebarPanel::Search => PANEL_SEARCH,
                SidebarPanel::Debug => PANEL_DEBUG,
                SidebarPanel::Git => PANEL_GIT,
                SidebarPanel::Extensions => PANEL_EXTENSIONS,
                SidebarPanel::Ai => PANEL_AI,
            }),
            Some(ActivityBarTarget::Settings) => Some(PANEL_SETTINGS),
            _ => None,
        };
        if let Some(panel_id) = target_panel_id {
            sidebar.ext_panel_name = None;
            engine.ext_panel_has_focus = false;
            engine.ext_panel_active = None;
            engine.toggle_sidebar_panel(panel_id);
            if engine.app_shell.sidebar_visible() {
                sidebar.has_focus = true;
            }
        }
        return sidebar_width;
    }

    // ── Sidebar panel area ────────────────────────────────────────────────────
    if engine.app_shell.sidebar_visible() && col < ab_width + sidebar_width {
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
            // #451: accept Up(Right) too (Alacritty/crossterm-0.28 only sends Up).
            if matches!(
                ev.kind,
                MouseEventKind::Down(MouseButton::Right) | MouseEventKind::Up(MouseButton::Right)
            ) {
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

            let flat_len = engine.ext_panel_flat_len();

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
                    } else {
                        // Single-click toggles sections/expandable items.
                        // Suppressed on double-click so the second Down doesn't
                        // un-toggle what the first one just toggled (#484).
                        engine.handle_ext_panel_key("Return", false, None);
                    }
                }
            }
        } else if engine.active_panel_is(PANEL_EXPLORER) {
            sidebar.has_focus = true;
            engine.explorer_has_focus = true;

            let tree_row = sidebar_row as usize + engine.explorer_tree.borrow().scroll_offset();
            if tree_row < engine.explorer_rows.len() {
                // Record potential drag source for DnD.
                *explorer_drag_src = Some(tree_row);
                engine
                    .explorer_tree
                    .borrow_mut()
                    .set_selected_path(Some(vec![tree_row as u16]));
                if engine.explorer_rows[tree_row].is_dir {
                    engine.explorer_toggle_dir(tree_row);
                } else {
                    let path = engine.explorer_rows[tree_row].path.clone();
                    engine.open_file_preview(&path);
                }
            }
        } else if engine.active_panel_is(PANEL_DEBUG) {
            sidebar.has_focus = true;
            engine.dap_sidebar_has_focus = true;

            if sidebar_row < 2 {
                // Chrome rows (title + action button).
                let guard = engine.dap_sidebar_action_hits.borrow();
                let matched = guard
                    .as_ref()
                    .map(|l| {
                        matches!(
                            l.hit_test(col as f32, 0.0),
                            quadraui::StatusBarHit::Segment(_)
                        )
                    })
                    .unwrap_or(false);
                drop(guard);
                if matched {
                    engine.handle_dap_sidebar_action_click();
                }
            } else {
                // Route body click through SidebarSystem.
                let rect = engine.dap_sidebar_body_rect.get();
                crate::render::populate_dap_sidebar_system(engine);
                let click_event = quadraui::UiEvent::MouseDown {
                    widget: None,
                    button: quadraui::MouseButton::Left,
                    position: quadraui::Point::new(col as f32, row as f32),
                    modifiers: quadraui::Modifiers::default(),
                };
                let mut tui_backend = super::backend::TuiBackend::default();
                let sidebar_event = engine.dap_sidebar_system.borrow_mut().handle(
                    &click_event,
                    &mut tui_backend,
                    rect,
                );
                engine.dispatch_dap_sidebar_event(sidebar_event);
            }
            return sidebar_width;
        } else if engine.active_panel_is(PANEL_GIT) {
            sidebar.has_focus = true;
            engine.sc_set_focus(true);

            // sidebar_row layout after #509 (option a, no padding):
            //   0               = header
            //   1 .. commit_end = commit input (quadraui::TextInput box,
            //                     including its 1-row border top+bottom — #480)
            //   commit_end      = toolbar slot (button row, SidebarPanel)
            //   commit_end+1 .. = sections (SidebarPanel content area)
            let commit_box_h = render::sc_commit_input_box_height(&engine.sc_commit_message);
            let commit_end = 1 + commit_box_h;

            if sidebar_row == 0 {
                engine.sc_commit_input_active = false;
            } else if sidebar_row >= 1 && sidebar_row < commit_end {
                engine.sc_commit_input_active = true;
                engine.sc_commit_cursor = engine.sc_commit_message.len();
            } else {
                // Route via cached SidebarPanelLayout (#509).
                engine.sc_commit_input_active = false;
                let hit = {
                    let layout = engine.sc_panel_layout.borrow();
                    layout.as_ref().map(|l| l.hit_test(col as f32, row as f32))
                };
                match hit {
                    Some(quadraui::SidebarPanelHit::ToolbarButton(_)) => {
                        if let Some(idx) = engine.sc_button_hit(col as f32, row as f32) {
                            engine.sc_activate_button(idx);
                        }
                    }
                    Some(quadraui::SidebarPanelHit::Content { .. }) => {
                        let click_ev = quadraui::UiEvent::MouseDown {
                            widget: None,
                            button: quadraui::MouseButton::Left,
                            position: quadraui::Point::new(col as f32, row as f32),
                            modifiers: quadraui::Modifiers::default(),
                        };
                        engine.handle_sc_sidebar_ui_event(click_ev);
                        let now = Instant::now();
                        let is_double = now.duration_since(*last_click_time)
                            < Duration::from_millis(400)
                            && *last_click_pos == (col, row);
                        *last_click_time = now;
                        *last_click_pos = (col, row);
                        if is_double {
                            let double_ev = quadraui::UiEvent::DoubleClick {
                                widget: None,
                                position: quadraui::Point::new(col as f32, row as f32),
                            };
                            engine.handle_sc_sidebar_ui_event(double_ev);
                        }
                    }
                    _ => {}
                }
            }
            return sidebar_width;
        } else if engine.active_panel_is(PANEL_SEARCH) {
            sidebar.has_focus = true;
            if !engine.search_has_focus {
                engine.search_set_focus(true);
            }
            let click_x = col as f32;
            let click_y = row as f32 + 0.5;
            let event = quadraui::UiEvent::MouseDown {
                widget: None,
                position: quadraui::Point::new(click_x, click_y),
                button: quadraui::MouseButton::Left,
                modifiers: quadraui::Modifiers::default(),
            };
            engine.handle_search_sidebar_ui_event(event);
            if !engine.search_has_focus {
                sidebar.has_focus = false;
            }
        } else if engine.active_panel_is(PANEL_EXTENSIONS) {
            sidebar.has_focus = true;
            engine.ext_sidebar_has_focus = true;
            if sidebar_row == 0 {
                // Panel header — no-op
            } else if sidebar_row == 1 {
                engine.ext_sidebar_input_active = true;
            }
            // Rows 2+ handled by SidebarSystem mouse intercept in main loop
        } else if engine.active_panel_is(PANEL_SETTINGS) {
            sidebar.has_focus = true;
            engine.settings_has_focus = true;
            let flat_total = engine.settings_flat_list().len();

            // Route scrollbar clicks through FormController.
            let sb_col = ab_width + sidebar_width - 1;
            if col == sb_col && sidebar_row >= 2 {
                let content_start = 2_u16;
                let content_height = term_height.saturating_sub(4);
                let q_rect = quadraui::Rect::new(
                    ab_width as f32,
                    content_start as f32,
                    sidebar_width as f32,
                    content_height as f32,
                );
                let click_ev = quadraui::UiEvent::MouseDown {
                    button: quadraui::MouseButton::Left,
                    position: quadraui::Point::new(col as f32, row as f32),
                    modifiers: Default::default(),
                    widget: None,
                };
                render::populate_settings_form_controller(engine);
                let result = engine
                    .settings_form_controller
                    .borrow_mut()
                    .handle_cached(&click_ev, q_rect);
                if !matches!(result, quadraui::FormControllerEvent::Ignored) {
                    engine.settings_scroll_top =
                        engine.settings_form_controller.borrow().scroll_offset();
                }
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
    engine.activity_bar_focused = false;
    engine.explorer_has_focus = false;
    engine.sc_set_focus(false);
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
            // #550: `layout.breadcrumbs[..].bounds` is already absolute.
            let bc_x = col as f64;
            let bc_y = row as f64;
            match render::resolve_breadcrumb_click(&layout.breadcrumbs, bc_x, bc_y, 1.0) {
                render::BreadcrumbClickResult::Hit(idx) => {
                    if !matches!(ev.kind, MouseEventKind::Down(MouseButton::Left)) {
                        return sidebar_width;
                    }
                    engine.handle_breadcrumb_click(idx);
                    return sidebar_width;
                }
                render::BreadcrumbClickResult::OnBar => {
                    return sidebar_width;
                }
                render::BreadcrumbClickResult::Miss => {}
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
                // #550: `gtb.bounds` is already absolute — no `menu_rows`/
                // `editor_left` offset addition, compare against raw `col`/`row`.
                let tab_bar_row = (gtb.bounds.y as u16).saturating_sub(click_tbh);
                let gx = gtb.bounds.x as u16;
                let gw = gtb.bounds.width as u16;
                if row == tab_bar_row && col >= gx && col < gx + gw {
                    let was_active = gtb.group_id == split.active_group;
                    matched_group = Some((
                        gtb.group_id,
                        col - gx,
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
                    match target {
                        TabBarClickTarget::Tab(_) => {
                            let needs_confirm = engine.handle_tab_bar_click(group_id, target);
                            if needs_confirm {
                                engine.show_close_tab_confirm();
                            }
                            *tab_drag_start = Some((col, row));
                        }
                        TabBarClickTarget::CloseTab(_) => {
                            let needs_confirm = engine.handle_tab_bar_click(group_id, target);
                            if needs_confirm {
                                engine.show_close_tab_confirm();
                            }
                        }
                        TabBarClickTarget::ActionMenu => {
                            engine.active_group = group_id;
                            // #434: pass tab-row height (1.0 row in TUI) so the
                            // engine drives Below placement; replaces the prior
                            // `row + 1` hack.
                            engine.open_editor_action_menu(group_id, col, row, 1.0);
                        }
                        _ => {
                            engine.handle_tab_bar_click(group_id, target);
                        }
                    }
                    return sidebar_width;
                }
                // #452: matched a group's tab-bar row but no actual tab/button.
                // For horizontal splits, the second group's tab bar IS the visual
                // divider — fall through to the group-divider hit-test below so
                // clicks on empty tab-bar space can start a resize drag.
            }
        }
        // Single group: check top tab bar row only.
        if row == menu_rows
            && layout.editor_group_split.is_none()
            && !engine.is_tab_bar_hidden(engine.active_group)
        {
            // #654: the last hand-rolled tab geometry in the TUI — this used
            // to rebuild the `TabBar` primitive and re-measure every tab with
            // `name.chars().count() + TAB_CLOSE_COLS` before calling
            // `hit_test`. The split-group branch above, the tooltip lookup and
            // the drag-slot map all already read `hit_regions`, so this now
            // does too: one geometry, computed once in `build_screen_layout`.
            let local_col = rel_col;
            match render::resolve_tab_bar_click(&layout.tab_bar_hit_regions, local_col) {
                Some(TabBarClickTarget::Tab(i)) => {
                    if i < engine.active_group().tabs.len() {
                        engine.goto_tab(i);
                        *tab_drag_start = Some((col, row));
                    }
                }
                Some(TabBarClickTarget::CloseTab(i)) => {
                    if i < engine.active_group().tabs.len() {
                        engine.active_group_mut().active_tab = i;
                        engine.line_annotations.clear();
                        if engine.dirty() {
                            engine.show_close_tab_confirm();
                        } else {
                            engine.close_tab();
                        }
                    }
                }
                Some(target) => {
                    let has_win = engine.windows.contains_key(&engine.active_window_id());
                    match target {
                        TabBarClickTarget::DiffPrev => {
                            if has_win {
                                engine.jump_prev_hunk();
                            }
                        }
                        TabBarClickTarget::DiffNext => {
                            if has_win {
                                engine.jump_next_hunk();
                            }
                        }
                        TabBarClickTarget::DiffToggle => {
                            engine.diff_toggle_hide_unchanged();
                        }
                        TabBarClickTarget::SplitRight => {
                            engine.open_editor_group(SplitDirection::Vertical);
                        }
                        TabBarClickTarget::SplitDown => {
                            engine.open_editor_group(SplitDirection::Horizontal);
                        }
                        TabBarClickTarget::ActionMenu => {
                            // #434: pass tab-row height (1.0 row in TUI).
                            engine.open_editor_action_menu(engine.active_group, col, row, 1.0);
                        }
                        TabBarClickTarget::Tab(_) | TabBarClickTarget::CloseTab(_) => {}
                    }
                }
                None => {}
            }
            return sidebar_width;
        }
    }

    // #550: `rw.rect`/`div.position`/`.cross_start` etc. below are all
    // already absolute terminal-screen coordinates, so the raw event
    // `col`/`row` are used directly throughout this block — no
    // editor-area-relative translation needed.

    // ── Group divider click — start drag ──────────────────────────────────────
    // #452: must use the same float-to-int conversion as the divider
    // renderer (`render_impl.rs::draw_frame` truncates with `as u16`) —
    // `render::divider_hit_test`'s `quantize: true` handles this. Using
    // `.round()` here meant clicks on a divider at e.g. col 40.5 (rendered at
    // 40) were hit-tested at 41 and missed.
    //
    // For horizontal splits the "visual divider" is the second group's
    // entire tab-bar block (per render_impl.rs:359). When breadcrumbs are
    // on the block is 2 rows tall — accept a click on either row (the
    // asymmetric `(0.0, tab_bar_rows)` horizontal tolerance below).
    if let Some(layout) = last_layout {
        if let Some(ref split) = layout.editor_group_split {
            let tab_bar_rows: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
            if let Some(i) = render::divider_hit_test(
                &split.dividers,
                col as f64,
                row as f64,
                (0.0, 1.0),
                (0.0, tab_bar_rows as f64),
                true,
            ) {
                *dragging_group_divider = Some(split.dividers[i].split_index);
                return sidebar_width;
            }
        }
    }

    // ── Window-split divider click — start drag (#582) ─────────────────────────
    // `:split`/`:vsplit` panes within a single editor group. `div.position` is
    // the boundary column/row itself (first column/row of the *second* pane),
    // but the only thing actually drawn there is one cell *before* it: for
    // `:vsplit`, `render_separators` paints the neighbouring window's own
    // separator/scrollbar column at `sep_x - 1`; for `:split`, the upper
    // window's per-window status line (its own reserved bottom row) occupies
    // `sep_y - 1` and there's no separate glyph at all. A `(0.0, 1.0)`
    // tolerance only accepted `col`/`row == position`, one cell past
    // whatever the user can actually see and click — 100% miss for
    // `:split` (no glyph ever sat at `position`) and a coin-flip for
    // `:vsplit` (only imprecise aim ever landed exactly on `position`
    // instead of the visible glyph at `position - 1`). Extend `tol_before`
    // to 1 so both the visible mark and the boundary cell register (#582
    // smoke-test failure: TUI :split divider unclickable, :vsplit flaky).
    if let Some(layout) = last_layout {
        if let Some(i) = render::divider_hit_test(
            &layout.window_dividers,
            col as f64,
            row as f64,
            (1.0, 1.0),
            (1.0, 1.0),
            true,
        ) {
            let div = &layout.window_dividers[i];
            *dragging_window_divider = Some((div.group_id, div.split_index));
            return sidebar_width;
        }
    }

    if let Some(layout) = last_layout {
        for rw in &layout.windows {
            let wx = rw.rect.x as u16;
            let wy = rw.rect.y as u16;
            let ww = rw.rect.width as u16;
            let wh = rw.rect.height as u16;

            if col >= wx && col < wx + ww && row >= wy && row < wy + wh {
                // Per-window status bar click — hit-test segments for actions.
                if rw.status_line.is_some() && wh > 1 && row == wy + wh - 1 {
                    if let Some(ref status) = rw.status_line {
                        let click_col = (col - wx) as usize;
                        if let Some(action) =
                            status_segment_hit_test(status, ww as usize, click_col)
                        {
                            if let Some(ea) = engine.handle_status_action(&action) {
                                use crate::core::engine::EngineAction;
                                match ea {
                                    EngineAction::ToggleSidebar => {
                                        // Engine handles this internally now.
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
                if has_v_scrollbar && col == wx + ww - 1 {
                    // #550: `wy` is already absolute (includes both the menu
                    // bar offset and tab_bar_height), so no `menu_rows +`.
                    let track_abs_start = wy;
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
                    let tl = track_len as f32;
                    drag_state.begin(quadraui::DragTarget::ScrollbarY {
                        widget: quadraui::WidgetId::new(format!(
                            "tui:editor:{}:vsb",
                            rw.window_id.0
                        )),
                        track_start: track_abs_start as f32,
                        track_length: tl,
                        thumb_length: (tl * track_visible as f32 / rw.total_lines.max(1) as f32)
                            .max(1.0),
                        max_scroll: rw.total_lines.saturating_sub(track_visible),
                        grab_offset,
                        inverted: false,
                    });
                    apply_scrollbar_drag(
                        drag_state,
                        quadraui::Point {
                            x: col as f32,
                            y: row as f32,
                        },
                        engine,
                        sidebar,
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
                if has_h_scrollbar && row == h_sb_row {
                    let track_x = wx + gutter;
                    let track_w = ww.saturating_sub(gutter + if has_v_scrollbar { 1 } else { 0 });
                    if col >= track_x && col < track_x + track_w && track_w > 0 {
                        // #550: `track_x` (derived from `wx`) is already absolute.
                        let track_abs_start = track_x;
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
                        let tl = track_w as f32;
                        drag_state.begin(quadraui::DragTarget::ScrollbarX {
                            widget: quadraui::WidgetId::new(format!(
                                "tui:editor:{}:hsb",
                                rw.window_id.0
                            )),
                            track_start: track_abs_start as f32,
                            track_length: tl,
                            thumb_length: (tl * track_visible as f32 / rw.max_col.max(1) as f32)
                                .max(1.0),
                            max_scroll: rw.max_col.saturating_sub(track_visible),
                            grab_offset,
                            inverted: false,
                        });
                        apply_scrollbar_drag(
                            drag_state,
                            quadraui::Point {
                                x: col as f32,
                                y: row as f32,
                            },
                            engine,
                            sidebar,
                        );
                        return sidebar_width;
                    }
                }

                // Check gutter area — shared resolution via render::resolve_gutter_action (#344).
                let view_row = (row - wy) as usize;
                if gutter > 0 && col >= wx && col < wx + gutter {
                    if let Some(rl) = rw.lines.get(view_row) {
                        let gutter_col = (col - wx) as usize;
                        use crate::render::GutterAction;
                        match crate::render::resolve_gutter_action(rw, rl.line_idx, gutter_col) {
                            Some(GutterAction::ToggleBreakpoint(line)) => {
                                let file = engine
                                    .windows
                                    .get(&rw.window_id)
                                    .and_then(|w| engine.buffer_manager.get(w.buffer_id))
                                    .and_then(|bs| bs.file_path.as_ref())
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                engine.dap_toggle_breakpoint(&file, line as u64 + 1);
                            }
                            Some(GutterAction::DiffPeek(line)) => {
                                engine.active_tab_mut().active_window = rw.window_id;
                                engine.view_mut().cursor.line = line;
                                engine.open_diff_peek();
                            }
                            Some(GutterAction::DiagnosticHover(line)) => {
                                engine.active_tab_mut().active_window = rw.window_id;
                                engine.view_mut().cursor.line = line;
                                engine.trigger_editor_hover_for_line(line);
                            }
                            Some(GutterAction::CodeAction(line)) => {
                                engine.active_tab_mut().active_window = rw.window_id;
                                engine.view_mut().cursor.line = line;
                                engine.show_code_actions_popup();
                            }
                            Some(GutterAction::ToggleFold(line)) => {
                                let has_fold_indicator =
                                    rl.gutter_text.chars().any(|c| c == '+' || c == '-');
                                if has_fold_indicator {
                                    engine.toggle_fold_at_line(line);
                                }
                            }
                            None => {}
                        }
                    }
                    return sidebar_width;
                }
                // Text area click — fold/wrap-aware row → buffer line mapping
                let clicked_rl = rw.lines.get(view_row);
                let buf_line = clicked_rl
                    .map(|l| l.line_idx)
                    .unwrap_or_else(|| rw.scroll_top + view_row);
                // #560: resolve the column via the shared quadraui
                // text-layout inverse (`EditorLayout::col_at_x`, which
                // folds in `scroll_left` and wrap `segment_col_offset`
                // itself) instead of hand-rolled cell math — the same
                // function GTK's `Backend::editor_col_at_x` falls back to,
                // so both backends' click math derives from one source.
                let (editor, editor_layout) = crate::render::editor_text_layout(rw, 1.0, 1.0);
                let col_in_text = editor_layout.col_at_x(&editor, view_row, col as f32);

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
                    // #565: arm the drag-origin so a following drag (extend
                    // word-wise selection) is arbitrated the same way a
                    // plain click-drag is — see the single-click branch below.
                    drag_state.begin(quadraui::DragTarget::TextSelection {
                        region: text_drag_widget_id(rw.window_id),
                        anchor: quadraui::Point {
                            x: col as f32,
                            y: row as f32,
                        },
                    });
                } else {
                    // Clear selection on click in VSCode mode.
                    if engine.is_vscode_mode() {
                        engine.vscode_clear_selection();
                    }
                    engine.mouse_click(rw.window_id, buf_line, col_in_text);
                    // #565: arm a DragTarget::TextSelection so a following
                    // Drag event can recover which window this gesture
                    // belongs to (see the drag-origin arbitration in the
                    // `MouseEventKind::Drag(Left)` handler above), replacing
                    // the old bespoke `mouse_text_drag` bool.
                    drag_state.begin(quadraui::DragTarget::TextSelection {
                        region: text_drag_widget_id(rw.window_id),
                        anchor: quadraui::Point {
                            x: col as f32,
                            y: row as f32,
                        },
                    });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_drag_widget_id_round_trips_through_origin_window() {
        let wid = crate::core::WindowId(7);
        let region = text_drag_widget_id(wid);
        assert_eq!(region.as_str(), "tui:editor:7:text");
        assert_eq!(text_drag_origin_window(&region), Some(wid));
    }

    #[test]
    fn text_drag_widget_id_distinguishes_windows() {
        let a = text_drag_widget_id(crate::core::WindowId(0));
        let b = text_drag_widget_id(crate::core::WindowId(1));
        assert_ne!(a, b);
        assert_eq!(text_drag_origin_window(&a), Some(crate::core::WindowId(0)));
        assert_eq!(text_drag_origin_window(&b), Some(crate::core::WindowId(1)));
    }

    #[test]
    fn text_drag_origin_window_rejects_unrelated_widget_ids() {
        // Scrollbar ids use the same `tui:editor:` prefix but a different
        // suffix — must not be misparsed as a text-drag origin.
        let scrollbar = quadraui::WidgetId::new("tui:editor:3:vsb");
        assert_eq!(text_drag_origin_window(&scrollbar), None);

        let unrelated = quadraui::WidgetId::new("explorer:sb");
        assert_eq!(text_drag_origin_window(&unrelated), None);

        let garbage = quadraui::WidgetId::new("tui:editor::text");
        assert_eq!(text_drag_origin_window(&garbage), None);
    }

    // ── #575: right-click sidebar panel routing ────────────────────────────

    /// Dispatch a single right-click `MouseEvent` at `(col, row)` through
    /// `handle_mouse` with every non-relevant parameter at its idle default.
    /// Mirrors the call site in `event_loop` (mod.rs) but with no drag/hover/
    /// popup state in flight, since these tests only exercise the right-click
    /// sidebar-panel gate.
    fn dispatch_right_click(engine: &mut Engine, col: u16, row: u16) {
        let ev = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        };
        let mut sidebar = TuiSidebar::new();
        let mut drag_state = quadraui::DragState::default();
        let mut modal_stack = quadraui::ModalStack::new();
        let mut last_click_time = Instant::now();
        let mut last_click_pos: (u16, u16) = (0, 0);
        let mut should_quit = false;

        handle_mouse(
            ev,
            &mut sidebar,
            engine,
            &Some(Size {
                width: 120,
                height: 40,
            }),
            SIDEBAR_WIDTH,
            &mut false,
            &mut false,
            &mut false,
            &mut None,
            &mut None,
            &mut drag_state,
            &mut modal_stack,
            None,
            &mut last_click_time,
            &mut last_click_pos,
            &mut None,
            &mut None,
            &mut false,
            &mut should_quit,
            &mut None,
            &mut None,
            &mut None,
            &mut false,
            &mut None,
            &mut None,
            &mut crate::core::window::DropZone::None,
            &[],
            None,
            None,
            &[],
            None,
            &mut false,
            &mut false,
            None,
            None,
            None,
        );
    }

    /// #575 Bug 1: right-clicking in the Debug sidebar must not open the
    /// Explorer's file context menu. `explorer_rows` is never cleared when
    /// the active panel switches away from Explorer (it's only refreshed on
    /// workspace-open and a periodic timer), so a right-click gate that
    /// falls back to "rows present" instead of strictly checking the active
    /// panel hijacks every sidebar right-click for whichever panel is
    /// showing. Regression test for the `explorer_active || has_rows` gate
    /// reverted in this fix.
    #[test]
    fn right_click_in_debug_panel_does_not_open_explorer_context_menu() {
        let mut engine = Engine::new();
        engine.focus_sidebar_panel(PANEL_DEBUG);
        engine.explorer_rows.push(crate::core::engine::ExplorerRow {
            depth: 0,
            name: "foo.txt".into(),
            path: std::path::PathBuf::from("/tmp/foo.txt"),
            is_dir: false,
            is_expanded: false,
        });
        assert!(engine.active_panel_is(PANEL_DEBUG));

        dispatch_right_click(&mut engine, ACTIVITY_BAR_WIDTH + 1, 0);

        assert!(
            engine.context_menu.is_none(),
            "right-click in the Debug panel must not open the Explorer context menu"
        );
    }

    /// Sanity counterpart: right-clicking a populated Explorer sidebar still
    /// opens the Explorer context menu (the gate must not become a no-op
    /// entirely — only non-Explorer panels should be excluded).
    #[test]
    fn right_click_in_explorer_panel_opens_explorer_context_menu() {
        let mut engine = Engine::new();
        engine.focus_sidebar_panel(PANEL_EXPLORER);
        engine.explorer_rows.push(crate::core::engine::ExplorerRow {
            depth: 0,
            name: "foo.txt".into(),
            path: std::path::PathBuf::from("/tmp/foo.txt"),
            is_dir: false,
            is_expanded: false,
        });
        assert!(engine.active_panel_is(PANEL_EXPLORER));

        dispatch_right_click(&mut engine, ACTIVITY_BAR_WIDTH + 1, 0);

        assert!(
            engine.context_menu.is_some(),
            "right-click in the Explorer panel must still open its context menu"
        );
    }

    // ── #575 Bug 2: File-menu dropdown dismiss on outside click ────────────

    /// Regression test for quadraui#429: an outside `MouseUp(Left)` must
    /// close an open dropdown even when `MouseDown(Left)` was never
    /// delivered — the Alacritty+tmux / some gnome-terminal quirk that
    /// drops `Down(Left)` and only delivers `Up(Left)` for a click (same
    /// class of terminal quirk as the documented `Down(Right)` drop this
    /// file's #451 comments cover). Before quadraui#429, `MenuSystem::handle`
    /// had no `MouseUp` arm at all, so the dropdown never dismissed on those
    /// terminals.
    ///
    /// Exercises the exact wiring vimcode's TUI event loop uses (mod.rs's
    /// "MenuSystem intercept" block, ~line 1293-1300): `render::build_menu_defs`
    /// feeds a real `quadraui::MenuSystem`, driven through a real `TuiBackend`.
    #[test]
    fn file_menu_dropdown_closes_on_outside_mouse_up() {
        use quadraui::Backend as _;

        let menus = crate::render::build_menu_defs(false);
        let mut menu_system = quadraui::MenuSystem::new(menus);
        let mut backend = super::backend::TuiBackend::default();
        let bar_rect = quadraui::Rect::new(0.0, 0.0, 120.0, 1.0);

        // Open the first bar menu ("File") via MouseDown, as the normal
        // click-to-open path would.
        let bar = menu_system.menu_bar();
        let bar_layout = backend.menu_bar_layout(bar_rect, &bar);
        let file_item = bar_layout
            .visible_items
            .first()
            .expect("menu bar must have at least one item");
        let open_x = file_item.bounds.x + 1.0;

        let open_event = quadraui::UiEvent::MouseDown {
            widget: None,
            button: quadraui::MouseButton::Left,
            position: quadraui::Point::new(open_x, 0.0),
            modifiers: quadraui::Modifiers::default(),
        };
        let opened = menu_system.handle(&open_event, &mut backend, bar_rect);
        assert_eq!(opened, quadraui::MenuEvent::StateChanged);
        assert!(
            menu_system.is_open(),
            "File menu must be open after MouseDown"
        );

        // Simulate a terminal that drops MouseDown(Left) and only delivers
        // MouseUp(Left), landing well outside the bar and the open dropdown.
        let outside_event = quadraui::UiEvent::MouseUp {
            widget: None,
            button: quadraui::MouseButton::Left,
            position: quadraui::Point::new(100.0, 30.0),
        };
        let closed = menu_system.handle(&outside_event, &mut backend, bar_rect);
        assert_eq!(closed, quadraui::MenuEvent::StateChanged);
        assert!(
            !menu_system.is_open(),
            "outside MouseUp must close the open File menu dropdown"
        );
    }

    /// Sanity counterpart: an outside `MouseUp(Left)` with no menu open must
    /// be a no-op — it must not spuriously report `StateChanged` on every
    /// idle click.
    #[test]
    fn outside_mouse_up_with_no_menu_open_is_ignored() {
        let menus = crate::render::build_menu_defs(false);
        let mut menu_system = quadraui::MenuSystem::new(menus);
        let mut backend = super::backend::TuiBackend::default();
        let bar_rect = quadraui::Rect::new(0.0, 0.0, 120.0, 1.0);

        assert!(!menu_system.is_open());

        let outside_event = quadraui::UiEvent::MouseUp {
            widget: None,
            button: quadraui::MouseButton::Left,
            position: quadraui::Point::new(100.0, 30.0),
        };
        let result = menu_system.handle(&outside_event, &mut backend, bar_rect);
        assert_eq!(result, quadraui::MenuEvent::Ignored);
    }

    // ── #550: absolute window-rect coordinate convention ────────────────────
    //
    // These tests build a *real* `ScreenLayout` via `build_screen_for_tui`
    // (the same production function `draw_frame` uses to paint) with the
    // sidebar and menu bar both visible, so `window_rects` carry a non-zero
    // origin. They then dispatch a click through `handle_mouse` at a
    // coordinate read straight off that `ScreenLayout` (not re-derived by
    // hand) and assert the click resolves correctly. Before #550, TUI's
    // window rects were content-area-relative and every click site
    // re-added `editor_left`/`menu_rows` on top of them — a bug reintroduced
    // in either the paint or the click math would show up here as a
    // click/paint coordinate mismatch (wrong group hit, or no hit at all).

    /// Build a hermetic engine with a vertical group split, sidebar visible,
    /// and the menu bar visible — the scenario with the largest non-zero
    /// `(x, y)` editor-area origin, to maximize the chance of catching an
    /// offset regression.
    fn split_engine_with_sidebar_and_menu() -> Engine {
        let mut e = Engine::new();
        e.settings = crate::core::settings::Settings::default();
        e.mode = crate::core::Mode::Normal;
        e.menu_bar_visible = true;
        if !e.app_shell.sidebar_visible() {
            e.toggle_sidebar();
        }
        e.open_editor_group(crate::core::window::SplitDirection::Vertical);
        e
    }

    fn dispatch_left_click(
        engine: &mut Engine,
        col: u16,
        row: u16,
        last_layout: Option<&render::ScreenLayout>,
        dragging_group_divider: &mut Option<usize>,
        dragging_window_divider: &mut Option<(crate::core::window::GroupId, usize)>,
    ) {
        let ev = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        };
        let mut sidebar = TuiSidebar::new();
        let mut drag_state = quadraui::DragState::default();
        let mut modal_stack = quadraui::ModalStack::new();
        let mut last_click_time = Instant::now();
        let mut last_click_pos: (u16, u16) = (0, 0);
        let mut should_quit = false;

        handle_mouse(
            ev,
            &mut sidebar,
            engine,
            &Some(Size {
                width: 120,
                height: 40,
            }),
            SIDEBAR_WIDTH,
            &mut false,
            &mut false,
            &mut false,
            dragging_group_divider,
            dragging_window_divider,
            &mut drag_state,
            &mut modal_stack,
            last_layout,
            &mut last_click_time,
            &mut last_click_pos,
            &mut None,
            &mut None,
            &mut false,
            &mut should_quit,
            &mut None,
            &mut None,
            &mut None,
            &mut false,
            &mut None,
            &mut None,
            &mut crate::core::window::DropZone::None,
            &[],
            None,
            None,
            &[],
            None,
            &mut false,
            &mut false,
            None,
            None,
            None,
        );
    }

    /// A click exactly on a group divider (per the freshly-painted
    /// `ScreenLayout`) must start the divider drag, and a click one column
    /// off it must not. With the sidebar (30 cols) + activity bar (3 cols) +
    /// menu bar (1 row) all visible, the divider's absolute column sits well
    /// past both the old content-relative value AND a "double-counted
    /// offset" value would — pinning it via the real painted position (not a
    /// hand-derived formula) means either regression breaks this test.
    #[test]
    fn group_divider_click_matches_painted_divider_position() {
        let engine = split_engine_with_sidebar_and_menu();
        let theme = crate::render::Theme::onedark();
        let sidebar = TuiSidebar::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };
        let screen = super::render_impl::build_screen_for_tui(
            &engine,
            &theme,
            area,
            &sidebar,
            SIDEBAR_WIDTH,
        );
        let split = screen
            .editor_group_split
            .as_ref()
            .expect("vertical split must produce Some(editor_group_split)");
        let div = split
            .dividers
            .first()
            .expect("a vertical split has exactly one divider");
        assert_eq!(div.direction, crate::core::window::SplitDirection::Vertical);

        // Sanity: this is genuinely testing a non-trivial offset, not a
        // degenerate zero-origin case.
        let editor_left = ACTIVITY_BAR_WIDTH + SIDEBAR_WIDTH + 1;
        assert!(
            (div.position as u16) > editor_left,
            "divider column {} should sit inside the editor area (left edge {editor_left})",
            div.position
        );

        let col = div.position as u16;
        let row = (div.cross_start + 1.0) as u16;

        let mut engine_hit = split_engine_with_sidebar_and_menu();
        let mut dragging = None;
        let mut dragging_window = None;
        dispatch_left_click(
            &mut engine_hit,
            col,
            row,
            Some(&screen),
            &mut dragging,
            &mut dragging_window,
        );
        assert_eq!(
            dragging,
            Some(div.split_index),
            "click at the painted divider column ({col}, {row}) must start the divider drag"
        );

        // One column off must NOT hit the divider.
        let mut engine_miss = split_engine_with_sidebar_and_menu();
        let mut dragging_miss = None;
        let mut dragging_window_miss = None;
        dispatch_left_click(
            &mut engine_miss,
            col - 1,
            row,
            Some(&screen),
            &mut dragging_miss,
            &mut dragging_window_miss,
        );
        assert_eq!(
            dragging_miss, None,
            "click one column off the divider must not start a drag"
        );
    }

    fn vsplit_engine_with_sidebar_and_menu() -> Engine {
        let mut e = Engine::new();
        e.settings = crate::core::settings::Settings::default();
        e.mode = crate::core::Mode::Normal;
        e.menu_bar_visible = true;
        if !e.app_shell.sidebar_visible() {
            e.toggle_sidebar();
        }
        // `:vsplit` — a vim window split *within* the single default editor
        // group, distinct from `open_editor_group` above (#582).
        e.split_window(crate::core::window::SplitDirection::Vertical, None);
        e
    }

    /// #582: clicking a `:vsplit` window-divider at its painted position must
    /// start the window-divider drag, and dragging must resize the panes —
    /// this is the actual bug (window splits had no divider hit-test/drag at
    /// all; only editor-group splits, a separate feature, worked). Mirrors
    /// `group_divider_click_matches_painted_divider_position` above.
    #[test]
    fn window_divider_click_starts_drag_and_resizes() {
        let engine = vsplit_engine_with_sidebar_and_menu();
        let theme = crate::render::Theme::onedark();
        let sidebar = TuiSidebar::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };
        let screen = super::render_impl::build_screen_for_tui(
            &engine,
            &theme,
            area,
            &sidebar,
            SIDEBAR_WIDTH,
        );
        let div = screen
            .window_dividers
            .first()
            .expect("a :vsplit must produce one window divider");
        assert_eq!(div.direction, crate::core::window::SplitDirection::Vertical);

        let editor_left = ACTIVITY_BAR_WIDTH + SIDEBAR_WIDTH + 1;
        assert!(
            (div.position as u16) > editor_left,
            "divider column {} should sit inside the editor area (left edge {editor_left})",
            div.position
        );

        let col = div.position as u16;
        let row = (div.cross_start + 1.0) as u16;
        let group_id = div.group_id;
        let split_index = div.split_index;

        // Click on the painted divider column starts the drag.
        let mut engine_hit = vsplit_engine_with_sidebar_and_menu();
        let mut dragging_group = None;
        let mut dragging_window = None;
        dispatch_left_click(
            &mut engine_hit,
            col,
            row,
            Some(&screen),
            &mut dragging_group,
            &mut dragging_window,
        );
        assert_eq!(
            dragging_window,
            Some((group_id, split_index)),
            "click at the painted window-divider column ({col}, {row}) must start the drag"
        );

        // `col - 1` is where `render_separators` actually paints the visible
        // mark (the left window's own separator/scrollbar column, one cell
        // before the boundary at `col`) — the hit-test tolerance deliberately
        // covers it too (#582 smoke-test fix: clicking the glyph the user can
        // actually see used to miss ~half the time). Two columns off must
        // still miss.
        let mut engine_hit_neighbor = vsplit_engine_with_sidebar_and_menu();
        let mut dragging_group_neighbor = None;
        let mut dragging_window_neighbor = None;
        dispatch_left_click(
            &mut engine_hit_neighbor,
            col - 1,
            row,
            Some(&screen),
            &mut dragging_group_neighbor,
            &mut dragging_window_neighbor,
        );
        assert_eq!(
            dragging_window_neighbor,
            Some((group_id, split_index)),
            "click on the visible glyph column ({}, {row}), one before the boundary, must also start the drag",
            col - 1
        );

        // Two columns off must NOT hit the divider.
        let mut engine_miss = vsplit_engine_with_sidebar_and_menu();
        let mut dragging_group_miss = None;
        let mut dragging_window_miss = None;
        dispatch_left_click(
            &mut engine_miss,
            col - 2,
            row,
            Some(&screen),
            &mut dragging_group_miss,
            &mut dragging_window_miss,
        );
        assert_eq!(
            dragging_window_miss, None,
            "click two columns off the window divider must not start a drag"
        );

        // Dragging right must move the divider and grow the first pane
        // (`WindowLayout::set_ratio_at_index`, previously a dead no-op — #582).
        let move_ev = MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: col + 10,
            row,
            modifiers: KeyModifiers::NONE,
        };
        let mut sidebar_state = TuiSidebar::new();
        let mut drag_state = quadraui::DragState::default();
        let mut modal_stack = quadraui::ModalStack::new();
        let mut last_click_time = Instant::now();
        let mut last_click_pos: (u16, u16) = (0, 0);
        let mut should_quit = false;
        handle_mouse(
            move_ev,
            &mut sidebar_state,
            &mut engine_hit,
            &Some(Size {
                width: 120,
                height: 40,
            }),
            SIDEBAR_WIDTH,
            &mut false,
            &mut false,
            &mut false,
            &mut dragging_group,
            &mut dragging_window,
            &mut drag_state,
            &mut modal_stack,
            Some(&screen),
            &mut last_click_time,
            &mut last_click_pos,
            &mut None,
            &mut None,
            &mut false,
            &mut should_quit,
            &mut None,
            &mut None,
            &mut None,
            &mut false,
            &mut None,
            &mut None,
            &mut crate::core::window::DropZone::None,
            &[],
            None,
            None,
            &[],
            None,
            &mut false,
            &mut false,
            None,
            None,
            None,
        );

        // Recompute the group's own window rects the same way
        // `Engine::calculate_window_dividers` does, then re-derive dividers
        // from that bounding box to confirm the ratio actually moved.
        let group = engine_hit
            .editor_groups
            .get(&group_id)
            .expect("group still exists");
        let ids = group.active_tab().layout.window_ids();
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        for w in &screen.windows {
            if ids.contains(&w.window_id) {
                min_x = min_x.min(w.rect.x);
                min_y = min_y.min(w.rect.y);
                max_x = max_x.max(w.rect.x + w.rect.width);
                max_y = max_y.max(w.rect.y + w.rect.height);
            }
        }
        let bounds = WindowRect::new(min_x, min_y, max_x - min_x, max_y - min_y);
        let redivided = group.active_tab().layout.dividers(bounds, &mut 0);
        assert!(
            redivided[0].position > div.position,
            "dragging right must move the window divider right (was {}, now {})",
            div.position,
            redivided[0].position
        );
    }

    fn hsplit_engine_with_sidebar_and_menu() -> Engine {
        let mut e = Engine::new();
        e.settings = crate::core::settings::Settings::default();
        e.mode = crate::core::Mode::Normal;
        e.menu_bar_visible = true;
        if !e.app_shell.sidebar_visible() {
            e.toggle_sidebar();
        }
        // `:split` — a vim window split *within* the single default editor
        // group (#582). `window_status_line` defaults to `true`, so the
        // upper window's own status line (its reserved bottom row) is the
        // only thing marking the boundary — `render_separators` skips
        // drawing a `─` glyph entirely in that case.
        e.split_window(crate::core::window::SplitDirection::Horizontal, None);
        e
    }

    /// #582 smoke-test fix: a `:split` (horizontal) window-divider was
    /// completely unclickable — `render_separators` draws nothing at all for
    /// the boundary when `window_status_line` is on (the default; the upper
    /// window's own status line row visually marks it instead), but the old
    /// `(0.0, 1.0)` tolerance only accepted a click exactly at `div.position`
    /// (the *lower* window's first content row) — one row below the status
    /// line the user actually sees and clicks. Mirrors
    /// `window_divider_click_starts_drag_and_resizes` (the `:vsplit` case)
    /// but for the row axis.
    #[test]
    fn window_divider_horizontal_click_starts_drag_and_resizes() {
        let engine = hsplit_engine_with_sidebar_and_menu();
        let theme = crate::render::Theme::onedark();
        let sidebar = TuiSidebar::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };
        let screen = super::render_impl::build_screen_for_tui(
            &engine,
            &theme,
            area,
            &sidebar,
            SIDEBAR_WIDTH,
        );
        let div = screen
            .window_dividers
            .first()
            .expect("a :split must produce one window divider");
        assert_eq!(
            div.direction,
            crate::core::window::SplitDirection::Horizontal
        );

        let col = (div.cross_start + 1.0) as u16;
        let row = div.position as u16;
        let group_id = div.group_id;
        let split_index = div.split_index;

        // Click on the upper window's status-line row — `row - 1`, the only
        // row actually marked in this default (`window_status_line = true`)
        // configuration — must start the drag.
        let mut engine_hit = hsplit_engine_with_sidebar_and_menu();
        let mut dragging_group = None;
        let mut dragging_window = None;
        dispatch_left_click(
            &mut engine_hit,
            col,
            row - 1,
            Some(&screen),
            &mut dragging_group,
            &mut dragging_window,
        );
        assert_eq!(
            dragging_window,
            Some((group_id, split_index)),
            "click on the status-line row ({col}, {}), one row above the boundary, must start the drag",
            row - 1
        );

        // Two rows off must NOT hit the divider.
        let mut engine_miss = hsplit_engine_with_sidebar_and_menu();
        let mut dragging_group_miss = None;
        let mut dragging_window_miss = None;
        dispatch_left_click(
            &mut engine_miss,
            col,
            row - 2,
            Some(&screen),
            &mut dragging_group_miss,
            &mut dragging_window_miss,
        );
        assert_eq!(
            dragging_window_miss, None,
            "click two rows off the window divider must not start a drag"
        );

        // Dragging down must move the divider down and grow the first pane.
        let move_ev = MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: col,
            row: row + 5,
            modifiers: KeyModifiers::NONE,
        };
        let mut sidebar_state = TuiSidebar::new();
        let mut drag_state = quadraui::DragState::default();
        let mut modal_stack = quadraui::ModalStack::new();
        let mut last_click_time = Instant::now();
        let mut last_click_pos: (u16, u16) = (0, 0);
        let mut should_quit = false;
        handle_mouse(
            move_ev,
            &mut sidebar_state,
            &mut engine_hit,
            &Some(Size {
                width: 120,
                height: 40,
            }),
            SIDEBAR_WIDTH,
            &mut false,
            &mut false,
            &mut false,
            &mut dragging_group,
            &mut dragging_window,
            &mut drag_state,
            &mut modal_stack,
            Some(&screen),
            &mut last_click_time,
            &mut last_click_pos,
            &mut None,
            &mut None,
            &mut false,
            &mut should_quit,
            &mut None,
            &mut None,
            &mut None,
            &mut false,
            &mut None,
            &mut None,
            &mut crate::core::window::DropZone::None,
            &[],
            None,
            None,
            &[],
            None,
            &mut false,
            &mut false,
            None,
            None,
            None,
        );

        let group = engine_hit
            .editor_groups
            .get(&group_id)
            .expect("group still exists");
        let ids = group.active_tab().layout.window_ids();
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        for w in &screen.windows {
            if ids.contains(&w.window_id) {
                min_x = min_x.min(w.rect.x);
                min_y = min_y.min(w.rect.y);
                max_x = max_x.max(w.rect.x + w.rect.width);
                max_y = max_y.max(w.rect.y + w.rect.height);
            }
        }
        let bounds = WindowRect::new(min_x, min_y, max_x - min_x, max_y - min_y);
        let redivided = group.active_tab().layout.dividers(bounds, &mut 0);
        assert!(
            redivided[0].position > div.position,
            "dragging down must move the window divider down (was {}, now {})",
            div.position,
            redivided[0].position
        );
    }

    /// Right-clicking a split group's tab bar at its painted absolute
    /// position must open that group's tab context menu. Exercises the
    /// same `gtb.bounds`-vs-`editor_left`/`menu_rows` arithmetic as the
    /// divider test above, via the right-click path instead of the
    /// left-click path.
    #[test]
    fn split_group_tab_bar_right_click_matches_painted_position() {
        let mut engine = split_engine_with_sidebar_and_menu();
        let theme = crate::render::Theme::onedark();
        let sidebar = TuiSidebar::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };
        let screen = super::render_impl::build_screen_for_tui(
            &engine,
            &theme,
            area,
            &sidebar,
            SIDEBAR_WIDTH,
        );
        let split = screen
            .editor_group_split
            .as_ref()
            .expect("vertical split must produce Some(editor_group_split)");
        let gtb = split
            .group_tab_bars
            .first()
            .expect("a 2-group split has two group tab bars");
        let tab_bar_row = (gtb.bounds.y as u16).saturating_sub(1);
        let col = gtb.bounds.x as u16 + 1;

        assert!(engine.context_menu.is_none());

        let ev = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: col,
            row: tab_bar_row,
            modifiers: KeyModifiers::NONE,
        };
        let mut sidebar_state = TuiSidebar::new();
        let mut drag_state = quadraui::DragState::default();
        let mut modal_stack = quadraui::ModalStack::new();
        let mut last_click_time = Instant::now();
        let mut last_click_pos: (u16, u16) = (0, 0);
        let mut should_quit = false;

        handle_mouse(
            ev,
            &mut sidebar_state,
            &mut engine,
            &Some(Size {
                width: 120,
                height: 40,
            }),
            SIDEBAR_WIDTH,
            &mut false,
            &mut false,
            &mut false,
            &mut None,
            &mut None,
            &mut drag_state,
            &mut modal_stack,
            Some(&screen),
            &mut last_click_time,
            &mut last_click_pos,
            &mut None,
            &mut None,
            &mut false,
            &mut should_quit,
            &mut None,
            &mut None,
            &mut None,
            &mut false,
            &mut None,
            &mut None,
            &mut crate::core::window::DropZone::None,
            &[],
            None,
            None,
            &[],
            None,
            &mut false,
            &mut false,
            None,
            None,
            None,
        );

        assert!(
            engine.context_menu.is_some(),
            "right-click at the painted tab-bar position ({col}, {tab_bar_row}) must open the tab context menu"
        );
    }
}
