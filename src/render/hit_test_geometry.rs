use super::*;

// ─── Command-line click/selection geometry (#816) ───────────────────────────
//
// quadraui#705 shipped `CommandLineLayout::hit_test` / `selection_bounds` —
// the character-offset hit test the TUI-only inverted-cell read-back trick
// used to get "for free" (and that GTK could never get at all, since it has
// no such trick). These helpers are the ONE place both backends map a click
// point to a command-line character offset, replacing `ee26268`'s hand-rolled
// `col - editor_left` arithmetic in `tui_main::mouse` with a call into the
// primitive.
//
// Both backends cache the painted row's ABSOLUTE bounds on
// `Engine::command_line_rect` at paint time (mirrors `global_status_rect`) —
// "cache what paint produced" rather than re-deriving the row's geometry at
// click time. TUI's units are character cells (`char_width = 1.0`); GTK's are
// pixels (`char_width = backend.char_width()`).

/// Byte offset (quadraui's [`quadraui::CommandLineLayout::hit_test`] and
/// `selection_bounds` contract) converted to a **character-count** offset —
/// the unit `Engine::command_cursor` / `Engine::cmd_sel` use throughout
/// (mirrors `cmd_char_to_byte`'s inverse in `core::engine::keys`). A no-op
/// for ASCII-only command text (the overwhelming case); multibyte prefixes
/// (the #503/#705 class of bug) are exactly where it matters.
fn command_line_byte_to_char_idx(text: &str, byte_offset: usize) -> usize {
    text.get(..byte_offset)
        .map(|s| s.chars().count())
        .unwrap_or_else(|| text.chars().count())
}

/// Whether `point` lands inside the painted command-line row, in the same
/// ABSOLUTE units as `rect` (`Engine::command_line_rect`).
///
/// Pulled out as its own function (#816 review) because the inline version
/// GTK's `MouseDown` handler originally wrote only compared `point.y` against
/// `rect.y`/`rect.height` — never `point.x`. `command_line_rect`'s `x`/`width`
/// come from `main_content_bounds` (after the activity bar + sidebar), so it
/// does not span the full window width; a `y`-only check treats any click in
/// the bottom `line_height`-px band as "over the command line" regardless of
/// `x`, including clicks over the sidebar/activity bar or past the content
/// area on the right. Since the GTK window is undecorated
/// (`w.set_decorated(false)`), that band is also the *only* way to grab the
/// window's South/SW/SE resize edges (`ctx.window_edge`), so the y-only
/// version silently disabled all three, essentially always (the command line
/// is basically always painted). Delegates to
/// [`quadraui::Rect::contains`] — the shared point-in-rect primitive —
/// instead of re-deriving both bounds by hand a second time.
///
/// #1528: the GTK `MouseDown` handler that used this to skip the whole
/// command-line row before running `ctx.window_edge` no longer needs to —
/// `render::WINDOW_RESIZE_GRIP_PX`'s thin margin already keeps `window_edge`
/// from firing anywhere in the row except its own outermost sliver, so the
/// guard's job (stop a full `line_height`-tall margin from eating the whole
/// row) is now `window_edge`'s own margin's job instead. Kept as a public
/// helper — still the correct both-axes point-in-`command_line_rect` check
/// for anything that needs one — rather than deleted with its call site.
pub fn point_over_command_line(rect: quadraui::Rect, point: quadraui::Point) -> bool {
    rect.width > 0.0 && rect.contains(point)
}

/// Whether the command/message line accepts a mouse-driven text selection in
/// `engine`'s current mode — the gate `ee26268` wrote twice (TUI's
/// Command/Search press arm vs. its separate Normal-with-message arm) as one
/// function, so GTK's press handler (#816) states the identical rule instead
/// of re-deriving it.
///
/// In `Normal`/`Visual`/`VisualLine`, this must agree with
/// [`build_command_line`] about *which text is actually painted*: when
/// `engine.peek_count()` is `Some`, the row shows the pending count instead
/// of `engine.message` (e.g. typing enough digits to hit the "Count limited
/// to 10,000" cap leaves both a live count AND a non-empty message at the
/// same time — `core::engine::keys` sets them together). Gating on
/// `!message.is_empty()` alone let a click there arm a selection whose
/// indices were hit-tested against the painted count text but whose Ctrl+C
/// copy read `engine.message` — a real, reachable text/geometry mismatch
/// (#816 review), not a hypothetical.
pub fn command_line_selection_allowed(engine: &Engine) -> bool {
    match engine.mode {
        Mode::Command | Mode::Search => true,
        Mode::Normal | Mode::Visual | Mode::VisualLine | Mode::VisualBlock => {
            engine.peek_count().is_none() && !engine.message.is_empty()
        }
        _ => false,
    }
}

/// Map a click/drag `point` to a **character-count** offset into `text`, via
/// quadraui's `CommandLine::layout(rect, ..).hit_test(x)` (issue #705/#816).
/// `rect` must be the ABSOLUTE bounds the line was painted into
/// (`Engine::command_line_rect`); `point` outside its row, or left of its
/// left edge, is not a hit. The primitive itself clamps `x` past the last
/// character to `text.len()` (previously unclamped: `ee26268`'s raw
/// `col - editor_left` could exceed the text and only `command_cursor`'s
/// separate `.min(buf_len)` caught it — `cmd_sel` itself did not).
pub fn command_line_click_char_idx(
    rect: quadraui::Rect,
    text: &str,
    char_width: f32,
    point: quadraui::Point,
) -> Option<usize> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    if point.y < rect.y || point.y >= rect.y + rect.height || point.x < rect.x {
        return None;
    }
    let cmd = quadraui::CommandLine {
        id: quadraui::WidgetId::new("cmdline:hit-test"),
        text: text.to_string(),
        cursor_offset: None,
        right_align: false,
    };
    let layout = cmd.layout(rect, quadraui::CommandLineMeasure::new(char_width));
    Some(command_line_byte_to_char_idx(
        text,
        layout.hit_test(point.x),
    ))
}

/// Convert a mouse selection `sel` (character-count `(anchor, head)`,
/// either order — the unit `Engine::cmd_sel` uses throughout, INCLUSIVE at
/// both ends: `route_cmdline_selection_key`'s Ctrl+C copy keeps every char
/// with `lo <= i <= hi`, and a bare click with no drag sets `(idx, idx)`
/// to highlight that one character) into the byte-offset `(start, end)`
/// pair [`quadraui::Backend::draw_command_line_selection`] expects, relative
/// to `text`. That pair is EXCLUSIVE at `end` (same contract as
/// `CommandLineLayout::selection_bounds`, which supplies its geometry) —
/// hence the `hi + 1` below, not a straight per-endpoint byte conversion.
/// Both backends' paint paths call this immediately before handing `cmd_sel`
/// to that method (issue #1185 — the consume side of quadraui#1001, which
/// shipped the primitive this function now feeds).
pub fn command_line_selection_bytes(text: &str, sel: (usize, usize)) -> (usize, usize) {
    let lo = sel.0.min(sel.1);
    let hi = sel.0.max(sel.1);
    (
        quadraui::text_util::char_to_byte_idx(text, lo),
        quadraui::text_util::char_to_byte_idx(text, hi + 1),
    )
}

// ─── Shared click target + layout geometry helpers ──────────────────────────
//
// These types and functions are used by all backends (GTK, TUI, Win-GUI) to
// avoid duplicating hit-testing geometry calculations.

/// Result of converting a click coordinate to a semantic editor target.
/// Shared across all backends.
#[derive(Debug, Clone, PartialEq)]
pub enum ClickTarget {
    /// Click was in the tab bar, tab already switched.
    TabBar,
    /// Click was in gutter — fold already toggled.
    Gutter,
    /// Click resolved to a buffer position in a specific window.
    BufferPos(WindowId, usize, usize),
    /// Click was on a tab's close button: (group_id, tab_idx). The actual
    /// close (and dirty-buffer confirmation) is applied by the caller via
    /// `Engine::handle_tab_bar_click` — every other tab-bar target is applied
    /// eagerly during resolution (#814), but a close needs to defer to a
    /// confirmation dialog before the engine mutates anything.
    CloseTab(GroupId, usize),
    /// Click was on a per-window status bar segment with an action.
    StatusBarAction(StatusAction),
    /// Click was on the editor action menu button.
    ActionMenuButton(GroupId),
    /// Click was on the code-overview minimap — the window has *already* been
    /// scrolled to the clicked fraction of the file (#35), like `Gutter`'s
    /// "fold already toggled". Carries the buffer line seeked to.
    Minimap(WindowId, usize),
    /// Click was outside any actionable area.
    None,
}

// ─── Shared screen-level hit-test (#344) ─────────────────────────────────────

/// Top-level screen zone identified by a coordinate hit-test.
///
/// Coordinates are in the "editor content bounds" frame — both backends
/// subtract their chrome (sidebar, menu bar, terminal panel, status bar)
/// before calling [`screen_zone_hit_test`].
#[derive(Debug)]
pub enum ScreenZone {
    /// Point is in a group's tab bar area.
    TabBar {
        group_id: GroupId,
        local_x: f64,
        bar_width: f64,
    },
    /// Point is on a breadcrumb bar.
    Breadcrumb {
        index: usize,
        local_x: f64,
        bar_width: f64,
    },
    /// Point is on a group divider.
    GroupDivider { split_index: usize },
    /// Point is in an editor window.
    Window {
        window_id: WindowId,
        window_idx: usize,
        rel_x: f64,
        rel_y: f64,
    },
    /// Point is outside all editor zones.
    None,
}

/// Sub-zone within an editor window.
#[derive(Debug)]
pub enum WindowZone {
    /// Per-window status bar.
    StatusBar { local_x: f64, bar_width: f64 },
    /// Gutter area (breakpoint, git diff, fold indicator columns).
    Gutter {
        view_row: usize,
        gutter_col: usize,
        line_idx: usize,
    },
    /// Vertical scrollbar column.
    VerticalScrollbar { view_row: usize },
    /// Horizontal scrollbar row.
    HorizontalScrollbar { local_x: f64 },
    /// Text area (editable content).
    TextArea {
        view_row: usize,
        buf_line: usize,
        seg_col_offset: usize,
        text_rel_x: f64,
    },
    /// The minimap strip's own column range, plus the scroll-affordance
    /// gutter it always leaves clear alongside it (#1094 review). Neither
    /// is text: real presses on the strip are resolved earlier by
    /// `apply_minimap_click`, before `window_zone_hit_test` ever runs, so
    /// every caller here treats this as a dead zone — the `_` arm in
    /// `click.rs::pixel_to_click_target`'s match, and the `let ... else` /
    /// `if let WindowZone::TextArea` patterns in `tui_main/mouse.rs`, both
    /// already fall through to "not resolved" for any non-`TextArea`
    /// variant without needing an explicit arm for this one. It exists so a
    /// text-selection drag whose pointer sweeps over the strip's pixels
    /// stops extending the selection there, matching the pre-#1094 outcome
    /// (when the narrower `rect` made such a point fall outside the window
    /// entirely) without re-narrowing `rect`, which #1094 deliberately
    /// stopped doing.
    Minimap,
}

/// Action to take on a gutter click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GutterAction {
    ToggleBreakpoint(usize),
    DiffPeek(usize),
    DiagnosticHover(usize),
    CodeAction(usize),
    ToggleFold(usize),
}

/// One group's tab-bar hit band: the rectangle a click must land in for
/// [`screen_zone_hit_test`] to report [`ScreenZone::TabBar`] for that group.
///
/// Deliberately mirrors [`TabBarDrawTarget`] on the *click* side (#553 — the
/// counterpart of #549's draw-loop unification): both are "which groups have a
/// tab bar this frame, and where is it?", so they must not be re-derived by two
/// independently-drifting branches. `TabBarDrawTarget` can't just be reused
/// here because it needs an `&Engine` and the backend's own single-group rect,
/// neither of which a pure hit-test over a cached `ScreenLayout` has.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabBarHitBand {
    pub group_id: GroupId,
    /// Left edge of the bar, in the caller's coordinate space.
    pub x: f64,
    /// Top edge of the reserved tab-bar band (tab row + breadcrumb row, if on).
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl TabBarHitBand {
    pub(crate) fn contains(&self, x: f64, y: f64) -> bool {
        y >= self.y && y < self.y + self.height && x >= self.x && x < self.x + self.width
    }
}

/// The tab-bar hit bands for every group that drew a tab bar this frame.
///
/// Single-group and split-group layouts used to derive this inline in two
/// separate `if let Some(split) = ... else { ... }` arms of
/// [`screen_zone_hit_test`], and drifted: the single-group arm hardcoded the
/// bar's top at `y >= 0.0` instead of deriving it from the window rects the way
/// the split arm did, so once #552 gave GTK a persistent menu/title-bar band
/// (`main_content_bounds.y > 0`) single-group tab clicks — activate *and* close
/// — silently stopped matching, while split layouts kept working (#546
/// FAILED-3, #553). Both arms now go through this one function so the two
/// shapes cannot diverge again.
///
/// In both cases the band's top edge comes from the *window content* top edge
/// minus `tab_bar_height`; the bar is drawn immediately above the content it
/// belongs to. `single_tab_hidden` is `is_tab_bar_hidden(active_group)`, which
/// can only be true in single-group mode (it requires `leaf_count() <= 1`), so
/// the split arm needs no equivalent filter.
///
/// # How much of the live click path this actually covers
///
/// Worth being precise about, since the historical defect above predates the
/// current routing: [`screen_zone_hit_test`] — and therefore this function —
/// has exactly one caller, `gtk::click` (`pixel_to_click_target` /
/// `resolve_tab_right_click`), and there it is the **fallback**. GTK resolves
/// clicks first through the cached `quadraui::FrameHitMap` (#449), into which
/// every TabBar surface is pushed on each `render_content` pass, so in steady
/// state a tab click is answered by the hit map and never reaches here; this
/// path serves hit-map misses and clicks arriving before the first paint. TUI
/// does not call it at all — `tui_main::mouse` has its own hit-test. So the
/// unification below is best read as removing the *shape* of divergence that
/// produced #546/#553 (one derivation instead of two that can drift), plus
/// correctness on the pre-paint/miss path — not as repairing an everyday
/// break for GTK users, which #449's hit map already covers.
pub fn tab_bar_hit_bands(
    layout: &ScreenLayout,
    tab_bar_height: f64,
    single_tab_hidden: bool,
    active_group: GroupId,
) -> Vec<TabBarHitBand> {
    if layout.editor_group_split.is_some() {
        return layout
            .group_tab_bars
            .iter()
            .filter(|gtb| gtb.bounds.width > 0.0)
            .map(|gtb| TabBarHitBand {
                group_id: gtb.group_id,
                x: gtb.bounds.x,
                y: gtb.bounds.y - tab_bar_height,
                width: gtb.bounds.width,
                height: tab_bar_height,
            })
            .collect();
    }
    if single_tab_hidden || layout.tab_bar.is_empty() || layout.windows.is_empty() {
        return Vec::new();
    }
    // Single group: there is no per-group `bounds`, so derive the bar from the
    // bounding box of the window rects it sits above — the same source of truth
    // `GroupTabBar::bounds` gives the split arm. `x`/`y` and the window rects
    // live in whatever space the caller built `ScreenLayout` in; TUI's is
    // content-relative (window rects start at `tab_bar_height`), GTK's is
    // absolute screen space anchored at `main_content_bounds`.
    let min_x = layout
        .windows
        .iter()
        .map(|w| w.rect.x)
        .fold(f64::MAX, f64::min);
    let min_y = layout
        .windows
        .iter()
        .map(|w| w.rect.y)
        .fold(f64::MAX, f64::min);
    let max_x = layout
        .windows
        .iter()
        .map(|w| w.rect.x + w.rect.width)
        .fold(f64::MIN, f64::max);
    let width = max_x - min_x;
    if width <= 0.0 {
        return Vec::new();
    }
    vec![TabBarHitBand {
        group_id: active_group,
        x: min_x,
        y: min_y - tab_bar_height,
        width,
        height: tab_bar_height,
    }]
}

/// Determine which top-level screen zone a point falls in.
///
/// `x` and `y` are in the editor content-bounds coordinate system.
/// `tab_bar_height` is the height of a tab bar row (in the same unit).
/// `single_tab_hidden` should be `true` when `hide_single_tab` is active and
/// there is only one tab — the tab bar row is not rendered and the window rect
/// extends upward to reclaim the space.
/// `active_group` is the engine's current active group ID — used as the group
/// ID for single-group tab bar hits (the ScreenLayout doesn't carry it).
/// Both backends subtract their own chrome before calling this.
pub fn screen_zone_hit_test(
    layout: &ScreenLayout,
    x: f64,
    y: f64,
    tab_bar_height: f64,
    single_tab_hidden: bool,
    active_group: GroupId,
) -> ScreenZone {
    // 1. Tab bars — check before windows because tab bars sit just above
    //    the window content area within the same group bounds. Split and
    //    single-group layouts share one derivation (`tab_bar_hit_bands`) so the
    //    two can't drift apart again the way they did in #546/#553.
    for band in tab_bar_hit_bands(layout, tab_bar_height, single_tab_hidden, active_group) {
        if band.contains(x, y) {
            return ScreenZone::TabBar {
                group_id: band.group_id,
                local_x: x - band.x,
                bar_width: band.width,
            };
        }
    }

    // 2. Breadcrumbs — sit within the tab-bar area, below the tab row.
    for (i, bc) in layout.breadcrumbs.iter().enumerate() {
        let b = &bc.bounds;
        if x >= b.x && x < b.x + b.width && y >= b.y && y < b.y + b.height {
            return ScreenZone::Breadcrumb {
                index: i,
                local_x: x - b.x,
                bar_width: b.width,
            };
        }
    }

    // 3. Group dividers. Naturally a no-op in single-group mode —
    // `group_dividers` is empty there (#551).
    {
        for div in &layout.group_dividers {
            let hit = match div.direction {
                SplitDirection::Vertical => {
                    let div_x = div.position;
                    (x - div_x).abs() < 0.5
                        && y >= div.cross_start
                        && y < div.cross_start + div.cross_size
                }
                SplitDirection::Horizontal => {
                    let div_y = div.position;
                    (y - div_y).abs() < 0.5
                        && x >= div.cross_start
                        && x < div.cross_start + div.cross_size
                }
            };
            if hit {
                return ScreenZone::GroupDivider {
                    split_index: div.split_index,
                };
            }
        }
    }

    // 4. Windows.
    for (i, rw) in layout.windows.iter().enumerate() {
        let r = &rw.rect;
        if x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height {
            return ScreenZone::Window {
                window_id: rw.window_id,
                window_idx: i,
                rel_x: x - r.x,
                rel_y: y - r.y,
            };
        }
    }

    ScreenZone::None
}

// ─── Divider hit-test / drag (shared by GroupLayout and WindowLayout, #582) ──
//
// #818 adopted `quadraui::SplitTree::layout` for the geometry *computation*
// this hit-test code consumes — `GroupLayout`/`WindowLayout::calculate_rects`
// and `::dividers` in `core/window.rs` used to re-derive the same split math
// in two separate hand-rolled recursive passes (the exact "second source of
// truth" risk `SplitTree`'s module docs call out); both now build a
// `quadraui::SplitTree` and read leaf rects + divider geometry off one
// `layout()` call.
//
// The hit-test/drag code below (`DividerGeometry`, `divider_hit_test`,
// `divider_ratio_from_pos`, `DividerMetrics`/`GTK_DIVIDER_METRICS`,
// `route_divider_grab`, `apply_divider_drag`) deliberately stays local
// rather than also moving onto `quadraui::SplitTreeLayout::hit_test_divider`/
// `hit_test_divider_cell`: those two methods only support a *symmetric*
// tolerance band (continuous) or an *exact single-cell* match (quantized).
// Neither can express the asymmetric multi-cell bands both backends need —
// GTK asks for `(6.0, 6.0)` uniformly, but TUI's `tol_before`/`tol_after`
// differ per divider (`(1.0, 1.0)` for most, `(0.0, tab_bar_rows)` for a
// horizontal group divider, whose grabbable band *is* the neighbouring
// group's whole tab-bar block — see `tui_main::mouse`'s call site). Faking
// that through `SplitTreeDivider`'s `thickness` field would require
// constructing a divider whose `thickness` differs from what was actually
// painted, defeating the "one number describes what was drawn" contract
// `cell_position()`'s doc comment relies on. This is the concrete gap #818
// asks to be filed against quadraui (asymmetric/multi-cell tolerance bands
// on `SplitTreeLayout::hit_test_divider`/`hit_test_divider_cell`) rather
// than papered over here.

/// Common geometry accessor so [`divider_hit_test`] and
/// [`divider_ratio_from_pos`] work identically over `GroupDivider`
/// (editor-group splits) and `WindowDivider` (in-group `:split`/`:vsplit`
/// window splits) without duplicating the hit-test/drag math per divider
/// kind (or, previously, per backend — see below).
pub trait DividerGeometry {
    fn direction(&self) -> SplitDirection;
    fn position(&self) -> f64;
    fn axis_start(&self) -> f64;
    fn axis_size(&self) -> f64;
    fn cross_start(&self) -> f64;
    fn cross_size(&self) -> f64;

    /// Screen space consumed by the divider itself, along the split axis —
    /// `axis_size() - divider_thickness()` is the *content* width/height the
    /// split's `ratio` actually divides (see [`divider_ratio_from_pos`]).
    /// Zero for every divider today except a `WindowDivider` in the
    /// `Vertical` direction, which — unlike `GroupDivider`'s editor-group
    /// splits, still `quadraui::SplitTreeMeasure::new(0.0)` — reserves one
    /// screen column for Neovim's real vertical divider bar
    /// (`WindowLayout::layout_snapped`, #1326). `position`/`axis_start`/
    /// `axis_size` themselves are untouched by this (they describe what was
    /// actually *painted*, which `divider_to_split`/`divider_hit_test` must
    /// stay anchored to) — only the drag-to-ratio conversion needs to know
    /// the reserved thickness, so it is its own method rather than folding
    /// into `axis_size()`.
    fn divider_thickness(&self) -> f64 {
        0.0
    }
}

impl DividerGeometry for GroupDivider {
    fn direction(&self) -> SplitDirection {
        self.direction
    }
    fn position(&self) -> f64 {
        self.position
    }
    fn axis_start(&self) -> f64 {
        self.axis_start
    }
    fn axis_size(&self) -> f64 {
        self.axis_size
    }
    fn cross_start(&self) -> f64 {
        self.cross_start
    }
    fn cross_size(&self) -> f64 {
        self.cross_size
    }
}

impl DividerGeometry for WindowDivider {
    fn direction(&self) -> SplitDirection {
        self.direction
    }
    fn position(&self) -> f64 {
        self.position
    }
    fn axis_start(&self) -> f64 {
        self.axis_start
    }
    fn axis_size(&self) -> f64 {
        self.axis_size
    }
    fn cross_start(&self) -> f64 {
        self.cross_start
    }
    fn cross_size(&self) -> f64 {
        self.cross_size
    }
    fn divider_thickness(&self) -> f64 {
        match self.direction {
            SplitDirection::Vertical => 1.0,
            SplitDirection::Horizontal => 0.0,
        }
    }
}

/// Asymmetric hit-band tolerance around a divider's `position`, along the
/// split axis: `(before, after)`.
pub type DividerTolerance = (f64, f64);

/// Hit-test a point against a list of dividers. Returns the index *into
/// `dividers`* (not `split_index`) so callers can recover any extra fields
/// (e.g. `WindowDivider::group_id`) from the matched element.
///
/// A single divider list can mix vertical and horizontal splits (nested
/// splits alternate direction), so tolerance is supplied per-direction:
/// `vertical_tol`/`horizontal_tol`. GTK wants a symmetric pixel tolerance
/// around the thin divider line in both directions (`before == after`),
/// while TUI's editor-group horizontal divider is grabbable across the
/// *second* group's whole tab-bar block rather than a single row
/// (`before == 0`, `after == tab_bar_rows` — see the call site for why).
///
/// `quantize`: TUI must hit-test against the *same truncated* position the
/// renderer draws at (`div.position as u16`), matching the renderer exactly
/// so a click on the rendered glyph always hits (see #452 — using a plain
/// tolerance window centered on the untruncated float position could match
/// the wrong adjacent cell). GTK renders at the continuous pixel position, so
/// it passes `false`.
pub fn divider_hit_test<D: DividerGeometry>(
    dividers: &[D],
    x: f64,
    y: f64,
    vertical_tol: DividerTolerance,
    horizontal_tol: DividerTolerance,
    quantize: bool,
) -> Option<usize> {
    for (i, div) in dividers.iter().enumerate() {
        let pos = if quantize {
            (div.position() as u16) as f64
        } else {
            div.position()
        };
        let (axis, cross, (tol_before, tol_after)) = match div.direction() {
            SplitDirection::Vertical => (x, y, vertical_tol),
            SplitDirection::Horizontal => (y, x, horizontal_tol),
        };
        let hit = axis >= pos - tol_before
            && axis < pos + tol_after
            && cross >= div.cross_start()
            && cross < div.cross_start() + div.cross_size();
        if hit {
            return Some(i);
        }
    }
    None
}

/// Given a divider being dragged and the current pointer position, compute
/// the new split ratio (unclamped — `set_ratio_at_index` on both
/// `GroupLayout` and `WindowLayout` already clamps to `0.1..0.9`).
///
/// Divides by `axis_size() - divider_thickness()`, not raw `axis_size()`
/// (#1326): `WindowLayout::layout_snapped` interprets a `Vertical` split's
/// `ratio` as a fraction of the *content* width (bounds width minus the
/// reserved divider column), so a drag that instead divided by the full
/// `axis_size()` would feed back a ratio the layout doesn't mean — a
/// perceptible drift on a narrow split, since the denominators differ by a
/// whole screen column. `GroupDivider`'s `divider_thickness()` is always
/// zero, so this is a no-op there.
pub fn divider_ratio_from_pos(div: &impl DividerGeometry, x: f64, y: f64) -> f64 {
    let mouse_pos = match div.direction() {
        SplitDirection::Vertical => x,
        SplitDirection::Horizontal => y,
    };
    let content_axis_size = (div.axis_size() - div.divider_thickness()).max(1.0);
    (mouse_pos - div.axis_start()) / content_axis_size
}

/// Convert a [`DividerGeometry`] divider into the `(quadraui::Split,
/// quadraui::Rect)` pair a backend needs to paint it via the shared
/// `backend.draw_split()` primitive (#582 follow-up).
///
/// `WindowLayout`/`GroupLayout` dividers had no dedicated visual — TUI
/// vertical splits only looked divided by coincidence (the neighbouring
/// window's own scrollbar/separator column), and GTK painted nothing at
/// all for `:vsplit`. Rather than hand-roll a second per-backend line
/// renderer, reuse quadraui's existing `Split` primitive (`primitives/
/// split.rs`, already wired for `backend.draw_split` in both backends) —
/// it draws exactly one divider line from a ratio + bounds, which is all
/// a single `DividerGeometry` node needs (the fact that `WindowLayout` as
/// a whole is an N-way tree doesn't matter here: each *divider* is always
/// a 2-pane boundary).
///
/// `ratio` is back-derived from `position`/`axis_start`/`axis_size`
/// (rather than threading the tree's own stored ratio through) so this
/// works uniformly for both `GroupDivider` and `WindowDivider`, neither of
/// which carries a ratio field.
pub fn divider_to_split(
    div: &impl DividerGeometry,
    id: quadraui::WidgetId,
) -> (quadraui::Split, quadraui::Rect) {
    let ratio = ((div.position() - div.axis_start()) / div.axis_size()) as f32;
    // vimcode's `Vertical` = side-by-side panes = quadraui's `Horizontal`
    // (their `Split::direction` names the divider's own orientation relative
    // to "panes side by side" vs "panes stacked", the inverse of vimcode's
    // "divider direction" naming — see `core::window::to_quadraui_direction`
    // and primitives/split.rs).
    let direction = crate::core::window::to_quadraui_direction(div.direction());
    let rect = match div.direction() {
        SplitDirection::Vertical => quadraui::Rect::new(
            div.axis_start() as f32,
            div.cross_start() as f32,
            div.axis_size() as f32,
            div.cross_size() as f32,
        ),
        SplitDirection::Horizontal => quadraui::Rect::new(
            div.cross_start() as f32,
            div.axis_start() as f32,
            div.cross_size() as f32,
            div.axis_size() as f32,
        ),
    };
    let split = quadraui::Split {
        id,
        direction,
        ratio,
        first_min: 0.0,
        second_min: 0.0,
    };
    (split, rect)
}

/// Find which window contains a point and return its index.
///
/// Coordinates are in the same frame as `screen_zone_hit_test` — editor
/// content bounds, after subtracting sidebar/menu/terminal chrome.
pub fn find_window_at(layout: &ScreenLayout, x: f64, y: f64) -> Option<usize> {
    layout.windows.iter().position(|rw| {
        let r = &rw.rect;
        x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
    })
}

/// Determine which sub-zone of a window a point falls in.
///
/// `rel_x` and `rel_y` are relative to the window's top-left corner.
/// `line_height` and `char_width` are in the same coordinate unit as the rect
/// (pixels for GTK, 1.0 for TUI).
pub fn window_zone_hit_test(
    rw: &RenderedWindow,
    rel_x: f64,
    rel_y: f64,
    line_height: f64,
    char_width: f64,
) -> WindowZone {
    let has_status = rw.status_line.is_some();
    let status_h = if has_status { line_height } else { 0.0 };
    let content_h = rw.rect.height - status_h;
    let viewport_lines = (content_h / line_height).floor() as usize;

    // 1. Per-window status bar (bottom row of window).
    if has_status && rel_y >= content_h {
        return WindowZone::StatusBar {
            local_x: rel_x,
            bar_width: rw.rect.width,
        };
    }

    let view_row = (rel_y / line_height).floor() as usize;

    let gutter_w = rw.gutter_char_width as f64 * char_width;
    let has_v_sb = rw.total_lines > viewport_lines;
    let sb_w = if has_v_sb { char_width } else { 0.0 };
    // #1094 review: reuse `rw.text_viewport_cols` — already computed by
    // `build_rendered_window` with the minimap strip's width (and any
    // scrollbar overlay reserve) subtracted — instead of an independent
    // recomputation from `rw.rect.width` that had no knowledge of the strip
    // at all. `rw.rect` now reaches the pane's *true* right edge (#1094's
    // own fix moved the scrollbar there), so re-deriving `viewport_cols`
    // from it directly would silently include the strip's columns,
    // undercounting how often `has_h_sb` should be true and letting a
    // click on a visibly-painted horizontal scrollbar fall through to
    // `TextArea` instead.
    let viewport_cols = rw.text_viewport_cols.max(1);
    let has_h_sb = rw.max_col > viewport_cols && viewport_lines > 1;

    // 2. Vertical scrollbar (rightmost column).
    if has_v_sb && rel_x >= rw.rect.width - sb_w {
        return WindowZone::VerticalScrollbar { view_row };
    }

    // 3. Minimap strip, plus the scroll-affordance gutter it always leaves
    // clear alongside it (#1094 review) — see `WindowZone::Minimap`'s doc
    // comment for why this has to be excluded here rather than relying on
    // `apply_minimap_click` alone (that resolver is skipped for
    // text-selection drag continuations). Checked after the vertical
    // scrollbar so a shown scrollbar's own pixels still resolve to
    // `VerticalScrollbar` above; this only ever matches the strip itself,
    // or (when the vertical scrollbar isn't currently shown) the sliver of
    // gutter that stays reserved for it regardless.
    if rw.minimap_reserved_w > 0.0 && rel_x >= rw.rect.width - rw.minimap_reserved_w {
        return WindowZone::Minimap;
    }

    // 4. Horizontal scrollbar (bottom content row, above status bar).
    let h_sb_y = content_h - line_height;
    if has_h_sb && rel_y >= h_sb_y && rel_y < content_h {
        return WindowZone::HorizontalScrollbar {
            local_x: rel_x - gutter_w,
        };
    }

    // Resolve view row to buffer line via cached RenderedLine data.
    let (line_idx, seg_col_offset) = rw
        .lines
        .get(view_row)
        .map(|rl| (rl.line_idx, rl.segment_col_offset))
        .unwrap_or((rw.scroll_top + view_row, 0));

    // 5. Gutter.
    if gutter_w > 0.0 && rel_x < gutter_w {
        let gutter_col = if char_width > 0.0 {
            (rel_x / char_width).floor() as usize
        } else {
            0
        };
        return WindowZone::Gutter {
            view_row,
            gutter_col,
            line_idx,
        };
    }

    // 6. Text area.
    let text_rel_x = rel_x - gutter_w;
    WindowZone::TextArea {
        view_row,
        buf_line: line_idx,
        seg_col_offset,
        text_rel_x,
    }
}

/// Outcome of resolving a click against an editor window's own scrollbar
/// track (vertical or horizontal), returned by
/// [`resolve_editor_scrollbar_click`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditorScrollbarClick {
    /// The click landed on the empty track, outside the thumb: page one
    /// viewport toward the click. The new scroll offset to apply.
    PageTo(usize),
    /// The click landed on the thumb itself: begin a drag. `grab_offset`
    /// (native units — pixels for GTK, whole cells for TUI) is the click's
    /// offset from the thumb's leading edge, preserved so the thumb doesn't
    /// jump out from under the cursor when the drag starts.
    BeginDrag { grab_offset: f32 },
}

/// Resolve a click on an editor window's scrollbar track — thumb-drag vs.
/// track page-jump — independent of axis (vertical/horizontal) and backend
/// geometry units (`unit_w`/`unit_h` convention: pixels for GTK, whole
/// cells for TUI).
///
/// #1061: this exact three-way decision (page toward the click on the empty
/// track either side of the thumb; begin a drag, with a grab offset that
/// keeps the cursor's relative position on the thumb, when the click lands
/// on the thumb itself) was hand-rolled four times — once per axis, once
/// per backend (`tui_main/mouse.rs`'s v/h scrollbar arms, `app.rs`'s v/h
/// scrollbar arms) — plus a fifth, independent re-derivation of the same
/// thumb math inside TUI's own `scrollbar_grab_offset` helper (now
/// deleted), which the other three copies never needed because their
/// grab-offset math already reused the same thumb bounds as their
/// page-vs-thumb decision. This is that one decision, shared.
///
/// `click_pos` is the click's position along the scroll axis (row for
/// vertical, column for horizontal — TUI; y/x — GTK), in the same native
/// unit and coordinate origin as `thumb_start`/`thumb_end`. `thumb_start`/
/// `thumb_end` are the thumb's absolute bounds along that axis — callers
/// derive them however their own backend already does (both currently via
/// `quadraui::fit_thumb`, TUI additionally quantizing to whole cells so the
/// click decision matches what's actually painted on a terminal grid).
/// `track_visible` is how many lines/cols fit in one page (a click-derived
/// scroll jumps by exactly this much); `max_scroll` is the largest valid
/// scroll offset; `current_scroll` is the scroll offset at click time.
pub fn resolve_editor_scrollbar_click(
    click_pos: f32,
    thumb_start: f32,
    thumb_end: f32,
    track_visible: usize,
    max_scroll: usize,
    current_scroll: usize,
) -> EditorScrollbarClick {
    if click_pos < thumb_start {
        EditorScrollbarClick::PageTo(current_scroll.saturating_sub(track_visible))
    } else if click_pos >= thumb_end {
        EditorScrollbarClick::PageTo((current_scroll + track_visible).min(max_scroll))
    } else {
        EditorScrollbarClick::BeginDrag {
            grab_offset: click_pos - thumb_start,
        }
    }
}

#[cfg(test)]
mod editor_scrollbar_click_tests {
    //! #1061: `resolve_editor_scrollbar_click` replaces four hand-rolled
    //! copies (TUI v/h in `tui_main/mouse.rs`, GTK v/h in `app.rs`) of this
    //! same three-way decision. These pin the pure decision logic directly;
    //! `harness.rs`'s `issue_987_group_scrollbar_inert_and_click_resizes`
    //! module and its new `tui_prod`-arm sibling drive it end-to-end through
    //! both backends' real click handlers.
    use super::*;

    #[test]
    fn click_above_thumb_pages_backward_by_one_viewport() {
        // Track [0, 100), thumb [40, 50), click at 10 (above the thumb).
        let outcome = resolve_editor_scrollbar_click(10.0, 40.0, 50.0, 20, 80, 40);
        assert_eq!(outcome, EditorScrollbarClick::PageTo(20));
    }

    #[test]
    fn click_above_thumb_saturates_at_zero() {
        let outcome = resolve_editor_scrollbar_click(2.0, 40.0, 50.0, 20, 80, 5);
        assert_eq!(outcome, EditorScrollbarClick::PageTo(0));
    }

    #[test]
    fn click_below_thumb_pages_forward_by_one_viewport() {
        // Track [0, 100), thumb [40, 50), click at 90 (below the thumb).
        let outcome = resolve_editor_scrollbar_click(90.0, 40.0, 50.0, 20, 80, 40);
        assert_eq!(outcome, EditorScrollbarClick::PageTo(60));
    }

    #[test]
    fn click_below_thumb_clamps_to_max_scroll() {
        let outcome = resolve_editor_scrollbar_click(90.0, 40.0, 50.0, 20, 55, 40);
        assert_eq!(outcome, EditorScrollbarClick::PageTo(55));
    }

    #[test]
    fn click_on_thumb_begins_a_drag_with_the_grab_offset_preserved() {
        // Thumb spans [40, 50); a click at 43 is 3 units into it.
        let outcome = resolve_editor_scrollbar_click(43.0, 40.0, 50.0, 20, 80, 40);
        assert_eq!(
            outcome,
            EditorScrollbarClick::BeginDrag { grab_offset: 3.0 }
        );
    }

    #[test]
    fn click_exactly_on_thumb_start_begins_a_drag_at_zero_offset() {
        let outcome = resolve_editor_scrollbar_click(40.0, 40.0, 50.0, 20, 80, 40);
        assert_eq!(
            outcome,
            EditorScrollbarClick::BeginDrag { grab_offset: 0.0 }
        );
    }

    #[test]
    fn click_exactly_on_thumb_end_is_track_not_drag() {
        // `thumb_end` is exclusive — this is the boundary #987's own
        // negative-space case cares about: a click one unit past the
        // thumb must not be swallowed as a drag.
        let outcome = resolve_editor_scrollbar_click(50.0, 40.0, 50.0, 20, 80, 40);
        assert_eq!(outcome, EditorScrollbarClick::PageTo(60));
    }
}

/// Resolve a gutter click to an action based on column and line data.
pub fn resolve_gutter_action(
    rw: &RenderedWindow,
    line_idx: usize,
    gutter_col: usize,
) -> Option<GutterAction> {
    let bp_offset: usize = if rw.has_breakpoints { 1 } else { 0 };
    let git_col = if rw.has_git_diff {
        bp_offset
    } else {
        usize::MAX
    };

    if rw.has_breakpoints && gutter_col == 0 {
        Some(GutterAction::ToggleBreakpoint(line_idx))
    } else if gutter_col == git_col {
        Some(GutterAction::DiffPeek(line_idx))
    } else if rw.diagnostic_gutter.contains_key(&line_idx) {
        Some(GutterAction::DiagnosticHover(line_idx))
    } else if rw.code_action_lines.contains(&line_idx) {
        Some(GutterAction::CodeAction(line_idx))
    } else {
        Some(GutterAction::ToggleFold(line_idx))
    }
}

/// Execute the engine-side effect of a resolved [`GutterAction`] for a
/// gutter click. Shared by both backends (#823 item 3) — GTK's
/// `execute_gutter_action` (`gtk/click.rs`) and TUI's inline match
/// (`tui_main/mouse.rs`) were an identical 5-arm match on
/// [`resolve_gutter_action`]'s result, except TUI additionally gated
/// `ToggleFold` on `gutter_text` actually containing a `+`/`-` fold
/// indicator glyph and GTK didn't. Folded in here rather than dropped: in
/// practice `Engine::toggle_fold_at_line`'s own `detect_fold_range` no-ops
/// when nothing is foldable at that line, so the two were never observed to
/// diverge in a live-reachable case, but the guard is one line to keep and
/// removing it would be a needless behavior narrowing for GTK to carry.
///
/// `gutter_text` is the resolved line's painted gutter glyphs — pass
/// `rw.lines[view_row].gutter_text.as_str()` (both backends already look
/// this row up before calling in).
pub fn apply_gutter_action(
    engine: &mut Engine,
    rw: &RenderedWindow,
    window_id: WindowId,
    line_idx: usize,
    gutter_col: usize,
    gutter_text: &str,
) {
    match resolve_gutter_action(rw, line_idx, gutter_col) {
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
            engine.active_tab_mut().focus_window(window_id);
            engine.view_mut().cursor.line = line;
            engine.open_diff_peek();
        }
        Some(GutterAction::DiagnosticHover(line)) => {
            engine.active_tab_mut().focus_window(window_id);
            engine.view_mut().cursor.line = line;
            engine.trigger_editor_hover_for_line(line);
        }
        Some(GutterAction::CodeAction(line)) => {
            engine.active_tab_mut().focus_window(window_id);
            engine.view_mut().cursor.line = line;
            engine.show_code_actions_popup();
        }
        Some(GutterAction::ToggleFold(line)) => {
            if gutter_text.chars().any(|c| c == '+' || c == '-') {
                engine.toggle_fold_at_line(line);
            }
        }
        None => {}
    }
}

/// Computed editor chrome layout — all heights in native units (pixels for
/// GTK/macOS, rows for TUI with `line_height = 1.0`).
#[derive(Debug, Clone, Copy)]
pub struct EditorLayout {
    pub tab_bar_h: f64,
    pub editor_top: f64,
    pub editor_bottom: f64,
    pub debug_toolbar_h: f64,
    pub quickfix_h: f64,
    pub terminal_h: f64,
    pub terminal_content_rows: u16,
    pub terminal_max_target_rows: u16,
    pub separated_status_h: f64,
    pub wildmenu_h: f64,
    pub status_bar_h: f64,
    pub command_line_h: f64,
}

/// One-shot layout computation used by all backends to derive editor window
/// rects and chrome positions. Reads engine state directly so callers don't
/// need to replicate the arithmetic.
///
/// * `total_height` — available viewport height (DA pixels for GTK, screen
///   rows as f64 for TUI).
/// * `line_height` — font line height (pixels for GTK, 1.0 for TUI).
/// * `menu_in_viewport` — `true` for TUI (menu bar is a content row),
///   `false` for GTK (menu bar is outside the DrawingArea).
pub fn compute_editor_layout(
    engine: &Engine,
    total_height: f64,
    line_height: f64,
    menu_in_viewport: bool,
) -> EditorLayout {
    let lh = line_height;
    let per_window = effective_window_status_line(engine);
    let bp_open = engine.terminal_open || engine.bottom_panel_open;

    let menu_h = if menu_in_viewport && engine.menu_bar_visible {
        lh
    } else {
        0.0
    };
    let tab_bar_h = if engine.terminal_maximized {
        0.0
    } else {
        tab_bar_height_px(lh, engine.settings.breadcrumbs)
    };
    let debug_toolbar_h = debug_toolbar_height_px(lh, engine.debug_toolbar_visible);
    // Shares one bottom "list rung" with the active window's location list
    // (#1155) — see `quickfix_panel_rows`, the TUI-side equivalent of this
    // same rule. #1307: a target with a real `WindowLayout` leaf takes its
    // own space from the split tree, so this overlay band must not reserve
    // rows for it too — see `quickfix_panel_rows`'s matching guard.
    let qf_or_loc_open = !engine.qf_has_real_window(None)
        && !engine.qf_has_real_window(Some(engine.active_window_id()))
        && ((engine.quickfix.open && !engine.quickfix.items.is_empty())
            || engine
                .location_lists
                .get(&engine.active_window_id())
                .is_some_and(|l| l.open && !l.items.is_empty()));
    let quickfix_h = if qf_or_loc_open { 6.0 * lh } else { 0.0 };
    let has_separated = per_window && !engine.settings.status_line_above_terminal && bp_open;
    let separated_status_h = separated_status_height_px(lh, has_separated);
    let wildmenu_h = if engine.wildmenu_items.is_empty() {
        0.0
    } else {
        lh
    };
    let status_bar_h = status_bar_height_px(
        lh,
        global_status_bar_visible(engine),
        !engine.wildmenu_items.is_empty(),
    );
    let command_line_h = lh;

    let (terminal_h, terminal_content_rows, terminal_max_target_rows) = if bp_open {
        let viewport_rows = (total_height / lh).floor() as u16;
        let chrome = PanelChromeDesc {
            viewport_rows,
            menu_rows: if menu_in_viewport && engine.menu_bar_visible {
                1
            } else {
                0
            },
            quickfix_rows: if qf_or_loc_open { 6 } else { 0 },
            debug_toolbar_rows: if engine.debug_toolbar_visible { 1 } else { 0 },
            wildmenu_rows: if engine.wildmenu_items.is_empty() {
                0
            } else {
                1
            },
            tab_bar_rows: if menu_in_viewport { 1 } else { 2 },
            separated_status_rows: if has_separated { 1 } else { 0 },
            status_cmd_rows: if per_window { 1 } else { 2 },
            panel_chrome_rows: 2,
            min_content_rows: 5,
        };
        let target = chrome.max_panel_content_rows();
        let rows = engine.effective_terminal_panel_rows(target);
        ((rows as f64 + 2.0) * lh, rows, target)
    } else {
        (0.0, 0, 0)
    };

    let editor_top = menu_h;
    let editor_bottom = total_height
        - status_bar_h
        - debug_toolbar_h
        - quickfix_h
        - terminal_h
        - separated_status_h;

    EditorLayout {
        tab_bar_h,
        editor_top,
        editor_bottom,
        debug_toolbar_h,
        quickfix_h,
        terminal_h,
        terminal_content_rows,
        terminal_max_target_rows,
        separated_status_h,
        wildmenu_h,
        status_bar_h,
        command_line_h,
    }
}

/// Fixed tab-bar row height in pixels, matching VS Code's `35px` tab height.
/// Used by GTK and Win-GUI backends.
///
/// #700: deliberately a constant, not derived from `line_height`. It used to
/// be `ceil(line_height * 1.6)`, so raising `settings.font_size` inflated the
/// tab-bar chrome right along with the editor text — VS Code's tab height is
/// fixed regardless of the editor's font size.
pub const TAB_ROW_HEIGHT_PX: f64 = 35.0;

/// Fixed breadcrumb row height in pixels, matching VS Code's compact
/// breadcrumb chrome. Used by GTK and Win-GUI backends.
///
/// #700: deliberately a constant, not `line_height` — same reasoning as
/// [`TAB_ROW_HEIGHT_PX`]. Only the row's reserved *height* is decoupled here;
/// the breadcrumb text itself still paints through
/// `Backend::draw_status_bar`, which has no per-call font override in the
/// pinned quadraui rev (unlike `draw_dialog`/`draw_rich_text_popup`, which
/// already take a `ui_font` description) — so breadcrumb text size still
/// tracks `settings.font_size` until that quadraui gap is closed. A
/// quadraui issue for a `draw_status_bar` font override must be filed —
/// do not treat #700's "breadcrumb text size independent of font_size"
/// acceptance bullet as satisfied until it lands; only the row height half
/// is fixed here.
pub const BREADCRUMB_ROW_HEIGHT_PX: f64 = 22.0;

/// Compute the tab bar row height in pixels (the row containing tab labels).
/// Used by GTK and Win-GUI backends. `line_height` is accepted for call-site
/// stability but no longer affects the result — see [`TAB_ROW_HEIGHT_PX`].
pub fn tab_row_height_px(_line_height: f64) -> f64 {
    TAB_ROW_HEIGHT_PX
}

/// Compute the full tab bar height including optional breadcrumb row.
/// Used by GTK and Win-GUI backends. `line_height` is accepted for
/// call-site stability but no longer affects the result — see
/// [`TAB_ROW_HEIGHT_PX`]/[`BREADCRUMB_ROW_HEIGHT_PX`].
pub fn tab_bar_height_px(_line_height: f64, breadcrumbs: bool) -> f64 {
    if breadcrumbs {
        TAB_ROW_HEIGHT_PX + BREADCRUMB_ROW_HEIGHT_PX
    } else {
        TAB_ROW_HEIGHT_PX
    }
}

/// One geometry unit [`crate::app::App`] paints in, chosen once by the
/// caller that constructs it (#1426) — never inferred at runtime by `App`
/// itself asking "am I GTK?", per the Platform-Neutrality Rule.
///
/// Before this struct existed, `App` hardcoded the pixel-flavoured half of
/// every backend-specific sizing pair listed on each field below (the
/// `gtk_*`/`GTK_*` name) directly in its own methods, which is correct for
/// GTK/macOS/Win but wrong on a cell grid — e.g. `TAB_ROW_HEIGHT_PX` (35.0)
/// read as 35 *rows* on an 80x24 terminal claims more rows than the terminal
/// has, collapsing the entire editor content band. Every pair already had a
/// `TUI_*`/cell-flavoured twin (built for the shipped the pre-#1434 TUI shell);
/// `UnitProfile` just bundles "which half of each pair" into one value
/// picked once, at construction, by code that already knows its own
/// backend: [`Self::px`] for GTK/macOS/Win, [`Self::cell`] for the `tui`
/// harness arm (`crate::tui_main::testing::conformance_harness` /
/// `crate::harness::build_app_and_config`).
///
/// `#[derive(Clone, Copy)]`: every field is either a plain value/tuple or a
/// capture-less `fn` pointer, so a whole profile is cheap to copy around
/// (`App` stores one directly, no `Rc`/`Box` needed).
#[derive(Clone, Copy)]
pub struct UnitProfile {
    /// [`tab_row_height_px`] (GTK: fixed 35px regardless of `lh`) vs a
    /// single cell row (`|lh| lh`, TUI: `lh` is always `1.0`).
    pub tab_row_h: fn(f64) -> f64,
    /// [`tab_bar_height_px`] (GTK: fixed 35/57px) vs `lh` or `2*lh` when
    /// breadcrumbs are on (TUI: one or two cell rows).
    pub tab_bar_h: fn(f64, bool) -> f64,
    /// [`BREADCRUMB_ROW_HEIGHT_PX`] (GTK: a fixed 22px regardless of `lh`)
    /// vs one more full cell row (`|lh| lh`, TUI).
    pub breadcrumb_row_h: fn(f64) -> f64,
    /// [`gtk_minimap_sizing`] vs [`TUI_MINIMAP_SIZING`] — both plain values,
    /// not functions, since neither depends on `line_height`.
    pub minimap: quadraui::MinimapSizing,
    /// [`gtk_picker_sizing`] vs [`TUI_PICKER_SIZING`] (ignores the `f32`).
    pub picker: fn(f32) -> PickerSizing,
    /// [`gtk_picker_rows`] vs [`TUI_PICKER_ROWS`] (ignores the `f32`).
    pub picker_rows: fn(f32) -> PickerRowMetrics,
    /// [`gtk_tab_switcher_sizing`] vs [`TUI_TAB_SWITCHER_SIZING`] (ignores
    /// the `f32`).
    pub tab_switcher: fn(f32) -> TabSwitcherSizing,
    /// [`GTK_FIND_REPLACE_ANCHOR`] vs [`TUI_FIND_REPLACE_ANCHOR`].
    pub find_replace_anchor: FindReplaceAnchor,
    /// [`GTK_DIVIDER_METRICS`] (ignores the `bool`) vs the cell metrics
    /// `tui_main::mouse`'s own divider rung builds inline. TUI's
    /// `group_horizontal` band reaches across however many rows the tab bar
    /// occupies (one, or two with breadcrumbs on) rather than back like
    /// every other band, so this needs the same `breadcrumbs` flag
    /// `App::render_content` already has in scope wherever it calls this.
    pub divider_metrics: fn(bool) -> DividerMetrics,
    /// [`gui_sidebar_system_metrics`] vs the fixed metrics
    /// the pre-#1434 TUI shell's `from_engine` seeds every `SidebarSystem` with once at
    /// startup (a cell grid never changes `line_height`, so TUI's never
    /// depends on the `f32`).
    pub sidebar_system_metrics: fn(f32) -> quadraui::MsvLayoutMetrics,
    /// Divider/tab-close grab tolerance: `(6.0, 6.0)` device pixels on GTK,
    /// `(1.0, 1.0)` cells on TUI — #1068's "irreducible" list.
    pub hit_tolerance: (f64, f64),
    /// [`quadraui::ShellConfig::with_activity_bar_width_px`]'s argument.
    /// `Some(48.0)` on GTK/macOS/Win (VS Code parity, independent of line
    /// height); `None` on TUI, which leaves
    /// [`quadraui::ShellConfig::activity_bar_width`]'s own default (a
    /// 3-line-height multiple) in charge — the same value
    /// the pre-#1434 TUI shell's `build_shell_config` sets explicitly.
    pub activity_bar_width_px: Option<f32>,
    /// [`quadraui::ShellConfig::with_title_bar`]'s `height_lh` argument.
    pub title_bar_lh: f32,
    /// [`quadraui::ShellConfig::default_sidebar_width`] — the width the
    /// sidebar opens at, in **line-height multiples** (quadraui's
    /// `AppShell::compute_layout` multiplies it by `line_height`, so one unit
    /// is one terminal row on TUI and ~23 device pixels on a GUI backend at
    /// the default editor font).
    ///
    /// #1798: this has to differ per profile, because the unit it is
    /// measured in differs by more than a factor of twenty. `App::
    /// shell_config` used to leave it at `ShellConfig::new`'s generic
    /// `20.0` on *every* backend. On TUI that is a 20-column sidebar beside
    /// an 80-ish column terminal — fine. On a GUI backend it is a **~460px**
    /// sidebar: next to the 48px activity bar, an 800x480 window had ~290px
    /// left for the editor *and* its tab bar, i.e. room for exactly one tab.
    /// That was the reported symptom — opening a second file moved the
    /// breadcrumb, content and status bar to it, but the tab strip kept
    /// reading only `sample.txt x`, because the second tab had nowhere to
    /// paint.
    ///
    /// # Why `15.0` on the GUI profile, and not lower
    ///
    /// `15.0` is [`ALT_SIDEBAR_WIDTH_MIN`], the floor `App::shell_config`
    /// *already* declares for the sidebar on every backend — so this makes
    /// the opening width agree with the bound sitting two lines from it,
    /// rather than inheriting an unrelated default. At ~345px it is in the
    /// same range as [`crate::core::session::Session::sidebar_width`]'s own
    /// persisted `260` default and VS Code's ~300px sidebar, and it leaves
    /// ~400px of an 800px window for the editor band — enough for several
    /// tabs (verified by the 800x480 driver test named below).
    ///
    /// It deliberately does **not** go below that floor, even though ~10
    /// (~230px) would be closer still to the persisted 260. The floor is
    /// shared with the Alt+Left/Right resize rung
    /// ([`alt_resized_sidebar_width`], #759), so an opening width beneath it
    /// makes the user's *first* Alt+Right jump discontinuously up to the
    /// floor with no way back: measured at `10.0`, the painted sidebar went
    /// 230px → 345px on Alt+Right and then stayed at 345px on Alt+Left
    /// (`gtk::testing::alt_rung::alt_right_widens_the_painted_sidebar_on_gtk`
    /// catches exactly this). Making that floor per-unit too is a change to
    /// the *shared* Alt rung's cross-backend contract and wants its own
    /// issue and its own two-backend coverage; it is not needed to fix the
    /// tab bar, so this field is the whole of #1798's production change and
    /// `min_sidebar_width`/`max_sidebar_width` stay shared and untouched.
    ///
    /// This is an `lh` multiple rather than a pixel value because
    /// `App::shell_config` runs before the runner's first font measurement
    /// (see the `#947`/`with_editor_font` comment at that call site), so it
    /// could not convert pixels into multiples even if quadraui exposed a
    /// `default_sidebar_width_px` to receive them — and an `lh` multiple is
    /// the better unit anyway: it tracks `:set font_size` for free.
    ///
    /// Behavioural coverage: `gtk::testing`'s
    /// `explorer_double_click_opens_second_file_in_a_second_tab_1798`, which
    /// runs at the reported 800x480 and fails if this is `20.0`.
    pub sidebar_width_lh: f32,
    /// Whether [`crate::app::App::shell_config`] should also request
    /// [`quadraui::ShellConfig::with_client_side_titlebar`].
    pub client_side_titlebar: bool,
    /// [`crate::icons::set_gui_backend`]'s argument.
    pub is_gui_backend: bool,
    /// The explorer tree's own per-row pixel/cell pitch, given the
    /// current line height — GTK: `quadraui::gtk::tree`'s own
    /// `item_height = (line_height * 1.4).round()` (no public accessor;
    /// duplicated here rather than re-derived from a `TreeController` hit
    /// test, since building the `TreeView` its layout needs is a private
    /// method — see `apply_explorer_drag_move`'s doc for the #1429
    /// consumer this feeds and the upstream gap this constant is a
    /// stand-in for). TUI: one whole cell (`|lh| lh`) — `quadraui::tui::
    /// tree`'s own doc states `TreeStyle::row_height` is "a GUI-pixel"
    /// concept only.
    pub explorer_row_h: fn(f64) -> f64,
    /// `(pad_x, pad_y)` the editor hover popup insets its content by,
    /// matching whichever rasteriser actually painted it:
    /// `quadraui::gtk::rich_text_popup`'s fixed 4px border/inset on GTK,
    /// `quadraui::tui::rich_text_popup`'s 2-cell-x/1-cell-y frame on TUI.
    /// `App::route_and_apply_editor_hover_popup` (press) and
    /// `handle_mouse_drag_msg`'s `HoverPopupSelection` arm (drag) both
    /// need the same value, or the two disagree about which content cell
    /// a given pixel/cell maps to (#1429: found hardcoded to the GTK pair
    /// in both places, which is why a TUI-native `App`, the `tui` harness
    /// arm, resolved a drag one row short of wherever the press itself
    /// had already been landing).
    pub hover_popup_pad: (f32, f32),
}

impl UnitProfile {
    /// GTK/macOS/Win: every value `App` used to hardcode as a `gtk_*`/
    /// `GTK_*` constant or function, unchanged — #1426's own acceptance bar
    /// is "no behaviour change on GTK/macOS/Win".
    pub fn px() -> Self {
        UnitProfile {
            tab_row_h: tab_row_height_px,
            tab_bar_h: tab_bar_height_px,
            breadcrumb_row_h: |_lh| BREADCRUMB_ROW_HEIGHT_PX,
            minimap: gtk_minimap_sizing(),
            picker: gtk_picker_sizing,
            picker_rows: gtk_picker_rows,
            tab_switcher: gtk_tab_switcher_sizing,
            find_replace_anchor: GTK_FIND_REPLACE_ANCHOR,
            divider_metrics: |_breadcrumbs| GTK_DIVIDER_METRICS,
            sidebar_system_metrics: gui_sidebar_system_metrics,
            hit_tolerance: (6.0, 6.0),
            activity_bar_width_px: Some(48.0),
            title_bar_lh: 2.0,
            // #1798: ~345px at the default editor font, down from the
            // inherited `20.0`'s ~460px — see the field doc for why this is
            // exactly `ALT_SIDEBAR_WIDTH_MIN` and not lower.
            sidebar_width_lh: ALT_SIDEBAR_WIDTH_MIN as f32,
            client_side_titlebar: true,
            is_gui_backend: true,
            explorer_row_h: |lh| (lh * 1.4).round(),
            hover_popup_pad: (4.0, 4.0),
        }
    }

    /// The `tui` harness arm: `App` painted through
    /// `quadraui::tui::TuiBackend`, whose `Backend::line_height`/
    /// `char_width` are always `1.0` (one terminal cell) — see that
    /// backend's own impl. Every value here is the cell-flavoured twin
    /// `px()` reads instead, matching the constant the shipped
    /// the pre-#1434 TUI shell already paints with (named in each field's own doc
    /// above).
    pub fn cell() -> Self {
        UnitProfile {
            tab_row_h: |lh| lh,
            tab_bar_h: |lh, breadcrumbs| if breadcrumbs { lh * 2.0 } else { lh },
            breadcrumb_row_h: |lh| lh,
            minimap: TUI_MINIMAP_SIZING,
            picker: |_lh| TUI_PICKER_SIZING,
            picker_rows: |_lh| TUI_PICKER_ROWS,
            tab_switcher: |_lh| TUI_TAB_SWITCHER_SIZING,
            find_replace_anchor: TUI_FIND_REPLACE_ANCHOR,
            divider_metrics: |breadcrumbs| DividerMetrics {
                group_vertical: (1.0, 1.0),
                group_horizontal: (0.0, if breadcrumbs { 2.0 } else { 1.0 }),
                window_vertical: (1.0, 1.0),
                window_horizontal: (1.0, 1.0),
                quantize: true,
            },
            sidebar_system_metrics: |_lh| quadraui::MsvLayoutMetrics {
                header_size: 1.0,
                divider_size: 0.0,
                scrollbar_size: 1.0,
                cell_quantum: 1.0,
            },
            hit_tolerance: (1.0, 1.0),
            activity_bar_width_px: None,
            title_bar_lh: 1.0,
            // #1798: unchanged from what the TUI already shipped — `20.0` is
            // `ShellConfig::new`'s own default, which `App::shell_config`
            // used to leave untouched on every backend. In *cells* 20 is the
            // right number; only the GUI profile was mis-scaled.
            sidebar_width_lh: 20.0,
            client_side_titlebar: false,
            is_gui_backend: false,
            explorer_row_h: |lh| lh,
            hover_popup_pad: (2.0, 1.0),
        }
    }
}

/// #1877: undo `AppShellLayout::main_content_bounds`'s shrink from
/// `App::shell_config`'s `with_command_line()`/`with_status_bar()`
/// reservation, recovering the full content height `render_content`'s own
/// `compute_editor_layout`/status-bar/command-line math was already
/// written against (that math independently — and correctly — subtracts
/// vimcode's own *dynamic* bottom-chrome height from whatever `h` it is
/// given; it must not also inherit a second, *static* subtraction the
/// shell performed first, or every bottom row ends up reserved twice,
/// leaving a blank gap between the command line and the window's true
/// bottom edge).
///
/// `shell_config`'s reservation exists purely so
/// `AppShellLayout::activity_bar_bounds` (and
/// `sidebar_header_bounds`/`sidebar_content_bounds`) stop short of the
/// window's bottom edge — see that call site's doc. Everything
/// `render_content` paints into `main_content_bounds` keeps using this
/// recovered height instead, so the one reservation shrinks only the
/// activity bar/sidebar's painted height, never where the editor, status
/// line or command line actually paint.
///
/// Only valid for an `AppShellLayout` produced by the one `ShellConfig`
/// that actually opted into both bands — `App::shell_config`, today the
/// sole legitimate producer. A layout built from a `ShellConfig` that
/// skipped `with_command_line()`/`with_status_bar()` just adds back
/// `0.0` for the missing band(s) (see the second half of this function's
/// own test), so a mismatched layout is silently off by nothing, not
/// silently off by two rows — there is nothing here for a mismatch to
/// get wrong.
///
/// Review round 1 (#1877): clamped to `window_bounds`'s own true bottom
/// edge, not just `main_content_bounds.height` plus both bands' heights
/// unconditionally. `AppShell::compute_layout` clamps `band_h` with
/// `.max(0.0)` *after* subtracting both rows from the viewport (quadraui
/// `app_shell.rs:1253`), but still *returns* full-height
/// `command_line_bounds`/`status_bar_bounds` rects even when the window is
/// too short to hold them — so on, say, a 3-row terminal,
/// `main_content_bounds.height` floors at `0.0` while the two bands still
/// report `lh.round()` each, and this function would hand back `2 *
/// lh.round()` for a window that cannot fit even one full row below the
/// title bar. That slipped past `render_content`'s `w < 1.0 || h < 1.0`
/// guard (previously the only thing standing between a too-short window
/// and a negative/degenerate paint) and tripped
/// `debug_assert_command_line_fits_viewport` instead — a debug-build panic
/// where the pre-#1877 code silently bailed. Clamping to the window's own
/// bottom edge restores that bail-out: a too-short window now reports a
/// true height of `0.0` (or less, if `main_content_bounds.y` already
/// exceeds the window), so the `h < 1.0` guard catches it exactly as
/// before.
pub fn main_content_true_height(layout: &quadraui::AppShellLayout) -> f32 {
    let reserved = layout.main_content_bounds.height
        + layout.command_line_bounds.map(|r| r.height).unwrap_or(0.0)
        + layout.status_bar_bounds.map(|r| r.height).unwrap_or(0.0);
    let window_bottom = layout.window_bounds.y + layout.window_bounds.height;
    reserved.min(window_bottom - layout.main_content_bounds.y)
}

/// #1877 review round 1 (blocking finding 1): the rect `shell_config`'s
/// bottom-chrome reservation reclaims from `activity_bar_bounds`/
/// `sidebar_header_bounds`/`sidebar_content_bounds`/`divider_bounds` but
/// that nothing else paints — the bottom-left strip, `two rows tall ×
/// (main_content_bounds.x - window_bounds.x)` wide, that used to be
/// covered by the activity bar/sidebar/divider chrome quadraui's own
/// `AppShell::render` paints (fully, edge to edge, before #1877) and now
/// isn't, because that chrome is painted *into* the now-shrunk bounds
/// only. Nothing downstream repaints the vacated strip: vimcode's own
/// status-bar/command-line rows are anchored at `main_content_bounds.x`
/// (`render_content`'s own `x`/`w` locals), not at the window's left
/// edge, so they never reach left of the activity bar/sidebar column
/// either. Left unpainted, a plain Cairo/CG/ratatui surface shows
/// whatever the frame clear left there — `theme.background` on every
/// backend — a visible notch against `theme.tab_bar_bg`'s activity-bar
/// color on any theme where the two differ (onedark: `#1a1a1a` vs
/// `#262633`).
///
/// Returns the reclaimed rect — `window_bounds.x` to `main_content_bounds
/// .x`, from the bottom of `activity_bar_bounds` (the shrunk edge every
/// one of the four reclaimed rects shares, by construction: `band_h` feeds
/// all four at once, quadraui `app_shell.rs:1296-1349`) to
/// `window_bounds`'s own true bottom edge — or `None` when there is
/// nothing to reclaim (no reservation active, so `activity_bar_bounds`
/// already runs flush to the window's bottom edge, or the window is too
/// short for either dimension to be positive). The caller fills this with
/// `theme.tab_bar_bg` — the same colour `AppShell::render` already uses
/// for `activity_bar_bounds` itself (quadraui `primitives/activity_bar.rs
/// :644-645`) and for `divider_bounds` (`theme.separator`, blended in by
/// eye at this width) — so the strip reads as a continuation of the
/// existing chrome instead of a second, differently-themed notch.
pub fn bottom_chrome_reservation_fill_rect(
    layout: &quadraui::AppShellLayout,
) -> Option<quadraui::Rect> {
    let ab = layout.activity_bar_bounds;
    let reclaimed_y = ab.y + ab.height;
    let window_bottom = layout.window_bounds.y + layout.window_bounds.height;
    let height = window_bottom - reclaimed_y;
    let width = layout.main_content_bounds.x - layout.window_bounds.x;
    if height < 1.0 || width < 1.0 {
        return None;
    }
    Some(quadraui::Rect::new(
        layout.window_bounds.x,
        reclaimed_y,
        width,
        height,
    ))
}

/// Compute the height of the bottom chrome (status bar + wildmenu) in pixels.
///
/// `show_global_status` is [`global_status_bar_visible`]'s value, **not**
/// `effective_window_status_line`'s — the always-present command line needs
/// only one row when no *global* bar occupies its own (either because each
/// window paints its own status, or `'laststatus'` hides the status line
/// entirely), and two when the global bar has its own row to sit in.
pub fn status_bar_height_px(line_height: f64, show_global_status: bool, has_wildmenu: bool) -> f64 {
    let wildmenu_px = if has_wildmenu { line_height } else { 0.0 };
    let global_rows = if show_global_status { 2.0 } else { 1.0 };
    line_height * global_rows + wildmenu_px
}

/// Compute the debug toolbar height in pixels (0 if hidden).
pub fn debug_toolbar_height_px(line_height: f64, visible: bool) -> f64 {
    if visible {
        line_height
    } else {
        0.0
    }
}

/// Compute the height of the separated status line row (0 if not active).
pub fn separated_status_height_px(line_height: f64, has_separated: bool) -> f64 {
    if has_separated {
        line_height
    } else {
        0.0
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // ─── Shared layout helper tests ─────────────────────────────────────────

    #[test]
    fn test_tab_bar_height_px() {
        let no_bc = tab_bar_height_px(20.0, false);
        let with_bc = tab_bar_height_px(20.0, true);
        assert_eq!(no_bc, TAB_ROW_HEIGHT_PX);
        assert_eq!(with_bc, TAB_ROW_HEIGHT_PX + BREADCRUMB_ROW_HEIGHT_PX);
    }

    /// #700: raising `line_height` (a stand-in for `settings.font_size`
    /// inflating editor text metrics) must NOT change the tab-bar or
    /// breadcrumb row height — VS Code's chrome stays fixed regardless of
    /// editor font size. RED-first check: reinstating the old
    /// `ceil(line_height * 1.6)` formula makes this fail because `40.0` and
    /// `20.0` no longer produce equal row heights.
    #[test]
    fn test_tab_bar_height_px_independent_of_font_size() {
        let small = tab_row_height_px(14.0);
        let large = tab_row_height_px(40.0);
        assert_eq!(small, large);
        assert_eq!(small, TAB_ROW_HEIGHT_PX);

        let small_bc = tab_bar_height_px(14.0, true);
        let large_bc = tab_bar_height_px(40.0, true);
        assert_eq!(small_bc, large_bc);
        assert_eq!(small_bc, TAB_ROW_HEIGHT_PX + BREADCRUMB_ROW_HEIGHT_PX);
    }

    /// #1877: `main_content_true_height` adds the shell's own
    /// `command_line_bounds`/`status_bar_bounds` heights back onto
    /// `main_content_bounds.height`, recovering the pre-reservation value
    /// — and is `0.0`-safe (treats either/both as absent) for callers that
    /// never opted into one or either band.
    #[test]
    fn main_content_true_height_adds_back_the_shells_bottom_chrome_reservation() {
        let mut layout = quadraui::AppShellLayout {
            window_bounds: quadraui::Rect::new(0.0, 0.0, 1400.0, 900.0),
            title_bar_bounds: None,
            activity_bar_bounds: quadraui::Rect::default(),
            sidebar_header_bounds: None,
            sidebar_content_bounds: None,
            divider_bounds: None,
            main_content_bounds: quadraui::Rect::new(0.0, 36.0, 1000.0, 828.0),
            bottom_panel_bounds: None,
            command_line_bounds: Some(quadraui::Rect::new(0.0, 864.0, 1000.0, 18.0)),
            status_bar_bounds: Some(quadraui::Rect::new(0.0, 882.0, 1000.0, 18.0)),
        };
        assert_eq!(main_content_true_height(&layout), 828.0 + 18.0 + 18.0);

        // Neither band reserved — the common, pre-#1877 shape — is a no-op.
        layout.command_line_bounds = None;
        layout.status_bar_bounds = None;
        assert_eq!(main_content_true_height(&layout), 828.0);
    }

    /// #1877 review round 1 (non-blocking finding 1): a window too short
    /// to hold the reservation must clamp to the window's own true bottom
    /// edge, not blindly add both bands' full (un-`.max(0.0)`-clamped-at-
    /// this-layer) heights back onto a `main_content_bounds.height` that
    /// `AppShell::compute_layout` already floored at `0.0`. Pre-fix this
    /// returned `2 * lh.round()` for a window with zero rows available
    /// below the title bar — enough to slip past `render_content`'s own
    /// `h < 1.0` bail-out and reach `debug_assert_command_line_fits_
    /// viewport` instead, a debug-build panic where the pre-#1877 code
    /// silently returned with nothing painted.
    #[test]
    fn main_content_true_height_clamps_to_the_window_when_too_short_to_reserve() {
        let layout = quadraui::AppShellLayout {
            window_bounds: quadraui::Rect::new(0.0, 0.0, 400.0, 40.0),
            title_bar_bounds: None,
            activity_bar_bounds: quadraui::Rect::default(),
            sidebar_header_bounds: None,
            sidebar_content_bounds: None,
            divider_bounds: None,
            // `AppShell::compute_layout` floored this at `0.0` already —
            // the window is too short to fit anything below the title bar.
            main_content_bounds: quadraui::Rect::new(0.0, 40.0, 400.0, 0.0),
            bottom_panel_bounds: None,
            // ...but it still *returns* two full-height bands.
            command_line_bounds: Some(quadraui::Rect::new(0.0, 40.0, 400.0, 18.0)),
            status_bar_bounds: Some(quadraui::Rect::new(0.0, 40.0, 400.0, 18.0)),
        };
        // Un-clamped this would be `0.0 + 18.0 + 18.0 = 36.0` — more rows
        // than the 0-tall window has left below `main_content_bounds.y`
        // (`window_bounds.y + window_bounds.height - main_content_bounds.y
        // == 40.0 - 40.0 == 0.0`). Clamped, it is `0.0`, which
        // `render_content`'s `h < 1.0` guard correctly bails out on.
        assert_eq!(main_content_true_height(&layout), 0.0);
    }

    /// #1877 review round 1 (blocking finding 1): the rect `shell_config`'s
    /// reservation vacates from `activity_bar_bounds` (and `sidebar_*_
    /// bounds`/`divider_bounds`) — nothing else in `render_content` paints
    /// it, so it must be reported precisely: from `activity_bar_bounds`'s
    /// (shrunk) bottom edge down to the window's true bottom edge, as wide
    /// as everything left of `main_content_bounds.x`.
    #[test]
    fn bottom_chrome_reservation_fill_rect_reports_the_vacated_strip() {
        let layout = quadraui::AppShellLayout {
            window_bounds: quadraui::Rect::new(0.0, 0.0, 1400.0, 900.0),
            title_bar_bounds: None,
            // Shrunk by the two-row (18.0px each) reservation, same as
            // `sidebar_header_bounds`/`sidebar_content_bounds`/
            // `divider_bounds` would be.
            activity_bar_bounds: quadraui::Rect::new(0.0, 36.0, 48.0, 828.0),
            sidebar_header_bounds: None,
            sidebar_content_bounds: None,
            divider_bounds: None,
            main_content_bounds: quadraui::Rect::new(48.0, 36.0, 1352.0, 828.0),
            bottom_panel_bounds: None,
            command_line_bounds: Some(quadraui::Rect::new(48.0, 864.0, 1352.0, 18.0)),
            status_bar_bounds: Some(quadraui::Rect::new(48.0, 882.0, 1352.0, 18.0)),
        };
        let rect = bottom_chrome_reservation_fill_rect(&layout)
            .expect("a reservation is active and the window has room for it");
        assert_eq!(rect, quadraui::Rect::new(0.0, 864.0, 48.0, 36.0));

        // No reservation active (`activity_bar_bounds` already flush with
        // the window's bottom edge) — nothing to reclaim.
        let mut no_reservation = layout.clone();
        no_reservation.activity_bar_bounds = quadraui::Rect::new(0.0, 36.0, 48.0, 864.0);
        no_reservation.command_line_bounds = None;
        no_reservation.status_bar_bounds = None;
        assert_eq!(bottom_chrome_reservation_fill_rect(&no_reservation), None);

        // No activity bar/sidebar column to reclaim either (`main_content_
        // bounds.x == window_bounds.x`) — zero-width, so `None`.
        let mut no_column = layout.clone();
        no_column.main_content_bounds.x = 0.0;
        assert_eq!(bottom_chrome_reservation_fill_rect(&no_column), None);
    }

    #[test]
    fn test_status_bar_height_px() {
        let lh = 16.0;
        // global bar not shown (per-window, or no status at all) → 1 row
        assert_eq!(status_bar_height_px(lh, false, false), lh);
        // global bar shown → 2 rows (bar + command line)
        assert_eq!(status_bar_height_px(lh, true, false), 2.0 * lh);
        // with wildmenu adds one line_height
        assert_eq!(status_bar_height_px(lh, false, true), 2.0 * lh);
    }

    #[test]
    fn test_compute_editor_layout_basic() {
        let engine = crate::core::engine::tests::engine_with_text("hello\nworld\n");
        let layout = compute_editor_layout(&engine, 800.0, 20.0, false);
        // per_window_status_line defaults to true → status bar = 1 cmd line (20px)
        assert_eq!(layout.status_bar_h, 20.0);
        assert!(layout.editor_bottom > 700.0);
        assert!(layout.editor_bottom < 800.0);
    }

    #[test]
    fn test_compute_editor_layout_tui_units() {
        let engine = crate::core::engine::tests::engine_with_text("hello\n");
        let layout = compute_editor_layout(&engine, 24.0, 1.0, true);
        // TUI: line_height=1.0, total=24 rows, menu not visible
        assert!(layout.editor_bottom > 20.0);
        assert!(layout.editor_bottom <= 24.0);
    }

    #[test]
    fn test_separated_status_height_px() {
        let lh = 18.0;
        assert_eq!(separated_status_height_px(lh, true), lh);
        assert_eq!(separated_status_height_px(lh, false), 0.0);
    }

    // ── #755: the shared editor hover popup rung ────────────────────────
    //
    // The behaviours below are what the two backends' bespoke copies
    // *disagreed* about. Black-box coverage lives in
    // `src/tui_main/shell_app.rs` and `src/gtk/testing.rs`; these pin the
    // arbitration itself, which neither driver can isolate.

    fn hover_state<'a>(
        links: &'a [(quadraui::Rect, String)],
        scrollbar: Option<PopupScrollbarHit>,
        has_focus: bool,
    ) -> EditorHoverPopupState<'a> {
        EditorHoverPopupState {
            popup: Some(quadraui::Rect::new(10.0, 10.0, 100.0, 40.0)),
            links,
            scrollbar,
            has_focus,
            content: PopupContentMetrics {
                pad_x: 2.0,
                pad_y: 1.0,
                col_width: 1.0,
                line_height: 1.0,
            },
        }
    }

    /// #504: abutting link rects must each be individually reachable. GTK's
    /// arm hit-tested with `<=` on the right and bottom edges, so rect *n*
    /// and rect *n+1* overlapped by a pixel on their shared boundary and
    /// `find()` could only ever return the earlier one. The shared router
    /// uses the half-open `[x, x+w)` convention, so a click on the second
    /// link's first column reaches the second link.
    #[test]
    fn hover_popup_abutting_link_rects_are_each_reachable() {
        let links = vec![
            (quadraui::Rect::new(12.0, 12.0, 8.0, 1.0), "http://a".into()),
            (quadraui::Rect::new(20.0, 12.0, 8.0, 1.0), "http://b".into()),
        ];
        let st = hover_state(&links, None, false);
        assert_eq!(
            route_editor_hover_popup_click(true, &st, 12.0, 12.0),
            EditorHoverPopupRoute::Link("http://a".into())
        );
        assert_eq!(
            route_editor_hover_popup_click(true, &st, 20.0, 12.0),
            EditorHoverPopupRoute::Link("http://b".into()),
            "the second of two abutting link rects must be reachable (#504)"
        );
    }

    /// A `command:` URI routes to `Command`, not `Link` — the apply pass
    /// navigates and dismisses rather than handing the string to the
    /// backend's clipboard/browser (#272, #491).
    #[test]
    fn hover_popup_command_uri_routes_to_command_not_link() {
        let links = vec![(
            quadraui::Rect::new(12.0, 12.0, 10.0, 1.0),
            "command:definition".to_string(),
        )];
        let st = hover_state(&links, None, true);
        assert_eq!(
            route_editor_hover_popup_click(true, &st, 14.0, 12.0),
            EditorHoverPopupRoute::Command("command:definition".into())
        );
    }

    /// The scrollbar is painted on top of the content, so it wins a point
    /// that a link rect also claims. TUI tested links first; GTK tested the
    /// scrollbar first. GTK's order is the one that matches the paint.
    #[test]
    fn hover_popup_scrollbar_outranks_a_link_rect_underneath_it() {
        let sb = PopupScrollbarHit {
            track: quadraui::Rect::new(105.0, 11.0, 1.0, 20.0),
            thumb: quadraui::Rect::new(105.0, 15.0, 1.0, 5.0),
            visible_rows: 10,
            total: 40,
        };
        let links = vec![(
            quadraui::Rect::new(100.0, 15.0, 9.0, 1.0),
            "http://under".to_string(),
        )];
        let st = hover_state(&links, Some(sb), true);
        let route = route_editor_hover_popup_click(true, &st, 105.0, 15.0);
        assert!(
            matches!(route, EditorHoverPopupRoute::Scrollbar(_)),
            "the painted scrollbar must outrank a link rect beneath it, got {route:?}"
        );
    }

    /// Grabbing the thumb preserves the cursor's offset within it; clicking
    /// the empty track seeks the thumb's top to the cursor. GTK hardcoded
    /// `0.0` for both, so a thumb grab teleported.
    #[test]
    fn hover_popup_thumb_grab_preserves_the_offset_and_track_click_does_not() {
        let sb = PopupScrollbarHit {
            track: quadraui::Rect::new(105.0, 11.0, 1.0, 20.0),
            thumb: quadraui::Rect::new(105.0, 15.0, 1.0, 5.0),
            visible_rows: 10,
            total: 40,
        };
        let st = hover_state(&[], Some(sb), true);
        let grab = |y: f64| match route_editor_hover_popup_click(true, &st, 105.0, y) {
            EditorHoverPopupRoute::Scrollbar(t) => match *t {
                quadraui::DragTarget::ScrollbarY { grab_offset, .. } => grab_offset,
                other => panic!("expected a ScrollbarY target, got {other:?}"),
            },
            other => panic!("expected the scrollbar arm, got {other:?}"),
        };
        assert_eq!(grab(19.0), 4.0, "thumb grab keeps the cursor on the thumb");
        assert_eq!(grab(29.0), 0.0, "a track click seeks the thumb's top");
    }

    /// An unfocused popup takes focus; a focused one starts a selection at
    /// the clicked content cell (the router works in viewport rows — the
    /// apply pass adds `scroll_top`).
    #[test]
    fn hover_popup_body_click_focuses_then_selects() {
        let unfocused = hover_state(&[], None, false);
        assert_eq!(
            route_editor_hover_popup_click(true, &unfocused, 20.0, 15.0),
            EditorHoverPopupRoute::Focus
        );
        let focused = hover_state(&[], None, true);
        assert_eq!(
            route_editor_hover_popup_click(true, &focused, 20.0, 15.0),
            EditorHoverPopupRoute::StartSelection {
                content_line: 4,
                content_col: 8,
            }
        );
    }

    /// A press outside a visible popup dismisses it but is *not* consumed,
    /// so the cursor still lands where the user aimed; with no popup at all
    /// the rung reports `None` and the ladder continues.
    #[test]
    fn hover_popup_outside_press_falls_through_and_invisible_is_a_no_op() {
        let st = hover_state(&[], None, true);
        assert_eq!(
            route_editor_hover_popup_click(true, &st, 200.0, 200.0),
            EditorHoverPopupRoute::DismissAndFallThrough
        );
        assert_eq!(
            route_editor_hover_popup_click(false, &st, 20.0, 15.0),
            EditorHoverPopupRoute::None
        );
    }

    #[test]
    fn test_scrollbar_click_to_scroll_top() {
        // Click at top → scroll 0
        assert_eq!(scrollbar_click_to_scroll_top(0.0, 100.0, 200, 50), 0);
        // Click at bottom → max scroll
        assert_eq!(scrollbar_click_to_scroll_top(100.0, 100.0, 200, 50), 150);
        // Click at 50% → half of max scroll
        assert_eq!(scrollbar_click_to_scroll_top(50.0, 100.0, 200, 50), 75);
        // No scrollbar needed
        assert_eq!(scrollbar_click_to_scroll_top(50.0, 100.0, 50, 50), 0);
        // Zero track
        assert_eq!(scrollbar_click_to_scroll_top(50.0, 0.0, 200, 50), 0);
    }

    #[test]
    fn test_display_col_to_buffer_col() {
        // Plain text
        assert_eq!(display_col_to_buffer_col("hello", 3, 4, 0), 3);
        // With tab
        assert_eq!(display_col_to_buffer_col("\thello", 0, 4, 0), 0);
        assert_eq!(display_col_to_buffer_col("\thello", 4, 4, 0), 1);
        assert_eq!(display_col_to_buffer_col("\thello", 5, 4, 0), 2);
        // Past end
        assert_eq!(display_col_to_buffer_col("hi", 10, 4, 0), 2);
        // With scroll_left
        assert_eq!(display_col_to_buffer_col("hello world", 0, 4, 6), 6);
    }

    #[test]
    fn test_is_tab_close_click() {
        assert!(!is_tab_close_click(0, 10, 2));
        assert!(!is_tab_close_click(7, 10, 2));
        assert!(is_tab_close_click(8, 10, 2));
        assert!(is_tab_close_click(9, 10, 2));
        // Edge case: tab too narrow for close button
        assert!(!is_tab_close_click(0, 2, 2));
    }

    #[test]
    fn test_matches_key_binding() {
        // Ctrl+B
        assert!(matches_key_binding(
            "<C-b>",
            true,
            false,
            false,
            Some('b'),
            false,
            false,
            false
        ));
        assert!(!matches_key_binding(
            "<C-b>",
            false,
            false,
            false,
            Some('b'),
            false,
            false,
            false
        ));
        // Ctrl+Shift+E
        assert!(matches_key_binding(
            "<C-S-e>",
            true,
            true,
            false,
            Some('e'),
            false,
            false,
            false
        ));
        // Tab
        assert!(matches_key_binding(
            "<C-Tab>", true, false, false, None, true, false, false
        ));
        // Alt+X
        assert!(matches_key_binding(
            "<A-x>",
            false,
            false,
            true,
            Some('x'),
            false,
            false,
            false
        ));
        // Case insensitive
        assert!(matches_key_binding(
            "<C-b>",
            true,
            false,
            false,
            Some('B'),
            false,
            false,
            false
        ));
        // Wrong modifier
        assert!(!matches_key_binding(
            "<C-S-e>",
            true,
            false,
            false,
            Some('e'),
            false,
            false,
            false
        ));
    }

    // ─── window_zone_hit_test / minimap click-drag routing (#1094 review) ──
    //
    // `window_zone_hit_test` is the shared classifier both `click.rs`'s
    // `pixel_to_click_target` (GTK) and `tui_main/mouse.rs` (TUI) route
    // through. #1094's own fix widened `RenderedWindow.rect` back out to
    // the pane's true right edge (so the scrollbar paints there, past the
    // strip) but initially left this function computing its own
    // `viewport_cols` straight from that wider `rect.width`, with no idea
    // the minimap strip eats into it. The two tests below pin the two
    // resulting drifts: a real horizontal-scrollbar click misclassified as
    // `TextArea`, and — worse — a text-selection drag whose pointer sweeps
    // over the strip's own pixels extending the selection underneath
    // painted minimap content instead of being treated as a miss.

    /// Minimal `RenderedWindow` fixture with every field controllable,
    /// mirroring `build_rendered_window`'s own `empty` closure (this
    /// struct has no `Default` impl) — used here instead of driving a full
    /// `Engine`/buffer through `build_screen_layout` so `max_col` and
    /// `text_viewport_cols` can be set to the exact values needed to land
    /// in the narrow gap the pre-fix and post-fix formulas disagree on.
    pub(crate) fn fixture_window(
        rect: WindowRect,
        gutter_char_width: usize,
        total_lines: usize,
        max_col: usize,
        text_viewport_cols: usize,
        minimap_reserved_w: f64,
    ) -> RenderedWindow {
        RenderedWindow {
            window_id: WindowId(0),
            rect,
            lines: vec![],
            visible_line_capacity: 1,
            cursor: None,
            extra_cursors: vec![],
            selection: None,
            extra_selections: vec![],
            yank_highlight: None,
            scroll_top: 0,
            scroll_left: 0,
            total_lines,
            gutter_char_width,
            text_viewport_cols,
            minimap_reserved_w,
            is_active: true,
            show_active_bg: false,
            has_git_diff: false,
            has_breakpoints: false,
            max_col,
            diagnostic_gutter: std::collections::HashMap::new(),
            code_action_lines: std::collections::HashSet::new(),
            bracket_match_positions: Vec::new(),
            active_indent_col: None,
            tabstop: 4,
            cursorline: false,
            status_line: None,
            plugin_view: None,
        }
    }

    /// RED against the pre-fix shape: `has_h_sb` was computed from a
    /// `viewport_cols` re-derived straight off `rw.rect.width` (with no
    /// minimap subtraction), so a line that overflows the real, narrower
    /// `text_viewport_cols` but not that inflated rect-width-only figure
    /// reported `has_h_sb == false` — a click on the horizontal scrollbar
    /// that's actually painted on screen fell through to `TextArea`
    /// instead of `HorizontalScrollbar`. Picks `max_col` to sit exactly in
    /// that disagreement gap: greater than `text_viewport_cols` (so the
    /// fixed formula, which reuses `rw.text_viewport_cols` directly, must
    /// report overflow) but not greater than `rect.width` itself (so the
    /// old buggy formula, which never subtracted the minimap's width at
    /// all, would have reported none).
    #[test]
    fn window_zone_hit_test_h_scrollbar_click_accounts_for_the_minimap_strip() {
        let rect = WindowRect::new(0.0, 0.0, 40.0, 10.0);
        let text_viewport_cols = 30; // rect.width(40) - minimap_reserved_w(10)
        let max_col = text_viewport_cols + 1; // overflows the real viewport...
        assert!(
            max_col <= rect.width as usize,
            "fixture invariant broken: max_col must still fall short of the \
             old rect-width-only formula, or this test isn't exercising the \
             regression at all"
        );
        let rw = fixture_window(rect, 0, 1, max_col, text_viewport_cols, 10.0);

        // Row just above the (absent) status bar, inside the horizontal
        // scrollbar's one-line band: `content_h(10) - line_height(1) = 9`.
        let zone = window_zone_hit_test(&rw, 5.0, 9.5, 1.0, 1.0);
        assert!(
            matches!(zone, WindowZone::HorizontalScrollbar { .. }),
            "a line overflowing the real (minimap-narrowed) text viewport \
             must resolve a click on its scrollbar row to \
             WindowZone::HorizontalScrollbar, not fall through to \
             TextArea; got {zone:?}"
        );
    }

    /// A point over the minimap strip's own column range must not resolve
    /// to `TextArea` — pre-#1094, `RenderedWindow.rect` was narrowed by the
    /// strip's width, so such a point fell outside the window entirely
    /// (`find_window_at` never even reached this function for it). Post-
    /// #1094, `rect` reaches the pane's true right edge instead, so
    /// `window_zone_hit_test` has to exclude the strip's columns
    /// explicitly or a text-selection drag whose pointer sweeps over the
    /// strip's pixels (`apply_tui_editor_text_drag`, and
    /// `pixel_to_click_target`'s `mutate_focus == false` continuation path)
    /// would extend the selection underneath the painted strip instead of
    /// being treated as a miss.
    #[test]
    fn window_zone_hit_test_excludes_the_minimap_strip_from_text_area() {
        let rect = WindowRect::new(0.0, 0.0, 40.0, 10.0);
        let rw = fixture_window(rect, 0, 1, 1, 30, 10.0);

        // Comfortably inside the reserved strip+gutter band (the
        // rightmost 10 columns of a 40-wide rect).
        let strip_x = 35.0;
        let zone = window_zone_hit_test(&rw, strip_x, 0.0, 1.0, 1.0);
        assert!(
            matches!(zone, WindowZone::Minimap),
            "a point over the minimap strip's own pixels must classify as \
             WindowZone::Minimap, not TextArea/Gutter/HorizontalScrollbar; \
             got {zone:?}"
        );

        // A point well inside the real text columns must still resolve to
        // TextArea — the exclusion must not eat real text.
        let zone = window_zone_hit_test(&rw, 2.0, 0.0, 1.0, 1.0);
        assert!(
            matches!(zone, WindowZone::TextArea { .. }),
            "a point in the real text columns must still resolve to \
             WindowZone::TextArea; got {zone:?}"
        );
    }

    /// vimcode#1696: `to_q_editor`'s `editor.rect` is `rw.rect` verbatim —
    /// the *pane's* full-width rect, by #1094's own design (the minimap
    /// strip and quadraui's own drawn v/h scrollbar both have to sit
    /// somewhere inside that wide rect, past the narrower text; see that
    /// function's own doc comment and `build_screen_layout_with_breadcrumb_
    /// row`'s "#1094" section above). `RenderedWindow.minimap_reserved_w`
    /// never reaches `editor.rect`, so nothing then re-narrows the viewport
    /// `quadraui::Editor::layout` computes `EditorLayout::text_bounds` from
    /// — the exact value both backends' rasterisers size their glyph-
    /// drawing budget off (Win-GUI's `visible_cols`-bounded
    /// `win::editor::paint_line_text`; GTK's Cairo clip set to
    /// `text_bounds.width`). This test pins that gap numerically rather
    /// than by reading the source, so it is a **concrete regression target**
    /// for the missing quadraui capability `docs/PENDING_QUADRAUI_ISSUES.md`'s
    /// matching #1696 entry asks for (a way to reserve trailing content
    /// width independently of where the v/h scrollbar anchors): once that
    /// lands and vimcode adopts it, `text_bounds`'s right edge below should
    /// land at `rect.width - minimap_reserved_w`, not `rect.width` itself.
    ///
    /// Deliberately **not** "fixed" by narrowing `to_q_editor`'s `rect`
    /// directly — doing so moves `EditorLayout::v_scrollbar_bounds`/
    /// `h_scrollbar_bounds` (both anchored to `viewport.x + viewport.width`)
    /// in to sit flush against the now-narrower text instead of past the
    /// strip at the pane's true right edge, which is the exact regression
    /// #1094's own fix (and `window_zone_hit_test_h_scrollbar_click_
    /// accounts_for_the_minimap_strip` above) was written to prevent. A
    /// correct fix needs `Editor`/`EditorPaintOptions` to let a caller
    /// reserve extra trailing width for content layout alone, leaving the
    /// scrollbar anchor at the full `viewport.width` untouched — which does
    /// not exist upstream today (confirmed by reading `layout_with_options`
    /// at the pinned rev: `text_w`/`visible_cols` and
    /// `v_scrollbar_bounds`/`h_scrollbar_bounds` are both derived from the
    /// one `viewport.width`, with no second parameter to decouple them).
    #[test]
    fn to_q_editor_does_not_narrow_the_viewport_for_the_minimap_strip_1696() {
        let rect = WindowRect::new(0.0, 0.0, 40.0, 10.0);
        let minimap_reserved_w = 10.0;
        // `total_lines <= visible_lines` (height 10 / line_height 1.0) keeps
        // `has_v_scrollbar` false, so `text_w` below reduces to plain
        // `viewport.width - gutter_w` and isolates exactly the minimap gap
        // this test is about — no scrollbar-width term to account for too.
        let rw = fixture_window(rect, 0, 1, 1, 30, minimap_reserved_w);

        let editor = to_q_editor(&rw);
        let layout = editor.layout(editor.rect, 1.0, 1.0);

        let minimap_left_edge = rect.width - minimap_reserved_w;
        let text_right_edge = (layout.text_bounds.x + layout.text_bounds.width) as f64;
        assert!(
            text_right_edge > minimap_left_edge,
            "expected today's (unfixed) `text_bounds` to reach past the \
             minimap strip's own left edge ({minimap_left_edge}) — got \
             text_bounds ending at {text_right_edge}. If this now fails, \
             `to_q_editor`/`Editor::layout` has started narrowing for the \
             minimap; update this test and the matching \
             `docs/PENDING_QUADRAUI_ISSUES.md` #1696 entry rather than \
             deleting either — see this test's own doc comment for why a \
             naive `to_q_editor` narrowing would itself be a regression of \
             #1094 and needs a new quadraui capability instead."
        );
    }

    /// #722 acceptance (narrowed by #989): below the point where
    /// `MINIMAP_TARGET_COLS_TUI` becomes affordable, the reserved width is
    /// still a proportion of the *pane's* width, so widening a genuinely
    /// narrow pane must still widen the strip. `char_width == 1.0`
    /// (TUI-shaped) keeps `resolve_width`'s column-normalisation a no-op, so
    /// both widths land `want = width * MINIMAP_WIDTH_FRACTION` strictly
    /// inside `[MINIMAP_MIN_COLS, MINIMAP_MAX_COLS]` (6 and 10.5, against a
    /// 6..30 band) and below `MINIMAP_TARGET_COLS_TUI` (12) — both widths
    /// stay in the pre-#989 proportional regime, unlike an *ordinary*
    /// terminal width (see `minimap_reserved_width_holds_steady_across_ordinary_pane_widths`
    /// below for the #989 fix itself, where the strip stops scaling).
    #[test]
    fn minimap_reserved_width_scales_with_a_narrow_pane_width() {
        let e = minimap_engine();
        let narrow = minimap_reserved_width(&e, 40.0, 1.0, TUI_MINIMAP_SIZING, 0.0);
        let wide = minimap_reserved_width(&e, 70.0, 1.0, TUI_MINIMAP_SIZING, 0.0);
        assert_eq!(narrow, 40.0 * MINIMAP_WIDTH_FRACTION);
        assert_eq!(wide, 70.0 * MINIMAP_WIDTH_FRACTION);
        assert!(
            wide > narrow * 1.5,
            "widening a pane still too narrow to afford the fixed target must \
             substantially widen the strip: \
             narrow(40)={narrow}, wide(70)={wide}"
        );
    }

    /// #989 fix: on ordinary terminal widths (80..200 cols) the TUI minimap
    /// must hold steady at `MINIMAP_TARGET_COLS_TUI`, not scale with the
    /// pane. RED against the pre-#989 shape (`TUI_MINIMAP_SIZING` reusing
    /// the pixel-flavoured `MINIMAP_TARGET_COLS` = 120, which can never bind
    /// in columns): at those same three widths this would have returned
    /// `15`, `22.5` and `30` (the `fraction`-driven, then max-clamped,
    /// values) respectively — three different widths instead of one.
    #[test]
    fn minimap_reserved_width_holds_steady_across_ordinary_pane_widths() {
        let e = minimap_engine();
        let at_100 = minimap_reserved_width(&e, 100.0, 1.0, TUI_MINIMAP_SIZING, 0.0);
        let at_150 = minimap_reserved_width(&e, 150.0, 1.0, TUI_MINIMAP_SIZING, 0.0);
        let at_200 = minimap_reserved_width(&e, 200.0, 1.0, TUI_MINIMAP_SIZING, 0.0);
        assert_eq!(
            (at_100, at_150, at_200),
            (
                MINIMAP_TARGET_COLS_TUI,
                MINIMAP_TARGET_COLS_TUI,
                MINIMAP_TARGET_COLS_TUI
            ),
            "the TUI minimap must hold a fixed width across ordinary \
             terminal widths, not scale with the pane: \
             100={at_100}, 150={at_150}, 200={at_200}"
        );
    }

    /// #1869 acceptance (updated from the pre-#1869
    /// `minimap_reserved_width_is_unchanged_by_font_size`, which pinned the
    /// *opposite* of VS Code's real behaviour — see `MINIMAP_WIDTH_FRACTION`'s
    /// doc comment for why that premise was wrong): below the 120-column
    /// cap, VS Code's own minimap width formula genuinely divides by the
    /// editor's real character width, so a wider editor font must narrow
    /// the strip at the same pane width, not leave it unchanged.
    ///
    /// RED against the pre-#1869 shape (`resolve_width` with a forced
    /// `char_width` of `1.0`): both font sizes would have resolved to the
    /// same `pane_width * MINIMAP_WIDTH_FRACTION`-derived number regardless
    /// of the `8.0`/`16.0` passed here.
    #[test]
    fn minimap_reserved_width_scales_with_char_width_per_vs_code_formula() {
        let e = minimap_engine();
        let pane_width = 900.0;
        let small_font = minimap_reserved_width(&e, pane_width, 8.0, gtk_minimap_sizing(), 0.0);
        let large_font = minimap_reserved_width(&e, pane_width, 16.0, gtk_minimap_sizing(), 0.0);
        // Hand-computed VS Code formula, independent of
        // `vs_code_minimap_width_px`'s own implementation:
        // floor((900 - 14 - 2) / (cw + 1)) + 8, capped at 120.
        // cw=8:  floor(884 / 9)  + 8 = 98 + 8 = 106
        // cw=16: floor(884 / 17) + 8 = 52 + 8 = 60
        assert_eq!(
            (small_font, large_font),
            (106.0, 60.0),
            "a wider editor font must narrow the minimap at a fixed pane \
             width, matching VS Code's own division by the character \
             width: 8px/char={small_font}, 16px/char={large_font}"
        );
        assert!(
            small_font < MINIMAP_TARGET_COLS && large_font < MINIMAP_TARGET_COLS,
            "test setup sanity: both results must be strictly below the \
             120-column cap, or this isn't exercising the formula at all \
             (it would be exercising the cap instead): \
             small={small_font}, large={large_font}"
        );
    }

    /// Direct unit coverage of [`vs_code_minimap_width_px`] itself (#1869),
    /// below the cap — the acceptance bullet "the minimap width equals the
    /// VS Code formula's result, which is less than 120 at a pane narrow
    /// enough to be under the cap". Hand-computed independently of the
    /// function under test: `floor((400 - 14 - 2) / (7.0 + 1.0)) + 8 =
    /// floor(384 / 8) + 8 = 48 + 8 = 56`.
    #[test]
    fn vs_code_minimap_width_px_matches_the_formula_below_the_cap() {
        let got = vs_code_minimap_width_px(400.0, 7.0);
        assert_eq!(got, 56.0);
        assert!(
            got < MINIMAP_TARGET_COLS,
            "this case must stay below the 120 cap, or it isn't testing \
             the below-cap branch: got {got}"
        );
    }

    /// [`vs_code_minimap_width_px`]'s true floor (#1869): once the pane is
    /// narrow enough that the division term itself would go negative, the
    /// formula clamps it to `0` before adding the gutter — so the result
    /// bottoms out at exactly [`MINIMAP_GUTTER_WIDTH_PX`] (8px), regardless
    /// of the editor's character width. (`minimap_reserved_width`'s own
    /// `MINIMAP_MIN_TEXT_COLS` suppression hides the strip entirely before
    /// a real pane ever reaches this narrow — this is `vs_code_minimap_
    /// width_px` in isolation, the formula's own floor, not the suppressed
    /// width a caller actually sees.)
    #[test]
    fn vs_code_minimap_width_px_floors_at_the_gutter_width_regardless_of_font() {
        let narrow_font = vs_code_minimap_width_px(10.0, 6.0);
        let wide_font = vs_code_minimap_width_px(10.0, 20.0);
        assert_eq!(
            (narrow_font, wide_font),
            (MINIMAP_GUTTER_WIDTH_PX, MINIMAP_GUTTER_WIDTH_PX),
            "a pane too narrow for the division term to go positive must \
             floor at the gutter width alone, independent of character \
             width: 6px/char={narrow_font}, 20px/char={wide_font}"
        );
    }

    /// #728/#1869 acceptance: an ordinary wide GTK/macOS/Win pane must cap
    /// at VS Code's 120-column minimap width — exactly, not approximately —
    /// across more than one editor font size, rather than scaling up with
    /// the pane the way the pre-#728 `rect_width * MINIMAP_WIDTH_FRACTION`
    /// formula did (caught at ~240px by
    /// `gtk::testing::minimap::minimap_strip_settles_at_vs_code_parity_width_on_a_wide_pane`,
    /// driven through the real paint path).
    #[test]
    fn minimap_reserved_width_caps_at_exactly_120_on_a_wide_pane_across_font_sizes() {
        let e = minimap_engine();
        let pane_width = 3000.0;
        let small_font = minimap_reserved_width(&e, pane_width, 8.0, gtk_minimap_sizing(), 0.0);
        let large_font = minimap_reserved_width(&e, pane_width, 16.0, gtk_minimap_sizing(), 0.0);
        assert_eq!(
            (small_font, large_font),
            (MINIMAP_TARGET_COLS, MINIMAP_TARGET_COLS),
            "a pane this wide must cap at exactly VS Code's 120-column \
             minimap width at every ordinary font size, not scale up with \
             the pane: 8px/char={small_font}, 16px/char={large_font}"
        );
    }

    /// #728 acceptance, still pinned at the exact scenario #728 originally
    /// caught (a 1600px pane at an 8px character width): under #1869's real
    /// formula this also happens to land exactly on the 120-column cap
    /// (`floor((1600 - 16) / 9) + 8 == 184`, `min(120, 184) == 120`), so
    /// the historical regression scenario stays covered without this test
    /// needing to change at all.
    #[test]
    fn minimap_reserved_width_matches_vs_code_parity_on_a_wide_pane() {
        let e = minimap_engine();
        let want = minimap_reserved_width(&e, 1600.0, 8.0, gtk_minimap_sizing(), 0.0);
        assert_eq!(
            want, MINIMAP_TARGET_COLS,
            "an ordinary wide GTK pane must settle at VS Code's ~120px \
             minimap width, not scale up with the pane: got {want}"
        );
    }

    // ── Command-line click/selection geometry (#816) ────────────────────

    /// #816 review: GTK's `MouseDown` handler used a hand-rolled guard that
    /// compared only `point.y` against the command line's rect, never
    /// `point.x`. Since `command_line_rect`'s `x`/`width` come from
    /// `main_content_bounds` (after the activity bar + sidebar), that guard
    /// treated ANY click in the bottom `line_height`-px band as "over the
    /// command line" — including clicks over the sidebar/activity bar
    /// (left of `rect.x`) or past the content area on the right (at/past
    /// `rect.x + rect.width`) — which silently disabled the undecorated
    /// window's only S/SW/SE resize grab. This asserts both axes are
    /// actually checked; it fails against the y-only version (a click at
    /// `x = rect.x - 1.0` would wrongly return `true`).
    #[test]
    fn point_over_command_line_checks_both_axes() {
        let rect = quadraui::Rect::new(100.0, 480.0, 600.0, 20.0);

        // Inside the row, inside the x-span: over the command line.
        assert!(point_over_command_line(
            rect,
            quadraui::Point::new(150.0, 485.0)
        ));

        // Inside the row's y-band, but LEFT of the command line's x-span
        // (over the sidebar/activity bar) — must NOT count as "over the
        // command line", or the window's bottom-left resize corner is dead.
        assert!(!point_over_command_line(
            rect,
            quadraui::Point::new(50.0, 485.0)
        ));

        // Inside the row's y-band, but RIGHT of the command line's x-span
        // (past the content area) — same requirement for the bottom-right
        // corner / south edge past the content area.
        assert!(!point_over_command_line(
            rect,
            quadraui::Point::new(750.0, 485.0)
        ));

        // Outside the y-band entirely (well above the command line): never
        // "over the command line" regardless of x.
        assert!(!point_over_command_line(
            rect,
            quadraui::Point::new(150.0, 10.0)
        ));

        // A zero-width rect (command line not yet painted) never counts.
        let empty = quadraui::Rect::new(0.0, 0.0, 0.0, 0.0);
        assert!(!point_over_command_line(
            empty,
            quadraui::Point::new(0.0, 0.0)
        ));
    }

    // ── divider_to_split (#582 follow-up) ───────────────────────────────────

    #[test]
    fn divider_to_split_vertical_maps_direction_ratio_and_bounds() {
        // A `:vsplit` (side-by-side panes) divider at x=40 within a 0..100
        // wide, 5..25 tall node.
        let div = WindowDivider {
            group_id: GroupId(0),
            split_index: 0,
            direction: SplitDirection::Vertical,
            position: 40.0,
            axis_start: 0.0,
            axis_size: 100.0,
            cross_start: 5.0,
            cross_size: 20.0,
        };
        let (split, rect) = divider_to_split(&div, quadraui::WidgetId::new("wdiv:0:0"));
        // vimcode's `Vertical` (side-by-side) is quadraui's `Horizontal`.
        assert_eq!(split.direction, quadraui::SplitDirection::Horizontal);
        assert!(
            (split.ratio - 0.4).abs() < 0.0001,
            "ratio = {}",
            split.ratio
        );
        // `rect` reconstructs the original node bounds exactly.
        assert_eq!(rect, quadraui::Rect::new(0.0, 5.0, 100.0, 20.0));
    }

    #[test]
    fn divider_to_split_horizontal_maps_direction_ratio_and_bounds() {
        // A `:split` (stacked panes) divider at y=30 within a 10..30 wide,
        // 0..50 tall node.
        let div = WindowDivider {
            group_id: GroupId(0),
            split_index: 0,
            direction: SplitDirection::Horizontal,
            position: 30.0,
            axis_start: 0.0,
            axis_size: 50.0,
            cross_start: 10.0,
            cross_size: 20.0,
        };
        let (split, rect) = divider_to_split(&div, quadraui::WidgetId::new("wdiv:0:0"));
        // vimcode's `Horizontal` (stacked) is quadraui's `Vertical`.
        assert_eq!(split.direction, quadraui::SplitDirection::Vertical);
        assert!(
            (split.ratio - 0.6).abs() < 0.0001,
            "ratio = {}",
            split.ratio
        );
        assert_eq!(rect, quadraui::Rect::new(10.0, 0.0, 20.0, 50.0));
    }

    /// #582 iteration-2 regression: GTK's `:vsplit` divider was unhittable
    /// because the hit-test rebuilt its bounds at origin `(0, 0)` while
    /// `render_content` painted from `main_content_bounds` — origin offset
    /// right by the activity bar/sidebar. The press then missed and fell
    /// through to text-selection.
    ///
    /// Pins the invariant the GTK fix relies on: a divider's hit band tracks
    /// its `axis_start`, so an origin-shifted (`0.0`) recomputation of the
    /// same split is NOT interchangeable with the painted one. If this ever
    /// passes at both positions, the two frames have been conflated again.
    #[test]
    fn divider_hit_test_follows_axis_start_not_screen_origin() {
        // A `:vsplit` inside a group whose content starts at x=300 (activity
        // bar + sidebar) — painted at 300 + 600*0.5 = 600.
        let div = WindowDivider {
            group_id: GroupId(0),
            split_index: 0,
            direction: SplitDirection::Vertical,
            position: 600.0,
            axis_start: 300.0,
            axis_size: 600.0,
            cross_start: 40.0,
            cross_size: 500.0,
        };
        let dividers = [div];

        // A click on the painted line hits.
        assert_eq!(
            divider_hit_test(&dividers, 600.0, 300.0, (6.0, 6.0), (6.0, 6.0), false),
            Some(0),
        );
        // The position an origin-at-zero recomputation would have painted
        // (0 + 600*0.5 = 300) is NOT the divider — this is the exact
        // displacement that made #582's `:vsplit` fall through.
        assert_eq!(
            divider_hit_test(&dividers, 300.0, 300.0, (6.0, 6.0), (6.0, 6.0), false),
            None,
        );

        // The drag ratio is likewise anchored to `axis_start`: dragging to
        // x=450 is ~25% across the group (150 of the 599-column content
        // width — `axis_size` minus #1326's one reserved divider column,
        // see `divider_ratio_from_pos`'s doc), not 75% (which is what
        // 450/600 would give if the origin were dropped).
        let r = divider_ratio_from_pos(&dividers[0], 450.0, 300.0);
        assert!((r - 150.0 / 599.0).abs() < 0.0001, "ratio = {r}");
    }

    /// Companion to the above for the paint side: `divider_to_split` must
    /// reconstruct the divider's *absolute* node bounds, so `Backend::
    /// draw_split` lands the line on the same pixels `divider_hit_test`
    /// accepts. An origin-relative rect here would paint the line away from
    /// its own hit band.
    #[test]
    fn divider_to_split_preserves_absolute_origin() {
        let div = WindowDivider {
            group_id: GroupId(0),
            split_index: 0,
            direction: SplitDirection::Vertical,
            position: 600.0,
            axis_start: 300.0,
            axis_size: 600.0,
            cross_start: 40.0,
            cross_size: 500.0,
        };
        let (split, rect) = divider_to_split(&div, quadraui::WidgetId::new("wdiv:0:0"));
        assert!(
            (split.ratio - 0.5).abs() < 0.0001,
            "ratio = {}",
            split.ratio
        );
        assert_eq!(rect, quadraui::Rect::new(300.0, 40.0, 600.0, 500.0));
    }

    /// #1586 (the "unit test on group geometry" acceptance bullet): for a
    /// stacked (`Ctrl+W s`) group split, no *painted* divider rect may
    /// intersect either group's own tab-row band.
    ///
    /// `GroupLayout::calculate_group_rects`/`dividers` are unchanged by
    /// #1586 (`GroupDivider::position` for a stacked split still coincides
    /// exactly with the lower group's tab-row top — see
    /// [`painted_group_dividers`]'s doc for why that geometry is fine to
    /// leave alone) — so this pins the *paint-time* invariant instead: after
    /// [`painted_group_dividers`] filters `GroupLayout::dividers`'s raw
    /// output, nothing left in the result can ever land inside a tab-row
    /// band, because a stacked entry never survives the filter at all.
    ///
    /// Asserts both halves, so this cannot pass vacuously: (1) the *raw*
    /// divider genuinely does intersect the lower group's tab-row band —
    /// proving the filter is doing real work, not filtering nothing — and
    /// (2) the *painted* (filtered) set is empty, so no rect from it can
    /// intersect anything.
    #[test]
    fn painted_group_dividers_excludes_stacked_entries_that_intersect_tab_rows_1586() {
        use crate::core::window::{GroupId, GroupLayout, WindowRect};

        let mut layout = GroupLayout::leaf(GroupId(0));
        layout.split_at(GroupId(0), SplitDirection::Horizontal, GroupId(1), false);
        let bounds = WindowRect::new(0.0, 0.0, 80.0, 24.0);
        let tab_bar_height = 1.0;

        let rects = layout.calculate_group_rects(bounds, tab_bar_height);
        let dividers = layout.dividers(bounds, &mut 0);
        assert_eq!(dividers.len(), 1);
        let raw = &dividers[0];
        assert_eq!(raw.direction, SplitDirection::Horizontal);

        // (1) The raw geometry really does collide with the lower group's
        // tab row — `GroupTabBar::bounds`'s own doc says that band is
        // `[rect.y - tab_bar_height, rect.y)`.
        let lower_tab_row = rects
            .iter()
            .map(|(_, r)| (r.y - tab_bar_height, r.y))
            .find(|&(top, _)| (raw.position - top).abs() < 0.001)
            .expect("the raw divider's position must coincide with some group's tab-row top");
        assert!(
            raw.position >= lower_tab_row.0 && raw.position < lower_tab_row.1,
            "raw divider at {} must fall inside tab row {lower_tab_row:?} — \
             otherwise there'd be nothing for `painted_group_dividers` to filter",
            raw.position
        );

        // (2) After filtering, nothing paints there at all.
        let painted = painted_group_dividers(&dividers);
        assert!(
            painted.is_empty(),
            "a stacked split must paint no group-divider line at all (#1586); got {painted:?}"
        );
    }
}
