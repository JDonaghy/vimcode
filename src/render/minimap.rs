use super::*;

// ─── Minimap (#35) ────────────────────────────────────────────────────────────

/// Fraction of a pane's own width the minimap strip may reserve, as an
/// *upper bound* — **TUI-only** since #1869 (`TUI_MINIMAP_SIZING`'s own
/// `fraction` field; TUI sizing is explicitly out of #1869's scope).
///
/// Before #1869 this doc comment claimed "VS Code does not scale its
/// minimap with window width at all: it derives a *fixed* width from
/// `minimap.maxColumn`" and this fraction/`gtk_minimap_sizing` shared it as
/// a narrow-pane cap on top of that fixed width. That claim was wrong —
/// confirmed against `EditorLayoutInfoComputer` in VS Code's own
/// `src/vs/editor/common/config/editorOptions.ts`: `minimap.maxColumn`
/// (120, [`MINIMAP_TARGET_COLS`]) is a *cap*, not the width. Below the cap
/// VS Code sizes the strip from the pane's remaining width, the editor's
/// real character width and its vertical scrollbar's width — see
/// [`vs_code_minimap_width_px`], which ports that real formula for
/// GTK/macOS/Win. This fraction survives only as TUI's own, deliberately
/// much cruder, column-based approximation.
pub const MINIMAP_WIDTH_FRACTION: f64 = 0.15;

/// Target minimap width for **GTK/macOS/Win**, in raw pixels — VS Code's
/// `minimap.maxColumn` default (#728): VS Code renders its minimap at one
/// pixel per assumed source column, so at 120px this is "precisely 1px per
/// column". #1869: this is the *cap* [`vs_code_minimap_width_px`]'s ported
/// formula clamps to — not, as a pre-#1869 version of this doc comment
/// claimed, the width itself.
///
/// #989: despite the name, this is GTK-only — it used to be reused for TUI's
/// `TUI_MINIMAP_SIZING` too on the theory that "columns for TUI" made the
/// same number trivially portable, but 120 *terminal columns* is wider than
/// almost any real terminal, so in `resolve_width` the fraction term always
/// won and the TUI strip scaled with the pane instead of holding steady. See
/// [`MINIMAP_TARGET_COLS_TUI`] for TUI's own, deliberately much smaller,
/// value.
pub(crate) const MINIMAP_TARGET_COLS: f64 = 120.0;

/// Target minimap width for TUI, in cell columns — **not** `MINIMAP_TARGET_COLS`
/// (#989). `MINIMAP_TARGET_COLS` (120) is meaningful only in GTK's pixel unit:
/// at 120*px* it is VS Code's "precisely 1px per column" parity, but the same
/// number read as 120 *columns* is wider than almost any real terminal, so in
/// `resolve_width`'s `target_cols.min(pane_width_cols * fraction)` the
/// `fraction` term always won instead — the strip scaled with the pane on
/// every ordinary terminal width, the exact bug #989 reports. This is the
/// same pixels-vs-columns unit split `gtk_minimap_sizing`'s doc comment
/// covers, just seen from the TUI side: reusing the pixel-flavoured constant
/// here is the defect, not a redundancy to "simplify" back.
///
/// Chosen so `target_cols.min(pane_width_cols * MINIMAP_WIDTH_FRACTION)`
/// resolves to this fixed value — not the fraction — across ordinary
/// terminal widths (80..200 cols): at the narrow end (80 cols),
/// `80 * 0.15 = 12`, so any target at or below 12 makes the target win from
/// 80 cols up. Below 80 cols the fraction (and eventually `MINIMAP_MIN_COLS`)
/// still take over, so the strip keeps narrowing smoothly on panes that
/// genuinely can't afford it.
pub(crate) const MINIMAP_TARGET_COLS_TUI: f64 = 12.0;

/// Floor on the reserved width for TUI, in cell columns directly — see
/// `gtk_minimap_sizing`'s doc comment for why GTK needs its own, separate
/// floor rather than this one scaled by `char_width`.
const MINIMAP_MIN_COLS: f64 = 6.0;

/// Ceiling on the reserved width for TUI, same units/caveats as
/// `MINIMAP_MIN_COLS`.
const MINIMAP_MAX_COLS: f64 = 30.0;

/// Text columns that must survive after reserving the strip. Below this the
/// minimap suppresses itself rather than squeezing the editor into a sliver.
pub(crate) const MINIMAP_MIN_TEXT_COLS: f64 = 30.0;

/// TUI's VS-Code-parity minimap width policy, in cell columns — see
/// `minimap_reserved_width`'s doc comment for why this and
/// [`gtk_minimap_sizing`] stay two backend-specific values (chosen by the
/// caller, who already knows its own backend) rather than one shared
/// column-based formula selected by a runtime `char_width` check. Named and
/// shaped after [`TUI_PICKER_SIZING`]/[`gtk_picker_sizing`] above — the same
/// established pattern for a genuine per-backend sizing difference.
pub const TUI_MINIMAP_SIZING: quadraui::MinimapSizing = quadraui::MinimapSizing::VsCodeParity {
    target_cols: MINIMAP_TARGET_COLS_TUI as f32,
    fraction: MINIMAP_WIDTH_FRACTION as f32,
    min: MINIMAP_MIN_COLS as f32,
    max: MINIMAP_MAX_COLS as f32,
};

/// GTK/macOS/Win's VS-Code-parity minimap width policy, in raw pixels —
/// see `minimap_reserved_width`'s doc comment.
///
/// A single column-based `MinimapSizing` (`resolve_width(pane_width,
/// real_char_width)`) using `resolve_width`'s own
/// `target_cols.min(pane_width_cols * fraction).clamp(min, max)` arithmetic
/// was tried first and reverted:
/// `resolve_width` normalises `pane_width` into columns via the *editor's*
/// `char_width` before applying `fraction`/`min`/`max`, then converts back —
/// correct when the bounds genuinely mean "columns of the editor's own
/// font", but VS Code's minimap renders in its *own*, much smaller font,
/// decoupled from the editor's for that formula's purposes. Feeding a real
/// editor `char_width` (7-9px) through that *linear-fraction* formula moved
/// an ordinary wide GTK pane's strip from VS Code's ~120px to ~180-240px —
/// caught by a driver test (`gtk::testing::minimap`) exercising the real
/// paint path.
///
/// #1869: VS Code's *real* width formula (`EditorLayoutInfoComputer` in
/// `editorOptions.ts`) is not that linear-fraction shape at all — it is
/// [`vs_code_minimap_width_px`], which genuinely does divide by the
/// editor's real character width (that's how VS Code arrives at ~100px for
/// Menlo 12 instead of a flat 120px). `resolve_width` has no way to express
/// that division, so `minimap_reserved_width` recognizes this policy by its
/// `target_cols` (120, [`MINIMAP_TARGET_COLS`] — never collides with TUI's
/// `target_cols` of 12, [`MINIMAP_TARGET_COLS_TUI`]) and calls
/// `vs_code_minimap_width_px` directly instead of `sizing.resolve_width()`.
/// `fraction`/`min`/`max` below are therefore inert placeholders, kept only
/// so this still type-checks as a `quadraui::MinimapSizing::VsCodeParity` —
/// see docs/IRREDUCIBLE_SURFACE.md.
pub fn gtk_minimap_sizing() -> quadraui::MinimapSizing {
    quadraui::MinimapSizing::VsCodeParity {
        target_cols: MINIMAP_TARGET_COLS as f32,
        fraction: 0.0,
        min: 0.0,
        max: MINIMAP_TARGET_COLS as f32,
    }
}

/// VS Code's own minimap width formula (#1869), ported from
/// `EditorLayoutInfoComputer` in
/// `src/vs/editor/common/config/editorOptions.ts`. [`MINIMAP_TARGET_COLS`]
/// (120, `minimap.maxColumn`) is a *cap* — below it, the strip is sized
/// from the pane's own remaining width, the editor's real character width
/// (`typical_char_width`, VS Code's `typicalHalfwidthCharacterWidth`) and
/// VS Code's configured vertical scrollbar width
/// ([`MINIMAP_VERTICAL_SCROLLBAR_WIDTH_PX`]).
///
/// ```text
/// minimapCharWidth  = 1                    // minimap.scale 1
/// minimapMaxWidth   = floor(120 * minimapCharWidth)
/// minimapWidth      = min(minimapMaxWidth,
///                         max(0, floor((remainingWidth - verticalScrollbarWidth - 2)
///                                       * minimapCharWidth
///                                       / (typicalHalfwidthCharacterWidth + minimapCharWidth)))
///                         + MINIMAP_GUTTER_WIDTH(8))
/// ```
///
/// `remaining_width` is **already gutter-subtracted** by the caller
/// ([`minimap_reserved_width`]) — matching the formula block quoted above,
/// where `remainingWidth = editor outer width - gutter (glyph margin + line
/// numbers + folding)`, never the pane's raw rect width. A pre-#1869-fix-
/// round-1 version of this function was fed the full, un-subtracted pane
/// width here and this doc comment claimed that was correct ("the caller's
/// own pane width") — it was not: at the issue's measured case (editor area
/// ≈872px, Menlo 12, `cw≈7.22`) that shape gave 112px against VS Code's own
/// ≈97–104px, off by roughly the gutter term it was skipping.
/// `typical_char_width` is the *real* editor font's cell width — unlike the
/// linear-fraction formula this replaces (see `gtk_minimap_sizing`'s doc
/// comment), this one is explicitly a function of it, so the minimap
/// genuinely narrows on a wider editor font, matching VS Code.
pub(crate) fn vs_code_minimap_width_px(remaining_width: f64, typical_char_width: f64) -> f64 {
    let cw = if typical_char_width > 0.0 {
        typical_char_width
    } else {
        1.0
    };
    let minimap_max_width = (MINIMAP_TARGET_COLS * MINIMAP_SCALE_CHAR_WIDTH_PX).floor();
    let inner = ((remaining_width - MINIMAP_VERTICAL_SCROLLBAR_WIDTH_PX - 2.0)
        * MINIMAP_SCALE_CHAR_WIDTH_PX
        / (cw + MINIMAP_SCALE_CHAR_WIDTH_PX))
        .floor()
        .max(0.0);
    (inner + MINIMAP_GUTTER_WIDTH_PX).min(minimap_max_width)
}

/// The minimap's own per-source-column pixel width (VS Code's
/// `minimapCharWidth`, `minimap.scale` 1) — distinct from
/// [`vs_code_minimap_width_px`]'s `typical_char_width` argument, which is
/// the *editor's* character width and only determines how many source
/// columns fit in the pane's remaining width.
const MINIMAP_SCALE_CHAR_WIDTH_PX: f64 = 1.0;

/// Padding VS Code reserves between the minimap strip and the content it
/// summarises (`MINIMAP_GUTTER_WIDTH` in `editorOptions.ts`).
pub(crate) const MINIMAP_GUTTER_WIDTH_PX: f64 = 8.0;

/// VS Code's own default `editor.scrollbar.verticalScrollbarSize` (14 CSS
/// px), consulted only by [`vs_code_minimap_width_px`]'s ported formula —
/// **not** the width any backend's own vertical scrollbar actually paints
/// at today. That gutter is quadraui's `EditorLayout::layout_with_options`
/// (`v_scrollbar_w = cell_width`, a font-sized ~7-9px column, not a fixed
/// 14px one) — a quadraui-side gap drafted in
/// `docs/PENDING_QUADRAUI_ISSUES.md` (not yet filed as a real GitHub issue:
/// #1869 cannot close until it is and quadraui ships a fix), since vimcode
/// must not work around it locally (Platform-Neutrality Rule). VS Code
/// itself computes the minimap's width against its *configured* scrollbar
/// width regardless of what ends up painted, so this constant is correct
/// for the minimap formula even while that scrollbar gap is still open.
const MINIMAP_VERTICAL_SCROLLBAR_WIDTH_PX: f64 = 14.0;

/// Buffer lines sampled per *display* row of minimap height.
///
/// This is quadraui's TUI braille factor — the denser of the two backends —
/// and it is deliberately used for both. Per quadraui's own `minimap_app`
/// example, the app "samples generously at the denser factor" and lets each
/// rasteriser group/tile the result via `Minimap::layout`, so vimcode never
/// has to know which backend is about to paint. For GTK (which paints one
/// buffer line per row) it also lands the row pitch at `line_height / 4`,
/// i.e. the ~2–6 px band #35 asks for.
pub(crate) const MINIMAP_LINES_PER_ROW: usize = 4;

/// Ceiling on the compression factor `K` [`build_minimap_data`] applies —
/// still a safety clamp (#1186's "compressed scale mode"), but since #1211 no
/// longer the *load-bearing* knob: `K` is derived from the strip's own
/// geometry (see [`MINIMAP_VIEWPORT_MULTIPLE`]), never from
/// `total_buffer_lines`, so `K` stays small and constant for every real
/// caller and only a pathological `rect`/`editor_visible_rows` pairing (a
/// strip far shorter than the editor's own viewport) could ever push it this
/// high.
///
/// Chosen empirically against [`quadraui::primitives::minimap::BLOCK_LINE_SAMPLE_CAP`]
/// (8): once a block is wider than the cap,
/// `quadraui::primitives::minimap::block_sample_indices` already
/// spreads exactly `cap` samples across it regardless of how much wider it
/// gets, so growing `K` past the point where blocks already exceed the cap
/// costs no extra line fetches — only a larger buffer span per block. A
/// `K_max` far beyond that point (64, i.e. blocks up to 8x the sample cap)
/// still keeps each block's aggregated row a meaningful, dithered summary of
/// real content rather than a coin-flip over the whole file.
///
/// `pub(crate)` since #1211 so `src/tui_main/shell_app.rs`'s tests can read
/// the real ceiling instead of hand-copying a `COMPRESSION_CEILING_MIRROR`
/// constant that could drift out of sync with this one.
pub(crate) const MINIMAP_MAX_COMPRESSION: usize = 64;

/// How many editor viewports [`build_minimap_data`]'s window covers at its
/// default, uncompressed scale (`K == 1`) — VS Code's own default minimap
/// (`editor.minimap.size: "proportional"`, decoded from the shipped bundle
/// for #1211): a fixed `BASE_CHAR_HEIGHT * scale` row pitch against a
/// `lineHeight` roughly 9x taller, independent of the file's length. `K` is
/// then chosen (see the comment at its own definition, below) so the
/// window's real line count — `target_lines * K` — comes out to
/// `editor_visible_rows * MINIMAP_VIEWPORT_MULTIPLE`, regardless of how
/// `target_lines` itself was derived (GTK's ~2px fixed pitch vs the TUI's
/// braille-native one): on GTK, `target_lines` already comes out to roughly
/// `9 * editor_visible_rows` (`gtk_row_capacity`, above), so `K` naturally
/// falls out to `1` — one buffer line per row, #1093's slide re-engaged. On
/// the TUI, `target_lines` is only `4 * editor_visible_rows`
/// (`MINIMAP_LINES_PER_ROW`), so `K` comes out to `ceil(9 / 4) == 3` — three
/// real buffer lines dithered into each braille row — the TUI's rough
/// approximation of the same ~9-viewport window, bounded and constant
/// instead of growing without limit as the file gets longer (#1211's root
/// cause: the pre-fix `K` was `total_buffer_lines.div_ceil(target_lines)`,
/// which squeezed the *whole file* into the strip for every file shorter
/// than `MINIMAP_MAX_COMPRESSION * target_lines` — i.e. essentially every
/// real file — disabling #1093's slide on both backends and regressing GTK,
/// which was already at exact VS Code parity at `K == 1`, from correct to
/// "whole file squeezed in").
const MINIMAP_VIEWPORT_MULTIPLE: usize = 9;

/// Ceiling on how many characters into a line `build_minimap_data`'s
/// `to_col` closure ever looks when measuring a highlight's own column.
/// Past this many characters, `aggregate_spans` would discard the column
/// anyway (see `grid.cols` below), so counting further is wasted, unbounded
/// work on a long (e.g. minified) line (#728). Renamed from
/// `MINIMAP_SPAN_COLS` by #1030, which stopped using it to size
/// `grid.cols` itself (that now comes from the painted strip's own width)
/// — it survives purely as this scan cap.
const MINIMAP_MAX_RELEVANT_COLS: usize = 400;

/// The `+ 1` keeps `to_col`'s boundary value itself exact rather than
/// off-by-one short (#728).
const MINIMAP_COL_SCAN_LIMIT: usize = MINIMAP_MAX_RELEVANT_COLS + 1;

// #1098: block partitioning, coverage aggregation and dither arithmetic all
// moved to quadraui#1012 (`quadraui::primitives::minimap::{block_bounds,
// block_sample_indices, sample_blocks, dither_threshold_met, BAYER4,
// BLOCK_LINE_SAMPLE_CAP}`) — this file used to carry byte-identical private
// copies (`minimap_block_bounds`, `MINIMAP_BLOCK_LINE_SAMPLE_CAP`,
// `minimap_block_sample_indices`, `MINIMAP_BAYER4`,
// `minimap_block_dither_threshold_met`, `minimap_block_text`), built here by
// #1085 before the primitive existed to hold them. `build_minimap_data`
// below now calls the quadraui functions directly; the arithmetic itself
// (and its unit tests) live in quadraui, not here.

// #1096: how many times `minimap_line_text` has actually fetched a buffer
// line's text — a deterministic work counter, not a wall-clock ceiling.
// `pub(crate)` and `#[cfg(test)]`-only (compiled out of any non-test build)
// so both this file's own tests and the black-box companion in
// `tui_main::shell_app` (which only has a `TuiDriver`, not this file's
// private sampling internals, to assert against) can pin "line fetches per
// frame is bounded by strip geometry, not buffer length" without the flake
// risk an absolute-time budget has — the old #728 guard's own doc comment
// records 517-586ms flakes at a ~20% margin under full-suite contention,
// and a *constant*-factor regression (e.g. 8x more reads at unchanged strip
// geometry) cancels out of a same-geometry ratio entirely, which is exactly
// why that guard stayed green through #1096's regression. Use
// `count_minimap_line_fetches` rather than touching this directly.
#[cfg(test)]
thread_local! {
    pub(crate) static MINIMAP_LINE_FETCH_COUNT: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// Reset [`MINIMAP_LINE_FETCH_COUNT`] and return its value after running
/// `f` — isolates exactly the line fetches `f` itself triggers (one
/// `build_minimap_data` call, or one driver redraw).
#[cfg(test)]
pub(crate) fn count_minimap_line_fetches(f: impl FnOnce()) -> usize {
    MINIMAP_LINE_FETCH_COUNT.with(|c| c.set(0));
    f();
    MINIMAP_LINE_FETCH_COUNT.with(|c| c.get())
}

/// Read buffer line `i`'s text, trimming the trailing newline — the exact
/// line-fetch `build_minimap_data` used inline before #1085 split it out,
/// and now hands to `quadraui::sample_blocks` as its `line_at` accessor
/// (#1098) so a block's aggregated row text is built without vimcode ever
/// materialising the window as a `Vec<String>`.
///
/// #1096: the highlight-mapping loop this doc comment used to say shared
/// this fetch no longer does — see `minimap_line_content_byte_len` below,
/// which answers the loop's only real question (a byte offset's character
/// column within one line) directly against the rope, without ever
/// materialising the line as a `String`. So every call to this function is
/// now bounded by `sample_blocks`'s own read budget, which reads at most
/// [`quadraui::primitives::minimap::BLOCK_LINE_SAMPLE_CAP`] lines per
/// block — `target_lines * BLOCK_LINE_SAMPLE_CAP` `String`s per frame,
/// never buffer length (the #728 invariant
/// `minimap_line_fetch_count_tracks_target_lines_not_buffer_size` pins
/// with [`MINIMAP_LINE_FETCH_COUNT`], incremented here in tests only).
fn minimap_line_text(rope: &ropey::Rope, i: usize) -> String {
    #[cfg(test)]
    MINIMAP_LINE_FETCH_COUNT.with(|c| c.set(c.get() + 1));
    rope.line(i)
        .as_str()
        .map(|s| s.trim_end_matches(['\n', '\r']).to_string())
        .unwrap_or_else(|| {
            rope.line(i)
                .to_string()
                .trim_end_matches(['\n', '\r'])
                .to_string()
        })
}

/// Byte length of a buffer line's content, excluding any trailing line
/// terminator (`\n`, `\r\n`, or `\r`) — the same trim `minimap_line_text`
/// applies, but read directly off the caller's own `RopeSlice` (a single
/// line, already fetched) so it costs no allocation and no further
/// whole-rope traversal. Used by `build_minimap_data`'s highlight loop
/// (#1096) to clamp a highlight span's byte offset to the real line it was
/// measured against, the same clamp `to_col` used to get for free from
/// `line_str.len()` before that loop fetched a `String` per line.
fn minimap_line_content_byte_len(slice: ropey::RopeSlice) -> usize {
    let mut n = slice.len_chars();
    let mut len = slice.len_bytes();
    while n > 0 {
        let ch = slice.char(n - 1);
        if ch == '\n' || ch == '\r' {
            len -= ch.len_utf8();
            n -= 1;
        } else {
            break;
        }
    }
    len
}

/// The active window's minimap: a quadraui `Minimap` primitive plus the strip
/// it occupies. Both backends consume this verbatim — `rect` goes straight to
/// `draw_minimap`, and the returned `MinimapLayout` answers clicks.
#[derive(Debug, Clone)]
pub struct RenderedMinimap {
    /// The editor window this strip belongs to.
    pub window_id: WindowId,
    /// The strip, in the caller's units (pixels for GTK, cells for TUI).
    pub rect: WindowRect,
    /// The quadraui descriptor. Already sampled — `lines` is final, but
    /// `syntax_spans` starts **empty**. Colour aggregation can't happen
    /// here: it needs `cols_per_cell`, and that scale is no longer a host
    /// constant since quadraui#1032 made TUI's default resolve adaptively
    /// from the buffer's own width (GTK's stays a fixed `1`). Only the
    /// backend that is about to paint knows which one it'll use — asking
    /// via `Backend::minimap_layout` requires a live backend, which this
    /// struct (built at screen-layout time, before any backend-specific
    /// call) does not have. `draw_minimap_strip` (`src/render.rs`) does
    /// have one, reads `cols_per_cell` back from it, aggregates
    /// `raw_syntax_spans` with a matching [`quadraui::MinimapGrid`], and
    /// fills this field in immediately before painting — see that
    /// function's doc comment (#1175).
    pub minimap: quadraui::Minimap,
    /// Syntax highlight spans in raw, un-aggregated form (real character
    /// columns, not cells) — [`draw_minimap_strip`] folds these into
    /// `minimap.syntax_spans` once it knows the real `cols_per_cell` to
    /// aggregate them at (#1175). Kept separate from `minimap` itself
    /// (rather than one `RenderedMinimap::raw_spans` living beside an
    /// eagerly-aggregated `syntax_spans`) so there is exactly one place
    /// aggregation ever happens, not two that could silently drift.
    pub raw_syntax_spans: Vec<quadraui::MinimapSpan>,
    /// Backend-resolved layout from this frame's [`draw_minimap_strip`] paint
    /// of this strip, if any (#1253) — read back by [`minimap_click_line`]
    /// and [`minimap_press`] instead of each re-deriving its own
    /// `layout_with_sizing` call. Mirrors [`BreadcrumbBar::draw_layout`]'s
    /// same paint-then-read-back shape: `RefCell` because `draw_minimap_strip`
    /// only ever sees `&ScreenLayout` (this `RenderedMinimap` is already
    /// behind the shared `cached_screen_layout` `RefCell` by the time
    /// painting runs) and still needs to fill this field in after paint.
    ///
    /// `None` until the first paint of a given frame's `ScreenLayout`, and
    /// permanently `None` for any `ScreenLayout` a caller builds without ever
    /// calling `draw_minimap_strip` — most of this module's own unit tests,
    /// which click-test geometry directly off `build_screen_layout`'s output.
    /// Both click resolvers fall back to re-deriving via `layout_with_sizing`
    /// in that case (this field's pre-#1253 behaviour verbatim), so a missing
    /// paint can only ever cost the recompute this issue exists to skip —
    /// never a dropped or misrouted click.
    pub resolved_layout: std::cell::RefCell<Option<quadraui::MinimapLayout>>,
    /// The [`quadraui::MinimapScale`] this strip's `lines`/`window_len`
    /// were sized against (`Settings::resolved_minimap_scale`, issue
    /// #1532) — [`draw_minimap_strip`] pushes this onto the live backend
    /// via `Backend::set_minimap_scale` immediately before painting, so
    /// the row pitch quadraui actually rasterises at can never drift from
    /// the one this strip's own sampling math (`build_minimap_data`'s
    /// `effective_row_pitch_px`) assumed.
    pub minimap_scale: quadraui::MinimapScale,
}

/// Width the minimap reserves alongside the editor, in the caller's units.
///
/// #828/quadraui#776: `sizing` — [`TUI_MINIMAP_SIZING`] or
/// [`gtk_minimap_sizing`] — is supplied by the caller, who already knows its
/// own backend by construction, instead of being selected here from a
/// `char_width > 1.0` runtime check. This function still never branches on
/// backend identity — only on which `sizing` *value* the caller explicitly
/// handed it, via the match below — and it only asks that value to resolve
/// a width before applying the on/off decision.
///
/// #1869: TUI's column-based `sizing.resolve_width()` path (unchanged,
/// still forced to `char_width == 1.0` — out of #1869's scope) cannot
/// express VS Code's real width formula, which divides by the editor's
/// character width rather than scaling linearly by a fraction. So
/// `gtk_minimap_sizing`'s `target_cols` (120, [`MINIMAP_TARGET_COLS`]) is
/// used as that value's own marker — still never colliding with TUI's
/// `target_cols` of 12 ([`MINIMAP_TARGET_COLS_TUI`]) — to route GTK/macOS/
/// Win to [`vs_code_minimap_width_px`] instead, fed the *real* `char_width`
/// rather than a forced `1.0`. The real `char_width` (`cw` below) is always
/// used for the `MINIMAP_MIN_TEXT_COLS` suppression check, which genuinely
/// wants the editor's own font metric regardless of which width formula
/// ran.
///
/// Delegates the on/off decision to `quadraui::reserved_width` so both
/// backends (and `build_screen_layout`, which shrinks each window rect by
/// exactly this much) reclaim identical geometry when `:set nominimap`
/// turns the strip off.
///
/// `gutter_width` (#1869 review round 1) is the pane's own line-number/fold
/// gutter, in the same unit as `rect_width` — subtracted *only* on the
/// [`vs_code_minimap_width_px`] path, matching VS Code's own
/// `remainingWidth = editor outer width - gutter` (see that function's doc
/// comment). TUI's `sizing.resolve_width()` path ignores it: TUI sizing is
/// out of #1869's scope and its callers (and every unit test below except
/// the real `build_screen_layout` call site) pass `0.0`, i.e. "rect_width is
/// already the remaining width" — the function's pre-#1869-round-1 contract,
/// preserved for every caller that isn't the real per-window production path.
pub fn minimap_reserved_width(
    engine: &Engine,
    rect_width: f64,
    char_width: f64,
    sizing: quadraui::MinimapSizing,
    gutter_width: f64,
) -> f64 {
    let want = match sizing {
        quadraui::MinimapSizing::VsCodeParity { target_cols, .. }
            if target_cols == MINIMAP_TARGET_COLS as f32 =>
        {
            vs_code_minimap_width_px((rect_width - gutter_width).max(0.0), char_width)
        }
        _ => sizing.resolve_width(rect_width as f32, 1.0).unwrap_or(0.0) as f64,
    };
    let cw = if char_width > 0.0 { char_width } else { 1.0 };
    let has = engine.settings.minimap && rect_width >= want + MINIMAP_MIN_TEXT_COLS * cw;
    quadraui::reserved_width(want as f32, has) as f64
}

/// This window's own line-number/fold gutter width, in the caller's pixel
/// unit — the `remaining_width = editor outer width - gutter` term
/// [`vs_code_minimap_width_px`]'s formula needs and the pre-#1869-round-1
/// shape silently skipped (2026-review finding: "remaining_width is wrong").
///
/// Computed from [`calculate_gutter_cols`] — the same function
/// `build_rendered_window` uses for the real painted gutter — fed this
/// window's own buffer/settings state directly, since this runs *before*
/// `build_rendered_window` (the `minimap_widths` map in
/// `build_screen_layout_with_breadcrumb_row` has to know the strip's width
/// before it can compute each window's rect). Two inputs
/// `build_rendered_window`'s own `has_git`/`has_bp` fold in are deliberately
/// **not** replicated here, both documented, bounded deviations:
///
/// - `has_git` there also covers the in-flight ACP-turn-checkpoint overlay
///   (`acp_turn_status`) — here it is just `!git_diff.is_empty()`, since the
///   checkpoint lookup needs a `pre_turn_content` fetch this sizing pass has
///   no reason to pay for on every frame.
/// - `has_bp` there also covers an in-viewport decor sign (`has_decor_sign`)
///   — here it is just "a real breakpoint is set, or a DAP session is
///   live", since the decor check needs a resolved scroll/visible-lines
///   window this sizing pass doesn't have yet.
///
/// Both gaps can only make the *real* painted gutter one column wider than
/// this estimate, and only while a checkpoint/decor sign is actually
/// outstanding — a transient, bounded case where the minimap strip comes
/// out fractionally wider than VS Code's formula predicts, not a silent
/// misrepresentation of the formula's own `remainingWidth` term.
pub(crate) fn window_minimap_gutter_width_px(
    engine: &Engine,
    window_id: WindowId,
    char_width: f64,
) -> f64 {
    let Some(window) = engine.windows.get(&window_id) else {
        return 0.0;
    };
    let Some(buffer_state) = engine.buffer_manager.get(window.buffer_id) else {
        return 0.0;
    };
    let total_lines = buffer_state.buffer.len_lines();
    let line_number_mode = if buffer_state.md_rendered.is_some() {
        LineNumberMode::None
    } else {
        engine.settings.line_numbers
    };
    let has_git = !buffer_state.git_diff.is_empty();
    let has_bp = buffer_state
        .file_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .and_then(|key| engine.dap_breakpoints.get(&key).map(|v| !v.is_empty()))
        .unwrap_or(false)
        || engine.dap_session_active;
    let cols = calculate_gutter_cols(line_number_mode, total_lines, char_width, has_git, has_bp);
    cols as f64 * char_width
}

/// Width of the scroll-affordance gutter a pane's rightmost edge must keep
/// clear of the minimap strip (#1094), in the caller's own unit.
///
/// `scrollbar_reserve` (0.0 for TUI) states only GTK's overlay-chrome
/// concept (`quadraui::Backend::scrollbar_reserve()`, #828/quadraui#776) —
/// it says nothing about TUI's own vertical scrollbar, which is an inline
/// column `quadraui::tui::editor::draw_editor` always paints at its own
/// `area.right - 1` whenever the window overflows (`quadraui`'s `Editor`
/// primitive, not a value this crate controls — see the doc comment on the
/// `Surface::Editor` push in `app.rs`). That column is exactly one
/// `char_width` wide. Taking the larger of the two generalises to both
/// backends without branching on which one is asking: on TUI
/// (`scrollbar_reserve == 0.0`, `char_width == 1.0`) this resolves to
/// exactly one cell; on GTK it resolves to `scrollbar_reserve` whenever
/// that's already the wider of the two, which is the ordinary case (a GTK
/// `char_width` in pixels and its 8px overlay reserve are the same order of
/// magnitude).
///
/// `pub(crate)` (not private) since #1094's GTK driver tests
/// (`gtk::testing::minimap`) need the same formula to predict the real
/// paint path's column count, not just this module's own tests.
///
/// #1094 review: on GTK the `.max` does *not* reliably resolve to
/// `scrollbar_reserve` — `gtk::testing`'s own
/// `cols_without_reserve`/`cols_with_reserve` fixture asserts real editor
/// char widths already exceed the 8px overlay reserve
/// (`cw_probe > SCROLLBAR_RESERVE_PX`), so `char_width` is the *ordinary*
/// winner on GTK, not a rare very-large-font edge case. That's still
/// correct here — reserving a full extra text column's worth of gutter
/// alongside the strip is a superset of the ~8px the real scrollbar
/// overlay needs, never a shortfall — but it does mean the strip can end
/// up a little narrower than `minimap_reserved_width` alone would afford
/// whenever `char_width > scrollbar_reserve`, everyday on GTK rather than
/// only at unusually large font sizes. Left as a comment rather than a
/// `debug_assert!`: there is no threshold here that's actually wrong to
/// cross, only a width trade-off worth knowing about.
pub(crate) fn scroll_gutter_width(scrollbar_reserve: f64, char_width: f64) -> f64 {
    scrollbar_reserve.max(char_width)
}

/// Build the quadraui `Minimap` for `window_id` over the strip `rect`.
///
/// Returns `None` when the setting is off (the caller passes a zero-width
/// strip in that case), when the strip cannot hold a single row, or when the
/// window/buffer has gone away. All sampling (`sample_blocks`) and colour
/// reduction (`aggregate_spans`) is quadraui's — this function only maps
/// vimcode's tree-sitter byte-offset highlights into quadraui's
/// `MinimapSpan` input type.
///
/// `editor_visible_rows` is the *editor pane's* own visible row count —
/// deliberately a separate parameter from `rect`/`line_height` (which size
/// the minimap *strip*, and drive `target_lines`, the sampling budget)
/// since #1085: the two only coincide because the strip is as tall as the
/// editor pane today. Passing the editor's own count rather than
/// re-deriving it from the strip's height keeps the viewport highlight
/// band correct even if a future layout ever makes the strip shorter than
/// the editor.
pub fn build_minimap_data(
    engine: &Engine,
    theme: &Theme,
    window_id: WindowId,
    rect: WindowRect,
    line_height: f64,
    editor_visible_rows: usize,
) -> Option<RenderedMinimap> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let lh = if line_height > 0.0 { line_height } else { 1.0 };
    let display_rows = (rect.height / lh).floor() as usize;
    if display_rows == 0 {
        return None;
    }
    // #1052: GTK's minimap rasteriser paints each sampled line at a
    // **fixed** `ROW_PITCH_PX` (2px) row pitch, completely decoupled from
    // the editor's own `line_height` (`quadraui::gtk::minimap`'s module
    // doc: "Rows tile at a fixed ROW_PITCH_PX ... independent of the
    // file's length"). `display_rows` above assumes the opposite — that
    // one minimap display row costs one `line_height` — which is correct
    // for TUI (whose braille row genuinely is cell-native, `lh == 1.0`)
    // but drastically under-samples for GTK: at a typical ~20px
    // `line_height`, GTK can actually paint ~10x more 2px rows in the same
    // strip height than `display_rows * MINIMAP_LINES_PER_ROW` ever
    // samples. Once those too-few samples run out, GTK's rasteriser (which
    // paints sampled rows top-aligned, not stretched to fill the strip —
    // see `Minimap::layout_with_sizing`'s `FixedPitch` arm) simply stops,
    // leaving the rest of the strip flat/background — a *vertical* colour
    // falloff by buffer line, reproduced and measured in
    // `gtk::testing::minimap::minimap_paints_syntax_colour_near_the_bottom_of_a_long_file_via_gtk_driver`
    // (distinct colours per decile of strip height dropped from ~9 to 1
    // right where this under-sampling predicted, on an unfixed `develop`).
    //
    // This mirrors `visible_span_cols`'s column-axis fix immediately below
    // (#1030): take the max of both backends' real row requirements rather
    // than branching on backend identity (Platform-Neutrality Rule — this
    // stays shared code). `rect.height` is in the caller's own native unit
    // (pixels for GTK, cell rows for TUI), exactly like `rect.width` was
    // for columns, so GTK's real requirement —
    // `rect.height / ROW_PITCH_PX`, at GTK's own 1-buffer-line-per-row
    // granularity — is only dimensionally meaningful when `rect.height` is
    // pixels. It is still safe to fold into the max unconditionally on the
    // TUI side too: TUI's own requirement is `rect.height *
    // MINIMAP_LINES_PER_ROW` there (`lh` is always `1.0` for TUI), which
    // exceeds `rect.height / ROW_PITCH_PX` for any positive `rect.height`
    // since `MINIMAP_LINES_PER_ROW` (4) is larger than `1.0 /
    // ROW_PITCH_PX` (0.5) — so this candidate can only ever win on GTK's
    // own numbers, never accidentally overriding TUI's.
    //
    // #1532: `resolved_minimap_scale` (quadraui#1143) resolves to
    // `MinimapScale::One` — whose `row_pitch_px()` is exactly the old
    // hardcoded `ROW_PITCH_PX` constant this replaced — everywhere except
    // a pixel (GUI) backend that has both `minimap_render_characters` on
    // and a `minimap_scale >= 2`; TUI always takes the `One` branch (see
    // that method's own doc comment), so every claim this comment block
    // makes about "GTK's real requirement" below is unaffected there.
    let minimap_scale = engine.settings.resolved_minimap_scale();
    let effective_row_pitch_px = minimap_scale.row_pitch_px();
    let gtk_row_capacity = (rect.height / effective_row_pitch_px).floor() as usize;
    let target_lines = display_rows
        .saturating_mul(MINIMAP_LINES_PER_ROW)
        .max(gtk_row_capacity)
        .max(1);

    let window = engine.windows.get(&window_id)?;
    let buffer_state = engine.buffer_manager.get(window.buffer_id)?;
    let rope = &buffer_state.buffer.content;
    let total_buffer_lines = rope.len_lines();
    if total_buffer_lines == 0 {
        return None;
    }

    // #1093: the strip holds a *fixed vertical scale* — one `lines` entry
    // is always worth exactly one buffer line — instead of the whole
    // buffer being squeezed end-to-end into the strip on every frame. The
    // old squeeze pinned the file's last line to the strip's bottom row at
    // *every* scroll position (so a bottom-of-strip click always jumped to
    // EOF); VS Code's `minimap.size: proportional` this issue asks for
    // instead shows a `target_lines`-line *window* onto the buffer that
    // slides as the editor scrolls, so both ends of the file are reachable
    // and a bottom click pages roughly one strip's worth of file.
    //
    // #1186: that fixed one-line-per-block window is #1093's *uncompressed*
    // (`K == 1`) case. `k` below grows the window to `k * target_lines`
    // lines — still capped, still O(target_lines) blocks — so
    // `quadraui::primitives::minimap::block_bounds` (just below) takes its
    // striding branch instead of its "never upscales" one and each block
    // becomes `k` real buffer lines wide, aggregated by the exact same
    // `quadraui::sample_blocks` path #1098 lifted from #1085's own
    // `minimap_block_text`.
    //
    // #1211: `k` is a function of the strip's own geometry
    // (`editor_visible_rows`, `target_lines`) — **never** of
    // `total_buffer_lines`. The pre-#1211 `k` here was
    // `total_buffer_lines.div_ceil(target_lines)`, chosen to fit the *whole
    // buffer* into one window — which squeezed the entire file into the
    // strip for every file shorter than `MINIMAP_MAX_COMPRESSION *
    // target_lines` (essentially every real file), pinning `max_start` to
    // `0` below and permanently disabling #1093's slide. See
    // `MINIMAP_VIEWPORT_MULTIPLE`'s own doc comment for why this
    // `desired_window_lines` formula reproduces VS Code's default
    // (`minimap.size: "proportional"`) fixed-scale behaviour on GTK
    // (`k == 1`) while still giving the TUI's coarser `target_lines` a small,
    // constant compression factor (`k == 3`) instead of one that grows
    // without bound as the file gets longer.
    let desired_window_lines = editor_visible_rows
        .max(1)
        .saturating_mul(MINIMAP_VIEWPORT_MULTIPLE);
    // #1532: in character-render mode (`minimap_scale` resolved to
    // `MinimapScale::Two`, above) every block must stay exactly one real
    // buffer line wide — a `k > 1` block would hand `sample_blocks` more
    // than one line to aggregate, and it has no font to render a
    // multi-line aggregate as real glyphs with, so it falls back to a
    // dither-text placeholder (`'x'`-filled) that paints as a uniform
    // block under the glyph atlas exactly like the density-dot path this
    // issue exists to move away from. Forcing `k == 1` here — instead of
    // letting the `div_ceil` below pick whatever compression the strip's
    // geometry would otherwise ask for — makes `window_len ==
    // target_lines` (below), so `block_bounds` always takes its
    // "never upscales" branch and every sampled row is a real, single
    // buffer line's own text. `target_lines` itself already shrank to
    // match (`effective_row_pitch_px` above), so the window still slides
    // (`window_start_line` below) — it just now covers however many
    // viewports the strip's real height affords at the taller
    // VS-Code-parity row pitch, rather than a fixed 9.
    let k = if minimap_scale != quadraui::MinimapScale::One {
        1
    } else {
        desired_window_lines
            .div_ceil(target_lines.max(1))
            .clamp(1, MINIMAP_MAX_COMPRESSION)
    };
    // #1247 (quadraui#1044): the window's *sizing* (`window_len`, via `k`
    // above) stays host-side — it depends on this strip's own geometry
    // (`target_lines`/`editor_visible_rows`), which quadraui has no way to
    // know — but where the window *starts* is now quadraui's own decision,
    // via `window_start_line`. Before #1044 that primitive didn't exist, so
    // this function pre-sliced the buffer down to a `window_len`-sized
    // range by hand and handed quadraui only the already-sliced result;
    // quadraui's own post-sample slide (`Minimap::layout_with_sizing`'s
    // `FixedPitch` arm, `slide_window_start_row`) never saw more `lines`
    // than the strip could already hold, so it always took its "already
    // fits" branch and returned `0` — permanently defeated, as that
    // function's own doc comment used to note. `window_start_line` is the
    // pre-sample counterpart #1044 added specifically to close that gap:
    // calling it here, instead of re-deriving the same fraction-of-buffer
    // arithmetic host-side, makes the slide live rather than defeated,
    // while still costing O(window_len) per frame, not O(file) (the
    // #728/#1085 regression this function exists to avoid — `window_len`
    // is still capped below `total_buffer_lines` before this call runs).
    let window_len = target_lines.saturating_mul(k).min(total_buffer_lines);
    // `total_at_position` is `max_scroll_top + 1`, the number of distinct
    // scroll positions the editor itself can reach — not `total_buffer_lines`
    // — so the slide is anchored to the editor's own scroll *ceiling*
    // (`total_buffer_lines - editor_visible_rows`, the same arithmetic
    // `View::ensure_cursor_visible` clamps `scroll_top` against) exactly the
    // way #1093/#1211 always have: the window reaches its own bottom exactly
    // when the editor viewport reaches the real bottom of the file, not some
    // fraction short of it (which a `scroll_top / total_buffer_lines`
    // fraction would leave — the editor's own `scroll_top` never reaches
    // `total_buffer_lines - 1` except in a one-row viewport).
    // `window_start_line`'s own `total_at_position <= 1` branch returns `0`
    // here exactly when `max_scroll_top == 0`, so a one-screen file still
    // stays top-aligned with no slide, byte-for-byte the pre-#1093
    // behaviour.
    let max_scroll_top = total_buffer_lines.saturating_sub(editor_visible_rows.max(1));
    let scroll_top_for_window = window.view.scroll_top.min(max_scroll_top);
    let window_start_line = quadraui::window_start_line(
        total_buffer_lines,
        window_len,
        scroll_top_for_window,
        max_scroll_top + 1,
    );
    // Downstream code (the viewport-highlight band `build_minimap_data`
    // paints further below, and every consumer of `Minimap::visible_row_start`)
    // relies on `scroll_top` always landing inside
    // `[window_start_line, window_start_line + window_len)` once the window
    // has slid. That holds here only because `target_lines` (hence
    // `window_len`) is derived from `display_rows`/`gtk_row_capacity` —
    // both sized off the same pane height `editor_visible_rows` comes from
    // at the one production call site — so the window is always at least as
    // tall as the editor's own viewport. It is **not** a general contract of
    // this function: a future caller that decouples the strip's height from
    // the editor's own visible-row count (passing a `rect`/`line_height`
    // pair that yields a `target_lines` smaller than `editor_visible_rows`)
    // could shrink the window below the viewport and violate it. Several of
    // the unit tests below call `build_minimap_data` directly with
    // deliberately mismatched `rect`/`editor_visible_rows` pairs to probe
    // the windowing math in isolation — keep that mismatch out of the one
    // real call site.

    // #1085/#1098: partition the *window* into `target_lines`-many blocks
    // *before* fetching any line text (same #728 discipline the old
    // point-sampler followed — decide what's worth reading before reading
    // it), then aggregate each block's lines (capped, see
    // [`quadraui::primitives::minimap::BLOCK_LINE_SAMPLE_CAP`]) into one
    // representative row instead of keeping one line and discarding the
    // rest of the block outright. The block-partitioning and text/dither
    // aggregation arithmetic itself now lives in quadraui
    // (quadraui#1012); this loop only maps vimcode's own tree-sitter
    // highlights onto the same block indices quadraui's own
    // `sample_blocks` call (below) will read.
    //
    // #1096: build the syntax-span mapping in the same bounded pass over
    // sampled lines (`block_sample_indices`, at most
    // `BLOCK_LINE_SAMPLE_CAP` per block), instead of a second, separate
    // pass over the *entire* `buffer_state.highlights` vec — `highlights`
    // spans the whole buffer (`update_syntax` always parses in full; the
    // viewport-scoped `refresh_syntax_visible` has no callers), so a full
    // scan of it is itself O(buffer) even when every non-matching entry is
    // skipped in O(1) (confirmed by hand: an O(1)-per-skip `sampled_at:
    // HashMap<line, block>` early-out, checked once per entry via
    // `rope.byte_to_line(*start)`, still left the ratio guard failing —
    // `buffer_state.highlights.len()` itself grows with the buffer, so
    // *touching* every entry once already costs more the bigger the file
    // is, allocation or not).
    //
    // `buffer_state.highlights` is sorted by start byte
    // (`update_syntax_with_limit` sorts it), so for each sampled line this
    // binary-searches straight to the handful of spans that start on it —
    // O(log highlights.len()) to find them, not O(highlights.len()) to
    // filter them. The same sorted-highlights-plus-`partition_point` idiom
    // already narrows highlights to the viewport a few thousand lines up in
    // this file; this is that same trick applied per sampled line instead
    // of to one contiguous window, since the minimap's sampled lines are
    // scattered across the whole buffer rather than contiguous.
    // `bounds` is *window*-relative (`0..window_len`), matching
    // `block_sample_indices`'s own contract — remapped to real buffer line
    // numbers below (`+ window_start_line`), both when reading from `rope`
    // and when stamping `MinimapLine::line_idx`. At `K == 1` (`window_len
    // <= target_lines`) this takes the "never upscales" branch and every
    // block is exactly one line wide, same as pre-#1186; at `K > 1`
    // (#1186's compressed mode, `window_len == target_lines * k`) it takes
    // the striding branch instead and each block becomes `k` real buffer
    // lines wide — the exact multi-line aggregation `sample_blocks` was
    // already built for by #1085/#1012, now actually exercised by the one
    // production call site instead of only by tests that pass a
    // `total_lines` bigger than `target_lines` directly.
    let bounds = quadraui::primitives::minimap::block_bounds(window_len, target_lines);
    if bounds.len() < 2 {
        return None;
    }
    let mut raw_spans: Vec<quadraui::MinimapSpan> = Vec::new();
    for r in 0..bounds.len() - 1 {
        let indices: Vec<usize> = quadraui::primitives::minimap::block_sample_indices(
            bounds[r],
            bounds[r + 1],
            quadraui::primitives::minimap::BLOCK_LINE_SAMPLE_CAP,
        )
        .into_iter()
        .map(|i| i + window_start_line)
        .collect();
        for &i in &indices {
            let line_start = rope.line_to_byte(i);
            let line_end = if i + 1 < total_buffer_lines {
                rope.line_to_byte(i + 1)
            } else {
                rope.len_bytes()
            };
            let lo = buffer_state
                .highlights
                .partition_point(|h| h.0 < line_start);
            let hi = buffer_state.highlights.partition_point(|h| h.0 < line_end);
            if lo >= hi {
                continue;
            }
            let slice = rope.line(i);
            // Clamp to this line's own content, excluding its terminator —
            // a highlight's byte offsets are only meaningful against the
            // line they actually came from, and a span that runs past it
            // (or into the next line) must not paint columns as if they
            // belonged here.
            let content_byte_len = minimap_line_content_byte_len(slice);
            // quadraui's rasterisers treat span columns as *character*
            // columns (GTK converts them back to byte offsets for Pango
            // attributes), so convert here rather than handing over raw
            // byte deltas — locally, against this one line's own
            // `RopeSlice`, which costs only O(log line_length) and does not
            // grow with the buffer the way a whole-rope `byte_to_char`
            // would (measured across #1096's fix iterations: an earlier
            // version that binary-searched to the right lines but still
            // converted columns via `rope.byte_to_char` on the whole rope
            // left the large side of the ratio guard over 600ms/frame).
            // Capped at `MINIMAP_COL_SCAN_LIMIT` chars: columns past
            // `MINIMAP_MAX_RELEVANT_COLS` never affect `aggregate_spans`'s
            // output (it drops any cell at/past `grid.cols`), so counting
            // further into a long — e.g. minified — line is wasted (#728).
            let to_col = |b: usize| -> usize {
                let b = b.min(content_byte_len);
                slice.byte_to_char(b).min(MINIMAP_COL_SCAN_LIMIT)
            };
            for (start, end, scope) in &buffer_state.highlights[lo..hi] {
                if end <= start {
                    continue;
                }
                let start_col = to_col(start.saturating_sub(line_start));
                let end_col = to_col(end.saturating_sub(line_start));
                if end_col <= start_col {
                    continue;
                }
                let c = theme.scope_color(scope);
                raw_spans.push(quadraui::MinimapSpan {
                    line_idx: r,
                    start_col,
                    end_col,
                    color: quadraui::Color::rgb(c.r, c.g, c.b),
                });
            }
        }
    }
    // #1098: `sample_blocks` takes a **line accessor**, not a materialised
    // slice — it calls `line_at` only for the (at most
    // `BLOCK_LINE_SAMPLE_CAP`-per-block) lines its own read budget says it
    // needs, so this never allocates a `String` for the whole window up
    // front the way the pre-#1098 `owned: Vec<String>` /
    // `borrowed: Vec<&str>` pair did — that whole-window materialisation
    // was #1096's own root cause. `total_lines: window_len` against
    // `target_rows: target_lines` recomputes the exact same `bounds` this
    // function already derived above (pure arithmetic, no rope access), so
    // `line_at` only ever gets called for the same window-relative indices
    // `block_sample_indices` picked for the highlight loop, shifted by
    // `window_start_line` here rather than there since `sample_blocks`
    // itself is window-relative.
    let mut lines = quadraui::sample_blocks(window_len, target_lines, |i| {
        minimap_line_text(rope, i + window_start_line)
    });
    if lines.is_empty() {
        return None;
    }
    // `sample_blocks` stamps each `MinimapLine::line_idx` with its own
    // block's *window*-relative start line (`bounds[r]`, the same `bounds`
    // this function computed above) — shift to a real buffer line number.
    for line in lines.iter_mut() {
        line.line_idx += window_start_line;
    }
    // #1175 (quadraui#1032): colour aggregation used to happen right here,
    // against a `MinimapGrid` built from a hardcoded `MINIMAP_COLS_PER_CELL`.
    // That stopped being safe the moment quadraui#1032 made TUI's default
    // `cols_per_cell` adaptive to the buffer's own widest sampled line
    // (GTK's stays a fixed `1`) — this function has no backend to ask, so
    // it cannot know which scale will actually be painted. Aggregation is
    // deferred to `draw_minimap_strip`, which *does* have a live
    // `&dyn quadraui::Backend` and reads `cols_per_cell` back from
    // `Backend::minimap_layout` immediately before painting (see that
    // function's doc comment). `raw_spans` survives to that point
    // unaggregated, in real (not cell) character columns.
    let raw_syntax_spans = raw_spans;

    // Where the editor's viewport lands inside `lines`. Uses the editor
    // pane's own visible row count (`editor_visible_rows`), not
    // `display_rows` (the minimap *strip's* sampling budget, above) — see
    // this function's doc comment (#1085).
    let scroll_top = window.view.scroll_top.min(total_buffer_lines);
    let viewport_end = scroll_top.saturating_add(editor_visible_rows.max(1));
    // #1186: each `lines` entry is a block that can now span several real
    // buffer lines (`K > 1`), so `scroll_top`/`viewport_end` will usually
    // fall *inside* a block's line range rather than land exactly on a
    // `line_idx` boundary. The pre-#1186 formula
    // (`position(|l| l.line_idx >= scroll_top)`) is only correct when every
    // block is exactly one line wide — otherwise it skips the block that
    // actually *contains* `scroll_top` and finds the next one instead.
    // `partition_point` finds the last block whose start is `<= scroll_top`
    // (i.e. the block `scroll_top` itself falls inside): at `K == 1` a block
    // with `line_idx == scroll_top` is always present, so the two formulas
    // agree exactly — this is a strict generalisation, not a behaviour
    // change, for the uncompressed case.
    let visible_row_start = lines
        .partition_point(|l| l.line_idx <= scroll_top)
        .saturating_sub(1);
    let visible_row_end = lines.partition_point(|l| l.line_idx < viewport_end);

    Some(RenderedMinimap {
        window_id,
        rect,
        minimap: quadraui::Minimap {
            id: quadraui::WidgetId::new(format!("minimap:{}", window_id.0)),
            lines,
            // Filled in by `draw_minimap_strip` once it knows the real,
            // backend-resolved `cols_per_cell` to aggregate at (#1175).
            syntax_spans: Vec::new(),
            visible_row_start,
            visible_row_count: visible_row_end.saturating_sub(visible_row_start).max(1),
            total_buffer_lines,
        },
        raw_syntax_spans,
        resolved_layout: std::cell::RefCell::new(None),
        minimap_scale,
    })
}

/// Paint every entry in `screen.minimap` (#722 — one per editor pane, not
/// just the active one) through the backend's own rasteriser and stash each
/// strip's resolved layout on its own `RenderedMinimap::resolved_layout`
/// (#1253) for [`minimap_click_line`]/[`minimap_press`] to read back at click
/// time instead of re-deriving via `layout_with_sizing`. No return value:
/// before #1253 this returned `Vec<quadraui::MinimapLayout>` in `screen.minimap`
/// order for the caller to zip back up itself, but every call site (both
/// backends' `render_content`) discarded it rather than doing that — the
/// per-strip cache is both the fix and a simpler contract, since a caller
/// can no longer get the pairing wrong.
///
/// This is the *entire* backend-side contract for the minimap: GTK's font
/// scaling and TUI's braille packing are quadraui's implementations of
/// `Backend::draw_minimap`, so each backend's wiring is a single call to this
/// function. Nothing about sampling, dot packing or the *mechanics* of
/// colour aggregation exists on either side of it in vimcode — only the
/// *timing* of the one `quadraui::aggregate_spans` call does (#1175).
///
/// # Colour aggregation happens here, not in `build_minimap_data` (#1175)
///
/// quadraui#1032 made TUI's default `cols_per_cell` adapt to the buffer's
/// own widest sampled line (GTK's stays a fixed `1`) instead of a constant
/// either backend could hardcode. `build_minimap_data` runs at
/// screen-layout time, before any backend-specific call, so it cannot know
/// which scale is about to be painted — it hands `RenderedMinimap` an
/// un-aggregated `raw_syntax_spans` (real character columns) alongside a
/// `Minimap` whose `syntax_spans` starts empty. This function *does* have
/// a live backend, so for each strip it:
///
/// 1. Asks `Backend::minimap_layout` for the `cols_per_cell` that backend
///    will actually paint with — a no-paint call, since `syntax_spans` is
///    still empty at this point and `minimap_layout` only reads `.lines`
///    (mirrors `examples/common/minimap_app.rs`'s own read-back pattern
///    upstream, which is what closed quadraui#1032's own colour/dot
///    desync risk on quadraui's side of this fix).
/// 2. Builds a [`quadraui::MinimapGrid`] from that *same* value — so the
///    grid a cell's colour is aggregated at can never drift from the scale
///    its dots are painted at, the exact desync #1000's review flagged and
///    quadraui#1032 reopened by making the scale adaptive.
/// 3. Aggregates `raw_syntax_spans` into that grid and paints.
///
/// #723: the strip carries its own scroll affordance — `Minimap::layout`
/// resolves a `viewport_highlight` band that *both* quadraui rasterisers
/// already paint (a background accent across the visible rows on TUI, a
/// translucent slider on GTK). `MinimapLayout.scrollbar` is deliberately
/// **not** painted on top of it: TUI's `draw_editor` already paints a solid
/// one-column vertical scrollbar in the column immediately left of the
/// strip, so an extra solid bar in the strip's own first column reads as
/// two bars jammed together — the exact regression
/// `test_tui_two_groups_single_boundary_scrollbar_481` guards. The pane's
/// scrollbar instead stays *beside* the strip on both backends; GTK's
/// native widget is inset past the strip by `native_scrollbar_margin_start`
/// in `src/gtk/mod.rs`, which reads the strip width from
/// [`minimap_reserved_width`] — the same call that reserved it here.
pub fn draw_minimap_strip(backend: &mut dyn quadraui::Backend, screen: &ScreenLayout) {
    for mm in &screen.minimap {
        // #1532: keep the live backend's `MinimapScale` in sync with the
        // one `build_minimap_data` already sized this strip's `lines`/
        // `window_len` against (`mm.minimap_scale`) — a backend that never
        // overrides `Backend::minimap_scale`/`set_minimap_scale` (TUI) no-ops
        // here, per that trait method's own doc comment, so this stays
        // exactly as safe to call unconditionally as every other
        // `Backend::draw_*` call in this file.
        backend.set_minimap_scale(mm.minimap_scale);
        let rect = minimap_strip_rect(mm);
        // No-paint probe: `mm.minimap.syntax_spans` is still empty
        // here, which is fine — `minimap_layout` (both backends' own
        // `Backend` impls) only ever reads `.lines` to resolve its
        // scale.
        let cols_per_cell = backend
            .minimap_layout(rect, &mm.minimap)
            .cols_per_cell
            .max(1);
        let grid = quadraui::MinimapGrid {
            rows: mm
                .minimap
                .lines
                .len()
                .div_ceil(MINIMAP_LINES_PER_ROW)
                .max(1),
            cols: minimap_grid_cols(mm.rect.width, cols_per_cell),
            lines_per_row: MINIMAP_LINES_PER_ROW,
            cols_per_cell,
        };
        let mut minimap = mm.minimap.clone();
        minimap.syntax_spans = quadraui::aggregate_spans(&mm.raw_syntax_spans, grid);
        let layout = backend.draw_minimap(rect, &minimap).layout;
        *mm.resolved_layout.borrow_mut() = Some(layout);
    }
}

/// Raw-column budget (`MinimapGrid::cols`) for a strip `cols_per_cell`
/// columns wide per cell — see [`draw_minimap_strip`]'s doc comment for
/// where `cols_per_cell` itself comes from (the backend's own resolved
/// scale, never a host constant, as of #1175/quadraui#1032).
///
/// `rect_width` cannot alone answer "how many raw columns does this strip
/// paint" for both backends, because it is in the *caller's own unit*
/// (`RenderedMinimap::rect`'s doc comment: cells for TUI, pixels for GTK)
/// and the two backends do not turn that unit into painted columns the
/// same way:
///
/// - TUI's braille cell packs `cols_per_cell` raw columns per painted
///   cell, so `rect_width` cells of TUI strip consult exactly
///   `rect_width * cols_per_cell` raw columns — `rect_width` really is a
///   column count here, and the formula is dimensionally exact.
/// - GTK's rasteriser (`quadraui::gtk::minimap::draw_minimap`) does *not*
///   scale its per-row paint walk with `rect_width`'s pixel value at all:
///   every row is capped at a fixed
///   `quadraui::primitives::minimap::COLUMN_CAPACITY` (120) character
///   columns regardless of how wide the strip is in pixels (see that
///   constant's doc comment upstream), and its own `cols_per_cell` is
///   always `1` (`MinimapLayout`'s `Default`, and what
///   `Minimap::layout_with_sizing` always sets), so `rect_width * 1` (a
///   pixel count, in the `0..=MINIMAP_TARGET_COLS` (0..=120) range
///   [`vs_code_minimap_width_px`] resolves to) is not dimensionally a
///   column count at all.
///
/// Rather than branch on backend identity here (Platform-Neutrality Rule
/// — this is shared code, not per-backend wiring), take the max of both
/// backends' real requirements: TUI's exact `rect_width * cols_per_cell`
/// and GTK's fixed `COLUMN_CAPACITY`. An overestimate only ever costs
/// unreachable aggregation work; an underestimate silently drops colour
/// data for columns a backend does paint (#990, and precisely the failure
/// mode #1175 exists to close for TUI's now-adaptive scale — a fixed
/// `COLUMN_CAPACITY` floor alone is not always enough once `cols_per_cell`
/// itself can widen past what a hardcoded floor anticipated).
fn minimap_grid_cols(rect_width: f64, cols_per_cell: usize) -> usize {
    let visible_span_cols = ((rect_width.round().max(1.0)) as usize * cols_per_cell)
        .max(quadraui::primitives::minimap::COLUMN_CAPACITY);
    visible_span_cols.div_ceil(cols_per_cell).max(1)
}

/// The strip a `RenderedMinimap` occupies, in quadraui coordinates.
/// Shared by the paint path and the click path so the two cannot drift.
pub fn minimap_strip_rect(mm: &RenderedMinimap) -> quadraui::Rect {
    quadraui::Rect::new(
        mm.rect.x as f32,
        mm.rect.y as f32,
        mm.rect.width as f32,
        mm.rect.height as f32,
    )
}

/// The `MinimapLayout` to hit-test `mm` against: [`draw_minimap_strip`]'s
/// backend-resolved paint-time layout if this frame has already painted this
/// strip (#1253, `mm.resolved_layout`), else re-derived via
/// `layout_with_sizing` exactly as every call site here did before #1253.
///
/// The fallback exists for callers that build a `ScreenLayout` without ever
/// calling `draw_minimap_strip` — most of this module's own unit tests,
/// which click-test geometry directly off `build_screen_layout`'s output —
/// so a missing paint can only ever cost the recompute this cache exists to
/// skip, never a dropped or misrouted click.
///
/// `FixedPitch(1.0)` is the fallback's sizing rather than
/// `MinimapSizing::Fill` (used to be #1093's choice) because it is safe for
/// *both* real rasterisers' own pitches: it never overestimates the strip's
/// real row pitch, so `rows_that_fit` can only come out larger than reality,
/// never smaller — and since `mm.minimap.lines` is already host-windowed to
/// fit the strip's real capacity (`build_minimap_data`), `rows_that_fit >=
/// lines.len() / lines_per_row` holds regardless, so `layout_with_sizing`
/// never re-slides on top of the host-side window already computed. This is
/// also why swapping in the *real* paint-time layout above never disagrees
/// with the fallback it replaces: TUI's own rasteriser already resolves at
/// this exact `FixedPitch(1.0)` (`quadraui::tui::minimap::tui_minimap_layout`),
/// and GTK's real `ROW_PITCH_PX` pitch only ever narrows `rows_shown` below
/// `row_count`, which the host-side windowing above already keeps from
/// happening in the first place.
///
/// CLAUDE.md's black-box test exemption, invoked explicitly: TUI ships no
/// new/updated `TuiDriver` test for this change because there is no
/// TUI-visible behaviour to cover — TUI's resolved paint-time layout and the
/// pre-#1253 `FixedPitch(1.0)` fallback it now caches are mathematically
/// identical (previous paragraph), and TUI's `shell_app.rs`/`render_impl.rs`
/// call sites already discarded `draw_minimap_strip`'s old `Vec` return
/// value, so no TUI wiring changed either (`scripts/prod_lines.py
/// src/tui_main` delta: 0). This is an internal refactor on the TUI side.
fn minimap_layout_for_click(mm: &RenderedMinimap) -> quadraui::MinimapLayout {
    if let Some(layout) = mm.resolved_layout.borrow().as_ref() {
        return layout.clone();
    }
    mm.minimap.layout_with_sizing(
        minimap_strip_rect(mm),
        MINIMAP_LINES_PER_ROW,
        quadraui::MinimapSizing::FixedPitch(1.0),
    )
}

/// Resolve a click/drag at `(x, y)` (backend units, same space as
/// [`minimap_strip_rect`]) against every pane's minimap track (#722),
/// returning the window it hit plus the buffer line to scroll that window
/// to. `None` when no pane has a minimap or the point misses all of them.
///
/// Panes never overlap, so at most one strip can claim a given point — the
/// loop stops at the first hit rather than needing to pick a "closest" one.
///
/// Needs no backend instance: `MinimapLayout::hit_test` resolves purely from
/// `bounds`, which is the same strip rect both rasterisers were handed —
/// `lines_per_row` only shapes the *painted* rows, never the hit fraction. So
/// "click the vertical middle → ~50% of the *painted window*" is one
/// behaviour computed once, not two implementations that can drift.
///
/// #1093: `sizing` used to be `MinimapSizing::Fill`, which (like every other
/// sizing variant) never actually changed `hit_test`'s fraction — `hit_test`
/// only ever reads `layout.bounds`, which is the `bounds` argument handed back
/// verbatim, never reshaped by the sizing branch. What sizing *does* change is
/// `layout.visible_lines`, which [`minimap_fraction_to_line`] now consults (the
/// `start_line_idx` bridge its own doc describes) to resolve the fraction
/// against the rows actually on screen rather than against
/// `total_buffer_lines` — so this now has to agree with whichever pitch the
/// real rasteriser painted with, or the two would resolve different windows.
/// See [`minimap_layout_for_click`]'s doc comment (#1253) for why the paint-time
/// layout it prefers and the `FixedPitch(1.0)` fallback it falls back to always
/// agree here.
pub fn minimap_click_line(screen: &ScreenLayout, x: f64, y: f64) -> Option<(WindowId, usize)> {
    for mm in &screen.minimap {
        let layout = minimap_layout_for_click(mm);
        if let quadraui::MinimapHit::Seek { fraction } = layout.hit_test(x as f32, y as f32) {
            return Some((
                mm.window_id,
                minimap_fraction_to_line(fraction, &layout, &mm.minimap),
            ));
        }
    }
    None
}

/// Apply a minimap click/drag: focus the *hit* pane, scroll it so the
/// clicked line sits at the *centre* of the viewport (VS Code parity), and
/// carry the cursor with it, so the next `ensure_cursor_visible` doesn't snap
/// the view straight back. Works against whichever pane's strip the point
/// landed on (#722), not just the active window.
///
/// The focus switch (`focus_group_for_window` + `set_cursor_for_window`,
/// the same pair `Engine::mouse_click` uses for a plain buffer click) is a
/// #722 follow-up: before every pane had its own minimap, only the active
/// pane's strip was ever clickable, so "click a strip without focusing its
/// pane" could never arise. Now that a background pane's strip is
/// reachable too, scrolling it without also bringing it to the front would
/// read as broken — a click that visibly moves a pane's view but leaves
/// focus (and keyboard input) somewhere else.
///
/// #1093: centres rather than top-aligns — `set_scroll_top_for_window` used
/// to be handed `line` directly, which put the clicked line at the very top
/// of the viewport. Combined with the strip no longer spanning the whole
/// file, top-aligning a bottom-of-strip click made the viewport's *bottom*
/// land a further `viewport_lines` past the clicked point, reading as an
/// overshoot/page-down rather than "scroll to roughly here" — VS Code
/// centres the viewport on the clicked point instead.
///
/// Returns the window scrolled and the buffer line scrolled to (the raw
/// clicked line — the cursor's own target — not the centred `scroll_top`),
/// or `None` when the point missed every strip (in which case the caller
/// must fall through to its normal editor click handling).
pub fn apply_minimap_click(
    engine: &mut Engine,
    screen: &ScreenLayout,
    x: f64,
    y: f64,
) -> Option<(WindowId, usize)> {
    let (window_id, line) = minimap_click_line(screen, x, y)?;
    engine.focus_group_for_window(window_id);
    let half_viewport = engine
        .windows
        .get(&window_id)
        .map(|w| w.view.viewport_lines / 2)
        .unwrap_or(0);
    engine.set_scroll_top_for_window(window_id, line.saturating_sub(half_viewport));
    engine.set_cursor_for_window(window_id, line, 0);
    Some((window_id, line))
}

/// `quadraui::WidgetId` for a minimap strip's own thumb drag (#1187) —
/// `minimap:<window_id>`, parsed back out by [`apply_scroll_offset`].
/// Shared by both backends' press rungs so the id can never drift from what
/// the apply-side table matches on.
pub fn minimap_drag_widget(window_id: WindowId) -> quadraui::WidgetId {
    quadraui::WidgetId::new(format!("minimap:{}", window_id.0))
}

/// Outcome of resolving a **press** (not a continued drag-move) against a
/// minimap strip (#1187) — mirrors [`EditorScrollbarClick`], but for the
/// strip's own viewport-highlight band rather than a `quadraui::Scrollbar`
/// thumb (#723's own scrollbar is deliberately never painted over the band —
/// see [`draw_minimap_strip`]'s doc comment).
///
/// Every press that hits the strip at all begins a
/// `quadraui::DragTarget::ScrollbarY` drag — there is no `PageTo`-style
/// track-page outcome here, unlike [`resolve_editor_scrollbar_click`] — a
/// press outside the band must still jump-to-position (#1093 centring),
/// which the caller performs by calling [`apply_minimap_click`] before
/// arming the drag whenever [`Self::jump`] is set, so a plain click's
/// existing behaviour is unchanged: only what happens on the *next* move
/// differs.
///
/// This is the fix for #1187: previously both backends re-ran
/// `apply_minimap_click` (an absolute seek against the strip's own,
/// scroll-following painted window) on every drag-move sample, which mostly
/// cancelled itself out — the window re-slid the same direction the click
/// just scrolled. Arming a real `DragTarget::ScrollbarY` against
/// `max_scroll` (the whole file's scroll ceiling, [`Self::max_scroll`]) once
/// on press, instead of re-seeking every move, makes a whole-strip drag
/// traverse the whole file exactly like the real vertical scrollbar does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimapPress {
    /// The pane whose strip was hit.
    pub window_id: WindowId,
    /// Track top, in the same native units as [`minimap_strip_rect`].
    pub track_start: f32,
    /// Track length (the strip's full height).
    pub track_length: f32,
    /// The painted viewport-highlight band's own height — the "thumb"
    /// [`quadraui::DragTarget::ScrollbarY`] drags, whether or not the press
    /// itself landed inside it.
    pub thumb_length: f32,
    /// The whole file's scroll ceiling: `total_buffer_lines -
    /// viewport_lines`, the same arithmetic `View::ensure_cursor_visible`
    /// clamps `scroll_top` against — never the strip's own (possibly much
    /// smaller) painted window.
    pub max_scroll: usize,
    /// Offset from the band's own top edge, preserved so continued drag
    /// moves don't jump the band out from under the cursor. `0.0` when
    /// [`Self::jump`] is set (the press landed on the track outside the
    /// band), matching [`resolve_editor_scrollbar_click`]'s track-click
    /// convention — the band's new top after the jump lands under the
    /// cursor by construction.
    pub grab_offset: f32,
    /// The press landed on the track *outside* the viewport-highlight band:
    /// the caller must call [`apply_minimap_click`] first (today's #1093
    /// jump-to-position + cursor move) before arming the drag described by
    /// the rest of this struct.
    pub jump: bool,
}

/// Resolve a press against every pane's minimap strip (#722), returning the
/// geometry needed to arm a [`quadraui::DragTarget::ScrollbarY`] drag. `None`
/// when no pane has a minimap or the point misses all of them — mirrors
/// [`minimap_click_line`]'s "first hit wins" contract (panes never overlap).
///
/// Read-only (`&Engine`, not `&mut`): unlike [`apply_minimap_click`], this
/// never mutates scroll/cursor state itself — see [`MinimapPress::jump`] for
/// why the caller still might need to call that function too.
///
/// `fine` is #1271's minimap-scale seek, **the default since #1828**: when
/// set, the geometry armed for the subsequent `ScrollbarY` drag is remapped
/// from the whole file (`max_scroll` stays file-wide — #1187's own
/// guarantee, unchanged) onto the strip's *currently painted window*
/// instead, via a **virtual track** fed to the same primitive — see
/// [`fine_seek_geometry`] for the derivation. This is the mapping VS Code
/// itself uses: the viewport slider moves within the minimap's own
/// (possibly sliding, #1093) scale, never the whole file's. `false`
/// reproduces the pre-#1828 (file-wide, #1187) geometry — a drag that
/// traverses virtually the whole file in one gesture, like a real
/// scrollbar handle. `click::pixel_to_click_target`'s call site passes
/// `!alt`, so holding **Alt** is now what reaches *this* geometry (the old
/// #1187 default), kept as a power-user "fast scroll" affordance rather
/// than dropped outright (#1828).
pub fn minimap_press(
    engine: &Engine,
    screen: &ScreenLayout,
    x: f64,
    y: f64,
    fine: bool,
) -> Option<MinimapPress> {
    for mm in &screen.minimap {
        let bounds = minimap_strip_rect(mm);
        // #1253: same paint-time-layout-first, `FixedPitch(1.0)`-fallback
        // resolution `minimap_click_line` uses — see
        // `minimap_layout_for_click`'s doc comment.
        let layout = minimap_layout_for_click(mm);
        if matches!(
            layout.hit_test(x as f32, y as f32),
            quadraui::MinimapHit::None
        ) {
            continue;
        }
        let band = layout.viewport_highlight;
        let py = y as f32;
        let in_band = band.height > 0.0 && py >= band.y && py < band.y + band.height;
        let viewport_lines = engine
            .windows
            .get(&mm.window_id)
            .map(|w| w.view.viewport_lines)
            .unwrap_or(0);
        let max_scroll = mm.minimap.total_buffer_lines.saturating_sub(viewport_lines);
        let scroll_top = engine
            .windows
            .get(&mm.window_id)
            .map(|w| w.view.scroll_top)
            .unwrap_or(0);

        let (track_start, track_length, thumb_length, grab_offset) = fine
            .then(|| {
                fine_seek_geometry(
                    &bounds,
                    &mm.minimap,
                    max_scroll,
                    band.height,
                    scroll_top,
                    py,
                    in_band,
                )
            })
            .flatten()
            .unwrap_or((
                bounds.y,
                bounds.height,
                band.height,
                if in_band { py - band.y } else { 0.0 },
            ));

        return Some(MinimapPress {
            window_id: mm.window_id,
            track_start,
            track_length,
            thumb_length,
            max_scroll,
            grab_offset,
            jump: !in_band,
        });
    }
    None
}

/// Derive the **virtual track** #1271's fine seek arms in place of
/// the real, file-wide one (#1828: the default drag mapping; the old
/// file-wide one now needs Alt held) — remapping the same strip pixels onto the
/// strip's *currently painted window* (`~MINIMAP_LINES_PER_ROW` lines per
/// cell) instead of the whole file (`~max_scroll / track_length` lines per
/// cell), while still feeding `quadraui::dispatch_mouse_drag`'s unmodified
/// `ScrollbarY` arithmetic — no quadraui change needed.
///
/// Let `S0`/`Sh` be the real strip's top/height (`bounds`), `M` the file-wide
/// `max_scroll`, `base` the first buffer line the strip currently paints and
/// `span` its line extent (both read off `minimap.lines`, matching the
/// `window_len` convention `minimap_click_at_the_middle_seeks_to_the_middle_
/// of_the_painted_window` already uses — `lines.last().line_idx + 1 -
/// lines[0].line_idx`, robust to #1186 multi-line blocks). Then:
///
/// ```text
/// effective_track = Sh * M / span      // dispatch's own track_length - thumb_length
/// track_start     = S0 - base * Sh / span
/// track_length    = effective_track + thumb_length
/// ```
///
/// `thumb_length` is taken straight from the caller's own, already-painted
/// [`quadraui::MinimapLayout::viewport_highlight`] band height — **not**
/// re-derived here as a naive `Sh * viewport_lines / span` proportion
/// (#1828 review: that naive form omits `quadraui::fit_thumb`'s
/// `min_thumb_len` floor, so on a file small enough that the real band is
/// clamped to the floor — the exact regime
/// `minimap_drag_keeps_seeking_while_the_button_is_held`'s short fixture
/// exercises — the naive thumb came out *smaller* than what is actually
/// painted/hit-tested, throwing off `effective_track`'s complement
/// `track_length - thumb_length` dispatch itself recomputes and landing a
/// drag to the strip's middle at ~19% of the file instead of ~50%).
/// Whichever `thumb_length` is passed in cancels exactly out of dispatch's
/// own `track_length - thumb_length` (since `track_length` is *defined* as
/// `effective_track + thumb_length` above), so reusing the real band height
/// costs nothing and keeps this virtual track's thumb bound to the one
/// users actually see and click.
///
/// Check: at `y = S0`, dispatch's `rel = base/M` → offset `base`; at
/// `y = S0 + Sh`, `rel = (base+span)/M` → offset `base + span` — the virtual
/// strip spans exactly the painted window, at `M/span` times the real
/// strip's resolution. This holds regardless of `thumb_length`'s value, by
/// the cancellation above.
///
/// `grab_offset` is derived by requiring the mapping to be an **identity at
/// the press point** (dragging zero pixels must reproduce the current
/// `scroll_top`) rather than by hand-rolling band arithmetic against the
/// virtual track: `grab_offset = py - track_start - (scroll_top / M) *
/// effective_track`. This subsumes the non-fine convention (`py - band.y`),
/// which is the same identity solved against the *real* track/thumb instead.
///
/// Returns `None` (falling back to the real, file-wide geometry) when the
/// window has no line extent (`span == 0`, an empty minimap) or the file
/// already fits the viewport (`max_scroll == 0`, matching
/// `dispatch_mouse_drag`'s own `*max_scroll > 0` guard — a zero-`max_scroll`
/// drag never moves either way, so the geometry choice is moot).
pub(crate) fn fine_seek_geometry(
    bounds: &quadraui::Rect,
    minimap: &quadraui::Minimap,
    max_scroll: usize,
    thumb_length: f32,
    scroll_top: usize,
    py: f32,
    in_band: bool,
) -> Option<(f32, f32, f32, f32)> {
    let first = minimap.lines.first()?;
    let last = minimap.lines.last()?;
    let base = first.line_idx as f32;
    let span = (last.line_idx + 1).saturating_sub(first.line_idx) as f32;
    if span <= 0.0 || max_scroll == 0 {
        return None;
    }
    let m = max_scroll as f32;
    let s0 = bounds.y;
    let sh = bounds.height;

    let effective_track = sh * m / span;
    let track_start = s0 - base * sh / span;
    let track_length = effective_track + thumb_length;

    let grab_offset = if in_band {
        py - track_start - (scroll_top as f32 / m) * effective_track
    } else {
        0.0
    };

    Some((track_start, track_length, thumb_length, grab_offset))
}

/// Buffer line a minimap click at `fraction` of the track should scroll to,
/// resolved against the strip's actual **painted window** — not against
/// [`quadraui::Minimap::total_buffer_lines`] (#1093). Once the strip holds a
/// fixed-scale window rather than the whole file, "50% down the track" means
/// "the row halfway through what's currently painted", which for a file
/// taller than the strip is a real buffer line far short of 50% of the file.
///
/// `layout` must be the same backend/pitch-matched
/// [`quadraui::MinimapLayout`] `fraction` itself came from (see
/// [`minimap_click_line`]'s doc comment on why the sizing has to agree with
/// paint). `layout.visible_lines[row].start_line_idx` bridges a resolved row
/// back to its position in `minimap.lines`, whose own
/// [`quadraui::MinimapLine::line_idx`] is the real buffer line
/// `build_minimap_data` already stamped it with — so this never needs
/// `total_buffer_lines` at all.
pub fn minimap_fraction_to_line(
    fraction: f32,
    layout: &quadraui::MinimapLayout,
    minimap: &quadraui::Minimap,
) -> usize {
    let last_line_idx = || minimap.lines.last().map(|l| l.line_idx).unwrap_or(0);
    if minimap.lines.is_empty() {
        return 0;
    }
    let row_count = layout.visible_lines.len();
    if row_count == 0 {
        return minimap.lines[0].line_idx;
    }
    let row = ((fraction.clamp(0.0, 1.0) as f64) * row_count as f64) as usize;
    let row = row.min(row_count - 1);
    let start_line_idx = layout.visible_lines[row].start_line_idx;
    minimap
        .lines
        .get(start_line_idx)
        .map(|l| l.line_idx)
        .unwrap_or_else(last_line_idx)
}

/// Context menu data for TUI rendering.
#[derive(Debug, Clone)]
pub struct ContextMenuPanel {
    pub items: Vec<ContextMenuRenderItem>,
    pub selected_idx: usize,
    pub screen_col: u16,
    pub screen_row: u16,
    /// Trigger element height in line_height units (f32; supports
    /// sub-cell rows like GTK's 1.6× tab row). 0.0 = no trigger →
    /// render at click coords (AnchorPoint). Non-zero opts into
    /// `ContextMenuPlacement::Below` (#434).
    pub trigger_height: f32,
}

/// A single rendered context menu item.
#[derive(Debug, Clone)]
pub struct ContextMenuRenderItem {
    pub label: String,
    pub shortcut: String,
    pub separator_after: bool,
    pub enabled: bool,
}

/// Convert a render-side `ContextMenuPanel` into a `quadraui::ContextMenu`
/// for D6 rasterisation. `separator_after` on an item becomes a separator
/// row (`id: None`) inserted immediately after that item in the
/// quadraui items list. Item ids are synthesised as `context:N` where
/// N is the original engine-side item index.
///
/// `selected_idx` is translated from engine-index (0..panel.items.len())
/// to quadraui-index (which includes separator rows) so the selection
/// highlight lines up visually when separators appear before the
/// selected item.
pub fn context_menu_panel_to_quadraui_context_menu(
    panel: &ContextMenuPanel,
) -> quadraui::ContextMenu {
    let mut items: Vec<quadraui::ContextMenuItem> = Vec::new();
    // engine_to_quadraui[engine_idx] = quadraui index of the same item.
    let mut engine_to_quadraui: Vec<usize> = Vec::with_capacity(panel.items.len());
    for (i, item) in panel.items.iter().enumerate() {
        engine_to_quadraui.push(items.len());
        items.push(quadraui::ContextMenuItem {
            id: Some(quadraui::WidgetId::new(format!("context:{i}"))),
            label: quadraui::StyledText::plain(item.label.clone()),
            detail: if item.shortcut.is_empty() {
                None
            } else {
                Some(quadraui::StyledText::plain(item.shortcut.clone()))
            },
            disabled: !item.enabled,
            ..Default::default()
        });
        if item.separator_after {
            items.push(quadraui::ContextMenuItem::default());
        }
    }
    let selected_idx = engine_to_quadraui
        .get(panel.selected_idx)
        .copied()
        .unwrap_or(0);
    let placement = if panel.trigger_height > 0.0 {
        quadraui::ContextMenuPlacement::Below
    } else {
        quadraui::ContextMenuPlacement::AnchorPoint
    };
    quadraui::ContextMenu {
        id: quadraui::WidgetId::new("context_menu"),
        items,
        selected_idx,
        bg: None,
        placement,
    }
}

/// The view-local point a `ContextMenuPanel`'s trigger anchors to, in
/// `char_width`/`line_height` units converted to pixels/cells.
///
/// Shared by [`context_menu_generic_layout`] (the in-window path, which
/// turns it into a zero-width `Rect` for `ContextMenu::layout_at`) and
/// [`paint_context_menu_rung`]'s native branch (#902, `Backend::
/// show_context_menu`'s `anchor: Point` parameter) — both must place the
/// popup at the same spot the trigger (right-click point / menu row) was
/// at, so this is computed once from `panel.screen_col`/`screen_row`
/// rather than copied into two call sites that could drift.
pub(crate) fn context_menu_anchor_point(
    panel: &ContextMenuPanel,
    char_width: f64,
    line_height: f64,
) -> quadraui::Point {
    quadraui::Point::new(
        (panel.screen_col as f64 * char_width) as f32,
        (panel.screen_row as f64 * line_height) as f32,
    )
}

/// Compute a backend-agnostic [`quadraui::ContextMenuLayout`] for a
/// `ContextMenuPanel` from just `char_width`/`line_height`. Ports the
/// char-count width budget both GTK's (formerly dead-code-only)
/// `draw_context_menu_popup` and TUI's `draw_frame` independently
/// duplicated, now shared so both backends derive their click geometry
/// from the same formula (#546).
///
/// `border_chrome_inset` shrinks the computed width by `2 * inset` (in
/// `char_width` units) to account for a border the rasteriser draws
/// *outside* `layout.bounds` rather than inside it — TUI's ASCII box-drawing
/// border is external, so it passes `1.0` (matching its pre-#546 inline
/// `outer_width - 2.0`); GTK draws its border/padding inside the bounds via
/// the primitive itself, so it passes `0.0`.
pub fn context_menu_generic_layout(
    panel: &ContextMenuPanel,
    viewport: quadraui::Rect,
    char_width: f64,
    line_height: f64,
    border_chrome_inset: f64,
) -> (quadraui::ContextMenu, quadraui::ContextMenuLayout) {
    let menu = context_menu_panel_to_quadraui_context_menu(panel);

    let max_label = panel.items.iter().map(|i| i.label.len()).max().unwrap_or(4);
    let max_sc = panel
        .items
        .iter()
        .map(|i| i.shortcut.len())
        .max()
        .unwrap_or(0);
    let content_cols = (max_label + max_sc + 6).clamp(20, 50);
    let menu_w =
        (content_cols as f64 * char_width - 2.0 * border_chrome_inset * char_width).max(char_width);

    let anchor = context_menu_anchor_point(panel, char_width, line_height);
    let trigger_height_px = panel.trigger_height as f64 * line_height;
    let item_height = |_i: usize| quadraui::ContextMenuItemMeasure::new(line_height as f32);

    let layout = menu.layout_at(
        quadraui::Rect::new(anchor.x, anchor.y, 0.0, trigger_height_px as f32),
        viewport,
        menu_w as f32,
        item_height,
    );
    (menu, layout)
}

/// Convert the menu-bar dropdown state into a `quadraui::ContextMenu`.
/// Build a `quadraui::CommandCenter` descriptor from engine state.
pub fn build_command_center_view(
    nav_back_enabled: bool,
    nav_forward_enabled: bool,
    title: &str,
) -> quadraui::CommandCenter {
    let search_label = if title.is_empty() {
        String::new()
    } else {
        format!("\u{1f50d} {title}")
    };
    quadraui::CommandCenter {
        id: quadraui::WidgetId::new("command-center"),
        back_enabled: nav_back_enabled,
        forward_enabled: nav_forward_enabled,
        search_label,
    }
}

/// A modal dialog displayed over the editor.
#[derive(Debug, Clone)]
pub struct DialogPanel {
    pub title: String,
    pub body: Vec<String>,
    /// Each button is `(formatted_label, is_selected, is_cancel)`. `is_cancel`
    /// is derived from the engine-side `DialogButton::action == "cancel"`
    /// (#727) — it has no other visual effect in-canvas, but flows through
    /// [`dialog_panel_to_quadraui_dialog`] into `quadraui::DialogButton::is_cancel`
    /// so a native GTK `AlertDialog` knows which button Escape/close-box
    /// should activate. Accepted: dialogs with no `action == "cancel"`
    /// button at all (e.g. the file-changed-on-disk "Yes"/"No" prompt in
    /// `src/core/engine/buffers.rs`) get `is_cancel: false` on every
    /// button, so `GtkAlertDialog::set_cancel_button` is never called for
    /// them — GTK's own dismiss-without-choosing path still falls back to
    /// `None` → `Engine::dialog_cancel()`, matching in-canvas Escape, but
    /// these dialogs lose the native "Escape activates a specific labeled
    /// button" affordance that dialogs with an explicit Cancel button get.
    pub buttons: Vec<(String, bool, bool)>,
    /// Optional text input field (e.g. for SSH passphrase).
    pub input: Option<DialogInputPanel>,
    /// When true, buttons are rendered as a vertical list instead of a horizontal row.
    pub vertical_buttons: bool,
}

/// Convert a render-side `DialogPanel` into a `quadraui::Dialog` for
/// backend rasterisation via the D6 layout pipeline.
///
/// Button ids are synthesised from their index (`"dialog:btn:N"`)
/// since `DialogPanel.buttons` doesn't carry engine-side ids —
/// backends dispatch clicks by index via
/// `Engine::dialog_click_button(idx)`. The `is_selected` flag on each
/// button maps to `is_default` on the quadraui button, used by
/// backends to style the primary / focused button.
pub fn dialog_panel_to_quadraui_dialog(panel: &DialogPanel) -> quadraui::Dialog {
    let buttons: Vec<quadraui::DialogButton> = panel
        .buttons
        .iter()
        .enumerate()
        .map(
            |(i, (label, is_selected, is_cancel))| quadraui::DialogButton {
                id: quadraui::WidgetId::new(format!("dialog:btn:{i}")),
                label: label.clone(),
                is_default: *is_selected,
                is_cancel: *is_cancel,
                tint: None,
            },
        )
        .collect();
    quadraui::Dialog {
        id: quadraui::WidgetId::new("dialog"),
        title: quadraui::StyledText::plain(panel.title.clone()),
        // Body is multi-line — join with newlines. Backends split on
        // `\n` when rendering.
        body: panel
            .body
            .iter()
            .map(|l| quadraui::StyledText::plain(l.clone()))
            .collect(),
        buttons,
        severity: None,
        vertical_buttons: panel.vertical_buttons,
        table: None,
        input: panel.input.as_ref().map(|inp| {
            quadraui::DialogInput::TextInput(quadraui::DialogTextInput {
                value: inp.display.clone(),
                placeholder: String::new(),
                cursor: None,
            })
        }),
    }
}

/// Render data for a dialog text input field.
#[derive(Debug, Clone)]
pub struct DialogInputPanel {
    /// Display text (masked for passwords).
    pub display: String,
}

/// Compute a backend-agnostic [`quadraui::DialogLayout`] for a `DialogPanel`
/// from just `char_width`/`line_height` — no text-measurement backend access
/// required. Ports the char-cell approximation formula TUI's `draw_frame`
/// used inline (`char_width == line_height == 1.0` there, one screen cell),
/// scaled by real pixel metrics for GTK (#546).
///
/// Both backends call this (TUI at render time; GTK at render time, since
/// its ShellApp `render_content` only has generic `&mut dyn Backend`
/// metrics, not raw Pango) so a dialog's clickable geometry is always
/// derived from the exact same math the renderer used to paint it — no
/// more per-backend hand-rolled dialog sizing that can silently drift from
/// what was actually drawn (previously TUI recomputed a *second*,
/// independently-formulated copy of this at click time in `mouse.rs`; see
/// that call site for the follow-up that now reuses this `DialogLayout`
/// directly via `hit_test` instead).
pub fn dialog_generic_layout(
    panel: &DialogPanel,
    viewport: quadraui::Rect,
    char_width: f64,
    line_height: f64,
) -> (quadraui::Dialog, quadraui::DialogLayout) {
    let dialog = dialog_panel_to_quadraui_dialog(panel);

    let body_max = panel
        .body
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);
    let btn_max_label = panel
        .buttons
        .iter()
        .map(|(lbl, _, _)| lbl.chars().count() + 4)
        .max()
        .unwrap_or(0);
    let btn_row_len: usize = if panel.vertical_buttons {
        btn_max_label + 2
    } else {
        panel
            .buttons
            .iter()
            .map(|(lbl, _, _)| lbl.chars().count() + 4)
            .sum::<usize>()
            + 2
    };
    let content_width = body_max
        .max(panel.title.chars().count() + 4)
        .max(btn_row_len);
    let min_w = 40.0 * char_width;
    let max_w = (viewport.width as f64 - 4.0 * char_width).max(min_w);
    let width = ((content_width as f64 + 4.0) * char_width).clamp(min_w, max_w);

    let n_buttons = panel.buttons.len().max(1) as f64;
    let inner = width - 2.0 * char_width;
    let capped_btn_w = if panel.vertical_buttons {
        btn_max_label as f64 * char_width
    } else {
        (btn_max_label as f64 * char_width).min(inner / n_buttons)
    };

    let measure = quadraui::DialogMeasure {
        width: width as f32,
        title_height: line_height as f32,
        body_height: (panel.body.len() as f64 * line_height) as f32,
        input_height: if panel.input.is_some() {
            (2.0 * line_height) as f32
        } else {
            0.0
        },
        button_row_height: (if panel.vertical_buttons {
            panel.buttons.len() as f64
        } else {
            1.0
        } * line_height) as f32,
        button_width: capped_btn_w as f32,
        button_gap: 0.0,
        padding: line_height as f32,
        table_height: 0.0,
    };
    let layout = dialog.layout(viewport, measure, |_| {
        quadraui::ToolbarItemMeasure::new(0.0)
    });
    (dialog, layout)
}

// Re-export hit-test types and functions from engine so backends can use `render::*`.
// The find/replace types live in `quadraui::primitives::find_replace`
// after #271; engine re-exports keep the legacy paths working.
pub use crate::core::engine::{compute_find_replace_hit_regions, FR_PANEL_WIDTH};

/// The inline find/replace overlay displayed at the top-right of the
/// active editor group. Lifted to [`quadraui::FindReplacePanel`] in
/// #271; this alias preserves the legacy `render::FindReplacePanel`
/// path so existing call sites compile unchanged.
pub type FindReplacePanel = quadraui::FindReplacePanel;

/// Format a button label with the hotkey character bracketed.
/// e.g., `format_button_label("Recover", 'r')` → `"[R]ecover"`.
pub fn format_button_label(label: &str, hotkey: char) -> String {
    // '\0' means no hotkey — return label as-is.
    if hotkey == '\0' {
        return label.to_string();
    }
    let lower = hotkey.to_ascii_lowercase();
    let upper = hotkey.to_ascii_uppercase();
    // Find the first case-insensitive match of the hotkey in the label.
    if let Some(pos) = label.find(|c: char| c.to_ascii_lowercase() == lower) {
        let ch = label.as_bytes()[pos] as char;
        format!(
            "{}[{}]{}",
            &label[..pos],
            ch.to_ascii_uppercase(),
            &label[pos + ch.len_utf8()..]
        )
    } else {
        // Hotkey not found in label — prepend it.
        format!("[{}] {}", upper, label)
    }
}

/// A floating popup showing a diff hunk preview with revert/stage actions.
#[derive(Debug, Clone)]
pub struct DiffPeekPopup {
    /// Buffer line the popup is anchored to (0-indexed).
    pub anchor_line: usize,
    /// Raw diff hunk lines (with +/-/space prefix) to display.
    pub hunk_lines: Vec<String>,
}

/// Convert a `DiffPeekPopup` into a multi-line `quadraui::Tooltip`.
///
/// Each diff hunk line becomes one styled row inside `styled_lines`,
/// with per-prefix colouring: `+` lines use `theme.git_added`, `-`
/// lines use `theme.git_deleted`, context lines use `theme.hover_fg`.
/// A trailing action-bar row (`"[s] Stage  [r] Revert  [q] Close"`)
/// is appended in the default fg.
///
/// Layout: width sized to the longest line + padding, capped at 30
/// rows total (action bar included). Placement `Top` with fallback
/// `Bottom`. Anchor width set to popup width so the centering math
/// left-aligns with the cursor cell — matches the legacy popup.
/// `unit_w` / `unit_h` scale cell/row-derived sizes into the caller's
/// coordinate space — see [`hover_popup_to_quadraui_tooltip`]'s doc for why
/// this is enough to make the one adapter serve both backends (#669).
pub fn diff_peek_to_quadraui_tooltip(
    peek: &DiffPeekPopup,
    anchor_x: f32,
    anchor_y: f32,
    viewport: quadraui::Rect,
    theme: &Theme,
    unit_w: f32,
    unit_h: f32,
) -> (quadraui::Tooltip, quadraui::TooltipLayout) {
    let fg = theme.hover_fg;
    let added = theme.git_added;
    let deleted = theme.git_deleted;

    // Cap at 29 hunk rows so the action bar (1 row) fits inside the
    // legacy 30-line ceiling.
    let visible: Vec<&String> = peek.hunk_lines.iter().take(29).collect();
    let max_len = visible.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let action_text = "[s] Stage  [r] Revert  [q] Close";
    let max_len = max_len.max(action_text.chars().count());
    // +4 = 1 left border + 1 left pad + 1 right pad + 1 right border.
    let width = ((max_len + 4) as f32).max(20.0) * unit_w;

    let mut styled_lines: Vec<quadraui::StyledText> = Vec::with_capacity(visible.len() + 1);
    for hline in &visible {
        let line_fg = if hline.starts_with('+') {
            added
        } else if hline.starts_with('-') {
            deleted
        } else {
            fg
        };
        styled_lines.push(quadraui::StyledText {
            spans: vec![quadraui::StyledSpan::with_fg(hline.as_str(), line_fg)],
        });
    }
    // Action bar row in default fg.
    styled_lines.push(quadraui::StyledText {
        spans: vec![quadraui::StyledSpan::with_fg(action_text, fg)],
    });

    let height = styled_lines.len() as f32 * unit_h;

    let mut tooltip = quadraui_tooltip(quadraui::WidgetId::new("diff_peek"), String::new());
    tooltip.styled_lines = Some(styled_lines);
    // Legacy diff peek always rendered below the anchor line — mirror
    // that with placement=Bottom (with primitive fallback to Top when
    // there's no room below).
    tooltip.placement = quadraui::TooltipPlacement::Bottom;
    // anchor.width = popup width so the centering math left-aligns
    // the popup with the cursor cell (matches legacy + hover popup
    // + sig help adapters).
    let anchor = quadraui::Rect::new(anchor_x, anchor_y, width, unit_h);
    let measure = quadraui::TooltipMeasure::new(width, height);
    let layout = tooltip.layout(anchor, viewport, measure, 0.0);
    (tooltip, layout)
}

#[cfg(test)]
#[path = "minimap_tests.rs"]
pub(crate) mod tests;
