use super::*;

// ─── Color ───────────────────────────────────────────────────────────────────
//
// #829 / quadraui#775: vimcode's own 24-bit `Color` type (plus its
// `from_hex` / `try_from_hex_over` / `lighten` / `darken`) was generic
// colour math with zero editor-specific meaning, so it was lifted verbatim
// into `quadraui::Color` upstream. We re-export that type directly instead
// of keeping a duplicate struct that every paint call had to convert
// to/from (the removed `to_q_color` / `to_quadraui_color` conversions).
//
// `quadraui::Color` adds an `a: u8` alpha channel vimcode's own type never
// had; every call site here only ever constructs opaque colours, so that
// channel is simply always 255 and invisible to existing callers.
pub use quadraui::Color;

/// Extension methods this file still needs on `Color` that don't belong
/// upstream: two are editor-specific derivations (`cursorline_tint`,
/// `colorcolumn_tint` — no meaning outside vimcode's cursor/colorcolumn
/// rendering), and the rest exist in `quadraui::Color` already but under a
/// different name/shape (`rgb` vs `from_rgb`) or not at all (`to_hex`,
/// `to_f32_rgba`) — cheaper to forward here than to rename every call site.
/// A free-standing `impl Color { .. }` isn't legal (orphan rule: `Color` is
/// now a foreign type), so this is a trait instead.
pub trait ColorExt: Sized {
    fn from_rgb(r: u8, g: u8, b: u8) -> Self;
    /// Derive a subtle cursorline background from this colour.
    /// Dark backgrounds get lightened; light backgrounds get darkened.
    fn cursorline_tint(self) -> Self;
    /// Derive a subtle colorcolumn background from this colour.
    /// Slightly less prominent than cursorline — a gentle column tint.
    fn colorcolumn_tint(self) -> Self;
    /// Normalise to `(f32, f32, f32, f32)` RGBA with full opacity.
    /// Used by Direct2D (`D2D1_COLOR_F`) and Core Graphics (`CGColor`).
    fn to_f32_rgba(self) -> (f32, f32, f32, f32);
    /// Format as a CSS `#rrggbb` hex string.
    fn to_hex(self) -> String;
}

impl ColorExt for Color {
    fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgb(r, g, b)
    }

    fn cursorline_tint(self) -> Self {
        let lum = 0.299 * self.r as f64 + 0.587 * self.g as f64 + 0.114 * self.b as f64;
        if lum < 128.0 {
            self.lighten(0.06)
        } else {
            self.darken(0.04)
        }
    }

    fn colorcolumn_tint(self) -> Self {
        let lum = 0.299 * self.r as f64 + 0.587 * self.g as f64 + 0.114 * self.b as f64;
        if lum < 128.0 {
            self.lighten(0.08)
        } else {
            self.darken(0.06)
        }
    }

    fn to_f32_rgba(self) -> (f32, f32, f32, f32) {
        (
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            1.0,
        )
    }

    fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// Parse a `#rrggbb` hex string, panicking on invalid input. All ~670
/// call sites are compile-time-constant literals in this file's preset
/// syntax-colour tables (kept local per #829 — editor-specific presets,
/// not a quadraui concern), so panicking here rather than threading
/// `Option` through every table entry is the same trade-off vimcode's
/// pre-#775 local `Color::from_hex` made. A free function, not a method,
/// because `quadraui::Color::from_hex` already exists under this name
/// with `Option`-returning (non-panicking) semantics.
pub(crate) fn hex(s: &str) -> Color {
    quadraui::Color::from_hex(s).unwrap_or_else(|| panic!("invalid hex color literal: {s:?}"))
}

/// Try to parse a hex colour string. Accepts `#rrggbb`, `#rrggbbaa` (alpha
/// is discarded), and `#rgb` shorthand, with or without the leading `#`.
/// Returns `None` on failure. Kept local rather than delegating to
/// `quadraui::Color::from_hex`: that upstream function requires the
/// leading `#` and doesn't accept 3-digit shorthand, both of which
/// vimcode's settings / custom-theme-file parsing relies on.
///
/// #1494: `s.len()` counts bytes, not chars, and the slices below
/// (`&s[0..2]`, etc.) are byte-index slices — both fine for a genuine hex
/// string (ASCII-only by definition), but a non-ASCII 6-or-8-byte theme
/// colour string (e.g. from a corrupt/hostile theme file) used to match
/// the `6 | 8` arm on byte length alone and then panic slicing mid
/// UTF-8-char-boundary. Guard on `is_ascii()` first so non-ASCII input
/// takes the `_ => None` fallback instead.
pub(crate) fn try_from_hex(s: &str) -> Option<Color> {
    let s = s.trim_start_matches('#');
    if !s.is_ascii() {
        return None;
    }
    let (r, g, b) = match s.len() {
        6 | 8 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            (r, g, b)
        }
        3 => {
            let r = u8::from_str_radix(&s[0..1], 16).ok()?;
            let g = u8::from_str_radix(&s[1..2], 16).ok()?;
            let b = u8::from_str_radix(&s[2..3], 16).ok()?;
            (r * 17, g * 17, b * 17)
        }
        _ => return None,
    };
    Some(Color::rgb(r, g, b))
}

/// Parse `#rrggbbaa` and alpha-blend against `bg`. If no alpha component is
/// present, behaves identically to `try_from_hex`. Kept local alongside
/// `try_from_hex` above rather than using `quadraui::Color::try_from_hex_over`
/// so both share the same `#`-optional / 3-digit-shorthand fallback.
pub(crate) fn try_from_hex_over(s: &str, bg: Color) -> Option<Color> {
    let s = s.trim_start_matches('#');
    if !s.is_ascii() {
        return None;
    }
    match s.len() {
        8 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            let a = u8::from_str_radix(&s[6..8], 16).ok()?;
            // Enforce minimum alpha so diff backgrounds stay visible in terminals.
            let alpha = (a as f64 / 255.0).max(0.25);
            let blend = |fg: u8, bg: u8| -> u8 {
                (fg as f64 * alpha + bg as f64 * (1.0 - alpha)).round() as u8
            };
            Some(Color::rgb(blend(r, bg.r), blend(g, bg.g), blend(b, bg.b)))
        }
        _ => try_from_hex(s),
    }
}

// ─── Style / StyledSpan ──────────────────────────────────────────────────────

/// Text style for a span of characters.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub fg: Color,
    /// Background override; `None` means the window background shows through.
    pub bg: Option<Color>,
    /// Whether the text should be rendered in bold.
    pub bold: bool,
    /// Whether the text should be rendered in italic.
    pub italic: bool,
    /// Font scale factor (1.0 = normal). Used by GTK for markdown headings.
    pub font_scale: f64,
}

/// A styled byte-range within a single line's text.
/// `start_byte` and `end_byte` are offsets into `RenderedLine::raw_text`.
#[derive(Debug, Clone)]
pub struct StyledSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub style: Style,
}

// ─── RenderedLine ─────────────────────────────────────────────────────────────

/// A single visible line ready for rendering.
#[derive(Debug, Clone)]
pub struct RenderedLine {
    /// Raw UTF-8 text (may include a trailing `\n`).
    pub raw_text: String,
    /// Pre-formatted gutter text (e.g. `"  42"` or `"   3"`).
    /// Empty string when line numbers are disabled.
    pub gutter_text: String,
    /// True when this is the line that contains the cursor (for highlighted
    /// gutter colour).
    pub is_current_line: bool,
    /// Syntax-highlight + search-match spans (byte-offset based).
    pub spans: Vec<StyledSpan>,
    /// True when this line is the header of a closed fold.
    pub is_fold_header: bool,
    /// Number of lines hidden in the fold (0 when `is_fold_header` is false).
    pub folded_line_count: usize,
    /// The buffer line index this rendered row corresponds to.
    /// Used by click handlers to map screen row → buffer line.
    pub line_idx: usize,
    /// Git diff status for this line (Added/Modified/None).
    /// `None` when the buffer is not tracked by git or the line is unchanged.
    pub git_diff: Option<GitLineStatus>,
    /// LSP diagnostic marks on this line (may be empty).
    pub diagnostics: Vec<DiagnosticMark>,
    /// Spell-check error marks on this line (may be empty).
    pub spell_errors: Vec<SpellMark>,
    /// Two-way diff status for this line (`None` when diff mode is off).
    pub diff_status: Option<DiffLine>,
    /// True when there is a DAP breakpoint set on this line.
    pub is_breakpoint: bool,
    /// True when the breakpoint on this line has a condition or hit count.
    pub is_conditional_bp: bool,
    /// True when the DAP adapter is currently stopped at this line.
    pub is_dap_current: bool,
    /// True when this is a -wrap continuation row (the 2nd+ visual row of a
    /// long buffer line). When true, `gutter_text` is blank and the line number
    /// belongs to the preceding non-continuation row.
    pub is_wrap_continuation: bool,
    /// Character offset within the buffer line where this visual segment begins.
    /// 0 for non-wrapped lines and the first visual segment of a wrapped line.
    pub segment_col_offset: usize,
    /// Optional inline annotation (virtual text) shown after line content in a
    /// muted colour. Set by Lua plugins via `vimcode.buf.annotate_line()`.
    pub annotation: Option<String>,
    /// AI ghost text shown after the cursor position on this line (Insert mode).
    /// Only set on the cursor line when `ai_completions` is enabled and a
    /// completion is available. Rendered in a muted ghost colour.
    pub ghost_suffix: Option<String>,
    /// True for virtual rows inserted to show AI completion continuation lines.
    /// These rows have empty `raw_text`; the full continuation text is in
    /// `ghost_suffix` and backends draw it at the left edge of the content area.
    pub is_ghost_continuation: bool,
    /// Column positions where indent guide lines should be drawn.
    /// Empty when `indent_guides` setting is off.
    pub indent_guides: Vec<usize>,
    /// Column positions where colorcolumn background should be drawn.
    /// Parsed from `settings.colorcolumn` (e.g. "80,120").
    pub colorcolumns: Vec<usize>,
}

/// A single diagnostic mark on a rendered line (for inline underlines/squiggles).
#[derive(Debug, Clone)]
pub struct DiagnosticMark {
    /// Start column (char index) within the line.
    pub start_col: usize,
    /// End column (char index, exclusive) within the line.
    pub end_col: usize,
    /// Severity level (drives colour).
    pub severity: crate::core::lsp::DiagnosticSeverity,
    /// Short message text (for tooltip/hover).
    pub message: String,
}

/// A misspelled word on a rendered line (for underline/squiggle rendering).
#[derive(Debug, Clone)]
pub struct SpellMark {
    /// Start column (char index) within the line.
    pub start_col: usize,
    /// End column (char index, exclusive) within the line.
    pub end_col: usize,
}

// ─── Cursor ───────────────────────────────────────────────────────────────────

/// The shape of the text cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    /// Filled block (Normal / Visual modes).
    Block,
    /// Thin vertical bar (Insert mode).
    Bar,
    /// Underline (pending replace-char `r` command).
    Underline,
}

/// Cursor position within the visible window area.
#[derive(Debug, Clone, Copy)]
pub struct CursorPos {
    /// Index into `RenderedWindow::lines` (0 = topmost visible line).
    pub view_line: usize,
    /// Column (character index within the line).
    pub col: usize,
}

// ─── Visual selection ─────────────────────────────────────────────────────────

/// Which flavour of visual selection is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    Char,
    Line,
    Block,
}

/// A normalised selection range (start ≤ end) in buffer coordinates.
#[derive(Debug, Clone, Copy)]
pub struct SelectionRange {
    pub kind: SelectionKind,
    /// First selected buffer line.
    pub start_line: usize,
    /// First selected column (Char / Block modes; ignored for Line mode).
    pub start_col: usize,
    /// Last selected buffer line (inclusive).
    pub end_line: usize,
    /// Last selected column (Char / Block modes; ignored for Line mode).
    pub end_col: usize,
}

// ─── TabInfo ──────────────────────────────────────────────────────────────────

/// Display information for a single tab-bar entry.
#[derive(Debug, Clone)]
pub struct TabInfo {
    /// Display label, e.g. `"main.rs "` (#700: no ordinal prefix, matching
    /// VS Code — but see the single trailing space). Checked against the
    /// pinned quadraui rev's `tui::tab_bar::draw_tab_bar` (#700 review
    /// nit): the TUI rasteriser paints the close glyph immediately after
    /// the label's own measured width, with its reserved `TAB_CLOSE_COLS`
    /// separator cell trailing the glyph, not leading it — so any
    /// breathing room before `×` has to come from the label text itself.
    /// Before #700 item 4 that space existed incidentally, baked into the
    /// old `" {i+1}: {name} "` ordinal prefix's own trailing space; the
    /// sole `TabInfo` builder for the editor tab bar
    /// (`build_tab_bar_for_group_by_id`) now adds it back deliberately.
    /// It has to live there rather than downstream at the
    /// `quadraui::TabItem` conversion, because [`tab_hit_width`] measures
    /// `t.name` directly — padding added only at the `TabItem` step would
    /// paint one column wider than the hit-test math expects, the exact
    /// measure/paint mismatch class #654 documents at length.
    pub name: String,
    /// Whether this is the currently active tab.
    pub active: bool,
    /// Whether the buffer has unsaved changes.
    pub dirty: bool,
    /// Whether the buffer is in preview mode.
    pub preview: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Visual selection painting (#1788 review) ──────────────────────────
    //
    // `build_selection`'s `end_col` must agree with what Ctrl+C/Ctrl+X
    // actually copy/delete — otherwise the highlighted span and the
    // clipboard payload disagree (non-blocking note on the #1788 review:
    // quadraui always paints `SelectionKind::Char` as `end_col + 1`, so an
    // *exclusive*-end selection's raw cursor column needs to be backed off
    // by one before being handed to `SelectionRange`).

    /// A VSCode-mode Shift+Right x6 selection (`visual_end_exclusive`,
    /// cursor one past the last selected char) must paint exactly 6 cells,
    /// matching the 6 characters Ctrl+C copies — not 7.
    #[test]
    fn build_selection_backs_off_exclusive_end_by_one_for_char_kind() {
        let mut engine = Engine::new_for_test();
        engine.buffer_mut().insert(0, "hello world\n");
        engine.mode = Mode::Visual;
        engine.visual_anchor = Some(Cursor { line: 0, col: 0 });
        engine.visual_end_exclusive = true;
        engine.view_mut().cursor = Cursor { line: 0, col: 6 };

        let sel = build_selection(&engine, 0, 10).expect("selection must be emitted");
        assert_eq!(sel.start_col, 0);
        assert_eq!(
            sel.end_col, 5,
            "quadraui paints Char selections as end_col + 1, so an \
             exclusive-end selection covering columns 0..6 must report \
             end_col = 5, not the raw cursor column 6"
        );
    }

    /// Plain Vim-style / mouse / Ctrl+D selections are inclusive-end
    /// (`visual_end_exclusive` is `false`) — `end_col` must stay exactly the
    /// cursor's own column, unchanged from before #1788.
    #[test]
    fn build_selection_leaves_inclusive_end_unchanged_for_char_kind() {
        let mut engine = Engine::new_for_test();
        engine.buffer_mut().insert(0, "hello world\n");
        engine.mode = Mode::Visual;
        engine.visual_anchor = Some(Cursor { line: 0, col: 0 });
        engine.visual_end_exclusive = false;
        engine.view_mut().cursor = Cursor { line: 0, col: 4 };

        let sel = build_selection(&engine, 0, 10).expect("selection must be emitted");
        assert_eq!(sel.start_col, 0);
        assert_eq!(sel.end_col, 4);
    }
}
