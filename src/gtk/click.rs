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
/// The text-area column is resolved via `backend.editor_col_at_x` (quadraui,
/// #420/#560) — the same Pango layout + attributes `draw_editor` painted
/// with — instead of a bespoke `xy_to_index` reconstruction, so paint and
/// click can never drift apart again.
#[allow(clippy::too_many_arguments)]
pub(super) fn pixel_to_click_target(
    engine: &mut Engine,
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    // Pixel-accurate per-group tab-bar hit geometry captured from the
    // rasteriser during `render_content` (via `Backend::tab_bar_layout`). GTK
    // draws tabs with proportional-font Pango widths, so the char-cell
    // `hit_regions` on `cached_layout` do NOT match the drawn geometry — clicks
    // must resolve against these actual pixel bounds. (#515)
    tab_pixel_hits: &TabPixelHitMap,
    // Legacy per-backend pixel maps — no longer consulted for tab-bar clicks.
    // Kept in the signature so existing call sites compile unchanged; slated for
    // removal along with the rest of the pixel-map plumbing. (#515)
    _tab_slot_positions: &TabSlotMap,
    _diff_btn_map: &DiffBtnMap,
    _split_btn_map: &SplitBtnMap,
    _action_btn_map: &ActionBtnMap,
    status_segment_map: &StatusSegmentMap,
) -> ClickTarget {
    let tab_bar_height = render_mod::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
    let single_tab_hidden = engine.is_tab_bar_hidden(engine.active_group);

    let zone = render_mod::screen_zone_hit_test(
        cached_layout,
        x,
        y,
        tab_bar_height,
        single_tab_hidden,
        engine.active_group,
    );
    match zone {
        ScreenZone::TabBar {
            group_id,
            local_x,
            bar_width: _,
        } => {
            engine.active_group = group_id;
            tab_bar_inner_hit_test(
                engine,
                group_id,
                local_x,
                char_width,
                cached_layout,
                tab_pixel_hits,
            )
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
                    view_row, buf_line, ..
                } => {
                    // #560: resolve the exact column via the shared
                    // quadraui text-layout inverse instead of a
                    // separately-built, attribute-less Pango layout —
                    // `editor_col_at_x` re-runs `xy_to_index` against the
                    // same per-span-attributed layout `draw_editor`
                    // painted with (or the cached last-painted clone when
                    // called outside a frame scope), so it can't drift
                    // from the glyphs actually drawn on screen. `x`/`y`
                    // are absolute surface coordinates, matching
                    // `editor.rect`'s coordinate space (`rw.rect` here),
                    // so the original click `x` is passed straight
                    // through — no gutter/scroll reconstruction needed.
                    let (editor, editor_layout) =
                        render_mod::editor_text_layout(rw, char_width, line_height);
                    use quadraui::Backend as _;
                    let col = backend.borrow().editor_col_at_x(
                        &editor_layout,
                        &editor,
                        view_row,
                        x as f32,
                    );
                    ClickTarget::BufferPos(window_id, buf_line, col)
                }
                _ => ClickTarget::None,
            }
        }
        _ => ClickTarget::None,
    }
}

/// Tab bar inner hit-test.
///
/// `local_x` is pixels relative to the tab bar's left edge. For GTK we resolve
/// against the pixel-accurate geometry the rasteriser actually drew this frame
/// (`tab_pixel_hits`, captured in `render_content` via `Backend::tab_bar_layout`).
/// GTK tabs are laid out with proportional-font Pango widths + fixed pixel
/// padding, so the char-cell `hit_regions` (correct for the monospace TUI) badly
/// mis-measure them — clicks in a tab's middle landed on the close button and
/// clicks near its right edge landed on the next tab (#515 regression). Falls
/// back to the char-cell path only if no pixel geometry was cached (e.g. a click
/// arriving before the first paint populated the map).
fn tab_bar_inner_hit_test(
    engine: &mut Engine,
    group_id: GroupId,
    local_x: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    tab_pixel_hits: &TabPixelHitMap,
) -> ClickTarget {
    let target = tab_pixel_hits
        .get(&group_id.0)
        .and_then(|ph| resolve_pixel_tab_click(ph, local_x))
        .or_else(|| resolve_charcell_tab_click(cached_layout, group_id, local_x, char_width));

    dispatch_tab_bar_target(engine, group_id, target)
}

/// Resolve a tab-bar click against the pixel-accurate drawn geometry.
///
/// Close buttons are checked before tab bodies (a close zone is a sub-region of
/// its tab), then tab bodies, then the disjoint right-segment buttons.
fn resolve_pixel_tab_click(
    ph: &TabBarPixelHits,
    local_x: f64,
) -> Option<crate::core::engine::TabBarClickTarget> {
    use crate::core::engine::TabBarClickTarget as T;

    let in_range = |(a, b): (f64, f64)| a != b && local_x >= a && local_x < b;

    for (idx, cb) in ph.close.iter().enumerate() {
        if let Some(&bounds) = cb.as_ref() {
            if in_range(bounds) {
                return Some(T::CloseTab(idx));
            }
        }
    }
    for (idx, &slot) in ph.slots.iter().enumerate() {
        if in_range(slot) {
            return Some(T::Tab(idx));
        }
    }
    for &(start, end, target) in &ph.segments {
        if in_range((start, end)) {
            return Some(target);
        }
    }
    None
}

/// Char-cell fallback (matches the TUI monospace layout). Only used before the
/// first paint has populated the pixel-hit cache.
fn resolve_charcell_tab_click(
    cached_layout: &render::ScreenLayout,
    group_id: GroupId,
    local_x: f64,
    char_width: f64,
) -> Option<crate::core::engine::TabBarClickTarget> {
    let col = (local_x / char_width).floor().max(0.0) as u16;
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
    render_mod::resolve_tab_bar_click(regions, col)
}

/// Resolve which tab (if any) a right-click landed on, without any of the
/// left-click side effects `tab_bar_inner_hit_test`/`dispatch_tab_bar_target`
/// apply (selecting the tab, closing it, opening a split, ...).
///
/// Right-clicks reach `ShellApp::handle` via a dedicated `MouseButton::Right`
/// branch that historically only ever built `Msg::EditorRightClick` — there
/// was no tab-bar-aware routing at all, so right-clicking a tab always opened
/// the *editor's* context menu instead of a tab-specific one (#546 FAILED-1).
/// This mirrors `pixel_to_click_target`'s zone resolution (read-only) so the
/// caller can tell a tab-bar right-click apart from an editor right-click
/// before deciding which `Msg` to dispatch.
pub(super) fn resolve_tab_right_click(
    engine: &Engine,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    tab_pixel_hits: &TabPixelHitMap,
) -> Option<(GroupId, usize)> {
    use crate::core::engine::TabBarClickTarget as T;

    let tab_bar_height = render_mod::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
    let single_tab_hidden = engine.is_tab_bar_hidden(engine.active_group);
    let zone = render_mod::screen_zone_hit_test(
        cached_layout,
        x,
        y,
        tab_bar_height,
        single_tab_hidden,
        engine.active_group,
    );
    let ScreenZone::TabBar {
        group_id, local_x, ..
    } = zone
    else {
        return None;
    };
    let target = tab_pixel_hits
        .get(&group_id.0)
        .and_then(|ph| resolve_pixel_tab_click(ph, local_x))
        .or_else(|| resolve_charcell_tab_click(cached_layout, group_id, local_x, char_width));
    match target {
        Some(T::Tab(idx)) | Some(T::CloseTab(idx)) => Some((group_id, idx)),
        _ => None,
    }
}

/// Apply the engine-side effect for a resolved tab-bar click target and return
/// the `ClickTarget` the caller dispatches.
fn dispatch_tab_bar_target(
    engine: &mut Engine,
    group_id: GroupId,
    target: Option<crate::core::engine::TabBarClickTarget>,
) -> ClickTarget {
    use crate::core::engine::TabBarClickTarget as T;
    use crate::core::window::SplitDirection;

    match target {
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
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    x: f64,
    y: f64,
    alt: bool,
    line_height: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    tab_pixel_hits: &TabPixelHitMap,
    tab_slot_positions: &TabSlotMap,
    diff_btn_map: &DiffBtnMap,
    split_btn_map: &SplitBtnMap,
    action_btn_map: &ActionBtnMap,
    status_segment_map: &StatusSegmentMap,
) -> (Option<bool>, Option<EngineAction>) {
    match pixel_to_click_target(
        engine,
        backend,
        x,
        y,
        line_height,
        char_width,
        cached_layout,
        tab_pixel_hits,
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
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    tab_pixel_hits: &TabPixelHitMap,
    tab_slot_positions: &TabSlotMap,
    diff_btn_map: &DiffBtnMap,
    split_btn_map: &SplitBtnMap,
    action_btn_map: &ActionBtnMap,
    status_segment_map: &StatusSegmentMap,
) {
    if let ClickTarget::BufferPos(wid, line, col) = pixel_to_click_target(
        engine,
        backend,
        x,
        y,
        line_height,
        char_width,
        cached_layout,
        tab_pixel_hits,
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
    backend: &Rc<RefCell<super::backend::GtkBackend>>,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    tab_pixel_hits: &TabPixelHitMap,
    tab_slot_positions: &TabSlotMap,
    diff_btn_map: &DiffBtnMap,
    split_btn_map: &SplitBtnMap,
    action_btn_map: &ActionBtnMap,
    status_segment_map: &StatusSegmentMap,
) {
    if let ClickTarget::BufferPos(wid, line, col) = pixel_to_click_target(
        engine,
        backend,
        x,
        y,
        line_height,
        char_width,
        cached_layout,
        tab_pixel_hits,
        tab_slot_positions,
        diff_btn_map,
        split_btn_map,
        action_btn_map,
        status_segment_map,
    ) {
        engine.mouse_drag(wid, line, col);
    }
}

#[cfg(test)]
mod emoji_click_column_tests {
    //! #560 regression: a manual smoke test on the shared-quadraui-inverse
    //! fix reported clicks landing one column to the right of the intended
    //! glyph on markdown lines containing emoji (✅ 🟡 ❌ ⏭️), with the
    //! drift compounding for every wide/multi-byte glyph preceding the
    //! click point on the line. Root-cause investigation (see the vimcode
    //! issue #560 durable-findings log) reproduced the *exact* symptom
    //! shape — perfect on plain monospace text, growing drift after each
    //! emoji — only when `GtkBackend::editor_col_at_x` falls back to
    //! `EditorLayout::col_at_x`'s uniform-monospace division (the TUI path,
    //! which assumes every glyph is exactly one `cell_width` wide). That
    //! fallback fires when no Pango layout is available; the real
    //! per-glyph `quadraui::gtk::editor_col_at_x` (Pango `xy_to_index`)
    //! path was verified byte-exact for this same string (base emoji,
    //! astral-plane emoji, and a variation-selector emoji) in isolation.
    //!
    //! This test pins the production pipeline end to end — real
    //! `md_inline_spans` bold-span byte offsets via `build_screen_layout`,
    //! then `render::editor_text_layout` + `quadraui::gtk::editor_col_at_x`
    //! — against a headless Pango layout, so a future regression that
    //! silently reintroduces the naive fallback (or corrupts the
    //! span byte-offset pipeline feeding Pango's attributes) fails a
    //! `cargo test`, not just a manual click in the running app.
    use super::*;
    use ::pangocairo::cairo::{Context, Format, ImageSurface};

    fn headless_pango_layout() -> pango::Layout {
        let surface = ImageSurface::create(Format::ARgb32, 900, 60).expect("create ImageSurface");
        let cr = Context::new(&surface).expect("Context::new");
        let ctx = pangocairo::create_context(&cr);
        ctx.set_font_description(Some(&pango::FontDescription::from_string("Monospace 12")));
        pango::Layout::new(&ctx)
    }

    #[test]
    fn click_resolves_exact_column_on_emoji_markdown_line() {
        let text = "Total: **58 commands**  \u{b7}  \u{2705} 36  \u{b7}  \u{1f7e1} 2  \u{b7}  \u{274c} 14  \u{b7}  \u{23ed}\u{fe0f} 6";
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, text);
        let buf_id = engine.active_buffer_id();
        engine.buffer_manager.get_mut(buf_id).unwrap().file_path =
            Some(std::path::PathBuf::from("notes.md"));

        let char_width = 9.0;
        let line_height = 18.0;
        let theme = Theme::onedark();
        let bounds = WindowRect::new(0.0, 0.0, 800.0, 600.0);
        let (rects, _) = engine.calculate_group_window_rects(bounds, 24.0);
        let layout = build_screen_layout(&engine, &theme, &rects, line_height, char_width, true);
        let rw = &layout.windows[0];
        assert_eq!(
            rw.lines[0].raw_text, text,
            "line should not wrap in an 800px window"
        );

        let (editor, editor_layout) = render::editor_text_layout(rw, char_width, line_height);
        let line = &editor.lines[0];

        // `editor_col_at_x` unconditionally `set_text`/`set_attributes`es
        // its layout argument from `line` before hit-testing, so a single
        // throwaway call (the same one every real click makes) leaves
        // `measure_layout` holding the exact attributed text `draw_editor`
        // paints — real bold-run glyph widths included — without vimcode
        // reimplementing quadraui's private `build_pango_attrs`. Reusing
        // this one layout for both measuring and resolving throughout
        // mirrors production, which caches and reuses a single
        // `last_editor_pango_layout` across an entire click.
        let measure_layout = headless_pango_layout();
        let _ = quadraui::gtk::editor_col_at_x(&measure_layout, line, &editor_layout, 0.0);

        let char_count = text.chars().count();
        let mut prev_pos: Option<(i32, i32)> = None;
        for (char_idx, (byte_idx, ch)) in text.char_indices().enumerate() {
            let pos = measure_layout.index_to_pos(byte_idx as i32);
            // A zero-width combining mark (e.g. the U+FE0F variation
            // selector on "⏭️") shares its base character's glyph cluster
            // — Pango reports the *identical* (x, width) rect for both
            // byte offsets, since there is no distinct on-screen pixel
            // region for the combining mark alone. A click can only ever
            // land on the cluster as a whole, so such chars have no
            // resolvable column of their own to assert against — skip
            // them rather than asserting an unreachable identity.
            if prev_pos == Some((pos.x(), pos.width())) {
                continue;
            }
            prev_pos = Some((pos.x(), pos.width()));

            let glyph_left_x =
                editor_layout.text_bounds.x as f64 + pos.x() as f64 / pango::SCALE as f64;
            let glyph_width = (pos.width() as f64 / pango::SCALE as f64).max(2.0);
            let click_x = glyph_left_x + glyph_width * 0.25;

            let resolved = quadraui::gtk::editor_col_at_x(
                &measure_layout,
                line,
                &editor_layout,
                click_x as f32,
            );
            assert_eq!(
                resolved, char_idx,
                "clicking char {char_idx} ({ch:?}) resolved to col {resolved}, not {char_idx} \
                 — the paint↔click column inverse has drifted for this glyph"
            );
        }
        assert_eq!(char_count, text.chars().count());
    }
}
