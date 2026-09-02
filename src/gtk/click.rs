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
///
/// `mutate_focus` gates every side effect this function performs purely as a
/// byproduct of resolving a pixel position — flipping `active_group`/the
/// active tab, and executing a gutter action (e.g. toggling a breakpoint).
/// Real clicks (`handle_mouse_click`, `handle_mouse_double_click`, Ctrl+click,
/// tab-drag-start detection) pass `true`, since landing on a pane or tab
/// should focus it. `handle_mouse_drag` passes `false`: while a text-selection
/// drag is held down, the mouse sweeping over a *different* split's tab bar or
/// gutter must not steal focus or fire actions there — `Engine::mouse_drag`'s
/// origin-window lock already keeps the selection pinned to the split the
/// drag started in (#568), but only if this hit-test stays a pure query
/// during a drag, matching how TUI's drag path (`src/tui_main/mouse.rs`)
/// never mutates engine focus state either.
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
    // Cached `quadraui::FrameHitMap` covering the Editor/TabBar surfaces
    // painted this frame (#449), plus a `FrameZone::TabBar { idx } -> (GroupId,
    // rect)` table keyed by the tab bar's *global* surface index (editors are
    // pushed into the same `ScreenLayout` before any tab bar, so a tab bar's
    // `idx` is offset by however many editor surfaces preceded it — a plain
    // 0-based `Vec` here would look up the wrong entry, or none at all).
    // `None` before the first paint. See `frame_zone_to_screen_zone` for how
    // these replace `screen_zone_hit_test`'s manual Window/TabBar rect-walk.
    frame_hit_map: Option<&quadraui::FrameHitMap>,
    tab_bar_zones: &HashMap<usize, (GroupId, quadraui::Rect)>,
    mutate_focus: bool,
) -> ClickTarget {
    // #752: the separated status line's arm was here, and the per-window
    // status line's arm was in the `WindowZone::StatusBar` match below. Both
    // are now status bands walked by `render::route_chrome_click`, which
    // `App::handle_mouse_click_msg` runs *before* it ever reaches this
    // function — so a status click can no longer arrive here at all, and the
    // shared router (not this backend) decides the order the three bars are
    // arbitrated in.

    // ── Minimap click / drag (#35, #722) ────────────────────────────────────
    // Pure rect plumbing: the shared resolver owns the hit-test and the
    // scroll. Checked before the zone walk because every window's strip is
    // carved out of that window's own rect, so a `ScreenZone::Window` hit
    // would otherwise swallow it. Gated on `mutate_focus` so a hover query
    // never scrolls. `apply_minimap_click` resolves against *every* pane's
    // strip and reports which one it hit — never assumed to be the active
    // window, since a split can have a strip on an inactive pane too.
    if mutate_focus {
        if let Some((window_id, line)) =
            render_mod::apply_minimap_click(engine, cached_layout, x, y)
        {
            return ClickTarget::Minimap(window_id, line);
        }
    }

    let tab_bar_height = render_mod::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
    let single_tab_hidden = engine.is_tab_bar_hidden(engine.active_group);

    let zone = frame_hit_map
        .and_then(|hit_map| {
            let z = frame_zone_to_screen_zone(hit_map, tab_bar_zones, cached_layout, x, y);
            (!matches!(z, ScreenZone::None)).then_some(z)
        })
        .unwrap_or_else(|| {
            render_mod::screen_zone_hit_test(
                cached_layout,
                x,
                y,
                tab_bar_height,
                single_tab_hidden,
                engine.active_group,
            )
        });
    match zone {
        ScreenZone::TabBar {
            group_id,
            local_x,
            bar_width: _,
        } => {
            if !mutate_focus {
                // A drag sweeping over another split's tab bar must not
                // switch tabs/focus there (#568) — treat it as a miss.
                return ClickTarget::None;
            }
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
            if mutate_focus {
                engine.activate_group_for_window(window_id);
            }

            let Some(rw) = cached_layout.windows.get(window_idx) else {
                return ClickTarget::None;
            };
            match render_mod::window_zone_hit_test(rw, rel_x, rel_y, line_height, char_width) {
                WindowZone::Gutter {
                    line_idx,
                    gutter_col,
                    ..
                } => {
                    if !mutate_focus {
                        // A drag sweeping over another split's gutter must
                        // not fire gutter actions (e.g. toggle a breakpoint)
                        // there (#568).
                        return ClickTarget::None;
                    }
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

/// Resolve the top-level `ScreenZone` using the cached `quadraui::FrameHitMap`
/// (#449), which covers exactly the `Editor`/`TabBar` surfaces painted in
/// `App::render_content` via `quadraui::ScreenLayout::hit_map()`
/// (quadraui#425) — pushed from the SAME objects/rects already painted, so
/// this can never drift from what's on screen. Returns `ScreenZone::None`
/// when the point isn't in an Editor/TabBar zone (including breadcrumb/
/// divider pixels, which have no `FrameZone` equivalent — the caller falls
/// back to `render_mod::screen_zone_hit_test` for those).
fn frame_zone_to_screen_zone(
    hit_map: &quadraui::FrameHitMap,
    // Keyed by `FrameZone::TabBar { idx }`'s global surface index, NOT a
    // per-tab-bar position — see the doc comment on `pixel_to_click_target`'s
    // `tab_bar_zones` parameter.
    tab_bar_zones: &HashMap<usize, (GroupId, quadraui::Rect)>,
    cached_layout: &render::ScreenLayout,
    x: f64,
    y: f64,
) -> ScreenZone {
    match hit_map.hit_test(x as f32, y as f32) {
        quadraui::FrameZone::TabBar { idx } => {
            if let Some((group_id, rect)) = tab_bar_zones.get(&idx) {
                return ScreenZone::TabBar {
                    group_id: *group_id,
                    local_x: x - rect.x as f64,
                    bar_width: rect.width as f64,
                };
            }
        }
        quadraui::FrameZone::Editor { idx } => {
            if let Some(rw) = cached_layout.windows.get(idx) {
                let r = &rw.rect;
                return ScreenZone::Window {
                    window_id: rw.window_id,
                    window_idx: idx,
                    rel_x: x - r.x,
                    rel_y: y - r.y,
                };
            }
        }
        _ => {}
    }
    ScreenZone::None
}

/// Build the Pango context the *click* backend uses to resolve editor
/// columns, matched to the editor's **painted** font.
///
/// vimcode keeps a separate `GtkBackend` for click hit-testing than the one
/// quadraui's ShellApp runner paints with (see `App::render_content`). At
/// click time `editor_col_at_x` runs `xy_to_index` against *this* context's
/// Pango layout, so its glyph advances must reproduce the ones the painted
/// glyphs actually used — otherwise column resolution scales by the wrong
/// cell width and drifts left, the drift growing with `x` (#560 iter-3
/// smoke failure).
///
/// The runner paints the editor with a hardcoded monospace font
/// (`quadraui::gtk::run` → `"Monospace 11"`), **ignoring** `settings.font_*`;
/// the resulting painted cell advance is what `Backend::char_width()` reports
/// and what `build_screen_layout` / `editor_text_layout` positioned glyphs
/// with. The earlier fix fonted this context from `settings.font_size` (14 by
/// default) while the paint ran at 11 — a ~1.27× scale error that produced
/// exactly the reported left-growing drift on plain text, bold, italic and
/// scrolled lines alike.
///
/// So we mirror the runner's family (`Monospace`) and tune only the point
/// size: measure a probe `'0'` advance and scale until it equals the painted
/// `char_width`. Because it is the same family at the reproduced size, *all*
/// glyph advances — including emoji/CJK fallback — line up with the paint.
pub(super) fn build_editor_click_context(paint_char_width: f64) -> Option<pango::Context> {
    let surface = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1).ok()?;
    let cr = gtk4::cairo::Context::new(&surface).ok()?;
    let ctx = pangocairo::create_context(&cr);

    // Mirror the runner's editor font family; only the size is tuned so the
    // measured '0' advance reproduces the painted cell width.
    let family = "Monospace";
    let mut size = 11.0_f64;
    let probe = pango::Layout::new(&ctx);
    probe.set_font_description(Some(&pango::FontDescription::from_string(&format!(
        "{family} {size}"
    ))));
    probe.set_text("0");
    let w0 = probe.pixel_size().0 as f64;
    if w0 > 0.1 && paint_char_width > 0.1 {
        size = (size * paint_char_width / w0).clamp(1.0, 400.0);
    }

    ctx.set_font_description(Some(&pango::FontDescription::from_string(&format!(
        "{family} {size}"
    ))));
    Some(ctx)
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
    )] = if cached_layout.editor_group_split.is_some() {
        cached_layout
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
/// branch that historically only ever opened the *editor* context menu — there
/// was no tab-bar-aware routing at all, so right-clicking a tab always opened
/// the *editor's* context menu instead of a tab-specific one (#546 FAILED-1).
/// This mirrors `pixel_to_click_target`'s zone resolution (read-only) so the
/// caller can tell a tab-bar right-click apart from an editor right-click
/// before deciding which `Msg` to dispatch.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_tab_right_click(
    engine: &Engine,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    tab_pixel_hits: &TabPixelHitMap,
    frame_hit_map: Option<&quadraui::FrameHitMap>,
    tab_bar_zones: &HashMap<usize, (GroupId, quadraui::Rect)>,
) -> Option<(GroupId, usize)> {
    use crate::core::engine::TabBarClickTarget as T;

    let tab_bar_height = render_mod::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
    let single_tab_hidden = engine.is_tab_bar_hidden(engine.active_group);
    let zone = frame_hit_map
        .and_then(|hit_map| {
            let z = frame_zone_to_screen_zone(hit_map, tab_bar_zones, cached_layout, x, y);
            (!matches!(z, ScreenZone::None)).then_some(z)
        })
        .unwrap_or_else(|| {
            render_mod::screen_zone_hit_test(
                cached_layout,
                x,
                y,
                tab_bar_height,
                single_tab_hidden,
                engine.active_group,
            )
        });
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
    frame_hit_map: Option<&quadraui::FrameHitMap>,
    tab_bar_zones: &HashMap<usize, (GroupId, quadraui::Rect)>,
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
        frame_hit_map,
        tab_bar_zones,
        true, // real click: focus/tab/gutter side effects are intended
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
    frame_hit_map: Option<&quadraui::FrameHitMap>,
    tab_bar_zones: &HashMap<usize, (GroupId, quadraui::Rect)>,
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
        frame_hit_map,
        tab_bar_zones,
        true, // real click: focus/tab/gutter side effects are intended
    ) {
        engine.mouse_double_click(wid, line, col);
    }
}

/// Apply a [`render::MouseDragRoute::EditorText`] drag — extend the visual
/// selection to the glyph under the cursor.
///
/// #568: this only ever fires while a mouse button is held (drag
/// continuation), so text-selection resolution goes through
/// `pixel_to_click_target` as a pure query (`mutate_focus: false`) — the
/// mouse sweeping over a different split's tab bar/gutter while the drag is
/// held must not steal focus or fire actions there. `Engine::mouse_drag`'s
/// origin-window lock then keeps the selection itself pinned to the split
/// the drag started in.
///
/// #756: the minimap check that used to open this function is gone — the
/// strip is now [`render::MouseDragRoute::Minimap`], arbitrated above the
/// editor text area by the shared drag router, so this function is only
/// reached once that router has already ruled the point out.
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
    frame_hit_map: Option<&quadraui::FrameHitMap>,
    tab_bar_zones: &HashMap<usize, (GroupId, quadraui::Rect)>,
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
        frame_hit_map,
        tab_bar_zones,
        false, // drag continuation: pure query, no focus/tab/gutter side effects
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
    use crate::render::build_screen_layout;
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

    /// #560 iteration 2: the test above calls `quadraui::gtk::editor_col_at_x`
    /// *directly* with a hand-built, correctly-fonted layout — it verifies the
    /// per-glyph Pango inverse but **bypasses** `GtkBackend::editor_col_at_x`'s
    /// runtime branch-selection (`current_frame_refs()` → `last_editor_pango_layout`
    /// → `pango_ctx` → naive `EditorLayout::col_at_x`). A live mouse click goes
    /// through the *trait* method, outside any frame scope, so it depends on the
    /// backend having stashed a correctly-fonted layout during paint. This test
    /// drives exactly that path: paint the editor through the trait (as
    /// `draw_window` does), then resolve clicks through
    /// `GtkBackend::editor_col_at_x` (as `pixel_to_click_target` does) — so a
    /// regression that makes live clicks fall through to the naive uniform-cell
    /// division (perfect on plain text, +1 col per preceding wide glyph on emoji
    /// lines — the exact reported symptom) fails here.
    /// #560 iteration 2 robustness: build the `Engine`/`Editor`/`EditorLayout`
    /// for the emoji markdown line and the pixel-`x` click for every glyph,
    /// then resolve each through `GtkBackend::editor_col_at_x` under the exact
    /// backend state named by `set_pango_context`. `paint_first` selects which
    /// fallback branch the trait method takes:
    ///
    /// * `true`  → an editor paint runs through the trait first, so
    ///   `last_editor_pango_layout` is stashed (the steady-state live path
    ///   after frame 1).
    /// * `false` → NO paint, but `set_pango_context` has stored a
    ///   correctly-fonted editor context (what `draw::draw_editor` now does
    ///   every frame), so the trait method resolves via the `pango_ctx`
    ///   fallback instead of the naive `EditorLayout::col_at_x` division.
    ///
    /// Both must land every click on its own glyph. Emoji here render 1.7–2.3×
    /// the cell width (see the sibling paint test's provenance), so a naive
    /// uniform-cell division would drift +1 column per preceding wide glyph and
    /// fail — this is what pins that neither branch degrades to it.
    fn assert_emoji_columns_resolve(paint_first: bool) {
        use quadraui::{Backend as _, ScreenLayout as QScreenLayout, Surface};
        use std::cell::RefCell;
        use std::rc::Rc;

        let text = "Total: **58 commands**  \u{b7}  \u{2705} 36  \u{b7}  \u{1f7e1} 2  \u{b7}  \u{274c} 14  \u{b7}  \u{23ed}\u{fe0f} 6";
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, text);
        let buf_id = engine.active_buffer_id();
        engine.buffer_manager.get_mut(buf_id).unwrap().file_path =
            Some(std::path::PathBuf::from("notes.md"));

        let surface = ImageSurface::create(Format::ARgb32, 1000, 200).expect("ImageSurface");
        let cr = Context::new(&surface).expect("Context::new");
        let pango_ctx = pangocairo::create_context(&cr);
        let font_desc = pango::FontDescription::from_string("Monospace 12");
        pango_ctx.set_font_description(Some(&font_desc));
        let layout = pango::Layout::new(&pango_ctx);
        layout.set_font_description(Some(&font_desc));
        let metrics = pango_ctx.metrics(Some(&font_desc), None);
        let line_height = (metrics.ascent() + metrics.descent()) as f64 / pango::SCALE as f64;
        layout.set_text("0");
        let char_width = layout.pixel_size().0 as f64;

        let theme = Theme::onedark();
        let bounds = WindowRect::new(0.0, 0.0, 800.0, 600.0);
        let (rects, _) = engine.calculate_group_window_rects(bounds, (line_height * 1.6).ceil());
        let screen = build_screen_layout(&engine, &theme, &rects, line_height, char_width, false);
        let rw = &screen.windows[0];
        assert_eq!(rw.lines[0].raw_text, text, "line should not wrap");

        let backend = Rc::new(RefCell::new(super::backend::GtkBackend::new()));
        // Mirror `App::render_content`: hand the click backend an editor-fonted
        // PangoCairo context (built from a throwaway surface, NOT the paint
        // layout) so the click-time fallback is per-glyph accurate.
        {
            let click_surface =
                ImageSurface::create(Format::ARgb32, 1, 1).expect("click ImageSurface");
            let click_cr = Context::new(&click_surface).expect("click Context");
            let click_ctx = pangocairo::create_context(&click_cr);
            click_ctx.set_font_description(Some(&font_desc));
            backend.borrow_mut().set_pango_context(click_ctx);
        }

        if paint_first {
            let editor = render::to_q_editor(rw);
            let rect = editor.rect;
            let mut b = backend.borrow_mut();
            b.set_current_theme(render::to_quadraui_theme(&theme));
            b.set_current_line_height(line_height);
            b.set_current_char_width(char_width);
            b.enter_frame_scope(&cr, &layout, |b| {
                let mut frame = QScreenLayout::new();
                frame.push(Surface::Editor {
                    rect,
                    editor: &editor,
                });
                frame.draw(b);
            });
        }

        let (editor, editor_layout) = render::editor_text_layout(rw, char_width, line_height);
        let measure = pango::Layout::new(&pango_ctx);
        measure.set_font_description(Some(&font_desc));
        measure.set_text(text);

        let mut prev_pos: Option<(i32, i32)> = None;
        for (char_idx, (byte_idx, ch)) in text.char_indices().enumerate() {
            let pos = measure.index_to_pos(byte_idx as i32);
            if prev_pos == Some((pos.x(), pos.width())) {
                continue;
            }
            prev_pos = Some((pos.x(), pos.width()));

            let glyph_left_x =
                editor_layout.text_bounds.x as f64 + pos.x() as f64 / pango::SCALE as f64;
            let glyph_width = (pos.width() as f64 / pango::SCALE as f64).max(2.0);
            let click_x = glyph_left_x + glyph_width * 0.25;

            let resolved =
                backend
                    .borrow()
                    .editor_col_at_x(&editor_layout, &editor, 0, click_x as f32);
            assert_eq!(
                resolved, char_idx,
                "paint_first={paint_first}: clicking char {char_idx} ({ch:?}) resolved to \
                 col {resolved} — GtkBackend::editor_col_at_x degraded to the naive \
                 uniform-cell division instead of a per-glyph Pango layout"
            );
        }
    }

    /// Live steady-state: `last_editor_pango_layout` stashed by a real paint.
    #[test]
    fn live_trait_editor_col_at_x_resolves_exact_column_after_paint() {
        assert_emoji_columns_resolve(true);
    }

    /// #560 robustness: even with NO stashed layout, the editor-fonted
    /// `pango_ctx` (now set every frame by `draw::draw_editor`) keeps the
    /// resolution on the per-glyph Pango path instead of the naive division —
    /// so a build that hasn't painted yet, or a quadraui lacking the stash,
    /// still resolves emoji clicks exactly.
    #[test]
    fn editor_col_at_x_falls_back_to_editor_font_context_not_naive_division() {
        assert_emoji_columns_resolve(false);
    }

    /// #560 iteration 3 (the smoke failure this fix targets): plain / bold /
    /// italic / scrolled clicks landed LEFT of the target, the drift growing
    /// with `x`. Root cause: the quadraui runner paints the editor with a
    /// hardcoded "Monospace 11" (ignoring `settings.font_*`), but the previous
    /// fix fonted the click backend's Pango context from `settings.font_size`
    /// (14) — so `editor_col_at_x`'s `xy_to_index` measured against glyphs
    /// ~1.27× too wide and scaled every column down, drifting left more the
    /// further right the click. The earlier emoji tests use ONE self-consistent
    /// font for both paint and resolve, so they never caught this size split.
    ///
    /// This test reproduces the split: paint at one size, resolve through the
    /// context `App::render_content` actually builds (`build_editor_click_context`,
    /// matched to the *painted* `char_width`), and assert every column on a long
    /// plain ASCII line resolves exactly — including the far right where a
    /// size-mismatched context drifts. The `bad_drift_seen` assertion pins that
    /// a mismatched context genuinely fails, so this test can't silently pass by
    /// resolving on a too-short line.
    #[test]
    fn click_context_matches_painted_font_not_settings_size() {
        // ── The runner's painted editor font (see quadraui `gtk::run`). ──
        let paint_surface =
            ImageSurface::create(Format::ARgb32, 2000, 60).expect("paint ImageSurface");
        let pcr = Context::new(&paint_surface).expect("paint Context");
        let pctx = pangocairo::create_context(&pcr);
        let paint_font = pango::FontDescription::from_string("Monospace 11");
        pctx.set_font_description(Some(&paint_font));
        let probe = pango::Layout::new(&pctx);
        probe.set_font_description(Some(&paint_font));
        probe.set_text("0");
        let paint_cw = probe.pixel_size().0 as f64;
        let metrics = pctx.metrics(Some(&paint_font), None);
        let line_height = (metrics.ascent() + metrics.descent()) as f64 / pango::SCALE as f64;

        // ── The click context production actually builds, matched to the
        //    painted char width — NOT to any `settings.font_size`. ──
        let click_ctx = super::build_editor_click_context(paint_cw).expect("click ctx");
        let click_probe = pango::Layout::new(&click_ctx);
        click_probe.set_text("0");
        let click_cw = click_probe.pixel_size().0 as f64;
        assert!(
            (click_cw - paint_cw).abs() <= 1.0,
            "build_editor_click_context('0' adv {click_cw}) must reproduce the painted \
             char width {paint_cw}, else column resolution scales by the wrong cell width"
        );

        // ── End-to-end on a long plain ASCII line. ──
        let text = "The quick brown fox jumps over the lazy dog end AAAA BBBB CCCC DDDD EEEE";
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, text);

        let theme = Theme::onedark();
        let bounds = WindowRect::new(0.0, 0.0, 2000.0, 400.0);
        let (rects, _) = engine.calculate_group_window_rects(bounds, (line_height * 1.6).ceil());
        let screen = build_screen_layout(&engine, &theme, &rects, line_height, paint_cw, false);
        let rw = &screen.windows[0];
        assert_eq!(rw.lines[0].raw_text, text, "line should not wrap");

        let (editor, editor_layout) = render::editor_text_layout(rw, paint_cw, line_height);
        let line = &editor.lines[0];

        // Glyph geometry from the PAINT font (what draw_editor rendered with).
        let measure = pango::Layout::new(&pctx);
        measure.set_font_description(Some(&paint_font));
        measure.set_text(text);

        // The good resolver: the production click context.
        let good_layout = pango::Layout::new(&click_ctx);

        // The pre-fix bug: font the resolver from `settings.font_size` (14).
        let bad_surface = ImageSurface::create(Format::ARgb32, 1, 1).expect("bad ImageSurface");
        let bad_cr = Context::new(&bad_surface).expect("bad Context");
        let bad_ctx = pangocairo::create_context(&bad_cr);
        bad_ctx.set_font_description(Some(&pango::FontDescription::from_string("Monospace 14")));
        let bad_layout = pango::Layout::new(&bad_ctx);

        let mut bad_drift_seen = false;
        for (char_idx, (byte_idx, ch)) in text.char_indices().enumerate() {
            let pos = measure.index_to_pos(byte_idx as i32);
            let glyph_left =
                editor_layout.text_bounds.x as f64 + pos.x() as f64 / pango::SCALE as f64;
            let gw = (pos.width() as f64 / pango::SCALE as f64).max(2.0);
            let click_x = (glyph_left + gw * 0.25) as f32;

            let good = quadraui::gtk::editor_col_at_x(&good_layout, line, &editor_layout, click_x);
            assert_eq!(
                good, char_idx,
                "clicking char {char_idx} ({ch:?}) resolved to col {good} — the \
                 production click context has drifted from the painted font"
            );

            let bad = quadraui::gtk::editor_col_at_x(&bad_layout, line, &editor_layout, click_x);
            if bad != char_idx {
                bad_drift_seen = true;
            }
        }
        assert!(
            bad_drift_seen,
            "a size-mismatched click context (the pre-fix bug) must drift on this line, \
             else the test can't prove the width-match is what fixes it"
        );
    }
}

#[cfg(test)]
mod cross_split_drag_focus_tests {
    //! #568 regression: dragging a text selection in one editor group (a
    //! GTK split pane created via the tab bar's split button /
    //! `Engine::open_editor_group`, i.e. VS Code-style side-by-side panes)
    //! must not steal focus to a neighboring group's window merely because
    //! the mouse passes over it while the button is held.
    //!
    //! `Engine::mouse_drag`'s origin-window lock (`mouse_drag_origin_window`)
    //! already keeps the selection *data* pinned to the originating window
    //! — see the core-level `test_mouse_drag_locked_to_origin_window`. But
    //! GTK's `pixel_to_click_target` used to call
    //! `engine.activate_group_for_window(window_id)` unconditionally, as a
    //! side effect of resolving ANY pixel position — including drag
    //! continuation. That flipped `engine.active_group` (and therefore
    //! `active_window_id()`) to the neighboring pane just from hovering over
    //! it mid-drag, which made `render::build_selection`'s `is_active` gate
    //! light up the wrong pane's selection overlay even though the
    //! underlying selection state never actually changed. This pins that a
    //! drag-continuation hit-test (`mutate_focus: false`) leaves
    //! `active_group`/`active_window_id()` untouched, while a genuine click
    //! (`mutate_focus: true`) still focuses the pane it lands in.
    use super::*;
    use crate::render::build_screen_layout;

    fn empty_maps() -> (
        TabPixelHitMap,
        TabSlotMap,
        DiffBtnMap,
        SplitBtnMap,
        ActionBtnMap,
    ) {
        (
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
        )
    }

    #[test]
    fn drag_continuation_does_not_steal_focus_to_neighboring_group() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        let wid_a = engine.active_window_id();
        let group_a = engine.active_group;

        // Open a second editor group side-by-side — GTK's split-pane
        // feature (bound to the tab bar's split button). `open_editor_group`
        // makes the new group/window active.
        engine.open_editor_group(crate::core::window::SplitDirection::Vertical);
        let group_b = engine.active_group;
        let wid_b = engine.active_window_id();
        assert_ne!(group_a, group_b);
        assert_ne!(wid_a, wid_b);

        // Simulate the user clicking back into the left pane to start the drag.
        engine.mouse_click(wid_a, 0, 1);
        assert_eq!(engine.active_group, group_a);
        assert_eq!(engine.active_window_id(), wid_a);
        engine.mouse_drag(wid_a, 0, 4);
        assert!(engine.mouse_drag_active);
        assert_eq!(engine.mouse_drag_origin_window, Some(wid_a));

        // Lay out both panes side by side and locate each window's rect.
        let theme = Theme::onedark();
        let bounds = core::WindowRect::new(0.0, 0.0, 1600.0, 400.0);
        let line_height: f64 = 18.0;
        let char_width: f64 = 9.0;
        let (rects, _) = engine.calculate_group_window_rects(bounds, (line_height * 1.6).ceil());
        let screen = build_screen_layout(&engine, &theme, &rects, line_height, char_width, false);
        let rw_b = screen
            .windows
            .iter()
            .find(|w| w.window_id == wid_b)
            .expect("window B should be laid out");
        // A pixel comfortably inside window B's text area (below its tab bar).
        let x_in_b = rw_b.rect.x + char_width * 2.0;
        let y_in_b = rw_b.rect.y + line_height * 2.0;

        let backend = Rc::new(RefCell::new(super::super::backend::GtkBackend::new()));
        let (tab_pixel_hits, tab_slot_positions, diff_btn_map, split_btn_map, action_btn_map) =
            empty_maps();

        // Drag continuation: the mouse is over window B's pixels, but this
        // must resolve as a pure hit-test — no focus/group side effects.
        let target = pixel_to_click_target(
            &mut engine,
            &backend,
            x_in_b,
            y_in_b,
            line_height,
            char_width,
            &screen,
            &tab_pixel_hits,
            &tab_slot_positions,
            &diff_btn_map,
            &split_btn_map,
            &action_btn_map,
            None, // no cached FrameHitMap in this test — exercises the
            // `screen_zone_hit_test` fallback path (#449)
            &HashMap::new(),
            false, // mutate_focus: drag continuation
        );
        assert_eq!(
            engine.active_group, group_a,
            "a held drag sweeping over the neighboring group must not steal active_group"
        );
        assert_eq!(
            engine.active_window_id(),
            wid_a,
            "a held drag sweeping over the neighboring group must not steal active_window_id \
             (render::build_selection's is_active gate keys off this)"
        );
        match target {
            ClickTarget::BufferPos(wid, _, _) => {
                assert_eq!(
                    wid, wid_b,
                    "the hit-test should still resolve the real window under the cursor"
                )
            }
            other => panic!("expected a BufferPos hit in window B's text area, got {other:?}"),
        }
        // The engine-level origin lock (already covered by
        // `test_mouse_drag_locked_to_origin_window`) rejects this mismatched
        // window_id, so the selection itself stays anchored to window A.
        engine.mouse_drag(wid_b, 0, 8);
        assert_eq!(engine.mouse_drag_origin_window, Some(wid_a));

        // Contrast: a genuine click landing in window B (mutate_focus: true)
        // — as a real MouseClick/DoubleClick event would — SHOULD focus it.
        // This proves the flag actually gates behavior rather than being a
        // no-op, and that real clicks keep working as before.
        let click_target = pixel_to_click_target(
            &mut engine,
            &backend,
            x_in_b,
            y_in_b,
            line_height,
            char_width,
            &screen,
            &tab_pixel_hits,
            &tab_slot_positions,
            &diff_btn_map,
            &split_btn_map,
            &action_btn_map,
            None,
            &HashMap::new(),
            true, // mutate_focus: genuine click
        );
        assert_eq!(
            engine.active_group, group_b,
            "a genuine click must still focus the pane it lands in"
        );
        assert!(matches!(click_target, ClickTarget::BufferPos(wid, _, _) if wid == wid_b));
    }
}

#[cfg(test)]
mod frame_hit_map_tests {
    //! #449 regression: `frame_zone_to_screen_zone` and the `frame_hit_map`
    //! branch of `pixel_to_click_target` are the actual mechanism this issue
    //! introduced — mapping `quadraui::FrameZone::TabBar { idx }` /
    //! `FrameZone::Editor { idx }` back to `render::ScreenZone`. These tests
    //! build a *real* `quadraui::FrameHitMap` via `ScreenLayout::hit_map()`
    //! (quadraui#425, landed as `c316f15`) from the same `Surface::Editor` /
    //! `Surface::TabBar` construction `App::render_content` uses
    //! (`src/gtk/mod.rs` ~7712-7830), instead of exercising only the
    //! pre-existing `screen_zone_hit_test` fallback (as the two tests in
    //! `cross_split_drag_focus_tests` above do by passing `None, &[]`).
    use super::*;
    use crate::render::build_screen_layout;
    use quadraui::{ScreenLayout as QSL, Surface};

    /// Lay out a single window / single (unsplit) tab bar and build the
    /// `FrameHitMap` + `tab_bar_zones` table the way `render_content` does
    /// (`src/gtk/mod.rs` ~7712-7839), so these tests exercise the production
    /// construction, not a hand-rolled stand-in — crucially including the
    /// same "editors pushed first, tab bars after" ordering, since
    /// `FrameZone::TabBar { idx }` carries the *global* surface index across
    /// that whole `ScreenLayout`, not a per-tab-bar position. `tab_bar_zones`
    /// must therefore be keyed by that same global index, not `0..`.
    fn build_hit_map(
        engine: &Engine,
        theme: &Theme,
        line_height: f64,
        char_width: f64,
    ) -> (
        render::ScreenLayout,
        quadraui::FrameHitMap,
        HashMap<usize, (GroupId, quadraui::Rect)>,
    ) {
        let bounds = WindowRect::new(0.0, 0.0, 800.0, 600.0);
        let (rects, _) = engine.calculate_group_window_rects(bounds, (line_height * 1.6).ceil());
        let screen = build_screen_layout(engine, theme, &rects, line_height, char_width, false);

        let window_editors: Vec<quadraui::Editor> =
            screen.windows.iter().map(render_mod::to_q_editor).collect();
        let mut hit_frame = QSL::new();
        for editor in &window_editors {
            hit_frame.push(Surface::Editor {
                rect: editor.rect,
                editor,
            });
        }

        let tab_row_h = render_mod::tab_row_height_px(line_height);
        let tab_bar_h = render_mod::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
        let mut tab_bar_zones: HashMap<usize, (GroupId, quadraui::Rect)> = HashMap::new();
        for (next_surface_idx, target) in (window_editors.len()..).zip(
            render_mod::tab_bar_draw_targets(engine, &screen, tab_row_h, tab_bar_h),
        ) {
            hit_frame.push(Surface::TabBar {
                rect: target.rect,
                bar: target.bar,
                hovered_close: None,
            });
            tab_bar_zones.insert(next_surface_idx, (target.group_id, target.rect));
        }

        let hit_map = hit_frame.hit_map();
        (screen, hit_map, tab_bar_zones)
    }

    #[test]
    fn frame_zone_to_screen_zone_resolves_editor_point() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        let wid = engine.active_window_id();
        let theme = Theme::onedark();
        let line_height = 18.0;
        let char_width = 9.0;

        let (screen, hit_map, tab_bar_zones) =
            build_hit_map(&engine, &theme, line_height, char_width);
        let rw = screen
            .windows
            .first()
            .expect("single window should be laid out");
        let x = rw.rect.x + char_width * 2.0;
        let y = rw.rect.y + line_height * 2.0;

        let zone = frame_zone_to_screen_zone(&hit_map, &tab_bar_zones, &screen, x, y);
        match zone {
            ScreenZone::Window {
                window_id,
                window_idx,
                ..
            } => {
                assert_eq!(window_id, wid);
                assert_eq!(window_idx, 0);
            }
            other => panic!("expected ScreenZone::Window from the FrameHitMap path, got {other:?}"),
        }
    }

    #[test]
    fn frame_zone_to_screen_zone_resolves_tab_bar_point() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        let group_id = engine.active_group;
        let theme = Theme::onedark();
        let line_height = 18.0;
        let char_width = 9.0;

        let (screen, hit_map, tab_bar_zones) =
            build_hit_map(&engine, &theme, line_height, char_width);
        assert!(
            !tab_bar_zones.is_empty(),
            "single-group mode should still push one tab bar surface"
        );
        let (zone_group, rect) = *tab_bar_zones
            .values()
            .next()
            .expect("just asserted tab_bar_zones is non-empty");
        assert_eq!(zone_group, group_id);
        let x = rect.x as f64 + 2.0;
        let y = rect.y as f64 + 2.0;

        let zone = frame_zone_to_screen_zone(&hit_map, &tab_bar_zones, &screen, x, y);
        match zone {
            ScreenZone::TabBar {
                group_id: resolved, ..
            } => assert_eq!(resolved, group_id),
            other => {
                panic!("expected ScreenZone::TabBar from the FrameHitMap path, got {other:?}")
            }
        }
    }

    #[test]
    fn pixel_to_click_target_consults_the_cached_frame_hit_map_for_editor_clicks() {
        // Proves `pixel_to_click_target` actually takes the `frame_hit_map`
        // branch (not silently falling through to `screen_zone_hit_test`)
        // when a real `Some(&FrameHitMap)` is supplied — the production
        // `Some` path exercised by `render_content`'s cached hit map, as
        // opposed to the `None` fallback path already covered by
        // `cross_split_drag_focus_tests`.
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        let wid = engine.active_window_id();
        let theme = Theme::onedark();
        let line_height = 18.0;
        let char_width = 9.0;

        let (screen, hit_map, tab_bar_zones) =
            build_hit_map(&engine, &theme, line_height, char_width);
        let rw = screen
            .windows
            .first()
            .expect("single window should be laid out");
        let x = rw.rect.x + char_width * 2.0;
        let y = rw.rect.y + line_height * 2.0;

        let backend = Rc::new(RefCell::new(super::super::backend::GtkBackend::new()));
        let empty_pixel_hits: TabPixelHitMap = HashMap::new();
        let empty_slots: TabSlotMap = HashMap::new();
        let empty_diff: DiffBtnMap = HashMap::new();
        let empty_split: SplitBtnMap = HashMap::new();
        let empty_action: ActionBtnMap = HashMap::new();

        let target = pixel_to_click_target(
            &mut engine,
            &backend,
            x,
            y,
            line_height,
            char_width,
            &screen,
            &empty_pixel_hits,
            &empty_slots,
            &empty_diff,
            &empty_split,
            &empty_action,
            Some(&hit_map),
            &tab_bar_zones,
            true,
        );
        match target {
            ClickTarget::BufferPos(id, _, _) => assert_eq!(id, wid),
            other => panic!(
                "expected a BufferPos hit resolved via the cached FrameHitMap, got {other:?}"
            ),
        }
    }
}

#[cfg(test)]
mod single_group_tab_click_dispatch_tests {
    //! #553 regression, pinned at the GTK **click-dispatch entry point** the
    //! issue's root-cause hint names (`pixel_to_click_target`, this file):
    //! with ONE tab group, clicking a non-active tab did not activate it and
    //! clicking a tab's × did not close it, while two or more groups worked.
    //!
    //! The defect lived in `screen_zone_hit_test`'s single-group arm, which
    //! hardcoded the tab row's top at the coordinate-system origin (`y >= 0.0`)
    //! instead of deriving it from the window rects the way the split arm did.
    //! Once #552 gave GTK a persistent menu/title-bar band the content origin
    //! moved down, so the single-group band pointed at chrome pixels no tab was
    //! ever drawn on — and every single-group tab click resolved to
    //! `ScreenZone::None` → `ClickTarget::None`. `render::tab_bar_hit_bands`
    //! (this PR) is what now forces both shapes through one derivation.
    //!
    //! Two things make these tests discriminate where the black-box
    //! `gtk::testing` pair does not (see the note in that module and in the PR
    //! description):
    //!
    //! 1. `frame_hit_map: None` — forcing the `screen_zone_hit_test` fallback
    //!    branch. GTK's production routing prefers the cached
    //!    `quadraui::FrameHitMap` (#449) and only falls back here on a hit-map
    //!    miss / before the first paint, so a driver-level click never reaches
    //!    the code under test. Same technique, same rationale as
    //!    `cross_split_drag_focus_tests::drag_continuation_does_not_steal_focus_to_neighboring_group`
    //!    above.
    //! 2. A **synthetic 100px content offset**, matching
    //!    `render::tests::test_tab_bar_hit_bands_single_and_split_share_one_derivation`.
    //!    The headless harness's default title-bar chrome only shifts the
    //!    content origin ~23px, which is small enough that the painted click y
    //!    falls inside *both* the correct band and the buggy hardcoded one —
    //!    the offset has to exceed the bar height to separate them.
    //!
    //! The activate and close cases are separate `#[test]`s deliberately: with
    //! the pre-`8fbbf85` bug reinstated in `render::tab_bar_hit_bands`'s
    //! single-group arm (`y: 0.0` instead of `y: min_y - tab_bar_height`) BOTH
    //! go red independently with `ClickTarget::None`, which a single test with
    //! two sequential assertions could not show (it would panic on the first
    //! and never reach the second). That FAIL/PASS pair is reproduced in the PR
    //! description.
    use super::*;
    use crate::render::build_screen_layout;

    /// Chrome-shifted editor content origin — the #552 menu/title-bar band, at
    /// an offset large enough to separate the correct band from the buggy one.
    const CONTENT_X: f64 = 50.0;
    const CONTENT_Y: f64 = 100.0;
    const CONTENT_W: f64 = 800.0;
    const CONTENT_H: f64 = 600.0;
    /// Synthetic per-tab pixel width, bar-relative (see [`synthetic_pixel_hits`]).
    const TAB_W: f64 = 120.0;

    /// The `TabBarPixelHits` the rasteriser would have cached for a single
    /// group with `tabs` tabs: contiguous `TAB_W`-wide slots from the bar's left
    /// edge, each with a 15px close (`×`) zone inset near its right edge.
    ///
    /// Bar-relative, exactly like `tab_hits_to_pixel_hits`'s output — which is
    /// what `pixel_to_click_target` matches `ScreenZone::TabBar { local_x }`
    /// against. Synthetic rather than rasterised so the test states its own
    /// geometry instead of depending on Pango font metrics.
    fn synthetic_pixel_hits(tabs: usize) -> TabBarPixelHits {
        TabBarPixelHits {
            slots: (0..tabs)
                .map(|i| (i as f64 * TAB_W, (i + 1) as f64 * TAB_W))
                .collect(),
            close: (0..tabs)
                .map(|i| Some((i as f64 * TAB_W + 100.0, i as f64 * TAB_W + 115.0)))
                .collect(),
            segments: Vec::new(),
        }
    }

    /// Bar-relative x of a point inside tab `idx`'s body, clear of its × zone.
    fn tab_body_local_x(idx: usize) -> f64 {
        idx as f64 * TAB_W + 20.0
    }

    /// Bar-relative x of a point inside tab `idx`'s × zone.
    fn tab_close_local_x(idx: usize) -> f64 {
        idx as f64 * TAB_W + 107.0
    }

    /// Three tabs in the default SINGLE editor group — the exact shape #553
    /// reports as dead — laid out at the chrome-shifted content origin, plus
    /// the cached rasteriser geometry and the empty legacy maps
    /// `pixel_to_click_target` still takes.
    ///
    /// No buffer edits anywhere, so a close click isn't diverted into the
    /// dirty-buffer confirm dialog.
    struct Fixture {
        engine: Engine,
        group: GroupId,
        screen: render::ScreenLayout,
        tab_pixel_hits: TabPixelHitMap,
        backend: Rc<RefCell<super::super::backend::GtkBackend>>,
        line_height: f64,
        char_width: f64,
    }

    impl Fixture {
        fn new() -> Self {
            let mut engine = Engine::new();
            engine.new_tab(None);
            engine.new_tab(None);
            let group = engine.active_group;
            assert_eq!(engine.editor_groups[&group].tabs.len(), 3);
            assert_eq!(
                engine.editor_groups[&group].active_tab, 2,
                "`new_tab` activates the tab it creates"
            );

            let theme = Theme::onedark();
            let line_height: f64 = 20.0;
            let char_width: f64 = 8.0;
            let tab_bar_height =
                render_mod::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
            let content = core::WindowRect::new(CONTENT_X, CONTENT_Y, CONTENT_W, CONTENT_H);
            let (rects, _) = engine.calculate_group_window_rects(content, tab_bar_height);
            let screen =
                build_screen_layout(&engine, &theme, &rects, line_height, char_width, false);
            assert!(
                screen.editor_group_split.is_none(),
                "these tests must exercise the single-group arm; a split layout would \
                 take the branch that never regressed"
            );

            let mut tab_pixel_hits: TabPixelHitMap = HashMap::new();
            tab_pixel_hits.insert(group.0, synthetic_pixel_hits(3));

            Self {
                engine,
                group,
                screen,
                tab_pixel_hits,
                backend: Rc::new(RefCell::new(super::super::backend::GtkBackend::new())),
                line_height,
                char_width,
            }
        }

        /// The tab row sits immediately ABOVE the window content, i.e. in
        /// `[CONTENT_Y, CONTENT_Y + tab_bar_height)`. The pre-fix code looked
        /// for it in `[0, tab_bar_height)`, which at this offset holds no tab
        /// pixels at all.
        const CLICK_Y: f64 = CONTENT_Y + 2.0;

        /// Resolve a tab-bar click through the production GTK dispatch entry
        /// point, with `frame_hit_map: None` to force the `screen_zone_hit_test`
        /// fallback branch #553 lives in (see this module's doc comment).
        fn click(&mut self, local_x: f64) -> ClickTarget {
            pixel_to_click_target(
                &mut self.engine,
                &self.backend,
                CONTENT_X + local_x,
                Self::CLICK_Y,
                self.line_height,
                self.char_width,
                &self.screen,
                &self.tab_pixel_hits,
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                None,
                &HashMap::new(),
                true, // a genuine click
            )
        }

        /// The same pixel driven through the real click handler —
        /// `handle_mouse_click` is the production caller that turns
        /// `ClickTarget::CloseTab` into `Engine::close_tab`.
        fn full_click(&mut self, local_x: f64) -> (Option<bool>, Option<EngineAction>) {
            handle_mouse_click(
                &mut self.engine,
                &self.backend,
                CONTENT_X + local_x,
                Self::CLICK_Y,
                false, // alt
                self.line_height,
                self.char_width,
                &self.screen,
                &self.tab_pixel_hits,
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                None,
                &HashMap::new(),
            )
        }
    }

    /// #553, half one: clicking a non-active tab in a single-group layout must
    /// resolve as a tab-bar hit and activate that tab.
    #[test]
    fn single_group_tab_click_activates_that_tab_via_click_dispatch() {
        let mut f = Fixture::new();
        let group = f.group;

        let target = f.click(tab_body_local_x(0));
        assert!(
            matches!(target, ClickTarget::TabBar),
            "a single-group click on tab 0's body must resolve as a tab-bar hit, got {target:?} \
             (pre-fix this was ClickTarget::None — the click missed the bar entirely)"
        );
        assert_eq!(
            f.engine.editor_groups[&group].active_tab, 0,
            "clicking tab 0 in a single-group layout must activate it (#553)"
        );
    }

    /// #553, half two: clicking a tab's × in a single-group layout must resolve
    /// to `CloseTab` for that tab, and actually close it through the production
    /// click handler.
    #[test]
    fn single_group_tab_close_click_targets_and_closes_that_tab_via_click_dispatch() {
        let mut f = Fixture::new();
        let group = f.group;

        let target = f.click(tab_close_local_x(1));
        assert_eq!(
            target,
            ClickTarget::CloseTab(group, 1),
            "a single-group click on tab 1's × must resolve to CloseTab for tab 1 \
             (pre-fix: ClickTarget::None, so nothing ever closed)"
        );

        let before = f.engine.editor_groups[&group].tabs.len();
        let (dirty_confirm, _) = f.full_click(tab_close_local_x(1));
        assert_eq!(
            dirty_confirm, None,
            "fixture buffers are unmodified, so no dirty-buffer confirm should intercept the close"
        );
        assert_eq!(
            f.engine.editor_groups[&group].tabs.len(),
            before - 1,
            "clicking a tab's × in a single-group layout must close it (#553)"
        );
    }
}
