use super::*;

// ─── Chrome mouse rung (#752 / #733 slice 2) ─────────────────────────────────
//
// The rung directly beneath [`route_modal_overlay_click`]: once no modal
// overlay has claimed the event, the *chrome* bands get their turn — the
// breadcrumb bar under each tab bar, and the three status bands (per-window,
// separated, global) along the bottom.
//
// Same story as slice 1, one rung down. The geometry was already shared
// (`resolve_breadcrumb_click`, `resolve_tab_bar_click`,
// `status_bar_zone_hit_test`) and the actions
// were already shared (`Engine::handle_breadcrumb_click`,
// `handle_tab_bar_click`, `handle_status_action`) — what was still transcribed
// twice was the **router that sequences them**, and, as always when a sequence
// is written down twice, the two copies had drifted:
//
//  1. **The global status bar was dead on both backends, differently.** TUI
//     swallowed the whole row with a `// no interactive segments` comment;
//     GTK routed exactly one segment (the git branch) through ~60 lines that
//     re-derived the bar's text by hand — a second copy of
//     [`build_status_line`]'s own formatting — and then hit-tested it with
//     `cached_char_width` rather than the width the frame actually painted
//     with (the #751 bug, one band lower).
//
//  2. **`branch_range` was measured in bytes and consumed as columns.**
//     [`build_status_line`] computed `prefix.len()` / `branch.len()`, i.e.
//     UTF-8 *byte* offsets, and its consumer compared them against a
//     character column. Any non-ASCII in the mode string, the filename or the
//     branch decoration — and the ahead/behind arrows `↑`/`↓` are three bytes
//     each, so merely being ahead of origin was enough — shifted the clickable
//     branch right of the painted one, growing with each multi-byte glyph.
//
//     Fixing the units would have been the small change; the range is gone
//     instead. [`build_global_status_bar`] now emits the branch as its own
//     `StatusBarSegment` with an `action_id`, so its hit region is produced by
//     the same `StatusBar::layout` pass that positions its glyphs — the way
//     the per-window bar has always worked. That also fixes a *second*, hidden
//     error the byte fix would have left behind: GTK paints the status bar at
//     a ~6.6px advance while `painted_char_width()` reports 8.0, so *any*
//     column-arithmetic hit test on that bar was wrong by ~20% of x even with
//     perfect units. Measured geometry beats arithmetic.
//
//  3. **TUI re-implemented `handle_tab_bar_click` inline** for the
//     single-group case (the split-group case two branches above it already
//     delegated), and the copy had lost `active_group` assignment and the
//     `lsp_ensure_active_buffer()` call, so clicking a tab in an unsplit
//     window left the LSP pointed at the previous buffer.
//
//  4. **GTK recovered status-bar hit zones from a painted layout in two
//     places** with the same nine-line loop; the global bar, needing a third,
//     is what made that worth naming once ([`status_bar_zones_from_layout`]).
//
// Deliberately unit-agnostic in the same way slice 1's router is: the caller
// states its own scale ([`ChromeState::line_height`], and status bands carry
// their painted rect plus bar-local zones), and the *sequence* underneath is
// unit-free. TUI passes cells with a `1.0` line height; GTK passes device
// pixels and its painted line height.
//
// **Not rungs of this router, deliberately** — three chrome surfaces that are
// shared by *other* means, with the verdict recorded here so the next slice
// does not re-litigate it:
//
//   * **The bottom-panel tab bar.** Both backends already delegate it to
//     `Engine::resolve_bottom_panel_zone` + `handle_bottom_tab_bar_click` in
//     three lines each, and the geometry lives on the engine
//     (`bottom_panel_geometry`, #418) rather than in either rasteriser. There
//     is no second copy of anything to converge.
//
//   * **The command centre.** Its hit test is one shared call
//     (`CommandCenterLayout::hit_test`) and its actions are now one shared
//     call ([`apply_command_center_hit`]) — but its *position in the ladder*
//     is intrinsically per-backend and must stay so. GTK paints the centre
//     into the CSD title bar, so it has to be arbitrated above the inline
//     window controls and the drag-to-move fallback, neither of which TUI
//     has; TUI arbitrates it on the menu-bar row, which GTK does not paint
//     the centre into. Folding it into the sequence below would force one
//     backend to state an order it cannot honour — the failure #735 warns
//     about, in reverse.
//
//   * **The tab bar.** Its *geometry* genuinely cannot be shared and is not a
//     drift bug: GTK lays tabs out with proportional-font Pango widths and
//     resolves clicks against the pixel bounds the rasteriser actually drew
//     (`tab_pixel_hits`, #515), while TUI's monospace char-cell `hit_regions`
//     are exact. What *was* transcribed is the dispatch of a resolved
//     [`crate::core::engine::TabBarClickTarget`], and the engine has owned
//     that since `Engine::handle_tab_bar_click` — GTK's split path and TUI's
//     split path both called it; only TUI's single-group path still had a
//     hand-rolled copy, now deleted (see item 3 above).
//
// The GTK-only CSD titlebar drag-to-move fallback has no TUI twin at all.

/// The subset of mouse actions the chrome rung distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeMouseAction {
    /// Left button pressed — the only action that *fires* a chrome control.
    LeftPress,
    /// Anything else: release, drag, right-click, scroll, motion. Chrome
    /// bands still **consume** these (a drag sweeping across the tab bar must
    /// not extend an editor selection through it), they just do not act.
    Other,
}

/// `(start, end, action)` hit zones for one status bar, **local to that bar's
/// own origin** — the shape [`status_bar_zones_from_layout`] produces and
/// [`status_bar_zone_hit_test`] consumes, in whatever unit the producer
/// measured in (pixels on GTK, cells on TUI).
pub type StatusZones = Vec<(f64, f64, StatusAction)>;

/// One painted status band, in the caller's own units.
///
/// Covers all three of vimcode's status surfaces — the per-window status line,
/// the separated status line above the terminal, and the global bottom bar —
/// because from the router's point of view they differ only in where they were
/// painted and which zones they carry.
#[derive(Debug, Clone)]
pub struct StatusBand<'a> {
    /// The rect the last frame actually painted this bar into.
    pub rect: quadraui::Rect,
    /// `(start, end, action)` triples **local to `rect`'s own origin**, as
    /// produced by [`status_bar_zones_from_layout`].
    pub zones: &'a [(f64, f64, StatusAction)],
}

/// The painted chrome bands, in the order they are arbitrated.
///
/// Every field is optional/empty-able: a backend that does not paint a band
/// this frame simply leaves it out, and the router skips that rung rather than
/// the caller having to guard the call.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChromeState<'a> {
    /// `Engine::settings.breadcrumbs` — the gate, kept separate from the data
    /// so an empty slice and "disabled" stay distinguishable.
    pub breadcrumbs_enabled: bool,
    /// Every group's painted breadcrumb bar (`ScreenLayout::breadcrumbs`).
    pub breadcrumbs: &'a [BreadcrumbBar],
    /// One text row, in the caller's units: `1.0` for TUI cells, the painted
    /// line height for GTK.
    pub line_height: f64,
    /// Status bands painted this frame, nearest-to-the-user first.
    pub status_bands: &'a [StatusBand<'a>],
    /// `true` when the caller's window-split-divider hit test (the shared
    /// [`divider_hit_test`], on both backends) already claims this point.
    ///
    /// The two bands genuinely overlap and one has to win. #582 established
    /// that with `window_status_line` on — the default — a `:split` boundary
    /// draws *no glyph of its own*: the upper window's own status-line row is
    /// the only thing marking it, and is therefore the row the user aims at to
    /// drag the split. So that row is both "the status bar" and "the divider
    /// grab handle".
    ///
    /// The **divider wins**, because that is what both backends did before
    /// #752 (each ran its divider rung above its status arm) and because
    /// losing a resize gesture is the worse failure — a status segment can be
    /// clicked again, a drag that silently does nothing reads as a broken
    /// window manager. Stated here rather than left implicit in each
    /// backend's ladder order, which is the whole point of the router.
    pub on_window_divider: bool,
}

/// What the chrome rung decided about one event.
#[derive(Debug, Clone, PartialEq)]
pub enum ChromeRoute {
    /// A breadcrumb segment was hit — `Engine::handle_breadcrumb_click`.
    Breadcrumb { group_id: GroupId, idx: usize },
    /// On a breadcrumb bar but not on a segment — consume.
    BreadcrumbBar,
    /// A status segment was hit — apply with [`apply_status_action`].
    StatusAction(StatusAction),
    /// On a status band but not on an actionable segment — consume.
    StatusBar,
    /// No chrome band claimed the event; the caller continues down its own
    /// ladder (tab bar, editor, …).
    None,
}

/// Sequence the chrome bands against one mouse event.
///
/// `(x, y)` are absolute, in the caller's own units — cells for TUI, device
/// pixels for GTK. See the module comment above this function for why the
/// order below is stated here rather than transcribed into each backend.
pub fn route_chrome_click(
    state: &ChromeState<'_>,
    action: ChromeMouseAction,
    x: f64,
    y: f64,
) -> ChromeRoute {
    // ── Breadcrumbs ─────────────────────────────────────────────────────
    // Above the tab bar in arbitration because it is painted *below* it and
    // the two bands abut: a click on the breadcrumb row must never be
    // mistaken for the tab row beneath which it sits.
    if state.breadcrumbs_enabled {
        match resolve_breadcrumb_click(state.breadcrumbs, x, y, state.line_height) {
            BreadcrumbClickResult::Hit(group_id, idx) => {
                if action == ChromeMouseAction::LeftPress {
                    return ChromeRoute::Breadcrumb { group_id, idx };
                }
                return ChromeRoute::BreadcrumbBar;
            }
            BreadcrumbClickResult::OnBar => return ChromeRoute::BreadcrumbBar,
            BreadcrumbClickResult::Miss => {}
        }
    }

    // ── Status bands ────────────────────────────────────────────────────
    // A window-split divider grab outranks the status row it shares — see
    // `ChromeState::on_window_divider`. The *global* bar is unaffected in
    // practice: it sits in the shell's own bottom band, outside every window
    // rect, so no divider can reach it.
    for band in state
        .status_bands
        .iter()
        .filter(|_| !state.on_window_divider)
    {
        if let Some(hit) = status_bar_zone_hit_test(band.rect, band.zones, x, y) {
            if action == ChromeMouseAction::LeftPress {
                return ChromeRoute::StatusAction(hit);
            }
            return ChromeRoute::StatusBar;
        }
        if point_in_rect(band.rect, x, y) {
            return ChromeRoute::StatusBar;
        }
    }

    ChromeRoute::None
}

/// Absolute point-in-rect, in whatever units the caller measures in.
fn point_in_rect(rect: quadraui::Rect, x: f64, y: f64) -> bool {
    x >= rect.x as f64
        && x < (rect.x + rect.width) as f64
        && y >= rect.y as f64
        && y < (rect.y + rect.height) as f64
}

/// Recover `(start, end, action)` hit zones from a status bar that has already
/// been laid out for painting.
///
/// The returned spans are **local to the bar's own origin**, matching
/// [`status_bar_zone_hit_test`]'s contract, because `StatusBar::layout` always
/// lays segments out from `(0, 0)` regardless of the rect it will be drawn
/// into.
///
/// #752: GTK had this nine-line loop transcribed once per painted bar (the
/// per-window line and the separated line), and the global bar needed a third.
pub fn status_bar_zones_from_layout(layout: &quadraui::StatusBarLayout) -> StatusZones {
    layout
        .hit_regions
        .iter()
        .filter_map(|(rect, hit)| match hit {
            quadraui::StatusBarHit::Segment(id) => status_action_from_id(id.as_str())
                .map(|action| (rect.x as f64, (rect.x + rect.width) as f64, action)),
            _ => None,
        })
        .collect()
}

/// Assemble this frame's [`StatusBand`]s, in the order [`route_chrome_click`]
/// arbitrates them: the separated status line (if painted this frame), then
/// each window's own status line, then the global bar last.
///
/// #1250: both backends now cache their status-bar layouts at **paint**
/// time — GTK's `status_segment_map` since #672, TUI's own copy since this
/// function's introduction — rather than one of them (TUI) re-deriving the
/// separated-status row's rect arithmetically and re-laying the bar's text
/// out from scratch on every click via the now-deleted
/// `window_status_line_zones`/`status_bar_zones_in_cells`. This is the one
/// place that turns "a painted rect + its cached hit zones" into the
/// `StatusBand` slice the router consumes, so the geometry (`rw.rect.y +
/// rw.rect.height - lh`, `rw.rect.height <= lh` skip, and the arbitration
/// order itself) is written once instead of transcribed per backend.
///
/// `windows`/`lh` are the same painted geometry [`ChromeState::line_height`]
/// documents (`1.0` for TUI cells, the painted line height for GTK).
/// `segment_map` is keyed by [`WindowId`] (`.0`), as populated by each
/// backend's own per-window and separated-status paint sites.
/// `separated`/`global` are `None` whenever that band did not paint this
/// frame — the empty/absent convention [`ChromeState`] documents.
///
/// #1690: the painted *background fill* under a single window's status bar
/// now extends edge-to-edge under the sidebar/activity bar too (see
/// `App::paint_editor_windows_rung`'s backdrop fill), but the clickable
/// band/zones recovered here deliberately stay window-bounded — AppShell's
/// own activity-bar/sidebar hit-test runs *before* `App::handle` ever sees
/// the event (quadraui's `ShellAdapter::handle`), so a click over that
/// visually-capped region never reaches this router at all; widening the
/// band here without widening *that* hit-test first would just make
/// `status_bar_zone_hit_test` resolve a position no click can actually
/// arrive at. See the issue for why this is paint-only.
pub fn status_bands<'a>(
    windows: &[RenderedWindow],
    lh: f64,
    segment_map: &'a crate::app_support::StatusSegmentMap,
    separated: Option<(quadraui::Rect, WindowId)>,
    global: Option<(quadraui::Rect, &'a StatusZones)>,
) -> Vec<StatusBand<'a>> {
    let mut bands = Vec::new();

    // The separated status line is listed first: it is painted in its own
    // full-width band *outside* every window's rect, so it can never be
    // reached through the per-window bars' geometry, and a click in that band
    // must not fall through to whatever sits underneath it.
    if let Some((rect, window_id)) = separated {
        if let Some(zones) = segment_map.get(&window_id.0) {
            bands.push(StatusBand { rect, zones });
        }
    }

    for rw in windows {
        if rw.status_line.is_none() || rw.rect.height <= lh {
            continue;
        }
        let Some(zones) = segment_map.get(&rw.window_id.0) else {
            continue;
        };
        // The status line occupies the window's bottom row — the same
        // `rect.height - lh` both paint paths subtract before drawing it.
        bands.push(StatusBand {
            rect: quadraui::Rect::new(
                rw.rect.x as f32,
                (rw.rect.y + rw.rect.height - lh) as f32,
                rw.rect.width as f32,
                lh as f32,
            ),
            zones,
        });
    }

    // The global bar last, spatially and in arbitration: it is the bottom
    // band of the shell, below every window.
    if let Some((rect, zones)) = global {
        bands.push(StatusBand { rect, zones });
    }

    bands
}

/// Apply a resolved [`StatusAction`], including the follow-up both backends
/// used to transcribe around `Engine::handle_status_action`.
///
/// `terminal_cols` is the width a newly-opened terminal should be created at.
/// Returns any [`crate::core::engine::EngineAction`] the *caller* still has to
/// act on — today only `ToggleSidebar`, which GTK answers by re-syncing its
/// sidebar widget and TUI by doing nothing (the engine already toggled it).
pub fn apply_status_action(
    engine: &mut Engine,
    action: &StatusAction,
    terminal_cols: u16,
) -> Option<crate::core::engine::EngineAction> {
    use crate::core::engine::EngineAction;
    match engine.handle_status_action(action) {
        Some(EngineAction::OpenTerminal) => {
            let rows = engine.session.terminal_panel_rows;
            engine.terminal_new_tab(terminal_cols, rows);
            None
        }
        other => other,
    }
}

/// Resolve a Command Center click, ignoring any part of the cached layout
/// that falls **outside the band the Command Center was painted into**.
///
/// `quadraui::CommandCenter::layout` centres its content in `bounds` via
/// `bounds.x + (bounds.width - content_width).max(0.0) / 2.0` — that
/// `.max(0.0)` clamps the *centring offset*, not the content, so when the
/// band is narrower than the content's minimum width the whole group is
/// left-aligned at `bounds.x` and simply **overflows the right edge**. On
/// every pixel backend that minimum is fixed at
/// `2 * ARROW_WIDTH_PX + 2 * GAP_PX + SEARCH_MIN_WIDTH_PX` = 344px
/// (`quadraui::CommandCenterMeasure`), so any title-bar band narrower than
/// that overflows — and the overflowing `SearchBox` rect is still recorded
/// as a hit region.
///
/// Whatever sits to the Command Center's right therefore loses its clicks
/// to the search box. On GTK that is the inline minimize/maximize/close
/// buttons (`measure_title_bar_bands`' `controls` band, which starts exactly
/// where `command_center` ends): the button paints, the press lands on the
/// search box instead, and `StatusBarInteraction` never arms — so the
/// release is a no-op and the window never minimizes.
///
/// How far the overflow reaches depends on `menu_end`, i.e. on the
/// *measured* width of the menu labels, i.e. on which UI font resolved from
/// `UI_FONT_FAMILY` — which is why this reproduces at 800x600 on a bare CI
/// runner (no Cantarell/Ubuntu installed, so the wider DejaVu Sans fallback
/// wins) and not on a GNOME desktop, and why the `#1530` titlebar tests were
/// green locally and red in CI. The underlying overlap is font-independent
/// though: it bites at *every* window width below roughly 815px on either
/// font.
///
/// A widget must not be clickable outside the region it was allotted, so
/// this clamps to `layout.bounds` before consulting `hit_test`. Shared here
/// rather than in a backend so every backend that routes a Command Center
/// click gets the same arbitration (Platform-Neutrality Rule).
///
/// Note this fixes the *click* half only. The paint still overflows — the
/// search box's rounded border draws over the window-control glyphs on a
/// narrow band, because `CommandCenter::layout` hands the backend
/// out-of-bounds rects and neither `gtk::draw_command_center` nor its
/// mac/win twins clip to `bounds`. That is a quadraui-side gap (the
/// primitive should shrink the search box to fit, or clip), not something
/// vimcode can fix without per-backend code.
pub fn command_center_hit_in_band(
    layout: &quadraui::CommandCenterLayout,
    x: f32,
    y: f32,
) -> Option<quadraui::CommandCenterHit> {
    let b = layout.bounds;
    if x < b.x || x >= b.x + b.width || y < b.y || y >= b.y + b.height {
        return None;
    }
    Some(layout.hit_test(x, y))
}

/// Apply a resolved [`quadraui::CommandCenterHit`]. Returns `true` when the
/// hit was an interactive control (so the caller consumes and redraws).
pub fn apply_command_center_hit(engine: &mut Engine, hit: quadraui::CommandCenterHit) -> bool {
    match hit {
        quadraui::CommandCenterHit::Back => engine.tab_nav_back(),
        quadraui::CommandCenterHit::Forward => engine.tab_nav_forward(),
        quadraui::CommandCenterHit::SearchBox => engine.open_command_center(),
        _ => return false,
    }
    true
}

// ═══ Divider + drag rung (#753, mouse-ladder slice 3) ═════════════════════════
//
// The rung *beneath* the chrome rung above: once no chrome band has claimed the
// point, the next things that can own it are the resize handles between editor
// groups and between `:split`/`:vsplit` panes, and the tab drag-and-drop
// gesture. All three are **stateful** — a press arms them, subsequent moves
// track them, a release commits — and that state machine was transcribed once
// per backend, with the usual results:
//
//  1. **Two mutually-exclusive `Option` fields per backend**
//     (`dragging_group_divider` + `dragging_window_divider` on TUI,
//     `group_divider_dragging` + `window_divider_dragging` on GTK) encoded
//     "at most one divider is grabbed" as an invariant nothing enforced.
//     [`DividerGrab`] makes it one field, so the illegal state is unrepresentable.
//
//  2. **The drag-application block was written four times** — find the divider
//     whose `split_index` matches the grab, [`divider_ratio_from_pos`], push the
//     ratio into `GroupLayout` or the group's active-tab `WindowLayout`. Two
//     copies per backend, ~11 lines each, differing only in how the divider list
//     was obtained. It is now [`apply_divider_drag`], called once per backend.
//
//  3. **`#582` is named in both backends' banner comments** — the issue title
//     the tracker uses for "the divider frame mismatch that had to be fixed
//     twice". The second fix existed *because* the first one landed in only one
//     rasteriser. The arbitration order (group divider, then window divider) and
//     the "a grabbed divider outranks everything below it" rule are now stated
//     once, in [`route_divider_grab`], rather than in two ladders that have
//     already drifted apart once.
//
//  4. **The tab-drag state machine was five fields and four call sites per
//     backend.** [`TabDragState`] owns arm → threshold → track → commit, and
//     both backends now hold one field.
//
// Deliberately unit-agnostic in the same way slices 1 and 2 are: the caller
// states its own scale (the tolerances in [`DividerMetrics`], the squared
// threshold in [`TabDragState::handle_move`]) and the *sequence* underneath is
// unit-free. TUI passes character cells, GTK device pixels.
//
// **Not a rung of this router, deliberately** — recorded here so the next slice
// does not re-litigate it:
//
//   * **The sidebar separator drag.** It has no GTK twin to converge *with*.
//     TUI paints the separator itself and resizes on drag because it owns every
//     cell of its own frame; GTK's sidebar is a real widget in the container
//     hierarchy and GTK's own layout machinery does the resize — there is no
//     `handle_mouse_drag_msg` arm for it, and adding one so that a shared
//     router would have two callers would be inventing a duplicate in order to
//     delete it. The divider *geometry* each backend feeds in is likewise its
//     own painted geometry (`ScreenLayout::group_dividers` on TUI, the
//     `cached_editor_bounds` recompute on GTK) for the same reason
//     `ChromeState` takes painted rects rather than deriving them.

/// Which divider a press grabbed. One field replaces the two
/// mutually-exclusive `Option`s each backend used to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DividerGrab {
    /// A boundary *between editor groups* (`Ctrl+W v` / `Ctrl+W s`), resized
    /// through `Engine::group_layout`.
    Group { split_index: usize },
    /// A `:split`/`:vsplit` boundary *within* one group's active tab (#582),
    /// resized through that group's `WindowLayout`.
    Window {
        group_id: GroupId,
        split_index: usize,
    },
}

/// How far off a divider's painted `position` a press still counts as a grab,
/// in the caller's own units.
///
/// Genuinely per-backend, because it describes what each rasteriser *drew*:
/// TUI's divider occupies exactly one character cell starting at `position`, so
/// its band is one cell wide and `quantize` must match the renderer's `as u16`
/// truncation (#452); GTK draws a thin continuous line and wants a symmetric
/// pixel grab margin around it.
#[derive(Debug, Clone, Copy)]
pub struct DividerMetrics {
    /// Tolerance for vertical (left/right) group dividers.
    pub group_vertical: DividerTolerance,
    /// Tolerance for horizontal (top/bottom) group dividers.
    pub group_horizontal: DividerTolerance,
    /// Tolerance for vertical window-split dividers.
    pub window_vertical: DividerTolerance,
    /// Tolerance for horizontal window-split dividers.
    pub window_horizontal: DividerTolerance,
    /// Hit-test against the truncated `position as u16` the renderer drew at
    /// (TUI) rather than the continuous float position (GTK). See
    /// [`divider_hit_test`].
    pub quantize: bool,
}

/// GTK's grab metrics: a symmetric 6-device-pixel margin around the thin
/// continuous line the Cairo rasteriser drew, in both split directions.
pub const GTK_DIVIDER_METRICS: DividerMetrics = DividerMetrics {
    group_vertical: (6.0, 6.0),
    group_horizontal: (6.0, 6.0),
    window_vertical: (6.0, 6.0),
    window_horizontal: (6.0, 6.0),
    quantize: false,
};

// ═══ Drag-rung tests (#756, mouse-ladder slice 6) ════════════════════════════

#[cfg(test)]
mod mouse_drag_router_tests {
    use super::*;
    use crate::core::Mode;

    /// A buffer long and deep enough that `build_screen_layout` paints a
    /// minimap strip — the rung the two backends disagreed about.
    fn drag_engine() -> Engine {
        crate::core::session::suppress_disk_saves();
        let mut e = Engine::new_for_test();
        e.mode = Mode::Normal;
        let mut text = String::new();
        for i in 0..400 {
            text.push_str(&format!("line {i} content that is reasonably long\n"));
        }
        e.buffer_mut().insert(0, &text);
        e.settings.minimap = true;
        e
    }

    /// Paint one frame at the caller's cell metrics.
    ///
    /// `cell = (1.0, 1.0)` is the TUI's whole-cell grid; `(9.0, 18.0)` stands in
    /// for a GTK font's advance/line height. The *same* logical 80×24 screen is
    /// described in both, which is what makes the parity assertion below mean
    /// "the two backends see one screen", not "two screens happen to agree".
    ///
    /// `scrollbar_reserve`/`minimap_sizing` are the caller-stated stand-ins
    /// for whichever backend `cell` represents' own
    /// `quadraui::Backend::scrollbar_reserve()` / `TUI_MINIMAP_SIZING` or
    /// `gtk_minimap_sizing()` (#828) — stated explicitly by each call site
    /// below, not derived from `cell` here, so this helper carries no
    /// backend sniff of its own.
    fn frame(
        engine: &Engine,
        cell: (f64, f64),
        scrollbar_reserve: f64,
        minimap_sizing: quadraui::MinimapSizing,
    ) -> ScreenLayout {
        let (cw, ch) = cell;
        let bounds = WindowRect::new(0.0, 0.0, 80.0 * cw, 24.0 * ch);
        let (rects, _) = engine.calculate_group_window_rects(bounds, ch);
        let theme = Theme::onedark();
        build_screen_layout(
            engine,
            &theme,
            &rects,
            ch,
            cw,
            true,
            scrollbar_reserve,
            minimap_sizing,
        )
    }

    /// Every rung, in the order [`route_mouse_drag`] must resolve them, paired
    /// with the one state field that selects it.
    ///
    /// Not a tautology: this is the only place the order is written down, so a
    /// reviewer reads it here, and reordering the router fails this first with
    /// a diff that names both rungs.
    #[test]
    fn armed_gestures_resolve_in_a_fixed_order() {
        type Arm = (&'static str, fn(&mut MouseDragState<'_>), MouseDragRoute);
        let ladder: &[Arm] = &[
            (
                "armed drag target",
                |s| s.armed_target = true,
                MouseDragRoute::ArmedTarget,
            ),
            (
                "hover popup selection",
                |s| s.hover_popup_selecting = true,
                MouseDragRoute::HoverPopupSelection,
            ),
            (
                "sidebar resize",
                |s| s.sidebar_resizing = true,
                MouseDragRoute::SidebarResize,
            ),
            (
                "explorer drag-and-drop",
                |s| s.explorer_dnd_active = true,
                MouseDragRoute::ExplorerDnd,
            ),
            (
                "tab drag",
                |s| s.tab_dragging = true,
                MouseDragRoute::TabDrag,
            ),
            (
                "command line selection",
                |s| s.command_line_selecting = true,
                MouseDragRoute::CommandLine,
            ),
            (
                "divider grab",
                |s| s.divider_grabbed = true,
                MouseDragRoute::Divider,
            ),
            (
                "terminal split divider",
                |s| s.terminal_split_dragging = true,
                MouseDragRoute::TerminalSplitDivider,
            ),
            (
                "terminal panel resize",
                |s| s.terminal_panel_resizing = true,
                MouseDragRoute::TerminalPanelResize,
            ),
            (
                "text selection already extending",
                |s| s.text_selection_active = true,
                MouseDragRoute::EditorText,
            ),
            (
                "modal swallow",
                |s| s.modal_hit = true,
                MouseDragRoute::ModalSwallow,
            ),
        ];

        // Turning every flag on at once must yield the *first* rung; dropping
        // that rung must reveal the next, all the way down. One pass pins both
        // membership and order.
        for skip in 0..ladder.len() {
            let mut state = MouseDragState::default();
            for (_, set, _) in &ladder[skip..] {
                set(&mut state);
            }
            let (name, _, expected) = &ladder[skip];
            assert_eq!(
                route_mouse_drag(&state, 10.0, 10.0),
                *expected,
                "with every rung from `{name}` down asserted, `{name}` must win"
            );
        }
    }

    /// #1429: `apply_explorer_drag_move` promotes an armed source into an
    /// active `(src, target)` pair once the pointer reaches a different
    /// row, and `apply_explorer_drop` turns that pair into the same
    /// move-confirm dialog `Engine::confirm_move_file` always opens.
    #[test]
    fn explorer_drag_move_then_drop_opens_confirm_move_dialog() {
        let mut engine = Engine::new_for_test();
        engine.explorer_rows = vec![
            crate::core::engine::ExplorerRow {
                depth: 0,
                name: "afile".into(),
                path: std::path::PathBuf::from("/tmp/afile"),
                is_dir: false,
                is_expanded: false,
            },
            crate::core::engine::ExplorerRow {
                depth: 0,
                name: "adir".into(),
                path: std::path::PathBuf::from("/tmp/adir"),
                is_dir: true,
                is_expanded: false,
            },
        ];
        let rect = quadraui::Rect::new(0.0, 0.0, 40.0, 10.0);
        let mut src = Some(0usize);
        let mut active: Option<(usize, Option<usize>)> = None;
        apply_explorer_drag_move(&engine, rect, 1.0, 5.0, 1.0, &mut src, &mut active);
        assert_eq!(
            active,
            Some((0, Some(1))),
            "moving onto row 1 must activate the drag with that row as the target"
        );
        if let Some((s, t)) = active.take() {
            apply_explorer_drop(&mut engine, s, t);
        }
        assert!(
            engine.dialog.is_some(),
            "dropping a file row onto a directory row must open the \
             move-confirm dialog"
        );
    }

    /// A held drag over the minimap strip belongs to the minimap.
    ///
    /// Red against unfixed `develop`: there was no shared drag router at all,
    /// and TUI's own minimap arm sat below a `Down`-only gate, so a TUI
    /// press-and-hold on the strip resolved to nothing after the first cell.
    ///
    /// `build_screen_layout` shrinks each window rect by exactly the reserved
    /// width, so the strip and the text area are spatially disjoint — this
    /// pins that the strip is *claimed*, and the editor assertion beside it
    /// pins that claiming it did not swallow the text area next door.
    #[test]
    fn a_held_drag_over_the_minimap_strip_keeps_seeking() {
        let engine = drag_engine();
        let layout = frame(&engine, (1.0, 1.0), 0.0, TUI_MINIMAP_SIZING);
        let mm = layout
            .minimap
            .first()
            .expect("the fixture must paint a minimap strip");
        let strip = minimap_strip_rect(mm);
        let state = MouseDragState {
            layout: Some(&layout),
            ..Default::default()
        };

        assert_eq!(
            route_mouse_drag(
                &state,
                (strip.x + strip.width / 2.0) as f64,
                (strip.y + strip.height / 2.0) as f64,
            ),
            MouseDragRoute::Minimap,
            "a held drag over the minimap must keep seeking"
        );

        let text_x = strip.x as f64 - 2.0;
        assert_eq!(
            route_mouse_drag(&state, text_x, (strip.y + strip.height / 2.0) as f64),
            MouseDragRoute::EditorText,
            "the text area immediately left of the strip must still select text"
        );
    }

    /// #756 review (non-blocking concern): once `armed_target` correctly
    /// excludes `DragTarget::TextSelection` (see [`drag_state_arms_scrollbar`]),
    /// an in-progress editor text-selection drag is arbitrated purely by
    /// geometry again — which means a pointer that strays over the minimap
    /// strip or into the terminal panel mid-selection would get hijacked
    /// into [`MouseDragRoute::Minimap`] / [`MouseDragRoute::TerminalContent`]
    /// instead of continuing to extend the editor selection. `text_selection_active`
    /// is the guard: pins that once a selection has started extending
    /// (`Engine::mouse_drag_active`), it keeps winning over both geometric
    /// rungs, the same way `explorer_dnd_active` keeps winning over the
    /// editor once an explorer drag has been picked up.
    #[test]
    fn a_selection_already_extending_beats_the_minimap_and_terminal_geometry() {
        let engine = drag_engine();
        let layout = frame(&engine, (1.0, 1.0), 0.0, TUI_MINIMAP_SIZING);
        let mm = layout
            .minimap
            .first()
            .expect("the fixture must paint a minimap strip");
        let strip = minimap_strip_rect(mm);
        let state = MouseDragState {
            layout: Some(&layout),
            text_selection_active: true,
            in_terminal_content: true,
            ..Default::default()
        };

        assert_eq!(
            route_mouse_drag(
                &state,
                (strip.x + strip.width / 2.0) as f64,
                (strip.y + strip.height / 2.0) as f64,
            ),
            MouseDragRoute::EditorText,
            "an in-progress text selection must not be stolen by the minimap \
             strip it happens to be dragged over"
        );

        let text_x = strip.x as f64 - 2.0;
        assert_eq!(
            route_mouse_drag(&state, text_x, (strip.y + strip.height / 2.0) as f64),
            MouseDragRoute::EditorText,
            "nor by `in_terminal_content` geometry, even though it is asserted \
             true here"
        );
    }

    /// **The parity test #756 asks for.** One engine, painted twice — once in
    /// the TUI's whole-cell units and once in GTK-shaped pixel units — then the
    /// *same logical points* driven through [`route_mouse_drag`] in each
    /// backend's own convention. Every point must land on the same rung.
    ///
    /// Verified RED by re-forking the ladder the way the two backends were
    /// forked before this slice: hardcoding `route_mouse_drag`'s editor-zone
    /// hit test to `1.0, 1.0` (the TUI's cell metrics, i.e. writing the shared
    /// router from one backend's side and letting the other inherit its
    /// units) fails here — `cell (0.5, 2.5): TUI routed a held drag to None
    /// but GTK routed the same point on the same screen to EditorText` — with
    /// a diff that names both rungs. That is the class of divergence — one
    /// backend's
    /// convention silently baked into a "shared" rung — that the two
    /// independently-ordered ladders kept producing.
    #[test]
    fn both_backends_resolve_the_same_layout_and_point_to_the_same_rung() {
        // #1869: the GTK char width here is chosen so the real VS Code
        // minimap width formula ([`vs_code_minimap_width_px`]) happens to
        // land its strip's left edge at the same *logical* column (68 of
        // 80) as TUI's independent, deliberately much cruder
        // `MINIMAP_TARGET_COLS_TUI` (12) policy — before #1869, GTK's
        // fraction-based formula made that coincidence hold at (9.0, 18.0)
        // instead. The two policies are not required to agree at every
        // char width (TUI sizing is out of #1869's scope and stays a crude
        // approximation), only at the one this test picks, so the shared
        // router can be exercised across both a minimap and a non-minimap
        // rung at the same logical point.
        const GTK_CELL: (f64, f64) = (6.2, 18.0);
        let engine = drag_engine();
        let tui = frame(&engine, (1.0, 1.0), 0.0, TUI_MINIMAP_SIZING);
        let gtk = frame(&engine, GTK_CELL, 8.0, gtk_minimap_sizing());

        assert_eq!(
            tui.windows.len(),
            gtk.windows.len(),
            "the two frames must describe the same screen"
        );
        assert_eq!(
            tui.minimap.len(),
            gtk.minimap.len(),
            "the two frames must describe the same screen"
        );

        // Logical cell coordinates walked across the whole screen, plus the
        // centre of the minimap strip so the rung that actually differed is
        // always sampled regardless of grid alignment.
        let mut points: Vec<(f64, f64)> = Vec::new();
        for col in (0..80).step_by(3) {
            for row in (0..24).step_by(2) {
                points.push((col as f64 + 0.5, row as f64 + 0.5));
            }
        }
        let strip = minimap_strip_rect(&tui.minimap[0]);
        points.push((
            (strip.x + strip.width / 2.0) as f64,
            (strip.y + strip.height / 2.0) as f64,
        ));

        let mut saw_minimap = false;
        let mut saw_editor = false;
        for (cx, cy) in points {
            let tui_route = route_mouse_drag(
                &MouseDragState {
                    layout: Some(&tui),
                    cell: (1.0, 1.0),
                    ..Default::default()
                },
                cx,
                cy,
            );
            let gtk_route = route_mouse_drag(
                &MouseDragState {
                    layout: Some(&gtk),
                    cell: GTK_CELL,
                    ..Default::default()
                },
                cx * GTK_CELL.0,
                cy * GTK_CELL.1,
            );
            assert_eq!(
                tui_route, gtk_route,
                "cell ({cx}, {cy}): TUI routed a held drag to {tui_route:?} but \
                 GTK routed the same point on the same screen to {gtk_route:?} \
                 — the ladder has re-forked"
            );
            saw_minimap |= tui_route == MouseDragRoute::Minimap;
            saw_editor |= tui_route == MouseDragRoute::EditorText;
        }
        assert!(
            saw_minimap && saw_editor,
            "the sampled points must actually exercise both the minimap and the \
             editor text area (minimap={saw_minimap}, editor={saw_editor}), or \
             agreement is vacuous"
        );
    }

    /// The union table from point 2 of the rung's banner: every widget id
    /// *either* backend emits must apply on *both*. The pre-#756 copies each
    /// knew roughly half of this list, so a scrollbar that tracked on one
    /// backend silently scrolled nothing on the other.
    #[test]
    fn the_scroll_offset_table_is_the_union_of_both_backends() {
        let ids = [
            "picker",
            "explorer:sb",
            "ext_panel:sb",
            "editor_hover",
            "terminal_scrollback",
            "debug_output",
            "tui:debug_output",
            "tui:settings",
            "tui:search_results",
            "debug_sidebar:0",
            "tui:editor:0:vsb",
            "tui:editor:0:hsb",
            "editor:h_sb:0",
            "minimap:0",
        ];
        let mut engine = drag_engine();
        for id in ids {
            assert!(
                apply_scroll_offset(&mut engine, id, 1, ScrollApplyContext::default()),
                "`{id}` is emitted by at least one backend and must apply on both"
            );
        }
        assert!(
            !apply_scroll_offset(
                &mut engine,
                "not:a:widget",
                1,
                ScrollApplyContext::default()
            ),
            "an unknown id must report unhandled so the caller can fall through"
        );
    }

    /// The union is not just an arm-count: an id only *one* backend registers
    /// today must still actually move the state it names when the other
    /// backend starts registering it. Asserted on the tree's own scroll
    /// offset, not on the table having an arm for the id.
    #[test]
    fn an_explorer_scrollbar_offset_moves_the_tree_on_either_backend() {
        let mut engine = drag_engine();
        engine.explorer_tree.borrow_mut().set_scroll_offset(0);
        assert!(apply_scroll_offset(
            &mut engine,
            "explorer:sb",
            7,
            ScrollApplyContext::default()
        ));
        assert_eq!(
            engine.explorer_tree.borrow().scroll_offset(),
            7,
            "applying an `explorer:sb` offset must move the tree, whichever \
             backend's drag emitted it"
        );
    }

    /// #1187: `minimap:<window_id>` — armed by `minimap_press` on a strip
    /// press — must move the *named window's* `scroll_top`, mirroring
    /// `editor:v_sb:<window_id>`. Asserted on the window's own scroll
    /// position, not just the table having an arm for the id.
    #[test]
    fn a_minimap_scrollbar_offset_moves_the_named_windows_scroll_top() {
        let mut engine = drag_engine();
        let win = engine.active_window_id();
        assert_eq!(engine.windows.get(&win).unwrap().view.scroll_top, 0);
        assert!(apply_scroll_offset(
            &mut engine,
            &format!("minimap:{}", win.0),
            123,
            ScrollApplyContext::default()
        ));
        assert_eq!(
            engine.windows.get(&win).unwrap().view.scroll_top,
            123,
            "applying a `minimap:<window_id>` offset must move that \
             window's scroll_top, whichever backend's drag emitted it"
        );
    }
}
