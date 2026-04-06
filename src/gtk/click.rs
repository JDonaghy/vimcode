use super::*;
use crate::render::{view_row_to_buf_line, view_row_to_buf_pos_wrap};

/// Result of converting pixel coordinates to buffer position.
pub(super) enum ClickTarget {
    /// Click was in the tab bar, tab already switched.
    TabBar,
    /// Click was in gutter — fold already toggled.
    Gutter,
    /// Click resolved to a buffer position in a specific window.
    BufferPos(core::WindowId, usize, usize),
    /// Click was on a tab-bar split button: (group_id, direction).
    SplitButton(core::window::GroupId, crate::core::window::SplitDirection),
    /// Click was on a tab's × close button: (group_id, tab_idx).
    CloseTab(core::window::GroupId, usize),
    /// Click was on a diff toolbar prev-change button.
    DiffToolbarPrev,
    /// Click was on a diff toolbar next-change button.
    DiffToolbarNext,
    /// Click was on a diff toolbar toggle-fold button.
    DiffToolbarToggleFold,
    /// Click was on a per-window status bar segment with an action.
    StatusBarAction(crate::core::engine::StatusAction),
    /// Click was on the editor action menu button ("…").
    ActionMenuButton(core::window::GroupId),
    /// Click was outside any actionable area.
    None,
}

/// Convert pixel (x, y) to a buffer position (window_id, line, col).
/// Also handles tab-bar clicks and gutter fold toggles.
#[allow(clippy::too_many_arguments)]
pub(super) fn pixel_to_click_target(
    engine: &mut Engine,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    line_height: f64,
    char_width: f64,
    tab_slot_positions: &TabSlotMap,
    diff_btn_map: &DiffBtnMap,
    split_btn_map: &SplitBtnMap,
    action_btn_map: &ActionBtnMap,
    status_segment_map: &StatusSegmentMap,
) -> ClickTarget {
    let tab_row_height = (line_height * 1.6).ceil();
    let tab_bar_height = if engine.settings.breadcrumbs {
        tab_row_height + line_height
    } else {
        tab_row_height
    };

    // Check if click is in a group's tab bar region.
    // Use the group layout tree to find group bounds (must match draw_editor layout).
    {
        let wildmenu_px = if engine.wildmenu_items.is_empty() {
            0.0
        } else {
            line_height
        };
        let per_window_status = engine.settings.window_status_line;
        let global_status_rows = if per_window_status { 1.0 } else { 2.0 };
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

        // Check if click is in any group's tab bar row.
        for (gid, grect) in &group_rects {
            if engine.is_tab_bar_hidden(*gid) {
                continue;
            }
            let tab_y = grect.y - tab_bar_height;
            let tab_x_start = grect.x;
            let bar_width = grect.width;
            if y >= tab_y
                && y < tab_y + tab_bar_height
                && x >= tab_x_start
                && x < tab_x_start + bar_width
            {
                let group_id = *gid;
                engine.active_group = group_id;
                let local_x = x - tab_x_start;

                // Hit-test diff toolbar buttons FIRST (they sit left of split
                // buttons, so check them before split to avoid boundary overlap).
                if let Some(&(prev_start, prev_end, next_start, next_end, fold_start, fold_end)) =
                    diff_btn_map.get(&group_id.0)
                {
                    if local_x >= prev_start && local_x < prev_end {
                        return ClickTarget::DiffToolbarPrev;
                    } else if local_x >= next_start && local_x < next_end {
                        return ClickTarget::DiffToolbarNext;
                    } else if local_x >= fold_start && local_x < fold_end {
                        return ClickTarget::DiffToolbarToggleFold;
                    }
                }

                // Hit-test split buttons using cached Pango-measured widths.
                // Split buttons sit to the left of the action menu button.
                if let Some(&(both_btns_px, btn_right_px)) = split_btn_map.get(&group_id.0) {
                    let action_offset = action_btn_map
                        .get(&group_id.0)
                        .map(|&(start, end)| end - start)
                        .unwrap_or(0.0);
                    let btn_down_px = both_btns_px - btn_right_px;
                    if local_x >= bar_width - btn_down_px - action_offset
                        && local_x < bar_width - action_offset
                    {
                        return ClickTarget::SplitButton(
                            group_id,
                            crate::core::window::SplitDirection::Horizontal,
                        );
                    }
                    if local_x >= bar_width - both_btns_px - action_offset
                        && local_x < bar_width - btn_down_px - action_offset
                    {
                        return ClickTarget::SplitButton(
                            group_id,
                            crate::core::window::SplitDirection::Vertical,
                        );
                    }
                }

                // Hit-test action menu button ("…") at the far right.
                if let Some(&(start_x, end_x)) = action_btn_map.get(&group_id.0) {
                    if local_x >= start_x && local_x < end_x {
                        return ClickTarget::ActionMenuButton(group_id);
                    }
                }

                // Hit-test tabs using cached Pango-measured positions from draw_tab_bar.
                let hit =
                    tab_slot_positions
                        .get(&group_id.0)
                        .and_then(|slots: &Vec<(f64, f64)>| {
                            for (i, &(slot_start, slot_end)) in slots.iter().enumerate() {
                                if local_x >= slot_start && local_x < slot_end {
                                    // Close button is in the last ~20% of the slot
                                    let close_zone = (slot_end - slot_start) * 0.2;
                                    let is_close = local_x >= slot_end - close_zone;
                                    return Some((i, is_close));
                                }
                            }
                            None
                        });
                if let Some((tab_idx, is_close)) = hit {
                    if is_close {
                        // For close, just set active_tab directly (tab will be removed)
                        if let Some(g) = engine.editor_groups.get_mut(&group_id) {
                            g.active_tab = tab_idx;
                        }
                        engine.line_annotations.clear();
                        return ClickTarget::CloseTab(group_id, tab_idx);
                    }
                    // For tab switch, use goto_tab which also updates MRU
                    engine.goto_tab(tab_idx);
                    return ClickTarget::TabBar;
                }
                return ClickTarget::TabBar;
            }
        }
    }

    let wildmenu_px = if engine.wildmenu_items.is_empty() {
        0.0
    } else {
        line_height
    };
    let global_status_rows = if engine.settings.window_status_line {
        1.0
    } else {
        2.0
    };
    let status_bar_height = line_height * global_status_rows + wildmenu_px;
    let qf_px2 = if engine.quickfix_open {
        let n = engine.quickfix_items.len().clamp(1, 10) as f64;
        (n + 1.0) * line_height
    } else {
        0.0
    };
    let term_px2 = if engine.terminal_open || engine.bottom_panel_open {
        (engine.session.terminal_panel_rows as usize + 2) as f64 * line_height
    } else {
        0.0
    };
    let dbg_px2 = if engine.debug_toolbar_visible {
        line_height
    } else {
        0.0
    };
    let editor_bottom = height - status_bar_height - dbg_px2 - qf_px2 - term_px2;

    if y >= editor_bottom {
        return ClickTarget::None;
    }

    let editor_bounds = WindowRect::new(0.0, 0.0, width, editor_bottom);
    let (window_rects, _dividers) =
        engine.calculate_group_window_rects(editor_bounds, tab_bar_height);
    let clicked_window = window_rects.iter().find(|(_, rect)| {
        x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
    });

    let (window_id, rect) = match clicked_window {
        Some((id, r)) => (*id, r),
        None => return ClickTarget::None,
    };

    // Update active_group if the clicked window belongs to a different group.
    for (&gid, group) in &engine.editor_groups {
        if group
            .tabs
            .iter()
            .any(|t| t.window_ids().contains(&window_id))
        {
            engine.active_group = gid;
            break;
        }
    }

    let window = match engine.windows.get(&window_id) {
        Some(w) => w,
        None => return ClickTarget::None,
    };

    let buffer_state = match engine.buffer_manager.get(window.buffer_id) {
        Some(s) => s,
        None => return ClickTarget::None,
    };

    let buffer = &buffer_state.buffer;
    let view = &window.view;

    let total_lines = buffer.content.len_lines();
    let has_git = !buffer_state.git_diff.is_empty();
    let has_bp_click = {
        let key = buffer_state
            .file_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        #[allow(clippy::unnecessary_map_or)] // is_none_or requires Rust 1.82+
        {
            !engine
                .dap_breakpoints
                .get(&key)
                .map_or(true, |v| v.is_empty())
                || engine.dap_session_active
        }
    };
    let gutter_char_width = render::calculate_gutter_cols(
        engine.settings.line_numbers,
        total_lines,
        char_width,
        has_git,
        has_bp_click,
    );
    let gutter_width = gutter_char_width as f64 * char_width;

    let per_window_status_px = if engine.settings.window_status_line {
        line_height
    } else {
        0.0
    };
    let text_area_height = rect.height - per_window_status_px;

    if y >= rect.y + text_area_height {
        // Click is in the per-window status bar area — use Pango-measured cached zones
        if engine.settings.window_status_line {
            let local_x = x - rect.x;
            if let Some(zones) = status_segment_map.get(&window_id.0) {
                for (start, end, action) in zones {
                    if local_x >= *start && local_x < *end {
                        return ClickTarget::StatusBarAction(action.clone());
                    }
                }
            }
        }
        return ClickTarget::None;
    }

    let relative_y = y - rect.y;
    let view_row = (relative_y / line_height).floor() as usize;

    // Compute the buffer line and segment column offset, accounting for wrapping.
    let (line, seg_col_offset) = if engine.settings.wrap {
        // Compute viewport_cols the same way render.rs does for word-wrap segments.
        let scrollbar_px: f64 = if char_width > 1.0 { 8.0 } else { 0.0 };
        let render_viewport_cols = if char_width > 0.0 {
            let total_chars = ((rect.width - scrollbar_px) / char_width).floor() as usize;
            total_chars.saturating_sub(gutter_char_width).max(1)
        } else {
            1
        };
        view_row_to_buf_pos_wrap(
            view,
            buffer,
            view.scroll_top,
            view_row,
            total_lines,
            render_viewport_cols,
        )
    } else {
        (
            view_row_to_buf_line(view, view.scroll_top, view_row, total_lines),
            0,
        )
    };

    // Gutter click
    if x >= rect.x && x < rect.x + gutter_width && gutter_width > 0.0 {
        // Determine which gutter column was clicked.
        let gutter_col = ((x - rect.x) / char_width).floor() as usize;
        let bp_offset = if has_bp_click { 1 } else { 0 };
        let git_col = if has_git { bp_offset } else { usize::MAX };

        if has_bp_click && gutter_col == 0 {
            // Breakpoint column (leftmost).
            let file = buffer_state
                .file_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            engine.dap_toggle_breakpoint(&file, line as u64 + 1);
        } else if gutter_col == git_col {
            // Git diff column — open diff peek popup.
            engine.active_tab_mut().active_window = window_id;
            engine.view_mut().cursor.line = line;
            engine.open_diff_peek();
        } else if engine.has_diagnostic_on_line(line) {
            // Diagnostic gutter indicator — show hover popup with details.
            engine.active_tab_mut().active_window = window_id;
            engine.view_mut().cursor.line = line;
            engine.trigger_editor_hover_for_line(line);
        } else if engine.has_code_actions_on_line(line) {
            // Code action lightbulb — show code actions popup.
            engine.active_tab_mut().active_window = window_id;
            engine.view_mut().cursor.line = line;
            engine.show_code_actions_popup();
        } else {
            engine.toggle_fold_at_line(line);
        }
        return ClickTarget::Gutter;
    }

    let relative_x = x - (rect.x + gutter_width);
    let line = line.min(buffer.content.len_lines().saturating_sub(1));

    // For a monospace font, column = floor(relative_x / char_width).
    // Tabs are displayed as 4 spaces, so we walk the line chars mapping
    // display columns to logical columns without any Pango calls.
    let display_col = if char_width > 0.0 && relative_x >= 0.0 {
        (relative_x / char_width) as usize
    } else {
        0
    };

    // Walk the line text starting from segment_col_offset to find the
    // logical column corresponding to the clicked display column.
    let line_text = buffer.content.line(line).to_string();
    let mut col = seg_col_offset;
    let mut display_pos = 0;
    for ch in line_text.chars().skip(seg_col_offset) {
        if display_pos >= display_col {
            break;
        }
        if ch == '\t' {
            display_pos += 4;
        } else {
            display_pos += 1;
        }
        col += 1;
    }

    ClickTarget::BufferPos(window_id, line, col)
}

/// Handle mouse click by converting coordinates to buffer position.
/// Returns: `(click, engine_action)` where click is `None` = non-buffer click,
/// `Some(true)` = close-tab on dirty buffer, `Some(false)` = normal buffer click;
/// `engine_action` is an optional action the caller must dispatch (e.g. sidebar toggle).
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_mouse_click(
    engine: &mut Engine,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    alt: bool,
    line_height: f64,
    char_width: f64,
    tab_slot_positions: &TabSlotMap,
    diff_btn_map: &DiffBtnMap,
    split_btn_map: &SplitBtnMap,
    action_btn_map: &ActionBtnMap,
    status_segment_map: &StatusSegmentMap,
) -> (Option<bool>, Option<EngineAction>) {
    match pixel_to_click_target(
        engine,
        x,
        y,
        width,
        height,
        line_height,
        char_width,
        tab_slot_positions,
        diff_btn_map,
        split_btn_map,
        action_btn_map,
        status_segment_map,
    ) {
        ClickTarget::BufferPos(wid, line, col) => {
            // Alt+Click in VSCode mode → add cursor at position
            if alt && engine.is_vscode_mode() {
                engine.add_cursor_at_pos(line, col);
            } else {
                engine.mouse_click(wid, line, col);
            }
            (Some(false), None)
        }
        ClickTarget::SplitButton(group_id, dir) => {
            engine.active_group = group_id;
            engine.open_editor_group(dir);
            (None, None)
        }
        ClickTarget::DiffToolbarPrev => {
            if engine.windows.contains_key(&engine.active_window_id()) {
                engine.jump_prev_hunk();
            }
            (None, None)
        }
        ClickTarget::DiffToolbarNext => {
            if engine.windows.contains_key(&engine.active_window_id()) {
                engine.jump_next_hunk();
            }
            (None, None)
        }
        ClickTarget::DiffToolbarToggleFold => {
            engine.diff_toggle_hide_unchanged();
            (None, None)
        }
        ClickTarget::CloseTab(group_id, tab_idx) => {
            if let Some(g) = engine.editor_groups.get_mut(&group_id) {
                g.active_tab = tab_idx;
            }
            engine.active_group = group_id;
            engine.line_annotations.clear();
            if engine.dirty() {
                return (Some(true), None);
            }
            engine.close_tab();
            (None, None)
        }
        ClickTarget::StatusBarAction(action) => {
            let ea = engine.handle_status_action(&action);
            (None, ea)
        }
        ClickTarget::ActionMenuButton(group_id) => {
            engine.open_editor_action_menu(group_id, 0, 0);
            (None, None)
        }
        _ => (None, None),
    }
}

/// Compute the drop zone for a tab drag based on cursor position.
#[allow(clippy::too_many_arguments)]
pub(super) fn compute_tab_drop_zone(
    engine: &Engine,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    line_height: f64,
    char_width: f64,
    tab_slot_positions: &TabSlotMap,
) -> crate::core::window::DropZone {
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
    let status_bar_height = line_height * 2.0 + wildmenu_px;
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

    for (gid, grect) in &group_rects {
        let tab_hidden = engine.is_tab_bar_hidden(*gid);
        let tab_x = grect.x;
        let group_id = *gid;

        // Check tab bar region (for reorder/center drop) — skip if tab bar is hidden
        let tab_y = grect.y - tab_bar_height;
        if !tab_hidden
            && y >= tab_y
            && y < tab_y + tab_row_height
            && x >= tab_x
            && x < tab_x + grect.width
        {
            // Determine insertion index from tab slot positions
            let local_x = x - tab_x;
            if let Some(slots) = tab_slot_positions.get(&group_id.0) {
                for (i, &(slot_start, slot_end)) in slots.iter().enumerate() {
                    let mid = (slot_start + slot_end) / 2.0;
                    if local_x < mid {
                        return DropZone::TabReorder(group_id, i);
                    }
                }
                return DropZone::TabReorder(group_id, slots.len());
            }
            return DropZone::Center(group_id);
        }

        // Check content area with edge margins
        let content_top = grect.y;
        let content_left = grect.x;
        let content_right = grect.x + grect.width;
        let content_bottom = grect.y + grect.height;
        if x >= content_left && x < content_right && y >= content_top && y < content_bottom {
            let w = grect.width;
            let h = grect.height;
            let rel_x = x - content_left;
            let rel_y = y - content_top;
            let margin = 0.2;

            // Minimum 40px or char_width*5 for edge zones
            let edge_w = (w * margin).min(char_width * 10.0).max(40.0);
            let edge_h = (h * margin).min(line_height * 3.0).max(40.0);

            if rel_x < edge_w {
                return DropZone::Split(group_id, SplitDirection::Vertical, true);
            }
            if rel_x > w - edge_w {
                return DropZone::Split(group_id, SplitDirection::Vertical, false);
            }
            if rel_y < edge_h {
                return DropZone::Split(group_id, SplitDirection::Horizontal, true);
            }
            if rel_y > h - edge_h {
                return DropZone::Split(group_id, SplitDirection::Horizontal, false);
            }
            return DropZone::Center(group_id);
        }
    }

    DropZone::None
}

/// Handle mouse double-click — select word at position.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_mouse_double_click(
    engine: &mut Engine,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    line_height: f64,
    char_width: f64,
    tab_slot_positions: &TabSlotMap,
    diff_btn_map: &DiffBtnMap,
    split_btn_map: &SplitBtnMap,
    action_btn_map: &ActionBtnMap,
    status_segment_map: &StatusSegmentMap,
) {
    if let ClickTarget::BufferPos(wid, line, col) = pixel_to_click_target(
        engine,
        x,
        y,
        width,
        height,
        line_height,
        char_width,
        tab_slot_positions,
        diff_btn_map,
        split_btn_map,
        action_btn_map,
        status_segment_map,
    ) {
        engine.mouse_double_click(wid, line, col);
    }
}

/// Handle mouse drag — extend visual selection.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_mouse_drag(
    engine: &mut Engine,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    line_height: f64,
    char_width: f64,
    tab_slot_positions: &TabSlotMap,
    diff_btn_map: &DiffBtnMap,
    split_btn_map: &SplitBtnMap,
    action_btn_map: &ActionBtnMap,
    status_segment_map: &StatusSegmentMap,
) {
    if let ClickTarget::BufferPos(wid, line, col) = pixel_to_click_target(
        engine,
        x,
        y,
        width,
        height,
        line_height,
        char_width,
        tab_slot_positions,
        diff_btn_map,
        split_btn_map,
        action_btn_map,
        status_segment_map,
    ) {
        engine.mouse_drag(wid, line, col);
    }
}
