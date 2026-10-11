use super::*;

// ─── Minimap ──────────────────────────────────────────────────────────────────

/// Fraction of a pane's own width the minimap strip may reserve, as an
/// *upper bound* — **TUI-only** (TUI sizing is out of scope for
/// the pixel-based formula [`vs_code_minimap_width_px`] ports).
pub const MINIMAP_WIDTH_FRACTION: f64 = 0.15;

/// Target minimap width for **GTK/macOS/Win**, in raw pixels — VS Code's
/// `minimap.maxColumn` default (120 pixels, [`MINIMAP_TARGET_COLS`]),
/// which is the *cap* [`vs_code_minimap_width_px`]'s ported formula
/// clamps to, not the width itself: VS Code sizes the strip from the
/// pane's remaining width, the editor's real character width, and its
/// vertical scrollbar's width (see [`vs_code_minimap_width_px`]).
/// Despite the name, this constant is used only for GTK/macOS/Win.
/// See [`MINIMAP_TARGET_COLS_TUI`] for TUI's own, deliberately much
/// smaller, value.
pub(crate) const MINIMAP_TARGET_COLS: f64 = 120.0;

/// Target minimap width for TUI, in cell columns — **not**
/// `MINIMAP_TARGET_COLS` (120), which is meaningful only in GTK's pixel
/// unit. At 120 *px* it is VS Code's "precisely 1px per column" parity,
/// but 120 *columns* is wider than almost any real terminal, so in
/// `resolve_width`'s `target_cols.min(pane_width_cols * fraction)` the
/// `fraction` term would always win — the strip would scale with the
/// pane on every ordinary terminal width instead of holding steady.
/// Chosen so `target_cols.min(pane_width_cols * MINIMAP_WIDTH_FRACTION)`
/// resolves to this fixed value across ordinary terminal widths
/// (80..200 cols): at the narrow end (80 cols), `80 * 0.15 = 12`, so any
/// target at or below 12 makes the target win from 80 cols up. Below 80
/// cols the fraction (and eventually `MINIMAP_MIN_COLS`) still take over.
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
/// VS Code's *real* width formula (`EditorLayoutInfoComputer` in
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

/// VS Code's own minimap width formula, ported from
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
/// `remaining_width` is **already gutter-subtracted** by the caller
/// ([`minimap_reserved_width`]) — matching the formula's own
/// `remainingWidth = editor outer width - gutter (glyph margin + line
/// numbers + folding)`. `typical_char_width` is the *real* editor
/// font's cell width — unlike the linear-fraction formula this replaces,
/// this one is explicitly a function of it, so the minimap genuinely
/// narrows on a wider editor font, matching VS Code.
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
/// px), consulted only by [`vs_code_minimap_width_px`]'s ported formula.
/// This constant remains correct for the minimap formula regardless of
/// the scrollbar width actually painted.
const MINIMAP_VERTICAL_SCROLLBAR_WIDTH_PX: f64 = 14.0;

/// Buffer lines sampled per *display* row of minimap height.
/// This is the TUI's braille factor — the denser of the two backends.
/// For GTK (which paints one buffer line per row) it also lands the row
/// pitch at `line_height / 4`, i.e. the ~2–6 px band of visual height.
pub(crate) const MINIMAP_LINES_PER_ROW: usize = 4;

/// Ceiling on the compression factor `K` [`build_minimap_data`] applies.
/// `K` is derived from the strip's own geometry (see
/// [`MINIMAP_VIEWPORT_MULTIPLE`]), never from `total_buffer_lines`, so
/// `K` stays small and constant for every real caller and only a
/// pathological `rect`/`editor_visible_rows` pairing could push it this
/// high. Chosen empirically: once a block is wider than the sample cap,
/// spreading exactly that many samples across it regardless of width
/// means growing `K` past the point where blocks already exceed the cap
/// costs no extra line fetches — only a larger buffer span per block.
/// `pub(crate)` so tests can read the real ceiling.
pub(crate) const MINIMAP_MAX_COMPRESSION: usize = 64;

/// How many editor viewports [`build_minimap_data`]'s window covers at
/// its default, uncompressed scale (`K == 1`) — VS Code's own default
/// minimap: a fixed `BASE_CHAR_HEIGHT * scale` row pitch against a
/// `lineHeight` roughly 9x taller, independent of file length. `K` is
/// then chosen so the window's real line count —
/// `target_lines * K` — comes out to
/// `editor_visible_rows * MINIMAP_VIEWPORT_MULTIPLE`. On GTK,
/// `target_lines` already comes out to roughly `9 * editor_visible_rows`,
/// so `K` naturally falls out to `1`. On the TUI, `target_lines` is only
/// `4 * editor_visible_rows` (`MINIMAP_LINES_PER_ROW`), so `K` comes out
/// to `ceil(9 / 4) == 3` — the TUI's approximation of the same window.
const MINIMAP_VIEWPORT_MULTIPLE: usize = 9;

/// Ceiling on how many characters into a line `build_minimap_data`'s
/// `to_col` closure ever looks when measuring a highlight's column.
/// Past this many characters, `aggregate_spans` would discard the column
/// anyway, so counting further is wasted work on long lines. Survives
/// purely as this scan cap.
const MINIMAP_MAX_RELEVANT_COLS: usize = 400;

/// The `+ 1` keeps `to_col`'s boundary value itself exact rather than
/// off-by-one short.
const MINIMAP_COL_SCAN_LIMIT: usize = MINIMAP_MAX_RELEVANT_COLS + 1;

// Block partitioning, coverage aggregation and dither arithmetic
// live in quadraui (`quadraui::primitives::minimap`). `build_minimap_data`
// below calls the quadraui functions directly.

// How many times `minimap_line_text` has actually fetched a buffer
// line's text — a deterministic work counter. `pub(crate)` and
// `#[cfg(test)]`-only so tests can pin "line fetches per frame is
// bounded by strip geometry, not buffer length" without flake risk.
// Use `count_minimap_line_fetches` rather than touching this directly.
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

/// Read buffer line `i`'s text, trimming the trailing newline.
/// The highlight-mapping loop this function was split from no longer
/// calls this — see `minimap_line_content_byte_len` below, which answers
/// the loop's only question (a byte offset's character column within one
/// line) directly without materialising the line as a `String`. Every
/// call is now bounded by `sample_blocks`'s own read budget, which reads
/// at most [`quadraui::primitives::minimap::BLOCK_LINE_SAMPLE_CAP`] lines
/// per block — never buffer length.
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
/// terminator — the same trim `minimap_line_text` applies, but read
/// directly off the caller's own `RopeSlice` so it costs no allocation.
/// Used by `build_minimap_data`'s highlight loop to clamp a highlight
/// span's byte offset to the real line it was measured against.
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
    /// here: `cols_per_cell` (since quadraui#1032, adaptive to the buffer's
    /// widest sampled line) is not known until paint time. `draw_minimap_strip`
    /// aggregates `raw_syntax_spans` with the backend's real `cols_per_cell`
    /// and fills this field in immediately before painting.
    pub minimap: quadraui::Minimap,
    /// Syntax highlight spans in raw, un-aggregated form (real character
    /// columns, not cells) — [`draw_minimap_strip`] folds these into
    /// `minimap.syntax_spans` once it knows the real `cols_per_cell` to
    /// aggregate them at. Kept separate from `minimap` itself
    /// (rather than one `RenderedMinimap::raw_spans` living beside an
    /// eagerly-aggregated `syntax_spans`) so there is exactly one place
    /// aggregation ever happens, not two that could silently drift.
    pub raw_syntax_spans: Vec<quadraui::MinimapSpan>,
    /// Backend-resolved layout from this frame's [`draw_minimap_strip`] paint,
    /// if any — read back by [`minimap_click_line`] and [`minimap_press`]
    /// instead of re-deriving `layout_with_sizing`. `RefCell` because
    /// `draw_minimap_strip` only sees `&ScreenLayout` and still needs to fill
    /// this field in after paint. `None` until the first paint, and
    /// permanently `None` for any `ScreenLayout` built without calling
    /// `draw_minimap_strip`. Both click resolvers fall back to re-deriving
    /// via `layout_with_sizing` in that case, so a missing paint only costs
    /// the recompute, never a dropped or misrouted click.
    pub resolved_layout: std::cell::RefCell<Option<quadraui::MinimapLayout>>,
    /// The [`quadraui::MinimapScale`] this strip's `lines`/`window_len`
    /// were sized against — [`draw_minimap_strip`] pushes this onto the
    /// backend via `Backend::set_minimap_scale` immediately before painting,
    /// so the row pitch actually rasterised can never drift from the one
    /// this strip's sampling math assumed.
    pub minimap_scale: quadraui::MinimapScale,
}

/// Width the minimap reserves alongside the editor, in the caller's units.
/// `sizing` (supplied by the caller) never branches on backend identity —
/// only on which `sizing` *value* the caller handed it. GTK/macOS/Win's
/// `gtk_minimap_sizing`'s `target_cols` (120, [`MINIMAP_TARGET_COLS`])
/// routes GTK to [`vs_code_minimap_width_px`] with the *real* `char_width`.
/// TUI's `sizing` uses its own [`MINIMAP_TARGET_COLS_TUI`] value (12).
/// `gutter_width` is the pane's own line-number/fold gutter, subtracted
/// *only* on the [`vs_code_minimap_width_px`] path to match VS Code's
/// `remainingWidth = editor outer width - gutter`.
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

/// This window's own line-number/fold gutter width, in the caller's
/// pixel unit — the `remaining_width` term [`vs_code_minimap_width_px`]
/// needs. Computed from [`calculate_gutter_cols`], the same function
/// `build_rendered_window` uses, fed this window's buffer/settings state
/// directly, since this runs *before* `build_rendered_window`. Deliberately
/// omits some status overlays to avoid expensive checks at sizing time;
/// the *real* painted gutter can be at most one column wider than this
/// estimate, and only while such overlays are outstanding.
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

/// Width of the scroll-affordance gutter a pane's rightmost edge must
/// keep clear of the minimap strip, in the caller's own unit. TUI's
/// vertical scrollbar is an inline column always painted at `area.right - 1`
/// when the window overflows, exactly one `char_width` wide. GTK has an
/// overlay-chrome reserve. Taking the larger of the two generalises to
/// both backends without branching: on TUI this resolves to exactly one
/// cell; on GTK it resolves to `scrollbar_reserve` whenever that's the
/// wider value. `pub(crate)` since GTK driver tests need this formula.
pub(crate) fn scroll_gutter_width(scrollbar_reserve: f64, char_width: f64) -> f64 {
    scrollbar_reserve.max(char_width)
}

/// Build the quadraui `Minimap` for `window_id` over the strip `rect`.
/// Returns `None` when the setting is off, when the strip cannot hold a
/// single row, or when the window/buffer has gone away. All sampling and
/// colour reduction is quadraui's — this function only maps vimcode's
/// tree-sitter byte-offset highlights into quadraui's `MinimapSpan` type.
/// `editor_visible_rows` is the *editor pane's* own visible row count —
/// deliberately separate from `rect`/`line_height` (which size the strip)
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
    // GTK's minimap rasteriser paints each sampled line at a
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
    //: take the max of both backends' real row requirements rather
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
    // `resolved_minimap_scale` resolves to
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

    // The strip holds a *fixed vertical scale* — one `lines` entry is
    // always worth exactly one buffer line — instead of the whole buffer
    // being squeezed into the strip at every frame. The window is
    // `target_lines`-line *window* onto the buffer that slides as the
    // editor scrolls, so both ends of the file are reachable. That fixed
    // window is the *uncompressed* case (`K == 1`). `k` grows the window
    // to `k * target_lines` lines — still capped, still O(target_lines)
    // blocks — so each block becomes `k` real buffer lines wide,
    // aggregated by the exact same path #1098 lifted from #1085. `k` is
    // derived from the strip's own geometry, **never** from
    // `total_buffer_lines`.
    let desired_window_lines = editor_visible_rows
        .max(1)
        .saturating_mul(MINIMAP_VIEWPORT_MULTIPLE);
    // In character-render mode (`minimap_scale` resolved to
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
    // The window's *sizing* (`window_len`, via `k` above) stays host-side —
    // it depends on this strip's own geometry (`target_lines`/
    // `editor_visible_rows`). But where the window *starts* is now
    // quadraui's own decision, via `window_start_line`, making the slide
    // live rather than permanently defeated.
    let window_len = target_lines.saturating_mul(k).min(total_buffer_lines);
    // `total_at_position` is `max_scroll_top + 1`, the number of distinct
    // scroll positions the editor itself can reach. The window reaches its
    // bottom exactly when the editor viewport reaches the real bottom of
    // the file. `window_start_line`'s own `total_at_position <= 1` branch
    // returns `0` exactly when `max_scroll_top == 0`, so a one-screen file
    // stays top-aligned with no slide.
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

    // Partition the *window* into `target_lines`-many blocks *before*
    // fetching any line text (same #728 discipline), then aggregate each
    // block's lines into one representative row. The block-partitioning
    // and text/dither aggregation arithmetic live in quadraui; this loop
    // only maps vimcode's tree-sitter highlights onto the same block
    // indices quadraui's `sample_blocks` will read. Build the syntax-span
    // mapping in the same bounded pass over sampled lines instead of a
    // second pass over the *entire* `buffer_state.highlights` vec, which
    // would be O(buffer). `buffer_state.highlights` is sorted by start
    // byte, so for each sampled line this binary-searches to the handful
    // of spans that start on it. `bounds` is *window*-relative, remapped
    // to real buffer line numbers below.
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
            // columns, so convert from byte offsets locally, against this
            // line's own `RopeSlice`, which costs only O(log line_length).
            // Capped at `MINIMAP_COL_SCAN_LIMIT` chars: columns past
            // `MINIMAP_MAX_RELEVANT_COLS` are dropped by `aggregate_spans`,
            // so counting further into a long line is wasted work.
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
    // `sample_blocks` takes a **line accessor**, not a materialised slice —
    // it calls `line_at` only for the lines its own read budget needs, so
    // this never allocates a `String` for the whole window up front. `total_lines`
    // and `target_rows` recompute the same `bounds` already derived, so
    // `line_at` only gets called for the same window-relative indices
    // the highlight loop picked.
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
    // Colour aggregation used to happen right here. That stopped being
    // safe when quadraui#1032 made TUI's default `cols_per_cell` adaptive
    // to the buffer's widest sampled line (GTK's stays a fixed `1`).
    // `build_minimap_data` has no backend to ask, so it cannot know which
    // scale will be painted. Aggregation is deferred to `draw_minimap_strip`,
    // which does have a live backend and reads `cols_per_cell` before
    // painting. `raw_spans` survives unaggregated, in real character columns.
    let raw_syntax_spans = raw_spans;

    // Where the editor's viewport lands inside `lines`. Uses the editor
    // pane's own visible row count (`editor_visible_rows`), not
    // `display_rows` (the minimap *strip's* sampling budget, above) — see
    // this function's doc comment.
    let scroll_top = window.view.scroll_top.min(total_buffer_lines);
    let viewport_end = scroll_top.saturating_add(editor_visible_rows.max(1));
    // Each `lines` entry is a block that can span several real buffer
    // lines (`K > 1`), so `scroll_top`/`viewport_end` will usually fall
    // *inside* a block's line range rather than land exactly on a
    // `line_idx` boundary. `partition_point` finds the last block whose
    // start is `<= scroll_top` (the block `scroll_top` falls inside).
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
            // backend-resolved `cols_per_cell` to aggregate at.
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

/// Paint every entry in `screen.minimap` through the backend's own
/// rasteriser and stash each strip's resolved layout on its own
/// `RenderedMinimap::resolved_layout` for click handlers to read back
/// instead of re-deriving via `layout_with_sizing`. No return value.
/// This is the *entire* backend-side contract for the minimap: each
/// backend's wiring is a single call to this function. Nothing about
/// sampling, dot packing or colour aggregation exists in vimcode — only
/// the *timing* of the one `quadraui::aggregate_spans` call.
pub fn draw_minimap_strip(backend: &mut dyn quadraui::Backend, screen: &ScreenLayout) {
    for mm in &screen.minimap {
        // Keep the live backend's `MinimapScale` in sync with the
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
/// where `cols_per_cell` comes from. `rect_width` is in the *caller's own
/// unit* (cells for TUI, pixels for GTK) and the two backends turn that
/// unit into painted columns differently: TUI's braille packs
/// `cols_per_cell` raw columns per cell, so `rect_width * cols_per_cell`
/// is exact; GTK's rasteriser does *not* scale with pixel width, capping
/// at a fixed `quadraui::primitives::minimap::COLUMN_CAPACITY`. Rather
/// than branch on backend identity, take the max of both backends'
/// requirements: an overestimate only costs unreachable aggregation work.
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
/// backend-resolved paint-time layout if this frame already painted this
/// strip, else re-derived via `layout_with_sizing` as every call site did
/// before #1253. The fallback exists for callers that build a
/// `ScreenLayout` without calling `draw_minimap_strip` — most of this
/// module's unit tests — so a missing paint only costs the recompute,
/// never a misrouted click. The fallback uses `FixedPitch(1.0)` because
/// it is safe for *both* real rasterisers' pitches: it never overestimates
/// the strip's row pitch, so `rows_that_fit` only comes out larger than
/// reality, never smaller.
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

/// Resolve a click/drag at `(x, y)` against every pane's minimap track,
/// returning the window it hit plus the buffer line to scroll to. `None`
/// when no pane has a minimap or the point misses all of them. Panes
/// never overlap, so at most one strip can claim a point. Needs no
/// backend instance: `MinimapLayout::hit_test` resolves purely from
/// `bounds`, which is the same strip rect both rasterisers were handed.
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

/// Apply a minimap click: focus the *hit* pane and scroll it so the
/// clicked line sits at the *centre* of the viewport (VS Code parity),
/// carrying the cursor with it. Works against whichever pane's strip the
/// point landed on, not just the active window. The focus switch is
/// necessary because every pane now has its own minimap, so a background
/// pane's strip can be clicked without bringing it to the front.
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

/// `quadraui::WidgetId` for a minimap strip's own thumb drag —
/// `minimap:<window_id>`, parsed back out by [`apply_scroll_offset`].
/// Shared by both backends' press rungs so the id can never drift from what
/// the apply-side table matches on.
pub fn minimap_drag_widget(window_id: WindowId) -> quadraui::WidgetId {
    quadraui::WidgetId::new(format!("minimap:{}", window_id.0))
}

/// Outcome of resolving a **press** against a minimap strip — mirrors
/// [`EditorScrollbarClick`], but for the strip's viewport-highlight band
/// rather than a `quadraui::Scrollbar` thumb. Every press begins a
/// `quadraui::DragTarget::ScrollbarY` drag; a press outside the band
/// must still jump-to-position, which the caller performs by calling
/// [`apply_minimap_click`] before arming the drag whenever [`Self::jump`]
/// is set, so a plain click's behaviour is unchanged: only what happens
/// on the *next* move differs. This fix (née #1187) prevents each
/// drag-move sample from re-running the seek, which would mostly cancel
/// itself out; arming a real drag once instead makes a whole-strip drag
/// traverse the whole file like the real vertical scrollbar does.
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
    /// the caller must call [`apply_minimap_click`] first (jump-to-position
    /// + cursor move) before arming the drag.
    pub jump: bool,
}

/// Resolve a press against every pane's minimap strip, returning the
/// geometry needed to arm a [`quadraui::DragTarget::ScrollbarY`] drag.
/// `None` when no pane has a minimap or the point misses all of them.
/// Read-only (`&Engine`, not `&mut`): unlike [`apply_minimap_click`],
/// this never mutates scroll/cursor state. `fine` remaps the drag
/// geometry from the whole file onto the strip's *currently painted window*
/// instead (the default since #1828), making the viewport slider move
/// within the minimap's own scale like VS Code does. `false` reproduces
/// the pre-#1828 file-wide geometry; holding **Alt** reaches that mode.
pub fn minimap_press(
    engine: &Engine,
    screen: &ScreenLayout,
    x: f64,
    y: f64,
    fine: bool,
) -> Option<MinimapPress> {
    for mm in &screen.minimap {
        let bounds = minimap_strip_rect(mm);
        // Same paint-time-layout-first, `FixedPitch(1.0)`-fallback
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

/// Derive the **virtual track** #1271's fine seek arms in place of the
/// real, file-wide one — remapping the same strip pixels onto the strip's
/// *currently painted window* instead of the whole file, while still
/// feeding `quadraui::dispatch_mouse_drag`'s unmodified `ScrollbarY`
/// arithmetic. Let `S0`/`Sh` be the real strip's top/height, `M` the
/// file-wide `max_scroll`, `base` the first buffer line the strip
/// currently paints, and `span` its line extent. Then:
/// ```text
/// effective_track = Sh * M / span      // dispatch's own track_length - thumb_length
/// track_start     = S0 - base * Sh / span
/// track_length    = effective_track + thumb_length
/// ```
/// `thumb_length` is taken straight from the caller's own, already-painted
/// [`quadraui::MinimapLayout::viewport_highlight`] band height, not
/// re-derived here — reusing the real band height keeps this virtual
/// track's thumb bound to the one users actually see. `grab_offset` is
/// derived by requiring the mapping to be an **identity at the press
/// point** (dragging zero pixels must reproduce the current `scroll_top`)
/// rather than hand-rolling band arithmetic. Returns `None` when the
/// window has no line extent or the file already fits the viewport.
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
/// [`quadraui::Minimap::total_buffer_lines`]. Once the strip holds a
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
    /// `ContextMenuPlacement::Below`.
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
/// `char_width`/`line_height` units converted to pixels/cells. Shared by
/// the in-window and native paint paths so the two cannot drift.
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
/// char-count width budget both backends independently duplicated, now
/// shared so both derive their click geometry from the same formula.
/// `border_chrome_inset` shrinks the computed width by `2 * inset` (in
/// `char_width` units) to account for a border the rasteriser draws
/// *outside* `layout.bounds` rather than inside it — TUI passes `1.0`,
/// GTK passes `0.0`.
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
    /// — it has no other visual effect in-canvas, but flows through
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
/// required. Uses a char-cell approximation formula scaled by real pixel metrics.
///
/// Both backends call this at render time so a dialog's clickable geometry is
/// always derived from the exact same math the renderer used to paint it.
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
pub use crate::core::engine::{compute_find_replace_hit_regions, FR_PANEL_WIDTH};

/// The inline find/replace overlay displayed at the top-right of the
/// active editor group. This alias provides the legacy `render::FindReplacePanel`
/// path for existing call sites.
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
/// this is enough to make the one adapter serve both backends.
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
