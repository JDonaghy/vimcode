//! Backend-neutral pixel→click-target resolution (#862).
//!
//! Moved out of `src/gtk/click.rs` (which stays behind the `gui` feature)
//! because every function here only ever touched `quadraui::Backend`,
//! `Engine` and pure geometry — never a `gtk4`/`pango`/`gio` type. Keeping
//! them nested inside `crate::gtk` meant `crate::app` (and any future
//! backend reusing it) could not resolve them without the `gui` feature,
//! even though nothing here is GTK-specific. `src/gtk/click.rs` re-exports
//! everything below so its own tests and the rest of `crate::gtk` keep
//! resolving these names unchanged; the one function that genuinely needs
//! GTK (`build_editor_click_context`, which builds a `pango::Context`)
//! stayed behind in `src/gtk/click.rs`.
use crate::core::engine::EngineAction;
use crate::core::window::GroupId;
use crate::core::{Engine, WindowId};
use crate::render;
use crate::render::{self as render_mod, ScreenZone, WindowZone};
use std::collections::HashMap;

/// Re-export the shared ClickTarget enum.
pub(crate) use render_mod::ClickTarget;

/// Per-group pixel-accurate tab-bar hit geometry recovered from
/// [`quadraui::Backend::tab_bar_layout`] during the ShellApp `render_content`
/// pass. All x-ranges are **relative to the group's tab-bar left edge** — the
/// same space as `render::screen_zone_hit_test`'s `local_x`.
///
/// This replaces the char-cell `hit_regions` approximation for GTK tab clicks.
/// GTK draws tabs with proportional-font Pango widths + fixed pixel padding
/// (`tab_pad`, `inner_gap`, close-glyph width), so a `name.chars() * char_width`
/// estimate under-measures every tab — shifting the tab/close boundaries and
/// making mid-tab clicks land on the close button and right-edge clicks land on
/// the next tab (#515 regression). The rasteriser reports the exact drawn
/// geometry, so we hit-test against that. (`hit_regions` stays authoritative for
/// the monospace TUI backend, whose char-cell layout matches its draw.)
#[derive(Default, Clone)]
pub(crate) struct TabBarPixelHits {
    /// `(start_x, end_x)` per tab index; `(0.0, 0.0)` for scrolled-off tabs.
    pub slots: Vec<(f64, f64)>,
    /// `Some((start_x, end_x))` close-button zone per tab, or `None`.
    pub close: Vec<Option<(f64, f64)>>,
    /// Right-segment hit zones (split / diff / action buttons) as
    /// `(start_x, end_x, target)`, disjoint from the tab slots.
    pub segments: Vec<(f64, f64, crate::core::engine::TabBarClickTarget)>,
}

/// Key = `group_id.0` (single-group mode keys under the active group's id, which
/// is what `screen_zone_hit_test` reports for it).
pub(crate) type TabPixelHitMap = HashMap<usize, TabBarPixelHits>;

// ── GTK editor-group tab-bar close-glyph metrics ─────────────────────────────
// The quadraui GTK rasteriser lays each non-compact tab out as
// `tab_pad | label | tab_inner_gap | × | tab_pad | tab_outer_gap` and reports a
// *padded* close-button hit zone spanning `[label_end, tab_right_edge]`. That
// zone is far wider than the drawn × glyph, so a click well before the glyph
// used to close the tab with no warning (#515). We trim the padded zone back to
// the glyph the rasteriser actually painted — plus the same 2px hover halo it
// draws behind the ×, so the clickable box equals the highlighted box.
//
// These mirror the non-compact constants in quadraui's `gtk::backend`
// (`tab_pad = 14`, `tab_inner_gap = 10`, `tab_outer_gap = 1`) and the 2px hover
// pad in `gtk::tab_bar`. Editor-group bars are always built with
// `compact: false` (see `render::build_tab_bar_primitive`). This duplication is
// the interim until quadraui exposes the tight glyph rect directly
// (quadraui#395 tracks the API gap); `tighten_close_bounds` is the single place
// it lives.
const CLOSE_TAB_INNER_GAP: f64 = 10.0;
const CLOSE_TAB_PAD: f64 = 14.0;
const CLOSE_TAB_OUTER_GAP: f64 = 1.0;
const CLOSE_HOVER_PAD: f64 = 2.0;

/// Trim a *padded* close-button hit zone `(start, end)` — as reported by
/// `quadraui::Backend::tab_bar_layout` — down to the tight × glyph box the
/// rasteriser actually draws (including its 2px hover halo). Leading
/// `tab_inner_gap` and trailing `tab_pad + tab_outer_gap` are dead padding that
/// should select the tab, not close it. Returns `None` if the padded zone is
/// degenerate (too small to contain a glyph). (#515)
fn tighten_close_bounds(start: f64, end: f64) -> Option<(f64, f64)> {
    let tight_start = start + CLOSE_TAB_INNER_GAP - CLOSE_HOVER_PAD;
    let tight_end = end - CLOSE_TAB_PAD - CLOSE_TAB_OUTER_GAP + CLOSE_HOVER_PAD;
    if tight_end > tight_start {
        Some((tight_start, tight_end))
    } else {
        None
    }
}

/// Convert a rasteriser [`quadraui::TabBarHits`] (absolute pixel x, from
/// `Backend::tab_bar_layout`) plus its source [`quadraui::TabBar`] into a
/// [`TabBarPixelHits`] with every x-range shifted to be **relative to
/// `bar_left_x`** (the group tab bar's left edge). Right-segment ids are mapped
/// to their `TabBarClickTarget` using the same `"tab:*"` ids that
/// `build_tab_bar_primitive` emits (mirrors `draw::draw_tab_bar`).
pub(crate) fn tab_hits_to_pixel_hits(
    hits: &quadraui::TabBarHits,
    bar: &quadraui::TabBar,
    bar_left_x: f64,
) -> TabBarPixelHits {
    use crate::core::engine::TabBarClickTarget as T;
    let rel = |a: f64, b: f64| (a - bar_left_x, b - bar_left_x);
    let slots = hits
        .slot_positions
        .iter()
        .map(|&(a, b)| {
            if (a, b) == (0.0, 0.0) {
                (0.0, 0.0) // scrolled-off sentinel — leave as zero-width
            } else {
                rel(a, b)
            }
        })
        .collect();
    // Trim the padded close zone the rasteriser reports down to the tight ×
    // glyph box (relative to the bar's left edge), so clicks/hover only fire on
    // the drawn glyph — not the ~25px of surrounding tab padding. (#515)
    let close = hits
        .close_bounds
        .iter()
        .map(|c| c.and_then(|(a, b)| tighten_close_bounds(a, b).map(|(ta, tb)| rel(ta, tb))))
        .collect();
    let mut segments = Vec::new();
    for (i, seg) in bar.right_segments.iter().enumerate() {
        let Some((a, b)) = hits.right_segment_bounds.get(i).copied() else {
            continue;
        };
        let Some(ref id) = seg.id else { continue };
        let target = match id.as_str() {
            "tab:split_right" => Some(T::SplitRight),
            "tab:split_down" => Some(T::SplitDown),
            "tab:diff_prev" => Some(T::DiffPrev),
            "tab:diff_next" => Some(T::DiffNext),
            "tab:diff_toggle" => Some(T::DiffToggle),
            "tab:action_menu" => Some(T::ActionMenu),
            _ => None,
        };
        if let Some(t) = target {
            let (s, e) = rel(a, b);
            segments.push((s, e, t));
        }
    }
    TabBarPixelHits {
        slots,
        close,
        segments,
    }
}

/// Build the absolute close-glyph hit record for one tab bar from its
/// bar-relative (already-tightened) close bounds. `bar_left_x` is the bar's
/// absolute left edge; `y_top`/`y_bot` bracket the tab row. Consumed by
/// `tab_close_hit_test` for hover. (#515)
pub(crate) fn abs_close_record(
    ph_close: &[Option<(f64, f64)>],
    bar_left_x: f64,
    y_top: f64,
    y_bot: f64,
) -> (f64, f64, Vec<Option<(f64, f64)>>) {
    let xs = ph_close
        .iter()
        .map(|c| c.map(|(a, b)| (a + bar_left_x, b + bar_left_x)))
        .collect();
    (y_top, y_bot, xs)
}

/// Collect the visible tab slots (absolute x-ranges) from a `TabBarHits`,
/// dropping the `(0.0, 0.0)` sentinels for scrolled-off / non-fitting tabs.
/// The result is a contiguous run starting at the tab bar's `scroll_offset`,
/// which the drop-zone reorder logic offsets back to absolute tab indices.
/// (#515)
pub(crate) fn abs_visible_slots(hits: &quadraui::TabBarHits) -> Vec<(f32, f32)> {
    hits.slot_positions
        .iter()
        .filter(|&&(a, b)| (a, b) != (0.0, 0.0))
        .map(|&(a, b)| (a as f32, b as f32))
        .collect()
}

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
pub(crate) fn pixel_to_click_target(
    engine: &mut Engine,
    backend: &dyn quadraui::Backend,
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
                    view_row,
                    line_idx,
                    gutter_col,
                } => {
                    if !mutate_focus {
                        // A drag sweeping over another split's gutter must
                        // not fire gutter actions (e.g. toggle a breakpoint)
                        // there (#568).
                        return ClickTarget::None;
                    }
                    execute_gutter_action(engine, rw, window_id, view_row, line_idx, gutter_col);
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
                    let col = backend.editor_col_at_x(&editor_layout, &editor, view_row, x as f32);
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
    let layout: Option<&quadraui::TabBarLayout> = if cached_layout.editor_group_split.is_some() {
        cached_layout
            .group_tab_bars
            .iter()
            .find(|g| g.group_id == group_id)
            .map(|g| &g.hit_regions)
    } else {
        Some(&cached_layout.tab_bar_hit_regions)
    };
    layout.and_then(|l| render_mod::resolve_tab_bar_click(l, col))
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
pub(crate) fn resolve_tab_right_click(
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

/// Resolve a tab-bar click target and, for everything except the two targets
/// that must defer to the caller, apply it through `Engine::handle_tab_bar_click`
/// — the single dispatch the engine, the TUI and (as of #814) GTK all share.
///
/// GTK used to re-implement this arm by arm (goto_tab / open_editor_group /
/// jump_prev_hunk / ... called directly), which is exactly how the TUI's own
/// duplicate copy silently dropped `lsp_ensure_active_buffer()` (#752) before
/// this file did the same. Routing through the engine's own function means a
/// future fix to any arm lands on both backends for free.
///
/// The two exceptions:
/// - `ActionMenu` needs the click's screen coordinates to place the popup,
///   which the engine doesn't have — its own arm for this target is a
///   deliberate no-op, so `handle_mouse_click`'s `ActionMenuButton` arm opens
///   the menu directly instead.
/// - `CloseTab` on a dirty buffer must defer to a confirmation dialog before
///   anything closes, so resolution only identifies *which* tab was
///   targeted — `handle_mouse_click`'s `CloseTab` arm makes the one call into
///   `Engine::handle_tab_bar_click` that decides confirm-vs-close.
fn dispatch_tab_bar_target(
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
/// resolution. The match itself moved to [`render::apply_gutter_action`]
/// in #823 item 3 — it was an identical 5-arm match on
/// [`render_mod::resolve_gutter_action`]'s result to TUI's copy
/// (`tui_main/mouse.rs`), modulo TUI's `ToggleFold` fold-indicator guard
/// (folded into the shared function too — see its doc comment).
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
    tab_pixel_hits: &TabPixelHitMap,
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
pub(crate) fn handle_mouse_double_click(
    engine: &mut Engine,
    backend: &dyn quadraui::Backend,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    tab_pixel_hits: &TabPixelHitMap,
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
pub(crate) fn handle_mouse_drag(
    engine: &mut Engine,
    backend: &dyn quadraui::Backend,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
    cached_layout: &render::ScreenLayout,
    tab_pixel_hits: &TabPixelHitMap,
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
        frame_hit_map,
        tab_bar_zones,
        false, // drag continuation: pure query, no focus/tab/gutter side effects
    ) {
        engine.mouse_drag(wid, line, col);
    }
}
