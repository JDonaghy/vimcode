use super::*;

// ─── EditorGroupSplitData ─────────────────────────────────────────────────────

/// Diff toolbar data shown in the tab bar when a diff view is active.
#[derive(Debug, Clone)]
pub struct DiffToolbarData {
    /// Label like "2 of 5", or `None` if cursor is not near a change.
    pub change_label: Option<String>,
    /// Total number of change regions.
    pub total_changes: usize,
    /// Whether unchanged sections are currently hidden (folded).
    pub unchanged_hidden: bool,
}

/// Tab bar + bounds for one editor group.
#[derive(Debug, Clone)]
pub struct GroupTabBar {
    pub group_id: GroupId,
    pub tabs: Vec<TabInfo>,
    /// Content area of this group (tab bar drawn at top edge).
    pub bounds: WindowRect,
    /// Diff toolbar data, present when the group is showing a diff view.
    pub diff_toolbar: Option<DiffToolbarData>,
    /// Index of the first visible tab (scroll offset for overflow tab bars).
    pub tab_scroll_offset: usize,
    /// The resolved `quadraui::TabBarLayout` this tab bar painted with, in
    /// char-cell units relative to the tab bar's left edge (column 0 = left
    /// edge of group bounds). Backends hit-test clicks against this directly
    /// via [`resolve_tab_bar_click`] instead of re-deriving geometry (#822).
    pub hit_regions: quadraui::TabBarLayout,
    /// Pre-built quadraui `TabBar` primitive — backends draw this directly.
    pub bar: quadraui::TabBar,
    /// Per-tab icon sidecar parallel to `bar.tabs` (#703), from
    /// [`build_tab_bar_icons`]. Empty when Nerd Fonts are off. Backends must
    /// pass this to **both** `Backend::draw_tab_bar_icons` and
    /// `Backend::tab_bar_layout_icons`.
    pub icons: Vec<Option<quadraui::TabIcon>>,
}

// ── Tab bar hit region constants (char-cell units) ──────────────────────────

/// Columns used by each tab's close button (the × itself + trailing space).
pub const TAB_CLOSE_COLS: u16 = 2;
/// Cell width of one tab in the tab bar: the label plus [`TAB_CLOSE_COLS`]
/// for the close glyph and its trailing separator.
///
/// # Why this counts display columns and not `char`s (#654, quadraui#554)
///
/// `.chars().count()` is *not* a terminal display width — a CJK ideograph or
/// a wide emoji occupies two columns. The width used here has to agree with
/// the width the **rasteriser** paints with, so for as long as quadraui's TUI
/// tab bar measured *and* painted per-`char` this function had to as well,
/// and #654 documented that at length rather than "fixing" it into a
/// mismatch.
///
/// quadraui#554 (`77a5142`, in the pin bumped by #659) fixed both quadraui
/// sides together: `TuiBackend::draw_tab_bar` / `tab_bar_layout` now measure
/// with `display_width`, and `quadraui::tui::draw_tab_bar` strides the
/// label-paint loop by [`quadraui::tui::char_cell_width`] instead of a flat
/// `x += 1`. That commit names this function as its downstream follow-up,
/// and #654's own note said this would be the single edit needed on the
/// vimcode side — #654 had already routed the tooltip, both context-menu
/// hit-tests, the click router and the drag-slot map through
/// [`compute_tab_bar_layout`], so nothing else measures a tab.
///
/// A tab named `"日本語.rs"` (9 chars, 12 columns) is now both measured
/// and painted 12 cells wide, so the next tab starts at cell 12 and every hit
/// box lands on the glyph it covers. Reverting this to `.chars().count()`
/// against a post-#554 quadraui shifts every hit box *left* of what is drawn
/// — the mirror image of the pre-#554 hazard, and caught by
/// `tui_main::render_impl::tests::
/// tab_hit_regions_match_painted_columns_for_wide_names`, which reads the
/// expected columns out of the rendered buffer.
///
/// `icon_cols` is the per-tab icon reservation from
/// [`quadraui::tab_icon_cols`] (#703) — glyph width + a 1-column gap, or `0`
/// for an undecorated tab. It has to be folded in here for the same reason
/// the trailing space on [`TabInfo::name`] does: `TuiBackend::draw_tab_bar_icons`
/// adds `tab_icon_cols` to its own per-tab width budget, so a hit box measured
/// without it sits one glyph left of the painted tab — the #654 desync again,
/// this time surfacing as "the close × closes the tab to its right".
fn tab_hit_width(t: &TabInfo, icon_cols: u16) -> usize {
    quadraui::tui::display_width(&t.name) + TAB_CLOSE_COLS as usize + icon_cols as usize
}

/// Build the per-tab icon sidecar that decorates a tab bar with VS Code's
/// coloured language badge (#703).
///
/// The result is a slice **parallel to `tabs`** (and therefore to the
/// `quadraui::TabBar::tabs` that [`build_tab_bar_primitive`] derives from the
/// same slice): entry `i` decorates tab `i`. Pass it to
/// `Backend::draw_tab_bar_icons` *and* `Backend::tab_bar_layout_icons` — a
/// caller that paints with icons but measures without them reports slot and
/// close-button bounds shifted left of the painted glyphs.
///
/// Returns an **empty** vec when Nerd Fonts are off. `&[]` is quadraui's
/// "no icons at all" argument and makes `draw_tab_bar_icons` byte-identical to
/// `draw_tab_bar`, so the no-Nerd-Font geometry is exactly what it was before
/// this feature existed. Deliberately *not* the ASCII fallbacks
/// [`icons::file_icon`] would otherwise return: a bare `R`/`P`/`#` before every
/// tab label is noise, not parity.
pub fn build_tab_bar_icons(tabs: &[TabInfo]) -> Vec<Option<quadraui::TabIcon>> {
    if !icons::nerd_fonts_enabled() {
        return Vec::new();
    }
    tabs.iter()
        .map(|t| {
            // `TabInfo::name` carries a deliberate trailing space (see its
            // doc); the filename lookup has to read the trimmed name so a
            // filename-badged file (`Dockerfile`, `.gitignore`, #992) can
            // still match exactly, not just its (often absent) extension.
            let name = tab_name_filename(&t.name);
            Some(quadraui::TabIcon {
                glyph: icons::file_icon_for_name(name).to_string(),
                color: tab_icon_color(name),
            })
        })
        .collect()
}

/// Extract the trimmed base filename from a [`TabInfo::name`] label, for
/// [`icons::file_icon_for_name`]/[`icons::file_icon_color_for_name`].
/// Scratch/special buffers (`"[Keymaps]"`, `"[No Name]"`) pass through
/// unmatched by the filename table and fall back to the (empty) extension
/// lookup, which [`icons::file_icon`] maps to the generic file glyph —
/// matching VS Code, which badges untitled editors with a plain file icon
/// rather than nothing.
fn tab_name_filename(name: &str) -> &str {
    name.trim()
}

/// The language identity colour for a tab icon. Thin adapter over
/// [`icons::file_icon_color_for_name`] so no rendering call site names an
/// RGB literal (see [`icons::file_icon_color`]'s design note for why this is
/// not a `Theme` token).
pub(crate) fn tab_icon_color(name: &str) -> Color {
    let (r, g, b) = icons::file_icon_color_for_name(name);
    Color::from_rgb(r, g, b)
}

/// Compute a group's tab bar layout.
///
/// Layout (left to right):
/// `[tab0][tab1]...[tabN]  [diff_toolbar?] [split_btns?] [action_btn]`
///
/// All positions are in char-cell columns relative to the tab bar left edge.
///
/// Per D6: layout math lives in `quadraui::TabBar::layout()`. This
/// function builds the TabBar primitive and asks it for a layout — the
/// same `quadraui::TabBarLayout` both backends cache on `GroupTabBar`
/// and hit-test directly via [`resolve_tab_bar_click`] /
/// `quadraui::TabBarLayout::hit_test`. (#822: this used to downconvert
/// into a vimcode-local `(TabBarHitRegion, TabBarClickTarget)` shape as
/// a bridge for backends that hadn't migrated to `TabBarLayout` yet —
/// both had, so the downconversion was pure duplication.)
///
/// `icons_sidecar` is the sidecar from [`build_tab_bar_icons`] — the *same*
/// slice the backend paints with. It must be threaded through here because an
/// icon widens its tab by `tab_icon_cols`; measuring with `&[]` while painting
/// with icons is the measure/paint desync #654 exists to prevent. (Named with
/// the suffix because bare `icons` would shadow the [`crate::icons`] module
/// this file uses throughout.)
pub fn compute_tab_bar_layout(
    tabs: &[TabInfo],
    icons_sidecar: &[Option<quadraui::TabIcon>],
    tab_scroll_offset: usize,
    bar_width: u16,
    has_diff_toolbar: bool,
    diff_label_cols: u16,
    has_split_buttons: bool,
) -> quadraui::TabBarLayout {
    // Synthesise a DiffToolbarData shaped to match diff_label_cols so
    // build_tab_bar_primitive emits the right segments. The primitive's
    // diff segments are fixed 3-cell widths each, so we just need a
    // label whose .chars().count() + 1 (for the leading space) equals
    // diff_label_cols.
    let synth_diff = if has_diff_toolbar {
        let label = if diff_label_cols > 1 {
            // Space padding so the resulting segment width matches.
            Some(" ".repeat((diff_label_cols - 1) as usize))
        } else {
            None
        };
        Some(DiffToolbarData {
            change_label: label,
            total_changes: 1,
            unchanged_hidden: false,
        })
    } else {
        None
    };

    let primitive = build_tab_bar_primitive(
        tabs,
        has_split_buttons,
        synth_diff.as_ref(),
        tab_scroll_offset,
        None,
    );

    // Per-tab width: see `tab_hit_width` — the single place tab geometry is
    // measured now that #654 routed every TUI hit-test through these regions.
    // Close hit region is the trailing 2 cells (matches legacy behaviour:
    // clicks on × or the trailing separator count as close).
    let tab_widths: Vec<usize> = tabs
        .iter()
        .enumerate()
        .map(|(i, t)| tab_hit_width(t, quadraui::tab_icon_cols(icons_sidecar, i)))
        .collect();

    primitive.layout(
        bar_width as f32,
        1.0,
        0.0, // scroll arrows disabled — matches existing TUI behaviour
        |i| quadraui::TabMeasure::new(tab_widths[i] as f32, TAB_CLOSE_COLS as f32),
        |i| {
            // TabBarSegment.width_cells is pre-computed by build_tab_bar_primitive
            // in legacy char-cell units, which is exactly what we want here.
            quadraui::SegmentMeasure::new(primitive.right_segments[i].width_cells as f32)
        },
    )
}

/// Map a `quadraui::TabBarHit` to the engine's `TabBarClickTarget`.
///
/// `RightSegment` ids are the ones `build_tab_bar_primitive` assigns to its
/// split/diff/action-menu segments; scroll arrows and `Empty` have no
/// engine-level click meaning.
fn tab_bar_hit_target(hit: quadraui::TabBarHit) -> Option<crate::core::engine::TabBarClickTarget> {
    use crate::core::engine::TabBarClickTarget;
    match hit {
        quadraui::TabBarHit::Tab(i) => Some(TabBarClickTarget::Tab(i)),
        quadraui::TabBarHit::TabClose(i) => Some(TabBarClickTarget::CloseTab(i)),
        quadraui::TabBarHit::RightSegment(id) => match id.as_str() {
            "tab:split_right" => Some(TabBarClickTarget::SplitRight),
            "tab:split_down" => Some(TabBarClickTarget::SplitDown),
            "tab:diff_prev" => Some(TabBarClickTarget::DiffPrev),
            "tab:diff_next" => Some(TabBarClickTarget::DiffNext),
            "tab:diff_toggle" => Some(TabBarClickTarget::DiffToggle),
            "tab:action_menu" => Some(TabBarClickTarget::ActionMenu),
            _ => None,
        },
        quadraui::TabBarHit::ScrollLeft
        | quadraui::TabBarHit::ScrollRight
        | quadraui::TabBarHit::Empty => None,
    }
}

/// Resolve an x position (relative to the tab bar's left edge, in the
/// layout's own unit — char cells for the TUI char-cell layout, pixels for a
/// pixel-accurate GTK paint) to a `TabBarClickTarget` by hit-testing the
/// `TabBarLayout` paint already produced — per `feedback_cache_paint_layout`,
/// this reads what paint produced instead of re-deriving geometry in the
/// click handler. Row is fixed at `0.0`: every hit region in a `TabBarLayout`
/// spans the bar's single row, so an x-only probe lands inside every
/// region's `y` range regardless of the layout's `bar_height`.
///
/// One function for both the TUI char-cell path (`x` is a whole column,
/// e.g. `col as f32`) and GTK's pixel-accurate path (#1491 migrated GTK's
/// own tab-bar click resolution off the deprecated `TabBarHits` onto this
/// same call, so both backends now share it rather than GTK hand-rolling a
/// parallel pixel hit-test).
pub fn resolve_tab_bar_click(
    layout: &quadraui::TabBarLayout,
    x: f32,
) -> Option<crate::core::engine::TabBarClickTarget> {
    tab_bar_hit_target(layout.hit_test(x, 0.0))
}

/// An empty `TabBarLayout` — no tabs, no hit regions — for contexts where no
/// tab bar was built (e.g. `ScreenLayout::tab_bar_hit_regions` in multi-group
/// mode, where each group carries its own layout instead).
pub(crate) fn empty_tab_bar_layout() -> quadraui::TabBarLayout {
    quadraui::TabBarLayout {
        bar_width: 0.0,
        bar_height: 0.0,
        visible_tabs: Vec::new(),
        visible_segments: Vec::new(),
        scroll_left: None,
        scroll_right: None,
        hit_regions: Vec::new(),
        resolved_scroll_offset: 0,
    }
}

/// One segment in the breadcrumb bar (either a path component or a symbol).
#[derive(Debug, Clone)]
pub struct BreadcrumbSegment {
    pub label: String,
    pub is_last: bool,
    pub is_symbol: bool,
    /// Index of this segment (0-based) — used by click handlers to identify which segment was clicked.
    pub index: usize,
    /// Accumulated path up to this segment (for path segments only).
    /// E.g. for `src > engine > mod.rs`, segment "engine" has path "src/engine".
    pub path_prefix: Option<std::path::PathBuf>,
    /// For symbol segments: the line number (0-indexed) where the symbol is defined.
    pub symbol_line: Option<usize>,
}

/// Breadcrumb bar data for one editor group.
#[derive(Debug)]
pub struct BreadcrumbBar {
    pub group_id: GroupId,
    pub segments: Vec<BreadcrumbSegment>,
    pub bounds: WindowRect,
    /// Pre-built quadraui `StatusBar` primitive — backends draw this directly.
    pub bar: quadraui::StatusBar,
    /// Cached layout from `Backend::draw_status_bar` — set at draw time,
    /// read by `resolve_breadcrumb_click` at click time.
    pub draw_layout: std::cell::RefCell<Option<quadraui::StatusBarLayout>>,
}

/// Convert a slice of `BreadcrumbSegment` plus the focus state into a
/// `quadraui::StatusBar` whose left segments alternate clickable
/// labels with non-clickable `" › "` separators. The leading 1-cell
/// pad matches the legacy renderer.
///
/// Each clickable label segment carries `action_id = "bc:N"` where N
/// is the engine-side segment index — paired with
/// [`breadcrumb_action_index`] for click resolution. The last segment
/// uses `breadcrumb_active_fg`; other segments use `breadcrumb_fg`.
/// When `focus_active && i == focus_selected`, the focused segment
/// inverts (bg = `breadcrumb_active_fg`, fg = `breadcrumb_bg`) — same
/// visual as the legacy renderer.
pub fn breadcrumbs_to_quadraui_status_bar(
    segments: &[BreadcrumbSegment],
    theme: &Theme,
    focus_active: bool,
    focus_selected: usize,
) -> quadraui::StatusBar {
    let bg = theme.breadcrumb_bg;
    let normal_fg = theme.breadcrumb_fg;
    let active_fg = theme.breadcrumb_active_fg;

    let mut left: Vec<quadraui::StatusBarSegment> = Vec::new();

    // 1-cell leading pad so the first label doesn't touch the left edge.
    left.push(quadraui::StatusBarSegment {
        text: " ".to_string(),
        fg: normal_fg,
        bg,
        bold: false,
        action_id: None,
    });

    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            left.push(quadraui::StatusBarSegment {
                text: " \u{203A} ".to_string(),
                fg: normal_fg,
                bg,
                bold: false,
                action_id: None,
            });
        }
        let is_focused = focus_active && i == focus_selected;
        let (fg, seg_bg) = if is_focused {
            (bg, active_fg)
        } else if seg.is_last {
            (active_fg, bg)
        } else {
            (normal_fg, bg)
        };
        left.push(quadraui::StatusBarSegment {
            text: seg.label.clone(),
            fg,
            bg: seg_bg,
            bold: false,
            action_id: Some(quadraui::WidgetId::new(format!("bc:{i}"))),
        });
    }

    quadraui::StatusBar {
        id: quadraui::WidgetId::new("breadcrumbs"),
        left_segments: left,
        right_segments: Vec::new(),
    }
}

/// Resolve a `WidgetId` produced by `breadcrumbs_to_quadraui_status_bar`
/// back to a `BreadcrumbSegment` index. Returns `None` if the id
/// doesn't match the `bc:N` pattern.
pub fn breadcrumb_action_index(id: &quadraui::WidgetId) -> Option<usize> {
    id.as_str().strip_prefix("bc:")?.parse().ok()
}

/// Result of resolving a breadcrumb click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreadcrumbClickResult {
    /// A clickable segment was hit — carries the group whose bar was clicked
    /// and the segment index *within that group's* breadcrumbs.
    ///
    /// The `GroupId` is load-bearing, not decoration (#555): every group's bar
    /// is scanned here, but the segment list a bare index would be resolved
    /// against downstream (`Engine::rebuild_breadcrumb_segments`) is the
    /// **active** group's. With two groups open on files of different path
    /// depth, clicking the deeper group's third segment while the shallower
    /// group holds focus produced an out-of-range index and the click silently
    /// did nothing — the "breadcrumb clicks are dead" report.
    Hit(GroupId, usize),
    /// Click landed on a breadcrumb bar but not on a segment.
    OnBar,
    /// Click was not on any breadcrumb bar.
    Miss,
}

/// Resolve a breadcrumb click at `(x, y)` across all editor groups.
///
/// Iterates each group's `BreadcrumbBar`, checks bounds, then delegates to
/// the cached `StatusBarLayout::hit_test()` for segment resolution.
///
/// Both backends call this — zero per-backend breadcrumb click code.
pub fn resolve_breadcrumb_click(
    breadcrumbs: &[BreadcrumbBar],
    x: f64,
    y: f64,
    line_height: f64,
) -> BreadcrumbClickResult {
    for bc in breadcrumbs {
        if bc.segments.is_empty() {
            continue;
        }
        let bx = bc.bounds.x;
        let by = bc.bounds.y;
        let bw = bc.bounds.width;
        if y >= by && y < by + line_height && x >= bx && x < bx + bw {
            let local_x = (x - bx) as f32;
            let local_y = (y - by) as f32;
            let guard = bc.draw_layout.borrow();
            if let Some(ref layout) = *guard {
                if let quadraui::StatusBarHit::Segment(ref id) = layout.hit_test(local_x, local_y) {
                    if let Some(idx) = breadcrumb_action_index(id) {
                        return BreadcrumbClickResult::Hit(bc.group_id, idx);
                    }
                }
            }
            return BreadcrumbClickResult::OnBar;
        }
    }
    BreadcrumbClickResult::Miss
}

/// One breadcrumb bar ready to be painted via `Surface::StatusBar` (GTK) or
/// `Backend::draw_status_bar` (TUI).
pub struct BreadcrumbDrawTarget<'a> {
    pub rect: quadraui::Rect,
    pub bar: &'a quadraui::StatusBar,
    /// Cache slot to fill with the draw-time layout so
    /// `resolve_breadcrumb_click` can hit-test segments later.
    pub draw_layout: &'a std::cell::RefCell<Option<quadraui::StatusBarLayout>>,
}

/// Compute which breadcrumb bars should be painted this frame, and where.
///
/// Both backends call this instead of re-deriving the skip conditions
/// themselves — TUI previously duplicated the same `segments.is_empty() ||
/// terminal_maximized` check in both its split-group and single-group
/// branches, and GTK's ShellApp render path was simply missing it entirely,
/// which was the root cause of the #547 breadcrumb regression (the legacy
/// Relm4-era draw path that *did* draw breadcrumbs stopped being called
/// after the #540 ShellApp migration and nothing replaced it).
///
/// `bc.bounds` is already in the caller's screen space: both backends feed
/// `build_screen_layout` window rects in absolute terminal/pixel coordinates
/// (#550 — TUI used to compute content-area-relative rects and every draw
/// call site had to re-add the editor area's origin via an `origin_offset`
/// param here; that offset is always `(0.0, 0.0)` now that TUI's
/// `content_bounds` origin matches GTK's convention, so the param was
/// dropped).
///
/// Targets with zero width (the `min_x == f64::MAX` fallback in
/// `build_screen_layout` when a group has no matching window rects, e.g.
/// during a transient group-tree mutation) are filtered out here rather than
/// left to each caller: TUI's pre-existing call sites already guarded on
/// `rect.width > 0.0`, but GTK's new one didn't, so centralizing it removes a
/// footgun instead of asking every backend to remember it independently.
///
/// The painted rect's height is `bc.bounds.height` — whatever
/// `build_screen_layout`/`build_screen_layout_with_breadcrumb_row` computed
/// the breadcrumb row at (#700: fixed-pixel on GTK, one row on TUI) — not a
/// separate `line_height` argument, so paint can never drift from the space
/// actually reserved above the window content.
pub fn breadcrumb_draw_targets(
    screen: &ScreenLayout,
    terminal_maximized: bool,
) -> Vec<BreadcrumbDrawTarget<'_>> {
    if terminal_maximized {
        return Vec::new();
    }
    screen
        .breadcrumbs
        .iter()
        .filter(|bc| breadcrumb_is_drawn(bc))
        .map(|bc| BreadcrumbDrawTarget {
            rect: quadraui::Rect::new(
                bc.bounds.x as f32,
                bc.bounds.y as f32,
                bc.bounds.width as f32,
                bc.bounds.height as f32,
            ),
            bar: &bc.bar,
            draw_layout: &bc.draw_layout,
        })
        .collect()
}

/// Sync the backend's Nerd-Font-glyph-vs-fallback selection with current
/// settings. Both backends call this at startup and once per frame so
/// runtime toggles (`:set nonerdfonts`) take effect immediately; centralizing
/// it avoids the #547 regression where GTK's only call site was inside a
/// message handler (GTK's `Msg::CacheFontMetrics`, retired in #732) that stopped firing after the
/// #540 ShellApp migration, silently freezing the GTK backend's nerd-fonts
/// flag at its default (`false`) forever.
pub fn sync_nerd_fonts(b: &mut dyn quadraui::Backend, engine: &Engine) {
    b.set_nerd_fonts(engine.settings.use_nerd_fonts());
}

/// The family name the bundled Nerd Font icon subset (`ICON_FONT_BYTES`)
/// registers under on every platform it's actually been checked against
/// (GTK's own hardcoded fallback, `quadraui::gtk::NERD_FONT_FALLBACK_FAMILY`,
/// is this same literal). Used as a fallback when
/// `Backend::register_font_from_memory` reports failure for a reason other
/// than "not a font" — see [`register_nerd_font_fallback`].
const NERD_FONT_FALLBACK_FAMILY: &str = "Symbols Nerd Font";

/// Register the bundled Nerd Font icon subset with the backend and point its
/// fallback cascade at it, so Nerd-Font glyphs resolve instead of painting
/// as tofu (#937).
///
/// `sync_nerd_fonts` above only ever toggles the glyph-vs-fallback *flag*
/// (`Backend::set_nerd_fonts`) — it never tells a backend which font to
/// resolve a glyph against. Core Text (macOS) and DirectWrite (Windows)
/// never cascade to an arbitrary installed font for Private-Use-Area
/// codepoints without an explicit per-font fallback list, which is exactly
/// what `Backend::register_font_from_memory` + `Backend::
/// set_nerd_font_fallback` install. GTK used to get this for free from a
/// vimcode-side fontconfig filesystem install (`install_bundled_icon_font`,
/// deleted in #1130); now that quadraui#1013 gives `GtkBackend` a real
/// `register_font_from_memory` override (in-process, no filesystem write and
/// no font-cache-refresh shell-out), this one call registers the font for
/// GTK too — the same call
/// that already covered macOS/Windows. TUI takes the trait's no-op default
/// harmlessly (a fixed-cell backend has no font concept), so this call is
/// platform-neutral: no `#[cfg(target_os)]` needed here or at either call
/// site.
///
/// Call once, from `setup()` — **not** the per-frame
/// `sync_nerd_fonts`/`sync_per_frame_backend_state` path. Unlike the
/// nerd-fonts flag, there is no runtime setting to re-sync every frame, and
/// on macOS a second `register_font_from_memory` call in the same process
/// (e.g. a test harness constructing more than one `App`/backend) fails with
/// "duplicate PostScript name already registered" — harmless here because a
/// `None` falls back to [`NERD_FONT_FALLBACK_FAMILY`], the literal name the
/// font is known to register under, so `set_nerd_font_fallback` still gets
/// a usable family either way.
///
/// # Why it registers at most once per process (#1853)
///
/// "Call once" is the contract, but a *process* can legitimately build many
/// `App`s/backends — every `GtkDriver`/`TuiDriver` harness in the test suite
/// does, several hundred times per `cargo test` run — and each one used to
/// hand the whole font to its backend again. Registration is **not** free
/// and **not** idempotent on the platform side:
///
/// - GTK registers via Fontconfig's `FcConfigAppFontAddFile`, which has no
///   in-memory entry point, so quadraui writes the bytes to a fresh temp
///   file per call and deliberately never deletes it (Fontconfig/FreeType
///   may re-open it lazily at shape time). Each call also appends another
///   `FcPattern` — with its full charset — to the process's application
///   font set, which every subsequent font match then has to sort.
/// - macOS fails outright on the second call (duplicate PostScript name).
///
/// With the pre-#1853 114-glyph subset (29 KB) the per-harness copies were
/// invisible. With the full Symbols Nerd Font (2.5 MB) the same suite wrote
/// ~1.5 GB of temp files and grew the application font set by ~600 copies of
/// a 10,627-codepoint font in a single `cargo test` run — enough to redden a
/// CI runner that has far less headroom (disk and RAM) than a dev box.
///
/// So the first backend that *accepts* the font wins: its resolved family is
/// remembered for the process, and later backends are only handed the font
/// again if they cannot already see that family themselves
/// ([`quadraui::Backend::has_font_family`]). That keeps the semantics each
/// platform actually needs rather than assuming process-global registration:
/// GTK/Fontconfig and Core Text both register process-wide, so a later
/// backend answers `Some(true)` and skips; Win-GUI's DirectWrite custom
/// collection is per-backend, so a fresh `WinBackend` answers `Some(false)`
/// and does register; the TUI backend answers `None` ("can't know") and
/// takes the no-op default exactly as before.
pub fn register_nerd_font_fallback(b: &mut dyn quadraui::Backend) {
    /// The family the bundled icon font actually registered under, the
    /// first time any backend in this process accepted it.
    static REGISTERED_FAMILY: std::sync::OnceLock<String> = std::sync::OnceLock::new();

    if let Some(family) = REGISTERED_FAMILY.get() {
        // Already registered in this process *and* this backend can
        // resolve it — handing it the same 2.5 MB again would only leak
        // another copy. `Some(false)`/`None` both fall through to a real
        // registration attempt: the font must be registered per backend
        // (Win-GUI), or the backend cannot answer at all (TUI).
        if b.has_font_family(family) == Some(true) {
            b.set_nerd_font_fallback(family);
            return;
        }
    }

    let registered = b
        .register_font_from_memory(crate::app_support::ICON_FONT_BYTES)
        .and_then(|families| families.into_iter().next());
    if let Some(family) = &registered {
        // Only a *successful* registration may be remembered: a `None`
        // (TUI's no-op default, or macOS's duplicate-name rejection) must
        // not stop a later, more capable backend from registering for real.
        let _ = REGISTERED_FAMILY.set(family.clone());
    }
    let family = registered.unwrap_or_else(|| NERD_FONT_FALLBACK_FAMILY.to_string());
    b.set_nerd_font_fallback(&family);
}

/// One tab bar ready to be painted via `Surface::TabBar` (GTK) or
/// `render_tab_bar` (TUI).
pub struct TabBarDrawTarget<'a> {
    pub rect: quadraui::Rect,
    pub bar: &'a quadraui::TabBar,
    /// Per-tab icon sidecar parallel to `bar.tabs` (#703). Pass to **both**
    /// `Backend::draw_tab_bar_icons` and `Backend::tab_bar_layout_icons` —
    /// see [`build_tab_bar_icons`].
    pub icons: &'a [Option<quadraui::TabIcon>],
    /// Group this tab bar belongs to — the active (and only) group in
    /// single-group mode.
    pub group_id: GroupId,
}

/// Compute which tab bar(s) should be painted this frame, and where.
///
/// Both backends previously re-derived the same skip-condition + rect math
/// independently in their `if let Some(split) = screen.editor_group_split
/// { .. } else { .. }` blocks (#549, follow-up from the #547 breadcrumb
/// unification which deliberately left this one out to keep that PR scoped).
/// Each backend still does its own drawing + hit-test-geometry recovery
/// afterwards (GTK caches pixel hit-tests into `Rc<RefCell<...>>` maps, TUI
/// tracks visible tab counts) — that part isn't shareable and stays inline
/// at each call site.
///
/// #549 unified the *call sites* but kept the split-vs-single branch inside
/// this function, with a caller-supplied `single_group_rect` for the N=1 case.
/// #551 deleted that too: `ScreenLayout::group_tab_bars` is now populated for
/// every group count, so one group is just a split of one and the generic
/// bounding-box math below produces the identical full-width rect the
/// hand-written single-group arm used to hard-code. That removes the last
/// place a single-group tab-bar calculation could silently drift from the
/// N-group one — the exact failure #547 hit with breadcrumbs.
///
/// `tab_row_h` is the height of the tab row itself (GTK: `lh * 1.6` in
/// pixels; TUI: `1.0` row). `reserved_h` is the *total* space reserved above
/// the group's window content — the tab row plus, when breadcrumbs are on,
/// the breadcrumb row too (GTK: `tab_bar_height_px`; TUI: `tui_tbh`, 1 or
/// 2 rows) — used to recover the tab row's own top edge from
/// `GroupTabBar::bounds.y`, which is the *window* content's top edge.
///
/// `bounds` is already in the caller's screen space, same convention as
/// `breadcrumb_draw_targets` (#550 — the `origin_offset` param this function
/// used to carry for TUI's content-area-relative rects was dropped once TUI
/// started feeding absolute rects like GTK).
///
/// Targets with zero-width bounds (the `min_x == f64::MAX` fallback in
/// `build_screen_layout` when a group has no matching window rects, e.g.
/// during a transient group-tree mutation) are filtered out here, same as
/// `breadcrumb_draw_targets` — TUI's pre-existing call site already guarded on
/// `tab_w > 0`, but GTK's didn't, so centralizing it removes a footgun instead
/// of asking every backend to remember it independently.
pub fn tab_bar_draw_targets<'a>(
    engine: &Engine,
    screen: &'a ScreenLayout,
    tab_row_h: f64,
    reserved_h: f64,
) -> Vec<TabBarDrawTarget<'a>> {
    screen
        .group_tab_bars
        .iter()
        .filter(|gtb| tab_bar_is_drawn(engine, gtb))
        .map(|gtb| TabBarDrawTarget {
            rect: quadraui::Rect::new(
                gtb.bounds.x as f32,
                (gtb.bounds.y - reserved_h) as f32,
                gtb.bounds.width as f32,
                tab_row_h as f32,
            ),
            bar: &gtb.bar,
            icons: &gtb.icons,
            group_id: gtb.group_id,
        })
        .collect()
}

/// Present when the editor area is split into two or more independent groups.
///
/// This is a *marker* for "2 or more editor groups", not a container for the
/// per-group chrome: the tab bars and dividers it used to own now live on
/// `ScreenLayout::group_tab_bars` / `ScreenLayout::group_dividers`, which are
/// populated uniformly for every group count including one (#551). Backends
/// draw from those unconditionally; this type only gates the hit-test paths
/// that genuinely differ between one group and many (single-group tab-bar
/// clicks resolve through `ScreenLayout::tab_bar_hit_regions`).
#[derive(Debug, Clone)]
pub struct EditorGroupSplitData {
    /// ID of the currently focused group.
    pub active_group: GroupId,
    /// Total number of groups (always >= 2 when this is Some).
    pub num_groups: usize,
}

// ─── Tab drop-zone (shared) ─────────────────────────────────────────────────
//
// #1370: this used to be vimcode's own hand-rolled geometry (`TabDropGroup`,
// `build_tab_drop_groups`, `compute_tab_drop_zone`, `compute_tab_drop_overlay`)
// layered on top of quadraui's geometry-only `compute_drop_zone` primitive.
// quadraui#998 landed the missing piece — `resolve_tab_drop`, a
// host-owned-model adoption path that takes the drag *source* (which pane/tab,
// per vimcode's own `Engine::editor_groups`) and returns a position-based
// `TabDropInstruction` instead of mutating a `TabGroupController`-owned
// `Vec<Pane>` vimcode doesn't have. [`TabDropCtx`]/[`build_tab_drop_ctx`] is
// now the only local adapter: it flattens a frame's group geometry into the
// parallel index-aligned slices `resolve_tab_drop` (mutation) and
// `drop_zone_hit_test`/`drop_zone_overlay` (overlay) both key off of the same
// `group_idx`/`pane_idx`.

/// Per-frame tab-drop geometry, index-aligned so `group_ids[i]` /
/// `rects[i]` / `tab_counts[i]` all describe the same pane — the same index
/// quadraui's `resolve_tab_drop` calls `pane_idx` and `drop_zone_hit_test`
/// calls `group_idx`.
#[derive(Default, Debug)]
pub struct TabDropCtx {
    pub group_ids: Vec<GroupId>,
    pub rects: Vec<quadraui::DropGroupRect>,
    pub tab_counts: Vec<usize>,
    pub tab_bar_height: f32,
}

/// Lightweight group-bounds descriptor for [`build_tab_drop_ctx`].
pub struct DropGroupBounds {
    pub group_id: GroupId,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub content_height: f32,
    pub tab_scroll_offset: usize,
}

/// Build a [`TabDropCtx`] from a set of group bounds.
///
/// `tab_bar_height` is in the same units as the bounds (cells for TUI,
/// pixels for GTK/Win-GUI). Each group's bounds describe the
/// **content area** — the function prepends `tab_bar_height` above.
///
/// `tab_slots_map` maps `GroupId.0` → per-tab slot positions, index-aligned
/// to the group's own `Vec<Tab>` with a `(0.0, 0.0)` sentinel for any tab
/// scrolled off the strip — the convention `quadraui::DropGroupRect::tab_slots`
/// and `PaneDragRect::tab_slots` both document, and the one `build_tui_tab_slots`
/// / `TabBarHits::slot_positions` already produce.
pub fn build_tab_drop_ctx(
    group_bounds: &[DropGroupBounds],
    engine: &crate::core::engine::Engine,
    tab_bar_height: f32,
    tab_slots_map: &std::collections::HashMap<usize, Vec<(f32, f32)>>,
) -> TabDropCtx {
    let mut group_ids = Vec::with_capacity(group_bounds.len());
    let mut rects = Vec::with_capacity(group_bounds.len());
    let mut tab_counts = Vec::with_capacity(group_bounds.len());
    let breadcrumbs = engine.settings.breadcrumbs;

    for gb in group_bounds {
        let hidden = engine.is_tab_bar_hidden(gb.group_id);
        let eff_tbh = if hidden {
            if breadcrumbs {
                tab_bar_height / 2.0
            } else {
                0.0
            }
        } else {
            tab_bar_height
        };
        let tab_slots = if hidden {
            Vec::new()
        } else {
            tab_slots_map
                .get(&gb.group_id.0)
                .cloned()
                .unwrap_or_default()
        };
        group_ids.push(gb.group_id);
        rects.push(quadraui::DropGroupRect {
            bounds: quadraui::Rect::new(
                gb.x,
                gb.y - eff_tbh,
                gb.width,
                eff_tbh + gb.content_height,
            ),
            tab_slots,
        });
        tab_counts.push(
            engine
                .editor_groups
                .get(&gb.group_id)
                .map(|g| g.tabs.len())
                .unwrap_or(0),
        );
    }

    let effective_tbh = if group_bounds
        .iter()
        .any(|gb| engine.is_tab_bar_hidden(gb.group_id))
    {
        0.0
    } else {
        tab_bar_height
    };
    TabDropCtx {
        group_ids,
        rects,
        tab_counts,
        tab_bar_height: effective_tbh,
    }
}

/// Build [`DropGroupBounds`] from a `ScreenLayout`. Both TUI and GTK call
/// this when the `ScreenLayout` is available (draw path, or TUI's cached
/// layout).
///
/// [`DropGroupBounds`] (and [`build_tab_drop_ctx`], which reconstructs the
/// tab-bar band by subtracting `tab_bar_height` back out) expects
/// **content-area** bounds — i.e. already past the tab bar — which is exactly
/// what every `GroupTabBar::bounds` is ("content area of this group; tab bar
/// drawn at top edge"), in absolute screen space (#550).
///
/// #551: this used to branch on `editor_group_split`, with a single-group arm
/// that re-derived the content rect from a caller-supplied
/// `editor_origin`/`editor_size`/`tab_bar_height` triple because there was no
/// per-group `bounds` to read in that mode (#477 fix iteration 1: omitting the
/// `tab_bar_height` skip there produced a negative `bounds.y` that put the
/// cursor's tab-bar row just *above* the computed tab-bar band, so drops
/// always fell through to `Split(Top)` instead of `TabReorder`).
/// `ScreenLayout::group_tab_bars` is now populated for one group too, so the
/// generic arm covers it and those three parameters — plus the
/// `if split.is_some() { (0,0) } else { editor origin }` dance every caller
/// had to perform to feed them (#515) — are gone.
pub fn screen_to_drop_group_bounds(screen: &ScreenLayout) -> Vec<DropGroupBounds> {
    screen
        .group_tab_bars
        .iter()
        .map(|gtb| DropGroupBounds {
            group_id: gtb.group_id,
            x: gtb.bounds.x as f32,
            y: gtb.bounds.y as f32,
            width: gtb.bounds.width as f32,
            content_height: gtb.bounds.height as f32,
            tab_scroll_offset: gtb.tab_scroll_offset,
        })
        .collect()
}

/// Map a [`quadraui::DropEdge`] to vimcode's own [`SplitDirection`] plus
/// which side the new pane lands on. Mirrors [`TabDropInstruction`]'s own
/// `edge` → `split_direction` mapping, but in vimcode's `SplitDirection`
/// naming rather than quadraui's (the two crates name the same physical
/// split oppositely: quadraui's `SplitDirection::Horizontal` is vimcode's
/// `SplitDirection::Vertical` — a side-by-side pair divided by a vertical
/// line).
fn split_edge_to_vimcode(edge: quadraui::DropEdge) -> (SplitDirection, bool) {
    match edge {
        quadraui::DropEdge::Left => (SplitDirection::Vertical, true),
        quadraui::DropEdge::Right => (SplitDirection::Vertical, false),
        quadraui::DropEdge::Top => (SplitDirection::Horizontal, true),
        quadraui::DropEdge::Bottom => (SplitDirection::Horizontal, false),
    }
}

/// Resolve a live tab drag against `ctx` into vimcode's own
/// [`crate::core::window::DropZone`] — the shape `Engine::apply_tab_drop_zone`
/// already mutates from — via quadraui's host-owned-model adoption path
/// (`quadraui::compose::resolve_tab_drop`, #998) instead of a hand-rolled
/// geometry walk (#1370, replaces the old `compute_tab_drop_zone`).
///
/// `source` is `(group, tab index)` of the tab being dragged, captured when
/// the drag began — [`resolve_tab_drop`][quadraui::compose::resolve_tab_drop]
/// needs it up front to decide same-pane reorder vs. cross-pane merge, unlike
/// the old geometry-only `compute_tab_drop_zone`, which deferred that
/// decision to `apply_tab_drop_zone` at commit time.
///
/// `MoveToPane` collapses what used to be two distinct `DropZone` variants
/// (`Center` for a content-area drop, `TabReorder` for a tab-bar drop into
/// another group) into one instruction — its `insert_idx` already defaults to
/// "append at the end" for the former, so mapping both to `DropZone::TabReorder`
/// here reproduces the old mutation exactly. The lost visual distinction
/// (highlight-the-whole-group vs. an insertion bar) is not lost overall: the
/// overlay is resolved independently, from the same `ctx`, by
/// [`tab_drop_overlay`] against quadraui's geometry-only `DropZoneKind`.
pub fn resolve_tab_drop_zone(
    ctx: &TabDropCtx,
    source: (GroupId, usize),
    cursor_x: f32,
    cursor_y: f32,
) -> crate::core::window::DropZone {
    use crate::core::window::DropZone;
    use quadraui::compose::{TabDragSource, TabDropInstruction};

    let Some(pane_idx) = ctx.group_ids.iter().position(|&g| g == source.0) else {
        return DropZone::None;
    };
    let instr = quadraui::compose::resolve_tab_drop(
        TabDragSource {
            pane_idx,
            tab_idx: source.1,
        },
        &ctx.tab_counts,
        cursor_x,
        cursor_y,
        &ctx.rects,
        ctx.tab_bar_height,
    );
    match instr {
        TabDropInstruction::NoOp => DropZone::None,
        TabDropInstruction::Reorder {
            pane_idx, to_idx, ..
        } => ctx
            .group_ids
            .get(pane_idx)
            .map(|&gid| DropZone::TabReorder(gid, to_idx))
            .unwrap_or(DropZone::None),
        TabDropInstruction::MoveToPane {
            to_pane_idx,
            insert_idx,
            ..
        } => ctx
            .group_ids
            .get(to_pane_idx)
            .map(|&gid| DropZone::TabReorder(gid, insert_idx))
            .unwrap_or(DropZone::None),
        TabDropInstruction::SplitToNewPane {
            target_pane_idx,
            edge,
            ..
        } => {
            let (direction, new_first) = split_edge_to_vimcode(edge);
            ctx.group_ids
                .get(target_pane_idx)
                .map(|&gid| DropZone::Split(gid, direction, new_first))
                .unwrap_or(DropZone::None)
        }
    }
}

/// Resolve the drop-zone overlay geometry for `ctx` at `(cursor_x, cursor_y)`
/// — straight from quadraui's own [`quadraui::drop_zone_hit_test`] /
/// [`quadraui::drop_zone_overlay`], no vimcode-local adapter (#1370, replaces
/// the old `compute_tab_drop_overlay`/`TabDropOverlay`).
///
/// Deliberately independent of [`resolve_tab_drop_zone`]: that function
/// answers "what mutation should this drop commit", collapsing `Center` and
/// cross-pane `TabReorder` into one `MoveToPane`; this function answers "what
/// should the overlay look like *right now*", which still needs that
/// distinction (a highlight over the whole pane vs. an insertion bar) to
/// paint correctly. Both are pure queries over the same `ctx`, so computing
/// the hit test twice (once per question) costs nothing correctness-relevant.
pub fn tab_drop_overlay(
    ctx: &TabDropCtx,
    cursor_x: f32,
    cursor_y: f32,
    bar_thickness: f32,
    ghost_offset: f32,
) -> Option<quadraui::DropOverlay> {
    match quadraui::drop_zone_hit_test(cursor_x, cursor_y, &ctx.rects, ctx.tab_bar_height) {
        quadraui::DropZoneHit::Zone(zone) => Some(quadraui::drop_zone_overlay(
            &zone,
            &ctx.rects,
            cursor_x,
            cursor_y,
            ctx.tab_bar_height,
            bar_thickness,
            ghost_offset,
        )),
        quadraui::DropZoneHit::Empty => None,
    }
}
