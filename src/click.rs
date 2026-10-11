//! Backend-neutral pixel→click-target resolution.
//!
//! These functions touch only `quadraui::Backend`, `Engine` and pure
//! geometry — never GTK-specific types. `src/gtk/click.rs` re-exports
//! everything below so GTK tests and code keep resolving these names
//! unchanged; the one function that genuinely needs GTK
//! (`build_editor_click_context`, which builds a `pango::Context`)
//! stays in `src/gtk/click.rs`.
//!
//! Tab-bar hit-testing uses `quadraui::TabBarLayout::hit_test` — see
//! [`GroupTabBarLayoutMap`].

use crate::core::engine::EngineAction;
use crate::core::window::GroupId;
use crate::core::{Engine, WindowId};
use crate::render;
use crate::render::{self as render_mod, ScreenZone, WindowZone};
use std::collections::HashMap;

/// Re-export the shared ClickTarget enum.
pub(crate) use render_mod::ClickTarget;

/// Per-group pixel-accurate tab-bar layout from the most recent paint:
/// the exact `(Rect, TabBarLayout)` the rasteriser drew, cached so clicks
/// resolve against what's actually on screen.
///
/// `Rect` is the bar's absolute origin; `TabBarLayout`'s own geometry
/// (`visible_tabs`/`visible_segments`/`hit_regions`) is bar-**relative**
/// (see that type's doc) — the same space `render::screen_zone_hit_test`'s
/// `local_x` uses, so [`render::resolve_tab_bar_click`] can hit-test
/// directly against it with no further shifting. The stored `Rect` is only
/// needed to translate that bar-relative geometry back to absolute screen
/// space for consumers outside the click path (tab-drop geometry, the
/// engine's visible-tab-count feedback — see
/// [`abs_slot_positions_from_layout`] / [`tab_bar_available_cols`]).
///
/// Key = `group_id.0` (single-group mode keys under the active group's id,
/// which is what `screen_zone_hit_test` reports for it).
pub(crate) type GroupTabBarLayoutMap = HashMap<usize, (quadraui::Rect, quadraui::TabBarLayout)>;

/// Absolute x-ranges for every tab slot in a group's just-painted
/// `TabBarLayout`, index-aligned to the group's own tab list, with a
/// `(0.0, 0.0)` sentinel left in place for any tab scrolled off the strip —
/// the convention `quadraui::DropGroupRect::tab_slots` /
/// `PaneDragRect::tab_slots` document natively, and what
/// `render::build_tab_drop_ctx` consumes.
///
/// `rect` is the bar's own absolute origin (see [`GroupTabBarLayoutMap`]);
/// `layout.visible_tabs[].bounds` is bar-relative, so this shifts each one
/// back to absolute screen space.
pub(crate) fn abs_slot_positions_from_layout(
    rect: quadraui::Rect,
    bar: &quadraui::TabBar,
    layout: &quadraui::TabBarLayout,
) -> Vec<(f32, f32)> {
    let mut slots = vec![(0.0_f32, 0.0_f32); bar.tabs.len()];
    for vt in &layout.visible_tabs {
        if let Some(slot) = slots.get_mut(vt.tab_idx) {
            *slot = (rect.x + vt.bounds.x, rect.x + vt.bounds.x + vt.bounds.width);
        }
    }
    slots
}

/// Tab-bar content width in character columns, derived from this frame's
/// painted geometry — the budget `Engine::set_tab_visible_count` uses for
/// tab visibility, even on a proportional-font backend.
///
/// Computed from the bar's painted width minus whatever right-aligned
/// segments the paint actually drew, divided by `char_width` (`1.0` on
/// TUI, so this reduces to the exact cell count; `Backend::char_width()`'s
/// Pango `approximate_char_width` on GTK, close enough for a visibility
/// budget that was always an estimate).
pub(crate) fn tab_bar_available_cols(
    rect: quadraui::Rect,
    layout: &quadraui::TabBarLayout,
    char_width: f32,
) -> usize {
    let reserved: f32 = layout.visible_segments.iter().map(|s| s.bounds.width).sum();
    let effective = (rect.width - reserved).max(0.0);
    (effective / char_width.max(1.0)).floor().max(0.0) as usize
}

/// Convert pixel (x, y) to a click target using the cached ScreenLayout
/// from the last paint pass. Zone detection delegates to the shared
/// `screen_zone_hit_test` / `window_zone_hit_test` / `resolve_gutter_action`
/// functions in render.rs so both backends use one source of truth.
///
/// Tab bar inner hit-testing (which specific tab/button) stays here because
/// it uses the pixel slot positions from `draw_tab_bar`.
///
/// The text-area column is resolved via `backend.editor_col_at_x` (quadraui)
/// — the same Pango layout + attributes `draw_editor` painted with — so
/// paint and click never drift.
///
/// `mutate_focus` gates every side effect this function performs: flipping
/// `active_group`/the active tab, and executing a gutter action (e.g.
/// toggling a breakpoint). Real clicks pass `true`, since landing on a pane
/// or tab should focus it. `handle_mouse_drag` passes `false`: while a
/// text-selection drag is held down, the mouse sweeping over a *different*
/// split's tab bar or gutter must not steal focus or fire actions there.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pixel_to_click_target(
    engine: &mut Engine,
    backend: &dyn quadraui::Backend,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    // Pixel-accurate per-group tab-bar layout captured from the rasteriser
    // during `render_content` (via `Backend::draw_tab_bar_icons_layout`).
    // GTK draws tabs with proportional-font Pango widths, so the char-cell
    // `hit_regions` on `cached_layout` do NOT match the drawn geometry — clicks
    // must resolve against this actual painted geometry instead.
    group_tab_bar_layouts: &GroupTabBarLayoutMap,
    // Cached `quadraui::FrameHitMap` covering the Editor/TabBar surfaces
    // painted this frame, plus a `FrameZone::TabBar { idx } -> (GroupId,
    // rect)` table keyed by the tab bar's *global* surface index (editors are
    // pushed into the same `ScreenLayout` before any tab bar, so a tab bar's
    // `idx` is offset by however many editor surfaces preceded it — a plain
    // 0-based `Vec` here would look up the wrong entry, or none at all).
    // `None` before the first paint. See `frame_zone_to_screen_zone` for how
    // these replace `screen_zone_hit_test`'s manual Window/TabBar rect-walk.
    frame_hit_map: Option<&quadraui::FrameHitMap>,
    tab_bar_zones: &HashMap<usize, (GroupId, quadraui::Rect)>,
    mutate_focus: bool,
    // The shared `quadraui::DragState` a real press arms so a
    // following drag-move traverses the whole file instead of re-seeking
    // per move — see `render::minimap_press`'s doc comment. Only ever
    // touched under `mutate_focus` (a hover/drag-continuation query must
    // never arm a new gesture), so callers that never pass `true` (there are
    // none left, but the parameter still has to exist for every call site)
    // could in principle thread a scratch `DragState`; every real call site
    // threads its own backend's live one.
    drag: &mut quadraui::DragState,
    // Alt held at press time — inverted and threaded into
    // `render::minimap_press`'s `fine` parameter below (`!alt`, see its
    // doc). A plain minimap drag maps within the minimap's own scale by
    // default, matching VS Code — holding Alt opts into the file-wide fast-scroll
    // mapping instead. Irrelevant to every other rung below the minimap, so
    // callers with no Alt context pass `false`.
    alt: bool,
) -> ClickTarget {
    // The separated status line and per-window status line are now walked by
    // `render::route_chrome_click`, which `App::handle_mouse_click_msg` runs
    // before this function — so a status click cannot arrive here, and the
    // shared router decides the order the bars are arbitrated in.

    // ── Minimap press ───────────────────────────────────────────────────────
    // Pure rect plumbing: the shared resolver owns the hit-test and the
    // jump-to-position. Checked before the zone walk because every window's
    // strip is carved out of that window's own rect, so a `ScreenZone::Window`
    // hit would otherwise swallow it. Gated on `mutate_focus` so a hover
    // query never scrolls or arms a drag. `minimap_press` resolves against
    // *every* pane's strip and reports which one it hit — never assumed to be
    // the active window, since a split can have a strip on an inactive pane too.
    if mutate_focus {
        // A plain press/drag defaults to the minimap's own scale, matching
        // VS Code; Alt held opts into the file-wide mapping instead. See
        // `render::minimap_press`'s doc.
        if let Some(press) = render_mod::minimap_press(engine, cached_layout, x, y, !alt) {
            if press.jump {
                render_mod::apply_minimap_click(engine, cached_layout, x, y);
            } else {
                // A press directly on the highlight band skips
                // `apply_minimap_click`'s centring (nothing should scroll
                // yet — that's the whole point of preserving the grab
                // offset), but a click on a *background* pane's strip must
                // still focus that pane, exactly like every other click
                // does. `apply_minimap_click`'s `jump` branch above already
                // covers this itself.
                engine.activate_window(press.window_id);
            }
            drag.begin(quadraui::DragTarget::ScrollbarY {
                widget: render_mod::minimap_drag_widget(press.window_id),
                track_start: press.track_start,
                track_length: press.track_length,
                thumb_length: press.thumb_length,
                max_scroll: press.max_scroll,
                grab_offset: press.grab_offset,
                inverted: false,
            });
            return ClickTarget::Minimap(press.window_id, 0);
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
                // switch tabs/focus there — treat it as a miss.
                return ClickTarget::None;
            }
            engine.active_group = group_id;
            tab_bar_inner_hit_test(
                engine,
                group_id,
                local_x,
                char_width,
                cached_layout,
                group_tab_bar_layouts,
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
                    view_row,
                    line_idx,
                    gutter_col,
                } => {
                    if !mutate_focus {
                        // A drag sweeping over another split's gutter must
                        // not fire gutter actions (e.g. toggle a breakpoint)
                        // there.
                        return ClickTarget::None;
                    }
                    execute_gutter_action(engine, rw, window_id, view_row, line_idx, gutter_col);
                    ClickTarget::Gutter
                }
                WindowZone::TextArea {
                    view_row, buf_line, ..
                } => {
                    // Resolve the exact column via the shared
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
                    let col = backend.editor_col_at_x(&editor_layout, &editor, view_row, x as f32);
                    ClickTarget::BufferPos(window_id, buf_line, col)
                }
                _ => ClickTarget::None,
            }
        }
        _ => ClickTarget::None,
    }
}

/// Resolve the top-level `ScreenZone` using the cached `quadraui::FrameHitMap`,
/// which covers exactly the `Editor`/`TabBar` surfaces painted in
/// `App::render_content` via `quadraui::ScreenLayout::hit_map()`
/// — pushed from the SAME objects/rects already painted, so
/// this can never drift from what's on screen. Returns `ScreenZone::None`
/// when the point isn't in an Editor/TabBar zone (including breadcrumb/
/// divider pixels, which have no `FrameZone` equivalent — the caller falls
/// back to `render_mod::screen_zone_hit_test` for those).
pub(crate) fn frame_zone_to_screen_zone(
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

/// Tab bar inner hit-test: resolve which tab/button a click hit.
///
/// `local_x` is in the tab bar's own unit, relative to its left edge —
/// pixels for GTK, char cells for TUI. For GTK we resolve against the
/// pixel-accurate `TabBarLayout` the rasteriser painted this frame
/// (`group_tab_bar_layouts`, captured in `render_content` via
/// `Backend::draw_tab_bar_icons_layout`). GTK tabs are laid out with
/// proportional-font Pango widths + fixed pixel padding, so the char-cell
/// `hit_regions` (correct for TUI) badly mis-measure them. Falls back to
/// the char-cell path only if no pixel geometry was cached (e.g. a click
/// before the first paint).
fn tab_bar_inner_hit_test(
    engine: &mut Engine,
    group_id: GroupId,
    local_x: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    group_tab_bar_layouts: &GroupTabBarLayoutMap,
) -> ClickTarget {
    let target = group_tab_bar_layouts
        .get(&group_id.0)
        .and_then(|(_, layout)| render_mod::resolve_tab_bar_click(layout, local_x as f32))
        .or_else(|| resolve_charcell_tab_click(cached_layout, group_id, local_x, char_width));

    dispatch_tab_bar_target(engine, group_id, target)
}

/// Char-cell fallback (matches the TUI monospace layout). Only used before the
/// first paint has populated the pixel-accurate layout cache.
fn resolve_charcell_tab_click(
    cached_layout: &render::ScreenLayout,
    group_id: GroupId,
    local_x: f64,
    char_width: f64,
) -> Option<crate::core::engine::TabBarClickTarget> {
    let col = (local_x / char_width).floor().max(0.0) as u16;
    let layout: Option<&quadraui::TabBarLayout> = if cached_layout.editor_group_split.is_some() {
        cached_layout
            .group_tab_bars
            .iter()
            .find(|g| g.group_id == group_id)
            .map(|g| &g.hit_regions)
    } else {
        Some(&cached_layout.tab_bar_hit_regions)
    };
    layout.and_then(|l| render_mod::resolve_tab_bar_click(l, col as f32))
}

/// Resolve which tab (if any) a right-click landed on, without any of the
/// side effects `tab_bar_inner_hit_test` applies (selecting the tab,
/// closing it, ...). This mirrors `pixel_to_click_target`'s zone resolution
/// (read-only) so the caller can tell a tab-bar right-click apart from an
/// editor right-click and decide which message to dispatch.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_tab_right_click(
    engine: &Engine,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    group_tab_bar_layouts: &GroupTabBarLayoutMap,
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
    let target = group_tab_bar_layouts
        .get(&group_id.0)
        .and_then(|(_, layout)| render_mod::resolve_tab_bar_click(layout, local_x as f32))
        .or_else(|| resolve_charcell_tab_click(cached_layout, group_id, local_x, char_width));
    match target {
        Some(T::Tab(idx)) | Some(T::CloseTab(idx)) => Some((group_id, idx)),
        _ => None,
    }
}

/// Resolve a tab-bar click target and apply it through
/// `Engine::handle_tab_bar_click` — the single dispatch the engine, TUI
/// and GTK all share.
///
/// The two exceptions:
/// - `ActionMenu` needs the click's screen coordinates to place the popup,
///   which the engine doesn't have — so `handle_mouse_click`'s
///   `ActionMenuButton` arm opens the menu directly instead.
/// - `CloseTab` on a dirty buffer must defer to a confirmation dialog
///   before closing — so resolution only identifies *which* tab was
///   targeted.
///
/// `pub(crate)` so `tui_main/mouse.rs` can call it directly.
pub(crate) fn dispatch_tab_bar_target(
    engine: &mut Engine,
    group_id: GroupId,
    target: Option<crate::core::engine::TabBarClickTarget>,
) -> ClickTarget {
    use crate::core::engine::TabBarClickTarget as T;

    match target {
        Some(T::ActionMenu) => ClickTarget::ActionMenuButton(group_id),
        Some(T::CloseTab(idx)) => ClickTarget::CloseTab(group_id, idx),
        Some(t) => {
            engine.handle_tab_bar_click(group_id, t);
            ClickTarget::TabBar
        }
        None => ClickTarget::TabBar,
    }
}

/// Execute the engine-side action for a gutter click using shared
/// resolution via [`render::apply_gutter_action`].
fn execute_gutter_action(
    engine: &mut Engine,
    rw: &render::RenderedWindow,
    window_id: WindowId,
    view_row: usize,
    line_idx: usize,
    gutter_col: usize,
) {
    let gutter_text = rw
        .lines
        .get(view_row)
        .map(|rl| rl.gutter_text.as_str())
        .unwrap_or_default();
    render_mod::apply_gutter_action(engine, rw, window_id, line_idx, gutter_col, gutter_text);
}

/// Handle mouse click by converting coordinates to buffer position.
/// Returns: `(click, engine_action)` where click is `None` = non-buffer click,
/// `Some(true)` = close-tab on dirty buffer, `Some(false)` = normal buffer click;
/// `engine_action` is an optional action the caller must dispatch (e.g. sidebar toggle).
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_mouse_click(
    engine: &mut Engine,
    backend: &dyn quadraui::Backend,
    x: f64,
    y: f64,
    alt: bool,
    line_height: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    group_tab_bar_layouts: &GroupTabBarLayoutMap,
    frame_hit_map: Option<&quadraui::FrameHitMap>,
    tab_bar_zones: &HashMap<usize, (GroupId, quadraui::Rect)>,
    drag: &mut quadraui::DragState,
) -> (Option<bool>, Option<EngineAction>) {
    match pixel_to_click_target(
        engine,
        backend,
        x,
        y,
        line_height,
        char_width,
        cached_layout,
        group_tab_bar_layouts,
        frame_hit_map,
        tab_bar_zones,
        true, // real click: focus/tab/gutter side effects are intended
        drag,
        alt, // Alt-press arms the minimap's file-wide (old #1187) track; plain press now fine-seeks by default
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
        ClickTarget::CloseTab(group_id, tab_idx) => {
            // The one call into `Engine::handle_tab_bar_click` for this
            // click — `dispatch_tab_bar_target` deliberately left the
            // dirty-check/close undone so it could happen exactly once, here,
            // where the caller is ready to show a confirmation dialog.
            let needs_confirm = engine.handle_tab_bar_click(
                group_id,
                crate::core::engine::TabBarClickTarget::CloseTab(tab_idx),
            );
            (needs_confirm.then_some(true), None)
        }
        ClickTarget::ActionMenuButton(group_id) => {
            let col = (x / char_width.max(1.0)) as u16;
            let row = (y / line_height.max(1.0)) as u16;
            // Pass the trigger's exact height in line_height units so
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

// Tab-drag drop-zone geometry is computed in `App::render_content` from
// `render::screen_to_drop_group_bounds` and cached for the drag hit-test.
// `render::resolve_tab_drop_zone` resolves through quadraui's
// `resolve_tab_drop`.

/// Handle mouse double-click — select word at position.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_mouse_double_click(
    engine: &mut Engine,
    backend: &dyn quadraui::Backend,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    group_tab_bar_layouts: &GroupTabBarLayoutMap,
    frame_hit_map: Option<&quadraui::FrameHitMap>,
    tab_bar_zones: &HashMap<usize, (GroupId, quadraui::Rect)>,
    drag: &mut quadraui::DragState,
) {
    if let ClickTarget::BufferPos(wid, line, col) = pixel_to_click_target(
        engine,
        backend,
        x,
        y,
        line_height,
        char_width,
        cached_layout,
        group_tab_bar_layouts,
        frame_hit_map,
        tab_bar_zones,
        true, // real click: focus/tab/gutter side effects are intended
        drag,
        false, // double-click has no Alt-fine-seek concept
    ) {
        engine.mouse_double_click(wid, line, col);
    }
}

/// Apply a [`render::MouseDragRoute::EditorText`] drag — extend the visual
/// selection to the glyph under the cursor.
///
/// This fires while a mouse button is held, so text-selection resolution
/// goes through `pixel_to_click_target` as a pure query (`mutate_focus:
/// false`) — the mouse sweeping over a different split's tab bar/gutter
/// must not steal focus or fire actions there.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_mouse_drag(
    engine: &mut Engine,
    backend: &dyn quadraui::Backend,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    group_tab_bar_layouts: &GroupTabBarLayoutMap,
    frame_hit_map: Option<&quadraui::FrameHitMap>,
    tab_bar_zones: &HashMap<usize, (GroupId, quadraui::Rect)>,
    drag: &mut quadraui::DragState,
) {
    if let ClickTarget::BufferPos(wid, line, col) = pixel_to_click_target(
        engine,
        backend,
        x,
        y,
        line_height,
        char_width,
        cached_layout,
        group_tab_bar_layouts,
        frame_hit_map,
        tab_bar_zones,
        false, // drag continuation: pure query, no focus/tab/gutter side effects
        drag,
        false, // mutate_focus is false, so the minimap-press rung never runs
    ) {
        engine.mouse_drag(wid, line, col);
    }
}
