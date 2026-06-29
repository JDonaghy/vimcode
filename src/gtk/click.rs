use super::*;
use crate::core::window::GroupId;
use crate::core::WindowId;
use crate::render::{self as render_mod, GutterAction, ScreenZone, WindowZone};

/// Re-export the shared ClickTarget enum.
pub(super) use render_mod::ClickTarget;

/// Convert pixel (x, y) to a click target using the cached ScreenLayout from
/// the last paint pass (#344). Zone detection delegates to the shared
/// `screen_zone_hit_test` / `window_zone_hit_test` / `resolve_gutter_action`
/// functions in render.rs so both backends use one source of truth.
///
/// Tab bar inner hit-testing (which specific tab/button) stays here because it
/// uses Pango-measured pixel slot positions from `draw_tab_bar`.
///
/// `pango_layout` must be configured with the editor monospace font — it is
/// used by `xy_to_index` to convert pixel offsets to character columns,
/// matching the same glyph positioning the paint code uses (#352).
#[allow(clippy::too_many_arguments)]
pub(super) fn pixel_to_click_target(
    engine: &mut Engine,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
    pango_layout: &pango::Layout,
    cached_layout: &render::ScreenLayout,
    // Legacy per-backend pixel maps — no longer consulted for tab-bar clicks
    // (which now resolve through the shared `hit_regions` path). Kept in the
    // signature so existing call sites compile unchanged; slated for removal
    // along with the rest of the pixel-map plumbing. (#515)
    _tab_slot_positions: &TabSlotMap,
    _diff_btn_map: &DiffBtnMap,
    _split_btn_map: &SplitBtnMap,
    _action_btn_map: &ActionBtnMap,
    status_segment_map: &StatusSegmentMap,
) -> ClickTarget {
    let tab_bar_height = render_mod::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
    let single_tab_hidden = engine.is_tab_bar_hidden(engine.active_group);

    match render_mod::screen_zone_hit_test(
        cached_layout,
        x,
        y,
        tab_bar_height,
        single_tab_hidden,
        engine.active_group,
    ) {
        ScreenZone::TabBar {
            group_id,
            local_x,
            bar_width: _,
        } => {
            engine.active_group = group_id;
            tab_bar_inner_hit_test(engine, group_id, local_x, char_width, cached_layout)
        }
        ScreenZone::Window {
            window_id,
            window_idx,
            rel_x,
            rel_y,
        } => {
            engine.activate_group_for_window(window_id);

            let Some(rw) = cached_layout.windows.get(window_idx) else {
                return ClickTarget::None;
            };
            match render_mod::window_zone_hit_test(rw, rel_x, rel_y, line_height, char_width) {
                WindowZone::StatusBar { local_x, .. } => {
                    if let Some(zones) = status_segment_map.get(&window_id.0) {
                        for (start, end, action) in zones {
                            if local_x >= *start && local_x < *end {
                                return ClickTarget::StatusBarAction(action.clone());
                            }
                        }
                    }
                    ClickTarget::None
                }
                WindowZone::Gutter {
                    line_idx,
                    gutter_col,
                    ..
                } => {
                    execute_gutter_action(engine, rw, window_id, line_idx, gutter_col);
                    ClickTarget::Gutter
                }
                WindowZone::TextArea {
                    view_row,
                    buf_line,
                    seg_col_offset,
                    text_rel_x,
                } => {
                    let raw_text = rw
                        .lines
                        .get(view_row)
                        .map(|rl| rl.raw_text.as_str())
                        .unwrap_or("");
                    pango_layout.set_text(raw_text);
                    pango_layout.set_attributes(None);
                    let scroll_px = rw.scroll_left as f64 * char_width;
                    let x_pango = ((text_rel_x + scroll_px).max(0.0) * pango::SCALE as f64) as i32;
                    let (_inside, byte_index, _trailing) = pango_layout.xy_to_index(x_pango, 0);
                    let clamped = (byte_index as usize).min(raw_text.len());
                    let col = raw_text[..clamped].chars().count() + seg_col_offset;
                    ClickTarget::BufferPos(window_id, buf_line, col)
                }
                _ => ClickTarget::None,
            }
        }
        _ => ClickTarget::None,
    }
}

/// Tab bar inner hit-test via the shared, backend-neutral hit regions.
///
/// `local_x` is pixels relative to the tab bar's left edge; dividing by
/// `char_width` yields the char-cell column that [`render::resolve_tab_bar_click`]
/// expects — the same column space the TUI backend uses. This replaces the
/// legacy per-backend pixel maps (`tab_slot_positions` / `*_btn_map`), which were
/// populated only by the now-dead Relm4 Cairo draw path. (#515)
fn tab_bar_inner_hit_test(
    engine: &mut Engine,
    group_id: GroupId,
    local_x: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
) -> ClickTarget {
    use crate::core::engine::TabBarClickTarget as T;
    use crate::core::window::SplitDirection;

    let col = (local_x / char_width).floor().max(0.0) as u16;

    // Multi-group: each group carries its own hit regions. Single-group: the
    // active tab bar's regions live on the ScreenLayout directly.
    let regions: &[(
        crate::core::engine::TabBarHitRegion,
        crate::core::engine::TabBarClickTarget,
    )] = if let Some(ref split) = cached_layout.editor_group_split {
        split
            .group_tab_bars
            .iter()
            .find(|g| g.group_id == group_id)
            .map(|g| g.hit_regions.as_slice())
            .unwrap_or(&[])
    } else {
        cached_layout.tab_bar_hit_regions.as_slice()
    };

    match render_mod::resolve_tab_bar_click(regions, col) {
        Some(T::Tab(idx)) => {
            engine.goto_tab(idx);
            ClickTarget::TabBar
        }
        Some(T::CloseTab(idx)) => {
            if let Some(g) = engine.editor_groups.get_mut(&group_id) {
                g.active_tab = idx;
            }
            engine.line_annotations.clear();
            ClickTarget::CloseTab(group_id, idx)
        }
        Some(T::SplitRight) => ClickTarget::SplitButton(group_id, SplitDirection::Vertical),
        Some(T::SplitDown) => ClickTarget::SplitButton(group_id, SplitDirection::Horizontal),
        Some(T::ActionMenu) => ClickTarget::ActionMenuButton(group_id),
        Some(T::DiffPrev) => ClickTarget::DiffToolbarPrev,
        Some(T::DiffNext) => ClickTarget::DiffToolbarNext,
        Some(T::DiffToggle) => ClickTarget::DiffToolbarToggleFold,
        None => ClickTarget::TabBar,
    }
}

/// Execute the engine-side action for a gutter click using shared resolution.
fn execute_gutter_action(
    engine: &mut Engine,
    rw: &render::RenderedWindow,
    window_id: WindowId,
    line_idx: usize,
    gutter_col: usize,
) {
    match render_mod::resolve_gutter_action(rw, line_idx, gutter_col) {
        Some(GutterAction::ToggleBreakpoint(line)) => {
            let file = engine
                .windows
                .get(&window_id)
                .and_then(|w| engine.buffer_manager.get(w.buffer_id))
                .and_then(|bs| bs.file_path.as_ref())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            engine.dap_toggle_breakpoint(&file, line as u64 + 1);
        }
        Some(GutterAction::DiffPeek(line)) => {
            engine.active_tab_mut().active_window = window_id;
            engine.view_mut().cursor.line = line;
            engine.open_diff_peek();
        }
        Some(GutterAction::DiagnosticHover(line)) => {
            engine.active_tab_mut().active_window = window_id;
            engine.view_mut().cursor.line = line;
            engine.trigger_editor_hover_for_line(line);
        }
        Some(GutterAction::CodeAction(line)) => {
            engine.active_tab_mut().active_window = window_id;
            engine.view_mut().cursor.line = line;
            engine.show_code_actions_popup();
        }
        Some(GutterAction::ToggleFold(line)) => {
            engine.toggle_fold_at_line(line);
        }
        None => {}
    }
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
    alt: bool,
    line_height: f64,
    char_width: f64,
    pango_layout: &pango::Layout,
    cached_layout: &render::ScreenLayout,
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
        line_height,
        char_width,
        pango_layout,
        cached_layout,
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
            let col = (x / char_width.max(1.0)) as u16;
            let row = (y / line_height.max(1.0)) as u16;
            // #434: pass the trigger's exact height in line_height units so
            // the menu sits flush against the button's bottom (no sub-cell
            // gap). GTK's tab row is ceil(1.6 * line_height).
            let trigger_h =
                (render_mod::tab_row_height_px(line_height) / line_height.max(1.0)) as f32;
            engine.open_editor_action_menu(group_id, col, row, trigger_h);
            (None, None)
        }
        _ => (None, None),
    }
}

// Tab-drag drop-zone geometry is now computed in `App::render_content` from the
// shared `render::screen_to_drop_group_bounds` pipeline and cached on the App for
// the drag hit-test to reuse — see `cached_drop_groups`. The former GTK-specific
// `build_gtk_tab_slots` / `compute_tab_drop_zone` helpers (which depended on the
// legacy per-backend pixel maps) were removed in #515.

/// Handle mouse double-click — select word at position.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_mouse_double_click(
    engine: &mut Engine,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
    pango_layout: &pango::Layout,
    cached_layout: &render::ScreenLayout,
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
        line_height,
        char_width,
        pango_layout,
        cached_layout,
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
    line_height: f64,
    char_width: f64,
    pango_layout: &pango::Layout,
    cached_layout: &render::ScreenLayout,
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
        line_height,
        char_width,
        pango_layout,
        cached_layout,
        tab_slot_positions,
        diff_btn_map,
        split_btn_map,
        action_btn_map,
        status_segment_map,
    ) {
        engine.mouse_drag(wid, line, col);
    }
}
