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
    divider_grab: &mut Option<render::DividerGrab>,
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
    tab_drag: &mut render::TabDragState,
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
    tab_switcher_popup_rect: Option<quadraui::Rect>,
) -> u16 {
    let col = ev.column;
    let row = ev.row;
    let term_height = terminal_size.map(|s| s.height).unwrap_or(24);

    // #695: the row offset every hit test below must subtract for the
    // menu-bar band, derived *once* here from `engine.menu_bar_rect` — the
    // same cache `TuiShellApp::render_content` populated this frame from
    // `layout.title_bar_bounds` (the shell's actual reservation) — instead
    // of each call site independently re-deriving `engine.menu_bar_visible
    // ? 1 : 0`. That per-site re-derivation (previously duplicated across
    // eight places in this function) assumed the flag and the reservation
    // always agree; caching what was actually painted removes the
    // assumption instead of just hoping it holds.
    let menu_rows: u16 = engine.menu_bar_rect.get().height.round() as u16;

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

    // ── Modal-overlay rung (#733 / #751) ──────────────────────────────────────
    //
    // Toast → dialog → context menu → tab switcher → completion → picker →
    // find/replace, sequenced ONCE in `render::route_modal_overlay_click` and
    // shared verbatim with GTK's `handle_mouse_click_msg`. Before #733 this
    // backend ran toast → find/replace → dialog → … → completion and GTK ran a
    // different order; worse, neither arbitrated every surface the other
    // did — the Ctrl+Tab switcher popup had no TUI mouse arm at all, so a
    // click on it fell straight through and moved the editor cursor
    // underneath. #751 folded in the last three rungs, whose per-backend
    // copies had drifted the same way — the context menu was arbitrated
    // ~1,100 lines *below* the picker and find/replace here even though it is
    // painted above both (`render::OVERLAY_Z_ORDER`), and GTK had no
    // context-menu hover arm at all (#373).
    //
    // Every layout below is the one the last frame actually PAINTED
    // (`tab_switcher_popup_rect`, `context_menu_layout`, `last_layout`), never
    // one recomputed here (#582 / #646).
    let picker_geometry = engine.picker_open.then(|| {
        let term_cols = terminal_size.map(|s| s.width).unwrap_or(80);
        let term_rows = terminal_size.map(|s| s.height).unwrap_or(24);
        let has_preview = engine.picker_preview.is_some();
        let geo = render::PickerGeometry::compute(
            term_cols as f32,
            term_rows as f32,
            has_preview,
            &render::TUI_PICKER_SIZING,
        );
        render::PickerHitGeometry::new(
            quadraui::Rect::new(geo.popup_x, geo.popup_y, geo.popup_w, geo.popup_h),
            1.0,
            has_preview,
            &render::TUI_PICKER_ROWS,
            engine,
        )
    });
    let find_replace_geometry = last_layout
        .and_then(|l| l.find_replace.as_ref())
        .map(|panel| {
            render::FindReplaceHitGeometry::from_panel(
                panel,
                (1.0, 1.0),
                &render::TUI_FIND_REPLACE_ANCHOR,
            )
        });
    // Keep the picker's modal-stack entry in step with the geometry above:
    // `dispatch_scroll` and the drag guard both consult the stack.
    if let Some(geo) = picker_geometry {
        modal_stack.push(quadraui::WidgetId::new("picker"), geo.bounds);
    }
    {
        let action = match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => render::ModalMouseAction::LeftPress,
            MouseEventKind::Up(MouseButton::Left) => render::ModalMouseAction::LeftRelease,
            MouseEventKind::Moved => render::ModalMouseAction::Move,
            _ => render::ModalMouseAction::Other,
        };
        let toast = engine.toast_layout.borrow().clone();
        let route = render::route_modal_overlay_click(
            &render::ModalOverlayState {
                toast: toast.as_ref(),
                dialog_open: engine.dialog.is_some(),
                dialog: dialog_layout,
                context_menu_open: engine.context_menu.is_some(),
                context_menu: context_menu_layout,
                // The TUI rasteriser draws the menu's box-drawing frame one
                // cell *outside* `ContextMenuLayout::bounds`, so a click on
                // the frame must consume rather than dismiss.
                context_menu_border: 1.0,
                tab_switcher_open: engine.tab_switcher_open,
                tab_switcher_bounds: tab_switcher_popup_rect,
                completion_open: engine.completion_idx.is_some(),
                completion: completion_layout,
                picker_open: engine.picker_open,
                picker: picker_geometry,
                find_replace_open: engine.find_replace_open,
                find_replace: find_replace_geometry,
            },
            col as f32,
            row as f32,
            action,
        );
        drop(toast);

        match route {
            render::ModalOverlayRoute::Toast(hit) => {
                if engine.handle_toast_hit(hit) {
                    return sidebar_width;
                }
            }
            render::ModalOverlayRoute::Dialog(hit) => {
                match hit {
                    quadraui::DialogHit::Button(id) => {
                        if let Some(idx) = id
                            .as_str()
                            .strip_prefix("dialog:btn:")
                            .and_then(|s| s.parse::<usize>().ok())
                        {
                            let dlg_action = engine.dialog_click_button(idx);
                            if engine.explorer_needs_refresh {
                                engine.explorer_needs_refresh = false;
                                engine.explorer_rebuild_rows();
                            }
                            if handle_action(engine, dlg_action) {
                                *should_quit = true;
                            }
                        }
                    }
                    quadraui::DialogHit::Outside => {
                        engine.dialog = None;
                        engine.pending_move = None;
                    }
                    quadraui::DialogHit::Body | quadraui::DialogHit::BodyToolbarButton(_) => {}
                }
                return sidebar_width;
            }
            render::ModalOverlayRoute::ContextMenu(cm) => match cm {
                render::ContextMenuRoute::Item(idx) => {
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
                render::ContextMenuRoute::Hover(idx) => {
                    if let Some(ref mut cm) = engine.context_menu {
                        cm.selected = idx;
                    }
                    return sidebar_width;
                }
                render::ContextMenuRoute::Consume => return sidebar_width,
                render::ContextMenuRoute::Dismiss => {
                    engine.close_context_menu();
                    return sidebar_width;
                }
                render::ContextMenuRoute::Fallthrough => {}
            },
            render::ModalOverlayRoute::TabSwitcher { inside } => {
                // Click anywhere dismisses; inside also consumes so the
                // editor underneath doesn't take a cursor move through it.
                engine.tab_switcher_open = false;
                if inside {
                    return sidebar_width;
                }
            }
            render::ModalOverlayRoute::Completion(hit) => {
                if engine.handle_completion_click(hit) {
                    return sidebar_width;
                }
            }
            // `picker_geometry` is `Some` whenever the router produced this
            // route — both come from the same `engine.picker_open` gate above.
            render::ModalOverlayRoute::UnifiedPicker(hit) if picker_geometry.is_some() => {
                let geo = picker_geometry.unwrap();
                match hit {
                    render::PickerRoute::Row(idx) => {
                        render::apply_picker_row_click(engine, idx);
                    }
                    render::PickerRoute::ScrollbarThumb { grab_offset } => {
                        drag_state
                            .begin(geo.drag_target(quadraui::WidgetId::new("picker"), grab_offset));
                    }
                    render::PickerRoute::ScrollbarTrack { toward_end } => {
                        render::apply_picker_scroll_offset(
                            engine,
                            geo.paged_offset(toward_end),
                            geo.visible_rows,
                        );
                    }
                    render::PickerRoute::Consume => {}
                    render::PickerRoute::Dismiss => {
                        engine.close_picker();
                        modal_stack.pop(&quadraui::WidgetId::new("picker"));
                    }
                }
                return sidebar_width;
            }
            render::ModalOverlayRoute::FindReplace(hit) => {
                match hit {
                    render::FindReplaceRoute::Target { target, is_input } => {
                        // Double-click inside an input field selects the word
                        // under the cursor. TUI-only: it needs the click-time
                        // history this backend keeps and GTK routes its own
                        // double-clicks through `UiEvent::DoubleClick`.
                        let now = Instant::now();
                        let is_double = now.duration_since(*last_click_time)
                            < Duration::from_millis(400)
                            && *last_click_pos == (col, row);
                        *last_click_time = now;
                        *last_click_pos = (col, row);

                        use crate::core::engine::FindReplaceClickTarget::*;
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
                        if is_input {
                            *fr_input_dragging = true;
                        }
                        engine.handle_find_replace_click(target);
                    }
                    render::FindReplaceRoute::Consume => {}
                }
                return sidebar_width;
            }
            render::ModalOverlayRoute::UnifiedPicker(_) => return sidebar_width,
            render::ModalOverlayRoute::Swallow => return sidebar_width,
            render::ModalOverlayRoute::None => {}
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

    // ── Find/replace input drag (TUI-only follow-through) ─────────────────────
    //
    // The *click* rung is shared (`render::ModalOverlayRoute::FindReplace`
    // above); what stays here is the press-and-drag selection gesture, which
    // GTK routes through `handle_mouse_drag_msg` instead. Both read the same
    // `find_replace_geometry`, so the drag can no longer land on different
    // columns than the click that started it.
    if let Some(geo) = engine
        .find_replace_open
        .then_some(find_replace_geometry)
        .flatten()
    {
        let b = geo.bounds;
        let on_panel = (col as f32) >= b.x
            && (col as f32) < b.x + b.width
            && (row as f32) >= b.y
            && (row as f32) < b.y + b.height;

        if let MouseEventKind::Drag(MouseButton::Left) = ev.kind {
            if *fr_input_dragging && on_panel {
                let rel_col = col.saturating_sub(geo.content_origin.0 as u16);
                let focus_row = if engine.find_replace_focus == 0 { 0 } else { 1 };
                let input_region = geo.hit_regions.iter().find(|(r, t)| {
                    matches!(
                        t,
                        crate::core::engine::FindReplaceClickTarget::FindInput(_)
                            | crate::core::engine::FindReplaceClickTarget::ReplaceInput(_)
                    ) && r.row == focus_row
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

        if let MouseEventKind::Up(MouseButton::Left) = ev.kind {
            if *fr_input_dragging {
                *fr_input_dragging = false;
                // Cursor never left the anchor — that was a plain click.
                if engine.find_replace_sel_anchor == Some(engine.find_replace_cursor) {
                    engine.find_replace_sel_anchor = None;
                }
                return sidebar_width;
            }
        }
    }
    // ── Folder picker mouse handling ────────────────────────────────────────────
    //
    // #751 verdict: **deliberately one-sided, do not converge.** Every other
    // rung in `handle_mouse` above has a GTK twin that
    // `render::route_modal_overlay_click` now arbitrates for both backends.
    // This one does not: GTK opens the *native* GTK file chooser, deferred
    // through `PendingFileDialog` and run from `tick()`, so there is no GTK
    // canvas surface to hit-test and nothing for a shared router to arbitrate
    // against. (`render::OVERLAY_Z_ORDER` records the same verdict for the
    // paint side.) Inventing a GTK canvas picker purely to make the two tables
    // match would add per-backend code, not remove it — the opposite of
    // `GOALS.md` milestone #7.
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

    // ── Unified picker: drag / scroll follow-through ──────────────────────────
    //
    // The picker's *click* rung is shared with GTK
    // (`render::ModalOverlayRoute::UnifiedPicker` above, resolved by
    // `render::PickerHitGeometry`). What is left here is the wheel and the
    // continuation of an already-armed scrollbar drag — gestures GTK receives
    // through different `UiEvent`s entirely.
    if let Some(geo) = picker_geometry {
        match ev.kind {
            MouseEventKind::Drag(MouseButton::Left) if drag_state.is_active() => {
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
                        render::apply_picker_scroll_offset(engine, *new_offset, geo.visible_rows);
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                drag_state.end();
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let scroll_down = matches!(ev.kind, MouseEventKind::ScrollDown);
                let on_preview =
                    engine.picker_preview.is_some() && (col as f32) > geo.bounds.x + geo.list_width;
                if on_preview {
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
            // Tab drag-and-drop: the arm → threshold → track machine is
            // shared with GTK (`render::TabDragState`, #753). `2.0` is the
            // squared cell threshold — see `handle_move`'s doc for why it is
            // exactly the Manhattan `dx + dy >= 2` this replaced.
            match tab_drag.handle_move(col as f64, row as f64, 2.0) {
                render::TabDragMove::Tracking => {
                    tab_drag.track(compute_tui_tab_drop_zone(
                        engine,
                        col,
                        row,
                        editor_left,
                        last_layout,
                        *terminal_size,
                    ));
                    return sidebar_width;
                }
                render::TabDragMove::Crossed { .. } => {
                    // Only the tab-bar arm arms the drag here, so the press
                    // point is known to have been on a tab: use the active
                    // group + active tab as the source (GTK has to re-resolve
                    // the press because its arm covers the whole band).
                    let gid = engine.active_group;
                    let tidx = engine
                        .editor_groups
                        .get(&gid)
                        .map(|g| g.active_tab)
                        .unwrap_or(0);
                    tab_drag.begin((gid, tidx), col as f64, row as f64);
                    return sidebar_width;
                }
                // Haven't moved enough yet — don't start any drag.
                render::TabDragMove::Pending => return sidebar_width,
                render::TabDragMove::Idle => {}
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
                let min_editor_chrome = 4 + menu_rows + 1; // 4 lines + menu + tab bar
                let max_rows = term_height
                    .saturating_sub(bottom_chrome + qf_h + min_editor_chrome + 2) // +2 for terminal tab bar + header
                    .max(5);
                let new_rows = available.saturating_sub(1).clamp(5, max_rows);
                engine.session.terminal_panel_rows = new_rows;
                return sidebar_width;
            }
            // Divider drag — group boundary or `:split` boundary, both
            // through the shared applier (#753).
            //
            // #550: `div.axis_start`/`.axis_size` are already absolute
            // terminal-screen coordinates, so `col`/`row` compare directly
            // with no editor-origin subtraction.
            if let Some(grab) = *divider_grab {
                if let Some(layout) = last_layout {
                    render::apply_divider_drag(
                        engine,
                        grab,
                        &layout.group_dividers,
                        &layout.window_dividers,
                        col as f64,
                        row as f64,
                    );
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
            // Tab drag-and-drop: execute drop on release (#753 — the same
            // `handle_release` GTK calls; it also clears any armed-but-never-
            // dragged press, which is what the bare `tab_drag_start = None`
            // this replaced was for).
            if tab_drag.handle_release(engine) {
                return sidebar_width;
            }
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
            *divider_grab = None;
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
                if layout.editor_group_split.is_some() {
                    let click_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
                    for gtb in layout.group_tab_bars.iter() {
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

    // The context-menu click intercept and hover rungs that used to sit here —
    // ~110 lines, arbitrated *below* the picker and find/replace even though
    // the menu paints above both — are now
    // `render::ModalOverlayRoute::ContextMenu`, resolved at the top of this
    // function and shared verbatim with GTK's `dispatch_context_menu_click`
    // (#751). Opening a menu on right-click stays below: which menu opens is a
    // question about explorer rows / tab-bar geometry / editor cells, not
    // about the menu.

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
                let rel_col = col - editor_left;

                if layout.editor_group_split.is_some() {
                    let click_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
                    for gtb in layout.group_tab_bars.iter() {
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

    // ── Chrome rung (#752) ────────────────────────────────────────────────────
    //
    // Breadcrumbs → status bands → global status bar, sequenced ONCE in
    // `render::route_chrome_click` and shared verbatim with GTK's
    // `handle_mouse_click_msg`. Four arms used to be transcribed here and
    // scattered down the ladder — see `route_and_apply_chrome_click` below for
    // where each one lived and what it had drifted into.
    //
    // Placed here — above the command line, below the modal band — because
    // every band it arbitrates is spatially disjoint from the rungs it now
    // jumps ahead of (scroll surfaces, the activity bar, the sidebar panel,
    // the editor area), so the move is a re-ordering of checks that cannot
    // both match. The one place two bands genuinely overlap — a `:split`
    // divider's grab row and the status line that marks it (#582) — is
    // arbitrated inside the shared router, not by this ordering.
    if route_and_apply_chrome_click(
        ev,
        engine,
        last_layout,
        terminal_size,
        ChromeGeometry {
            editor_left,
            term_height,
            bottom_chrome,
            sep_status_rows,
        },
    ) {
        return sidebar_width;
    }

    // ── Command line click — start text selection ──────────────────────────────
    // Skip when click is in the activity bar column (settings button lives there).
    //
    // #752 verdict, recorded rather than converged: this rung stays one-sided.
    // Sharing it would mean *adding* a command-line text-selection
    // implementation to GTK, and GTK has no `cmd_sel`/`cmd_dragging` state, no
    // inverted-cell read-back pass (`render_command_line`'s `cmd_sel`
    // argument), and paints its command line through `Surface::CommandLine`,
    // which exposes no character-offset hit test. That is a quadraui gap, not
    // a vimcode transcription: per `CLAUDE.md`'s Platform-Neutrality Rule the
    // fix is a `CommandLineLayout::hit_test` in quadraui, then one shared rung
    // here — not ~80 lines of new GTK-specific selection code. Left as-is.
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

    // #752: the global status bar row was swallowed here with a
    // `// no interactive segments` comment. It has one — the git branch, which
    // GTK routed and TUI did not — and it is now the global-status-bar rung of
    // `render::route_chrome_click` above, hit-testing the rect the frame
    // actually painted.

    // Bottom row is cmd — ignore (but not in the activity bar column)
    if row + 1 >= term_height && col >= ab_width {
        return sidebar_width;
    }

    // ── Menu bar row click — command center only ──────────────────────────────
    // Menu bar item clicks and dropdown clicks are handled by
    // MenuSystem::handle() in the UiEvent intercept (mod.rs).
    // The command center (nav arrows + search box) is still separate.
    //
    // #695: `row == engine.menu_bar_rect.get().y` instead of a hardcoded
    // `row == 0` — the bar's row today is always the shell's top row (`y ==
    // 0`), but that's a fact about the current layout, not a guarantee this
    // call site should hardcode; reading it from the same cache paint wrote
    // keeps this arm correct if that ever changes, for free.
    let menu_bar_row = engine.menu_bar_rect.get().y.round() as u16;
    if engine.menu_bar_visible && menu_rows > 0 && row == menu_bar_row {
        let cc_hit = engine
            .command_center_layout
            .borrow()
            .as_ref()
            .map(|l| l.hit_test(col as f32, row as f32 + 0.5));
        // Shared with GTK's identical match arm as `render::apply_command_center_hit`
        // (#752).
        if let Some(hit) = cc_hit {
            if crate::render::apply_command_center_hit(engine, hit) {
                return sidebar_width;
            }
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
    // #752: the separated status line's arm lived here, with its own copy of
    // the `handle_status_action` → `OpenTerminal` follow-up. It is now the
    // first status band `render::route_chrome_click` walks (above), and the
    // follow-up is `render::apply_status_action`.

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
    // and editor content down by `menu_rows` (computed once, near the top of
    // this function).

    // #752: the breadcrumb arm lived here. It is now the first rung of
    // `render::route_chrome_click`, called near the top of this function — the
    // same router GTK's `handle_mouse_click_msg` calls.

    // ── Tab bar click ──────────────────────────────────────────────────────
    // For split groups, any group's tab bar row is clickable (not just the top row).
    if let Some(layout) = last_layout {
        // #752: no `rel_col = col - editor_left` here any more — *both* arms
        // below now measure against the painted `GroupTabBar::bounds.x`, so
        // neither needs the live sidebar-derived origin (nor can underflow on
        // it when the sidebar's visibility changed earlier in this same click).
        if let Some(ref split) = layout.editor_group_split {
            // Find which group's tab bar row matches the clicked row.
            // Tab bar sits tab_bar_height rows above the group's window content.
            let click_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
            let mut matched_group = None;
            for gtb in layout.group_tab_bars.iter() {
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
                let hit_target = layout
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
                            tab_drag.arm(col as f64, row as f64);
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
        // Single group: check the active group's painted tab-bar row.
        //
        // #752: the row *and* the left edge come from `gtb.bounds` — the rect
        // this frame actually painted — instead of the live
        // `menu_rows`/`editor_left` re-derivation this arm used to do. Same
        // reasoning as #695 moving `menu_rows` onto the painted
        // `menu_bar_rect` cache: a click can *change* the state those are
        // derived from before this hit test runs. Concretely, with the
        // hamburger "Menu" panel open in the sidebar, the click that lands on
        // a tab first dismisses that panel; when no panel takes its place the
        // sidebar collapses, `sb_visible` flips false and `editor_left` drops
        // by the sidebar's whole width — so `rel_col` pointed ~31 columns
        // right of the tab the user clicked and the switch was silently
        // swallowed (the user had to click the tab twice). The split-group
        // branch above has hit-tested against absolute painted bounds since
        // #550; this arm now does too, so both read one geometry.
        let single_group_bar = if layout.editor_group_split.is_none()
            && !engine.is_tab_bar_hidden(engine.active_group)
        {
            layout
                .group_tab_bars
                .iter()
                .find(|gtb| gtb.group_id == engine.active_group)
        } else {
            None
        };
        // The tab bar sits `click_tbh` rows above the group's window content
        // (2 with breadcrumbs, 1 without) — the same offset the split branch
        // above applies.
        let single_tab_bar_row = single_group_bar.map(|gtb| {
            let click_tbh: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
            (gtb.bounds.y as u16).saturating_sub(click_tbh)
        });
        if let (Some(gtb), Some(tab_bar_row)) = (single_group_bar, single_tab_bar_row) {
            let gx = gtb.bounds.x as u16;
            let gw = gtb.bounds.width as u16;
            if row == tab_bar_row && col >= gx && col < gx + gw {
                // #654: the last hand-rolled tab geometry in the TUI — this used
                // to rebuild the `TabBar` primitive and re-measure every tab with
                // `name.chars().count() + TAB_CLOSE_COLS` before calling
                // `hit_test`. The split-group branch above, the tooltip lookup and
                // the drag-slot map all already read `hit_regions`, so this now
                // does too: one geometry, computed once in `build_screen_layout`.
                let local_col = col - gx;
                // #752: this used to re-implement `Engine::handle_tab_bar_click`
                // arm by arm — the split-group branch a few lines above already
                // delegated to it, and GTK's `dispatch_tab_bar_target` did too, so
                // this was the last hand-rolled copy of a dispatch the engine has
                // owned all along. The copy had drifted: it never set
                // `active_group` and never called `lsp_ensure_active_buffer()`,
                // so clicking a tab in an *unsplit* window left the LSP pointed at
                // the previously-active buffer, while doing the same in a split
                // did not.
                let group_id = engine.active_group;
                // Per-group `hit_regions`, not `layout.tab_bar_hit_regions`: both
                // are computed from the same tabs, scroll offset and bounding-box
                // width, but only the per-group one is paired with the `bounds`
                // this arm now measures `local_col` against (#735's audit note on
                // `ScreenLayout::tab_bar_hit_regions` called this arm out as its
                // last remaining reader).
                match render::resolve_tab_bar_click(&gtb.hit_regions, local_col) {
                    Some(TabBarClickTarget::ActionMenu) => {
                        // Needs screen coordinates, so the engine's own arm is a
                        // deliberate no-op (see `handle_tab_bar_click`). #434:
                        // pass the tab-row height (1.0 row in TUI) so the engine
                        // drives `Below` placement.
                        engine.active_group = group_id;
                        engine.open_editor_action_menu(group_id, col, row, 1.0);
                    }
                    Some(target) => {
                        let is_tab = matches!(target, TabBarClickTarget::Tab(_));
                        if engine.handle_tab_bar_click(group_id, target) {
                            engine.show_close_tab_confirm();
                        } else if is_tab {
                            tab_drag.arm(col as f64, row as f64);
                        }
                    }
                    None => {}
                }
                return sidebar_width;
            }
        }
    }

    // #550: `rw.rect`/`div.position`/`.cross_start` etc. below are all
    // already absolute terminal-screen coordinates, so the raw event
    // `col`/`row` are used directly throughout this block — no
    // editor-area-relative translation needed.

    // ── Divider click — start drag (#753 shared rung) ─────────────────────────
    // Group boundaries then `:split`/`:vsplit` boundaries, sequenced by
    // `render::route_divider_grab`. Only the tolerances below are TUI-specific:
    // they describe what *this* rasteriser drew, and the full rationale for the
    // shape of `DividerMetrics` lives on that type. In short:
    //
    // * `quantize: true` (#452) — hit-test against the same `position as u16`
    //   truncation `render_impl.rs::draw_frame` draws at.
    // * `tol_before = 1` on every *vertical* band and on the window bands
    //   (#582, and #753 for the group one) — `div.position` is the first
    //   column/row of the *second* pane and carries no glyph; the visible mark
    //   is one cell before it (a neighbouring separator/scrollbar column for
    //   `:vsplit`, the upper window's status row for `:split`, the group
    //   divider glyph for a group split). Reaching back one cell is what makes
    //   the thing the user aims at grabbable.
    // * `group_horizontal: (0.0, tab_bar_rows)` — here the visible divider *is*
    //   the lower group's whole tab-bar block (render_impl.rs:359), 2 rows tall
    //   with breadcrumbs on, so the band reaches forward instead of back; the
    //   row before it is the upper group's own status line and must keep its
    //   clicks. #551: no `editor_group_split.is_some()` gate is needed —
    //   `group_dividers` is empty with one group, so nothing can match.
    if let Some(layout) = last_layout {
        let tab_bar_rows: u16 = if engine.settings.breadcrumbs { 2 } else { 1 };
        if let Some(grab) = render::route_divider_grab(
            &render::DividerState {
                group_dividers: &layout.group_dividers,
                window_dividers: &layout.window_dividers,
                metrics: render::DividerMetrics {
                    group_vertical: (1.0, 1.0),
                    group_horizontal: (0.0, tab_bar_rows as f64),
                    window_vertical: (1.0, 1.0),
                    window_horizontal: (1.0, 1.0),
                    quantize: true,
                },
                on_tab_bar: false,
            },
            col as f64,
            row as f64,
        ) {
            *divider_grab = Some(grab);
            return sidebar_width;
        }
    }

    // ── Minimap click / drag (#35) ──────────────────────────────────────────
    // Pure rect plumbing: hand the click's cell coordinates to the shared
    // resolver, which owns the hit-test and the scroll. Drag keeps seeking
    // while the button is held, matching the GTK side.
    //
    // Deliberately *after* both divider hit-tests: the strip is carved off
    // the active window's right edge, so in a `:vsplit` it abuts (and would
    // otherwise swallow) the divider's grab column — and a divider drag is
    // the more destructive gesture to lose.
    if matches!(
        ev.kind,
        MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left)
    ) {
        if let Some(layout) = last_layout {
            if render::apply_minimap_click(engine, layout, col as f64, row as f64).is_some() {
                return sidebar_width;
            }
        }
    }

    if let Some(layout) = last_layout {
        for rw in &layout.windows {
            let wx = rw.rect.x as u16;
            let wy = rw.rect.y as u16;
            let ww = rw.rect.width as u16;
            let wh = rw.rect.height as u16;

            if col >= wx && col < wx + ww && row >= wy && row < wy + wh {
                // #752: the per-window status bar's arm was here — a third
                // copy of the same `handle_status_action` → `OpenTerminal`
                // follow-up, buried inside this window walk where GTK's
                // equivalent sits in a flat ladder. Every window's bar is now
                // a `render::StatusBand` fed to `render::route_chrome_click`
                // above, so control only reaches this walk when the click was
                // *not* on a status row.

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

/// The bits of `handle_mouse`'s own layout arithmetic the chrome rung needs to
/// place the separated status line, which is the one band with no cached rect
/// of its own.
struct ChromeGeometry {
    editor_left: u16,
    term_height: u16,
    bottom_chrome: u16,
    sep_status_rows: u16,
}

/// Assemble this backend's [`render::ChromeState`] in character cells, run the
/// shared chrome rung over it, and apply whatever it decides. Returns `true`
/// when the event was consumed.
///
/// The TUI twin of `gtk::App::route_and_apply_chrome_click`. #752 folded four
/// arms into this one call:
///
///  * the **breadcrumb** arm, ~700 lines down `handle_mouse`;
///  * the **separated status line**, with its own copy of the
///    `handle_status_action` → `OpenTerminal` follow-up;
///  * the **per-window status line**, a third copy of that follow-up, buried
///    another ~250 lines below inside the window walk;
///  * the **global status bar** row, which was not routed at all — swallowed
///    by `if row + 2 == term_height { return }` under a
///    `// no interactive segments` comment that had stopped being true.
///
/// Unlike GTK, which caches zones at paint time, TUI re-derives each bar's
/// layout here on every click — exactly as `render_window_status_line` derives
/// it on every paint, and exactly as the deleted `status_segment_hit_test`
/// did. Same measure function, same `min_gap`, so hit and paint agree.
fn route_and_apply_chrome_click(
    ev: MouseEvent,
    engine: &mut Engine,
    last_layout: Option<&render::ScreenLayout>,
    terminal_size: &Option<Size>,
    geom: ChromeGeometry,
) -> bool {
    let (col, row) = (ev.column, ev.row);
    let mut zone_store: Vec<(quadraui::Rect, render::StatusZones)> = Vec::new();
    if let Some(layout) = last_layout {
        let bar_width = terminal_size.map(|s| s.width).unwrap_or(80) as usize;
        // The separated status line first, for the same reason GTK lists it
        // first: it paints in its own full-width band *outside* every window's
        // rect, so a click there must not fall through to what sits under it.
        if let Some(status) = &layout.separated_status_line {
            let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
            let strip_rows: u16 = if engine.terminal_open {
                super::effective_terminal_panel_rows_tui(engine, geom.term_height) + 1
            } else {
                0
            };
            let term_strip_top = geom
                .term_height
                .saturating_sub(geom.bottom_chrome + qf_rows + strip_rows);
            let sep_row = term_strip_top.saturating_sub(geom.sep_status_rows);
            zone_store.push((
                quadraui::Rect::new(
                    geom.editor_left as f32,
                    sep_row as f32,
                    bar_width.saturating_sub(geom.editor_left as usize) as f32,
                    1.0,
                ),
                render::window_status_line_zones(status, bar_width),
            ));
        }
        // Each window's own status line occupies its bottom row — the same row
        // `render_window` subtracts before computing viewport geometry.
        for rw in &layout.windows {
            let (Some(status), true) = (&rw.status_line, rw.rect.height > 1.0) else {
                continue;
            };
            zone_store.push((
                quadraui::Rect::new(
                    rw.rect.x as f32,
                    (rw.rect.y + rw.rect.height - 1.0) as f32,
                    rw.rect.width as f32,
                    1.0,
                ),
                render::window_status_line_zones(status, rw.rect.width as usize),
            ));
        }
        // The global bar last, spatially and in arbitration: it is the shell's
        // own bottom band, below every window. Its rect is the one the paint
        // path published (#752), not a re-derived `term_height - 2`.
        let global_rect = engine.global_status_rect.get();
        if let (Some(bar), true) = (
            layout.global_status_bar.as_ref(),
            global_rect.width > 0.0 && global_rect.height > 0.0,
        ) {
            zone_store.push((
                global_rect,
                render::status_bar_zones_in_cells(bar, global_rect.width as usize),
            ));
        }
    }
    let bands: Vec<render::StatusBand<'_>> = zone_store
        .iter()
        .map(|(rect, zones)| render::StatusBand { rect: *rect, zones })
        .collect();

    let empty_breadcrumbs: [render::BreadcrumbBar; 0] = [];
    let route = render::route_chrome_click(
        &render::ChromeState {
            breadcrumbs_enabled: engine.settings.breadcrumbs,
            breadcrumbs: last_layout
                .map(|l| l.breadcrumbs.as_slice())
                .unwrap_or(&empty_breadcrumbs),
            // TUI measures in whole cells, so one row *is* the unit.
            line_height: 1.0,
            status_bands: &bands,
            // The same shared hit test, with the same tolerances, the
            // window-divider rung in `handle_mouse` runs — see
            // `render::ChromeState::on_window_divider` (#582/#752).
            on_window_divider: last_layout.is_some_and(|l| {
                render::divider_hit_test(
                    &l.window_dividers,
                    col as f64,
                    row as f64,
                    (1.0, 1.0),
                    (1.0, 1.0),
                    true,
                )
                .is_some()
            }),
        },
        if matches!(ev.kind, MouseEventKind::Down(MouseButton::Left)) {
            render::ChromeMouseAction::LeftPress
        } else {
            render::ChromeMouseAction::Other
        },
        col as f64,
        row as f64,
    );
    match route {
        render::ChromeRoute::None => false,
        render::ChromeRoute::Breadcrumb { group_id, idx } => {
            engine.handle_breadcrumb_click(group_id, idx);
            true
        }
        render::ChromeRoute::StatusAction(action) => {
            let cols = terminal_size.as_ref().map(|s| s.width).unwrap_or(80);
            render::apply_status_action(engine, &action, cols);
            true
        }
        render::ChromeRoute::BreadcrumbBar | render::ChromeRoute::StatusBar => true,
    }
}

// #752: `status_segment_hit_test` lived here — it built the `StatusBar`
// primitive, laid it out and hit-tested a single column, and its three callers
// each wrapped it in their own copy of the `handle_status_action` follow-up.
// It is now `render::window_status_line_zones`, which returns *zones* rather
// than answering one column, so the shared `render::route_chrome_click` can
// treat a TUI status line and a GTK one as the same `render::StatusBand`.

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
            &mut render::TabDragState::default(),
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
        // #695: `handle_mouse`'s `menu_rows` now reads `engine.menu_bar_rect`
        // (the single source of truth both paint and hit-test consume in
        // the live `TuiShellApp::render_content` pipeline) rather than
        // re-deriving a row count from `menu_bar_visible` alone. This
        // hermetic fixture drives `handle_mouse` directly, bypassing
        // `render_content`, so it must seed the same cache `render_content`
        // would have populated by now — otherwise `menu_rows` reads back 0
        // even though `menu_bar_visible` is true, defeating this fixture's
        // whole point (maximizing the editor-area offset).
        e.menu_bar_rect
            .set(quadraui::Rect::new(0.0, 0.0, 120.0, 1.0));
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
        divider_grab: &mut Option<render::DividerGrab>,
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
            divider_grab,
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
            &mut render::TabDragState::default(),
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
        assert!(
            screen.editor_group_split.is_some(),
            "vertical split must produce Some(editor_group_split)"
        );
        let div = screen
            .group_dividers
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
        let mut grab = None;
        dispatch_left_click(&mut engine_hit, col, row, Some(&screen), &mut grab);
        assert_eq!(
            grab,
            Some(render::DividerGrab::Group {
                split_index: div.split_index
            }),
            "click at the painted divider column ({col}, {row}) must start the divider drag"
        );

        // `col - 1` is where `render_group_dividers` actually paints the
        // visible │ — `div.position` is the first column of the *second*
        // group and carries no glyph of its own. #753 widened `tol_before` to
        // 1 so the column the user can see is grabbable too; before that, the
        // only grabbable column was the invisible one (the #582 off-by-one,
        // fixed for window dividers then and for group dividers now).
        let mut engine_glyph = split_engine_with_sidebar_and_menu();
        let mut grab_glyph = None;
        dispatch_left_click(
            &mut engine_glyph,
            col - 1,
            row,
            Some(&screen),
            &mut grab_glyph,
        );
        assert_eq!(
            grab_glyph,
            Some(render::DividerGrab::Group {
                split_index: div.split_index
            }),
            "click on the visible divider glyph ({}, {row}), one column before \
             the boundary, must also start the drag",
            col - 1
        );

        // Two columns off must NOT hit the divider.
        let mut engine_miss = split_engine_with_sidebar_and_menu();
        let mut grab_miss = None;
        dispatch_left_click(
            &mut engine_miss,
            col - 2,
            row,
            Some(&screen),
            &mut grab_miss,
        );
        assert_eq!(
            grab_miss, None,
            "click two columns off the divider must not start a drag"
        );
    }

    fn vsplit_engine_with_sidebar_and_menu() -> Engine {
        let mut e = Engine::new();
        e.settings = crate::core::settings::Settings::default();
        e.mode = crate::core::Mode::Normal;
        e.menu_bar_visible = true;
        // #695: `handle_mouse`'s `menu_rows` now reads `engine.menu_bar_rect`
        // (the single source of truth both paint and hit-test consume in
        // the live `TuiShellApp::render_content` pipeline) rather than
        // re-deriving a row count from `menu_bar_visible` alone. This
        // hermetic fixture drives `handle_mouse` directly, bypassing
        // `render_content`, so it must seed the same cache `render_content`
        // would have populated by now — otherwise `menu_rows` reads back 0
        // even though `menu_bar_visible` is true, defeating this fixture's
        // whole point (maximizing the editor-area offset).
        e.menu_bar_rect
            .set(quadraui::Rect::new(0.0, 0.0, 120.0, 1.0));
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
        let mut grab = None;
        dispatch_left_click(&mut engine_hit, col, row, Some(&screen), &mut grab);
        assert_eq!(
            grab,
            Some(render::DividerGrab::Window {
                group_id,
                split_index
            }),
            "click at the painted window-divider column ({col}, {row}) must start the drag"
        );

        // `col - 1` is where `render_separators` actually paints the visible
        // mark (the left window's own separator/scrollbar column, one cell
        // before the boundary at `col`) — the hit-test tolerance deliberately
        // covers it too (#582 smoke-test fix: clicking the glyph the user can
        // actually see used to miss ~half the time). Two columns off must
        // still miss.
        let mut engine_hit_neighbor = vsplit_engine_with_sidebar_and_menu();
        let mut grab_neighbor = None;
        dispatch_left_click(
            &mut engine_hit_neighbor,
            col - 1,
            row,
            Some(&screen),
            &mut grab_neighbor,
        );
        assert_eq!(
            grab_neighbor,
            Some(render::DividerGrab::Window {
                group_id,
                split_index
            }),
            "click on the visible glyph column ({}, {row}), one before the boundary, must also start the drag",
            col - 1
        );

        // Two columns off must NOT hit the divider.
        let mut engine_miss = vsplit_engine_with_sidebar_and_menu();
        let mut grab_miss = None;
        dispatch_left_click(
            &mut engine_miss,
            col - 2,
            row,
            Some(&screen),
            &mut grab_miss,
        );
        assert_eq!(
            grab_miss, None,
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
            &mut grab,
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
            &mut render::TabDragState::default(),
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
        // #695: `handle_mouse`'s `menu_rows` now reads `engine.menu_bar_rect`
        // (the single source of truth both paint and hit-test consume in
        // the live `TuiShellApp::render_content` pipeline) rather than
        // re-deriving a row count from `menu_bar_visible` alone. This
        // hermetic fixture drives `handle_mouse` directly, bypassing
        // `render_content`, so it must seed the same cache `render_content`
        // would have populated by now — otherwise `menu_rows` reads back 0
        // even though `menu_bar_visible` is true, defeating this fixture's
        // whole point (maximizing the editor-area offset).
        e.menu_bar_rect
            .set(quadraui::Rect::new(0.0, 0.0, 120.0, 1.0));
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
        let mut grab = None;
        dispatch_left_click(&mut engine_hit, col, row - 1, Some(&screen), &mut grab);
        assert_eq!(
            grab,
            Some(render::DividerGrab::Window {
                group_id,
                split_index
            }),
            "click on the status-line row ({col}, {}), one row above the boundary, must start the drag",
            row - 1
        );

        // Two rows off must NOT hit the divider.
        let mut engine_miss = hsplit_engine_with_sidebar_and_menu();
        let mut grab_miss = None;
        dispatch_left_click(
            &mut engine_miss,
            col,
            row - 2,
            Some(&screen),
            &mut grab_miss,
        );
        assert_eq!(
            grab_miss, None,
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
            &mut grab,
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
            &mut render::TabDragState::default(),
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
        assert!(
            screen.editor_group_split.is_some(),
            "vertical split must produce Some(editor_group_split)"
        );
        let gtb = screen
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
            &mut render::TabDragState::default(),
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
            None,
        );

        assert!(
            engine.context_menu.is_some(),
            "right-click at the painted tab-bar position ({col}, {tab_bar_row}) must open the tab context menu"
        );
    }
}
