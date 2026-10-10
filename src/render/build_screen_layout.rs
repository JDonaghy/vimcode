use super::*;

// ─── build_screen_layout ──────────────────────────────────────────────────────

/// Build a complete `ScreenLayout` from current engine state.
///
/// # Parameters
/// - `engine` — current editor state (no GTK types)
/// - `theme` — colour scheme
/// - `window_rects` — pixel-space rects for each window in the current tab,
///   as returned by `engine.calculate_group_window_rects()`
/// - `line_height` — pixel height of one text line (from Pango font metrics)
/// - `char_width` — pixel width of one character (from Pango font metrics),
///   used to compute gutter width
/// - `scrollbar_reserve` — width, in the caller's own unit, the *caller's*
///   backend reserves for its native scrollbar overlay alongside the
///   editor's text (#828/quadraui#776: `quadraui::Backend::scrollbar_reserve()`
///   — `0.0` for TUI, GTK's `ScrolledWindow` overlay's own reserve for GTK).
///   This function only subtracts it from the viewport-column computation;
///   it never decides what the value is, so it carries no backend identity
///   of its own.
/// - `minimap_sizing` — the caller's own [`TUI_MINIMAP_SIZING`] or
///   [`gtk_minimap_sizing`] (#828/quadraui#776), forwarded verbatim to
///   `minimap_reserved_width` — again supplied, not decided, here.
///
/// This function is intentionally *pure* — no side effects, no GTK/Cairo calls.
#[allow(clippy::too_many_arguments)]
pub fn build_screen_layout(
    engine: &Engine,
    theme: &Theme,
    window_rects: &[(WindowId, WindowRect)],
    line_height: f64,
    char_width: f64,
    color_headings: bool,
    scrollbar_reserve: f64,
    minimap_sizing: quadraui::MinimapSizing,
) -> ScreenLayout {
    // Breadcrumb row height defaults to `line_height` — correct for TUI's
    // row-based model (`line_height` is always `1.0`, i.e. exactly one row)
    // and for any GTK call site that hasn't opted into the fixed-pixel
    // breadcrumb row via `build_screen_layout_with_breadcrumb_row` (#700).
    build_screen_layout_with_breadcrumb_row(
        engine,
        theme,
        window_rects,
        line_height,
        char_width,
        color_headings,
        line_height,
        scrollbar_reserve,
        minimap_sizing,
    )
}

/// Like [`build_screen_layout`], but lets the caller decouple the breadcrumb
/// bar's own height/position from `line_height` (#700).
///
/// GTK's tab-bar and breadcrumb rows are fixed-pixel chrome
/// (`tab_row_height_px`/`BREADCRUMB_ROW_HEIGHT_PX`) independent of
/// `settings.font_size` — raising the editor font must not inflate the
/// breadcrumb row along with it. The space reserved above the window
/// content (`window_rects`, computed by the caller via
/// `calculate_group_window_rects` using the same fixed row heights) already
/// reflects that; this variant makes the breadcrumb bar's own painted
/// `bounds` agree, instead of assuming (as plain `line_height` would) that
/// the reserved breadcrumb space is exactly one editor text line tall.
#[allow(clippy::too_many_arguments)]
pub fn build_screen_layout_with_breadcrumb_row(
    engine: &Engine,
    theme: &Theme,
    window_rects: &[(WindowId, WindowRect)],
    line_height: f64,
    char_width: f64,
    color_headings: bool,
    breadcrumb_row_h: f64,
    scrollbar_reserve: f64,
    minimap_sizing: quadraui::MinimapSizing,
) -> ScreenLayout {
    let active_window_id = engine.active_window_id();
    let multi_window = engine.windows.len() > 1;

    let tab_bar = build_tab_bar(engine);

    let per_window_status = effective_window_status_line(engine);
    let bottom_panel_open = engine.terminal_open || engine.bottom_panel_open;
    // When status_line_above_terminal is OFF and the terminal is open, extract the
    // active window's status into a separated bar rendered above the terminal.
    // When the setting is ON (default), per-window status bars stay inside each
    // window — they're naturally above the terminal by being part of the editor area.
    let separate_status =
        per_window_status && !engine.settings.status_line_above_terminal && bottom_panel_open;
    // Single source of truth for "does this window paint its own bottom-row
    // status line" (#728). #1128: GTK's h-scrollbar geometry no longer
    // consults this — see `window_status_row_reserved`'s doc for why.
    let own_status_row = window_status_row_reserved(engine);

    // Window-split dividers (#582) — independent of the `n >= 2` editor-group
    // check below, since `:split`/`:vsplit` panes exist within a single group.
    let window_dividers = engine.calculate_window_dividers(window_rects);

    // Minimap strip (#35, #722). Reserved off *every* window's right edge —
    // not just the active one — and subtracted from that same window's text
    // width before it's laid out, so each pane reclaims exactly its own
    // strip's width when `:set nominimap` turns the strip off. Keyed per
    // window (rather than a single scalar) because `minimap_reserved_width`
    // is a function of that window's own rect width, so unevenly split
    // panes legitimately get differently-sized strips.
    //
    // #1094: VS Code's order is text, then the strip, then the scroll
    // column at the pane's outermost edge — but both backends' scrollbars
    // anchor to *their own painted rect's* right edge (quadraui's TUI
    // `draw_editor` always reserves one inline column at `area.right - 1`
    // when the window overflows; a future GTK native scrollbar would do the
    // same at its own widget's right edge, per the doc comment on the
    // `Surface::Editor` push in `app.rs`). The only way to make that edge
    // land past the strip rather than immediately before it is for the rect
    // this module hands to the paint path (`RenderedWindow.rect`, below) to
    // reach the pane's *true* right edge — not a copy narrowed by the
    // strip's width, as it used to be. The strip itself is then positioned
    // in the gap that opens up between the (now-narrower) text and that
    // rect's edge — see `scroll_gutter_width` and the strip's own `x` below.
    //
    // Narrowing the affordability check the same way the strip's own
    // position now is: `minimap_reserved_width`'s own budget only checks
    // against the *pane's* width, with no notion of the scroll gutter now
    // sitting beyond the strip — a narrow pane could otherwise reserve both
    // and leave less than `MINIMAP_MIN_TEXT_COLS` for the text between them.
    // Suppressing the strip (falling back to the width it already returns
    // for "off") is the same self-suppression behaviour a pane too narrow
    // to afford the strip alone already has.
    let minimap_widths: std::collections::HashMap<WindowId, f64> = window_rects
        .iter()
        .map(|(id, r)| {
            let gutter_px = window_minimap_gutter_width_px(engine, *id, char_width);
            let raw =
                minimap_reserved_width(engine, r.width, char_width, minimap_sizing, gutter_px);
            let w = if raw > 0.0 {
                let cw = if char_width > 0.0 { char_width } else { 1.0 };
                let gutter = scroll_gutter_width(scrollbar_reserve, char_width);
                if r.width - raw - gutter < MINIMAP_MIN_TEXT_COLS * cw {
                    0.0
                } else {
                    raw
                }
            } else {
                0.0
            };
            (*id, w)
        })
        .collect();

    let windows = window_rects
        .iter()
        .map(|(window_id, rect)| {
            let mut visible_lines = (rect.height / line_height).floor() as usize;
            if own_status_row && visible_lines > 1 {
                visible_lines -= 1; // reserve bottom row for per-window status bar
            }
            let is_active = *window_id == active_window_id;
            let raw_minimap_w = minimap_widths.get(window_id).copied().unwrap_or(0.0);
            // `rect` reaches the pane's true right edge unmodified (#1094,
            // see the doc comment above) — `build_rendered_window` takes a
            // `minimap_w` to keep the text-column count excluding the strip
            // without narrowing the rect it's painted into.
            //
            // The strip's own `x` (below) sits a full `scroll_gutter_width`
            // in from that edge, not just `raw_minimap_w` — so when a strip
            // is actually present, the text has to give up that same extra
            // sliver too, or its last column and the strip's first column
            // would coincide (whichever paints later, the strip, would
            // silently eat the text's own last character on a long enough
            // line). No-op when there's no strip (`raw_minimap_w == 0.0`,
            // off or self-suppressed) — nothing to leave room *for* then,
            // and reserving it anyway would cost the minimap-off case a
            // column it doesn't owe (acceptance criterion 2).
            let minimap_w = if raw_minimap_w > 0.0 {
                raw_minimap_w
                    + (scroll_gutter_width(scrollbar_reserve, char_width) - scrollbar_reserve)
            } else {
                0.0
            };
            let mut rw = build_rendered_window(
                engine,
                theme,
                *window_id,
                rect,
                visible_lines,
                char_width,
                is_active,
                multi_window,
                color_headings,
                scrollbar_reserve,
                minimap_w,
            );
            if own_status_row {
                rw.status_line = Some(build_window_status_line(
                    engine, theme, *window_id, is_active,
                ));
            }
            engine
                .paint_viewport_cols
                .borrow_mut()
                .insert(*window_id, rw.text_viewport_cols);
            rw
        })
        .collect();

    // The strips themselves, minus the per-window status row when one is
    // painted inside that window. One `RenderedMinimap` per window that has
    // a strip, in `window_rects` order — a `:vsplit` therefore carries two
    // independent strips, each over its own pane's buffer, instead of one
    // that migrates with focus.
    //
    // #1094: positioned one `scroll_gutter_width` in from the pane's right
    // edge rather than flush against it, so the scroll column painted at
    // that edge (see the doc comment above `minimap_widths`) lands outside
    // the strip instead of colliding with its last column.
    let minimap: Vec<RenderedMinimap> = window_rects
        .iter()
        .filter_map(|(id, r)| {
            let minimap_w = minimap_widths.get(id).copied().unwrap_or(0.0);
            if minimap_w <= 0.0 {
                return None;
            }
            let status_h = if own_status_row && r.height > line_height {
                line_height
            } else {
                0.0
            };
            // The editor pane's own visible row count — the same
            // computation the `windows` map above runs over the same
            // `window_rects` entry — handed to `build_minimap_data`
            // separately from the strip's own `rect`/`line_height` (#1085:
            // see that function's doc comment for why these must not be
            // conflated).
            let mut editor_visible_rows = (r.height / line_height).floor() as usize;
            if own_status_row && editor_visible_rows > 1 {
                editor_visible_rows -= 1;
            }
            let gutter = scroll_gutter_width(scrollbar_reserve, char_width);
            build_minimap_data(
                engine,
                theme,
                *id,
                WindowRect::new(
                    r.x + r.width - gutter - minimap_w,
                    r.y,
                    minimap_w,
                    (r.height - status_h).max(0.0),
                ),
                line_height,
                editor_visible_rows,
            )
        })
        .collect();

    let separated_status_line = if separate_status {
        Some(build_window_status_line(
            engine,
            theme,
            active_window_id,
            true,
        ))
    } else {
        None
    };

    let global_status_bar = if global_status_bar_visible(engine) {
        Some(build_global_status_bar(engine, theme))
    } else {
        None
    };
    let command = build_command_line(engine);

    let wildmenu = if engine.wildmenu_items.is_empty() {
        None
    } else {
        // For argument completions (e.g. "set wrap"), display only the last word
        let display_items: Vec<String> = engine
            .wildmenu_items
            .iter()
            .map(|item| {
                item.rsplit_once(' ')
                    .map(|(_, arg)| arg.to_string())
                    .unwrap_or_else(|| item.clone())
            })
            .collect();
        Some(WildmenuData {
            items: display_items,
            selected: engine.wildmenu_selected,
        })
    };

    let completion = engine.completion_idx.map(|idx| {
        let max_width = engine
            .completion_candidates
            .iter()
            .map(|c| c.label.chars().count())
            .max()
            .unwrap_or(0);
        CompletionMenu {
            candidates: engine.completion_candidates.clone(),
            selected_idx: idx,
            max_width,
        }
    });

    let hover = engine.lsp_hover_text.as_ref().map(|text| HoverPopup {
        text: text.clone(),
        anchor_line: engine.view().cursor.line,
        anchor_col: engine.view().cursor.col,
    });

    // The global quickfix list takes priority over the active window's
    // location list when both happen to be open — matching how `:copen`
    // and `:lopen` share this one bottom "list rung" (#1155;
    // `QuickfixPanel::title` doc comment has the full rationale).
    //
    // #1307: this overlay is superseded for any target that already has a
    // real `WindowLayout` leaf — that leaf paints through the ordinary
    // per-window content path (`windows`, below) like any other window, so
    // painting it *again* here would double it up. `qf_has_real_window` is
    // `false` for every caller that still drives `open`/`items` directly
    // without going through `qf_open` (most of this codebase's own
    // rendering tests, and `qf_set_list`'s implicit `:grep` auto-open), so
    // this stays exactly as before for them.
    let quickfix = if engine.qf_has_real_window(None)
        || engine.qf_has_real_window(Some(engine.active_window_id()))
    {
        None
    } else if engine.quickfix.open && !engine.quickfix.items.is_empty() {
        Some(quickfix_list_to_panel(&engine.quickfix, "QUICKFIX"))
    } else {
        engine
            .location_lists
            .get(&engine.active_window_id())
            .filter(|l| l.open && !l.items.is_empty())
            .map(|l| quickfix_list_to_panel(l, "LOCATION LIST"))
    };

    let signature_help = engine
        .lsp_signature_help
        .as_ref()
        .map(|sh: &SignatureHelpData| SignatureHelp {
            label: sh.label.clone(),
            params: sh.params.clone(),
            active_param: sh.active_param,
            anchor_line: engine.view().cursor.line,
            anchor_col: engine.view().cursor.col,
        });

    let menu_bar_visible = engine.menu_bar_visible;

    let debug_toolbar = engine.debug_toolbar_visible.then(|| DebugToolbarData {
        buttons: DEBUG_BUTTONS.to_vec(),
        session_active: engine.dap_session_active,
    });

    // Build the debug sidebar data (always present). The item lists
    // themselves (variables/watch/frames/breakpoints) are painted by
    // `populate_dap_sidebar_system`'s `build_dap_*_rows` helpers directly
    // into `engine.dap_sidebar_system`, not through this struct (#1489) —
    // only the chrome fields and the Debug Output tab's lines live here.
    let debug_sidebar = {
        // Output lines for the Debug Output tab (up to 200, oldest-first).
        let debug_output_lines: Vec<String> = engine
            .dap_output_lines
            .iter()
            .rev()
            .take(200)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let launch_config_name = engine
            .dap_launch_configs
            .get(engine.dap_selected_launch_config)
            .map(|c| c.name.clone());

        DebugSidebarData {
            session_active: engine.dap_session_active,
            stopped: engine.dap_stopped_thread.is_some(),
            launch_config_name,
            debug_output_lines,
        }
    };

    // Build bottom panel tabs.
    let terminal = build_terminal_panel(engine);
    let bottom_tabs = BottomPanelTabs {
        active: engine.bottom_panel_kind.clone(),
        output_lines: debug_sidebar.debug_output_lines.clone(),
        terminal,
    };

    // Build Source Control panel data (populated when the panel is visible).
    let source_control = build_source_control_data(engine);

    let tab_switcher = engine.tab_switcher_open.then(|| TabSwitcherPanel {
        items: engine.tab_switcher_items(),
        selected_idx: engine.tab_switcher_selected,
    });

    let n = engine.group_layout.leaf_count();
    // ── Per-group chrome, built uniformly for EVERY group count (#551) ────────
    // `group_tab_bars` and `group_dividers` used to live inside an
    // `if n >= 2 { .. } else { None }` block, which forced every backend to
    // carry a parallel hand-written "exactly one group" draw path beside the
    // generic N-group one. A single group is just a split of one: the same
    // bounding-box math produces the identical full-width tab bar rect, and
    // `GroupLayout::Leaf::dividers()` already returns `vec![]`, so the generic
    // path covers N=1 with no special case. `editor_group_split` below is now
    // only a *marker* for "2 or more groups" (it still gates the hit-test
    // paths that legitimately differ), and no longer the storage for this data
    // — one source of truth, so a single-group calculation can't silently
    // drift from the N-group one the way #547's breadcrumb y-offset did.
    let group_ids = engine.group_layout.group_ids();
    // Compute group bounds from the window_rects: each group's bounds is
    // the bounding box of its windows (the tab bar is drawn just above it).
    let group_tab_bars: Vec<GroupTabBar> = group_ids
        .iter()
        .map(|&gid| {
            let tabs = build_tab_bar_for_group_by_id(engine, gid);
            // Find bounding rect for all windows in this group
            let mut min_x = f64::MAX;
            let mut min_y = f64::MAX;
            let mut max_x = f64::MIN;
            let mut max_y = f64::MIN;
            if let Some(group) = engine.editor_groups.get(&gid) {
                for wr in window_rects {
                    if group.active_tab().layout.window_ids().contains(&wr.0) {
                        min_x = min_x.min(wr.1.x);
                        min_y = min_y.min(wr.1.y);
                        max_x = max_x.max(wr.1.x + wr.1.width);
                        max_y = max_y.max(wr.1.y + wr.1.height);
                    }
                }
            }
            if min_x == f64::MAX {
                min_x = 0.0;
                min_y = 0.0;
                max_x = 0.0;
                max_y = 0.0;
            }
            let bounds = WindowRect::new(min_x, min_y, max_x - min_x, max_y - min_y);
            // Populate diff toolbar if this group contains a diff window.
            let diff_toolbar = if engine.is_in_diff_view() {
                if let Some((a, b)) = engine.diff_window_pair {
                    let group = engine.editor_groups.get(&gid);
                    let has_diff_win = group.is_some_and(|g| {
                        let wids = g.active_tab().layout.window_ids();
                        wids.contains(&a) || wids.contains(&b)
                    });
                    if has_diff_win {
                        let (_, total) = engine.diff_unified_regions();
                        let change_label = engine
                            .diff_current_change_index()
                            .map(|(c, t)| format!("{c} of {t}"));
                        Some(DiffToolbarData {
                            change_label,
                            total_changes: total,
                            unchanged_hidden: engine.diff_unchanged_hidden,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let tab_scroll_offset = engine
                .editor_groups
                .get(&gid)
                .map(|g| g.tab_scroll_offset)
                .unwrap_or(0);
            // Hit regions are expressed in char-CELLS so they are
            // backend-neutral. TUI passes char_width=1.0 (bounds already in
            // cells); GTK passes pixel bounds + real char_width, so divide to
            // recover cells. Without this, GTK's right-aligned button regions
            // (split/diff/action) would land at pixel columns and never match
            // a cell-converted click. (#515)
            let bar_width = (bounds.width / char_width).round() as u16;
            let has_diff_toolbar = diff_toolbar.is_some();
            let diff_label_cols = diff_toolbar
                .as_ref()
                .and_then(|dt| dt.change_label.as_ref())
                .map(|l| l.len() as u16 + 1)
                .unwrap_or(0);
            let is_active = gid == engine.active_group;
            let has_split = is_active || engine.is_in_diff_view();
            // #703: built once and reused for measurement *and* paint — the
            // sidecar the backend hands to `tab_bar_layout_icons` is the same
            // one it hands to `draw_tab_bar_icons`.
            let tab_icons = build_tab_bar_icons(&tabs);
            let hit_regions = compute_tab_bar_layout(
                &tabs,
                &tab_icons,
                tab_scroll_offset,
                bar_width,
                has_diff_toolbar,
                diff_label_cols,
                has_split,
            );
            let accent = if is_active {
                Some(theme.tab_active_accent)
            } else {
                None
            };
            let bar = build_tab_bar_primitive(
                &tabs,
                has_split,
                diff_toolbar.as_ref(),
                tab_scroll_offset,
                accent,
            );
            GroupTabBar {
                group_id: gid,
                tabs,
                bounds,
                diff_toolbar,
                tab_scroll_offset,
                hit_regions,
                bar,
                icons: tab_icons,
            }
        })
        .collect();
    // Collect dividers — use the total content bounds from window_rects.
    // `GroupLayout::Leaf::dividers()` returns an empty vec, so this is
    // naturally empty in single-group mode (#551).
    let content_bounds = if !window_rects.is_empty() {
        let min_x = window_rects.iter().map(|r| r.1.x).fold(f64::MAX, f64::min);
        let min_y = window_rects
            .iter()
            .map(|r| r.1.y - line_height)
            .fold(f64::MAX, f64::min);
        let max_x = window_rects
            .iter()
            .map(|r| r.1.x + r.1.width)
            .fold(f64::MIN, f64::max);
        let max_y = window_rects
            .iter()
            .map(|r| r.1.y + r.1.height)
            .fold(f64::MIN, f64::max);
        WindowRect::new(min_x, min_y, max_x - min_x, max_y - min_y)
    } else {
        WindowRect::new(0.0, 0.0, 0.0, 0.0)
    };
    let group_dividers = engine.group_layout.dividers(content_bounds, &mut 0);
    let editor_group_split = (n >= 2).then_some(EditorGroupSplitData {
        active_group: engine.active_group,
        num_groups: n,
    });

    let board = build_board_data(engine);

    // Build breadcrumbs for each editor group
    let breadcrumbs = if engine.settings.breadcrumbs {
        let group_ids = engine.group_layout.group_ids();
        group_ids
            .iter()
            .map(|&gid| {
                let segments = build_breadcrumbs_for_group(engine, gid);
                // Compute bounds from the group's windows
                let mut min_x = f64::MAX;
                let mut min_y = f64::MAX;
                let mut max_x = f64::MIN;
                if let Some(group) = engine.editor_groups.get(&gid) {
                    for wr in window_rects {
                        if group.active_tab().layout.window_ids().contains(&wr.0) {
                            min_x = min_x.min(wr.1.x);
                            min_y = min_y.min(wr.1.y);
                            max_x = max_x.max(wr.1.x + wr.1.width);
                        }
                    }
                }
                if min_x == f64::MAX {
                    min_x = 0.0;
                    min_y = 0.0;
                    max_x = 0.0;
                }
                // Place bounds at the actual breadcrumb row, `breadcrumb_row_h`
                // above the window content top — NOT `line_height` (#700):
                // GTK's breadcrumb row is fixed-pixel and can differ from the
                // editor's text line height.
                let bc_y = (min_y - breadcrumb_row_h).max(0.0);
                let bounds = WindowRect::new(min_x, bc_y, max_x - min_x, breadcrumb_row_h);
                let bar = breadcrumbs_to_quadraui_status_bar(
                    &segments,
                    theme,
                    engine.breadcrumb_focus,
                    engine.breadcrumb_selected,
                );
                BreadcrumbBar {
                    group_id: gid,
                    segments,
                    bounds,
                    bar,
                    draw_layout: std::cell::RefCell::new(None),
                }
            })
            .collect()
    } else {
        vec![]
    };

    // The single-group / active tab bar's layout, in char-cells. #551 made
    // `group_tab_bars` populated for every group count (a single group is a
    // split of one), so when `n < 2` it holds exactly one entry built from
    // the same tabs/scroll-offset/bar-width inputs this mirror used to
    // recompute independently. Cloning that entry's layout — rather than
    // calling `compute_tab_bar_layout` a second time — means this field is
    // always byte-for-byte what `GroupTabBar::bar` painted (#822,
    // `feedback_cache_paint_layout`: hit-testing must read what paint
    // produced, never re-derive it). Empty in multi-group mode (handled
    // per-group on each `GroupTabBar`). (#515)
    let tab_bar_hit_regions = if n >= 2 || window_rects.is_empty() {
        empty_tab_bar_layout()
    } else {
        group_tab_bars
            .first()
            .map(|g| g.hit_regions.clone())
            .unwrap_or_else(empty_tab_bar_layout)
    };

    ScreenLayout {
        tab_bar,
        tab_bar_hit_regions,
        windows,
        global_status_bar,
        command,
        wildmenu,
        active_window_id,
        completion,
        hover,
        quickfix,
        bottom_tabs,
        signature_help,
        menu_bar_visible,
        debug_toolbar,
        debug_sidebar,
        source_control,
        picker: engine.picker_open.then(|| {
            use crate::core::engine::PickerSource;
            // `PickerSource::Custom` also covers `vimcode.picker.open`
            // (#1630) — a plugin item may declare a file/buffer preview
            // (`Engine::picker_load_preview`'s plugin branch), and gating it
            // out here would leave that state populated but never painted,
            // the exact "state vs. paint" bug class #587/#592 warn about.
            let has_preview = matches!(
                engine.picker_source,
                PickerSource::Files | PickerSource::Grep | PickerSource::Custom(_)
            );
            PickerPanel {
                title: engine.picker_title.clone(),
                query: engine.picker_query.clone(),
                items: engine
                    .picker_items
                    .iter()
                    .map(|item| PickerPanelItem {
                        display: item.display.clone(),
                        detail: item.detail.clone(),
                        match_positions: item.match_positions.clone(),
                        depth: item.depth,
                        expandable: item.expandable,
                        expanded: item.expanded,
                    })
                    .collect(),
                selected_idx: engine.picker_selected,
                scroll_top: engine.picker_scroll_top,
                total_count: if engine.picker_source == PickerSource::Grep {
                    engine.picker_items.len()
                } else {
                    engine.picker_all_items.len()
                },
                preview: if has_preview {
                    engine
                        .picker_preview
                        .as_ref()
                        .map(|p| p.lines.clone())
                        .or_else(|| Some(Vec::new()))
                } else {
                    None
                },
                preview_scroll: engine.picker_preview_scroll,
            }
        }),
        tab_switcher,
        editor_group_split,
        group_tab_bars,
        group_dividers,
        window_dividers,
        minimap,
        board,
        ext_panel: build_ext_panel_data(engine),
        breadcrumbs,
        diff_peek: engine.diff_peek.as_ref().map(|dp| DiffPeekPopup {
            anchor_line: dp.anchor_line,
            hunk_lines: dp.hunk_lines.clone(),
        }),
        change_review: engine.change_review.clone(),
        panel_hover: engine.panel_hover.as_ref().map(|ph| PanelHoverPopupData {
            markdown: ph.markdown.clone(),
            line_text: ph.line_text.clone(),
            code_highlights: ph.code_highlights.clone(),
            links: ph.links.clone(),
            item_index: ph.item_index,
            panel_name: ph.panel_name.clone(),
        }),
        editor_hover: engine.editor_hover.as_ref().map(|eh| EditorHoverPopupData {
            markdown: eh.markdown.clone(),
            line_text: eh.line_text.clone(),
            code_highlights: eh.code_highlights.clone(),
            links: eh.links.clone(),
            anchor_line: eh.anchor_line,
            anchor_col: eh.anchor_col,
            scroll_top: eh.scroll_top,
            focused_link: eh.focused_link,
            has_focus: engine.editor_hover_has_focus,
            popup_width: eh.popup_width,
            frozen_scroll_top: eh.frozen_scroll_top,
            frozen_scroll_left: eh.frozen_scroll_left,
            selection: eh.selection.as_ref().map(|s| s.normalized()),
        }),
        dialog: engine.dialog.as_ref().map(|d| DialogPanel {
            title: d.title.clone(),
            body: d.body.clone(),
            buttons: d
                .buttons
                .iter()
                .enumerate()
                .map(|(i, btn)| {
                    (
                        format_button_label(&btn.label, btn.hotkey),
                        i == d.selected,
                        btn.action == "cancel",
                    )
                })
                .collect(),
            input: d.input.as_ref().map(|inp| DialogInputPanel {
                display: if inp.is_password {
                    format!("{}|", "*".repeat(inp.value.len()))
                } else {
                    format!("{}|", inp.value)
                },
            }),
            vertical_buttons: d.tag == "code_actions",
        }),
        context_menu: engine
            .context_menu
            .as_ref()
            .map(context_menu_state_to_panel),
        find_replace: if engine.find_replace_open {
            let match_info = if engine.search_matches.is_empty() {
                if engine.find_replace_query.is_empty() {
                    String::new()
                } else {
                    "No results".to_string()
                }
            } else {
                match engine.search_index {
                    Some(idx) => format!("{} of {}", idx + 1, engine.search_matches.len()),
                    None => format!("{} matches", engine.search_matches.len()),
                }
            };
            // Compute active group bounds from window rects
            let active_group_bounds = {
                let active_group = &engine.active_group;
                let group_window_ids: Vec<_> = engine
                    .editor_groups
                    .get(active_group)
                    .map(|g| g.active_tab().layout.window_ids())
                    .unwrap_or_default();
                let mut min_x = f64::MAX;
                let mut min_y = f64::MAX;
                let mut max_x = 0.0f64;
                let mut max_y = 0.0f64;
                for (wid, rect) in window_rects {
                    if group_window_ids.contains(wid) {
                        min_x = min_x.min(rect.x);
                        min_y = min_y.min(rect.y);
                        max_x = max_x.max(rect.x + rect.width);
                        max_y = max_y.max(rect.y + rect.height);
                    }
                }
                if min_x < f64::MAX {
                    WindowRect::new(min_x, min_y, max_x - min_x, max_y - min_y)
                } else {
                    // Fallback: use first window rect or zero
                    window_rects
                        .first()
                        .map(|(_, r)| *r)
                        .unwrap_or_else(|| WindowRect::new(0.0, 0.0, 800.0, 600.0))
                }
            };
            let panel_w = FR_PANEL_WIDTH;
            let (hit_regions, _input_w) = compute_find_replace_hit_regions(
                panel_w,
                engine.find_replace_show_replace,
                &match_info,
            );
            // Convert vimcode's f64 WindowRect to quadraui::Rect (f32).
            let qr = quadraui::Rect::new(
                active_group_bounds.x as f32,
                active_group_bounds.y as f32,
                active_group_bounds.width as f32,
                active_group_bounds.height as f32,
            );
            Some(FindReplacePanel {
                query: engine.find_replace_query.clone(),
                replacement: engine.find_replace_replacement.clone(),
                show_replace: engine.find_replace_show_replace,
                focus: engine.find_replace_focus,
                cursor: engine.find_replace_cursor,
                sel_anchor: engine.find_replace_sel_anchor,
                match_info,
                case_sensitive: engine.find_replace_options.case_sensitive,
                whole_word: engine.find_replace_options.whole_word,
                use_regex: engine.find_replace_options.use_regex,
                preserve_case: engine.find_replace_options.preserve_case,
                in_selection: engine.find_replace_options.in_selection,
                group_bounds: qr,
                panel_width: panel_w,
                replace_one_glyph: crate::icons::FIND_REPLACE.s().to_string(),
                replace_all_glyph: crate::icons::FIND_REPLACE_ALL.s().to_string(),
                hit_regions,
            })
        } else {
            None
        },
        tab_tooltip: engine.tab_hover_tooltip.clone(),
        separated_status_line,
    }
}
